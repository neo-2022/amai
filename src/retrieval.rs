use crate::cli::ContextPackArgs;
use crate::config::AppConfig;
use crate::edge_cache;
use crate::postgres::{
    self, ChunkHit, DocumentHit, ProjectRecord, SymbolHit, VisibleProjectRecord,
};
use crate::qdrant;
use crate::s3;
use crate::token_budget;
use crate::working_state;
use anyhow::{Context, Result, anyhow};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use qdrant_client::qdrant::point_id::PointIdOptions;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
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
pub struct ContextPackResult {
    pub payload: Value,
    pub stats: ContextPackStats,
}

#[derive(Debug, Clone)]
struct PreparedContextPack {
    context_pack_id: Uuid,
    project: ProjectRecord,
    namespace_id: Uuid,
    effective_mode: String,
    visible_projects_json: Value,
    payload: Arc<Value>,
    payload_json: Arc<str>,
    stats: ContextPackStats,
    cache_key: String,
    scope_signature: String,
    cache_hit: bool,
    durably_persisted: bool,
}

#[derive(Debug, Clone)]
struct ResolvedVisibleScope {
    visible: VisibleProjectRecord,
    namespace: postgres::NamespaceRecord,
}

#[derive(Debug, Clone)]
struct SemanticGuardSummary {
    query_terms: Vec<String>,
    lexical_signal_count: usize,
    accepted_hits: usize,
    rejected_hits: usize,
    abstained: bool,
    reason: Option<&'static str>,
}

struct CachedQueryEmbedder {
    model: String,
    embedder: TextEmbedding,
}

#[derive(Debug, Clone)]
struct LocalContextPackEntry {
    context_pack_id: Uuid,
    payload: Arc<Value>,
    exact_documents: usize,
    symbol_hits: usize,
    lexical_chunks: usize,
    semantic_chunks: usize,
    durably_persisted: bool,
    cached_at_epoch_ms: u128,
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
static LOCAL_CONTEXT_PACK_CACHE: OnceLock<Mutex<HashMap<String, LocalContextPackEntry>>> =
    OnceLock::new();
static LOCAL_FAST_CONTEXT_PACK_CACHE: OnceLock<Mutex<HashMap<String, LocalContextPackEntry>>> =
    OnceLock::new();

pub async fn build_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<()> {
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    cache_context_pack_entry(cfg, args, &prepared, true)?;
    record_context_pack_token_budget_event(db, prepared.payload.as_ref()).await?;
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
    Ok(
        execute_context_pack_capture_with_options(cfg, db, args, persist, true)
            .await?
            .stats,
    )
}

pub async fn execute_context_pack_capture(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<ContextPackResult> {
    execute_context_pack_capture_with_options(cfg, db, args, persist, true).await
}

pub async fn execute_context_pack_with_options(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
    track_token_usage: bool,
) -> Result<ContextPackStats> {
    Ok(
        execute_context_pack_capture_with_options(cfg, db, args, persist, track_token_usage)
            .await?
            .stats,
    )
}

pub async fn execute_context_pack_capture_with_options(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
    track_token_usage: bool,
) -> Result<ContextPackResult> {
    if let Some(cached) = try_execute_context_pack_fast_cached(cfg, args, persist)? {
        if track_token_usage {
            record_context_pack_token_budget_event(db, &cached.payload).await?;
        }
        return Ok(cached);
    }
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    if persist {
        ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    }
    let durably_persisted = persist || prepared.durably_persisted;
    cache_context_pack_entry(cfg, args, &prepared, durably_persisted)?;
    if track_token_usage {
        record_context_pack_token_budget_event(db, prepared.payload.as_ref()).await?;
    }
    Ok(ContextPackResult {
        payload: prepared.payload.as_ref().clone(),
        stats: prepared.stats,
    })
}

async fn record_context_pack_token_budget_event(db: &Client, payload: &Value) -> Result<()> {
    if !payload.is_object() {
        return Ok(());
    }
    token_budget::record_live_context_pack_event(db, payload).await?;
    working_state::record_context_pack_event(db, payload).await
}

pub fn try_execute_context_pack_fast_cached(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackResult>> {
    if args.disable_cache {
        return Ok(None);
    }
    let Some(effective_mode) = args.retrieval_mode.as_deref() else {
        return Ok(None);
    };
    let cache_key = cache_key(cfg, args, effective_mode);
    let Some(cached) = local_fast_context_pack_cache_get(&cache_key, cfg.local_fast_cache_ttl_ms)?
    else {
        return Ok(None);
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(context_pack_result_from_local_entry(cached)))
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
    let visible_scopes = resolve_visible_scopes(db, &visible_projects, &namespace.code).await?;
    let resolve_scope_ms = resolve_started.elapsed().as_millis();
    let scope_signature = scope_signature(&visible_scopes);
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
            let local_cached = local_entry_from_edge(cached)?;
            local_context_pack_cache_put(&cache_key, &scope_signature, &local_cached)?;
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
                local_cached,
            );
        }
    }
    let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

