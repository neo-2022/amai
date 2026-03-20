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
use qdrant_client::qdrant::point_id::PointIdOptions;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ContextPackTimings {
    pub resolve_scope_ms: u128,
    pub cache_lookup_ms: u128,
    pub exact_lookup_ms: u128,
    pub symbol_lookup_ms: u128,
    pub lexical_lookup_ms: u128,
    pub query_embed_ms: u128,
    pub semantic_search_ms: u128,
    pub semantic_hydrate_ms: u128,
    pub serialize_ms: u128,
    pub persist_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ContextPackStats {
    pub context_pack_id: Uuid,
    pub exact_documents: usize,
    pub symbol_hits: usize,
    pub lexical_chunks: usize,
    pub semantic_chunks: usize,
    pub cache_hit: bool,
    pub scope_signature: String,
    pub timings: ContextPackTimings,
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
    cache_key: String,
    scope_signature: String,
    cache_hit: bool,
    durably_persisted: bool,
}

struct CachedQueryEmbedder {
    model: String,
    embedder: TextEmbedding,
}

#[derive(Debug, Default, Clone)]
struct SemanticTimings {
    query_embed_ms: u128,
    search_ms: u128,
    hydrate_ms: u128,
}

#[derive(Debug, Clone)]
struct CacheHydrationContext<'a> {
    project: &'a ProjectRecord,
    namespace_id: Uuid,
    effective_mode: &'a str,
    scope_signature: String,
    cache_key: String,
    resolve_scope_ms: u128,
    cache_lookup_ms: u128,
}

static QUERY_EMBEDDER: OnceLock<Mutex<Option<CachedQueryEmbedder>>> = OnceLock::new();
static LOCAL_CONTEXT_PACK_CACHE: OnceLock<
    Mutex<HashMap<String, edge_cache::CachedContextPackEntry>>,
> = OnceLock::new();

pub async fn build_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<()> {
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    cache_context_pack_entry(cfg, args, &prepared, true)?;
    if prepared.cache_hit {
        eprintln!(
            "context pack cache hit: {} :: scope={}",
            prepared.context_pack_id, prepared.scope_signature
        );
    } else {
        eprintln!(
            "context pack stored: s3://{}/context-packs/{}/{}/{}.json :: {}",
            cfg.s3_bucket_context,
            prepared.project.code,
            prepared.effective_mode,
            prepared.context_pack_id,
            prepared.context_pack_id
        );
    }
    println!("{}", prepared.payload_json);
    Ok(())
}

pub async fn execute_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<ContextPackStats> {
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    if persist {
        ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    }
    cache_context_pack_entry(cfg, args, &prepared, persist || prepared.durably_persisted)?;
    Ok(prepared.stats)
}

