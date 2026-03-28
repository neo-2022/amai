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
use crate::workspace_graph;
use anyhow::{Context, Result, anyhow};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use qdrant_client::qdrant::point_id::PointIdOptions;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
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
    pub ranking_ms: u128,
    pub provenance_ms: u128,
    pub pack_assembly_ms: u128,
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
pub struct FastContextPackProbe {
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
    require_persist: bool,
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
    detail: Option<String>,
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

type FastCacheKey = u128;

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
static LOCAL_CONTEXT_PACK_CACHE: OnceLock<RwLock<HashMap<String, LocalContextPackEntry>>> =
    OnceLock::new();
static LOCAL_FAST_CONTEXT_PACK_CACHE: OnceLock<
    RwLock<HashMap<FastCacheKey, LocalContextPackEntry>>,
> = OnceLock::new();

pub async fn build_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<()> {
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    cache_context_pack_entry(cfg, args, &prepared, true)?;
    record_context_pack_token_budget_event(db, prepared.payload.as_ref(), args).await?;
    let compact_output = model_visible_context_pack_payload(prepared.payload.as_ref());
    let compact_output_json = serde_json::to_string(&compact_output)?;
    let _ = token_budget::observe_cli_context_pack_tool_overhead(
        db,
        &prepared.context_pack_id.to_string(),
        compact_output_json.as_str(),
    )
    .await?;
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
    println!("{compact_output_json}");
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
    if !track_token_usage
        && let Some(stats) = try_execute_context_pack_stats_cached(cfg, db, args, persist).await?
    {
        return Ok(stats);
    }
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
            record_context_pack_token_budget_event(db, &cached.payload, args).await?;
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
        record_context_pack_token_budget_event(db, prepared.payload.as_ref(), args).await?;
    }
    Ok(ContextPackResult {
        payload: prepared.payload.as_ref().clone(),
        stats: prepared.stats,
    })
}

async fn record_context_pack_token_budget_event(
    db: &Client,
    payload: &Value,
    args: &ContextPackArgs,
) -> Result<()> {
    if !payload.is_object() {
        return Ok(());
    }
    let token_budget_payload = with_whole_cycle_observed_overrides(payload, args);
    token_budget::record_context_pack_event(db, &token_budget_payload, &args.token_source_kind)
        .await?;
    working_state::record_context_pack_event(db, payload).await
}

fn with_whole_cycle_observed_overrides(payload: &Value, args: &ContextPackArgs) -> Value {
    if args.client_prompt_tokens.is_none()
        && args.assistant_generation_tokens.is_none()
        && args.tool_overhead_tokens.is_none()
        && args.continuity_restore_tokens.is_none()
    {
        return payload.clone();
    }
    let mut augmented = payload.clone();
    let Some(root) = augmented.as_object_mut() else {
        return payload.clone();
    };
    let whole_cycle = root
        .entry("whole_cycle_observed".to_string())
        .or_insert_with(|| json!({}));
    if !whole_cycle.is_object() {
        *whole_cycle = json!({});
    }
    let Some(whole_cycle_object) = whole_cycle.as_object_mut() else {
        return payload.clone();
    };
    if let Some(tokens) = args.client_prompt_tokens {
        whole_cycle_object.insert("client_prompt_tokens".to_string(), Value::from(tokens));
    }
    if let Some(tokens) = args.assistant_generation_tokens {
        whole_cycle_object.insert(
            "assistant_generation_tokens".to_string(),
            Value::from(tokens),
        );
    }
    if let Some(tokens) = args.tool_overhead_tokens {
        whole_cycle_object.insert("tool_overhead_tokens".to_string(), Value::from(tokens));
    }
    if let Some(tokens) = args.continuity_restore_tokens {
        whole_cycle_object.insert("continuity_restore_tokens".to_string(), Value::from(tokens));
    }
    augmented
}

pub(crate) fn model_visible_context_pack_payload(payload: &Value) -> Value {
    let (lexical_chunks, semantic_chunks) = compact_retrieval_chunks(&payload["retrieval"]);
    json!({
        "context_pack_id": payload["context_pack_id"].clone(),
        "project": {
            "code": payload["project"]["code"].clone(),
        },
        "namespace": {
            "code": payload["namespace"]["code"].clone(),
        },
        "query": payload["query"].clone(),
        "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
        "visible_projects": compact_visible_projects(&payload["visible_projects"]),
        "decision_trace": compact_decision_trace(&payload["decision_trace"]),
        "retrieval": {
            "exact_documents": compact_exact_documents(&payload["retrieval"]["exact_documents"]),
            "symbol_hits": compact_symbol_hits(&payload["retrieval"]["symbol_hits"]),
            "lexical_chunks": lexical_chunks,
            "semantic_chunks": semantic_chunks,
        }
    })
}

fn compact_visible_projects(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "project_code": item["project_code"].clone(),
            })
        })
        .collect()
}

fn compact_decision_trace(value: &Value) -> Value {
    json!({
        "scope": {
            "effective_retrieval_mode": value["scope"]["effective_retrieval_mode"].clone(),
            "project_code": value["scope"]["project_code"].clone(),
            "namespace_code": value["scope"]["namespace_code"].clone(),
            "visible_projects_total": value["scope"]["visible_projects_total"].clone(),
        },
        "selection_priority": value["selection_priority"].clone(),
        "included": compact_decision_trace_items(&value["included"]),
        "not_included": compact_decision_trace_items(&value["not_included"]),
        "semantic_guard": compact_semantic_guard(&value["semantic_guard"]),
    })
}

fn compact_decision_trace_items(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "strategy": item["strategy"].clone(),
                "count": item["count"].clone(),
            })
        })
        .collect()
}

fn compact_semantic_guard(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "abstained": value["abstained"].clone(),
        "accepted_hits": value["accepted_hits"].clone(),
        "rejected_hits": value["rejected_hits"].clone(),
        "lexical_signal_count": value["lexical_signal_count"].clone(),
        "query_terms": value["query_terms"].clone(),
        "reason": value["reason"].clone(),
    })
}