    let exact_started = Instant::now();
    let mut documents = Vec::new();
    for scope in &visible_scopes {
        documents.extend(
            postgres::search_documents_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                args.limit_documents as i64,
            )
            .await?,
        );
    }
    let exact_lookup_ms = exact_started.elapsed().as_millis();

    let symbol_started = Instant::now();
    let mut symbols = Vec::new();
    for scope in &visible_scopes {
        symbols.extend(
            postgres::search_symbols_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                args.limit_symbols as i64,
            )
            .await?,
        );
    }
    let symbol_lookup_ms = symbol_started.elapsed().as_millis();

    let lexical_started = Instant::now();
    let mut chunks = Vec::new();
    for scope in &visible_scopes {
        chunks.extend(
            postgres::search_chunks_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
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

    let lexical_signal_count = documents.len() + symbols.len() + chunks.len();
    let (semantic_chunks, semantic_timings, semantic_guard) = semantic_chunks(
        cfg,
        db,
        &visible_scopes,
        &args.query,
        args.limit_semantic_chunks,
        lexical_signal_count,
        &chunks,
    )
    .await?;
    let visible_projects_json = json!(
        visible_scopes
            .iter()
            .map(|scope| {
                json!({
                    "project_code": scope.visible.project.code,
                    "display_name": scope.visible.project.display_name,
                    "repo_root": scope.visible.project.repo_root,
                    "relation_type": scope.visible.relation_type,
                    "shared_contour": scope.visible.shared_contour,
                    "access_mode": scope.visible.access_mode,
                    "namespace_code": scope.namespace.code,
                    "namespace_display_name": scope.namespace.display_name
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
        "quality": {
            "semantic_guard": semantic_guard_to_json(&semantic_guard)
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
        ],
        "retrieval_runtime": runtime_json(&stats)
    });
    let payload_json: Arc<str> = Arc::from(serde_json::to_string(&payload)?);
    let mut stats = stats;
    stats.timings.serialize_ms = serialize_started.elapsed().as_millis();

    Ok(PreparedContextPack {
        context_pack_id,
        project,
        namespace_id: namespace.namespace_id,
        effective_mode,
        visible_projects_json,
        payload: Arc::new(payload),
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
        prepared.payload_json.as_ref(),
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
        prepared.payload_json.as_ref(),
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
            payload: prepared.payload.as_ref(),
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
            payload_json: prepared.payload_json.as_ref(),
            durably_persisted,
        },
    )?;
    local_context_pack_cache_put(
        &prepared.cache_key,
        &prepared.scope_signature,
        &local_entry_from_prepared(prepared, durably_persisted),
    )?;
    local_fast_context_pack_cache_put(
        &prepared.cache_key,
        &local_entry_from_prepared(prepared, durably_persisted),
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

async fn resolve_visible_scopes(
    db: &Client,
    visible_projects: &[VisibleProjectRecord],
    namespace_code: &str,
) -> Result<Vec<ResolvedVisibleScope>> {
    let mut scopes = Vec::new();
    for visible in visible_projects {
        let Some(namespace) =
            postgres::find_namespace_by_code(db, visible.project.project_id, namespace_code)
                .await?
        else {
            continue;
        };
        scopes.push(ResolvedVisibleScope {
            visible: visible.clone(),
            namespace,
        });
    }
    Ok(scopes)
}

async fn semantic_chunks(
    cfg: &AppConfig,
    db: &Client,
    visible_scopes: &[ResolvedVisibleScope],
    query: &str,
    limit: usize,
    lexical_signal_count: usize,
    lexical_fallback_chunks: &[ChunkHit],
) -> Result<(Vec<Value>, SemanticTimings, SemanticGuardSummary)> {
    if limit == 0 {
        return Ok((
            Vec::new(),
            SemanticTimings::default(),
            SemanticGuardSummary {
                query_terms: Vec::new(),
                lexical_signal_count,
                accepted_hits: 0,
                rejected_hits: 0,
                abstained: false,
                reason: None,
            },
        ));
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
    for scope in visible_scopes {
        let result = qdrant::search_namespace_points(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            vector.clone(),
            &scope.visible.project.code,
            &scope.namespace.code,
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
    if hits.is_empty() && !lexical_fallback_chunks.is_empty() {
        let exact_matches = lexical_fallback_chunks
            .iter()
            .filter(|chunk| chunk.content.contains(query))
            .collect::<Vec<_>>();
        let query_lower = query.to_lowercase();
        let case_folded_matches = lexical_fallback_chunks
            .iter()
            .filter(|chunk| chunk.content.to_lowercase().contains(&query_lower))
            .collect::<Vec<_>>();
        let fallback_hits = if !exact_matches.is_empty() {
            exact_matches
        } else if !case_folded_matches.is_empty() {
            case_folded_matches
        } else {
            lexical_fallback_chunks.iter().collect::<Vec<_>>()
        };
        return Ok((
            fallback_hits
                .iter()
                .take(limit)
                .map(|chunk| semantic_chunk_fallback_to_json(chunk))
                .collect(),
            SemanticTimings {
                query_embed_ms,
                search_ms,
                hydrate_ms,
            },
            SemanticGuardSummary {
                query_terms: query_terms(query),
                lexical_signal_count,
                accepted_hits: fallback_hits.len().min(limit),
                rejected_hits: 0,
                abstained: false,
                reason: None,
            },
        ));
    }
    let semantic_guard = apply_semantic_relevance_guard(query, lexical_signal_count, hits);
    let mut guarded_hits = semantic_guard.0;
    guarded_hits.truncate(limit);
    Ok((
        guarded_hits,
        SemanticTimings {
            query_embed_ms,
            search_ms,
            hydrate_ms,
        },
        semantic_guard.1,
    ))
}

fn semantic_chunk_to_json(chunk: &ChunkHit) -> Value {
    json!({
        "score": chunk.score,
        "retrieval_strategy": "vector_search",
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

fn semantic_chunk_fallback_to_json(chunk: &ChunkHit) -> Value {
    json!({
        "score": chunk.score,
        "retrieval_strategy": "lexical_fallback",
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

fn apply_semantic_relevance_guard(
    query: &str,
    lexical_signal_count: usize,
    hits: Vec<Value>,
) -> (Vec<Value>, SemanticGuardSummary) {
    let query_terms = query_terms(query);
    let require_overlap = lexical_signal_count == 0 && !query_terms.is_empty();
    let mut accepted_hits = Vec::new();
    let mut rejected_hits = 0usize;

    for hit in hits {
        if !require_overlap || semantic_hit_has_query_overlap(&hit, &query_terms) {
            accepted_hits.push(hit);
        } else {
            rejected_hits += 1;
        }
    }

    let accepted_count = accepted_hits.len();
    let abstained = require_overlap && accepted_count == 0 && rejected_hits > 0;
    (
        accepted_hits,
        SemanticGuardSummary {
            query_terms,
            lexical_signal_count,
            accepted_hits: accepted_count,
            rejected_hits,
            abstained,
            reason: if abstained {
                Some("semantic_hits_missing_query_overlap")
            } else {
                None
            },
        },
    )
}

fn semantic_hit_has_query_overlap(hit: &Value, query_terms: &[String]) -> bool {
    if query_terms.is_empty() {
        return true;
    }

    let relative_path = hit["relative_path"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let content = hit["content"].as_str().unwrap_or_default().to_lowercase();

    query_terms
        .iter()
        .any(|term| relative_path.contains(term) || content.contains(term))
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let mut current = String::new();

    let push_current =
        |current: &mut String, terms: &mut Vec<String>, seen: &mut HashSet<String>| {
            if current.chars().count() >= 3 {
                let lowered = current.to_lowercase();
                if seen.insert(lowered.clone()) {
                    terms.push(lowered);
                }
            }
            current.clear();
        };

    for ch in query.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            push_current(&mut current, &mut terms, &mut seen);
        }
    }
    if !current.is_empty() {
        push_current(&mut current, &mut terms, &mut seen);
    }

    terms
}

fn semantic_guard_to_json(guard: &SemanticGuardSummary) -> Value {
    json!({
        "query_terms": guard.query_terms,
        "lexical_signal_count": guard.lexical_signal_count,
        "accepted_hits": guard.accepted_hits,
        "rejected_hits": guard.rejected_hits,
        "abstained": guard.abstained,
        "reason": guard.reason
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

fn scope_signature(visible_scopes: &[ResolvedVisibleScope]) -> String {
    visible_scopes
        .iter()
        .map(|scope| {
            format!(
                "{}:{}:{}:{}:{}:{}:{}",
                scope.visible.project.code,
                scope.visible.project.updated_at,
                scope.visible.relation_type,
                scope.visible.shared_contour,
                scope.visible.access_mode,
                scope.namespace.namespace_id,
                scope.namespace.code
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
    cached: LocalContextPackEntry,
) -> Result<PreparedContextPack> {
    let stats = ContextPackStats {
        context_pack_id: cached.context_pack_id,
        exact_documents: cached.exact_documents,
        symbol_hits: cached.symbol_hits,
        lexical_chunks: cached.lexical_chunks,
        semantic_chunks: cached.semantic_chunks,
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
    let mut payload = cached.payload.as_ref().clone();
    payload["retrieval_runtime"] = runtime_json(&stats);
    let payload_json: Arc<str> = Arc::from(serde_json::to_string(&payload)?);

    Ok(PreparedContextPack {
        context_pack_id: cached.context_pack_id,
        project: context.project.clone(),
        namespace_id: context.namespace_id,
        effective_mode: context.effective_mode.to_string(),
        visible_projects_json: payload["visible_projects"].clone(),
        payload: Arc::new(payload),
        payload_json,
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
) -> Result<Option<LocalContextPackEntry>> {
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
    entry: &LocalContextPackEntry,
) -> Result<()> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("local context-pack cache lock poisoned"))?;
    let key = format!("{cache_key}::{scope_signature}");
    guard.insert(key, entry.clone());
    Ok(())
}

fn local_entry_from_prepared(
    prepared: &PreparedContextPack,
    durably_persisted: bool,
) -> LocalContextPackEntry {
    LocalContextPackEntry {
        context_pack_id: prepared.context_pack_id,
        payload: Arc::clone(&prepared.payload),
        exact_documents: prepared.stats.exact_documents,
        symbol_hits: prepared.stats.symbol_hits,
        lexical_chunks: prepared.stats.lexical_chunks,
        semantic_chunks: prepared.stats.semantic_chunks,
        durably_persisted,
        cached_at_epoch_ms: now_epoch_ms(),
    }
}

fn local_entry_from_edge(
    cached: edge_cache::CachedContextPackEntry,
) -> Result<LocalContextPackEntry> {
    let payload: Value = serde_json::from_str(&cached.payload_json)
        .context("failed to decode cached context pack payload")?;
    let context_pack_id =
        Uuid::parse_str(&cached.context_pack_id).context("cached context pack id is not a UUID")?;
    Ok(LocalContextPackEntry {
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
        payload: Arc::new(payload),
        durably_persisted: cached.durably_persisted,
        cached_at_epoch_ms: now_epoch_ms(),
    })
}

fn context_pack_result_from_local_entry(cached: LocalContextPackEntry) -> ContextPackResult {
    let stats = ContextPackStats {
        context_pack_id: cached.context_pack_id,
        exact_documents: cached.exact_documents,
        symbol_hits: cached.symbol_hits,
        lexical_chunks: cached.lexical_chunks,
        semantic_chunks: cached.semantic_chunks,
        cache_hit: true,
        scope_signature: "local_fast_cache".to_string(),
        timings: ContextPackTimings {
            resolve_scope_ms: 0,
            cache_lookup_ms: 0,
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
    let mut payload = cached.payload.as_ref().clone();
    payload["retrieval_runtime"] = runtime_json(&stats);
    ContextPackResult { payload, stats }
}

fn runtime_json(stats: &ContextPackStats) -> Value {
    let timings = &stats.timings;
    let total_ms = timings.resolve_scope_ms
        + timings.cache_lookup_ms
        + timings.exact_lookup_ms
        + timings.symbol_lookup_ms
        + timings.lexical_lookup_ms
        + timings.query_embed_ms
        + timings.semantic_search_ms
        + timings.semantic_hydrate_ms
        + timings.serialize_ms
        + timings.persist_ms;
    json!({
        "cache_hit": stats.cache_hit,
        "resolve_scope_ms": timings.resolve_scope_ms as f64,
        "cache_lookup_ms": timings.cache_lookup_ms as f64,
        "exact_lookup_ms": timings.exact_lookup_ms as f64,
        "symbol_lookup_ms": timings.symbol_lookup_ms as f64,
        "lexical_lookup_ms": timings.lexical_lookup_ms as f64,
        "query_embed_ms": timings.query_embed_ms as f64,
        "semantic_search_ms": timings.semantic_search_ms as f64,
        "semantic_hydrate_ms": timings.semantic_hydrate_ms as f64,
        "serialize_ms": timings.serialize_ms as f64,
        "persist_ms": timings.persist_ms as f64,
        "total_ms": total_ms as f64
    })
}

fn local_fast_context_pack_cache_get(
    cache_key: &str,
    ttl_ms: u128,
) -> Result<Option<LocalContextPackEntry>> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
    let Some(entry) = guard.get(cache_key).cloned() else {
        return Ok(None);
    };
    if now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms {
        guard.remove(cache_key);
        return Ok(None);
    }
    Ok(Some(entry))
}

fn local_fast_context_pack_cache_put(cache_key: &str, entry: &LocalContextPackEntry) -> Result<()> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
    guard.insert(cache_key.to_string(), entry.clone());
    Ok(())
}

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
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

#[cfg(test)]
mod tests {
    use super::{apply_semantic_relevance_guard, query_terms, semantic_hit_has_query_overlap};
    use serde_json::json;

    #[test]
    fn query_terms_extracts_unique_alphanumeric_tokens() {
        assert_eq!(
            query_terms("Amai onboarding MCP VS Code integration"),
            vec![
                "amai".to_string(),
                "onboarding".to_string(),
                "mcp".to_string(),
                "code".to_string(),
                "integration".to_string()
            ]
        );
    }

    #[test]
    fn semantic_guard_abstains_without_lexical_signals_and_overlap() {
        let hit = json!({
            "relative_path": "packages/art-i18n/src/lib.rs",
            "content": "m.insert(\"shell.title\", \"Art Console\");"
        });
        let (accepted, summary) =
            apply_semantic_relevance_guard("Amai onboarding MCP VS Code integration", 0, vec![hit]);
        assert!(accepted.is_empty());
        assert!(summary.abstained);
        assert_eq!(summary.rejected_hits, 1);
        assert_eq!(summary.reason, Some("semantic_hits_missing_query_overlap"));
    }

    #[test]
    fn semantic_guard_keeps_hits_with_query_overlap() {
        let hit = json!({
            "relative_path": "docs/MCP_INTEGRATION.md",
            "content": "VS Code MCP onboarding guide"
        });
        assert!(semantic_hit_has_query_overlap(
            &hit,
            &query_terms("Amai onboarding MCP VS Code integration")
        ));
        let (accepted, summary) =
            apply_semantic_relevance_guard("Amai onboarding MCP VS Code integration", 0, vec![hit]);
        assert_eq!(accepted.len(), 1);
        assert!(!summary.abstained);
        assert_eq!(summary.accepted_hits, 1);
    }
}
