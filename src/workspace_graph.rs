use crate::postgres::{
    ChunkHit, DocumentHit, DocumentScopedSymbolRecord, DocumentStructureRecord, SymbolHit,
};
use crate::retrieval_science;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FileAggregate {
    node_id: String,
    project_code: String,
    namespace_code: String,
    repo_root: String,
    relative_path: String,
    language: Option<String>,
    source_kind: String,
    git_commit_sha: Option<String>,
    structure_nodes: BTreeMap<String, Value>,
    symbol_nodes: BTreeMap<String, Value>,
    reference_nodes: BTreeMap<String, Value>,
    call_nodes: BTreeMap<String, Value>,
    retrieved_via: BTreeSet<String>,
}

pub fn build_context_pack_workspace_graph(
    context_pack_id: &Uuid,
    query: &str,
    effective_mode: &str,
    scope_signature: &str,
    visible_projects: &Value,
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
    structures: &[DocumentStructureRecord],
    scoped_symbols: &[DocumentScopedSymbolRecord],
) -> anyhow::Result<Value> {
    let catalog = retrieval_science::workspace_graph_catalog_json()?;
    let context_pack_node_id = format!("context_pack:{context_pack_id}");
    let mut nodes = BTreeMap::<String, Value>::new();
    let mut edges = BTreeMap::<(String, String, String), Value>::new();
    let mut files = BTreeMap::<String, FileAggregate>::new();
    let mut chunk_nodes = BTreeMap::<String, Value>::new();

    nodes.insert(
        context_pack_node_id.clone(),
        json!({
            "node_id": context_pack_node_id,
            "node_type": "context_pack",
            "context_pack_id": context_pack_id,
            "query": query,
            "effective_retrieval_mode": effective_mode,
            "scope_signature": scope_signature,
        }),
    );

    for structure in structures {
        let file = ensure_file_aggregate_from_structure(&mut files, structure);
        populate_structure_nodes(file, structure);
        populate_reference_nodes(file, &structure.imports, "import_ref");
        populate_reference_nodes(file, &structure.exports, "export_ref");
        populate_call_nodes(file, &structure.metadata["call_references"]);
    }

    for scoped_symbol in scoped_symbols {
        let file = ensure_file_aggregate_from_scoped_symbol(&mut files, scoped_symbol);
        populate_scoped_symbol_node(file, scoped_symbol);
    }

    for hit in documents {
        let file = ensure_file_aggregate_from_document_hit(&mut files, hit);
        file.retrieved_via.insert("exact_document".to_string());
        edges.insert(
            (
                format!("context_pack:{context_pack_id}"),
                file.node_id.clone(),
                "retrieved_exact_document".to_string(),
            ),
            json!({
                "from_node_id": format!("context_pack:{context_pack_id}"),
                "to_node_id": file.node_id.clone(),
                "relation": "retrieved_exact_document",
            }),
        );
    }

    for hit in symbols {
        let file = ensure_file_aggregate_from_symbol_hit(&mut files, hit);
        file.retrieved_via.insert("symbol_hit".to_string());
        let symbol_node_id = populate_symbol_hit_node(file, hit);
        edges.insert(
            (
                file.node_id.clone(),
                symbol_node_id.clone(),
                "contains_symbol".to_string(),
            ),
            json!({
                "from_node_id": file.node_id.clone(),
                "to_node_id": symbol_node_id,
                "relation": "contains_symbol",
            }),
        );
        edges.insert(
            (
                format!("context_pack:{context_pack_id}"),
                symbol_node_id.clone(),
                "retrieved_symbol".to_string(),
            ),
            json!({
                "from_node_id": format!("context_pack:{context_pack_id}"),
                "to_node_id": symbol_node_id,
                "relation": "retrieved_symbol",
            }),
        );
    }

    for hit in chunks {
        let file = ensure_file_aggregate_from_chunk_hit(&mut files, hit);
        file.retrieved_via.insert("lexical_chunk".to_string());
        let chunk_node_id = chunk_node_id(
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
            &hit.chunk_id.to_string(),
        );
        chunk_nodes.entry(chunk_node_id.clone()).or_insert_with(|| {
            json!({
                "node_id": chunk_node_id,
                "node_type": "chunk",
                "project_code": hit.project_code,
                "namespace_code": hit.namespace_code,
                "relative_path": hit.relative_path,
                "chunk_id": hit.chunk_id,
                "chunk_index": hit.chunk_index,
                "start_line": hit.start_line,
                "end_line": hit.end_line,
                "retrieval_strategy": "lexical_search",
                "symbols_defined": hit.metadata["symbols_defined"].clone(),
            })
        });
        edges.insert(
            (
                file.node_id.clone(),
                chunk_node_id.clone(),
                "contains_chunk".to_string(),
            ),
            json!({
                "from_node_id": file.node_id.clone(),
                "to_node_id": chunk_node_id,
                "relation": "contains_chunk",
            }),
        );
        edges.insert(
            (
                format!("context_pack:{context_pack_id}"),
                chunk_node_id.clone(),
                "retrieved_lexical_chunk".to_string(),
            ),
            json!({
                "from_node_id": format!("context_pack:{context_pack_id}"),
                "to_node_id": chunk_node_id,
                "relation": "retrieved_lexical_chunk",
            }),
        );
    }

    for hit in semantic_chunks {
        let Some(project_code) = hit["project_code"].as_str() else {
            continue;
        };
        let Some(namespace_code) = hit["namespace_code"].as_str() else {
            continue;
        };
        let Some(relative_path) = hit["relative_path"].as_str() else {
            continue;
        };
        let Some(chunk_id) = hit["provenance"]["chunk_id"].as_str() else {
            continue;
        };
        let strategy = hit["retrieval_strategy"]
            .as_str()
            .unwrap_or("vector_search");
        let file = ensure_file_aggregate_from_path(
            &mut files,
            project_code,
            namespace_code,
            hit["provenance"]["repo_root"].as_str().unwrap_or_default(),
            relative_path,
            None,
            "code_chunk",
            None,
        );
        file.retrieved_via.insert(strategy.to_string());
        let chunk_node_id = chunk_node_id(project_code, namespace_code, relative_path, chunk_id);
        chunk_nodes.entry(chunk_node_id.clone()).or_insert_with(|| {
            json!({
                "node_id": chunk_node_id,
                "node_type": "chunk",
                "project_code": project_code,
                "namespace_code": namespace_code,
                "relative_path": relative_path,
                "chunk_id": chunk_id,
                "chunk_index": hit["chunk"]["chunk_index"],
                "start_line": hit["chunk"]["start_line"],
                "end_line": hit["chunk"]["end_line"],
                "retrieval_strategy": strategy,
                "symbols_defined": hit["chunk"]["metadata"]["symbols_defined"].clone(),
            })
        });
        edges.insert(
            (
                file.node_id.clone(),
                chunk_node_id.clone(),
                "contains_chunk".to_string(),
            ),
            json!({
                "from_node_id": file.node_id.clone(),
                "to_node_id": chunk_node_id,
                "relation": "contains_chunk",
            }),
        );
        edges.insert(
            (
                format!("context_pack:{context_pack_id}"),
                chunk_node_id.clone(),
                if strategy == "lexical_fallback" {
                    "retrieved_lexical_fallback_chunk".to_string()
                } else {
                    "retrieved_semantic_chunk".to_string()
                },
            ),
            json!({
                "from_node_id": format!("context_pack:{context_pack_id}"),
                "to_node_id": chunk_node_id,
                "relation": if strategy == "lexical_fallback" {
                    "retrieved_lexical_fallback_chunk"
                } else {
                    "retrieved_semantic_chunk"
                },
            }),
        );
    }

    append_resolved_reference_edges(&files, &mut edges);

    for file in files.into_values() {
        let file_node_id = file.node_id.clone();
        let structure_index = structure_match_index(&file.structure_nodes);
        nodes.insert(
            file.node_id.clone(),
            json!({
                "node_id": file.node_id,
                "node_type": "file",
                "project_code": file.project_code,
                "namespace_code": file.namespace_code,
                "repo_root": file.repo_root,
                "relative_path": file.relative_path,
                "language": file.language,
                "source_kind": file.source_kind,
                "git_commit_sha": file.git_commit_sha,
                "retrieved_via": file.retrieved_via.into_iter().collect::<Vec<_>>(),
                "structure_count": file.structure_nodes.len(),
                "symbol_count": file.symbol_nodes.len(),
                "reference_count": file.reference_nodes.len(),
                "call_count": file.call_nodes.len(),
            }),
        );
        for structure in file.structure_nodes.into_values() {
            let structure_node_id = structure["node_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            nodes.insert(structure_node_id.clone(), structure);
            edges.insert(
                (
                    file_node_id.clone(),
                    structure_node_id.clone(),
                    "contains_structure".to_string(),
                ),
                json!({
                    "from_node_id": file_node_id,
                    "to_node_id": structure_node_id,
                    "relation": "contains_structure",
                }),
            );
        }
        for symbol in file.symbol_nodes.into_values() {
            let symbol_node_id = symbol["node_id"].as_str().unwrap_or_default().to_string();
            nodes.insert(symbol_node_id.clone(), symbol.clone());
            edges.insert(
                (
                    file_node_id.clone(),
                    symbol_node_id.clone(),
                    "contains_symbol".to_string(),
                ),
                json!({
                    "from_node_id": file_node_id,
                    "to_node_id": symbol_node_id,
                    "relation": "contains_symbol",
                }),
            );
            if let Some(structure_node_id) = match_structure_for_symbol(&structure_index, &symbol) {
                edges.insert(
                    (
                        structure_node_id.clone(),
                        symbol_node_id.clone(),
                        "defines_symbol".to_string(),
                    ),
                    json!({
                        "from_node_id": structure_node_id,
                        "to_node_id": symbol_node_id,
                        "relation": "defines_symbol",
                    }),
                );
            }
        }
        for call in file.call_nodes.into_values() {
            let call_node_id = call["node_id"].as_str().unwrap_or_default().to_string();
            nodes.insert(call_node_id.clone(), call);
            edges.insert(
                (
                    file_node_id.clone(),
                    call_node_id.clone(),
                    "calls_reference".to_string(),
                ),
                json!({
                    "from_node_id": file_node_id,
                    "to_node_id": call_node_id,
                    "relation": "calls_reference",
                }),
            );
        }
        for reference in file.reference_nodes.into_values() {
            let reference_node_id = reference["node_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let relation = if reference["node_type"].as_str() == Some("import_ref") {
                "imports_reference"
            } else {
                "exports_reference"
            };
            nodes.insert(reference_node_id.clone(), reference);
            edges.insert(
                (
                    file_node_id.clone(),
                    reference_node_id.clone(),
                    relation.to_string(),
                ),
                json!({
                    "from_node_id": file_node_id,
                    "to_node_id": reference_node_id,
                    "relation": relation,
                }),
            );
        }
    }

    for chunk in chunk_nodes.into_values() {
        let chunk_node_id = chunk["node_id"].as_str().unwrap_or_default().to_string();
        nodes.insert(chunk_node_id, chunk);
    }

    let node_values = nodes.into_values().collect::<Vec<_>>();
    let edge_values = edges.into_values().collect::<Vec<_>>();
    Ok(json!({
        "workspace_graph_model_version": catalog["workspace_graph_model_version"].clone(),
        "artifact_lineage_model_version": catalog["artifact_lineage_model_version"].clone(),
        "lineage_model_version": catalog["lineage_model_version"].clone(),
        "truth_ranking": catalog["truth_ranking"].clone(),
        "scope_signature": scope_signature,
        "visible_projects": visible_projects.clone(),
        "source_context_pack_ids": [context_pack_id.to_string()],
        "nodes": node_values.clone(),
        "edges": edge_values.clone(),
        "summary": graph_summary(&node_values, &edge_values),
    }))
}

pub fn merge_workspace_graphs(graphs: &[Value]) -> Value {
    let mut nodes = BTreeMap::<String, Value>::new();
    let mut edges = BTreeMap::<(String, String, String), Value>::new();
    let mut scope_signatures = BTreeSet::<String>::new();
    let mut source_context_pack_ids = BTreeSet::<String>::new();
    let mut visible_projects = BTreeMap::<String, Value>::new();
    let mut workspace_graph_model_version = None::<Value>;
    let mut artifact_lineage_model_version = None::<Value>;
    let mut lineage_model_version = None::<Value>;
    let mut truth_ranking = None::<Value>;

    for graph in graphs {
        if graph.is_null() {
            continue;
        }
        workspace_graph_model_version
            .get_or_insert_with(|| graph["workspace_graph_model_version"].clone());
        artifact_lineage_model_version
            .get_or_insert_with(|| graph["artifact_lineage_model_version"].clone());
        lineage_model_version.get_or_insert_with(|| graph["lineage_model_version"].clone());
        truth_ranking.get_or_insert_with(|| graph["truth_ranking"].clone());
        if let Some(scope_signature) = graph["scope_signature"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            scope_signatures.insert(scope_signature.to_string());
        }
        if let Some(items) = graph["source_context_pack_ids"].as_array() {
            for item in items {
                if let Some(value) = item.as_str().filter(|value| !value.is_empty()) {
                    source_context_pack_ids.insert(value.to_string());
                }
            }
        }
        if let Some(items) = graph["visible_projects"].as_array() {
            for item in items {
                let key = format!(
                    "{}::{}",
                    item["project_code"].as_str().unwrap_or(""),
                    item["namespace_code"].as_str().unwrap_or("")
                );
                visible_projects.entry(key).or_insert_with(|| item.clone());
            }
        }
        if let Some(items) = graph["nodes"].as_array() {
            for item in items {
                let Some(node_id) = item["node_id"].as_str() else {
                    continue;
                };
                nodes
                    .entry(node_id.to_string())
                    .or_insert_with(|| item.clone());
            }
        }
        if let Some(items) = graph["edges"].as_array() {
            for item in items {
                let Some(from_node_id) = item["from_node_id"].as_str() else {
                    continue;
                };
                let Some(to_node_id) = item["to_node_id"].as_str() else {
                    continue;
                };
                let Some(relation) = item["relation"].as_str() else {
                    continue;
                };
                edges
                    .entry((
                        from_node_id.to_string(),
                        to_node_id.to_string(),
                        relation.to_string(),
                    ))
                    .or_insert_with(|| item.clone());
            }
        }
    }

    if nodes.is_empty() && edges.is_empty() {
        return Value::Null;
    }

    let node_values = nodes.into_values().collect::<Vec<_>>();
    let edge_values = edges.into_values().collect::<Vec<_>>();
    json!({
        "workspace_graph_model_version": workspace_graph_model_version.unwrap_or(Value::Null),
        "artifact_lineage_model_version": artifact_lineage_model_version.unwrap_or(Value::Null),
        "lineage_model_version": lineage_model_version.unwrap_or(Value::Null),
        "truth_ranking": truth_ranking.unwrap_or_else(|| json!([])),
        "scope_signatures": scope_signatures.into_iter().collect::<Vec<_>>(),
        "visible_projects": visible_projects.into_values().collect::<Vec<_>>(),
        "source_context_pack_ids": source_context_pack_ids.into_iter().collect::<Vec<_>>(),
        "nodes": node_values.clone(),
        "edges": edge_values.clone(),
        "summary": graph_summary(&node_values, &edge_values),
    })
}

pub fn human_summary(graph: &Value) -> Option<String> {
    if graph.is_null() {
        return None;
    }
    let counts = graph["summary"]["node_counts"].as_object()?;
    let files = counts.get("file").and_then(Value::as_u64).unwrap_or(0);
    let structures = counts
        .get("structure_item")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let symbols = counts.get("symbol").and_then(Value::as_u64).unwrap_or(0);
    let chunks = counts.get("chunk").and_then(Value::as_u64).unwrap_or(0);
    let imports = counts
        .get("import_ref")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let exports = counts
        .get("export_ref")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let calls = counts.get("call_ref").and_then(Value::as_u64).unwrap_or(0);
    let context_packs = graph["source_context_pack_ids"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    if files == 0 && symbols == 0 && chunks == 0 && imports == 0 && exports == 0 && calls == 0 {
        return None;
    }
    Some(format!(
        "структурный граф: контекстных снимков {context_packs}, файлов {files}, структурных узлов {structures}, символов {symbols}, фрагментов {chunks}, импортов {imports}, экспортов {exports}, вызовов {calls}"
    ))
}

fn ensure_file_aggregate_from_structure<'a>(
    files: &'a mut BTreeMap<String, FileAggregate>,
    structure: &DocumentStructureRecord,
) -> &'a mut FileAggregate {
    let node_id = file_node_id(
        &structure.project_code,
        &structure.namespace_code,
        &structure.relative_path,
    );
    files
        .entry(node_id.clone())
        .or_insert_with(|| FileAggregate {
            node_id,
            project_code: structure.project_code.clone(),
            namespace_code: structure.namespace_code.clone(),
            repo_root: structure.repo_root.clone(),
            relative_path: structure.relative_path.clone(),
            language: structure.language.clone(),
            source_kind: structure.source_kind.clone(),
            git_commit_sha: structure.git_commit_sha.clone(),
            structure_nodes: BTreeMap::new(),
            symbol_nodes: BTreeMap::new(),
            reference_nodes: BTreeMap::new(),
            call_nodes: BTreeMap::new(),
            retrieved_via: BTreeSet::new(),
        })
}