fn compact_exact_documents(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "project_code": item["project_code"].clone(),
                "relative_path": item["relative_path"].clone(),
                "snippet": item["snippet"].clone(),
                "source_kind": item["source_kind"].clone(),
            })
        })
        .collect()
}

fn compact_retrieval_chunks(retrieval: &Value) -> (Vec<Value>, Vec<Value>) {
    let mut seen_signatures = HashSet::new();
    let lexical_chunks = compact_chunks(&retrieval["lexical_chunks"], &mut seen_signatures);
    let semantic_chunks = compact_chunks(&retrieval["semantic_chunks"], &mut seen_signatures);
    (lexical_chunks, semantic_chunks)
}

fn compact_symbol_hits(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "project_code": item["project_code"].clone(),
                "relative_path": item["relative_path"].clone(),
                "name": item["name"].clone(),
                "kind": item["kind"].clone(),
                "provenance": {
                    "source_project": item["provenance"]["source_project"].clone(),
                }
            })
        })
        .collect()
}

fn compact_chunks(
    value: &Value,
    seen_signatures: &mut HashSet<String>,
) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let signature = compact_chunk_signature(item)?;
            if !seen_signatures.insert(signature) {
                return None;
            }
            Some(item)
        })
        .map(|item| {
            json!({
                "project_code": item["project_code"].clone(),
                "relative_path": item["relative_path"].clone(),
                "content": item["content"].clone(),
                "provenance": {
                    "source_project": item["provenance"]["source_project"].clone(),
                }
            })
        })
        .collect()
}

fn compact_chunk_signature(item: &Value) -> Option<String> {
    let project_code = item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .unwrap_or_default();
    let relative_path = item["relative_path"].as_str().unwrap_or_default();
    let content = item["content"].as_str().unwrap_or_default();
    if project_code.is_empty() && relative_path.is_empty() && content.is_empty() {
        return None;
    }
    Some(format!("{project_code}\u{1f}{relative_path}\u{1f}{content}"))
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
    let fast_cache_key = fast_cache_key(cfg, args, effective_mode);
    let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    else {
        return Ok(None);
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(context_pack_result_from_local_entry(cached)))
}

pub fn prepare_fast_context_pack_probe(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<FastContextPackProbe>> {
    if args.disable_cache {
        return Ok(None);
    }
    let Some(effective_mode) = args.retrieval_mode.as_deref() else {
        return Ok(None);
    };
    let fast_cache_key = fast_cache_key(cfg, args, effective_mode);
    let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    else {
        return Ok(None);
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(FastContextPackProbe {
        fast_cache_key,
        ttl_ms: cfg.local_fast_cache_ttl_ms,
        require_persist: persist,
        stats: cached_fast_context_pack_stats(&cached),
    }))
}

pub fn fast_context_pack_probe_hit(probe: &FastContextPackProbe) -> Result<bool> {
    local_fast_context_pack_cache_contains(
        probe.fast_cache_key,
        probe.ttl_ms,
        probe.require_persist,
    )
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
    let visible_scopes =
        resolve_visible_scopes(db, &visible_projects, project.project_id, &namespace).await?;
    let resolve_scope_ms = resolve_started.elapsed().as_millis();
    let scope_signature = scope_signature(&visible_scopes);
    let cache_key = cache_key(cfg, args, &effective_mode);
    let edge_cache_path = if args.disable_cache {
        None
    } else {
        Some(edge_cache::ensure(&cfg.edge_cache_path)?)
    };

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
        if let Some(edge_cache_path) = edge_cache_path.as_ref()
            && let Some(cached) = edge_cache::get_context_pack_cache_entry(
                edge_cache_path,
                &cache_key,
                &scope_signature,
            )?
        {
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

    let mut documents = Vec::new();
    let exact_started = Instant::now();
    if args.limit_documents > 0 {
        let should_try_exact = should_try_exact_document_lookup(&args.query);
        for scope in &visible_scopes {
            let mut scope_hits = if should_try_exact {
                postgres::search_documents_exact_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_documents as i64,
                )
                .await?
            } else {
                Vec::new()
            };
            if scope_hits.is_empty() {
                scope_hits = postgres::search_documents_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_documents as i64,
                )
                .await?;
            }
            documents.extend(scope_hits);
        }
    }
    let exact_lookup_ms = exact_started.elapsed().as_millis();

    let mut symbols = Vec::new();
    let symbol_started = Instant::now();
    if args.limit_symbols > 0 {
        let should_try_exact = should_try_exact_symbol_lookup(&args.query);
        for scope in &visible_scopes {
            let mut scope_hits = if should_try_exact {
                postgres::search_symbols_exact_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_symbols as i64,
                )
                .await?
            } else {
                Vec::new()
            };
            if scope_hits.is_empty() {
                scope_hits = postgres::search_symbols_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_symbols as i64,
                )
                .await?;
            }
            symbols.extend(scope_hits);
        }
    }
    let symbol_lookup_ms = symbol_started.elapsed().as_millis();

    let mut chunks = Vec::new();
    let lexical_started = Instant::now();
    if args.limit_chunks > 0 {
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
    }
    let lexical_lookup_ms = lexical_started.elapsed().as_millis();

    let ranking_started = Instant::now();
    sort_and_truncate_documents(&mut documents, args.limit_documents);
    sort_and_truncate_symbols(&mut symbols, args.limit_symbols);
    sort_and_truncate_chunks(&mut chunks, args.limit_chunks);
    let ranking_ms = ranking_started.elapsed().as_millis();

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
    let provenance_started = Instant::now();
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
    let workspace_requests = collect_workspace_document_requests(
        &visible_scopes,
        &documents,
        &symbols,
        &chunks,
        &semantic_chunks,
    );
    let minimal_document_context = should_use_minimal_document_workspace_graph(
        &documents,
        &symbols,
        &chunks,
        &semantic_chunks,
    );
    let minimal_symbol_context =
        should_use_minimal_symbol_workspace_graph(&documents, &symbols, &chunks, &semantic_chunks);
    let (workspace_documents, workspace_symbols) = if minimal_document_context
        || minimal_symbol_context
    {
        (Vec::new(), Vec::new())
    } else {
        (
            postgres::list_document_structures_for_namespace_paths(db, &workspace_requests).await?,
            postgres::list_document_symbols_for_namespace_paths(db, &workspace_requests).await?,
        )
    };
    let workspace_graph = workspace_graph::build_context_pack_workspace_graph(
        &context_pack_id,
        &args.query,
        &effective_mode,
        &scope_signature,
        &visible_projects_json,
        &documents,
        &symbols,
        &chunks,
        &semantic_chunks,
        &workspace_documents,
        &workspace_symbols,
    )?;
    let provenance_minimum = json!([
        "source_project",
        "repo_root",
        "commit_sha",
        "path",
        "symbol",
        "chunk_id",
        "source_kind",
        "trust_level"
    ]);
    let provenance_ms = provenance_started.elapsed().as_millis();
    let generated_epoch_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs();
    let pack_assembly_started = Instant::now();
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
            ranking_ms,
            provenance_ms,
            pack_assembly_ms: 0,
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
        "workspace_graph": workspace_graph,
        "provenance_minimum": provenance_minimum,
        "retrieval_runtime": runtime_json(&stats)
    });
    let mut payload = payload;
    ensure_context_pack_decision_trace(&mut payload);
    let pack_assembly_ms = pack_assembly_started.elapsed().as_millis();
    let payload_json: Arc<str> = Arc::from(serde_json::to_string(&payload)?);
    let mut stats = stats;
    stats.timings.pack_assembly_ms = pack_assembly_ms;
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
    if args.disable_cache {
        return Ok(());
    }
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
    let fast_cache_key = fast_cache_key(cfg, args, &prepared.effective_mode);
    local_fast_context_pack_cache_put(
        fast_cache_key,
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
    if effective_mode == "local_strict" {
        return Ok(visible);
    }
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
    local_project_id: Uuid,
    local_namespace: &postgres::NamespaceRecord,
) -> Result<Vec<ResolvedVisibleScope>> {
    let mut scopes = Vec::new();
    for visible in visible_projects {
        let namespace = if visible.project.project_id == local_project_id {
            local_namespace.clone()
        } else {
            let Some(namespace) = postgres::find_namespace_by_code(
                db,
                visible.project.project_id,
                &local_namespace.code,
            )
            .await?
            else {
                continue;
            };
            namespace
        };
        scopes.push(ResolvedVisibleScope {
            visible: visible.clone(),
            namespace,
        });
    }
    Ok(scopes)
}

