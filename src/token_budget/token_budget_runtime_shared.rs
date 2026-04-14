use super::*;

pub(super) const CONFIG_RELATIVE_PATH: &str = "config/token_budget_profiles.toml";
pub(super) const AGENT_CYCLE_TIMELINE_MAX_POINTS: usize = 256;
pub(super) const ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS: i64 = 60_000;
pub(super) const ASSISTANT_GENERATION_TURN_OBSERVED_SNAPSHOT_KIND: &str =
    "assistant_generation_turn_observed";
pub(super) const CONTINUITY_SNAPSHOT_QUERY_LABEL: &str = "Continuity snapshot";
pub(super) const CONTINUITY_PRE_AMAI_BASELINE_STRATEGY: &str =
    "truthful_pre_amai_continuity_summaries_v1";
pub(super) const DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS: u64 = 10_000;
pub(super) const DASHBOARD_EXACT_CLIENT_LIMITS_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/exact_client_limits_cache.json";
pub(super) const DASHBOARD_EXACT_CLIENT_LIMITS_SHARED_CACHE_VERSION: &str =
    "dashboard-exact-client-limits-cache-v1";
pub(super) const DASHBOARD_TOKEN_EVENTS_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/dashboard_token_events_cache.json";
pub(super) const DASHBOARD_TOKEN_EVENTS_SHARED_CACHE_VERSION: &str =
    "dashboard-token-events-cache-v2";
pub(super) const DASHBOARD_TOKEN_EVENTS_INVALIDATION_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/dashboard_token_events_invalidation.json";
pub(super) const DASHBOARD_TOKEN_EVENTS_INVALIDATION_SHARED_CACHE_VERSION: &str =
    "dashboard-token-events-invalidation-v1";
pub(super) const DASHBOARD_CURRENT_SESSION_EVENTS_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/dashboard_current_session_events_cache.json";
pub(super) const DASHBOARD_CURRENT_SESSION_EVENTS_SHARED_CACHE_VERSION: &str =
    "dashboard-current-session-events-cache-v2";
pub(super) const CONTINUITY_RESTORE_OBSERVED_DEDUPE_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/continuity_restore_observed_dedupe.json";
pub(super) const CONTINUITY_RESTORE_OBSERVED_DEDUPE_SHARED_CACHE_VERSION: &str =
    "continuity-restore-observed-dedupe-v1";
pub(super) const CONTINUITY_RESTORE_OBSERVED_DEDUPE_TTL_MS: u64 = 30_000;
pub(super) const DASHBOARD_LIVE_TURN_RETRIEVAL_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/live_turn_retrieval_context_pack_cache.json";
pub(super) const DASHBOARD_LIVE_TURN_RETRIEVAL_SHARED_CACHE_VERSION: &str =
    "dashboard-live-turn-retrieval-cache-v1";
pub(super) const DASHBOARD_LIVE_TURN_RETRIEVAL_INVALIDATION_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/live_turn_retrieval_context_pack_invalidation.json";
pub(super) const DASHBOARD_LIVE_TURN_RETRIEVAL_INVALIDATION_SHARED_CACHE_VERSION: &str =
    "dashboard-live-turn-retrieval-invalidation-v1";
pub(super) const ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/observe/active_thread_hint.json";
pub(super) const ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION: &str = "active-thread-hint-cache-v1";
pub(super) const ACTIVE_THREAD_HINT_MAX_AGE_MS: u64 = 30 * 60 * 1000;
pub(super) const THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION: &str =
    "thread-bound-budget-snapshot-cache-v2";
pub(super) const THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION: &str =
    "thread-bound-snapshot-invalidation-v1";
pub(super) const DASHBOARD_SAME_METER_SYNC_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/dashboard_same_meter_sync_cache.json";
pub(super) const DASHBOARD_SAME_METER_SYNC_SHARED_CACHE_VERSION: &str =
    "dashboard-same-meter-sync-cache-v1";
pub(super) const DASHBOARD_CURRENT_SESSION_RECENT_EVENTS_LIMIT: i64 = 512;
pub(super) const EXACT_CLIENT_LIMIT_SAMPLE_SNAPSHOT_KIND: &str = "client_status_bar_rate_limits";
pub(super) const CLIENT_LIMIT_TREND_ANALYSIS_SNAPSHOT_KIND: &str = "client_limit_hourly_burn_trend";
pub(super) const PERSONAL_AGENT_KPI_WINDOW_HOURS: i64 = 5;
pub(super) const DEFAULT_CLIENT_LIMIT_HOURLY_BURN_WINDOW_MINUTES: u64 = 60;
pub(super) const DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MAX_LIVE_AGE_SECONDS: u64 = 10;
pub(super) const DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MIN_HISTORY_SPAN_MINUTES: u64 = 55;
pub(crate) const DEFAULT_CLIENT_LIMIT_TREND_ANALYSIS_LOOKBACK_MINUTES: u64 = 15;
pub(super) const TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS: i64 = 5_000;
pub(super) const CLI_CONTEXT_PACK_TOOL_OVERHEAD_CONTRACT_VERSION: &str =
    "context_pack_cli_model_visible_output_v2";
pub(super) const CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION: &str =
    "context_pack_cli_raw_output_v1";
pub(super) const MCP_CONTEXT_PACK_TOOL_OVERHEAD_CONTRACT_VERSION: &str =
    "context_pack_mcp_structured_content_v2";

pub(super) static DASHBOARD_ROLLOUT_OBSERVATION_CACHE: OnceLock<
    Mutex<Option<DashboardRolloutObservationCache>>,
> = OnceLock::new();
pub(super) static DASHBOARD_SAME_METER_SYNC_CACHE: OnceLock<
    Mutex<Option<DashboardSameMeterSyncCache>>,
> = OnceLock::new();
pub(super) static DASHBOARD_TOKEN_EVENTS_CACHE: OnceLock<Mutex<Option<DashboardTokenEventsCache>>> =
    OnceLock::new();
pub(super) static DASHBOARD_CURRENT_SESSION_EVENTS_CACHE: OnceLock<
    Mutex<Option<DashboardCurrentSessionEventsCache>>,
> = OnceLock::new();
pub(super) static DASHBOARD_LIVE_TURN_RETRIEVAL_CACHE: OnceLock<
    Mutex<Option<DashboardLiveTurnRetrievalCache>>,
> = OnceLock::new();
pub(super) static DASHBOARD_WORKING_STATE_METADATA_CACHE: OnceLock<
    Mutex<Option<DashboardWorkingStateMetadataCache>>,
> = OnceLock::new();
pub(super) static DASHBOARD_EXACT_CLIENT_LIMITS_CACHE: OnceLock<
    Mutex<Option<DashboardExactClientLimitsCache>>,
> = OnceLock::new();
pub(super) static TOKEN_BUDGET_CONFIG_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, CachedTokenBudgetConfig>>,
> = OnceLock::new();