fn ensure_file_aggregate_from_scoped_symbol<'a>(
    files: &'a mut BTreeMap<String, FileAggregate>,
    symbol: &DocumentScopedSymbolRecord,
) -> &'a mut FileAggregate {
    ensure_file_aggregate_from_path(
        files,
        &symbol.project_code,
        &symbol.namespace_code,
        &symbol.repo_root,
        &symbol.relative_path,
        symbol.language.as_deref(),
        &symbol.source_kind,
        symbol.git_commit_sha.as_deref(),
    )
}

fn ensure_file_aggregate_from_document_hit<'a>(
    files: &'a mut BTreeMap<String, FileAggregate>,
    hit: &DocumentHit,
) -> &'a mut FileAggregate {
    ensure_file_aggregate_from_path(
        files,
        &hit.project_code,
        &hit.namespace_code,
        &hit.repo_root,
        &hit.relative_path,
        hit.language.as_deref(),
        &hit.source_kind,
        hit.git_commit_sha.as_deref(),
    )
}

fn ensure_file_aggregate_from_symbol_hit<'a>(
    files: &'a mut BTreeMap<String, FileAggregate>,
    hit: &SymbolHit,
) -> &'a mut FileAggregate {
    ensure_file_aggregate_from_path(
        files,
        &hit.project_code,
        &hit.namespace_code,
        &hit.repo_root,
        &hit.relative_path,
        hit.metadata["language"].as_str(),
        "symbol",
        None,
    )
}

