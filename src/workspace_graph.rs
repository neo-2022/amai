use crate::postgres::{ChunkHit, DocumentHit, DocumentStructureRecord, SymbolHit};
use crate::retrieval_science;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
    reference_nodes: BTreeMap<String, Value>,
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
        let symbol_node_id = symbol_node_id(
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
            &hit.name,
            hit.start_line,
        );
        nodes.entry(symbol_node_id.clone()).or_insert_with(|| {
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
                "metadata": compact_symbol_metadata(&hit.metadata),
            })
        });
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

    for file in files.into_values() {
        let file_node_id = file.node_id.clone();
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
                "reference_count": file.reference_nodes.len(),
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
    let context_packs = graph["source_context_pack_ids"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    if files == 0 && symbols == 0 && chunks == 0 && imports == 0 && exports == 0 {
        return None;
    }
    Some(format!(
        "структурный граф: контекстных снимков {context_packs}, файлов {files}, структурных узлов {structures}, символов {symbols}, фрагментов {chunks}, импортов {imports}, экспортов {exports}"
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
            reference_nodes: BTreeMap::new(),
            retrieved_via: BTreeSet::new(),
        })
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
            reference_nodes: BTreeMap::new(),
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
                    "kind": item["kind"].clone(),
                    "start_line": item["start_line"].clone(),
                    "end_line": item["end_line"].clone(),
                })
            });
    }
}

fn compact_symbol_metadata(metadata: &Value) -> Value {
    json!({
        "language": metadata["language"].clone(),
        "node_kind": metadata["node_kind"].clone(),
    })
}

fn graph_summary(nodes: &[Value], edges: &[Value]) -> Value {
    let mut counts = BTreeMap::<String, usize>::new();
    for node in nodes {
        let key = node["node_type"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(key).or_default() += 1;
    }
    json!({
        "node_counts": counts,
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
    use super::{build_context_pack_workspace_graph, human_summary, merge_workspace_graphs};
    use crate::postgres::{ChunkHit, DocumentHit, DocumentStructureRecord, SymbolHit};
    use serde_json::json;
    use uuid::Uuid;

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
    }

    #[test]
    fn merge_workspace_graphs_deduplicates_nodes_and_keeps_context_pack_lineage() {
        let merged = merge_workspace_graphs(&[
            json!({
                "workspace_graph_model_version": "workspace-graph-v1",
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
                "workspace_graph_model_version": "workspace-graph-v1",
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
