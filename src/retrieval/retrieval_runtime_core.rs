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


include!("retrieval_runtime_entrypoints.inc");
include!("retrieval_runtime_prepare.inc");
include!("retrieval_runtime_router.inc");
include!("retrieval_runtime_support.inc");
include!("retrieval_runtime_tests.inc");