fn ensure_file_aggregate_from_chunk_hit<'a>(
    files: &'a mut BTreeMap<String, FileAggregate>,
    hit: &ChunkHit,
) -> &'a mut FileAggregate {
    ensure_file_aggregate_from_path(
        files,
        &hit.project_code,
        &hit.namespace_code,
        &hit.repo_root,
        &hit.relative_path,
        hit.metadata["language"].as_str(),
        "code_chunk",
        None,
    )
}

fn ensure_file_aggregate_from_path<'a>(
    files: &'a mut BTreeMap<String, FileAggregate>,
    project_code: &str,
    namespace_code: &str,
    repo_root: &str,
    relative_path: &str,
    language: Option<&str>,
    source_kind: &str,
    git_commit_sha: Option<&str>,
) -> &'a mut FileAggregate {
    let node_id = file_node_id(project_code, namespace_code, relative_path);
    files
        .entry(node_id.clone())
        .or_insert_with(|| FileAggregate {
            node_id,
            project_code: project_code.to_string(),
            namespace_code: namespace_code.to_string(),
            repo_root: repo_root.to_string(),
            relative_path: relative_path.to_string(),
            language: language.map(ToOwned::to_owned),
            source_kind: source_kind.to_string(),
            git_commit_sha: git_commit_sha.map(ToOwned::to_owned),
            structure_nodes: BTreeMap::new(),
            symbol_nodes: BTreeMap::new(),
            reference_nodes: BTreeMap::new(),
            call_nodes: BTreeMap::new(),
            retrieved_via: BTreeSet::new(),
        })
}

fn populate_structure_nodes(file: &mut FileAggregate, structure: &DocumentStructureRecord) {
    let Some(items) = structure.structure.as_array() else {
        return;
    };
    for item in items {
        let structure_node_id = structure_node_id(
            &file.project_code,
            &file.namespace_code,
            &file.relative_path,
            item["kind"].as_str().unwrap_or("structure"),
            item["name"].as_str(),
            item["start_line"].as_i64().unwrap_or_default(),
        );
        file.structure_nodes
            .entry(structure_node_id.clone())
            .or_insert_with(|| {
                json!({
                    "node_id": structure_node_id,
                    "node_type": "structure_item",
                    "project_code": file.project_code,
                    "namespace_code": file.namespace_code,
                    "relative_path": file.relative_path,
                    "kind": item["kind"].clone(),
                    "name": item["name"].clone(),
                    "start_line": item["start_line"].clone(),
                    "end_line": item["end_line"].clone(),
                })
            });
    }
}

fn populate_reference_nodes(file: &mut FileAggregate, nodes: &Value, node_type: &str) {
    let Some(items) = nodes.as_array() else {
        return;
    };
    for item in items {
        let label = item["name"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| item["text"].as_str().unwrap_or("reference"));
        let reference_node_id = reference_node_id(
            node_type,
            &file.project_code,
            &file.namespace_code,
            &file.relative_path,
            label,
            item["start_line"].as_i64().unwrap_or_default(),
        );
        file.reference_nodes
            .entry(reference_node_id.clone())
            .or_insert_with(|| {
                json!({
                    "node_id": reference_node_id,
                    "node_type": node_type,
                    "project_code": file.project_code,
                    "namespace_code": file.namespace_code,
                    "relative_path": file.relative_path,
                    "label": label,
                    "name": item["name"].clone(),
                    "text": item["text"].clone(),
                    "kind": item["kind"].clone(),
                    "start_line": item["start_line"].clone(),
                    "end_line": item["end_line"].clone(),
                })
            });
    }
}

fn populate_call_nodes(file: &mut FileAggregate, nodes: &Value) {
    let Some(items) = nodes.as_array() else {
        return;
    };
    for item in items {
        let label = item["callee_path"]
            .as_str()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                item["callee_name"]
                    .as_str()
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_else(|| item["text"].as_str().unwrap_or("call"));
        let call_node_id = call_node_id(
            &file.project_code,
            &file.namespace_code,
            &file.relative_path,
            label,
            item["start_line"].as_i64().unwrap_or_default(),
        );
        file.call_nodes
            .entry(call_node_id.clone())
            .or_insert_with(|| {
                json!({
                    "node_id": call_node_id,
                    "node_type": "call_ref",
                    "project_code": file.project_code,
                    "namespace_code": file.namespace_code,
                    "relative_path": file.relative_path,
                    "kind": item["kind"].clone(),
                    "call_style": item["call_style"].clone(),
                    "callee_name": item["callee_name"].clone(),
                    "callee_path": item["callee_path"].clone(),
                    "receiver_text": item["receiver_text"].clone(),
                    "enclosing_owner_kind": item["enclosing_owner_kind"].clone(),
                    "enclosing_owner_name": item["enclosing_owner_name"].clone(),
                    "enclosing_owner_path": item["enclosing_owner_path"].clone(),
                    "enclosing_trait_name": item["enclosing_trait_name"].clone(),
                    "generic": item["generic"].clone(),
                    "label": label,
                    "text": item["text"].clone(),
                    "start_line": item["start_line"].clone(),
                    "end_line": item["end_line"].clone(),
                })
            });
    }
}

fn populate_scoped_symbol_node(file: &mut FileAggregate, symbol: &DocumentScopedSymbolRecord) {
    let symbol_node_id = symbol_node_id(
        &file.project_code,
        &file.namespace_code,
        &file.relative_path,
        &symbol.name,
        symbol.start_line,
    );
    file.symbol_nodes
        .entry(symbol_node_id.clone())
        .or_insert_with(|| {
            json!({
                "node_id": symbol_node_id,
                "node_type": "symbol",
                "project_code": file.project_code,
                "namespace_code": file.namespace_code,
                "relative_path": file.relative_path,
                "name": symbol.name,
                "kind": symbol.kind,
                "start_line": symbol.start_line,
                "end_line": symbol.end_line,
                "start_byte": symbol.start_byte,
                "end_byte": symbol.end_byte,
                "metadata": compact_symbol_metadata(&symbol.metadata),
            })
        });
}

fn populate_symbol_hit_node(file: &mut FileAggregate, hit: &SymbolHit) -> String {
    let symbol_node_id = symbol_node_id(
        &hit.project_code,
        &hit.namespace_code,
        &hit.relative_path,
        &hit.name,
        hit.start_line,
    );
    file.symbol_nodes
        .entry(symbol_node_id.clone())
        .or_insert_with(|| {
            json!({
                "node_id": symbol_node_id,
                "node_type": "symbol",
                "project_code": hit.project_code,
                "namespace_code": hit.namespace_code,
                "relative_path": hit.relative_path,
                "name": hit.name,
                "kind": hit.kind,
                "start_line": hit.start_line,
                "end_line": hit.end_line,
                "start_byte": hit.start_byte,
                "end_byte": hit.end_byte,
                "metadata": compact_symbol_metadata(&hit.metadata),
            })
        });
    symbol_node_id
}

fn compact_symbol_metadata(metadata: &Value) -> Value {
    json!({
        "language": metadata["language"].clone(),
        "node_kind": metadata["node_kind"].clone(),
        "owner_kind": metadata["owner_kind"].clone(),
        "owner_name": metadata["owner_name"].clone(),
        "owner_path": metadata["owner_path"].clone(),
        "trait_name": metadata["trait_name"].clone(),
    })
}

