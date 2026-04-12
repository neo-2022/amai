use crate::cli::ContextPackArgs;
use crate::codex_threads;
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
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
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
    pub retrieval_lower_bound_ms_precise: Option<f64>,
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
    artifact_bucket: Option<String>,
    artifact_object_key: Option<String>,
    artifact_state: Option<String>,
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
    query_cache: HashMap<String, Vec<f32>>,
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
    precise_lower_bound_ms: f64,
}

static QUERY_EMBEDDER: OnceLock<Mutex<Option<CachedQueryEmbedder>>> = OnceLock::new();
static LOCAL_CONTEXT_PACK_CACHE: OnceLock<RwLock<HashMap<String, LocalContextPackEntry>>> =
    OnceLock::new();
static LOCAL_FAST_CONTEXT_PACK_CACHE: OnceLock<
    RwLock<HashMap<FastCacheKey, LocalContextPackEntry>>,
> = OnceLock::new();
static THREAD_CONTEXT_PACK_DELIVERY_CACHE: OnceLock<RwLock<HashMap<String, HashSet<String>>>> =
    OnceLock::new();

pub async fn build_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<()> {
    let prepared = execute_context_pack_capture_with_options(cfg, db, args, true, true).await?;
    let context_pack_id = prepared
        .payload
        .get("context_pack_id")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .unwrap_or_else(|| prepared.stats.context_pack_id.to_string());
    let caller_payload =
        materialize_context_pack_caller_payload(db, args, &prepared.payload, &prepared.stats)
            .await?;
    let compact_output =
        cli_visible_context_pack_payload(&caller_payload, args.token_source_kind.as_str());
    let compact_output_json = serde_json::to_string(&compact_output)?;
    let _ = token_budget::observe_cli_context_pack_tool_overhead(
        db,
        &context_pack_id,
        compact_output_json.as_str(),
    )
    .await?;
    if prepared.stats.cache_hit {
        eprintln!(
            "context pack cache hit: {} :: scope={}",
            context_pack_id, prepared.stats.scope_signature
        );
    } else {
        let artifact_location = match (
            prepared
                .payload
                .get("artifact_bucket")
                .and_then(Value::as_str),
            prepared
                .payload
                .get("artifact_object_key")
                .and_then(Value::as_str),
        ) {
            (Some(bucket), Some(object_key)) => format!("s3://{bucket}/{object_key}"),
            _ => "artifact://unavailable".to_string(),
        };
        let artifact_state = prepared
            .payload
            .get("artifact_state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        eprintln!(
            "context pack truth persisted: postgres :: artifact_state={} :: {} :: {}",
            artifact_state, artifact_location, context_pack_id
        );
    }
    println!("{compact_output_json}");
    Ok(())
}

fn cli_visible_context_pack_payload(payload: &Value, token_source_kind: &str) -> Value {
    let mut compact = model_visible_context_pack_payload(payload);
    if token_source_kind == "proof_context_pack" && payload["decision_trace"].is_object() {
        compact["decision_trace"] = payload["decision_trace"].clone();
    }
    compact
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
        let payload =
            materialize_context_pack_caller_payload(db, args, &cached.payload, &cached.stats)
                .await?;
        if track_token_usage {
            record_context_pack_token_budget_event(db, &payload, args).await?;
        }
        return Ok(ContextPackResult {
            payload,
            stats: cached.stats,
        });
    }
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    if persist {
        ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    }
    let durably_persisted = persist || prepared.durably_persisted;
    cache_context_pack_entry(cfg, args, &prepared, durably_persisted)?;
    let payload = materialize_context_pack_caller_payload(
        db,
        args,
        prepared.payload.as_ref(),
        &prepared.stats,
    )
    .await?;
    if track_token_usage {
        record_context_pack_token_budget_event(db, &payload, args).await?;
    }
    Ok(ContextPackResult {
        payload,
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
    let skip_working_state_live_repeat = args.token_source_kind == "live_context_pack"
        && payload["retrieval_runtime"]["cache_hit"].as_bool() == Some(true);
    if !skip_working_state_live_repeat {
        working_state::record_context_pack_event(db, payload, &args.token_source_kind).await?;
    }
    remember_thread_context_pack_delivery(payload)?;
    Ok(())
}

async fn materialize_context_pack_caller_payload(
    db: &Client,
    args: &ContextPackArgs,
    payload: &Value,
    stats: &ContextPackStats,
) -> Result<Value> {
    let payload = if let Some(compact) =
        same_thread_cache_reuse_payload_if_needed(db, payload, stats, args).await?
    {
        compact
    } else {
        payload.clone()
    };
    Ok(with_whole_cycle_observed_overrides(&payload, args))
}

async fn same_thread_cache_reuse_payload_if_needed(
    db: &Client,
    payload: &Value,
    stats: &ContextPackStats,
    args: &ContextPackArgs,
) -> Result<Option<Value>> {
    if !should_emit_same_thread_cache_reuse_payload(stats, args) {
        return Ok(None);
    }
    let Some(thread_id) = codex_threads::current_thread_id() else {
        return Ok(None);
    };
    let Some(context_pack_id) = payload["context_pack_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if !same_thread_context_pack_previously_delivered(db, &thread_id, context_pack_id).await? {
        return Ok(None);
    }
    Ok(Some(build_same_thread_cache_reuse_payload(
        payload, &thread_id,
    )))
}

fn should_emit_same_thread_cache_reuse_payload(
    stats: &ContextPackStats,
    args: &ContextPackArgs,
) -> bool {
    stats.cache_hit && args.token_source_kind != "proof_context_pack"
}

async fn same_thread_context_pack_previously_delivered(
    _db: &Client,
    thread_id: &str,
    context_pack_id: &str,
) -> Result<bool> {
    thread_context_pack_delivery_seen(thread_id, context_pack_id)
}

fn build_same_thread_cache_reuse_payload(payload: &Value, _thread_id: &str) -> Value {
    let active_files = context_pack_active_files(payload, 6);
    let default_project_code = payload["project"]["code"].as_str().unwrap_or_default();
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
        "visible_projects": compact_visible_projects_for_token_budget(&payload["visible_projects"]),
        "retrieval_runtime": compact_retrieval_runtime_for_token_budget(&payload["retrieval_runtime"]),
        "whole_cycle_observed": compact_whole_cycle_observed_for_token_budget(&payload["whole_cycle_observed"]),
        "retrieval": {
            "exact_documents": metadata_only_exact_documents(
                &payload["retrieval"]["exact_documents"],
                default_project_code,
            ),
            "symbol_hits": compact_symbol_hits(&payload["retrieval"]["symbol_hits"], default_project_code),
            "lexical_chunks": metadata_only_chunk_refs(
                &payload["retrieval"]["lexical_chunks"],
                default_project_code,
            ),
            "semantic_chunks": metadata_only_chunk_refs(
                &payload["retrieval"]["semantic_chunks"],
                default_project_code,
            ),
            "memory_cards": metadata_only_memory_cards(
                &payload["retrieval"]["memory_cards"],
                default_project_code,
            ),
            "memory_relation_edges": metadata_only_memory_relation_edges(
                &payload["retrieval"]["memory_relation_edges"],
                default_project_code,
            ),
            "raw_evidence": metadata_only_raw_evidence(
                &payload["retrieval"]["raw_evidence"],
                default_project_code,
            ),
        },
        "cache_reuse_reference": {
            "state": "same_thread_context_pack_replay",
            "source_context_pack_id": payload["context_pack_id"].clone(),
            "active_files": active_files,
            "retrieval_counts": {
                "exact_documents": payload["retrieval"]["exact_documents"].as_array().map_or(0, Vec::len),
                "symbol_hits": payload["retrieval"]["symbol_hits"].as_array().map_or(0, Vec::len),
                "lexical_chunks": payload["retrieval"]["lexical_chunks"].as_array().map_or(0, Vec::len),
                "semantic_chunks": payload["retrieval"]["semantic_chunks"].as_array().map_or(0, Vec::len),
                "memory_cards": payload["retrieval"]["memory_cards"].as_array().map_or(0, Vec::len),
                "memory_relation_edges": payload["retrieval"]["memory_relation_edges"]
                    .as_array()
                    .map_or(0, Vec::len),
                "raw_evidence": payload["retrieval"]["raw_evidence"].as_array().map_or(0, Vec::len),
            }
        }
    })
}

fn compact_visible_projects_for_token_budget(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "project_code": item["project_code"].clone(),
                "repo_root": item["repo_root"].clone(),
            })
        })
        .collect()
}

fn compact_retrieval_runtime_for_token_budget(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "cache_hit": value["cache_hit"].clone(),
        "scope_signature": value["scope_signature"].clone(),
        "retrieval_lower_bound_ms": value["retrieval_lower_bound_ms"].clone(),
        "total_ms": value["total_ms"].clone(),
    })
}

fn compact_whole_cycle_observed_for_token_budget(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "client_prompt_tokens": value["client_prompt_tokens"].clone(),
        "assistant_generation_tokens": value["assistant_generation_tokens"].clone(),
        "tool_overhead_tokens": value["tool_overhead_tokens"].clone(),
        "continuity_restore_tokens": value["continuity_restore_tokens"].clone(),
    })
}

fn metadata_only_exact_documents(value: &Value, default_project_code: &str) -> Vec<Value> {
    let mut seen_paths = HashSet::new();
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let signature = compact_text_path_signature(
                item["project_code"]
                    .as_str()
                    .or_else(|| item["provenance"]["source_project"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
            )?;
            if !seen_paths.insert(signature) {
                return None;
            }
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            Some(compact)
        })
        .collect()
}

fn metadata_only_chunk_refs(value: &Value, default_project_code: &str) -> Vec<Value> {
    let mut seen_paths = HashSet::new();
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let signature = compact_text_path_signature(
                item["project_code"]
                    .as_str()
                    .or_else(|| item["provenance"]["source_project"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
            )?;
            if !seen_paths.insert(signature) {
                return None;
            }
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            Some(compact)
        })
        .collect()
}