fn should_try_exact_document_lookup(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 256
        && (trimmed.contains('/')
            || trimmed.contains('.')
            || trimmed.contains('_')
            || trimmed.contains('-'))
}

fn should_try_exact_symbol_lookup(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty() && trimmed.len() <= 128 && !trimmed.chars().any(char::is_whitespace)
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
                detail: None,
            },
        ));
    }

    let mut any_vectors = false;
    for scope in visible_scopes {
        if postgres::namespace_has_vector_points(
            db,
            scope.visible.project.project_id,
            scope.namespace.namespace_id,
        )
        .await?
        {
            any_vectors = true;
            break;
        }
    }
    if !any_vectors {
        return Ok(semantic_fallback_result(
            query,
            limit,
            lexical_signal_count,
            lexical_fallback_chunks,
            SemanticTimings::default(),
            "no_vector_points_in_scope",
            None,
        ));
    }

    let (vector, query_embed_ms) = match embed_query(cfg, query) {
        Ok(value) => value,
        Err(error) => {
            return Ok(semantic_fallback_result(
                query,
                limit,
                lexical_signal_count,
                lexical_fallback_chunks,
                SemanticTimings::default(),
                "embedding_unavailable",
                Some(error.to_string()),
            ));
        }
    };
    if vector.len() as u64 != cfg.qdrant_code_dim {
        return Ok(semantic_fallback_result(
            query,
            limit,
            lexical_signal_count,
            lexical_fallback_chunks,
            SemanticTimings {
                query_embed_ms,
                ..SemanticTimings::default()
            },
            "embedding_unavailable",
            Some(format!(
                "query embedding size mismatch: expected {}, got {}",
                cfg.qdrant_code_dim,
                vector.len()
            )),
        ));
    }

    let search_started = Instant::now();
    let qdrant_client = match qdrant::connect(cfg) {
        Ok(client) => client,
        Err(error) => {
            return Ok(semantic_fallback_result(
                query,
                limit,
                lexical_signal_count,
                lexical_fallback_chunks,
                SemanticTimings {
                    query_embed_ms,
                    ..SemanticTimings::default()
                },
                "vector_layer_unavailable",
                Some(error.to_string()),
            ));
        }
    };
    let per_project_limit = limit.max(1);
    let mut points = Vec::new();
    for scope in visible_scopes {
        let result = match qdrant::search_namespace_points(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            vector.clone(),
            &scope.visible.project.code,
            &scope.namespace.code,
            per_project_limit,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                return Ok(semantic_fallback_result(
                    query,
                    limit,
                    lexical_signal_count,
                    lexical_fallback_chunks,
                    SemanticTimings {
                        query_embed_ms,
                        search_ms: search_started.elapsed().as_millis(),
                        ..SemanticTimings::default()
                    },
                    "vector_layer_unavailable",
                    Some(error.to_string()),
                ));
            }
        };
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
        let fallback_hits = lexical_fallback_hits(query, lexical_fallback_chunks);
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
                detail: None,
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

