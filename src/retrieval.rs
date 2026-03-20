use crate::cli::ContextPackArgs;
use crate::config::AppConfig;
use crate::edge_cache;
use crate::postgres::{
    self, ChunkHit, DocumentHit, ProjectRecord, SymbolHit, VisibleProjectRecord,
};
use crate::qdrant;
use crate::s3;
use anyhow::{Context, Result, anyhow};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use qdrant_client::qdrant::{ScoredPoint, point_id::PointIdOptions};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ContextPackStats {
    pub context_pack_id: Uuid,
    pub exact_documents: usize,
    pub symbol_hits: usize,
    pub lexical_chunks: usize,
    pub semantic_chunks: usize,
}

#[derive(Debug, Clone)]
struct PreparedContextPack {
    context_pack_id: Uuid,
    project: ProjectRecord,
    namespace_id: Uuid,
    effective_mode: String,
    visible_projects_json: Value,
    payload: Value,
    payload_json: String,
    stats: ContextPackStats,
}

pub async fn build_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<()> {
    let prepared = prepare_context_pack(cfg, db, args).await?;
    persist_context_pack(cfg, db, args, &prepared).await?;
    eprintln!(
        "context pack stored: s3://{}/context-packs/{}/{}/{}.json :: {}",
        cfg.s3_bucket_context,
        prepared.project.code,
        prepared.effective_mode,
        prepared.context_pack_id,
        prepared.context_pack_id
    );
    println!("{}", prepared.payload_json);
    Ok(())
}

pub async fn execute_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<ContextPackStats> {
    let prepared = prepare_context_pack(cfg, db, args).await?;
    if persist {
        persist_context_pack(cfg, db, args, &prepared).await?;
    }
    Ok(prepared.stats)
}

async fn prepare_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<PreparedContextPack> {
    let project = postgres::get_project_by_code(db, &args.project).await?;
    let namespace =
        postgres::get_namespace_by_code(db, project.project_id, &args.namespace).await?;
    let effective_mode = args
        .retrieval_mode
        .clone()
        .unwrap_or_else(|| namespace.retrieval_mode.clone());
    let visible_projects = resolve_visible_projects(db, &project, &effective_mode).await?;

    let mut documents = Vec::new();
    let mut symbols = Vec::new();
    let mut chunks = Vec::new();
    for visible in &visible_projects {
        documents.extend(
            postgres::search_documents_for_project(
                db,
                visible.project.project_id,
                &args.query,
                args.limit_documents as i64,
            )
            .await?,
        );
        symbols.extend(
            postgres::search_symbols_for_project(
                db,
                visible.project.project_id,
                &args.query,
                args.limit_symbols as i64,
            )
            .await?,
        );
        chunks.extend(
            postgres::search_chunks_for_project(
                db,
                visible.project.project_id,
                &args.query,
                args.limit_chunks as i64,
            )
            .await?,
        );
    }

    sort_and_truncate_documents(&mut documents, args.limit_documents);
    sort_and_truncate_symbols(&mut symbols, args.limit_symbols);
    sort_and_truncate_chunks(&mut chunks, args.limit_chunks);

    let semantic_chunks = semantic_chunks(
        cfg,
        db,
        &visible_projects,
        &args.query,
        args.limit_semantic_chunks,
    )
    .await?;
    let visible_projects_json = json!(
        visible_projects
            .iter()
            .map(|visible| {
                json!({
                    "project_code": visible.project.code,
                    "display_name": visible.project.display_name,
                    "repo_root": visible.project.repo_root,
                    "relation_type": visible.relation_type,
                    "shared_contour": visible.shared_contour,
                    "access_mode": visible.access_mode
                })
            })
            .collect::<Vec<_>>()
    );

    let context_pack_id = Uuid::new_v4();
    let generated_epoch_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs();
    let stats = ContextPackStats {
        context_pack_id,
        exact_documents: documents.len(),
        symbol_hits: symbols.len(),
        lexical_chunks: chunks.len(),
        semantic_chunks: semantic_chunks.len(),
    };
    let payload = json!({
        "context_pack_id": context_pack_id,
        "stack_name": cfg.stack_name,
        "tool_name": "Art-memory-agent-index",
        "tool_short_name": "Amai",
        "generated_at_epoch_s": generated_epoch_s,
        "project": {
            "code": project.code,
            "display_name": project.display_name,
            "repo_root": project.repo_root
        },
        "namespace": {
            "code": namespace.code,
            "display_name": namespace.display_name
        },
        "query": args.query,
        "effective_retrieval_mode": effective_mode,
        "visible_projects": visible_projects_json,
        "retrieval": {
            "exact_documents": documents.iter().map(document_to_json).collect::<Vec<_>>(),
            "symbol_hits": symbols.iter().map(symbol_to_json).collect::<Vec<_>>(),
            "lexical_chunks": chunks.iter().map(chunk_to_json).collect::<Vec<_>>(),
            "semantic_chunks": semantic_chunks
        },
        "provenance_minimum": [
            "source_project",
            "repo_root",
            "commit_sha",
            "path",
            "symbol",
            "chunk_id",
            "source_kind",
            "trust_level"
        ]
    });
    let payload_json = serde_json::to_string_pretty(&payload)?;

    Ok(PreparedContextPack {
        context_pack_id,
        project,
        namespace_id: namespace.namespace_id,
        effective_mode,
        visible_projects_json,
        payload,
        payload_json,
        stats,
    })
}