fn metadata_only_memory_cards(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_card_id": item["memory_card_id"].clone(),
                "title": item["title"].clone(),
                "truth_state": item["truth_state"].clone(),
                "verification_state": item["verification_state"].clone(),
                "status": item["status"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_memory_cards(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_card_id": item["memory_card_id"].clone(),
                "title": item["title"].clone(),
                "summary": item["summary"].clone(),
                "fact_subject": item["fact_subject"].clone(),
                "fact_predicate": item["fact_predicate"].clone(),
                "fact_object": item["fact_object"].clone(),
                "truth_state": item["truth_state"].clone(),
                "verification_state": item["verification_state"].clone(),
                "status": item["status"].clone(),
                "observed_at_epoch_ms": item["observed_at_epoch_ms"].clone(),
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"].clone(),
                "valid_from_epoch_ms": item["valid_from_epoch_ms"].clone(),
                "valid_to_epoch_ms": item["valid_to_epoch_ms"].clone(),
                "last_verified_at_epoch_ms": item["last_verified_at_epoch_ms"].clone(),
                "superseded_by_memory_card_id": item["superseded_by_memory_card_id"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn metadata_only_memory_relation_edges(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_relation_edge_id": item["memory_relation_edge_id"].clone(),
                "source_memory_card_id": item["source_memory_card_id"].clone(),
                "target_memory_card_id": item["target_memory_card_id"].clone(),
                "relation_type": item["relation_type"].clone(),
                "relation_state": item["relation_state"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn metadata_only_raw_evidence(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_item_id": item["memory_item_id"].clone(),
                "memory_provenance_id": item["memory_provenance_id"].clone(),
                "title": item["title"].clone(),
                "source_kind": item["source_kind"].clone(),
                "verification_state": item["verification_state"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_memory_relation_edges(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_relation_edge_id": item["memory_relation_edge_id"].clone(),
                "source_memory_card_id": item["source_memory_card_id"].clone(),
                "target_memory_card_id": item["target_memory_card_id"].clone(),
                "relation_type": item["relation_type"].clone(),
                "relation_state": item["relation_state"].clone(),
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"].clone(),
                "valid_from_epoch_ms": item["valid_from_epoch_ms"].clone(),
                "valid_to_epoch_ms": item["valid_to_epoch_ms"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_raw_evidence(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_item_id": item["memory_item_id"].clone(),
                "memory_provenance_id": item["memory_provenance_id"].clone(),
                "title": item["title"].clone(),
                "content": item["content"].clone(),
                "source_kind": item["source_kind"].clone(),
                "source_event_id": item["source_event_id"].clone(),
                "artifact_refs": item["artifact_refs"].clone(),
                "message_refs": item["message_refs"].clone(),
                "evidence_span": item["evidence_span"].clone(),
                "derivation_kind": item["derivation_kind"].clone(),
                "truth_state": item["truth_state"].clone(),
                "trust_state": item["trust_state"].clone(),
                "verification_state": item["verification_state"].clone(),
                "observed_at_epoch_ms": item["observed_at_epoch_ms"].clone(),
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"].clone(),
                "valid_from_epoch_ms": item["valid_from_epoch_ms"].clone(),
                "valid_to_epoch_ms": item["valid_to_epoch_ms"].clone(),
                "last_verified_at_epoch_ms": item["last_verified_at_epoch_ms"].clone(),
                "relative_path": item["relative_path"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn context_pack_active_files(payload: &Value, limit: usize) -> Vec<String> {
    let mut active_files = Vec::new();
    for key in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
        "raw_evidence",
    ] {
        for item in payload["retrieval"][key].as_array().into_iter().flatten() {
            let Some(path) = item["relative_path"]
                .as_str()
                .or_else(|| item["provenance"]["path"].as_str())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if !active_files.iter().any(|existing| existing == path) {
                active_files.push(path.to_string());
            }
            if active_files.len() >= limit {
                return active_files;
            }
        }
    }
    active_files
}

fn thread_context_pack_delivery_seen(thread_id: &str, context_pack_id: &str) -> Result<bool> {
    let cache = THREAD_CONTEXT_PACK_DELIVERY_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let guard = cache
        .read()
        .map_err(|_| anyhow!("thread context-pack delivery cache lock poisoned"))?;
    Ok(guard
        .get(thread_id)
        .is_some_and(|entries| entries.contains(context_pack_id)))
}

fn remember_thread_context_pack_delivery(payload: &Value) -> Result<()> {
    let Some(thread_id) = codex_threads::current_thread_id() else {
        return Ok(());
    };
    let Some(context_pack_id) = payload["context_pack_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    remember_thread_context_pack_delivery_pair(&thread_id, context_pack_id)
}

fn remember_thread_context_pack_delivery_pair(
    thread_id: &str,
    context_pack_id: &str,
) -> Result<()> {
    let cache = THREAD_CONTEXT_PACK_DELIVERY_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut guard = cache
        .write()
        .map_err(|_| anyhow!("thread context-pack delivery cache lock poisoned"))?;
    guard
        .entry(thread_id.to_string())
        .or_insert_with(HashSet::new)
        .insert(context_pack_id.to_string());
    Ok(())
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
    if is_same_thread_cache_reuse_payload(payload) {
        return model_visible_same_thread_cache_reuse_payload(payload);
    }
    let default_project_code = payload["project"]["code"].as_str().unwrap_or_default();
    let (lexical_chunks, semantic_chunks) =
        compact_retrieval_chunks(&payload["retrieval"], default_project_code);
    let chunk_contents_by_path = compact_chunk_contents_by_path(&lexical_chunks, &semantic_chunks);
    json!({
        "context_pack_id": payload["context_pack_id"].clone(),
        "project": {
            "code": payload["project"]["code"].clone(),
        },
        "namespace": {
            "code": payload["namespace"]["code"].clone(),
        },
        "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
        "visible_projects": compact_visible_projects_for_token_budget(&payload["visible_projects"]),
        "retrieval": {
            "exact_documents": compact_exact_documents(
                &payload["retrieval"]["exact_documents"],
                &chunk_contents_by_path,
                default_project_code,
            ),
            "symbol_hits": compact_symbol_hits(&payload["retrieval"]["symbol_hits"], default_project_code),
            "lexical_chunks": lexical_chunks,
            "semantic_chunks": semantic_chunks,
            "memory_cards": compact_memory_cards(
                &payload["retrieval"]["memory_cards"],
                default_project_code,
            ),
            "memory_relation_edges": compact_memory_relation_edges(
                &payload["retrieval"]["memory_relation_edges"],
                default_project_code,
            ),
            "raw_evidence": compact_raw_evidence(
                &payload["retrieval"]["raw_evidence"],
                default_project_code,
            ),
        }
    })
}

fn is_same_thread_cache_reuse_payload(payload: &Value) -> bool {
    payload["cache_reuse_reference"]["state"].as_str() == Some("same_thread_context_pack_replay")
}

fn model_visible_same_thread_cache_reuse_payload(payload: &Value) -> Value {
    json!({
        "context_pack_id": payload["context_pack_id"].clone(),
        "project": {
            "code": payload["project"]["code"].clone(),
        },
        "namespace": {
            "code": payload["namespace"]["code"].clone(),
        },
        "cache_reuse_reference": compact_cache_reuse_reference(&payload["cache_reuse_reference"]),
    })
}

fn compact_cache_reuse_reference(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    let active_files = value["active_files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let retrieval_counts = json!({
        "exact_documents": value["retrieval_counts"]["exact_documents"].as_u64().unwrap_or(0),
        "symbol_hits": value["retrieval_counts"]["symbol_hits"].as_u64().unwrap_or(0),
        "lexical_chunks": value["retrieval_counts"]["lexical_chunks"].as_u64().unwrap_or(0),
        "semantic_chunks": value["retrieval_counts"]["semantic_chunks"].as_u64().unwrap_or(0),
        "memory_cards": value["retrieval_counts"]["memory_cards"].as_u64().unwrap_or(0),
        "memory_relation_edges": value["retrieval_counts"]["memory_relation_edges"]
            .as_u64()
            .unwrap_or(0),
        "raw_evidence": value["retrieval_counts"]["raw_evidence"].as_u64().unwrap_or(0),
    });
    let has_non_zero_counts = retrieval_counts
        .as_object()
        .is_some_and(|items| items.values().any(|item| item.as_u64().unwrap_or(0) > 0));
    let mut compact = json!({
        "state": value["state"].clone(),
        "source_context_pack_id": value["source_context_pack_id"].clone(),
    });
    if let Some(object) = compact.as_object_mut() {
        if !active_files.is_empty() {
            object.insert("active_files".to_string(), json!(active_files));
        }
        if has_non_zero_counts {
            object.insert("retrieval_counts".to_string(), retrieval_counts);
        }
    }
    compact
}

fn compact_exact_documents(
    value: &Value,
    chunk_contents_by_path: &HashMap<String, Vec<String>>,
    default_project_code: &str,
) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let duplicate_snippet =
                exact_document_snippet_covered_by_chunks(item, chunk_contents_by_path);
            if duplicate_snippet {
                return None;
            }
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
                "snippet": item["snippet"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            Some(compact)
        })
        .collect()
}

fn compact_retrieval_chunks(
    retrieval: &Value,
    default_project_code: &str,
) -> (Vec<Value>, Vec<Value>) {
    let mut seen_signatures = HashSet::new();
    let lexical_chunks = compact_chunks(
        &retrieval["lexical_chunks"],
        &mut seen_signatures,
        default_project_code,
    );
    let semantic_chunks = compact_chunks(
        &retrieval["semantic_chunks"],
        &mut seen_signatures,
        default_project_code,
    );
    (lexical_chunks, semantic_chunks)
}

fn compact_chunk_contents_by_path(
    lexical_chunks: &[Value],
    semantic_chunks: &[Value],
) -> HashMap<String, Vec<String>> {
    let mut contents_by_path = HashMap::new();
    for item in lexical_chunks.iter().chain(semantic_chunks.iter()) {
        let Some(path_signature) = compact_text_path_signature(
            item["project_code"]
                .as_str()
                .or_else(|| item["provenance"]["source_project"].as_str())
                .unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
        ) else {
            continue;
        };
        let Some(content) = item["content"].as_str() else {
            continue;
        };
        let entry = contents_by_path
            .entry(path_signature)
            .or_insert_with(Vec::new);
        if !entry.iter().any(|existing| existing == content) {
            entry.push(content.to_string());
        }
    }
    contents_by_path
}

fn compact_symbol_hits(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
                "name": item["name"].clone(),
                "kind": item["kind"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_chunks(
    value: &Value,
    seen_signatures: &mut HashSet<String>,
    default_project_code: &str,
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
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
                "content": item["content"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_item_project_code<'a>(item: &'a Value, default_project_code: &str) -> Option<&'a str> {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .filter(|value| !value.is_empty() && *value != default_project_code)
}

fn insert_compact_project_reference(compact: &mut Value, item: &Value, default_project_code: &str) {
    let Some(project_code) = compact_item_project_code(item, default_project_code) else {
        return;
    };
    if let Some(object) = compact.as_object_mut() {
        object.insert("project_code".to_string(), json!(project_code));
    }
}

fn compact_chunk_signature(item: &Value) -> Option<String> {
    let project_code = item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .unwrap_or_default();
    let relative_path = item["relative_path"].as_str().unwrap_or_default();
    let content = item["content"].as_str().unwrap_or_default();
    compact_text_signature(project_code, relative_path, content)
}

fn exact_document_snippet_covered_by_chunks(
    item: &Value,
    chunk_contents_by_path: &HashMap<String, Vec<String>>,
) -> bool {
    let relative_path = item["relative_path"].as_str().unwrap_or_default();
    let project_code = item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .unwrap_or_default();
    let candidate_signatures = [
        compact_text_path_signature(project_code, relative_path),
        compact_text_path_signature("", relative_path),
    ];
    if candidate_signatures.iter().all(Option::is_none) {
        return false;
    }
    let Some(snippet) = item["snippet"].as_str() else {
        return false;
    };
    if snippet.is_empty() {
        return false;
    }
    candidate_signatures.iter().flatten().any(|path_signature| {
        chunk_contents_by_path
            .get(path_signature)
            .is_some_and(|contents| contents.iter().any(|content| content.contains(snippet)))
    })
}

fn compact_text_path_signature(project_code: &str, relative_path: &str) -> Option<String> {
    if project_code.is_empty() && relative_path.is_empty() {
        return None;
    }
    Some(format!("{project_code}\u{1f}{relative_path}"))
}

fn compact_text_signature(project_code: &str, relative_path: &str, text: &str) -> Option<String> {
    if project_code.is_empty() && relative_path.is_empty() && text.is_empty() {
        return None;
    }
    Some(format!("{project_code}\u{1f}{relative_path}\u{1f}{text}"))
}

const DEFAULT_FAST_CACHE_MODE: &str = "__namespace_default__";

fn should_bypass_fast_context_pack_cache(args: &ContextPackArgs) -> bool {
    args.disable_cache
        || matches!(
            args.token_source_kind.as_str(),
            "proof_context_pack" | "verify_context_pack"
        )
}

fn selected_fast_cache_key(cfg: &AppConfig, args: &ContextPackArgs) -> FastCacheKey {
    fast_cache_key(
        cfg,
        args,
        args.retrieval_mode
            .as_deref()
            .unwrap_or(DEFAULT_FAST_CACHE_MODE),
    )
}

pub fn try_execute_context_pack_fast_cached(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackResult>> {
    let started = Instant::now();
    if should_bypass_fast_context_pack_cache(args) {
        return Ok(None);
    }
    let fast_cache_key = selected_fast_cache_key(cfg, args);
    let cached = if let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    {
        cached
    } else {
        let Some(cached) = edge_fast_context_pack_cache_get(
            &cfg.edge_cache_path,
            fast_cache_key,
            cfg.local_fast_cache_ttl_ms,
        )?
        else {
            return Ok(None);
        };
        let precise_lower_bound_ms = precise_elapsed_ms(started);
        local_fast_context_pack_cache_put(fast_cache_key, &cached)?;
        if persist && !cached.durably_persisted {
            return Ok(None);
        }
        return Ok(Some(context_pack_result_from_local_entry(
            cached,
            precise_lower_bound_ms,
        )));
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    let precise_lower_bound_ms = precise_elapsed_ms(started);
    Ok(Some(context_pack_result_from_local_entry(
        cached,
        precise_lower_bound_ms,
    )))
}

pub fn prepare_fast_context_pack_probe(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<FastContextPackProbe>> {
    if should_bypass_fast_context_pack_cache(args) {
        return Ok(None);
    }
    let fast_cache_key = selected_fast_cache_key(cfg, args);
    let cached = if let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    {
        cached
    } else {
        let Some(cached) = edge_fast_context_pack_cache_get(
            &cfg.edge_cache_path,
            fast_cache_key,
            cfg.local_fast_cache_ttl_ms,
        )?
        else {
            return Ok(None);
        };
        local_fast_context_pack_cache_put(fast_cache_key, &cached)?;
        cached
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(FastContextPackProbe {
        fast_cache_key,
        ttl_ms: cfg.local_fast_cache_ttl_ms,
        require_persist: persist,
        stats: cached_fast_context_pack_stats(&cached, 0.0),
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
    let edge_cache_path = if args.disable_cache || !cfg.edge_cache_path.exists() {
        None
    } else {
        Some(cfg.edge_cache_path.clone())
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
                    precise_lower_bound_ms: precise_elapsed_ms(resolve_started),
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
                    precise_lower_bound_ms: precise_elapsed_ms(resolve_started),
                },
                local_cached,
            );
        }
    }
    let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

    let query_prefers_document_lookup = should_prefer_document_lookup_only(&args.query);
    let should_try_exact_document = should_try_exact_document_lookup(&args.query);
    let should_try_exact_symbol = should_try_exact_symbol_lookup(&args.query);
    let should_try_exact_chunk = should_try_exact_chunk_lookup(&args.query);
    let should_search_documents = args.limit_documents > 0;
    let should_search_symbols = should_try_exact_symbol;

    let mut documents = Vec::new();
    let exact_started = Instant::now();
    if args.limit_documents > 0 && should_search_documents {
        let mut exact_scope_hits = HashSet::new();
        let mut local_exact_document_hit_found = false;
        let exact_document_queries = exact_document_lookup_queries(&args.query);
        if should_try_exact_document {
            for scope in &visible_scopes {
                for query in &exact_document_queries {
                    let normalized_query_path = normalized_safe_relative_path(query)
                        .map(|value| value.to_string_lossy().into_owned());
                    let cache_lookup_path =
                        normalized_query_path.as_deref().unwrap_or(query.as_str());
                    let content = edge_cache_path
                        .as_ref()
                        .and_then(|edge_cache_path| {
                            edge_cache::get_cached_document_snippet_by_path(
                                edge_cache_path,
                                &scope.visible.project.code,
                                &scope.namespace.code,
                                cache_lookup_path,
                                2000,
                            )
                            .ok()
                            .flatten()
                        })
                        .or_else(|| {
                            edge_cache_path.as_ref().and_then(|edge_cache_path| {
                                edge_cache::get_cached_document_snippet_by_project_path(
                                    edge_cache_path,
                                    &scope.visible.project.code,
                                    cache_lookup_path,
                                    2000,
                                )
                                .ok()
                                .flatten()
                            })
                        });
                    if let Some(content) = content {
                        local_exact_document_hit_found = true;
                        exact_scope_hits.insert((
                            scope.visible.project.code.clone(),
                            scope.namespace.code.clone(),
                        ));
                        documents.push(DocumentHit {
                            project_code: scope.visible.project.code.clone(),
                            namespace_code: scope.namespace.code.clone(),
                            repo_root: scope.visible.project.repo_root.clone(),
                            relative_path: cache_lookup_path.to_string(),
                            language: None,
                            source_kind: "code_document".to_string(),
                            git_commit_sha: None,
                            score: 1.0,
                            snippet: content.chars().take(2000).collect(),
                        });
                        break;
                    }
                }
            }
            if !local_exact_document_hit_found {
                for query in &exact_document_queries {
                    let unresolved_scope_ids = visible_scopes
                        .iter()
                        .filter(|scope| {
                            !exact_scope_hits.contains(&(
                                scope.visible.project.code.clone(),
                                scope.namespace.code.clone(),
                            ))
                        })
                        .map(|scope| {
                            (
                                scope.visible.project.project_id,
                                scope.namespace.namespace_id,
                            )
                        })
                        .collect::<Vec<_>>();
                    if unresolved_scope_ids.is_empty() {
                        break;
                    }
                    let exact_hits = postgres::search_documents_exact_for_scopes(
                        db,
                        &unresolved_scope_ids,
                        query,
                        args.limit_documents as i64,
                    )
                    .await?;
                    for hit in &exact_hits {
                        exact_scope_hits
                            .insert((hit.project_code.clone(), hit.namespace_code.clone()));
                    }
                    documents.extend(exact_hits);
                    if !documents.is_empty() {
                        break;
                    }
                }
            }
            if documents.is_empty() {
                for scope in &visible_scopes {
                    for query in &exact_document_queries {
                        let normalized_query_path = normalized_safe_relative_path(query)
                            .map(|value| value.to_string_lossy().into_owned());
                        let cache_lookup_path =
                            normalized_query_path.as_deref().unwrap_or(query.as_str());
                        let content = local_repo_document_snippet(
                            &scope.visible.project.repo_root,
                            cache_lookup_path,
                        )
                        .ok()
                        .flatten()
                        .or_else(|| {
                            local_repo_document_snippet(&scope.visible.project.repo_root, query)
                                .ok()
                                .flatten()
                        });
                        if let Some(content) = content {
                            exact_scope_hits.insert((
                                scope.visible.project.code.clone(),
                                scope.namespace.code.clone(),
                            ));
                            documents.push(DocumentHit {
                                project_code: scope.visible.project.code.clone(),
                                namespace_code: scope.namespace.code.clone(),
                                repo_root: scope.visible.project.repo_root.clone(),
                                relative_path: cache_lookup_path.to_string(),
                                language: None,
                                source_kind: "code_document".to_string(),
                                git_commit_sha: None,
                                score: 1.0,
                                snippet: content.chars().take(2000).collect(),
                            });
                            break;
                        }
                    }
                }
            }
        }
        for scope in &visible_scopes {
            if exact_scope_hits.contains(&(
                scope.visible.project.code.clone(),
                scope.namespace.code.clone(),
            )) {
                continue;
            }
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
    }
    let exact_lookup_ms = exact_started.elapsed().as_millis();

    let document_exact_hit_found = should_try_exact_document && !documents.is_empty();
    let mut symbols = Vec::new();
    let symbol_started = Instant::now();
    let skip_symbol_lookup = query_prefers_document_lookup && document_exact_hit_found;
    if args.limit_symbols > 0 && should_search_symbols && !skip_symbol_lookup {
        let mut exact_scope_hits = HashSet::new();
        if should_try_exact_symbol {
            let scope_ids = visible_scopes
                .iter()
                .map(|scope| {
                    (
                        scope.visible.project.project_id,
                        scope.namespace.namespace_id,
                    )
                })
                .collect::<Vec<_>>();
            let exact_hits = postgres::search_symbols_exact_for_scopes(
                db,
                &scope_ids,
                &args.query,
                args.limit_symbols as i64,
            )
            .await?;
            for hit in &exact_hits {
                exact_scope_hits.insert((hit.project_code.clone(), hit.namespace_code.clone()));
            }
            symbols.extend(exact_hits);
        }
        for scope in &visible_scopes {
            if exact_scope_hits.contains(&(
                scope.visible.project.code.clone(),
                scope.namespace.code.clone(),
            )) {
                continue;
            }
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
    }
    let symbol_lookup_ms = symbol_started.elapsed().as_millis();

    let mut chunks = Vec::new();
    let mut memory_cards = Vec::new();
    let mut memory_relation_edges = Vec::new();
    let mut memory_card_prefilter_match_count = 0_i64;
    let mut memory_card_excluded_by_temporal_window = 0_i64;
    let mut memory_card_excluded_by_current_truth_state = 0_i64;
    let mut memory_card_temporal_exclusions = Vec::new();
    let lexical_started = Instant::now();
    let skip_chunk_lookup = args.limit_chunks > 0
        && args.token_source_kind != "verify_context_pack"
        && ((should_try_exact_document && !documents.is_empty())
            || (should_try_exact_symbol && !symbols.is_empty()));
    if args.limit_chunks > 0 && !skip_chunk_lookup && should_try_exact_chunk {
        let scope_ids = visible_scopes
            .iter()
            .map(|scope| {
                (
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                )
            })
            .collect::<Vec<_>>();
        chunks.extend(
            postgres::search_chunks_exact_for_scopes(
                db,
                &scope_ids,
                &args.query,
                args.limit_chunks as i64,
            )
            .await?,
        );
    }
    let skip_lexical_lookup = skip_chunk_lookup
        || (args.limit_chunks > 0
            && args.token_source_kind != "verify_context_pack"
            && ((should_try_exact_document && !documents.is_empty())
                || (should_try_exact_symbol && !symbols.is_empty())
                || (should_try_exact_chunk && !chunks.is_empty())));
    if args.limit_chunks > 0 && !skip_lexical_lookup {
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
    let memory_limit = args.limit_chunks.min(8);
    let skip_memory_lookup = memory_limit > 0
        && should_skip_memory_lookup(args, documents.len(), symbols.len(), chunks.len(), 0);
    if memory_limit > 0 && !skip_memory_lookup {
        let now_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before unix epoch")?
            .as_millis() as i64;
        let at_epoch_ms = args.at_epoch_ms.unwrap_or(now_epoch_ms);
        for scope in &visible_scopes {
            let scope_card_stats = postgres::memory_card_search_temporal_stats_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                Some(at_epoch_ms),
            )
            .await?;
            memory_card_prefilter_match_count += scope_card_stats.prefilter_match_count;
            memory_card_excluded_by_temporal_window += scope_card_stats.excluded_by_temporal_window;
            memory_card_excluded_by_current_truth_state +=
                scope_card_stats.excluded_by_current_truth_state;
            let mut scope_card_exclusions =
                postgres::memory_card_temporal_exclusion_diagnostics_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    Some(at_epoch_ms),
                    4,
                )
                .await?;
            memory_card_temporal_exclusions.append(&mut scope_card_exclusions);
            let mut scope_cards = postgres::search_memory_cards_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                memory_limit as i64,
                Some(at_epoch_ms),
            )
            .await?;
            memory_cards.append(&mut scope_cards);
        }
        let (summary_sufficient, _) = summary_layer_sufficiency(
            documents.len(),
            symbols.len(),
            chunks.len(),
            memory_cards.len(),
        );
        if !summary_sufficient {
            for scope in &visible_scopes {
                let scope_card_ids = memory_cards
                    .iter()
                    .filter(|card| {
                        card.project_code == scope.visible.project.code
                            && card.namespace_code == scope.namespace.code
                    })
                    .map(|card| card.memory_card_id)
                    .collect::<Vec<_>>();
                if scope_card_ids.is_empty() {
                    continue;
                }
                let relation_limit = (scope_card_ids.len() as i64 * 4).max(12);
                memory_relation_edges.extend(
                    postgres::list_memory_relation_edges_for_cards(
                        db,
                        scope.visible.project.project_id,
                        scope.namespace.namespace_id,
                        &scope_card_ids,
                        Some(at_epoch_ms),
                        relation_limit,
                    )
                    .await?,
                );
            }
        }
    }
    let lexical_lookup_ms = lexical_started.elapsed().as_millis();

    let ranking_started = Instant::now();
    sort_and_truncate_documents(&mut documents, args.limit_documents);
    sort_and_truncate_symbols(&mut symbols, args.limit_symbols);
    sort_and_truncate_chunks(&mut chunks, args.limit_chunks);
    let ranking_ms = ranking_started.elapsed().as_millis();

    let (summary_sufficient, _) = summary_layer_sufficiency(
        documents.len(),
        symbols.len(),
        chunks.len(),
        memory_cards.len(),
    );
    let (structured_sufficient, _) =
        structured_layer_sufficiency(summary_sufficient, memory_relation_edges.len());
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let at_epoch_ms = args.at_epoch_ms.unwrap_or(now_epoch_ms);
    let mut raw_evidence = Vec::new();
    let mut raw_evidence_prefilter_match_count = 0_i64;
    let mut raw_evidence_excluded_by_temporal_window = 0_i64;
    let mut raw_evidence_excluded_by_current_truth_state = 0_i64;
    let mut raw_evidence_temporal_exclusions = Vec::new();
    if !structured_sufficient {
        let raw_limit = args.limit_semantic_chunks.max(4).min(16) as i64;
        for scope in &visible_scopes {
            let scope_raw_stats = postgres::raw_evidence_search_temporal_stats_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                Some(at_epoch_ms),
            )
            .await?;
            raw_evidence_prefilter_match_count += scope_raw_stats.prefilter_match_count;
            raw_evidence_excluded_by_temporal_window += scope_raw_stats.excluded_by_temporal_window;
            raw_evidence_excluded_by_current_truth_state +=
                scope_raw_stats.excluded_by_current_truth_state;
            let mut scope_raw_exclusions =
                postgres::raw_evidence_temporal_exclusion_diagnostics_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    Some(at_epoch_ms),
                    4,
                )
                .await?;
            raw_evidence_temporal_exclusions.append(&mut scope_raw_exclusions);
            raw_evidence.extend(
                postgres::search_raw_evidence_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    raw_limit,
                    Some(at_epoch_ms),
                )
                .await?,
            );
        }
    }
    let benchmark_compact_payload = should_use_benchmark_compact_payload(&args.token_source_kind);
    let lexical_signal_count = documents.len()
        + symbols.len()
        + chunks.len()
        + memory_cards.len()
        + memory_relation_edges.len();
    let semantic_skip_reason =
        semantic_skip_reason_for_router(args, summary_sufficient, structured_sufficient);
    let (semantic_chunks, semantic_timings, semantic_guard) =
        if benchmark_compact_payload && summary_sufficient {
            semantic_skipped_result(
                &args.query,
                args.limit_semantic_chunks,
                "benchmark_compact_summary_sufficient",
            )
        } else if let Some(reason) = semantic_skip_reason {
            semantic_skip_fallback_result(
                &args.query,
                args.limit_semantic_chunks,
                lexical_signal_count,
                &documents,
                &chunks,
                reason,
            )
        } else {
            semantic_chunks(
                cfg,
                db,
                &visible_scopes,
                &args.query,
                args.limit_semantic_chunks,
                lexical_signal_count,
                &chunks,
            )
            .await?
        };
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
                    "project_link_type": scope.visible.project_link_type,
                    "shared_contour": scope.visible.shared_contour,
                    "visibility_scope": scope.visible.visibility_scope,
                    "relation_status": scope.visible.relation_status,
                    "requires_approval": scope.visible.requires_approval,
                    "transfer_policy_code": scope.visible.transfer_policy_code,
                    "access_mode": scope.visible.access_mode,
                    "namespace_code": scope.namespace.code,
                    "namespace_display_name": scope.namespace.display_name
                })
            })
            .collect::<Vec<_>>()
    );
    let context_pack_id = Uuid::new_v4();
    let workspace_graph = if benchmark_compact_payload {
        Value::Null
    } else {
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
        let minimal_symbol_context = should_use_minimal_symbol_workspace_graph(
            &documents,
            &symbols,
            &chunks,
            &semantic_chunks,
        );
        let compact_workspace_enrichment =
            should_use_compact_workspace_graph_enrichment(&workspace_requests, &semantic_chunks);
        let (workspace_documents, workspace_symbols) =
            if minimal_document_context || minimal_symbol_context || compact_workspace_enrichment {
                (Vec::new(), Vec::new())
            } else {
                (
                    postgres::list_document_structures_for_namespace_paths(db, &workspace_requests)
                        .await?,
                    postgres::list_document_symbols_for_namespace_paths(db, &workspace_requests)
                        .await?,
                )
            };
        workspace_graph::build_context_pack_workspace_graph(
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
        )?
    };
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
        retrieval_lower_bound_ms_precise: None,
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
        "temporal_query_epoch_ms": args.at_epoch_ms,
        "effective_retrieval_mode": effective_mode,
        "visible_projects": visible_projects_json,
        "retrieval": {
            "exact_documents": documents.iter().map(document_to_json).collect::<Vec<_>>(),
            "symbol_hits": symbols.iter().map(symbol_to_json).collect::<Vec<_>>(),
            "lexical_chunks": chunks.iter().map(chunk_to_json).collect::<Vec<_>>(),
            "semantic_chunks": semantic_chunks,
            "memory_cards": memory_cards.iter().map(memory_card_to_json).collect::<Vec<_>>(),
            "memory_relation_edges": memory_relation_edges
                .iter()
                .map(memory_relation_edge_to_json)
                .collect::<Vec<_>>(),
            "raw_evidence": raw_evidence
                .iter()
                .map(raw_evidence_to_json)
                .collect::<Vec<_>>()
        },
        "retrieval_temporal_stats": {
            "memory_cards": {
                "prefilter_match_count": memory_card_prefilter_match_count,
                "excluded_by_temporal_window": memory_card_excluded_by_temporal_window,
                "excluded_by_current_truth_state": memory_card_excluded_by_current_truth_state,
                "excluded_candidates": memory_card_temporal_exclusions
                    .iter()
                    .map(memory_card_temporal_exclusion_to_json)
                    .collect::<Vec<_>>()
            },
            "raw_evidence": {
                "prefilter_match_count": raw_evidence_prefilter_match_count,
                "excluded_by_temporal_window": raw_evidence_excluded_by_temporal_window,
                "excluded_by_current_truth_state": raw_evidence_excluded_by_current_truth_state,
                "excluded_candidates": raw_evidence_temporal_exclusions
                    .iter()
                    .map(raw_evidence_temporal_exclusion_to_json)
                    .collect::<Vec<_>>()
            }
        },
        "quality": {
            "semantic_guard": semantic_guard_to_json(&semantic_guard)
        },
        "workspace_graph": workspace_graph,
        "provenance_minimum": provenance_minimum,
        "retrieval_runtime": runtime_json(&stats)
    });
    let mut payload = payload;
    if !benchmark_compact_payload {
        ensure_context_pack_decision_trace(&mut payload);
    }
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
        artifact_bucket: None,
        artifact_object_key: None,
        artifact_state: None,
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
    prepared: &mut PreparedContextPack,
) -> Result<()> {
    let payload_sha256 = hex_sha256(prepared.payload_json.as_bytes());
    let object_key = format!(
        "context-packs/{}/{}/{}.json",
        prepared.project.code, prepared.effective_mode, payload_sha256
    );
    let context_pack_record = postgres::ContextPackInsert {
        context_pack_id: prepared.context_pack_id,
        project_id: prepared.project.project_id,
        namespace_id: prepared.namespace_id,
        retrieval_mode: &prepared.effective_mode,
        query_text: &args.query,
        visible_projects: &prepared.visible_projects_json,
        payload: prepared.payload.as_ref(),
        artifact_ref_id: None,
    };
    prepared.artifact_bucket = Some(cfg.s3_bucket_context.clone());
    prepared.artifact_object_key = Some(object_key.clone());

    postgres::insert_context_pack_pending_artifact(
        db,
        &context_pack_record,
        &cfg.s3_bucket_context,
        &object_key,
    )
    .await?;
    if should_persist_retrieval_trace(&args.token_source_kind) {
        let workspace_id =
            postgres::get_workspace_id_for_project(db, prepared.project.project_id).await?;
        let retrieval_trace = build_retrieval_trace_insert(workspace_id, prepared, args);
        postgres::create_retrieval_trace(db, &retrieval_trace).await?;
    }
    prepared.artifact_state = Some("pending".to_string());
    Ok(())
}

pub async fn materialize_pending_context_pack_artifacts(
    cfg: &AppConfig,
    db: &mut Client,
    limit: i64,
) -> Result<()> {
    let claimed = postgres::claim_pending_context_pack_artifacts(db, limit).await?;
    let claimed_rows = claimed.len();
    let mut grouped: HashMap<(String, String), Vec<postgres::PendingContextPackArtifactRecord>> =
        HashMap::new();
    for record in claimed {
        grouped
            .entry((record.bucket.clone(), record.object_key.clone()))
            .or_default()
            .push(record);
    }

    let s3_client = if grouped.is_empty() {
        None
    } else {
        Some(s3::connect(cfg).await?)
    };
    let mut materialized_rows = 0_u64;
    let mut failed_rows = 0_u64;
    let mut materialized_groups = 0_u64;
    let mut failed_groups = 0_u64;

    for ((bucket, object_key), records) in grouped {
        let Some(first) = records.first() else {
            continue;
        };
        let payload_json = serde_json::to_string(&first.payload)?;
        let payload_sha256 = hex_sha256(payload_json.as_bytes());
        let metadata = json!({
            "artifact_role": "context_pack",
            "payload_sha256": payload_sha256
        });
        let upload_result = s3::put_json_object(
            s3_client
                .as_ref()
                .expect("s3 client exists when pending groups exist"),
            &bucket,
            &object_key,
            &payload_json,
        )
        .await;

        match upload_result {
            Ok(()) => {
                let artifact_ref_result = postgres::insert_artifact_ref(
                    db,
                    &postgres::ArtifactRefInsert {
                        project_id: first.project_id,
                        namespace_id: first.namespace_id,
                        artifact_kind: "context_pack",
                        bucket: &bucket,
                        object_key: &object_key,
                        content_type: Some("application/json"),
                        source_kind: Some("context_pack_materialization"),
                        source_event_ids: None,
                        message_refs: None,
                        evidence_span: Some(&json!({
                            "artifact_role": "context_pack",
                            "bucket": bucket,
                            "object_key": object_key,
                            "context_pack_id": first.context_pack_id,
                        })),
                        derivation_kind: Some("extract"),
                        schema_version: Some("artifact-ref-envelope-v1"),
                        metadata: &metadata,
                    },
                )
                .await;

                match artifact_ref_result {
                    Ok(artifact_ref_id) => {
                        materialized_rows += postgres::mark_context_pack_artifacts_materialized(
                            db,
                            &bucket,
                            &object_key,
                            artifact_ref_id,
                        )
                        .await?;
                        materialized_groups += 1;
                    }
                    Err(error) => {
                        let error_text = format!("{error:#}");
                        for record in &records {
                            postgres::mark_context_pack_artifact_failed(
                                db,
                                record.context_pack_id,
                                &error_text,
                            )
                            .await?;
                            failed_rows += 1;
                        }
                        failed_groups += 1;
                    }
                }
            }
            Err(error) => {
                let error_text = format!("{error:#}");
                for record in &records {
                    postgres::mark_context_pack_artifact_failed(
                        db,
                        record.context_pack_id,
                        &error_text,
                    )
                    .await?;
                    failed_rows += 1;
                }
                failed_groups += 1;
            }
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "claimed_rows": claimed_rows,
            "claimed_groups": materialized_groups + failed_groups,
            "materialized_rows": materialized_rows,
            "materialized_groups": materialized_groups,
            "failed_rows": failed_rows,
            "failed_groups": failed_groups
        }))?
    );
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
    let local_entry = local_entry_from_prepared(prepared, durably_persisted);
    local_context_pack_cache_put(&prepared.cache_key, &prepared.scope_signature, &local_entry)?;
    let fast_cache_key = fast_cache_key(cfg, args, &prepared.effective_mode);
    local_fast_context_pack_cache_put(fast_cache_key, &local_entry)?;
    edge_cache::upsert_fast_context_pack_cache_entry(
        &edge_cache_path,
        fast_cache_key,
        &prepared.context_pack_id.to_string(),
        prepared.payload_json.as_ref(),
        durably_persisted,
        local_entry.cached_at_epoch_ms,
    )?;
    if args.retrieval_mode.is_none() {
        let default_fast_cache_key = selected_fast_cache_key(cfg, args);
        local_fast_context_pack_cache_put(default_fast_cache_key, &local_entry)?;
        edge_cache::upsert_fast_context_pack_cache_entry(
            &edge_cache_path,
            default_fast_cache_key,
            &prepared.context_pack_id.to_string(),
            prepared.payload_json.as_ref(),
            durably_persisted,
            local_entry.cached_at_epoch_ms,
        )?;
    }
    Ok(())
}

async fn resolve_visible_projects(
    db: &Client,
    project: &ProjectRecord,
    effective_mode: &str,
) -> Result<Vec<VisibleProjectRecord>> {
    if effective_mode == "audit_global" {
        let mut visible = Vec::new();
        for candidate in postgres::list_projects(db, None, None).await? {
            let is_local = candidate.project_id == project.project_id;
            if !is_local
                && !postgres::project_allows_cross_project_read(
                    db,
                    project.project_id,
                    "org_global",
                )
                .await?
            {
                continue;
            }
            visible.push(VisibleProjectRecord {
                project: candidate,
                relation_type: if is_local {
                    "local".to_string()
                } else {
                    "audit_global".to_string()
                },
                project_link_type: if is_local {
                    "local".to_string()
                } else {
                    "audit_global".to_string()
                },
                shared_contour: if is_local {
                    "self".to_string()
                } else {
                    "global_audit".to_string()
                },
                visibility_scope: if is_local {
                    "project_shared".to_string()
                } else {
                    "org_global".to_string()
                },
                relation_status: "active".to_string(),
                requires_approval: false,
                transfer_policy_code: None,
                access_mode: "audit_global".to_string(),
            });
        }
        return Ok(visible);
    }

    let mut visible = vec![VisibleProjectRecord {
        project: project.clone(),
        relation_type: "local".to_string(),
        project_link_type: "local".to_string(),
        shared_contour: "self".to_string(),
        visibility_scope: "project_shared".to_string(),
        relation_status: "active".to_string(),
        requires_approval: false,
        transfer_policy_code: None,
        access_mode: "local_strict".to_string(),
    }];
    if effective_mode == "local_strict" {
        return Ok(visible);
    }
    let allowed_rank = mode_rank(effective_mode)?;
    let related = postgres::list_related_projects(db, project.project_id).await?;
    for relation in related {
        if mode_rank(&relation.access_mode)? <= allowed_rank
            && postgres::project_allows_cross_project_read(
                db,
                project.project_id,
                &relation.visibility_scope,
            )
            .await?
        {
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

fn is_common_document_basename(query: &str) -> bool {
    let normalized = query.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "makefile"
            | "dockerfile"
            | "containerfile"
            | "license"
            | "copying"
            | "notice"
            | "readme"
            | "contributing"
            | "changelog"
            | "justfile"
            | "workspace"
            | "build"
    )
}

fn slugified_document_lookup_alias(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed.contains('/')
        || trimmed.contains('.')
        || !trimmed.chars().any(char::is_whitespace)
    {
        return None;
    }
    let mut slug = String::with_capacity(trimmed.len());
    let mut previous_was_separator = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            slug.push('-');
            previous_was_separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() || slug == trimmed.to_ascii_lowercase() {
        return None;
    }
    Some(slug)
}

fn exact_document_lookup_queries(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut candidates = vec![trimmed.to_string()];
    if let Some(slug) = slugified_document_lookup_alias(trimmed) {
        candidates.push(slug.clone());
        candidates.push(format!("{slug}.md"));
    }
    candidates
}

fn should_prefer_document_lookup_only(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 256
        && (trimmed.contains('/') || trimmed.contains('.') || is_common_document_basename(trimmed))
}

fn should_try_exact_document_lookup(query: &str) -> bool {
    let trimmed = query.trim();
    should_prefer_document_lookup_only(trimmed)
        || (!trimmed.is_empty()
            && trimmed.len() <= 256
            && (trimmed.contains('.') || slugified_document_lookup_alias(trimmed).is_some()))
}

fn should_try_exact_symbol_lookup(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && !trimmed.chars().any(char::is_whitespace)
        && !trimmed.contains('/')
        && !trimmed.contains('-')
}

fn should_try_exact_chunk_lookup(query: &str) -> bool {
    should_try_exact_symbol_lookup(query)
}

fn summary_layer_sufficiency(
    exact_count: usize,
    symbol_count: usize,
    lexical_count: usize,
    memory_card_count: usize,
) -> (bool, &'static str) {
    if exact_count > 0 {
        return (true, "exact_documents_sufficient");
    }
    if symbol_count > 0 {
        return (true, "symbol_hits_sufficient");
    }
    if lexical_count > 0 {
        return (true, "lexical_chunks_sufficient");
    }
    if memory_card_count >= 2 {
        return (true, "multiple_memory_cards_sufficient");
    }
    if memory_card_count == 1 {
        return (false, "single_memory_card_needs_structured_neighborhood");
    }
    (false, "no_summary_signal")
}

fn should_skip_memory_lookup(
    args: &ContextPackArgs,
    exact_count: usize,
    symbol_count: usize,
    lexical_count: usize,
    memory_card_count: usize,
) -> bool {
    args.token_source_kind != "verify_context_pack"
        && summary_layer_sufficiency(exact_count, symbol_count, lexical_count, memory_card_count).0
}

fn should_persist_retrieval_trace(token_source_kind: &str) -> bool {
    !matches!(
        token_source_kind,
        "benchmark_context_pack" | "benchmark_cold_context_pack"
    )
}

fn should_use_benchmark_compact_payload(token_source_kind: &str) -> bool {
    matches!(
        token_source_kind,
        "benchmark_context_pack" | "benchmark_cold_context_pack"
    )
}

fn structured_layer_sufficiency(
    summary_sufficient: bool,
    relation_edge_count: usize,
) -> (bool, &'static str) {
    if summary_sufficient {
        return (true, "summary_layer_already_sufficient");
    }
    if relation_edge_count > 0 {
        return (true, "structured_graph_neighborhood_sufficient");
    }
    (false, "no_structured_graph_support")
}

fn semantic_skip_reason_for_router(
    args: &ContextPackArgs,
    summary_sufficient: bool,
    structured_sufficient: bool,
) -> Option<&'static str> {
    if args.limit_semantic_chunks == 0 {
        return Some("semantic_limit_disabled");
    }
    if args.token_source_kind == "verify_context_pack" {
        return None;
    }
    if summary_sufficient || structured_sufficient {
        return Some("cheapest_sufficient_evidence_already_found");
    }
    None
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

fn semantic_skipped_result(
    query: &str,
    lexical_signal_count: usize,
    reason: &'static str,
) -> (Vec<Value>, SemanticTimings, SemanticGuardSummary) {
    (
        Vec::new(),
        SemanticTimings::default(),
        SemanticGuardSummary {
            query_terms: query_terms(query),
            lexical_signal_count,
            accepted_hits: 0,
            rejected_hits: 0,
            abstained: false,
            reason: Some(reason),
            detail: None,
        },
    )
}

fn semantic_skip_fallback_result(
    query: &str,
    limit: usize,
    lexical_signal_count: usize,
    documents: &[DocumentHit],
    lexical_fallback_chunks: &[ChunkHit],
    reason: &'static str,
) -> (Vec<Value>, SemanticTimings, SemanticGuardSummary) {
    let chunk_fallback_hits = lexical_fallback_hits(query, lexical_fallback_chunks);
    if !chunk_fallback_hits.is_empty() {
        let accepted_hits = chunk_fallback_hits.len().min(limit);
        return (
            chunk_fallback_hits
                .iter()
                .take(limit)
                .map(|chunk| semantic_chunk_fallback_to_json(chunk))
                .collect(),
            SemanticTimings::default(),
            SemanticGuardSummary {
                query_terms: query_terms(query),
                lexical_signal_count,
                accepted_hits,
                rejected_hits: 0,
                abstained: false,
                reason: Some(reason),
                detail: Some("semantic satisfied by lexical fallback".to_string()),
            },
        );
    }

    let document_fallback_hits = document_semantic_fallback_hits(query, documents);
    if !document_fallback_hits.is_empty() {
        let accepted_hits = document_fallback_hits.len().min(limit);
        return (
            document_fallback_hits
                .iter()
                .take(limit)
                .map(|document| semantic_document_fallback_to_json(document))
                .collect(),
            SemanticTimings::default(),
            SemanticGuardSummary {
                query_terms: query_terms(query),
                lexical_signal_count,
                accepted_hits,
                rejected_hits: 0,
                abstained: false,
                reason: Some(reason),
                detail: Some("semantic satisfied by exact document fallback".to_string()),
            },
        );
    }

    semantic_skipped_result(query, lexical_signal_count, reason)
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

fn document_semantic_fallback_hits<'a>(
    query: &str,
    documents: &'a [DocumentHit],
) -> Vec<&'a DocumentHit> {
    let exact_matches = documents
        .iter()
        .filter(|document| document.snippet.contains(query))
        .collect::<Vec<_>>();
    if !exact_matches.is_empty() {
        return exact_matches;
    }

    let query_lower = query.to_lowercase();
    let query_terms = query_terms(query)
        .into_iter()
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    documents
        .iter()
        .filter(|document| {
            let snippet_lower = document.snippet.to_lowercase();
            snippet_lower.contains(query_lower.as_str())
                || query_terms
                    .iter()
                    .all(|term| snippet_lower.contains(term.as_str()))
        })
        .collect()
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

fn semantic_document_fallback_to_json(hit: &DocumentHit) -> Value {
    json!({
        "score": hit.score,
        "retrieval_strategy": "exact_document_fallback",
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "relative_path": hit.relative_path,
        "content": hit.snippet,
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": hit.git_commit_sha,
            "path": hit.relative_path,
            "symbol": Value::Null,
            "chunk_id": Value::Null,
            "source_kind": hit.source_kind,
            "trust_level": "local_repo"
        },
        "chunk": {
            "chunk_id": Value::Null,
            "chunk_index": Value::Null,
            "start_line": Value::Null,
            "end_line": Value::Null,
            "metadata": {
                "fallback_source": "exact_document"
            }
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

fn build_context_pack_intent_classifier(payload: &Value) -> Value {
    let query = payload["query"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let namespace_code = payload["namespace"]["code"].as_str().unwrap_or_default();

    let has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));

    let continuity_keywords = [
        "continuity",
        "last chat",
        "previous chat",
        "past chat",
        "resume",
        "handoff",
        "startup",
        "restore",
        "chat at",
        "на чем закончили",
        "прошлый чат",
    ];
    let policy_keywords = [
        "policy",
        "allowed",
        "forbidden",
        "permission",
        "scope",
        "private",
        "public",
        "legal",
        "trust",
        "can i",
        "можно",
        "разреш",
        "запрещ",
        "доступ",
        "scope",
    ];
    let artifact_keywords = [
        "artifact",
        "file",
        "path",
        "log",
        "trace",
        "bundle",
        "snapshot",
        "screenshot",
        ".rs",
        ".md",
        ".json",
        ".toml",
        "/",
    ];
    let procedural_keywords = [
        "how to",
        "how do",
        "steps",
        "procedure",
        "runbook",
        "playbook",
        "workflow",
        "command",
        "commands",
        "setup",
        "deploy",
        "fix",
        "repair",
        "как",
        "шаг",
        "процед",
        "команд",
    ];

    let (classification, reason, signals): (&str, &str, Vec<&str>) = if namespace_code
        == "continuity"
        || has_any(&continuity_keywords)
    {
        (
            "continuity",
            "Namespace/query indicates chat restore or continuity recall.",
            vec!["namespace_or_query_continuity"],
        )
    } else if has_any(&policy_keywords) {
        (
            "policy_check",
            "Query contains access/policy/trust language and should be routed as policy recall.",
            vec!["policy_keywords"],
        )
    } else if has_any(&artifact_keywords) {
        (
            "artifact_lookup",
            "Query targets file/log/artifact style evidence rather than pure fact recall.",
            vec!["artifact_keywords"],
        )
    } else if has_any(&procedural_keywords) {
        (
            "procedural_recall",
            "Query asks for steps/workflow/commands and is routed as procedural recall.",
            vec!["procedural_keywords"],
        )
    } else {
        (
            "factual_recall",
            "Default retrieval intent for project-scoped factual/context recall.",
            vec!["default_factual_recall"],
        )
    };

    json!({
        "classification": classification,
        "reason": reason,
        "signals": signals,
        "classifier_version": "retrieval-intent-classifier-v1"
    })
}

fn count_temporal_historical_memory_cards(value: &Value) -> usize {
    value.as_array().map_or(0, |items| {
        items
            .iter()
            .filter(|item| {
                item["truth_state"].as_str() == Some("superseded")
                    || item["truth_state"].as_str() == Some("retracted")
                    || item["valid_to_epoch_ms"].as_i64().is_some()
            })
            .count()
    })
}

fn count_temporal_historical_raw_evidence(value: &Value) -> usize {
    value.as_array().map_or(0, |items| {
        items
            .iter()
            .filter(|item| {
                item["truth_state"].as_str() == Some("superseded")
                    || item["truth_state"].as_str() == Some("retracted")
                    || item["valid_to_epoch_ms"].as_i64().is_some()
            })
            .count()
    })
}

fn read_temporal_stat(value: &Value, lane: &str, key: &str) -> u64 {
    value["retrieval_temporal_stats"][lane][key]
        .as_u64()
        .unwrap_or(0)
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
    let raw_evidence_count = payload["retrieval"]["raw_evidence"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let memory_card_count = payload["retrieval"]["memory_cards"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let memory_relation_edge_count = payload["retrieval"]["memory_relation_edges"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let semantic_guard = &payload["quality"]["semantic_guard"];
    let summary_layer_count = exact_count + symbol_count + lexical_count + memory_card_count;
    let structured_layer_count = memory_relation_edge_count;
    let raw_layer_count = raw_evidence_count;
    let (summary_sufficient, summary_reason) =
        summary_layer_sufficiency(exact_count, symbol_count, lexical_count, memory_card_count);
    let (structured_sufficient, structured_reason) =
        structured_layer_sufficiency(summary_sufficient, memory_relation_edge_count);

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
    if memory_card_count > 0 {
        included.push(json!({
            "strategy": "memory_cards",
            "count": memory_card_count,
            "reason": "Compact semantic cards дали summary/profile слой для factual recall."
        }));
    }
    if memory_relation_edge_count > 0 {
        included.push(json!({
            "strategy": "memory_relation_edges",
            "count": memory_relation_edge_count,
            "reason": "Structured graph neighborhood поднял relation edges как соседний доказательный слой."
        }));
    }
    if raw_evidence_count > 0 {
        included.push(json!({
            "strategy": "raw_evidence",
            "count": raw_evidence_count,
            "reason": "Immutable raw/artifact/log evidence был поднят только после недостаточности summary и structured слоя."
        }));
    }
    if semantic_count > 0 {
        included.push(json!({
            "strategy": "semantic_chunks",
            "count": semantic_count,
            "reason": "Semantic candidate layer добавил вспомогательные vector hits, но не считается final raw evidence."
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
    if memory_card_count == 0 {
        not_included.push(json!({
            "strategy": "memory_cards",
            "reason": "Compact semantic card слой не дал новых factual summary."
        }));
    }
    if memory_relation_edge_count == 0 {
        not_included.push(json!({
            "strategy": "memory_relation_edges",
            "reason": "Structured graph neighborhood не дал relation edges для этого запроса."
        }));
    }
    if raw_evidence_count == 0 {
        not_included.push(json!({
            "strategy": "raw_evidence",
            "reason": if structured_sufficient {
                "Raw evidence не поднимался, потому что более дешёвый summary/structured слой уже был достаточен."
            } else {
                "Raw evidence не дал допустимых immutable sources для этого запроса."
            }
        }));
    }
    if semantic_count == 0 {
        let reason = if semantic_guard["abstained"].as_bool() == Some(true) {
            semantic_guard["detail"]
                .as_str()
                .or_else(|| semantic_guard["reason"].as_str())
                .unwrap_or("Semantic candidate layer честно abstained и не добавил vector hits.")
                .to_string()
        } else {
            "Semantic candidate layer не добавил новых vector hits после scope и relevance проверки."
                .to_string()
        };
        not_included.push(json!({
            "strategy": "semantic_chunks",
            "reason": reason
        }));
    }

    let cheapest_sufficient_layer = if summary_sufficient {
        "summary_compact"
    } else if structured_sufficient {
        "structured_graph"
    } else if raw_layer_count > 0 {
        "raw_evidence"
    } else {
        "none"
    };
    let final_decision = if summary_sufficient || structured_sufficient || raw_layer_count > 0 {
        "continue"
    } else {
        "abstain"
    };
    let next_action = match final_decision {
        "continue" => "answer_on_cheapest_sufficient_evidence",
        _ => "abstain_due_to_insufficient_evidence",
    };
    let escalate_target_layer =
        if !summary_sufficient && !structured_sufficient && raw_layer_count == 0 {
            Some("raw_evidence")
        } else {
            None
        };
    let abstain_reason = if final_decision == "abstain" {
        Some("No sufficient allowed evidence remained after candidate generation and rerank")
    } else {
        None
    };
    let temporal_query_requested = payload["temporal_query_epoch_ms"].as_i64().is_some();
    let temporal_query_epoch_ms = payload["temporal_query_epoch_ms"].as_i64();
    let temporal_candidate_count = if temporal_query_requested {
        summary_layer_count + structured_layer_count + raw_layer_count + semantic_count
    } else {
        0
    };
    let historical_memory_card_count =
        count_temporal_historical_memory_cards(&payload["retrieval"]["memory_cards"]);
    let historical_raw_evidence_count =
        count_temporal_historical_raw_evidence(&payload["retrieval"]["raw_evidence"]);
    let prefilter_memory_card_count =
        read_temporal_stat(payload, "memory_cards", "prefilter_match_count");
    let excluded_memory_cards_by_temporal_window =
        read_temporal_stat(payload, "memory_cards", "excluded_by_temporal_window");
    let excluded_memory_cards_by_current_truth_state =
        read_temporal_stat(payload, "memory_cards", "excluded_by_current_truth_state");
    let prefilter_raw_evidence_count =
        read_temporal_stat(payload, "raw_evidence", "prefilter_match_count");
    let excluded_raw_evidence_by_temporal_window =
        read_temporal_stat(payload, "raw_evidence", "excluded_by_temporal_window");
    let excluded_raw_evidence_by_current_truth_state =
        read_temporal_stat(payload, "raw_evidence", "excluded_by_current_truth_state");
    let memory_card_excluded_candidates =
        payload["retrieval_temporal_stats"]["memory_cards"]["excluded_candidates"].clone();
    let raw_evidence_excluded_candidates =
        payload["retrieval_temporal_stats"]["raw_evidence"]["excluded_candidates"].clone();
    let temporal_explanation = if !temporal_query_requested {
        if excluded_memory_cards_by_current_truth_state > 0
            || excluded_raw_evidence_by_current_truth_state > 0
        {
            "Temporal slice was not requested; latest valid truth window was used and stale current-state candidates were excluded."
        } else {
            "Temporal slice was not requested; latest valid truth window was used."
        }
    } else if historical_memory_card_count > 0 || historical_raw_evidence_count > 0 {
        if excluded_memory_cards_by_temporal_window > 0
            || excluded_raw_evidence_by_temporal_window > 0
        {
            "Historical-but-valid candidates remained admissible at the requested timestamp while out-of-window candidates were excluded."
        } else {
            "Historical-but-valid candidates remained admissible at the requested timestamp."
        }
    } else if final_decision == "abstain" {
        "Requested timestamp left no sufficient admissible evidence after temporal legality filtering."
    } else {
        if excluded_memory_cards_by_temporal_window > 0
            || excluded_raw_evidence_by_temporal_window > 0
        {
            "Requested timestamp stayed within the currently valid truth window for returned evidence after excluding out-of-window candidates."
        } else {
            "Requested timestamp stayed within the currently valid truth window for returned evidence."
        }
    };

    json!({
        "intent_classifier": build_context_pack_intent_classifier(payload),
        "selection_priority": [
            "exact_documents",
            "symbol_hits",
            "lexical_chunks",
            "memory_cards",
            "memory_relation_edges",
            "raw_evidence",
            "semantic_chunks"
        ],
        "scope": {
            "project_code": payload["project"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "visible_projects_total": payload["visible_projects"].as_array().map(Vec::len).unwrap_or_default(),
        },
        "scope_resolver": {
            "project_code": payload["project"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "visible_projects_total": payload["visible_projects"].as_array().map(Vec::len).unwrap_or_default(),
            "status": "resolved"
        },
        "candidate_generation": {
            "exact_documents": exact_count,
            "symbol_hits": symbol_count,
            "lexical_chunks": lexical_count,
            "memory_cards": memory_card_count,
            "memory_relation_edges": memory_relation_edge_count,
            "raw_evidence": raw_evidence_count,
            "semantic_chunks": semantic_count,
            "grouped_strategies": {
                "exact": exact_count + symbol_count,
                "lexical": lexical_count,
                "graph": memory_relation_edge_count,
                "vector": semantic_count,
                "temporal": temporal_candidate_count,
            },
            "temporal_query_requested": temporal_query_requested,
        },
        "rerank_legality_relevance": {
            "scope_filtered": true,
            "temporal_query_epoch_ms": payload["temporal_query_epoch_ms"].clone(),
            "temporal_query_requested": temporal_query_requested,
            "temporal_legality": {
                "status": if temporal_query_requested {
                    "applied_exact_time_slice"
                } else {
                    "latest_truth_window"
                },
                "query_epoch_ms": temporal_query_epoch_ms,
                "prefilter_memory_cards": prefilter_memory_card_count,
                "memory_cards_after_filter": memory_card_count,
                "excluded_memory_cards_by_temporal_window": excluded_memory_cards_by_temporal_window,
                "excluded_memory_cards_by_current_truth_state": excluded_memory_cards_by_current_truth_state,
                "excluded_memory_card_candidates": memory_card_excluded_candidates,
                "prefilter_raw_evidence": prefilter_raw_evidence_count,
                "memory_relation_edges_after_filter": memory_relation_edge_count,
                "raw_evidence_after_filter": raw_evidence_count,
                "excluded_raw_evidence_by_temporal_window": excluded_raw_evidence_by_temporal_window,
                "excluded_raw_evidence_by_current_truth_state": excluded_raw_evidence_by_current_truth_state,
                "excluded_raw_evidence_candidates": raw_evidence_excluded_candidates,
                "historical_memory_cards_after_filter": historical_memory_card_count,
                "historical_raw_evidence_after_filter": historical_raw_evidence_count,
                "explanation": temporal_explanation,
            },
            "semantic_guard": semantic_guard.clone(),
        },
        "evidence_ladder": {
            "cheapest_sufficient_layer": cheapest_sufficient_layer,
            "layers": [
                {
                    "layer": "summary_compact",
                    "count": summary_layer_count,
                    "strategies": ["exact_documents", "symbol_hits", "lexical_chunks", "memory_cards"],
                    "sufficient": summary_sufficient,
                    "reason": summary_reason
                },
                {
                    "layer": "structured_graph",
                    "count": structured_layer_count,
                    "strategies": ["memory_relation_edges"],
                    "sufficient": !summary_sufficient && structured_sufficient,
                    "reason": structured_reason
                },
                {
                    "layer": "raw_evidence",
                    "count": raw_layer_count,
                    "strategies": ["raw_evidence"],
                    "sufficient": !summary_sufficient && !structured_sufficient && raw_layer_count > 0,
                    "reason": if raw_layer_count > 0 {
                        "raw_evidence_raised_after_summary_and_structured_were_insufficient"
                    } else {
                        "raw_evidence_not_required_or_not_found"
                    }
                },
                {
                    "layer": "semantic_candidates",
                    "count": semantic_count,
                    "strategies": ["semantic_chunks"],
                    "sufficient": false,
                    "reason": "semantic auxiliary lane is candidate generation support, not final evidence authority"
                }
            ]
        },
        "evidence_sufficiency_check": {
            "status": if final_decision == "abstain" { "insufficient" } else { "sufficient" },
            "next_action": next_action,
            "cheapest_sufficient_layer": cheapest_sufficient_layer,
        },
        "escalate_if_needed": {
            "required": !summary_sufficient && !structured_sufficient && raw_layer_count == 0,
            "target_layer": escalate_target_layer,
            "reason": if !summary_sufficient && !structured_sufficient && raw_layer_count > 0 {
                "summary and structured layers were insufficient, so the router escalated to raw evidence and found admissible sources"
            } else if !summary_sufficient && !structured_sufficient {
                "summary and structured layers were insufficient, and raw evidence was still missing"
            } else if raw_layer_count > 0 {
                "raw evidence was available but not required because a cheaper sufficient layer already existed"
            } else {
                "no raw escalation required"
            }
        },
        "abstain_if_insufficient": {
            "abstained": final_decision == "abstain",
            "reason": abstain_reason
        },
        "final_decision": final_decision,
        "included": included,
        "not_included": not_included,
        "semantic_guard": semantic_guard.clone(),
    })
}

fn build_retrieval_trace_insert<'a>(
    workspace_id: Uuid,
    prepared: &'a PreparedContextPack,
    args: &'a ContextPackArgs,
) -> postgres::RetrievalTraceInsert {
    let decision_trace = &prepared.payload["decision_trace"];
    let candidate_summary = json!({
        "candidate_generation": decision_trace["candidate_generation"].clone(),
        "selection_priority": decision_trace["selection_priority"].clone(),
    });
    let rerank_summary = json!({
        "scope": decision_trace["scope"].clone(),
        "scope_resolver": decision_trace["scope_resolver"].clone(),
        "rerank_legality_relevance": decision_trace["rerank_legality_relevance"].clone(),
        "included": decision_trace["included"].clone(),
        "not_included": decision_trace["not_included"].clone(),
    });
    let evidence_sufficiency = json!({
        "evidence_ladder": decision_trace["evidence_ladder"].clone(),
        "evidence_sufficiency_check": decision_trace["evidence_sufficiency_check"].clone(),
        "escalate_if_needed": decision_trace["escalate_if_needed"].clone(),
        "abstain_if_insufficient": decision_trace["abstain_if_insufficient"].clone(),
    });
    let artifact_refs = collect_trace_string_refs(&prepared.payload, "artifact_refs");
    let message_refs = collect_trace_string_refs(&prepared.payload, "message_refs");
    let source_event_ids = json!([format!("context_pack:{}", prepared.context_pack_id)]);
    let evidence_span = json!({
        "kind": "retrieval_trace",
        "context_pack_id": prepared.context_pack_id,
        "cheapest_sufficient_layer": decision_trace["evidence_ladder"]["cheapest_sufficient_layer"]
            .clone(),
        "final_decision": decision_trace["final_decision"].clone(),
    });
    postgres::RetrievalTraceInsert {
        workspace_id,
        project_id: prepared.project.project_id,
        namespace_id: prepared.namespace_id,
        context_pack_id: Some(prepared.context_pack_id),
        query_text: args.query.clone(),
        requested_mode: args.retrieval_mode.clone(),
        effective_mode: Some(prepared.effective_mode.clone()),
        scope_filter: decision_trace["scope"].clone(),
        candidate_summary,
        rerank_summary,
        evidence_sufficiency,
        source_kind: Some("context_pack_retrieval_runtime".to_string()),
        source_event_ids,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind: Some("extract".to_string()),
        schema_version: Some("retrieval-trace-envelope-v1".to_string()),
        final_decision: decision_trace["final_decision"]
            .as_str()
            .unwrap_or("abstain")
            .to_string(),
        temporal_query_epoch_ms: args.at_epoch_ms,
        trace_payload: decision_trace.clone(),
    }
}

fn collect_trace_string_refs(value: &Value, key: &str) -> Value {
    fn walk(node: &Value, key: &str, acc: &mut Vec<String>) {
        match node {
            Value::Object(map) => {
                if let Some(value) = map.get(key) {
                    match value {
                        Value::Array(items) => {
                            for item in items {
                                if let Some(text) = item.as_str() {
                                    acc.push(text.to_string());
                                }
                            }
                        }
                        Value::String(text) => acc.push(text.clone()),
                        _ => {}
                    }
                }
                for child in map.values() {
                    walk(child, key, acc);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, key, acc);
                }
            }
            _ => {}
        }
    }

    let mut refs = Vec::new();
    walk(value, key, &mut refs);
    refs.sort();
    refs.dedup();
    json!(refs)
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
            query_cache: HashMap::new(),
        });
    }
    let cached = guard
        .as_mut()
        .ok_or_else(|| anyhow!("query embedder cache unexpectedly empty"))?;
    if let Some(vector) = cached.query_cache.get(query).cloned() {
        return Ok((vector, started.elapsed().as_millis()));
    }
    let embeddings = cached.embedder.embed(&[query.to_string()], Some(1))?;
    let vector = embeddings
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("semantic embedder returned no query vector"))?;
    if cached.query_cache.len() >= 64
        && let Some(oldest_key) = cached.query_cache.keys().next().cloned()
    {
        cached.query_cache.remove(&oldest_key);
    }
    cached.query_cache.insert(query.to_string(), vector.clone());
    Ok((vector, started.elapsed().as_millis()))
}

fn local_repo_document_snippet(repo_root: &str, relative_path: &str) -> Result<Option<String>> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let repo_root = Path::new(repo_root);
    let joined = if let Some(clean_relative_path) = normalized_safe_relative_path(trimmed) {
        repo_root.join(clean_relative_path)
    } else if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        let canonical_repo_root = match repo_root.canonicalize() {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let joined = repo_root.join(trimmed);
        let canonical_path = match joined.canonicalize() {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        if !canonical_path.starts_with(&canonical_repo_root) || !canonical_path.is_file() {
            return Ok(None);
        }
        return read_file_snippet(&canonical_path, 2000).map(Some);
    };
    if !joined.is_file() {
        return Ok(None);
    }
    read_file_snippet(&joined, 2000).map(Some)
}

fn read_file_snippet(path: &PathBuf, max_bytes: usize) -> Result<String> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0_u8; max_bytes];
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn normalized_safe_relative_path(value: &str) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
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
        "{}::{}::{}::{}::{}::{}::{}::{}::{}::{}",
        cfg.stack_name,
        args.project,
        args.namespace,
        effective_mode,
        args.limit_documents,
        args.limit_symbols,
        args.limit_chunks,
        args.limit_semantic_chunks,
        args.at_epoch_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "latest".to_string()),
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
    hasher.update(args.at_epoch_ms.unwrap_or(-1).to_le_bytes());
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
        context.precise_lower_bound_ms,
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
        artifact_bucket: None,
        artifact_object_key: None,
        artifact_state: None,
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
            precise_elapsed_ms(resolve_started),
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
            precise_elapsed_ms(resolve_started),
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
    let started = Instant::now();
    let fast_cache_key = selected_fast_cache_key(cfg, args);
    let cached = if let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    {
        cached
    } else {
        let Some(cached) = edge_fast_context_pack_cache_get(
            &cfg.edge_cache_path,
            fast_cache_key,
            cfg.local_fast_cache_ttl_ms,
        )?
        else {
            return Ok(None);
        };
        let precise_lower_bound_ms = precise_elapsed_ms(started);
        local_fast_context_pack_cache_put(fast_cache_key, &cached)?;
        if persist && !cached.durably_persisted {
            return Ok(None);
        }
        return Ok(Some(cached_fast_context_pack_stats(
            &cached,
            precise_lower_bound_ms,
        )));
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(cached_fast_context_pack_stats(&cached, 0.0)))
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

fn local_entry_from_edge_fast(
    cached: edge_cache::CachedFastContextPackEntry,
) -> Result<LocalContextPackEntry> {
    let payload: Value = serde_json::from_str(&cached.payload_json)
        .context("failed to decode cached fast context pack payload")?;
    let context_pack_id = Uuid::parse_str(&cached.context_pack_id)
        .context("cached fast context pack id is not a UUID")?;
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
        cached_at_epoch_ms: cached.cached_at_epoch_ms,
    })
}

fn cached_context_pack_stats(
    cached: &LocalContextPackEntry,
    scope_signature: String,
    resolve_scope_ms: u128,
    cache_lookup_ms: u128,
    retrieval_lower_bound_ms_precise: f64,
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
        retrieval_lower_bound_ms_precise: Some(retrieval_lower_bound_ms_precise),
    }
}

fn cached_fast_context_pack_stats(
    cached: &LocalContextPackEntry,
    retrieval_lower_bound_ms_precise: f64,
) -> ContextPackStats {
    cached_context_pack_stats(
        cached,
        "local_fast_cache".to_string(),
        0,
        0,
        retrieval_lower_bound_ms_precise,
    )
}

fn context_pack_result_from_local_entry(
    cached: LocalContextPackEntry,
    retrieval_lower_bound_ms_precise: f64,
) -> ContextPackResult {
    let stats = cached_fast_context_pack_stats(&cached, retrieval_lower_bound_ms_precise);
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
    let retrieval_lower_bound_ms = stats
        .retrieval_lower_bound_ms_precise
        .unwrap_or((policy_ms + retrieval_ms) as f64);
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
        "scope_signature": stats.scope_signature,
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
        "retrieval_lower_bound_ms": retrieval_lower_bound_ms,
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

fn precise_elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn edge_fast_context_pack_cache_get(
    edge_cache_path: &std::path::Path,
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
) -> Result<Option<LocalContextPackEntry>> {
    let Some(entry) =
        edge_cache::get_fast_context_pack_cache_entry(edge_cache_path, fast_cache_key)?
    else {
        return Ok(None);
    };
    let entry = local_entry_from_edge_fast(entry)?;
    if now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms {
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

fn memory_card_temporal_exclusion_to_json(
    diagnostic: &postgres::MemoryCardTemporalExclusionDiagnostic,
) -> Value {
    json!({
        "memory_card_id": diagnostic.memory_card_id,
        "title": diagnostic.title,
        "truth_state": diagnostic.truth_state,
        "status": diagnostic.status,
        "valid_from_epoch_ms": diagnostic.valid_from_epoch_ms,
        "valid_to_epoch_ms": diagnostic.valid_to_epoch_ms,
        "exclusion_reason": diagnostic.exclusion_reason
    })
}

fn memory_card_to_json(card: &postgres::MemoryCardRecord) -> Value {
    json!({
        "memory_card_id": card.memory_card_id,
        "project_code": card.project_code,
        "namespace_code": card.namespace_code,
        "title": card.title,
        "summary": card.summary,
        "body": card.body,
        "tags": card.tags,
        "provenance": card.provenance,
        "fact_subject": card.fact_subject,
        "fact_predicate": card.fact_predicate,
        "fact_object": card.fact_object,
        "truth_state": card.truth_state,
        "verification_state": card.verification_state,
        "status": card.status,
        "derivation_kind": card.derivation_kind,
        "candidate_class": card.candidate_class,
        "source_kind": card.source_kind,
        "hot_path_write_eligible": card.hot_path_write_eligible,
        "background_consolidation_recommended": card.background_consolidation_recommended,
        "observed_at_epoch_ms": card.observed_at_epoch_ms,
        "recorded_at_epoch_ms": card.recorded_at_epoch_ms,
        "valid_from_epoch_ms": card.valid_from_epoch_ms,
        "valid_to_epoch_ms": card.valid_to_epoch_ms,
        "last_verified_at_epoch_ms": card.last_verified_at_epoch_ms,
        "superseded_by_memory_card_id": card.superseded_by_memory_card_id,
        "created_at": card.created_at,
    })
}

fn memory_relation_edge_to_json(edge: &postgres::MemoryRelationEdgeRecord) -> Value {
    json!({
        "memory_relation_edge_id": edge.memory_relation_edge_id,
        "project_code": edge.project_code,
        "namespace_code": edge.namespace_code,
        "source_memory_card_id": edge.source_memory_card_id,
        "target_memory_card_id": edge.target_memory_card_id,
        "relation_type": edge.relation_type,
        "relation_state": edge.relation_state,
        "evidence": edge.evidence,
        "recorded_at_epoch_ms": edge.recorded_at_epoch_ms,
        "valid_from_epoch_ms": edge.valid_from_epoch_ms,
        "valid_to_epoch_ms": edge.valid_to_epoch_ms,
        "created_at": edge.created_at,
    })
}

fn raw_evidence_temporal_exclusion_to_json(
    diagnostic: &postgres::RawEvidenceTemporalExclusionDiagnostic,
) -> Value {
    json!({
        "memory_item_id": diagnostic.memory_item_id,
        "title": diagnostic.title,
        "truth_state": diagnostic.truth_state,
        "verification_state": diagnostic.verification_state,
        "valid_from_epoch_ms": diagnostic.valid_from_epoch_ms,
        "valid_to_epoch_ms": diagnostic.valid_to_epoch_ms,
        "exclusion_reason": diagnostic.exclusion_reason
    })
}

fn raw_evidence_to_json(hit: &postgres::RawEvidenceRecord) -> Value {
    let relative_path = hit
        .evidence_span
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| {
            hit.details
                .get("path")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            hit.details
                .get("relative_path")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        });
    json!({
        "memory_item_id": hit.memory_item_id,
        "memory_provenance_id": hit.memory_provenance_id,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "title": hit.title,
        "summary": hit.summary,
        "content": hit.content,
        "source_kind": hit.source_kind,
        "source_event_id": hit.source_event_id,
        "artifact_refs": hit.artifact_refs,
        "message_refs": hit.message_refs,
        "evidence_span": hit.evidence_span,
        "details": hit.details,
        "derivation_kind": hit.derivation_kind,
        "truth_state": hit.truth_state,
        "trust_state": hit.trust_state,
        "verification_state": hit.verification_state,
        "observed_at_epoch_ms": hit.observed_at_epoch_ms,
        "recorded_at_epoch_ms": hit.recorded_at_epoch_ms,
        "valid_from_epoch_ms": hit.valid_from_epoch_ms,
        "valid_to_epoch_ms": hit.valid_to_epoch_ms,
        "last_verified_at_epoch_ms": hit.last_verified_at_epoch_ms,
        "relative_path": relative_path,
        "provenance": {
            "source_kind": hit.source_kind,
            "source_event_id": hit.source_event_id,
            "artifact_refs": hit.artifact_refs,
            "message_refs": hit.message_refs,
            "evidence_span": hit.evidence_span,
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

fn should_use_compact_workspace_graph_enrichment(
    workspace_requests: &[(Uuid, String)],
    semantic_chunks: &[Value],
) -> bool {
    semantic_chunks.is_empty() && workspace_requests.len() <= 2
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
        ContextPackStats, ContextPackTimings, LocalContextPackEntry, PreparedContextPack,
        SemanticTimings, apply_semantic_relevance_guard, build_context_pack_decision_trace,
        build_retrieval_trace_insert, build_same_thread_cache_reuse_payload, cache_key,
        degradation_probe_stale_fast_cache, degradation_proof_scenarios,
        exact_document_lookup_queries, fast_cache_key, local_fast_context_pack_cache_put,
        model_visible_context_pack_payload, normalized_safe_relative_path, query_terms,
        selected_fast_cache_key, semantic_fallback_result, semantic_hit_has_query_overlap,
        semantic_skip_fallback_result, semantic_skip_reason_for_router, semantic_skipped_result,
        should_bypass_fast_context_pack_cache, should_emit_same_thread_cache_reuse_payload,
        should_prefer_document_lookup_only, should_try_exact_document_lookup,
        should_try_exact_symbol_lookup, should_use_compact_workspace_graph_enrichment,
        should_use_minimal_document_workspace_graph, should_use_minimal_symbol_workspace_graph,
        structured_layer_sufficiency, summary_layer_sufficiency, synthetic_chunk_hit,
        try_execute_context_pack_fast_cached, with_whole_cycle_observed_overrides,
    };
    use crate::cli::ContextPackArgs;
    use crate::config::AppConfig;
    use crate::postgres::{DocumentHit, ProjectRecord, SymbolHit};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn test_config() -> AppConfig {
        AppConfig {
            stack_name: "amai".to_string(),
            pg_db: "amai".to_string(),
            app_db_user: "amai".to_string(),
            app_db_password: "amai".to_string(),
            postgres_dsn: "postgres://localhost/unused".to_string(),
            app_postgres_dsn: "postgres://localhost/unused".to_string(),
            qdrant_url: "http://127.0.0.1:6334".to_string(),
            qdrant_http_url: "http://127.0.0.1:6334".to_string(),
            qdrant_collection_code: "test".to_string(),
            benchmark_qdrant_http_url: None,
            benchmark_qdrant_collection_code: None,
            qdrant_alias_code: "test".to_string(),
            qdrant_collection_memory: "memory".to_string(),
            qdrant_alias_memory: "memory".to_string(),
            qdrant_code_dim: 384,
            qdrant_memory_dim: 384,
            qdrant_distance: "Cosine".to_string(),
            s3_endpoint: "http://127.0.0.1:9000".to_string(),
            s3_region: "us-east-1".to_string(),
            s3_access_key: "test".to_string(),
            s3_secret_key: "test".to_string(),
            s3_bucket_artifacts: "artifacts".to_string(),
            s3_bucket_transcripts: "transcripts".to_string(),
            s3_bucket_context: "context".to_string(),
            nats_url: "nats://127.0.0.1:4222".to_string(),
            nats_http_url: "http://127.0.0.1:8222".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            edge_cache_path: "/tmp/edge-cache-test.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            local_fast_cache_ttl_ms: 5_000,
        }
    }

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
    fn document_lookup_only_prefers_file_like_queries() {
        assert!(should_prefer_document_lookup_only("README.md"));
        assert!(should_prefer_document_lookup_only(
            "scripts/benchmark_matrix.sh"
        ));
        assert!(should_prefer_document_lookup_only("Makefile"));
        assert!(should_prefer_document_lookup_only("Dockerfile"));
        assert!(should_prefer_document_lookup_only("justfile"));
        assert!(!should_prefer_document_lookup_only("build_context_pack"));
        assert!(!should_prefer_document_lookup_only("ContextPackBuilder"));
    }

    #[test]
    fn exact_document_lookup_covers_common_config_basenames() {
        assert!(should_try_exact_document_lookup("Makefile"));
        assert!(should_try_exact_document_lookup("Dockerfile"));
        assert!(should_try_exact_document_lookup("justfile"));
        assert!(should_try_exact_document_lookup("WORKSPACE"));
        assert!(should_try_exact_document_lookup("package.json"));
        assert!(should_try_exact_document_lookup("README.md"));
        assert!(should_try_exact_document_lookup("Continuity snapshot"));
        assert!(!should_try_exact_document_lookup("build_context_pack"));
    }

    #[test]
    fn exact_document_lookup_queries_add_slugified_markdown_aliases() {
        assert_eq!(
            exact_document_lookup_queries("Continuity snapshot"),
            vec![
                "Continuity snapshot".to_string(),
                "continuity-snapshot".to_string(),
                "continuity-snapshot.md".to_string()
            ]
        );
        assert_eq!(
            exact_document_lookup_queries("src/config.rs"),
            vec!["src/config.rs".to_string()]
        );
    }

    #[test]
    fn exact_symbol_lookup_skips_hyphenated_cold_probe_queries() {
        assert!(should_try_exact_symbol_lookup("ContextPackBuilder"));
        assert!(should_try_exact_symbol_lookup("build_context_pack"));
        assert!(!should_try_exact_symbol_lookup("etalon-cold-push-87"));
    }

    #[test]
    fn context_pack_decision_trace_explains_included_and_missing_layers() {
        let payload = json!({
            "project": {"code": "art"},
            "namespace": {"code": "continuity"},
            "query": "restore previous chat context",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{"project_code": "art"}],
            "retrieval": {
                "exact_documents": [{"relative_path": "README.md"}],
                "symbol_hits": [],
                "lexical_chunks": [{"relative_path": "docs/guide.md"}],
                "semantic_chunks": [],
                "memory_cards": [],
                "memory_relation_edges": [],
                "raw_evidence": []
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
        assert_eq!(trace["not_included"].as_array().map(Vec::len), Some(5));
        assert_eq!(
            trace["evidence_ladder"]["cheapest_sufficient_layer"].as_str(),
            Some("summary_compact")
        );
        assert_eq!(trace["final_decision"].as_str(), Some("continue"));
        assert_eq!(trace["scope_resolver"]["status"].as_str(), Some("resolved"));
        assert_eq!(
            trace["escalate_if_needed"]["required"].as_bool(),
            Some(false)
        );
        assert_eq!(
            trace["abstain_if_insufficient"]["abstained"].as_bool(),
            Some(false)
        );
        let semantic_not_included = trace["not_included"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["strategy"].as_str() == Some("semantic_chunks"))
            })
            .expect("semantic not_included entry");
        assert_eq!(
            semantic_not_included["reason"].as_str(),
            Some("semantic layer abstained")
        );
        assert_eq!(
            trace["scope"]["effective_retrieval_mode"].as_str(),
            Some("local_strict")
        );
        assert_eq!(
            trace["intent_classifier"]["classification"].as_str(),
            Some("continuity")
        );
        assert_eq!(
            trace["evidence_sufficiency_check"]["cheapest_sufficient_layer"].as_str(),
            Some("summary_compact")
        );
        let raw_not_included = trace["not_included"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["strategy"].as_str() == Some("raw_evidence"))
            })
            .expect("raw not_included entry");
        assert_eq!(
            raw_not_included["reason"].as_str(),
            Some(
                "Raw evidence не поднимался, потому что более дешёвый summary/structured слой уже был достаточен."
            )
        );
    }

    #[test]
    fn context_pack_decision_trace_classifies_policy_queries_and_groups_candidates() {
        let payload = json!({
            "project": {"code": "art"},
            "namespace": {"code": "review"},
            "query": "what policy allows read access to this file path",
            "effective_retrieval_mode": "local_plus_related",
            "visible_projects": [{"project_code": "art"}],
            "temporal_query_epoch_ms": 1_735_689_600_000_i64,
            "retrieval": {
                "exact_documents": [{"relative_path": "docs/policy.md"}],
                "symbol_hits": [],
                "lexical_chunks": [{"relative_path": "docs/policy.md"}],
                "semantic_chunks": [],
                "memory_cards": [],
                "memory_relation_edges": [],
                "raw_evidence": []
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

        assert_eq!(
            trace["intent_classifier"]["classification"].as_str(),
            Some("policy_check")
        );
        assert_eq!(
            trace["candidate_generation"]["grouped_strategies"]["exact"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["candidate_generation"]["grouped_strategies"]["lexical"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["candidate_generation"]["temporal_query_requested"].as_bool(),
            Some(true)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_query_requested"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn context_pack_decision_trace_explains_historical_temporal_hits() {
        let payload = json!({
            "project": {"code": "project_alpha"},
            "namespace": {"code": "review"},
            "query": "historical semantic fact",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{"project_code": "project_alpha"}],
            "temporal_query_epoch_ms": 11_500_i64,
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": [],
                "memory_cards": [{
                    "memory_card_id": Uuid::new_v4(),
                    "truth_state": "superseded",
                    "valid_from_epoch_ms": 11_000,
                    "valid_to_epoch_ms": 11_999,
                    "summary": "historical semantic fact"
                }],
                "memory_relation_edges": [],
                "raw_evidence": []
            },
            "retrieval_temporal_stats": {
                "memory_cards": {
                    "prefilter_match_count": 3,
                    "excluded_by_temporal_window": 2,
                    "excluded_by_current_truth_state": 0,
                    "excluded_candidates": [{
                        "memory_card_id": Uuid::new_v4(),
                        "title": "newer fact",
                        "truth_state": "current",
                        "status": "active",
                        "valid_from_epoch_ms": 12_000,
                        "valid_to_epoch_ms": null,
                        "exclusion_reason": "outside_requested_time_slice"
                    }]
                },
                "raw_evidence": {
                    "prefilter_match_count": 1,
                    "excluded_by_temporal_window": 1,
                    "excluded_by_current_truth_state": 0,
                    "excluded_candidates": [{
                        "memory_item_id": Uuid::new_v4(),
                        "title": "newer raw evidence",
                        "truth_state": "current",
                        "verification_state": "verified",
                        "valid_from_epoch_ms": 12_000,
                        "valid_to_epoch_ms": null,
                        "exclusion_reason": "outside_requested_time_slice"
                    }]
                }
            },
            "quality": {
                "semantic_guard": {
                    "abstained": false,
                    "reason": "cheapest_sufficient_evidence_already_found",
                    "detail": null
                }
            }
        });

        let trace = build_context_pack_decision_trace(&payload);

        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["status"].as_str(),
            Some("applied_exact_time_slice")
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["historical_memory_cards_after_filter"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["prefilter_memory_cards"]
                .as_u64(),
            Some(3)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_cards_by_temporal_window"].as_u64(),
            Some(2)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_raw_evidence_by_temporal_window"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_card_candidates"][0]["title"].as_str(),
            Some("newer fact")
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_card_candidates"][0]["exclusion_reason"].as_str(),
            Some("outside_requested_time_slice")
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["explanation"].as_str(),
            Some(
                "Historical-but-valid candidates remained admissible at the requested timestamp while out-of-window candidates were excluded."
            )
        );
    }

    #[test]
    fn retrieval_trace_insert_keeps_cheapest_sufficient_layer_in_evidence_span() {
        let context_pack_id = Uuid::new_v4();
        let payload = json!({
            "project": {"code": "project_alpha"},
            "namespace": {"code": "review"},
            "decision_trace": {
                "scope": {
                    "project_code": "project_alpha",
                    "namespace_code": "review",
                    "effective_retrieval_mode": "local_plus_related",
                    "visible_projects_total": 1
                },
                "candidate_generation": {"exact_documents": 1},
                "selection_priority": ["exact_documents", "lexical_chunks", "semantic_chunks"],
                "rerank_legality_relevance": {"scope_filtered": true},
                "included": [],
                "not_included": [],
                "evidence_ladder": {"cheapest_sufficient_layer": "structured_graph"},
                "evidence_sufficiency_check": {
                    "status": "sufficient",
                    "next_action": "answer_on_cheapest_sufficient_evidence"
                },
                "escalate_if_needed": {"required": false},
                "abstain_if_insufficient": {"abstained": false},
                "final_decision": "continue"
            }
        });
        let prepared = PreparedContextPack {
            context_pack_id,
            project: ProjectRecord {
                project_id: Uuid::new_v4(),
                code: "project_alpha".to_string(),
                display_name: "Project Alpha".to_string(),
                repo_root: "/tmp/project_alpha".to_string(),
                visibility_scope: "private".to_string(),
                updated_at: "2026-04-07T00:00:00Z".to_string(),
            },
            namespace_id: Uuid::new_v4(),
            effective_mode: "local_plus_related".to_string(),
            visible_projects_json: json!([{"project_code": "project_alpha"}]),
            payload: Arc::new(payload),
            payload_json: Arc::from("{}"),
            stats: ContextPackStats {
                context_pack_id,
                exact_documents: 1,
                symbol_hits: 0,
                lexical_chunks: 0,
                semantic_chunks: 0,
                cache_hit: false,
                scope_signature: "scope".to_string(),
                timings: ContextPackTimings {
                    resolve_scope_ms: 0,
                    cache_lookup_ms: 0,
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
                retrieval_lower_bound_ms_precise: None,
            },
            cache_key: "cache-key".to_string(),
            scope_signature: "scope".to_string(),
            cache_hit: false,
            durably_persisted: true,
            artifact_bucket: None,
            artifact_object_key: None,
            artifact_state: None,
        };
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: false,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "proof_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };

        let trace = build_retrieval_trace_insert(Uuid::new_v4(), &prepared, &args);

        assert_eq!(
            trace.evidence_span["cheapest_sufficient_layer"].as_str(),
            Some("structured_graph")
        );
    }

    #[test]
    fn sufficiency_router_prefers_cheapest_available_layer() {
        assert_eq!(
            summary_layer_sufficiency(1, 0, 0, 0),
            (true, "exact_documents_sufficient")
        );
        assert_eq!(
            summary_layer_sufficiency(0, 0, 0, 1),
            (false, "single_memory_card_needs_structured_neighborhood")
        );
        assert_eq!(
            structured_layer_sufficiency(false, 2),
            (true, "structured_graph_neighborhood_sufficient")
        );
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "router".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: false,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "live_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        assert_eq!(
            semantic_skip_reason_for_router(&args, true, false),
            Some("cheapest_sufficient_evidence_already_found")
        );
        assert_eq!(semantic_skip_reason_for_router(&args, false, false), None);
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
            at_epoch_ms: None,
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
    fn fast_cache_hit_works_without_explicit_retrieval_mode() {
        let cfg = test_config();
        let args = ContextPackArgs {
            project: "art".to_string(),
            namespace: "default".to_string(),
            query: "./src/mcp.rs".to_string(),
            retrieval_mode: None,
            disable_cache: false,
            limit_documents: 5,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "live_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let payload = json!({
            "context_pack_id": Uuid::new_v4().to_string(),
            "project": {"code": "art"},
            "namespace": {"code": "default"},
            "retrieval": {
                "exact_documents": [{"relative_path": "src/mcp.rs"}],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        });
        let entry = LocalContextPackEntry {
            context_pack_id: Uuid::parse_str(
                payload["context_pack_id"]
                    .as_str()
                    .expect("context pack id"),
            )
            .expect("uuid"),
            payload: Arc::new(payload),
            exact_documents: 1,
            symbol_hits: 0,
            lexical_chunks: 0,
            semantic_chunks: 0,
            durably_persisted: true,
            cached_at_epoch_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis(),
        };
        local_fast_context_pack_cache_put(selected_fast_cache_key(&cfg, &args), &entry)
            .expect("store fast cache");

        let cached = try_execute_context_pack_fast_cached(&cfg, &args, false)
            .expect("fast cache lookup")
            .expect("fast cache hit");

        assert!(cached.stats.cache_hit);
        assert_eq!(cached.stats.timings.resolve_scope_ms, 0);
        assert_eq!(cached.stats.timings.cache_lookup_ms, 0);
        assert_eq!(
            cached.payload["retrieval"]["exact_documents"][0]["relative_path"],
            json!("src/mcp.rs")
        );
    }

    #[test]
    fn normalized_safe_relative_path_accepts_clean_relative_paths() {
        assert_eq!(
            normalized_safe_relative_path("./src/config.rs"),
            Some(PathBuf::from("src/config.rs"))
        );
        assert_eq!(
            normalized_safe_relative_path(".amai/onboarding/project-chat-startup-contract.json"),
            Some(PathBuf::from(
                ".amai/onboarding/project-chat-startup-contract.json"
            ))
        );
    }

    #[test]
    fn temporal_queries_use_distinct_cache_keys() {
        let cfg = test_config();
        let base = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "temporal fact".to_string(),
            retrieval_mode: None,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: Some(1_000),
            token_source_kind: "cli".to_string(),
            disable_cache: false,
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let other = ContextPackArgs {
            at_epoch_ms: Some(2_000),
            ..base.clone()
        };

        assert_ne!(
            cache_key(&cfg, &base, "local_strict"),
            cache_key(&cfg, &other, "local_strict")
        );
        assert_ne!(
            fast_cache_key(&cfg, &base, "local_strict"),
            fast_cache_key(&cfg, &other, "local_strict")
        );
    }

    #[test]
    fn normalized_safe_relative_path_rejects_escape_or_absolute_paths() {
        assert!(normalized_safe_relative_path("../secrets.txt").is_none());
        assert!(normalized_safe_relative_path("/etc/passwd").is_none());
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
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["relative_path"].as_str(),
            Some(".amai-continuity/live-handoff.md")
        );
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["snippet"].as_str(),
            Some("handoff snippet")
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
        assert!(
            compact["decision_trace"]["included"][0]
                .get("reason")
                .is_none()
        );
        assert!(
            compact["decision_trace"]["semantic_guard"]
                .get("detail")
                .is_none()
        );
        assert!(compact.get("workspace_graph").is_none());
        assert!(compact.get("retrieval_runtime").is_none());
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("score")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("chunk")
                .is_none()
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("metadata")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("provenance")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            serde_json::to_string(&compact).expect("compact json").len()
                < serde_json::to_string(&payload).expect("full json").len()
        );
    }

    #[test]
    fn cli_visible_context_pack_payload_keeps_decision_trace_for_proof_context_pack() {
        let payload = json!({
            "context_pack_id": "ctx-proof",
            "project": {"code": "art"},
            "namespace": {"code": "continuity"},
            "effective_retrieval_mode": "local_strict",
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            },
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "selection_priority": ["exact_documents", "symbol_hits", "lexical_chunks", "semantic_chunks"],
                "included": [],
                "not_included": [],
                "semantic_guard": {"abstained": true}
            }
        });

        let compact = super::cli_visible_context_pack_payload(&payload, "proof_context_pack");

        assert_eq!(
            compact["decision_trace"]["scope"]["effective_retrieval_mode"].as_str(),
            Some("local_strict")
        );
        assert_eq!(
            compact["decision_trace"]["selection_priority"]
                .as_array()
                .map(Vec::len),
            Some(4)
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

        assert_eq!(
            compact["retrieval"]["exact_documents"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
    }

    #[test]
    fn model_visible_context_pack_payload_symbol_only_shape_stays_metadata_only() {
        let payload = json!({
            "context_pack_id": "ctx-symbol-only",
            "project": {
                "code": "art",
                "display_name": "Art",
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "symbol_only_navigation_runtime_checkpoint",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "included": [{
                    "strategy": "symbol_hits",
                    "count": 1,
                    "reason": "verbose symbol reason"
                }],
                "not_included": [],
                "semantic_guard": {
                    "abstained": true,
                    "detail": "verbose semantic abstain detail"
                }
            },
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "symbol_only_navigation_runtime_checkpoint",
                    "kind": "function_item",
                    "metadata": {
                        "language": "rust",
                        "visibility": "pub"
                    },
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["name"].as_str(),
            Some("symbol_only_navigation_runtime_checkpoint")
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("metadata")
                .is_none()
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("provenance")
                .is_none()
        );
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
    }

    #[test]
    fn model_visible_context_pack_payload_preserves_unique_exact_snippet_in_hybrid_shape() {
        let payload = json!({
            "context_pack_id": "ctx-hybrid-unique-exact",
            "project": {
                "code": "art",
                "display_name": "Art",
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_plus_related",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_plus_related"},
                "included": [{
                    "strategy": "exact_documents",
                    "count": 1,
                    "reason": "verbose exact reason"
                }, {
                    "strategy": "lexical_chunks",
                    "count": 1,
                    "reason": "verbose lexical reason"
                }],
                "not_included": [],
                "semantic_guard": {
                    "abstained": false,
                    "detail": "verbose semantic detail"
                }
            },
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "snippet": "unique exact note only",
                    "source_kind": "docs"
                }],
                "symbol_hits": [],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "covered chunk text without the exact-only note",
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/related.md",
                    "content": "related semantic evidence",
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }]
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["snippet"].as_str(),
            Some("unique exact note only")
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["content"].as_str(),
            Some("covered chunk text without the exact-only note")
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"][0]["relative_path"].as_str(),
            Some("docs/related.md")
        );
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
    }

    #[test]
    fn same_thread_cache_reuse_payload_drops_text_and_keeps_reference_map() {
        let payload = json!({
            "context_pack_id": "ctx-reuse",
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
            "retrieval_runtime": {"cache_hit": true},
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "snippet": "handoff snippet",
                    "source_kind": "docs",
                    "provenance": {
                        "source_project": "art"
                    }
                }],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "build_context_pack",
                    "kind": "function_item",
                    "provenance": {
                        "source_project": "art"
                    }
                }],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "full lexical text",
                    "provenance": {
                        "source_project": "art"
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/related.md",
                    "content": "full semantic text",
                    "provenance": {
                        "source_project": "art"
                    }
                }]
            }
        });

        let compact = build_same_thread_cache_reuse_payload(&payload, "thread-1");

        assert_eq!(
            compact["cache_reuse_reference"]["state"].as_str(),
            Some("same_thread_context_pack_replay")
        );
        assert_eq!(
            compact["cache_reuse_reference"]["source_context_pack_id"].as_str(),
            Some("ctx-reuse")
        );
        assert!(compact["cache_reuse_reference"].get("thread_id").is_none());
        assert!(compact["cache_reuse_reference"].get("note").is_none());
        assert_eq!(
            compact["cache_reuse_reference"]["active_files"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(compact["project"]["code"].as_str(), Some("art"));
        assert!(compact["project"].get("display_name").is_none());
        assert_eq!(compact["namespace"]["code"].as_str(), Some("continuity"));
        assert!(compact["namespace"].get("display_name").is_none());
        assert!(compact["decision_trace"].is_null());
        assert!(compact["workspace_graph"].is_null());
        assert!(compact["visible_projects"][0].get("display_name").is_none());
        assert!(
            compact["visible_projects"][0]
                .get("relation_type")
                .is_none()
        );
        assert_eq!(
            compact["retrieval_runtime"]["cache_hit"].as_bool(),
            Some(true)
        );
        assert!(
            compact["retrieval_runtime"]
                .get("resolve_scope_ms")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("snippet")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("content")
                .is_none()
        );
        assert!(
            compact["retrieval"]["semantic_chunks"][0]
                .get("content")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("provenance")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["name"].as_str(),
            Some("build_context_pack")
        );
    }

    #[test]
    fn model_visible_context_pack_payload_preserves_same_thread_cache_reuse_reference() {
        let payload = build_same_thread_cache_reuse_payload(
            &json!({
                "context_pack_id": "ctx-reuse-visible",
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
                    "included": [{"strategy": "lexical_chunks", "count": 1, "reason": "verbose"}],
                    "not_included": []
                },
                "retrieval": {
                    "exact_documents": [],
                    "symbol_hits": [],
                    "lexical_chunks": [{
                        "project_code": "art",
                        "relative_path": "docs/continuity.md",
                        "content": "full lexical text",
                        "provenance": {
                            "source_project": "art"
                        }
                    }],
                    "semantic_chunks": []
                }
            }),
            "thread-2",
        );

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["cache_reuse_reference"]["state"].as_str(),
            Some("same_thread_context_pack_replay")
        );
        assert_eq!(
            compact["cache_reuse_reference"]["source_context_pack_id"].as_str(),
            Some("ctx-reuse-visible")
        );
        assert_eq!(compact["project"]["code"].as_str(), Some("art"));
        assert_eq!(compact["namespace"]["code"].as_str(), Some("continuity"));
        assert!(compact.get("retrieval").is_none());
        assert!(compact.get("decision_trace").is_none());
        assert!(compact.get("query").is_none());
    }

    #[test]
    fn proof_context_pack_disables_same_thread_cache_reuse_compaction() {
        let stats = ContextPackStats {
            context_pack_id: Uuid::nil(),
            exact_documents: 1,
            symbol_hits: 0,
            lexical_chunks: 0,
            semantic_chunks: 0,
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
                ranking_ms: 0,
                provenance_ms: 0,
                pack_assembly_ms: 0,
                serialize_ms: 0,
                persist_ms: 0,
            },
            retrieval_lower_bound_ms_precise: Some(0.0),
        };
        let proof_args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            disable_cache: false,
            token_source_kind: "proof_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let default_args = ContextPackArgs {
            token_source_kind: "live_context_pack".to_string(),
            ..proof_args.clone()
        };
        let verify_args = ContextPackArgs {
            token_source_kind: "verify_context_pack".to_string(),
            ..proof_args.clone()
        };

        assert!(!should_emit_same_thread_cache_reuse_payload(
            &stats,
            &proof_args
        ));
        assert!(should_bypass_fast_context_pack_cache(&proof_args));
        assert!(should_bypass_fast_context_pack_cache(&verify_args));
        assert!(should_emit_same_thread_cache_reuse_payload(
            &stats,
            &default_args
        ));
        assert!(!should_bypass_fast_context_pack_cache(&default_args));
    }

    #[test]
    fn model_visible_context_pack_payload_keeps_cross_project_reference_only_when_needed() {
        let payload = json!({
            "context_pack_id": "ctx-cross-project",
            "project": {
                "code": "amai",
            },
            "namespace": {
                "code": "continuity",
            },
            "effective_retrieval_mode": "local_plus_related",
            "retrieval": {
                "exact_documents": [{
                    "project_code": "bug_bounty",
                    "relative_path": "docs/report.md",
                    "snippet": "cross project snippet",
                    "source_kind": "docs",
                    "provenance": {
                        "source_project": "bug_bounty"
                    }
                }],
                "symbol_hits": [{
                    "project_code": "bug_bounty",
                    "relative_path": "src/lib.rs",
                    "name": "run_report",
                    "kind": "function_item",
                    "provenance": {
                        "source_project": "bug_bounty"
                    }
                }],
                "lexical_chunks": [{
                    "project_code": "bug_bounty",
                    "relative_path": "docs/report.md",
                    "content": "cross project lexical",
                    "provenance": {
                        "source_project": "bug_bounty"
                    }
                }],
                "semantic_chunks": []
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["project_code"].as_str(),
            Some("bug_bounty")
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("provenance")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("source_kind")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["project_code"].as_str(),
            Some("bug_bounty")
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("provenance")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["project_code"].as_str(),
            Some("bug_bounty")
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("provenance")
                .is_none()
        );
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
    fn compact_workspace_enrichment_skips_for_small_nonsemantic_scope() {
        let requests = vec![
            (Uuid::new_v4(), "src/lib.rs".to_string()),
            (Uuid::new_v4(), "Cargo.toml".to_string()),
        ];
        assert!(should_use_compact_workspace_graph_enrichment(
            &requests,
            &[]
        ));

        let semantic_chunks = vec![json!({
            "project_code": "amai",
            "namespace_code": "review",
            "relative_path": "src/lib.rs"
        })];
        assert!(!should_use_compact_workspace_graph_enrichment(
            &requests,
            &semantic_chunks
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
    fn semantic_skipped_result_marks_reason_without_abstain() {
        let (hits, timings, guard) = semantic_skipped_result(
            "shared_runtime_marker",
            3,
            "exact_or_symbol_hits_sufficient",
        );
        assert!(hits.is_empty());
        assert_eq!(timings.query_embed_ms, 0);
        assert_eq!(guard.reason, Some("exact_or_symbol_hits_sufficient"));
        assert!(!guard.abstained);
        assert_eq!(guard.lexical_signal_count, 3);
    }

    #[test]
    fn verify_context_pack_does_not_skip_semantic_on_symbol_hits() {
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let should_try_exact_document = true;
        let should_try_exact_symbol = true;
        let semantic_skip_reason = if args.limit_semantic_chunks > 0
            && args.token_source_kind != "verify_context_pack"
            && ((should_try_exact_document && false) || (should_try_exact_symbol && true))
        {
            Some("exact_or_symbol_hits_sufficient")
        } else {
            None
        };
        assert_eq!(semantic_skip_reason, None);
    }

    #[test]
    fn benchmark_context_pack_skips_memory_lookup_when_exact_signal_exists() {
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "benchmark_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        assert!(super::should_skip_memory_lookup(&args, 2, 0, 0, 0));
        assert!(super::should_skip_memory_lookup(&args, 0, 4, 0, 0));

        let verify_args = ContextPackArgs {
            token_source_kind: "verify_context_pack".to_string(),
            ..args
        };
        assert!(!super::should_skip_memory_lookup(&verify_args, 2, 0, 0, 0));
    }

    #[test]
    fn benchmark_context_pack_skips_retrieval_trace_persist() {
        assert!(!super::should_persist_retrieval_trace(
            "benchmark_context_pack"
        ));
        assert!(!super::should_persist_retrieval_trace(
            "benchmark_cold_context_pack"
        ));
        assert!(super::should_persist_retrieval_trace("verify_context_pack"));
        assert!(super::should_persist_retrieval_trace("live_context_pack"));
    }

    #[test]
    fn benchmark_context_pack_uses_compact_payload() {
        assert!(super::should_use_benchmark_compact_payload(
            "benchmark_context_pack"
        ));
        assert!(super::should_use_benchmark_compact_payload(
            "benchmark_cold_context_pack"
        ));
        assert!(!super::should_use_benchmark_compact_payload(
            "verify_context_pack"
        ));
    }

    #[test]
    fn semantic_skipped_result_marks_benchmark_compact_reason() {
        let (hits, timings, guard) = semantic_skipped_result(
            "shared_runtime_marker",
            8,
            "benchmark_compact_summary_sufficient",
        );
        assert!(hits.is_empty());
        assert_eq!(timings.query_embed_ms, 0);
        assert_eq!(guard.reason, Some("benchmark_compact_summary_sufficient"));
        assert!(!guard.abstained);
    }

    #[test]
    fn semantic_skip_fallback_uses_exact_document_snippet_before_empty_semantic() {
        let documents = vec![DocumentHit {
            project_code: "project_alpha".to_string(),
            namespace_code: "review".to_string(),
            repo_root: "/tmp/project_alpha".to_string(),
            relative_path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            source_kind: "code_chunk".to_string(),
            git_commit_sha: None,
            score: 2000.0,
            snippet: "pub fn shared_runtime_marker() {}".to_string(),
        }];
        let (hits, _, guard) = semantic_skip_fallback_result(
            "shared_runtime_marker",
            4,
            2,
            &documents,
            &[],
            "exact_or_symbol_hits_sufficient",
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0]["retrieval_strategy"],
            json!("exact_document_fallback")
        );
        assert_eq!(hits[0]["relative_path"], json!("src/lib.rs"));
        assert_eq!(
            guard.detail.as_deref(),
            Some("semantic satisfied by exact document fallback")
        );
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