fn semantic_fallback_result(
    query: &str,
    limit: usize,
    lexical_signal_count: usize,
    lexical_fallback_chunks: &[ChunkHit],
    timings: SemanticTimings,
    reason: &'static str,
    detail: Option<String>,
) -> (Vec<Value>, SemanticTimings, SemanticGuardSummary) {
    let fallback_hits = lexical_fallback_hits(query, lexical_fallback_chunks);
    let accepted_hits = fallback_hits.len().min(limit);
    let abstained = accepted_hits == 0 && lexical_signal_count == 0;
    (
        fallback_hits
            .iter()
            .take(limit)
            .map(|chunk| semantic_chunk_fallback_to_json(chunk))
            .collect(),
        timings,
        SemanticGuardSummary {
            query_terms: query_terms(query),
            lexical_signal_count,
            accepted_hits,
            rejected_hits: 0,
            abstained,
            reason: Some(reason),
            detail,
        },
    )
}

fn lexical_fallback_hits<'a>(
    query: &str,
    lexical_fallback_chunks: &'a [ChunkHit],
) -> Vec<&'a ChunkHit> {
    let exact_matches = lexical_fallback_chunks
        .iter()
        .filter(|chunk| chunk.content.contains(query))
        .collect::<Vec<_>>();
    if !exact_matches.is_empty() {
        return exact_matches;
    }
    let query_lower = query.to_lowercase();
    let case_folded_matches = lexical_fallback_chunks
        .iter()
        .filter(|chunk| chunk.content.to_lowercase().contains(&query_lower))
        .collect::<Vec<_>>();
    if !case_folded_matches.is_empty() {
        return case_folded_matches;
    }
    lexical_fallback_chunks.iter().collect::<Vec<_>>()
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
            detail: None,
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
        "reason": guard.reason,
        "detail": guard.detail
    })
}

fn ensure_context_pack_decision_trace(payload: &mut Value) {
    if payload["decision_trace"].is_object() {
        return;
    }
    payload["decision_trace"] = build_context_pack_decision_trace(payload);
}