async fn persist_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    prepared: &PreparedContextPack,
) -> Result<()> {
    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;
    edge_cache::cache_context_pack(
        &edge_cache_path,
        &prepared.context_pack_id.to_string(),
        &prepared.project.code,
        &prepared.effective_mode,
        &prepared.payload_json,
    )?;

    let object_key = format!(
        "context-packs/{}/{}/{}.json",
        prepared.project.code, prepared.effective_mode, prepared.context_pack_id
    );
    let s3_client = s3::connect(cfg).await?;
    s3::put_json_object(
        &s3_client,
        &cfg.s3_bucket_context,
        &object_key,
        &prepared.payload_json,
    )
    .await?;

    let artifact_metadata = json!({
        "context_pack_id": prepared.context_pack_id,
        "query": args.query,
        "effective_retrieval_mode": prepared.effective_mode
    });
    let artifact_ref_id = postgres::insert_artifact_ref(
        db,
        &postgres::ArtifactRefInsert {
            project_id: prepared.project.project_id,
            namespace_id: prepared.namespace_id,
            artifact_kind: "context_pack",
            bucket: &cfg.s3_bucket_context,
            object_key: &object_key,
            content_type: Some("application/json"),
            metadata: &artifact_metadata,
        },
    )
    .await?;

    postgres::insert_context_pack(
        db,
        &postgres::ContextPackInsert {
            context_pack_id: prepared.context_pack_id,
            project_id: prepared.project.project_id,
            namespace_id: prepared.namespace_id,
            retrieval_mode: &prepared.effective_mode,
            query_text: &args.query,
            visible_projects: &prepared.visible_projects_json,
            payload: &prepared.payload,
            artifact_ref_id: Some(artifact_ref_id),
        },
    )
    .await?;
    Ok(())
}

async fn resolve_visible_projects(
    db: &Client,
    project: &ProjectRecord,
    effective_mode: &str,
) -> Result<Vec<VisibleProjectRecord>> {
    if effective_mode == "audit_global" {
        let mut visible = Vec::new();
        for candidate in postgres::list_projects(db).await? {
            let is_local = candidate.project_id == project.project_id;
            visible.push(VisibleProjectRecord {
                project: candidate,
                relation_type: if is_local {
                    "local".to_string()
                } else {
                    "audit_global".to_string()
                },
                shared_contour: if is_local {
                    "self".to_string()
                } else {
                    "global_audit".to_string()
                },
                access_mode: "audit_global".to_string(),
            });
        }
        return Ok(visible);
    }

    let mut visible = vec![VisibleProjectRecord {
        project: project.clone(),
        relation_type: "local".to_string(),
        shared_contour: "self".to_string(),
        access_mode: "local_strict".to_string(),
    }];
    let allowed_rank = mode_rank(effective_mode)?;
    let related = postgres::list_related_projects(db, project.project_id).await?;
    for relation in related {
        if mode_rank(&relation.access_mode)? <= allowed_rank {
            visible.push(relation);
        }
    }
    Ok(visible)
}

