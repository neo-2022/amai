use super::*;

#[derive(Clone)]
pub(super) struct ObserveState {
    pub(super) dashboard_refresh_ms: u64,
    pub(super) cfg: AppConfig,
    pub(super) bind: String,
    pub(super) cache: Arc<RwLock<ObserveCache>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct ThreadBindingQuery {
    pub(super) thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ClientBudgetTargetUpdateRequest {
    pub(super) percent: u64,
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    pub(super) namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ClientBudgetCompactChatRequest {
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    pub(super) namespace: String,
    #[serde(default)]
    pub(super) launch_host: bool,
    #[serde(default)]
    pub(super) refresh_handoff: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContinuityHandoffRequest {
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    pub(super) namespace: String,
    pub(super) headline: String,
    pub(super) next_step: String,
    #[serde(default)]
    pub(super) details: Option<String>,
    #[serde(default)]
    pub(super) resolved_headlines: Vec<String>,
    #[serde(default)]
    pub(super) resolved_task_ids: Vec<String>,
    #[serde(default)]
    pub(super) resolve_current_goal: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ClientBudgetHostControlFeedbackRequest {
    pub(super) feedback_kind: String,
    #[serde(default)]
    pub(super) command_id: Option<String>,
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    pub(super) namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ClientBudgetHostControlLaunchRequest {
    #[serde(default)]
    pub(super) command_id: Option<String>,
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    pub(super) namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct AgentDisplayNameUpdateRequest {
    pub(super) agent_scope: String,
    pub(super) display_name: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ObserveCache {
    pub(super) snapshot: Option<Value>,
    pub(super) dashboard_payload: Option<Value>,
    pub(super) dashboard_live_summary_payload: Option<Value>,
    pub(super) dashboard_live_summary_thread_id: Option<String>,
    pub(super) dashboard_live_summary_completed_epoch_ms: Option<u64>,
    pub(super) dashboard_live_summary_refresh_in_progress: bool,
    pub(super) client_budget_live_payload: Option<Value>,
    pub(super) client_budget_live_thread_id: Option<String>,
    pub(super) client_budget_live_completed_epoch_ms: Option<u64>,
    pub(super) client_live_meter_refresh_in_progress: bool,
    pub(super) client_live_meter_refresh_started_epoch_ms: Option<u64>,
    pub(super) thread_bound_snapshot: Option<Value>,
    pub(super) thread_bound_snapshot_thread_id: Option<String>,
    pub(super) thread_bound_snapshot_completed_epoch_ms: Option<u64>,
    pub(super) last_http_request_epoch_ms: Option<u64>,
    pub(super) last_refresh_started_epoch_ms: Option<u64>,
    pub(super) last_refresh_completed_epoch_ms: Option<u64>,
    pub(super) last_refresh_duration_ms: Option<u64>,
    pub(super) refresh_in_progress: bool,
    pub(super) last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CachedClientLiveMeterState {
    pub(super) working_state_thread_id: Option<String>,
    pub(super) thread_id: Option<String>,
    pub(super) turn_id: Option<String>,
    pub(super) ended_at_epoch_ms: Option<i64>,
    pub(super) client_turn_total_tokens: Option<u64>,
    pub(super) primary_limit_used_percent: Option<u64>,
    pub(super) secondary_limit_used_percent: Option<u64>,
}

pub(super) const SNAPSHOT_RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(3600);
pub(super) const CLIENT_LIMIT_TREND_ANALYSIS_INTERVAL: Duration = Duration::from_secs(900);
pub(super) const CLIENT_LIMIT_LIVE_SOURCE_TTL_MS: u64 = 3_000;
pub(super) const CLIENT_BUDGET_LIVE_PAYLOAD_CACHE_TTL_MS: u64 = 20_000;
pub(super) const COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS: u64 = 20_000;
pub(super) const DASHBOARD_LIVE_SUMMARY_CACHE_TTL_MS: u64 = 60_000;
pub(super) const ACTIVE_AGENT_CARD_MAX_SOURCE_DRIFT_MS: i64 = 10_000;
pub(super) const CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS: u64 =
    COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS;
pub(super) const CLIENT_BUDGET_SURFACES_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/observe/client_budget_surfaces_cache.json";
pub(super) const CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION: &str =
    "client-budget-surfaces-cache-v7";
pub(super) const CLIENT_BUDGET_GATE_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/observe/client_budget_gate_cache.json";
pub(super) const CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION: &str = "client-budget-gate-cache-v7";
pub(super) const THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION: &str =
    "thread-bound-budget-snapshot-cache-v2";
pub(super) const ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/observe/active_thread_hint.json";
pub(super) const ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION: &str = "active-thread-hint-cache-v1";
pub(super) const ACTIVE_THREAD_HINT_MAX_AGE_MS: u64 = 30 * 60 * 1000;
pub(super) const THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION: &str =
    "thread-bound-snapshot-invalidation-v1";
pub(super) const OBSERVE_SYSTEM_SNAPSHOT_PERSIST_ADVISORY_LOCK_KEY: i64 = 4_147_508_042;
pub(super) const OBSERVE_REFRESH_TIMEOUT_MS: u64 = 120_000;
pub(super) const OBSERVE_REFRESH_STUCK_GRACE_MS: u64 = 5_000;

pub(super) fn default_continuity_namespace() -> String {
    "continuity".to_string()
}

pub(super) async fn resolve_request_repo_root_for_project(
    cfg: &AppConfig,
    project_code: Option<&str>,
) -> Result<PathBuf> {
    if let Some(project_code) = project_code
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(db) = postgres::connect_app(cfg).await
            && let Ok(project) = postgres::get_project_by_code(&db, project_code).await
        {
            return Ok(PathBuf::from(project.repo_root));
        }
        let db = postgres::connect_admin(cfg).await?;
        postgres::bootstrap_schema(&db, cfg).await?;
        let project = postgres::get_project_by_code(&db, project_code).await?;
        return Ok(PathBuf::from(project.repo_root));
    }
    discover_repo_root(None)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedClientBudgetSurfacesCache {
    pub(super) cache_version: String,
    pub(super) fetched_at_epoch_ms: u64,
    #[serde(default)]
    pub(super) thread_id: Option<String>,
    pub(super) root_cause: Value,
    pub(super) gate: Value,
    pub(super) guard: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedClientBudgetGateCache {
    pub(super) cache_version: String,
    pub(super) fetched_at_epoch_ms: u64,
    #[serde(default)]
    pub(super) thread_id: Option<String>,
    pub(super) gate: Value,
    pub(super) guard: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedActiveThreadHint {
    pub(super) cache_version: String,
    pub(super) updated_at_epoch_ms: u64,
    pub(super) thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedThreadBoundSnapshotInvalidation {
    pub(super) cache_version: String,
    pub(super) invalidated_at_epoch_ms: u64,
    pub(super) thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedThreadBoundBudgetSnapshotCache {
    pub(super) cache_version: String,
    pub(super) fetched_at_epoch_ms: u64,
    pub(super) thread_id: String,
    pub(super) snapshot: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ObservabilityProfile {
    pub(super) snapshot: SnapshotProfile,
    pub(super) dashboard: DashboardProfile,
    pub(super) postgres: PostgresThresholds,
    pub(super) qdrant: QdrantThresholds,
    pub(super) nats: NatsThresholds,
    pub(super) retrieval: RetrievalThresholds,
    pub(super) parser: ParserThresholds,
    pub(super) accuracy: AccuracyThresholds,
    pub(super) load: LoadThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DashboardProfile {
    pub(super) refresh_ms: u64,
    #[serde(default = "default_exact_client_limit_prewarm_seconds")]
    pub(super) exact_client_limit_prewarm_seconds: u64,
    #[serde(default = "default_compact_client_budget_prewarm_seconds")]
    pub(super) compact_client_budget_prewarm_seconds: u64,
    pub(super) timing_format: DashboardTimingFormatProfile,
}

pub(super) fn default_exact_client_limit_prewarm_seconds() -> u64 {
    5
}

pub(super) fn default_compact_client_budget_prewarm_seconds() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DashboardTimingFormatProfile {
    pub(super) switch_to_nanoseconds_below_ms: f64,
    pub(super) switch_to_microseconds_below_ms: f64,
    pub(super) switch_to_seconds_at_or_above_ms: f64,
    pub(super) non_positive_floor_label: String,
    pub(super) seconds_suffix: String,
    pub(super) milliseconds_suffix: String,
    pub(super) microseconds_suffix: String,
    pub(super) nanoseconds_suffix: String,
    pub(super) seconds_decimals: u64,
    pub(super) milliseconds_decimals: u64,
    pub(super) microseconds_decimals: u64,
    pub(super) nanoseconds_decimals: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct SnapshotProfile {
    pub(super) postgres_query_probe_iterations: usize,
    pub(super) nats_publish_probe_iterations: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct PostgresThresholds {
    pub(super) target_connection_usage_ratio: f64,
    pub(super) alert_connection_usage_ratio: f64,
    pub(super) critical_connection_usage_ratio: f64,
    pub(super) target_query_probe_p95_ms: f64,
    pub(super) alert_query_probe_p95_ms: f64,
    pub(super) critical_query_probe_p95_ms: f64,
    pub(super) target_replica_lag_seconds: f64,
    pub(super) alert_replica_lag_seconds: f64,
    pub(super) critical_replica_lag_seconds: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct QdrantThresholds {
    pub(super) target_index_optimize_queue: f64,
    pub(super) alert_index_optimize_queue: f64,
    pub(super) critical_index_optimize_queue: f64,
    pub(super) target_search_p95_ms: f64,
    pub(super) alert_search_p95_ms: f64,
    pub(super) critical_search_p95_ms: f64,
    pub(super) target_update_queue_length: f64,
    pub(super) alert_update_queue_length: f64,
    pub(super) critical_update_queue_length: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct NatsThresholds {
    pub(super) target_publish_p95_ms: f64,
    pub(super) alert_publish_p95_ms: f64,
    pub(super) critical_publish_p95_ms: f64,
    pub(super) target_consumer_lag_msgs: f64,
    pub(super) alert_consumer_lag_msgs: f64,
    pub(super) critical_consumer_lag_msgs: f64,
    pub(super) target_jetstream_disk_usage_ratio: f64,
    pub(super) alert_jetstream_disk_usage_ratio: f64,
    pub(super) critical_jetstream_disk_usage_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RetrievalThresholds {
    pub(super) target_p95_ms: f64,
    pub(super) target_cold_sample_count: u64,
    pub(super) target_hot_p95_ms: f64,
    pub(super) target_hot_sample_count: u64,
    pub(super) target_hot_benchmark_iterations: u64,
    pub(super) target_hot_benchmark_warmup: u64,
    pub(super) alert_p95_ms: f64,
    pub(super) critical_p95_ms: f64,
    pub(super) stretch_hot_p95_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ParserThresholds {
    pub(super) target_coverage_ratio: f64,
    pub(super) alert_coverage_ratio: f64,
    pub(super) critical_coverage_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct AccuracyThresholds {
    pub(super) target_symbol_precision: f64,
    pub(super) alert_symbol_precision: f64,
    pub(super) critical_symbol_precision: f64,
    pub(super) target_semantic_precision: f64,
    pub(super) alert_semantic_precision: f64,
    pub(super) critical_semantic_precision: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct LoadThresholds {
    pub(super) target_hot_qps: f64,
    pub(super) target_hot_p50_ms: f64,
    pub(super) target_hot_p95_ms: f64,
    pub(super) target_hot_p99_ms: f64,
    pub(super) target_hot_max_ms: f64,
    pub(super) target_hot_workers: u64,
    pub(super) target_hot_sample_count: u64,
    pub(super) alert_hot_qps: f64,
    pub(super) critical_hot_qps: f64,
    pub(super) target_hot_error_rate: f64,
    pub(super) alert_hot_error_rate: f64,
    pub(super) critical_hot_error_rate: f64,
}

pub(super) fn current_epoch_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}