fn build_context_pack_decision_trace(payload: &Value) -> Value {
    let exact_count = payload["retrieval"]["exact_documents"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let symbol_count = payload["retrieval"]["symbol_hits"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let lexical_count = payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let semantic_count = payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let semantic_guard = &payload["quality"]["semantic_guard"];

    let mut included = Vec::new();
    if exact_count > 0 {
        included.push(json!({
            "strategy": "exact_documents",
            "count": exact_count,
            "reason": "Нашлись точные document/path совпадения внутри видимого контура."
        }));
    }
    if symbol_count > 0 {
        included.push(json!({
            "strategy": "symbol_hits",
            "count": symbol_count,
            "reason": "Нашлись symbol-совпадения по именам и доступным namespace."
        }));
    }
    if lexical_count > 0 {
        included.push(json!({
            "strategy": "lexical_chunks",
            "count": lexical_count,
            "reason": "Лексический поиск добавил текстовые фрагменты с прямым совпадением по запросу."
        }));
    }
    if semantic_count > 0 {
        included.push(json!({
            "strategy": "semantic_chunks",
            "count": semantic_count,
            "reason": "Semantic layer добавил фрагменты после relevance guard и scope-фильтра."
        }));
    }

    let mut not_included = Vec::new();
    if exact_count == 0 {
        not_included.push(json!({
            "strategy": "exact_documents",
            "reason": "Точных document/path совпадений по этому запросу не нашлось."
        }));
    }
    if symbol_count == 0 {
        not_included.push(json!({
            "strategy": "symbol_hits",
            "reason": "По этому запросу не нашлось symbol-совпадений в видимом контуре."
        }));
    }
    if lexical_count == 0 {
        not_included.push(json!({
            "strategy": "lexical_chunks",
            "reason": "Лексический поиск не дал новых chunk-фрагментов."
        }));
    }
    if semantic_count == 0 {
        let reason = if semantic_guard["abstained"].as_bool() == Some(true) {
            semantic_guard["detail"]
                .as_str()
                .or_else(|| semantic_guard["reason"].as_str())
                .unwrap_or("Semantic layer честно abstained и не добавил фрагменты.")
                .to_string()
        } else {
            "Semantic layer не добавил новых фрагментов после scope и relevance проверки."
                .to_string()
        };
        not_included.push(json!({
            "strategy": "semantic_chunks",
            "reason": reason
        }));
    }

    json!({
        "selection_priority": [
            "exact_documents",
            "symbol_hits",
            "lexical_chunks",
            "semantic_chunks"
        ],
        "scope": {
            "project_code": payload["project"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "visible_projects_total": payload["visible_projects"].as_array().map(Vec::len).unwrap_or_default(),
        },
        "included": included,
        "not_included": not_included,
        "semantic_guard": semantic_guard.clone(),
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

fn fast_cache_key(cfg: &AppConfig, args: &ContextPackArgs, effective_mode: &str) -> FastCacheKey {
    let mut hasher = Sha256::new();
    hasher.update(cfg.stack_name.as_bytes());
    hasher.update([0]);
    hasher.update(args.project.as_bytes());
    hasher.update([0]);
    hasher.update(args.namespace.as_bytes());
    hasher.update([0]);
    hasher.update(effective_mode.as_bytes());
    hasher.update([0]);
    hasher.update(args.limit_documents.to_le_bytes());
    hasher.update(args.limit_symbols.to_le_bytes());
    hasher.update(args.limit_chunks.to_le_bytes());
    hasher.update(args.limit_semantic_chunks.to_le_bytes());
    hasher.update(args.query.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0_u8; 16];
    key.copy_from_slice(&digest[..16]);
    u128::from_be_bytes(key)
}

fn prepared_from_cached_payload(
    context: CacheHydrationContext<'_>,
    cached: LocalContextPackEntry,
) -> Result<PreparedContextPack> {
    let stats = cached_context_pack_stats(
        &cached,
        context.scope_signature.clone(),
        context.resolve_scope_ms,
        context.cache_lookup_ms,
    );
    let mut payload = cached.payload.as_ref().clone();
    payload["retrieval_runtime"] = runtime_json(&stats);
    ensure_context_pack_decision_trace(&mut payload);
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

async fn try_execute_context_pack_stats_cached(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackStats>> {
    if let Some(stats) = try_context_pack_stats_from_local_fast_cache(cfg, args, persist)? {
        return Ok(Some(stats));
    }
    if args.disable_cache {
        return Ok(None);
    }

    let resolve_started = Instant::now();
    let project = postgres::get_project_by_code(db, &args.project).await?;
    let namespace =
        postgres::get_namespace_by_code(db, project.project_id, &args.namespace).await?;
    let effective_mode = args
        .retrieval_mode
        .clone()
        .unwrap_or_else(|| namespace.retrieval_mode.clone());
    let visible_projects = resolve_visible_projects(db, &project, &effective_mode).await?;
    let visible_scopes =
        resolve_visible_scopes(db, &visible_projects, project.project_id, &namespace).await?;
    let resolve_scope_ms = resolve_started.elapsed().as_millis();
    let scope_signature = scope_signature(&visible_scopes);
    let cache_key = cache_key(cfg, args, &effective_mode);
    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;

    let cache_lookup_started = Instant::now();
    if let Some(cached) = local_context_pack_cache_get(&cache_key, &scope_signature)? {
        if persist && !cached.durably_persisted {
            return Ok(None);
        }
        let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
        return Ok(Some(cached_context_pack_stats(
            &cached,
            scope_signature,
            resolve_scope_ms,
            cache_lookup_ms,
        )));
    }

    if let Some(cached) =
        edge_cache::get_context_pack_cache_entry(&edge_cache_path, &cache_key, &scope_signature)?
    {
        let local_cached = local_entry_from_edge(cached)?;
        if persist && !local_cached.durably_persisted {
            return Ok(None);
        }
        local_context_pack_cache_put(&cache_key, &scope_signature, &local_cached)?;
        let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
        return Ok(Some(cached_context_pack_stats(
            &local_cached,
            scope_signature,
            resolve_scope_ms,
            cache_lookup_ms,
        )));
    }

    Ok(None)
}

fn try_context_pack_stats_from_local_fast_cache(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackStats>> {
    if args.disable_cache {
        return Ok(None);
    }
    let Some(effective_mode) = args.retrieval_mode.as_deref() else {
        return Ok(None);
    };
    let fast_cache_key = fast_cache_key(cfg, args, effective_mode);
    let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    else {
        return Ok(None);
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(cached_fast_context_pack_stats(&cached)))
}

fn local_context_pack_cache_get(
    cache_key: &str,
    scope_signature: &str,
) -> Result<Option<LocalContextPackEntry>> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let guard = cache
        .read()
        .map_err(|_| anyhow!("local context-pack cache lock poisoned"))?;
    let key = format!("{cache_key}::{scope_signature}");
    Ok(guard.get(&key).cloned())
}

fn local_context_pack_cache_put(
    cache_key: &str,
    scope_signature: &str,
    entry: &LocalContextPackEntry,
) -> Result<()> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut guard = cache
        .write()
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

fn cached_context_pack_stats(
    cached: &LocalContextPackEntry,
    scope_signature: String,
    resolve_scope_ms: u128,
    cache_lookup_ms: u128,
) -> ContextPackStats {
    ContextPackStats {
        context_pack_id: cached.context_pack_id,
        exact_documents: cached.exact_documents,
        symbol_hits: cached.symbol_hits,
        lexical_chunks: cached.lexical_chunks,
        semantic_chunks: cached.semantic_chunks,
        cache_hit: true,
        scope_signature,
        timings: ContextPackTimings {
            resolve_scope_ms,
            cache_lookup_ms,
            exact_lookup_ms: 0,
            symbol_lookup_ms: 0,
            lexical_lookup_ms: 0,
            query_embed_ms: 0,
            semantic_search_ms: 0,
            semantic_hydrate_ms: 0,
            ranking_ms: 0,
            provenance_ms: 0,
            pack_assembly_ms: 0,
            serialize_ms: 0,
            persist_ms: 0,
        },
    }
}

fn cached_fast_context_pack_stats(cached: &LocalContextPackEntry) -> ContextPackStats {
    cached_context_pack_stats(cached, "local_fast_cache".to_string(), 0, 0)
}

fn context_pack_result_from_local_entry(cached: LocalContextPackEntry) -> ContextPackResult {
    let stats = cached_fast_context_pack_stats(&cached);
    let mut payload = cached.payload.as_ref().clone();
    payload["retrieval_runtime"] = runtime_json(&stats);
    ensure_context_pack_decision_trace(&mut payload);
    ContextPackResult { payload, stats }
}

fn runtime_json(stats: &ContextPackStats) -> Value {
    let timings = &stats.timings;
    let policy_ms = timings.resolve_scope_ms + timings.cache_lookup_ms;
    let retrieval_ms = timings.exact_lookup_ms
        + timings.symbol_lookup_ms
        + timings.lexical_lookup_ms
        + timings.query_embed_ms
        + timings.semantic_search_ms
        + timings.semantic_hydrate_ms;
    let total_ms = timings.resolve_scope_ms
        + timings.cache_lookup_ms
        + timings.exact_lookup_ms
        + timings.symbol_lookup_ms
        + timings.lexical_lookup_ms
        + timings.query_embed_ms
        + timings.semantic_search_ms
        + timings.semantic_hydrate_ms
        + timings.ranking_ms
        + timings.provenance_ms
        + timings.pack_assembly_ms
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
        "ranking_ms": timings.ranking_ms as f64,
        "provenance_ms": timings.provenance_ms as f64,
        "pack_assembly_ms": timings.pack_assembly_ms as f64,
        "serialize_ms": timings.serialize_ms as f64,
        "persist_ms": timings.persist_ms as f64,
        "stage_group_ms": {
            "policy_ms": policy_ms as f64,
            "retrieval_ms": retrieval_ms as f64,
            "ranking_ms": timings.ranking_ms as f64,
            "provenance_ms": timings.provenance_ms as f64,
            "pack_assembly_ms": (timings.pack_assembly_ms + timings.serialize_ms) as f64,
            "orchestration_total_ms": total_ms as f64
        },
        "total_ms": total_ms as f64
    })
}

fn local_fast_context_pack_cache_get(
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
) -> Result<Option<LocalContextPackEntry>> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let entry = {
        let guard = cache
            .read()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        guard.get(&fast_cache_key).cloned()
    };
    let Some(entry) = entry else {
        return Ok(None);
    };
    if now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms {
        let mut guard = cache
            .write()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        guard.remove(&fast_cache_key);
        return Ok(None);
    }
    Ok(Some(entry))
}

fn local_fast_context_pack_cache_contains(
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
    require_persist: bool,
) -> Result<bool> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let status = {
        let guard = cache
            .read()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        let Some(entry) = guard.get(&fast_cache_key) else {
            return Ok(false);
        };
        let expired = now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms;
        let persisted_ok = !require_persist || entry.durably_persisted;
        (expired, persisted_ok)
    };
    if status.0 {
        let mut guard = cache
            .write()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        guard.remove(&fast_cache_key);
        return Ok(false);
    }
    if !status.1 {
        return Ok(false);
    }
    Ok(true)
}

fn local_fast_context_pack_cache_put(
    fast_cache_key: FastCacheKey,
    entry: &LocalContextPackEntry,
) -> Result<()> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut guard = cache
        .write()
        .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
    guard.insert(fast_cache_key, entry.clone());
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

fn collect_workspace_document_requests(
    visible_scopes: &[ResolvedVisibleScope],
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
) -> Vec<(Uuid, String)> {
    let scope_ids = visible_scopes
        .iter()
        .map(|scope| {
            (
                format!("{}::{}", scope.visible.project.code, scope.namespace.code),
                scope.namespace.namespace_id,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut requests = Vec::new();
    for hit in documents {
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
        );
    }
    for hit in symbols {
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
        );
    }
    for hit in chunks {
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
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
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            project_code,
            namespace_code,
            relative_path,
        );
    }
    requests
}

fn push_workspace_request(
    scope_ids: &HashMap<String, Uuid>,
    seen: &mut HashSet<String>,
    requests: &mut Vec<(Uuid, String)>,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
) {
    let key = format!("{project_code}::{namespace_code}");
    let Some(namespace_id) = scope_ids.get(&key) else {
        return;
    };
    let dedupe_key = format!("{namespace_id}::{relative_path}");
    if seen.insert(dedupe_key) {
        requests.push((*namespace_id, relative_path.to_string()));
    }
}

fn should_use_minimal_document_workspace_graph(
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
) -> bool {
    documents.len() == 1 && symbols.is_empty() && chunks.is_empty() && semantic_chunks.is_empty()
}

fn should_use_minimal_symbol_workspace_graph(
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
) -> bool {
    documents.is_empty() && chunks.is_empty() && semantic_chunks.is_empty() && symbols.len() == 1
}

pub fn degradation_proof_scenarios(local_fast_cache_ttl_ms: u128) -> Result<Vec<Value>> {
    let lexical_chunk = synthetic_chunk_hit(
        "src/degradation.rs",
        "fn stable_semantic_fallback() { /* lexical safety net */ }",
    );
    let (qdrant_hits, _, qdrant_guard) = semantic_fallback_result(
        "stable semantic fallback",
        4,
        1,
        &[lexical_chunk.clone()],
        SemanticTimings::default(),
        "vector_layer_unavailable",
        Some("synthetic Qdrant outage".to_string()),
    );
    let qdrant_unavailable_pass = qdrant_hits
        .first()
        .and_then(|item| item["retrieval_strategy"].as_str())
        == Some("lexical_fallback")
        && qdrant_guard.reason == Some("vector_layer_unavailable")
        && qdrant_guard.accepted_hits == 1;

    let (empty_embedding_hits, _, empty_embedding_guard) = semantic_fallback_result(
        "stable semantic fallback",
        4,
        1,
        &[lexical_chunk],
        SemanticTimings::default(),
        "embedding_unavailable",
        Some("synthetic empty embedding vector".to_string()),
    );
    let empty_embeddings_pass = empty_embedding_hits
        .first()
        .and_then(|item| item["retrieval_strategy"].as_str())
        == Some("lexical_fallback")
        && empty_embedding_guard.reason == Some("embedding_unavailable")
        && empty_embedding_guard.accepted_hits == 1;

    let stale_cache_state = degradation_probe_stale_fast_cache(local_fast_cache_ttl_ms)?;
    let stale_cache_pass = stale_cache_state["cache_hit"].as_bool() == Some(false)
        && stale_cache_state["cache_entry_remaining"].as_bool() == Some(false);

    Ok(vec![
        json!({
            "class_key": "qdrant_unavailable",
            "title": "Qdrant недоступен",
            "status": if qdrant_unavailable_pass { "pass" } else { "critical" },
            "reason": if qdrant_unavailable_pass {
                "retrieval держит безопасный lexical fallback, когда vector layer недоступен."
            } else {
                "retrieval не удержал безопасный lexical fallback при недоступном vector layer."
            },
            "details": {
                "retrieval_strategy": qdrant_hits.first().and_then(|item| item["retrieval_strategy"].as_str()).unwrap_or("none"),
                "semantic_guard_reason": qdrant_guard.reason,
                "semantic_guard_detail": qdrant_guard.detail,
            }
        }),
        json!({
            "class_key": "stale_cache",
            "title": "Устаревший cache",
            "status": if stale_cache_pass { "pass" } else { "critical" },
            "reason": if stale_cache_pass {
                "expired local fast cache не выдаётся как свежий hit и честно вычищается перед reuse."
            } else {
                "expired local fast cache продолжил выглядеть как свежий hit."
            },
            "details": stale_cache_state,
        }),
        json!({
            "class_key": "empty_embeddings",
            "title": "Пустые embeddings",
            "status": if empty_embeddings_pass { "pass" } else { "critical" },
            "reason": if empty_embeddings_pass {
                "retrieval честно уходит в lexical fallback, когда semantic embedding layer не даёт usable vector."
            } else {
                "retrieval потерял безопасный fallback при пустом semantic embedding layer."
            },
            "details": {
                "retrieval_strategy": empty_embedding_hits.first().and_then(|item| item["retrieval_strategy"].as_str()).unwrap_or("none"),
                "semantic_guard_reason": empty_embedding_guard.reason,
                "semantic_guard_detail": empty_embedding_guard.detail,
            }
        }),
    ])
}

fn degradation_probe_stale_fast_cache(local_fast_cache_ttl_ms: u128) -> Result<Value> {
    let proof_key = 0xfeed_u128;
    let expired_at = now_epoch_ms().saturating_sub(local_fast_cache_ttl_ms.saturating_add(1));
    let entry = LocalContextPackEntry {
        context_pack_id: Uuid::new_v4(),
        payload: Arc::new(json!({
            "context_pack_id": Uuid::nil(),
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        })),
        exact_documents: 0,
        symbol_hits: 0,
        lexical_chunks: 0,
        semantic_chunks: 0,
        durably_persisted: true,
        cached_at_epoch_ms: expired_at,
    };
    local_fast_context_pack_cache_put(proof_key, &entry)?;
    let cache_hit =
        local_fast_context_pack_cache_contains(proof_key, local_fast_cache_ttl_ms, true)?;
    let cache_entry_remaining =
        local_fast_context_pack_cache_get(proof_key, local_fast_cache_ttl_ms)?.is_some();
    Ok(json!({
        "cache_hit": cache_hit,
        "cache_entry_remaining": cache_entry_remaining,
        "expired_by_ms": now_epoch_ms().saturating_sub(expired_at).saturating_sub(local_fast_cache_ttl_ms),
    }))
}

fn synthetic_chunk_hit(relative_path: &str, content: &str) -> ChunkHit {
    ChunkHit {
        project_code: "art".to_string(),
        namespace_code: "continuity".to_string(),
        repo_root: "/tmp/degradation-proof".to_string(),
        relative_path: relative_path.to_string(),
        chunk_id: Uuid::new_v4(),
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        score: 1.0,
        content: content.to_string(),
        metadata: json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SemanticTimings, apply_semantic_relevance_guard, build_context_pack_decision_trace,
        degradation_probe_stale_fast_cache, degradation_proof_scenarios,
        model_visible_context_pack_payload, query_terms, semantic_fallback_result,
        semantic_hit_has_query_overlap, should_use_minimal_document_workspace_graph,
        should_use_minimal_symbol_workspace_graph, synthetic_chunk_hit,
        with_whole_cycle_observed_overrides,
    };
    use crate::cli::ContextPackArgs;
    use crate::postgres::{DocumentHit, SymbolHit};
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

    #[test]
    fn context_pack_decision_trace_explains_included_and_missing_layers() {
        let payload = json!({
            "project": {"code": "art"},
            "namespace": {"code": "continuity"},
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{"project_code": "art"}],
            "retrieval": {
                "exact_documents": [{"relative_path": "README.md"}],
                "symbol_hits": [],
                "lexical_chunks": [{"relative_path": "docs/guide.md"}],
                "semantic_chunks": []
            },
            "quality": {
                "semantic_guard": {
                    "abstained": true,
                    "reason": "semantic_hits_missing_query_overlap",
                    "detail": "semantic layer abstained"
                }
            }
        });
        let trace = build_context_pack_decision_trace(&payload);
        assert_eq!(trace["included"].as_array().map(Vec::len), Some(2));
        assert_eq!(trace["not_included"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            trace["not_included"][1]["reason"].as_str(),
            Some("semantic layer abstained")
        );
        assert_eq!(
            trace["scope"]["effective_retrieval_mode"].as_str(),
            Some("local_strict")
        );
    }

    #[test]
    fn whole_cycle_observed_overrides_are_added_to_payload() {
        let payload = json!({
            "query": "token report"
        });
        let args = ContextPackArgs {
            project: "art".to_string(),
            namespace: "default".to_string(),
            query: "token report".to_string(),
            retrieval_mode: None,
            disable_cache: false,
            limit_documents: 5,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "live_context_pack".to_string(),
            client_prompt_tokens: Some(42),
            assistant_generation_tokens: Some(24),
            tool_overhead_tokens: Some(7),
            continuity_restore_tokens: Some(3),
        };

        let updated = with_whole_cycle_observed_overrides(&payload, &args);

        assert_eq!(updated["whole_cycle_observed"]["client_prompt_tokens"], 42);
        assert_eq!(
            updated["whole_cycle_observed"]["assistant_generation_tokens"],
            24
        );
        assert_eq!(updated["whole_cycle_observed"]["tool_overhead_tokens"], 7);
        assert_eq!(
            updated["whole_cycle_observed"]["continuity_restore_tokens"],
            3
        );
    }

    #[test]
    fn model_visible_context_pack_payload_keeps_required_fields_and_drops_heavy_runtime_details() {
        let payload = json!({
            "context_pack_id": "ctx-1",
            "project": {
                "code": "art",
                "display_name": "Art",
                "repo_root": "/home/art/Art"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "included": [{"strategy": "exact_documents", "count": 1}],
                "not_included": []
            },
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": ".amai-continuity/live-handoff.md",
                    "snippet": "handoff snippet",
                    "source_kind": "docs",
                    "score": 2000.0,
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art",
                        "commit_sha": "abc"
                    }
                }],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "build_context_pack",
                    "kind": "function_item",
                    "metadata": {"language": "rust"},
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/guide.md",
                    "content": "lexical excerpt",
                    "chunk": {"chunk_id": "c1"},
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/guide.md",
                    "content": "semantic excerpt",
                    "retrieval_strategy": "semantic",
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }]
            },
            "workspace_graph": {"heavy": true},
            "retrieval_runtime": {"total_ms": 88}
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(compact["context_pack_id"].as_str(), Some("ctx-1"));
        assert_eq!(compact["project"]["code"].as_str(), Some("art"));
        assert_eq!(compact["namespace"]["code"].as_str(), Some("continuity"));
        assert!(compact["project"].get("display_name").is_none());
        assert!(compact["namespace"].get("display_name").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact["visible_projects"][0].get("repo_root").is_none());
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["relative_path"].as_str(),
            Some(".amai-continuity/live-handoff.md")
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["content"].as_str(),
            Some("lexical excerpt")
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"][0]["content"].as_str(),
            Some("semantic excerpt")
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["name"].as_str(),
            Some("build_context_pack")
        );
        assert!(compact["decision_trace"]["included"][0].get("reason").is_none());
        assert!(compact["decision_trace"]["semantic_guard"].get("detail").is_none());
        assert!(compact.get("workspace_graph").is_none());
        assert!(compact.get("retrieval_runtime").is_none());
        assert!(compact["retrieval"]["exact_documents"][0].get("score").is_none());
        assert!(compact["retrieval"]["lexical_chunks"][0].get("chunk").is_none());
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("metadata")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["provenance"]["source_project"].as_str(),
            Some("art")
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]["provenance"]
                .get("repo_root")
                .is_none()
        );
        assert!(
            serde_json::to_string(&compact).expect("compact json").len()
                < serde_json::to_string(&payload).expect("full json").len()
        );
    }

    #[test]
    fn model_visible_context_pack_payload_dedupes_duplicate_semantic_chunks_only() {
        let payload = json!({
            "context_pack_id": "ctx-dup",
            "project": {
                "code": "art",
                "display_name": "Art",
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "selection_priority": ["exact_documents", "lexical_chunks", "semantic_chunks"],
                "included": [{
                    "strategy": "lexical_chunks",
                    "count": 1,
                    "reason": "verbose reason"
                }],
                "not_included": [{
                    "strategy": "symbol_hits",
                    "reason": "verbose miss reason"
                }],
                "semantic_guard": {
                    "abstained": false,
                    "accepted_hits": 1,
                    "rejected_hits": 0,
                    "lexical_signal_count": 2,
                    "query_terms": ["continuity", "snapshot"],
                    "reason": "no_vector_points_in_scope",
                    "detail": "verbose detail"
                }
            },
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "snippet": "same excerpt",
                    "source_kind": "docs"
                }],
                "symbol_hits": [],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "same excerpt",
                    "provenance": {
                        "source_project": "art",
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "same excerpt",
                    "provenance": {
                        "source_project": "art",
                    }
                }]
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(compact["retrieval"]["exact_documents"].as_array().unwrap().len(), 1);
        assert_eq!(compact["retrieval"]["lexical_chunks"].as_array().unwrap().len(), 1);
        assert_eq!(compact["retrieval"]["semantic_chunks"].as_array().unwrap().len(), 0);
        assert!(compact["decision_trace"]["included"][0].get("reason").is_none());
        assert!(compact["decision_trace"]["not_included"][0].get("reason").is_none());
        assert!(compact["decision_trace"]["semantic_guard"].get("detail").is_none());
    }

    #[test]
    fn minimal_symbol_workspace_graph_only_applies_to_single_symbol_only_shape() {
        let symbols = vec![SymbolHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "src/verify.rs".to_string(),
            name: "run_text_compare".to_string(),
            kind: "function_item".to_string(),
            start_line: 1212,
            end_line: 1425,
            start_byte: 0,
            end_byte: 0,
            score: 2000.0,
            metadata: json!({"language":"rust"}),
        }];
        assert!(should_use_minimal_symbol_workspace_graph(
            &[],
            &symbols,
            &[],
            &[]
        ));

        let docs = vec![DocumentHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "README.md".to_string(),
            language: Some("markdown".to_string()),
            source_kind: "docs".to_string(),
            git_commit_sha: None,
            score: 1500.0,
            snippet: "readme".to_string(),
        }];
        assert!(!should_use_minimal_symbol_workspace_graph(
            &docs,
            &symbols,
            &[],
            &[]
        ));
    }

    #[test]
    fn minimal_document_workspace_graph_only_applies_to_single_document_only_shape() {
        let docs = vec![DocumentHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "src/config.rs".to_string(),
            language: Some("rust".to_string()),
            source_kind: "code".to_string(),
            git_commit_sha: None,
            score: 2000.0,
            snippet: "config".to_string(),
        }];
        assert!(should_use_minimal_document_workspace_graph(
            &docs,
            &[],
            &[],
            &[]
        ));

        let symbols = vec![SymbolHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "src/config.rs".to_string(),
            name: "from_env".to_string(),
            kind: "function_item".to_string(),
            start_line: 45,
            end_line: 94,
            start_byte: 0,
            end_byte: 0,
            score: 2000.0,
            metadata: json!({"language":"rust"}),
        }];
        assert!(!should_use_minimal_document_workspace_graph(
            &docs,
            &symbols,
            &[],
            &[]
        ));
    }

    #[test]
    fn semantic_fallback_result_marks_vector_layer_unavailable() {
        let chunk = synthetic_chunk_hit("src/lib.rs", "fn stable_semantic_fallback() {}");
        let (hits, _, guard) = semantic_fallback_result(
            "stable semantic fallback",
            2,
            1,
            &[chunk],
            SemanticTimings::default(),
            "vector_layer_unavailable",
            Some("synthetic qdrant outage".to_string()),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["retrieval_strategy"], json!("lexical_fallback"));
        assert_eq!(guard.reason, Some("vector_layer_unavailable"));
        assert_eq!(guard.detail.as_deref(), Some("synthetic qdrant outage"));
    }

    #[test]
    fn degradation_probe_stale_fast_cache_evicts_expired_entry() {
        let state = degradation_probe_stale_fast_cache(1).expect("stale cache proof");
        assert_eq!(state["cache_hit"], json!(false));
        assert_eq!(state["cache_entry_remaining"], json!(false));
    }

    #[test]
    fn degradation_proof_scenarios_cover_retrieval_fallback_classes() {
        let scenarios = degradation_proof_scenarios(1).expect("scenarios");
        assert_eq!(scenarios.len(), 3);
        assert!(
            scenarios
                .iter()
                .all(|scenario| scenario["status"].as_str() == Some("pass"))
        );
    }
}