async fn semantic_chunks(
    cfg: &AppConfig,
    db: &Client,
    visible_projects: &[VisibleProjectRecord],
    query: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut embedder = build_query_embedder(cfg)?;
    let embeddings = embedder.embed(&[query.to_string()], Some(1))?;
    let vector = embeddings
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("semantic embedder returned no query vector"))?;
    if vector.len() as u64 != cfg.qdrant_code_dim {
        return Err(anyhow!(
            "query embedding size mismatch: expected {}, got {}",
            cfg.qdrant_code_dim,
            vector.len()
        ));
    }

    let qdrant_client = qdrant::connect(cfg)?;
    let per_project_limit = limit.max(1);
    let mut hits = Vec::new();
    for visible in visible_projects {
        let result = qdrant::search_project_points(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            vector.clone(),
            &visible.project.code,
            per_project_limit,
        )
        .await?;
        for point in result {
            if let Some(hit) = semantic_point_to_json(db, point).await? {
                hits.push(hit);
            }
        }
    }

    hits.sort_by(|left, right| {
        right["score"]
            .as_f64()
            .partial_cmp(&left["score"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    hits.retain(|hit| {
        let key = format!(
            "{}::{}",
            hit["provenance"]["chunk_id"].as_str().unwrap_or(""),
            hit["provenance"]["path"].as_str().unwrap_or("")
        );
        seen.insert(key)
    });
    hits.truncate(limit);
    Ok(hits)
}

async fn semantic_point_to_json(db: &Client, point: ScoredPoint) -> Result<Option<Value>> {
    let point_id = match point.id.and_then(|id| id.point_id_options) {
        Some(PointIdOptions::Uuid(value)) => Uuid::parse_str(&value).ok(),
        _ => None,
    };
    let Some(point_id) = point_id else {
        return Ok(None);
    };
    let Some(mut chunk) = postgres::get_chunk_by_qdrant_point_id(db, point_id).await? else {
        return Ok(None);
    };
    chunk.score = point.score;
    Ok(Some(json!({
        "score": chunk.score,
        "project_code": chunk.project_code,
        "namespace_code": chunk.namespace_code,
        "relative_path": chunk.relative_path,
        "content": chunk.content,
        "provenance": {
            "source_project": chunk.project_code,
            "repo_root": chunk.repo_root,
            "commit_sha": Value::Null,
            "path": chunk.relative_path,
            "symbol": Value::Null,
            "chunk_id": chunk.chunk_id,
            "source_kind": "code_chunk",
            "trust_level": "local_repo"
        },
        "chunk": {
            "chunk_index": chunk.chunk_index,
            "start_line": chunk.start_line,
            "end_line": chunk.end_line,
            "metadata": chunk.metadata
        }
    })))
}

fn build_query_embedder(cfg: &AppConfig) -> Result<TextEmbedding> {
    let model = match cfg.code_embed_model.as_str() {
        "jina_base_code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        "multilingual_e5_small" => EmbeddingModel::MultilingualE5Small,
        "multilingual_e5_base" => EmbeddingModel::MultilingualE5Base,
        other => return Err(anyhow!("unsupported code embedding model: {other}")),
    };
    TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(true))
}

fn mode_rank(mode: &str) -> Result<u8> {
    match mode {
        "local_strict" => Ok(0),
        "local_plus_related" => Ok(1),
        "explicit_foreign" => Ok(2),
        "audit_global" => Ok(3),
        other => Err(anyhow!("unsupported retrieval mode: {other}")),
    }
}

fn sort_and_truncate_documents(documents: &mut Vec<DocumentHit>, limit: usize) {
    documents.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    documents.retain(|document| {
        seen.insert(format!(
            "{}::{}::{}",
            document.project_code, document.namespace_code, document.relative_path
        ))
    });
    documents.truncate(limit);
}

fn sort_and_truncate_symbols(symbols: &mut Vec<SymbolHit>, limit: usize) {
    symbols.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    symbols.retain(|symbol| {
        seen.insert(format!(
            "{}::{}::{}::{}::{}",
            symbol.project_code,
            symbol.namespace_code,
            symbol.relative_path,
            symbol.name,
            symbol.start_line
        ))
    });
    symbols.truncate(limit);
}

fn sort_and_truncate_chunks(chunks: &mut Vec<ChunkHit>, limit: usize) {
    chunks.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    chunks.retain(|chunk| seen.insert(chunk.chunk_id));
    chunks.truncate(limit);
}

fn document_to_json(hit: &DocumentHit) -> Value {
    json!({
        "score": hit.score,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "language": hit.language,
        "source_kind": hit.source_kind,
        "relative_path": hit.relative_path,
        "snippet": hit.snippet,
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": hit.git_commit_sha,
            "path": hit.relative_path,
            "symbol": Value::Null,
            "chunk_id": Value::Null,
            "source_kind": hit.source_kind,
            "trust_level": "local_repo"
        }
    })
}

fn symbol_to_json(hit: &SymbolHit) -> Value {
    json!({
        "score": hit.score,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "relative_path": hit.relative_path,
        "name": hit.name,
        "kind": hit.kind,
        "start_line": hit.start_line,
        "end_line": hit.end_line,
        "start_byte": hit.start_byte,
        "end_byte": hit.end_byte,
        "metadata": hit.metadata,
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": Value::Null,
            "path": hit.relative_path,
            "symbol": hit.name,
            "chunk_id": Value::Null,
            "source_kind": "symbol",
            "trust_level": "local_repo"
        }
    })
}

fn chunk_to_json(hit: &ChunkHit) -> Value {
    json!({
        "score": hit.score,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "relative_path": hit.relative_path,
        "content": hit.content,
        "chunk": {
            "chunk_id": hit.chunk_id,
            "chunk_index": hit.chunk_index,
            "start_line": hit.start_line,
            "end_line": hit.end_line,
            "metadata": hit.metadata
        },
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": Value::Null,
            "path": hit.relative_path,
            "symbol": Value::Null,
            "chunk_id": hit.chunk_id,
            "source_kind": "code_chunk",
            "trust_level": "local_repo"
        }
    })
}