#[derive(Debug, Clone)]
struct ReferenceResolution {
    target_file_key: ScopedPathKey,
    target_file_node_id: String,
    target_symbol_node_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScopedPathKey {
    project_code: String,
    namespace_code: String,
    relative_path: String,
}

#[derive(Debug, Default)]
struct GraphLookup {
    file_node_ids: BTreeMap<ScopedPathKey, String>,
    symbol_node_ids: BTreeMap<(ScopedPathKey, String), Vec<String>>,
    owner_symbol_node_ids: BTreeMap<(ScopedPathKey, String, String), Vec<String>>,
}

fn append_resolved_reference_edges(
    files: &BTreeMap<String, FileAggregate>,
    edges: &mut BTreeMap<(String, String, String), Value>,
) {
    let lookup = build_graph_lookup(files);
    let mut imported_files_by_source = BTreeMap::<ScopedPathKey, BTreeSet<ScopedPathKey>>::new();
    for file in files.values() {
        let source_key = scoped_path_key(
            &file.project_code,
            &file.namespace_code,
            &file.relative_path,
        );
        for reference in file.reference_nodes.values() {
            let Some(reference_node_id) = reference["node_id"].as_str() else {
                continue;
            };
            let Some(reference_kind) = reference["node_type"].as_str() else {
                continue;
            };
            let Some(resolution) = resolve_reference(file, reference, &lookup) else {
                continue;
            };
            let file_relation = if reference_kind == "import_ref" {
                "imports_file"
            } else {
                "re_exports_file"
            };
            edges.insert(
                (
                    file.node_id.clone(),
                    resolution.target_file_node_id.clone(),
                    file_relation.to_string(),
                ),
                json!({
                    "from_node_id": file.node_id,
                    "to_node_id": resolution.target_file_node_id,
                    "relation": file_relation,
                }),
            );
            edges.insert(
                (
                    reference_node_id.to_string(),
                    resolution.target_file_node_id.clone(),
                    "resolves_file".to_string(),
                ),
                json!({
                    "from_node_id": reference_node_id,
                    "to_node_id": resolution.target_file_node_id,
                    "relation": "resolves_file",
                }),
            );
            imported_files_by_source
                .entry(source_key.clone())
                .or_default()
                .insert(resolution.target_file_key.clone());
            if let Some(symbol_node_id) = resolution.target_symbol_node_id {
                let symbol_relation = if reference_kind == "import_ref" {
                    "imports_symbol"
                } else {
                    "re_exports_symbol"
                };
                edges.insert(
                    (
                        file.node_id.clone(),
                        symbol_node_id.clone(),
                        symbol_relation.to_string(),
                    ),
                    json!({
                        "from_node_id": file.node_id,
                        "to_node_id": symbol_node_id,
                        "relation": symbol_relation,
                    }),
                );
                edges.insert(
                    (
                        reference_node_id.to_string(),
                        symbol_node_id.clone(),
                        "resolves_symbol".to_string(),
                    ),
                    json!({
                        "from_node_id": reference_node_id,
                        "to_node_id": symbol_node_id,
                        "relation": "resolves_symbol",
                    }),
                );
            }
        }
        for call in file.call_nodes.values() {
            let Some(call_node_id) = call["node_id"].as_str() else {
                continue;
            };
            let Some(resolution) = resolve_call(
                file,
                call,
                &lookup,
                imported_files_by_source.get(&source_key),
            ) else {
                continue;
            };
            if resolution.target_file_key != source_key {
                edges.insert(
                    (
                        file.node_id.clone(),
                        resolution.target_file_node_id.clone(),
                        "calls_file".to_string(),
                    ),
                    json!({
                        "from_node_id": file.node_id,
                        "to_node_id": resolution.target_file_node_id,
                        "relation": "calls_file",
                    }),
                );
            }
            edges.insert(
                (
                    call_node_id.to_string(),
                    resolution.target_file_node_id.clone(),
                    "resolves_call_file".to_string(),
                ),
                json!({
                    "from_node_id": call_node_id,
                    "to_node_id": resolution.target_file_node_id,
                    "relation": "resolves_call_file",
                }),
            );
            if let Some(symbol_node_id) = resolution.target_symbol_node_id {
                edges.insert(
                    (
                        file.node_id.clone(),
                        symbol_node_id.clone(),
                        "calls_symbol".to_string(),
                    ),
                    json!({
                        "from_node_id": file.node_id,
                        "to_node_id": symbol_node_id,
                        "relation": "calls_symbol",
                    }),
                );
                edges.insert(
                    (
                        call_node_id.to_string(),
                        symbol_node_id.clone(),
                        "resolves_call_symbol".to_string(),
                    ),
                    json!({
                        "from_node_id": call_node_id,
                        "to_node_id": symbol_node_id,
                        "relation": "resolves_call_symbol",
                    }),
                );
            }
        }
    }
}

fn build_graph_lookup(files: &BTreeMap<String, FileAggregate>) -> GraphLookup {
    let mut lookup = GraphLookup::default();
    for file in files.values() {
        let file_key = scoped_path_key(
            &file.project_code,
            &file.namespace_code,
            &file.relative_path,
        );
        lookup
            .file_node_ids
            .entry(file_key.clone())
            .or_insert_with(|| file.node_id.clone());
        for symbol in file.symbol_nodes.values() {
            let Some(name) = symbol["name"].as_str() else {
                continue;
            };
            let Some(symbol_node_id) = symbol["node_id"].as_str() else {
                continue;
            };
            lookup
                .symbol_node_ids
                .entry((file_key.clone(), name.to_string()))
                .or_default()
                .push(symbol_node_id.to_string());
            if let Some(owner_name) = symbol["metadata"]["owner_name"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                lookup
                    .owner_symbol_node_ids
                    .entry((file_key.clone(), owner_name.to_string(), name.to_string()))
                    .or_default()
                    .push(symbol_node_id.to_string());
            }
        }
    }
    lookup
}

fn resolve_reference(
    source_file: &FileAggregate,
    reference: &Value,
    lookup: &GraphLookup,
) -> Option<ReferenceResolution> {
    let language = source_file.language.as_deref()?;
    let target = match language {
        "rust" => resolve_rust_path_target(
            source_file,
            &extract_rust_reference_path(reference)?,
            lookup,
        )?,
        "javascript" | "typescript" | "tsx" => {
            let target_relative_path =
                resolve_ecmascript_reference(source_file, reference, lookup)?;
            let target_file_key = scoped_path_key(
                &source_file.project_code,
                &source_file.namespace_code,
                &target_relative_path,
            );
            let target_file_node_id = lookup.file_node_ids.get(&target_file_key)?;
            ReferenceResolution {
                target_file_key,
                target_file_node_id: target_file_node_id.clone(),
                target_symbol_node_id: None,
            }
        }
        _ => return None,
    };
    Some(target)
}

fn resolve_call(
    source_file: &FileAggregate,
    call: &Value,
    lookup: &GraphLookup,
    imported_files: Option<&BTreeSet<ScopedPathKey>>,
) -> Option<ReferenceResolution> {
    let call_style = call["call_style"].as_str()?;
    let local_owner_name = call["enclosing_owner_name"].as_str();
    match call_style {
        "scoped_identifier" | "macro_scoped_identifier" => {
            resolve_rust_path_target(source_file, call["callee_path"].as_str()?, lookup).or_else(
                || {
                    resolve_rust_owner_symbol_path_target(
                        source_file,
                        call["callee_path"].as_str()?,
                        lookup,
                        imported_files,
                        local_owner_name,
                    )
                },
            )
        }
        "identifier" | "macro_identifier" => resolve_rust_symbol_name_target(
            source_file,
            call["callee_name"].as_str()?,
            lookup,
            imported_files,
        ),
        "field_expression"
            if call["receiver_text"].as_str() == Some("self") && local_owner_name.is_some() =>
        {
            resolve_rust_owned_symbol_name_target(
                source_file,
                local_owner_name?,
                call["callee_name"].as_str()?,
                lookup,
                None,
            )
        }
        _ => None,
    }
}

fn resolve_rust_path_target(
    source_file: &FileAggregate,
    path: &str,
    lookup: &GraphLookup,
) -> Option<ReferenceResolution> {
    let (base_dir, segments) = rust_reference_base_and_segments(&source_file.relative_path, &path)?;
    let path_target = select_rust_target_path(
        &source_file.project_code,
        &source_file.namespace_code,
        &base_dir,
        &segments,
        lookup,
    )?;
    let file_key = scoped_path_key(
        &source_file.project_code,
        &source_file.namespace_code,
        &path_target.target_relative_path,
    );
    let target_file_node_id = lookup.file_node_ids.get(&file_key)?.clone();
    let target_symbol_node_id = match path_target.trailing_segments.as_slice() {
        [] => None,
        [symbol_name] => resolve_unique_symbol_node_id(lookup, &file_key, symbol_name),
        [owner_name, symbol_name] => {
            resolve_unique_owned_symbol_node_id(lookup, &file_key, owner_name, symbol_name)
        }
        _ => None,
    };
    Some(ReferenceResolution {
        target_file_key: file_key,
        target_file_node_id,
        target_symbol_node_id,
    })
}

fn resolve_rust_owner_symbol_path_target(
    source_file: &FileAggregate,
    path: &str,
    lookup: &GraphLookup,
    imported_files: Option<&BTreeSet<ScopedPathKey>>,
    local_owner_name: Option<&str>,
) -> Option<ReferenceResolution> {
    let (_, segments) = rust_reference_base_and_segments(&source_file.relative_path, path)?;
    let [owner_name, symbol_name] = segments.as_slice() else {
        return None;
    };
    let effective_owner_name = if owner_name == "Self" {
        local_owner_name?
    } else {
        owner_name.as_str()
    };
    resolve_rust_owned_symbol_name_target(
        source_file,
        effective_owner_name,
        symbol_name,
        lookup,
        imported_files,
    )
}

fn resolve_rust_symbol_name_target(
    source_file: &FileAggregate,
    symbol_name: &str,
    lookup: &GraphLookup,
    imported_files: Option<&BTreeSet<ScopedPathKey>>,
) -> Option<ReferenceResolution> {
    let source_key = scoped_path_key(
        &source_file.project_code,
        &source_file.namespace_code,
        &source_file.relative_path,
    );
    let mut candidate_files = BTreeSet::new();
    candidate_files.insert(source_key.clone());
    if let Some(imported_files) = imported_files {
        candidate_files.extend(imported_files.iter().cloned());
    }
    let mut matches = Vec::<(ScopedPathKey, String)>::new();
    for candidate_file in candidate_files {
        let symbol_key = (candidate_file.clone(), symbol_name.to_string());
        match lookup.symbol_node_ids.get(&symbol_key) {
            Some(symbol_node_ids) if symbol_node_ids.len() == 1 => {
                matches.push((candidate_file, symbol_node_ids[0].clone()));
            }
            Some(_) => return None,
            None => {}
        }
    }
    if matches.len() != 1 {
        return None;
    }
    let (target_file_key, target_symbol_node_id) = matches.into_iter().next()?;
    let target_file_node_id = lookup.file_node_ids.get(&target_file_key)?.clone();
    Some(ReferenceResolution {
        target_file_key,
        target_file_node_id,
        target_symbol_node_id: Some(target_symbol_node_id),
    })
}

fn resolve_rust_owned_symbol_name_target(
    source_file: &FileAggregate,
    owner_name: &str,
    symbol_name: &str,
    lookup: &GraphLookup,
    imported_files: Option<&BTreeSet<ScopedPathKey>>,
) -> Option<ReferenceResolution> {
    let source_key = scoped_path_key(
        &source_file.project_code,
        &source_file.namespace_code,
        &source_file.relative_path,
    );
    let mut candidate_files = BTreeSet::new();
    candidate_files.insert(source_key.clone());
    if let Some(imported_files) = imported_files {
        candidate_files.extend(imported_files.iter().cloned());
    }
    let mut matches = Vec::<(ScopedPathKey, String)>::new();
    for candidate_file in candidate_files {
        let symbol_key = (
            candidate_file.clone(),
            owner_name.to_string(),
            symbol_name.to_string(),
        );
        match lookup.owner_symbol_node_ids.get(&symbol_key) {
            Some(symbol_node_ids) if symbol_node_ids.len() == 1 => {
                matches.push((candidate_file, symbol_node_ids[0].clone()));
            }
            Some(_) => return None,
            None => {}
        }
    }
    if matches.len() != 1 {
        return None;
    }
    let (target_file_key, target_symbol_node_id) = matches.into_iter().next()?;
    let target_file_node_id = lookup.file_node_ids.get(&target_file_key)?.clone();
    Some(ReferenceResolution {
        target_file_key,
        target_file_node_id,
        target_symbol_node_id: Some(target_symbol_node_id),
    })
}

fn resolve_unique_symbol_node_id(
    lookup: &GraphLookup,
    file_key: &ScopedPathKey,
    symbol_name: &str,
) -> Option<String> {
    let symbol_key = (file_key.clone(), symbol_name.to_string());
    let symbol_node_ids = lookup.symbol_node_ids.get(&symbol_key)?;
    if symbol_node_ids.len() == 1 {
        Some(symbol_node_ids[0].clone())
    } else {
        None
    }
}

fn resolve_unique_owned_symbol_node_id(
    lookup: &GraphLookup,
    file_key: &ScopedPathKey,
    owner_name: &str,
    symbol_name: &str,
) -> Option<String> {
    let symbol_key = (
        file_key.clone(),
        owner_name.to_string(),
        symbol_name.to_string(),
    );
    let symbol_node_ids = lookup.owner_symbol_node_ids.get(&symbol_key)?;
    if symbol_node_ids.len() == 1 {
        Some(symbol_node_ids[0].clone())
    } else {
        None
    }
}

fn resolve_ecmascript_reference(
    source_file: &FileAggregate,
    reference: &Value,
    lookup: &GraphLookup,
) -> Option<String> {
    let specifier = extract_ecmascript_module_specifier(reference)?;
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None;
    }
    let base_dir = Path::new(&source_file.relative_path)
        .parent()
        .unwrap_or(Path::new(""));
    let joined = normalize_relative_path(base_dir.join(specifier))?;
    let mut candidates = BTreeSet::new();
    if Path::new(&joined).extension().is_some() {
        if scoped_file_exists(
            lookup,
            &source_file.project_code,
            &source_file.namespace_code,
            &joined,
        ) {
            candidates.insert(joined.clone());
        }
    } else {
        for suffix in [
            ".ts",
            ".tsx",
            ".js",
            ".jsx",
            ".json",
            "/index.ts",
            "/index.tsx",
            "/index.js",
            "/index.jsx",
            "/index.json",
        ] {
            let candidate = format!("{joined}{suffix}");
            if scoped_file_exists(
                lookup,
                &source_file.project_code,
                &source_file.namespace_code,
                &candidate,
            ) {
                candidates.insert(candidate);
            }
        }
    }
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

fn extract_rust_reference_path(reference: &Value) -> Option<String> {
    let raw = reference["text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| reference["name"].as_str().filter(|value| !value.is_empty()))
        .or_else(|| {
            reference["label"]
                .as_str()
                .filter(|value| !value.is_empty())
        })?
        .trim();
    let use_index = raw.find("use ").map(|index| index + 4).unwrap_or(0);
    let mut path = raw[use_index..].trim().trim_end_matches(';').trim();
    if let Some((head, _)) = path.split_once(" as ") {
        path = head.trim();
    }
    if path.contains('{')
        || path.contains('}')
        || path.contains(',')
        || path.contains('*')
        || path.is_empty()
    {
        return None;
    }
    Some(path.to_string())
}

fn rust_reference_base_and_segments(
    source_relative_path: &str,
    path: &str,
) -> Option<(PathBuf, Vec<String>)> {
    let raw_segments = path
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if raw_segments.is_empty() {
        return None;
    }

    let mut base_dir = rust_module_dir(source_relative_path);
    let mut index = 0usize;
    if raw_segments.first() == Some(&"crate") {
        base_dir = rust_crate_root_dir(source_relative_path);
        index = 1;
    } else {
        while raw_segments.get(index) == Some(&"super") {
            base_dir = base_dir.parent()?.to_path_buf();
            index += 1;
        }
        if raw_segments.get(index) == Some(&"self") {
            index += 1;
        }
    }
    let segments = raw_segments[index..]
        .iter()
        .map(|segment| segment.to_string())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some((base_dir, segments))
}

#[derive(Debug, Clone)]
struct RustPathTarget {
    target_relative_path: String,
    trailing_segments: Vec<String>,
}

fn select_rust_target_path(
    project_code: &str,
    namespace_code: &str,
    base_dir: &Path,
    segments: &[String],
    lookup: &GraphLookup,
) -> Option<RustPathTarget> {
    let mut matches = BTreeMap::<usize, BTreeSet<String>>::new();
    for consumed in 1..=segments.len() {
        let relative_path = normalize_relative_path(
            segments[..consumed]
                .iter()
                .fold(base_dir.to_path_buf(), |path, segment| path.join(segment)),
        )?;
        for candidate in rust_file_candidates(&relative_path) {
            if scoped_file_exists(lookup, project_code, namespace_code, &candidate) {
                matches.entry(consumed).or_default().insert(candidate);
            }
        }
    }
    if matches.is_empty() {
        return None;
    }
    if let Some(full_matches) = matches.get(&segments.len()) {
        if full_matches.len() == 1 {
            return Some(RustPathTarget {
                target_relative_path: full_matches.iter().next()?.clone(),
                trailing_segments: Vec::new(),
            });
        }
        return None;
    }
    let best_depth = *matches.keys().max()?;
    let best_matches = matches.get(&best_depth)?;
    if best_matches.len() != 1 {
        return None;
    }
    Some(RustPathTarget {
        target_relative_path: best_matches.iter().next()?.clone(),
        trailing_segments: segments[best_depth..].to_vec(),
    })
}

fn rust_file_candidates(relative_path: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let path = Path::new(relative_path);
    if path.extension().is_some() {
        candidates.push(relative_path.to_string());
        return candidates;
    }
    candidates.push(format!("{relative_path}.rs"));
    candidates.push(format!("{relative_path}/mod.rs"));
    candidates
}

fn extract_ecmascript_module_specifier(reference: &Value) -> Option<String> {
    let raw = reference["text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            reference["label"]
                .as_str()
                .filter(|value| !value.is_empty())
        })?
        .trim();
    let mut quote_iter = raw
        .char_indices()
        .filter(|(_, ch)| *ch == '\'' || *ch == '"');
    let (start_index, quote) = quote_iter.next()?;
    let remainder = &raw[start_index + 1..];
    let end_index = remainder.find(quote)?;
    Some(remainder[..end_index].to_string())
}

fn scoped_file_exists(
    lookup: &GraphLookup,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
) -> bool {
    lookup.file_node_ids.contains_key(&scoped_path_key(
        project_code,
        namespace_code,
        relative_path,
    ))
}

fn scoped_path_key(project_code: &str, namespace_code: &str, relative_path: &str) -> ScopedPathKey {
    ScopedPathKey {
        project_code: project_code.to_string(),
        namespace_code: namespace_code.to_string(),
        relative_path: relative_path.to_string(),
    }
}

fn rust_crate_root_dir(source_relative_path: &str) -> PathBuf {
    let path = Path::new(source_relative_path);
    match path.components().next() {
        Some(Component::Normal(segment)) => PathBuf::from(segment),
        _ => PathBuf::new(),
    }
}

fn rust_module_dir(source_relative_path: &str) -> PathBuf {
    let path = Path::new(source_relative_path);
    let parent = path.parent().unwrap_or(Path::new(""));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if file_name == "mod.rs" || stem == "lib" || stem == "main" {
        return parent.to_path_buf();
    }
    parent.join(stem)
}

fn normalize_relative_path(path: PathBuf) -> Option<String> {
    let mut parts = Vec::<String>::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return None;
                }
            }
            Component::Normal(segment) => parts.push(segment.to_string_lossy().into_owned()),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