async fn prepare_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<PreparedContextPack> {
    let resolve_started = Instant::now();
    let project = postgres::get_project_by_code(db, &args.project).await?;
    let namespace =
        postgres::get_namespace_by_code(db, project.project_id, &args.namespace).await?;
    let effective_mode = args
        .retrieval_mode
        .clone()
        .unwrap_or_else(|| namespace.retrieval_mode.clone());
    let visible_projects = resolve_visible_projects(db, &project, &effective_mode).await?;
    let resolve_scope_ms = resolve_started.elapsed().as_millis();
    let scope_signature = scope_signature(&visible_projects);
    let cache_key = cache_key(cfg, args, &effective_mode);
    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;

    let cache_lookup_started = Instant::now();
    if !args.disable_cache {
        if let Some(cached) = local_context_pack_cache_get(&cache_key, &scope_signature)? {
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
            return prepared_from_cached_payload(
                CacheHydrationContext {
                    project: &project,
                    namespace_id: namespace.namespace_id,
                    effective_mode: &effective_mode,
                    scope_signature,
                    cache_key,
                    resolve_scope_ms,
                    cache_lookup_ms,
                },
                cached,
            );
        }
        if let Some(cached) = edge_cache::get_context_pack_cache_entry(
            &edge_cache_path,
            &cache_key,
            &scope_signature,
        )? {
            local_context_pack_cache_put(&cache_key, &scope_signature, &cached)?;
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
            return prepared_from_cached_payload(
                CacheHydrationContext {
                    project: &project,
                    namespace_id: namespace.namespace_id,
                    effective_mode: &effective_mode,
                    scope_signature,
                    cache_key,
                    resolve_scope_ms,
                    cache_lookup_ms,
                },
                cached,
            );
        }
    }
    let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

    let exact_started = Instant::now();
    let mut documents = Vec::new();
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
    }
    let exact_lookup_ms = exact_started.elapsed().as_millis();

    let symbol_started = Instant::now();
    let mut symbols = Vec::new();
    for visible in &visible_projects {
        symbols.extend(
            postgres::search_symbols_for_project(
                db,
                visible.project.project_id,
                &args.query,
                args.limit_symbols as i64,
            )
            .await?,
        );
    }
    let symbol_lookup_ms = symbol_started.elapsed().as_millis();

    let lexical_started = Instant::now();
    let mut chunks = Vec::new();
    for visible in &visible_projects {
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
    let lexical_lookup_ms = lexical_started.elapsed().as_millis();

    sort_and_truncate_documents(&mut documents, args.limit_documents);
    sort_and_truncate_symbols(&mut symbols, args.limit_symbols);
    sort_and_truncate_chunks(&mut chunks, args.limit_chunks);

    let (semantic_chunks, semantic_timings) = semantic_chunks(
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
    let serialize_started = Instant::now();
    let stats = ContextPackStats {
        context_pack_id,
        exact_documents: documents.len(),
        symbol_hits: symbols.len(),
        lexical_chunks: chunks.len(),
        semantic_chunks: semantic_chunks.len(),
        cache_hit: false,
        scope_signature: scope_signature.clone(),
        timings: ContextPackTimings {
            resolve_scope_ms,
            cache_lookup_ms,
            exact_lookup_ms,
            symbol_lookup_ms,
            lexical_lookup_ms,
            query_embed_ms: semantic_timings.query_embed_ms,
            semantic_search_ms: semantic_timings.search_ms,
            semantic_hydrate_ms: semantic_timings.hydrate_ms,
            serialize_ms: 0,
            persist_ms: 0,
        },
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
    let mut stats = stats;
    stats.timings.serialize_ms = serialize_started.elapsed().as_millis();

    Ok(PreparedContextPack {
        context_pack_id,
        project,
        namespace_id: namespace.namespace_id,
        effective_mode,
        visible_projects_json,
        payload,
        payload_json,
        stats,
        cache_key,
        scope_signature,
        cache_hit: false,
        durably_persisted: false,
    })
}

async fn ensure_context_pack_persisted(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    prepared: &mut PreparedContextPack,
) -> Result<()> {
    if prepared.durably_persisted {
        return Ok(());
    }
    let persist_started = Instant::now();
    persist_context_pack(cfg, db, args, prepared).await?;
    prepared.durably_persisted = true;
    prepared.stats.timings.persist_ms = persist_started.elapsed().as_millis();
    Ok(())
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

fn cache_context_pack_entry(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    prepared: &PreparedContextPack,
    durably_persisted: bool,
) -> Result<()> {
    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;
    edge_cache::cache_context_pack(
        &edge_cache_path,
        &prepared.context_pack_id.to_string(),
        &prepared.project.code,
        &prepared.effective_mode,
        &prepared.payload_json,
    )?;
    edge_cache::upsert_context_pack_cache_entry(
        &edge_cache_path,
        &edge_cache::ContextPackCacheRecord {
            cache_key: &prepared.cache_key,
            scope_signature: &prepared.scope_signature,
            context_pack_id: &prepared.context_pack_id.to_string(),
            project_code: &prepared.project.code,
            namespace_code: &args.namespace,
            retrieval_mode: &prepared.effective_mode,
            payload_json: &prepared.payload_json,
            durably_persisted,
        },
    )?;
    local_context_pack_cache_put(
        &prepared.cache_key,
        &prepared.scope_signature,
        &edge_cache::CachedContextPackEntry {
            context_pack_id: prepared.context_pack_id.to_string(),
            payload_json: prepared.payload_json.clone(),
            durably_persisted,
        },
    )?;
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
) -> Result<(Vec<Value>, SemanticTimings)> {
    if limit == 0 {
        return Ok((Vec::new(), SemanticTimings::default()));
    }

    let (vector, query_embed_ms) = embed_query(cfg, query)?;
    if vector.len() as u64 != cfg.qdrant_code_dim {
        return Err(anyhow!(
            "query embedding size mismatch: expected {}, got {}",
            cfg.qdrant_code_dim,
            vector.len()
        ));
    }

    let search_started = Instant::now();
    let qdrant_client = qdrant::connect(cfg)?;
    let per_project_limit = limit.max(1);
    let mut points = Vec::new();
    for visible in visible_projects {
        let result = qdrant::search_project_points(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            vector.clone(),
            &visible.project.code,
            per_project_limit,
        )
        .await?;
        points.extend(result);
    }
    let search_ms = search_started.elapsed().as_millis();

    let hydrate_started = Instant::now();
    let mut scored_points = Vec::new();
    let mut point_ids = Vec::new();
    for point in points {
        let point_id = match point.id.and_then(|id| id.point_id_options) {
            Some(PointIdOptions::Uuid(value)) => Uuid::parse_str(&value).ok(),
            _ => None,
        };
        let Some(point_id) = point_id else {
            continue;
        };
        point_ids.push(point_id);
        scored_points.push((point_id, point.score));
    }

    let hydrated = postgres::list_chunks_by_qdrant_point_ids(db, &point_ids).await?;
    let hydrated_by_point = hydrated.into_iter().collect::<HashMap<_, _>>();
    let mut hits = Vec::new();
    for (point_id, score) in scored_points {
        let Some(mut chunk) = hydrated_by_point.get(&point_id).cloned() else {
            continue;
        };
        chunk.score = score;
        hits.push(semantic_chunk_to_json(&chunk));
    }
    let hydrate_ms = hydrate_started.elapsed().as_millis();

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
    Ok((
        hits,
        SemanticTimings {
            query_embed_ms,
            search_ms,
            hydrate_ms,
        },
    ))
}

fn semantic_chunk_to_json(chunk: &ChunkHit) -> Value {
    json!({
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
    })
}

fn build_query_embedder(cfg: &AppConfig) -> Result<TextEmbedding> {
    let model = match cfg.code_embed_model.as_str() {
        "jina_base_code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        "multilingual_e5_small" => EmbeddingModel::MultilingualE5Small,
        "multilingual_e5_base" => EmbeddingModel::MultilingualE5Base,
        other => return Err(anyhow!("unsupported code embedding model: {other}")),
    };
    TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(false))
}

fn embed_query(cfg: &AppConfig, query: &str) -> Result<(Vec<f32>, u128)> {
    let started = Instant::now();
    let cache = QUERY_EMBEDDER.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("query embedder cache lock poisoned"))?;
    let needs_rebuild = guard
        .as_ref()
        .map(|cached| cached.model != cfg.code_embed_model)
        .unwrap_or(true);
    if needs_rebuild {
        *guard = Some(CachedQueryEmbedder {
            model: cfg.code_embed_model.clone(),
            embedder: build_query_embedder(cfg)?,
        });
    }
    let cached = guard
        .as_mut()
        .ok_or_else(|| anyhow!("query embedder cache unexpectedly empty"))?;
    let embeddings = cached.embedder.embed(&[query.to_string()], Some(1))?;
    let vector = embeddings
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("semantic embedder returned no query vector"))?;
    Ok((vector, started.elapsed().as_millis()))
}

fn scope_signature(visible_projects: &[VisibleProjectRecord]) -> String {
    visible_projects
        .iter()
        .map(|visible| {
            format!(
                "{}:{}:{}:{}:{}",
                visible.project.code,
                visible.project.updated_at,
                visible.relation_type,
                visible.shared_contour,
                visible.access_mode
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn cache_key(cfg: &AppConfig, args: &ContextPackArgs, effective_mode: &str) -> String {
    format!(
        "{}::{}::{}::{}::{}::{}::{}::{}::{}",
        cfg.stack_name,
        args.project,
        args.namespace,
        effective_mode,
        args.limit_documents,
        args.limit_symbols,
        args.limit_chunks,
        args.limit_semantic_chunks,
        hex_sha256(args.query.as_bytes())
    )
}

fn prepared_from_cached_payload(
    context: CacheHydrationContext<'_>,
    cached: edge_cache::CachedContextPackEntry,
) -> Result<PreparedContextPack> {
    let payload: Value = serde_json::from_str(&cached.payload_json)
        .context("failed to decode cached context pack payload")?;
    let context_pack_id =
        Uuid::parse_str(&cached.context_pack_id).context("cached context pack id is not a UUID")?;
    let stats = ContextPackStats {
        context_pack_id,
        exact_documents: payload["retrieval"]["exact_documents"]
            .as_array()
            .map_or(0, Vec::len),
        symbol_hits: payload["retrieval"]["symbol_hits"]
            .as_array()
            .map_or(0, Vec::len),
        lexical_chunks: payload["retrieval"]["lexical_chunks"]
            .as_array()
            .map_or(0, Vec::len),
        semantic_chunks: payload["retrieval"]["semantic_chunks"]
            .as_array()
            .map_or(0, Vec::len),
        cache_hit: true,
        scope_signature: context.scope_signature.clone(),
        timings: ContextPackTimings {
            resolve_scope_ms: context.resolve_scope_ms,
            cache_lookup_ms: context.cache_lookup_ms,
            exact_lookup_ms: 0,
            symbol_lookup_ms: 0,
            lexical_lookup_ms: 0,
            query_embed_ms: 0,
            semantic_search_ms: 0,
            semantic_hydrate_ms: 0,
            serialize_ms: 0,
            persist_ms: 0,
        },
    };

    Ok(PreparedContextPack {
        context_pack_id,
        project: context.project.clone(),
        namespace_id: context.namespace_id,
        effective_mode: context.effective_mode.to_string(),
        visible_projects_json: payload["visible_projects"].clone(),
        payload,
        payload_json: cached.payload_json,
        stats,
        cache_key: context.cache_key,
        scope_signature: context.scope_signature,
        cache_hit: true,
        durably_persisted: cached.durably_persisted,
    })
}

fn local_context_pack_cache_get(
    cache_key: &str,
    scope_signature: &str,
) -> Result<Option<edge_cache::CachedContextPackEntry>> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = cache
        .lock()
        .map_err(|_| anyhow!("local context-pack cache lock poisoned"))?;
    let key = format!("{cache_key}::{scope_signature}");
    Ok(guard.get(&key).cloned())
}

fn local_context_pack_cache_put(
    cache_key: &str,
    scope_signature: &str,
    entry: &edge_cache::CachedContextPackEntry,
) -> Result<()> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("local context-pack cache lock poisoned"))?;
    let key = format!("{cache_key}::{scope_signature}");
    guard.insert(key, entry.clone());
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
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