fn match_structure_for_symbol(
    structure_index: &BTreeMap<(String, i64), String>,
    symbol: &Value,
) -> Option<String> {
    let symbol_name = symbol["name"].as_str()?;
    let symbol_start_line = symbol["start_line"].as_i64()?;
    structure_index
        .get(&(symbol_name.to_string(), symbol_start_line))
        .cloned()
}

fn structure_match_index(
    structure_nodes: &BTreeMap<String, Value>,
) -> BTreeMap<(String, i64), String> {
    let mut index = BTreeMap::new();
    for structure in structure_nodes.values() {
        let Some(name) = structure["name"].as_str() else {
            continue;
        };
        let Some(start_line) = structure["start_line"].as_i64() else {
            continue;
        };
        let Some(node_id) = structure["node_id"].as_str() else {
            continue;
        };
        index
            .entry((name.to_string(), start_line))
            .or_insert_with(|| node_id.to_string());
    }
    index
}

fn graph_summary(nodes: &[Value], edges: &[Value]) -> Value {
    let mut counts = BTreeMap::<String, usize>::new();
    let mut relation_counts = BTreeMap::<String, usize>::new();
    for node in nodes {
        let key = node["node_type"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(key).or_default() += 1;
    }
    for edge in edges {
        let key = edge["relation"].as_str().unwrap_or("unknown").to_string();
        *relation_counts.entry(key).or_default() += 1;
    }
    json!({
        "node_counts": counts,
        "relation_counts": relation_counts,
        "edge_count": edges.len(),
    })
}

fn file_node_id(project_code: &str, namespace_code: &str, relative_path: &str) -> String {
    format!("file:{project_code}:{namespace_code}:{relative_path}")
}

fn symbol_node_id(
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    name: &str,
    start_line: i32,
) -> String {
    format!("symbol:{project_code}:{namespace_code}:{relative_path}:{name}:{start_line}")
}

fn chunk_node_id(
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    chunk_id: &str,
) -> String {
    format!("chunk:{project_code}:{namespace_code}:{relative_path}:{chunk_id}")
}

fn structure_node_id(
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    kind: &str,
    name: Option<&str>,
    start_line: i64,
) -> String {
    format!(
        "structure:{project_code}:{namespace_code}:{relative_path}:{kind}:{}:{start_line}",
        name.unwrap_or("_")
    )
}

fn call_node_id(
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    label: &str,
    start_line: i64,
) -> String {
    format!(
        "call_ref:{project_code}:{namespace_code}:{relative_path}:{}:{start_line}",
        short_hash(label)
    )
}

fn reference_node_id(
    node_type: &str,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
    label: &str,
    start_line: i64,
) -> String {
    format!(
        "{node_type}:{project_code}:{namespace_code}:{relative_path}:{}:{start_line}",
        short_hash(label)
    )
}

fn short_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())[0..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        FileAggregate, GraphLookup, build_context_pack_workspace_graph, file_node_id,
        human_summary, merge_workspace_graphs, resolve_rust_owned_symbol_name_target,
        resolve_rust_symbol_name_target, scoped_path_key,
    };
    use crate::postgres::{
        ChunkHit, DocumentHit, DocumentScopedSymbolRecord, DocumentStructureRecord, SymbolHit,
    };
    use proptest::prelude::*;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use uuid::Uuid;

    fn fake_file(relative_path: &str) -> FileAggregate {
        FileAggregate {
            node_id: file_node_id("project_alpha", "default", relative_path),
            project_code: "project_alpha".to_string(),
            namespace_code: "default".to_string(),
            repo_root: "/repo".to_string(),
            relative_path: relative_path.to_string(),
            language: Some("rust".to_string()),
            source_kind: "git_tracked".to_string(),
            git_commit_sha: None,
            structure_nodes: BTreeMap::new(),
            symbol_nodes: BTreeMap::new(),
            reference_nodes: BTreeMap::new(),
            call_nodes: BTreeMap::new(),
            retrieved_via: BTreeSet::new(),
        }
    }

    fn fake_lookup() -> GraphLookup {
        let mut lookup = GraphLookup::default();
        for relative_path in ["src/lib.rs", "src/alpha.rs", "src/beta.rs"] {
            let file_key = scoped_path_key("project_alpha", "default", relative_path);
            lookup.file_node_ids.insert(
                file_key,
                file_node_id("project_alpha", "default", relative_path),
            );
        }
        lookup
    }

    #[test]
    fn context_pack_workspace_graph_contains_structure_symbol_chunk_and_reference_nodes() {
        let context_pack_id = Uuid::parse_str("00000000-0000-0000-0000-000000000321").unwrap();
        let graph = build_context_pack_workspace_graph(
            &context_pack_id,
            "alpha runtime",
            "local_strict",
            "scope-1",
            &json!([{
                "project_code": "project_alpha",
                "namespace_code": "default"
            }]),
            &[DocumentHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                score: 10.0,
                snippet: "fn alpha() {}".to_string(),
            }],
            &[SymbolHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                name: "alpha".to_string(),
                kind: "function_item".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: 20,
                score: 9.0,
                metadata: json!({"language":"rust","node_kind":"function_item","text":"fn alpha() {}"}),
            }],
            &[ChunkHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                chunk_id: Uuid::parse_str("00000000-0000-0000-0000-000000000111").unwrap(),
                chunk_index: 0,
                start_line: 1,
                end_line: 3,
                score: 8.0,
                content: "fn alpha() {}".to_string(),
                metadata: json!({"language":"rust","symbols_defined":["alpha"]}),
            }],
            &[json!({
                "project_code": "project_alpha",
                "namespace_code": "default",
                "relative_path": "src/lib.rs",
                "retrieval_strategy": "vector_search",
                "provenance": {
                    "repo_root": "/repo",
                    "chunk_id": "00000000-0000-0000-0000-000000000222"
                },
                "chunk": {
                    "chunk_index": 1,
                    "start_line": 4,
                    "end_line": 6,
                    "metadata": {
                        "symbols_defined": ["beta"]
                    }
                }
            })],
            &[DocumentStructureRecord {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                structure: json!([{
                    "kind": "function_item",
                    "name": "alpha",
                    "start_line": 1,
                    "end_line": 3
                }]),
                imports: json!([{
                    "kind": "use_declaration",
                    "name": "crate::beta",
                    "text": "use crate::beta;",
                    "start_line": 1,
                    "end_line": 1
                }]),
                exports: json!([]),
                metadata: json!({"call_references":[]}),
            }],
            &[DocumentScopedSymbolRecord {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                name: "alpha".to_string(),
                kind: "function_item".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: 20,
                metadata: json!({"language":"rust","node_kind":"function_item","text":"fn alpha() {}"}),
            }],
        )
        .expect("graph");
        assert_eq!(graph["summary"]["node_counts"]["file"].as_u64(), Some(1));
        assert_eq!(graph["summary"]["node_counts"]["symbol"].as_u64(), Some(1));
        assert_eq!(graph["summary"]["node_counts"]["chunk"].as_u64(), Some(2));
        assert_eq!(
            graph["summary"]["node_counts"]["structure_item"].as_u64(),
            Some(1)
        );
        assert_eq!(
            graph["summary"]["node_counts"]["import_ref"].as_u64(),
            Some(1)
        );
        let edges = graph["edges"].as_array().expect("edges");
        assert!(
            edges
                .iter()
                .any(|edge| edge["relation"] == json!("retrieved_exact_document"))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge["relation"] == json!("retrieved_symbol"))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge["relation"] == json!("retrieved_lexical_chunk"))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge["relation"] == json!("retrieved_semantic_chunk"))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge["relation"] == json!("contains_structure"))
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge["relation"] == json!("imports_reference"))
        );
        assert_eq!(
            graph["summary"]["relation_counts"]["contains_symbol"].as_u64(),
            Some(1)
        );
        assert_eq!(
            graph["summary"]["relation_counts"]["defines_symbol"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn context_pack_workspace_graph_resolves_rust_file_and_symbol_imports() {
        let context_pack_id = Uuid::parse_str("00000000-0000-0000-0000-000000000654").unwrap();
        let graph = build_context_pack_workspace_graph(
            &context_pack_id,
            "Beta",
            "local_strict",
            "scope-1",
            &json!([{
                "project_code": "project_alpha",
                "namespace_code": "default"
            }]),
            &[DocumentHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                score: 10.0,
                snippet: "use crate::alpha::Beta;".to_string(),
            }],
            &[],
            &[],
            &[],
            &[
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/lib.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([]),
                    imports: json!([{
                        "kind": "use_declaration",
                        "name": "crate::alpha::Beta",
                        "text": "use crate::alpha::Beta;",
                        "start_line": 1,
                        "end_line": 1
                    }]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([{
                        "kind": "struct_item",
                        "name": "Beta",
                        "start_line": 1,
                        "end_line": 3
                    }]),
                    imports: json!([]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
            ],
            &[DocumentScopedSymbolRecord {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/alpha.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                name: "Beta".to_string(),
                kind: "struct_item".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: 32,
                metadata: json!({"language":"rust","node_kind":"struct_item","text":"pub struct Beta;"}),
            }],
        )
        .expect("graph");
        let edges = graph["edges"].as_array().expect("edges");
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("imports_file")
                && edge["to_node_id"] == json!("file:project_alpha:default:src/alpha.rs")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("imports_symbol")
                && edge["to_node_id"] == json!("symbol:project_alpha:default:src/alpha.rs:Beta:1")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("resolves_symbol")
                && edge["to_node_id"] == json!("symbol:project_alpha:default:src/alpha.rs:Beta:1")
        }));
        assert_eq!(
            graph["summary"]["relation_counts"]["imports_file"].as_u64(),
            Some(1)
        );
        assert_eq!(
            graph["summary"]["relation_counts"]["imports_symbol"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn context_pack_workspace_graph_fails_closed_on_ambiguous_rust_module_targets() {
        let context_pack_id = Uuid::parse_str("00000000-0000-0000-0000-000000000655").unwrap();
        let graph = build_context_pack_workspace_graph(
            &context_pack_id,
            "Beta",
            "local_strict",
            "scope-1",
            &json!([{
                "project_code": "project_alpha",
                "namespace_code": "default"
            }]),
            &[DocumentHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                score: 10.0,
                snippet: "use crate::alpha::Beta;".to_string(),
            }],
            &[],
            &[],
            &[],
            &[
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/lib.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([]),
                    imports: json!([{
                        "kind": "use_declaration",
                        "name": "crate::alpha::Beta",
                        "text": "use crate::alpha::Beta;",
                        "start_line": 1,
                        "end_line": 1
                    }]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([]),
                    imports: json!([]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha/mod.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([]),
                    imports: json!([]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
            ],
            &[],
        )
        .expect("graph");
        let relation_counts = graph["summary"]["relation_counts"]
            .as_object()
            .expect("relation counts");
        assert!(!relation_counts.contains_key("imports_file"));
        assert!(!relation_counts.contains_key("imports_symbol"));
    }

    #[test]
    fn context_pack_workspace_graph_resolves_calls_to_imported_symbols() {
        let context_pack_id = Uuid::parse_str("00000000-0000-0000-0000-000000000656").unwrap();
        let graph = build_context_pack_workspace_graph(
            &context_pack_id,
            "beta_name",
            "local_strict",
            "scope-1",
            &json!([{
                "project_code": "project_alpha",
                "namespace_code": "default"
            }]),
            &[DocumentHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                score: 10.0,
                snippet: "beta_name()".to_string(),
            }],
            &[],
            &[],
            &[],
            &[
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/lib.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([{
                        "kind": "function_item",
                        "name": "runtime_summary",
                        "start_line": 3,
                        "end_line": 5
                    }]),
                    imports: json!([{
                        "kind": "use_declaration",
                        "name": "crate::alpha::beta_name",
                        "text": "use crate::alpha::beta_name;",
                        "start_line": 1,
                        "end_line": 1
                    }]),
                    exports: json!([]),
                    metadata: json!({
                        "call_references": [{
                            "kind": "call_expression",
                            "call_style": "identifier",
                            "callee_name": "beta_name",
                            "callee_path": "beta_name",
                            "receiver_text": null,
                            "generic": false,
                            "start_line": 4,
                            "end_line": 4,
                            "text": "beta_name()"
                        }]
                    }),
                },
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([{
                        "kind": "function_item",
                        "name": "beta_name",
                        "start_line": 1,
                        "end_line": 3
                    }]),
                    imports: json!([]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
            ],
            &[DocumentScopedSymbolRecord {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/alpha.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                name: "beta_name".to_string(),
                kind: "function_item".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: 40,
                metadata: json!({"language":"rust","node_kind":"function_item","text":"pub fn beta_name() -> &'static str { \"beta\" }"}),
            }],
        )
        .expect("graph");
        let edges = graph["edges"].as_array().expect("edges");
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("calls_file")
                && edge["to_node_id"] == json!("file:project_alpha:default:src/alpha.rs")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("calls_symbol")
                && edge["to_node_id"]
                    == json!("symbol:project_alpha:default:src/alpha.rs:beta_name:1")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("resolves_call_symbol")
                && edge["to_node_id"]
                    == json!("symbol:project_alpha:default:src/alpha.rs:beta_name:1")
        }));
        assert_eq!(
            graph["summary"]["node_counts"]["call_ref"].as_u64(),
            Some(1)
        );
        assert_eq!(
            graph["summary"]["relation_counts"]["calls_symbol"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn context_pack_workspace_graph_resolves_owner_scoped_calls_via_imported_type() {
        let context_pack_id = Uuid::parse_str("00000000-0000-0000-0000-000000000657").unwrap();
        let graph = build_context_pack_workspace_graph(
            &context_pack_id,
            "Beta::new",
            "local_strict",
            "scope-1",
            &json!([{
                "project_code": "project_alpha",
                "namespace_code": "default"
            }]),
            &[DocumentHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/lib.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                score: 10.0,
                snippet: "Beta::new()".to_string(),
            }],
            &[],
            &[],
            &[],
            &[
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/lib.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([{
                        "kind": "function_item",
                        "name": "runtime_summary",
                        "start_line": 3,
                        "end_line": 5
                    }]),
                    imports: json!([{
                        "kind": "use_declaration",
                        "name": "crate::alpha::Beta",
                        "text": "use crate::alpha::Beta;",
                        "start_line": 1,
                        "end_line": 1
                    }]),
                    exports: json!([]),
                    metadata: json!({
                        "call_references": [{
                            "kind": "call_expression",
                            "call_style": "scoped_identifier",
                            "callee_name": "new",
                            "callee_path": "Beta::new",
                            "receiver_text": null,
                            "generic": false,
                            "start_line": 4,
                            "end_line": 4,
                            "text": "Beta::new()"
                        }]
                    }),
                },
                DocumentStructureRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    structure: json!([
                        {
                            "kind": "struct_item",
                            "name": "Beta",
                            "start_line": 1,
                            "end_line": 1
                        },
                        {
                            "kind": "function_item",
                            "name": "new",
                            "start_line": 4,
                            "end_line": 6
                        },
                        {
                            "kind": "function_item",
                            "name": "new",
                            "start_line": 9,
                            "end_line": 11
                        }
                    ]),
                    imports: json!([]),
                    exports: json!([]),
                    metadata: json!({"call_references":[]}),
                },
            ],
            &[
                DocumentScopedSymbolRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    name: "Beta".to_string(),
                    kind: "struct_item".to_string(),
                    start_line: 1,
                    end_line: 1,
                    start_byte: 0,
                    end_byte: 16,
                    metadata: json!({"language":"rust","node_kind":"struct_item","text":"pub struct Beta;"}),
                },
                DocumentScopedSymbolRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    name: "new".to_string(),
                    kind: "function_item".to_string(),
                    start_line: 4,
                    end_line: 6,
                    start_byte: 17,
                    end_byte: 62,
                    metadata: json!({
                        "language":"rust",
                        "node_kind":"function_item",
                        "owner_kind":"impl_item",
                        "owner_name":"Beta",
                        "owner_path":"Beta",
                        "text":"impl Beta { pub fn new() -> Self { Self } }"
                    }),
                },
                DocumentScopedSymbolRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    name: "new".to_string(),
                    kind: "function_item".to_string(),
                    start_line: 9,
                    end_line: 11,
                    start_byte: 63,
                    end_byte: 92,
                    metadata: json!({
                        "language":"rust",
                        "node_kind":"function_item",
                        "text":"pub fn new() -> Beta { Beta }"
                    }),
                },
            ],
        )
        .expect("graph");
        let edges = graph["edges"].as_array().expect("edges");
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("calls_symbol")
                && edge["to_node_id"] == json!("symbol:project_alpha:default:src/alpha.rs:new:4")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("resolves_call_symbol")
                && edge["to_node_id"] == json!("symbol:project_alpha:default:src/alpha.rs:new:4")
        }));
        assert_eq!(
            graph["summary"]["relation_counts"]["calls_symbol"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn context_pack_workspace_graph_resolves_self_field_calls_inside_impl() {
        let context_pack_id = Uuid::parse_str("00000000-0000-0000-0000-000000000658").unwrap();
        let graph = build_context_pack_workspace_graph(
            &context_pack_id,
            "self.helper",
            "local_strict",
            "scope-1",
            &json!([{
                "project_code": "project_alpha",
                "namespace_code": "default"
            }]),
            &[DocumentHit {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/alpha.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                score: 10.0,
                snippet: "self.helper()".to_string(),
            }],
            &[],
            &[],
            &[],
            &[DocumentStructureRecord {
                project_code: "project_alpha".to_string(),
                namespace_code: "default".to_string(),
                repo_root: "/repo".to_string(),
                relative_path: "src/alpha.rs".to_string(),
                language: Some("rust".to_string()),
                source_kind: "git_tracked".to_string(),
                git_commit_sha: Some("abc".to_string()),
                structure: json!([
                    {
                        "kind": "function_item",
                        "name": "helper",
                        "start_line": 4,
                        "end_line": 6
                    },
                    {
                        "kind": "function_item",
                        "name": "helper",
                        "start_line": 9,
                        "end_line": 11
                    },
                    {
                        "kind": "function_item",
                        "name": "make",
                        "start_line": 13,
                        "end_line": 15
                    }
                ]),
                imports: json!([]),
                exports: json!([]),
                metadata: json!({
                    "call_references": [{
                        "kind": "call_expression",
                        "call_style": "field_expression",
                        "callee_name": "helper",
                        "callee_path": "self.helper",
                        "receiver_text": "self",
                        "enclosing_owner_kind": "impl_item",
                        "enclosing_owner_name": "Beta",
                        "enclosing_owner_path": "Beta",
                        "enclosing_trait_name": null,
                        "generic": false,
                        "start_line": 14,
                        "end_line": 14,
                        "text": "self.helper()"
                    }]
                }),
            }],
            &[
                DocumentScopedSymbolRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    name: "helper".to_string(),
                    kind: "function_item".to_string(),
                    start_line: 4,
                    end_line: 6,
                    start_byte: 17,
                    end_byte: 62,
                    metadata: json!({
                        "language":"rust",
                        "node_kind":"function_item",
                        "owner_kind":"impl_item",
                        "owner_name":"Beta",
                        "owner_path":"Beta",
                        "text":"fn helper(&self) -> Self { Self }"
                    }),
                },
                DocumentScopedSymbolRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    name: "helper".to_string(),
                    kind: "function_item".to_string(),
                    start_line: 9,
                    end_line: 11,
                    start_byte: 63,
                    end_byte: 92,
                    metadata: json!({
                        "language":"rust",
                        "node_kind":"function_item",
                        "text":"pub fn helper() -> Beta { Beta }"
                    }),
                },
                DocumentScopedSymbolRecord {
                    project_code: "project_alpha".to_string(),
                    namespace_code: "default".to_string(),
                    repo_root: "/repo".to_string(),
                    relative_path: "src/alpha.rs".to_string(),
                    language: Some("rust".to_string()),
                    source_kind: "git_tracked".to_string(),
                    git_commit_sha: Some("abc".to_string()),
                    name: "make".to_string(),
                    kind: "function_item".to_string(),
                    start_line: 13,
                    end_line: 15,
                    start_byte: 93,
                    end_byte: 140,
                    metadata: json!({
                        "language":"rust",
                        "node_kind":"function_item",
                        "owner_kind":"impl_item",
                        "owner_name":"Beta",
                        "owner_path":"Beta",
                        "text":"pub fn make(&self) -> Self { self.helper() }"
                    }),
                },
            ],
        )
        .expect("graph");
        let edges = graph["edges"].as_array().expect("edges");
        assert!(
            !edges
                .iter()
                .any(|edge| edge["relation"] == json!("calls_file"))
        );
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("calls_symbol")
                && edge["to_node_id"] == json!("symbol:project_alpha:default:src/alpha.rs:helper:4")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("resolves_call_file")
                && edge["to_node_id"] == json!("file:project_alpha:default:src/alpha.rs")
        }));
        assert!(edges.iter().any(|edge| {
            edge["relation"] == json!("resolves_call_symbol")
                && edge["to_node_id"] == json!("symbol:project_alpha:default:src/alpha.rs:helper:4")
        }));
    }

    proptest! {
        #[test]
        fn plain_symbol_resolution_stays_fail_closed_under_candidate_ambiguity(
            source_matches in 0usize..3,
            alpha_matches in 0usize..3,
            beta_matches in 0usize..3,
        ) {
            let source_file = fake_file("src/lib.rs");
            let alpha_key = scoped_path_key("project_alpha", "default", "src/alpha.rs");
            let beta_key = scoped_path_key("project_alpha", "default", "src/beta.rs");
            let mut imported_files = BTreeSet::new();
            imported_files.insert(alpha_key.clone());
            imported_files.insert(beta_key.clone());
            let mut lookup = fake_lookup();

            for (relative_path, count, label) in [
                ("src/lib.rs", source_matches, "source"),
                ("src/alpha.rs", alpha_matches, "alpha"),
                ("src/beta.rs", beta_matches, "beta"),
            ] {
                if count == 0 {
                    continue;
                }
                let file_key = scoped_path_key("project_alpha", "default", relative_path);
                let entry = lookup
                    .symbol_node_ids
                    .entry((file_key, "new".to_string()))
                    .or_default();
                for index in 0..count {
                    entry.push(format!("symbol:{label}:new:{index}"));
                }
            }

            let resolution = resolve_rust_symbol_name_target(
                &source_file,
                "new",
                &lookup,
                Some(&imported_files),
            );

            let file_states = [
                ("src/lib.rs", source_matches),
                ("src/alpha.rs", alpha_matches),
                ("src/beta.rs", beta_matches),
            ];
            let ambiguous = file_states.iter().any(|(_, count)| *count > 1);
            let unique_files = file_states
                .iter()
                .filter(|(_, count)| *count == 1)
                .map(|(relative_path, _)| *relative_path)
                .collect::<Vec<_>>();

            if ambiguous || unique_files.len() != 1 {
                prop_assert!(resolution.is_none());
            } else {
                let resolution = resolution.expect("unique resolution");
                prop_assert_eq!(resolution.target_file_key.relative_path, unique_files[0]);
            }
        }

        #[test]
        fn owner_symbol_resolution_stays_fail_closed_under_candidate_ambiguity(
            source_matches in 0usize..3,
            alpha_matches in 0usize..3,
            beta_matches in 0usize..3,
        ) {
            let source_file = fake_file("src/lib.rs");
            let alpha_key = scoped_path_key("project_alpha", "default", "src/alpha.rs");
            let beta_key = scoped_path_key("project_alpha", "default", "src/beta.rs");
            let mut imported_files = BTreeSet::new();
            imported_files.insert(alpha_key.clone());
            imported_files.insert(beta_key.clone());
            let mut lookup = fake_lookup();

            for (relative_path, count, label) in [
                ("src/lib.rs", source_matches, "source"),
                ("src/alpha.rs", alpha_matches, "alpha"),
                ("src/beta.rs", beta_matches, "beta"),
            ] {
                if count == 0 {
                    continue;
                }
                let file_key = scoped_path_key("project_alpha", "default", relative_path);
                let entry = lookup
                    .owner_symbol_node_ids
                    .entry((file_key, "Beta".to_string(), "new".to_string()))
                    .or_default();
                for index in 0..count {
                    entry.push(format!("symbol:{label}:Beta:new:{index}"));
                }
            }

            let resolution = resolve_rust_owned_symbol_name_target(
                &source_file,
                "Beta",
                "new",
                &lookup,
                Some(&imported_files),
            );

            let file_states = [
                ("src/lib.rs", source_matches),
                ("src/alpha.rs", alpha_matches),
                ("src/beta.rs", beta_matches),
            ];
            let ambiguous = file_states.iter().any(|(_, count)| *count > 1);
            let unique_files = file_states
                .iter()
                .filter(|(_, count)| *count == 1)
                .map(|(relative_path, _)| *relative_path)
                .collect::<Vec<_>>();

            if ambiguous || unique_files.len() != 1 {
                prop_assert!(resolution.is_none());
            } else {
                let resolution = resolution.expect("unique owner resolution");
                prop_assert_eq!(resolution.target_file_key.relative_path, unique_files[0]);
            }
        }
    }

    #[test]
    fn merge_workspace_graphs_deduplicates_nodes_and_keeps_context_pack_lineage() {
        let merged = merge_workspace_graphs(&[
            json!({
                "workspace_graph_model_version": "workspace-graph-v5",
                "artifact_lineage_model_version": "artifact-lineage-v1",
                "lineage_model_version": "lineage-v2",
                "truth_ranking": ["continuity_handoff"],
                "scope_signature": "scope-a",
                "visible_projects": [{"project_code":"project_alpha","namespace_code":"default"}],
                "source_context_pack_ids": ["ctx-1"],
                "nodes": [
                    {"node_id":"file:project_alpha:default:src/lib.rs","node_type":"file"}
                ],
                "edges": [
                    {"from_node_id":"context_pack:ctx-1","to_node_id":"file:project_alpha:default:src/lib.rs","relation":"retrieved_exact_document"}
                ]
            }),
            json!({
                "workspace_graph_model_version": "workspace-graph-v5",
                "artifact_lineage_model_version": "artifact-lineage-v1",
                "lineage_model_version": "lineage-v2",
                "truth_ranking": ["continuity_handoff"],
                "scope_signature": "scope-b",
                "visible_projects": [{"project_code":"project_alpha","namespace_code":"default"}],
                "source_context_pack_ids": ["ctx-2"],
                "nodes": [
                    {"node_id":"file:project_alpha:default:src/lib.rs","node_type":"file"},
                    {"node_id":"symbol:project_alpha:default:src/lib.rs:alpha:1","node_type":"symbol"}
                ],
                "edges": [
                    {"from_node_id":"file:project_alpha:default:src/lib.rs","to_node_id":"symbol:project_alpha:default:src/lib.rs:alpha:1","relation":"contains_symbol"}
                ]
            }),
        ]);
        assert_eq!(
            merged["source_context_pack_ids"].as_array().unwrap().len(),
            2
        );
        assert_eq!(merged["scope_signatures"].as_array().unwrap().len(), 2);
        assert_eq!(merged["summary"]["node_counts"]["file"].as_u64(), Some(1));
        assert_eq!(merged["summary"]["node_counts"]["symbol"].as_u64(), Some(1));
        assert_eq!(merged["summary"]["edge_count"].as_u64(), Some(2));
    }

    #[test]
    fn human_summary_mentions_structural_counts() {
        let summary = human_summary(&json!({
            "source_context_pack_ids": ["ctx-a"],
            "summary": {
                "node_counts": {
                    "file": 2,
                    "structure_item": 3,
                    "symbol": 4,
                    "chunk": 1,
                    "import_ref": 2,
                    "export_ref": 1
                }
            }
        }))
        .expect("summary");
        assert!(summary.contains("файлов 2"));
        assert!(summary.contains("символов 4"));
    }
}
