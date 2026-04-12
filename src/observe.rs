use crate::config::{AppConfig, discover_repo_root};
use crate::{
    artifact_cleanup,
    cli::{
        ContinuityClientBudgetTargetArgs, ContinuityCompactChatArgs,
        ObserveClientBudgetHostControlLaunchArgs,
    },
    codex_threads, compatibility, continuity, dashboard, external_benchmark, nats,
    observability_policy, postgres, retrieval_science, s3, token_budget, working_state,
};
use anyhow::{Context, Result, anyhow, bail};
use axum::{
    Router,
    extract::{Json, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::process::Command as ProcessCommand;
use tokio::sync::RwLock;
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Clone)]
struct ObserveState {
    dashboard_refresh_ms: u64,
    cfg: AppConfig,
    bind: String,
    cache: Arc<RwLock<ObserveCache>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ThreadBindingQuery {
    thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientBudgetTargetUpdateRequest {
    percent: u64,
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientBudgetCompactChatRequest {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    namespace: String,
    #[serde(default)]
    launch_host: bool,
    #[serde(default)]
    refresh_handoff: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ContinuityHandoffRequest {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    namespace: String,
    headline: String,
    next_step: String,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    resolved_headlines: Vec<String>,
    #[serde(default)]
    resolved_task_ids: Vec<String>,
    #[serde(default)]
    resolve_current_goal: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientBudgetHostControlFeedbackRequest {
    feedback_kind: String,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientBudgetHostControlLaunchRequest {
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentDisplayNameUpdateRequest {
    agent_scope: String,
    display_name: String,
}

#[derive(Debug, Clone, Default)]
struct ObserveCache {
    snapshot: Option<Value>,
    dashboard_payload: Option<Value>,
    dashboard_live_summary_payload: Option<Value>,
    dashboard_live_summary_thread_id: Option<String>,
    dashboard_live_summary_completed_epoch_ms: Option<u64>,
    dashboard_live_summary_refresh_in_progress: bool,
    client_budget_live_payload: Option<Value>,
    client_budget_live_thread_id: Option<String>,
    client_budget_live_completed_epoch_ms: Option<u64>,
    client_live_meter_refresh_in_progress: bool,
    client_live_meter_refresh_started_epoch_ms: Option<u64>,
    thread_bound_snapshot: Option<Value>,
    thread_bound_snapshot_thread_id: Option<String>,
    thread_bound_snapshot_completed_epoch_ms: Option<u64>,
    last_http_request_epoch_ms: Option<u64>,
    last_refresh_started_epoch_ms: Option<u64>,
    last_refresh_completed_epoch_ms: Option<u64>,
    last_refresh_duration_ms: Option<u64>,
    refresh_in_progress: bool,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CachedClientLiveMeterState {
    working_state_thread_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    ended_at_epoch_ms: Option<i64>,
    client_turn_total_tokens: Option<u64>,
    primary_limit_used_percent: Option<u64>,
    secondary_limit_used_percent: Option<u64>,
}

const SNAPSHOT_RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(3600);
const CLIENT_LIMIT_TREND_ANALYSIS_INTERVAL: Duration = Duration::from_secs(900);
const CLIENT_LIMIT_LIVE_SOURCE_TTL_MS: u64 = 3_000;
const CLIENT_BUDGET_LIVE_PAYLOAD_CACHE_TTL_MS: u64 = 20_000;
const COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS: u64 = 20_000;
const DASHBOARD_LIVE_SUMMARY_CACHE_TTL_MS: u64 = 60_000;
const OBSERVE_BACKGROUND_REFRESH_IDLE_GRACE_MS: u64 = 30_000;
const ACTIVE_AGENT_CARD_MAX_SOURCE_DRIFT_MS: i64 = 10_000;
const CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS: u64 =
    COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS;
const CLIENT_BUDGET_SURFACES_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/observe/client_budget_surfaces_cache.json";
const CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION: &str = "client-budget-surfaces-cache-v7";
const CLIENT_BUDGET_GATE_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/observe/client_budget_gate_cache.json";
const CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION: &str = "client-budget-gate-cache-v7";
const THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION: &str =
    "thread-bound-budget-snapshot-cache-v2";
const ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH: &str = "state/observe/active_thread_hint.json";
const ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION: &str = "active-thread-hint-cache-v1";
const ACTIVE_THREAD_HINT_MAX_AGE_MS: u64 = 30 * 60 * 1000;
const THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION: &str =
    "thread-bound-snapshot-invalidation-v1";
const OBSERVE_SYSTEM_SNAPSHOT_PERSIST_ADVISORY_LOCK_KEY: i64 = 4_147_508_042;
const OBSERVE_REFRESH_TIMEOUT_MS: u64 = 120_000;
const OBSERVE_REFRESH_STUCK_GRACE_MS: u64 = 5_000;

fn default_continuity_namespace() -> String {
    "continuity".to_string()
}

async fn resolve_request_repo_root_for_project(
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
struct PersistedClientBudgetSurfacesCache {
    cache_version: String,
    fetched_at_epoch_ms: u64,
    #[serde(default)]
    thread_id: Option<String>,
    root_cause: Value,
    gate: Value,
    guard: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedClientBudgetGateCache {
    cache_version: String,
    fetched_at_epoch_ms: u64,
    #[serde(default)]
    thread_id: Option<String>,
    gate: Value,
    guard: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedActiveThreadHint {
    cache_version: String,
    updated_at_epoch_ms: u64,
    thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedThreadBoundSnapshotInvalidation {
    cache_version: String,
    invalidated_at_epoch_ms: u64,
    thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedThreadBoundBudgetSnapshotCache {
    cache_version: String,
    fetched_at_epoch_ms: u64,
    thread_id: String,
    snapshot: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ObservabilityProfile {
    snapshot: SnapshotProfile,
    dashboard: DashboardProfile,
    postgres: PostgresThresholds,
    qdrant: QdrantThresholds,
    nats: NatsThresholds,
    retrieval: RetrievalThresholds,
    parser: ParserThresholds,
    accuracy: AccuracyThresholds,
    load: LoadThresholds,
}

#[derive(Debug, Clone, Deserialize)]
struct DashboardProfile {
    refresh_ms: u64,
    #[serde(default = "default_exact_client_limit_prewarm_seconds")]
    exact_client_limit_prewarm_seconds: u64,
    #[serde(default = "default_compact_client_budget_prewarm_seconds")]
    compact_client_budget_prewarm_seconds: u64,
    timing_format: DashboardTimingFormatProfile,
}

fn default_exact_client_limit_prewarm_seconds() -> u64 {
    5
}

fn default_compact_client_budget_prewarm_seconds() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
struct DashboardTimingFormatProfile {
    switch_to_nanoseconds_below_ms: f64,
    switch_to_microseconds_below_ms: f64,
    switch_to_seconds_at_or_above_ms: f64,
    non_positive_floor_label: String,
    seconds_suffix: String,
    milliseconds_suffix: String,
    microseconds_suffix: String,
    nanoseconds_suffix: String,
    seconds_decimals: u64,
    milliseconds_decimals: u64,
    microseconds_decimals: u64,
    nanoseconds_decimals: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct SnapshotProfile {
    postgres_query_probe_iterations: usize,
    nats_publish_probe_iterations: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct PostgresThresholds {
    target_connection_usage_ratio: f64,
    alert_connection_usage_ratio: f64,
    critical_connection_usage_ratio: f64,
    target_query_probe_p95_ms: f64,
    alert_query_probe_p95_ms: f64,
    critical_query_probe_p95_ms: f64,
    target_replica_lag_seconds: f64,
    alert_replica_lag_seconds: f64,
    critical_replica_lag_seconds: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct QdrantThresholds {
    target_index_optimize_queue: f64,
    alert_index_optimize_queue: f64,
    critical_index_optimize_queue: f64,
    target_search_p95_ms: f64,
    alert_search_p95_ms: f64,
    critical_search_p95_ms: f64,
    target_update_queue_length: f64,
    alert_update_queue_length: f64,
    critical_update_queue_length: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct NatsThresholds {
    target_publish_p95_ms: f64,
    alert_publish_p95_ms: f64,
    critical_publish_p95_ms: f64,
    target_consumer_lag_msgs: f64,
    alert_consumer_lag_msgs: f64,
    critical_consumer_lag_msgs: f64,
    target_jetstream_disk_usage_ratio: f64,
    alert_jetstream_disk_usage_ratio: f64,
    critical_jetstream_disk_usage_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct RetrievalThresholds {
    target_cold_p50_ms: f64,
    target_p95_ms: f64,
    target_cold_p99_ms: f64,
    target_cold_max_ms: f64,
    live_readiness_cold_sample_count: u64,
    target_cold_sample_count: u64,
    target_hot_p50_ms: f64,
    target_hot_p95_ms: f64,
    target_hot_p99_ms: f64,
    target_hot_max_ms: f64,
    live_readiness_hot_sample_count: u64,
    target_hot_sample_count: u64,
    target_hot_benchmark_iterations: u64,
    target_hot_benchmark_warmup: u64,
    alert_p95_ms: f64,
    critical_p95_ms: f64,
    stretch_hot_p95_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct ParserThresholds {
    target_coverage_ratio: f64,
    alert_coverage_ratio: f64,
    critical_coverage_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct AccuracyThresholds {
    target_symbol_precision: f64,
    alert_symbol_precision: f64,
    critical_symbol_precision: f64,
    target_semantic_precision: f64,
    alert_semantic_precision: f64,
    critical_semantic_precision: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct LoadThresholds {
    target_hot_qps: f64,
    target_hot_p50_ms: f64,
    target_hot_p95_ms: f64,
    target_hot_p99_ms: f64,
    target_hot_max_ms: f64,
    target_hot_workers: u64,
    target_hot_sample_count: u64,
    alert_hot_qps: f64,
    critical_hot_qps: f64,
    target_hot_error_rate: f64,
    alert_hot_error_rate: f64,
    critical_hot_error_rate: f64,
}

pub async fn print_snapshot(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot(cfg).await?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

pub async fn print_snapshot_preview(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot_preview(cfg).await?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

pub async fn print_budget_snapshot_preview(cfg: &AppConfig) -> Result<()> {
    let snapshot =
        if let Some(payload) = try_fetch_local_observe_budget_snapshot_preview_via_http().await {
            payload
        } else {
            compact_budget_snapshot_preview_payload(&collect_budget_snapshot_preview(cfg).await?)
        };
    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}

pub async fn run_sla_check(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot(cfg).await?;
    let summary = &snapshot["sla"]["summary"];
    let critical = summary["critical"].as_u64().unwrap_or(0);
    let unknown = summary["unknown"].as_u64().unwrap_or(0);
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    if critical > 0 || unknown > 0 {
        return Err(anyhow!(
            "sla check failed: critical={critical}, unknown={unknown}"
        ));
    }
    Ok(())
}

pub async fn print_guardrails(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let prefix = format!("observe-guardrail-{}", Uuid::new_v4());
    let result = collect_guardrail_report(&db, &prefix).await;
    let cleanup_result = cleanup_guardrail_rows(&db, &prefix).await;
    match (result, cleanup_result) {
        (Ok(report), Ok(())) => {
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(error), Err(cleanup_error)) => Err(anyhow!(
            "{error:#}\nsecondary cleanup failure: {cleanup_error:#}"
        )),
    }
}

pub async fn print_client_budget_guard(
    cfg: &AppConfig,
    enforce_reply_gate: bool,
    explicit_thread_id: Option<&str>,
) -> Result<()> {
    let CompactClientBudgetSurfaces {
        guard,
        guard_payload: payload,
        ..
    } = if let Some(thread_id) = resolved_local_observe_thread_id(explicit_thread_id) {
        let repo_root = discover_repo_root(None)?;
        if let Some(materialized) =
            try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                &repo_root, &thread_id,
            )
        {
            materialized.surfaces
        } else {
            let snapshot = collect_client_budget_snapshot_with_thread_hint(
                cfg,
                &repo_root,
                Some(&thread_id),
                None,
                None,
            )
            .await?;
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(&thread_id))
                .surfaces
        }
    } else {
        collect_compact_client_budget_surfaces(cfg).await?
    };
    println!("{}", serde_json::to_string(&payload)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&guard) {
        let action_kind = guard["reply_execution_gate"]["action_kind"]
            .as_str()
            .unwrap_or("continue_current_chat");
        let blocked_reply_hint = match action_kind {
            "wait_for_global_client_budget_recovery" => {
                "wait for global client budget recovery before replying"
            }
            "rotate_chat_for_client_budget" => "rotate into a fresh chat before replying",
            _ => "refresh the live client budget gate before replying",
        };
        return Err(anyhow!(
            "client budget reply gate blocked this reply: {blocked_reply_hint}"
        ));
    }
    Ok(())
}

pub async fn print_client_budget_gate(
    cfg: &AppConfig,
    enforce_reply_gate: bool,
    explicit_thread_id: Option<&str>,
) -> Result<()> {
    let payload = if let Some(payload) =
        try_fetch_local_observe_gate_payload_via_http(explicit_thread_id).await
    {
        payload
    } else if let Some(thread_id) = resolved_local_observe_thread_id(explicit_thread_id) {
        let repo_root = discover_repo_root(None)?;
        if let Some(cached) = load_shared_compact_client_budget_gate(
            &repo_root,
            current_epoch_ms_u64(),
            Some(&thread_id),
        ) {
            cached.gate
        } else if let Some(materialized) =
            try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                &repo_root, &thread_id,
            )
        {
            materialized.gate.gate_payload
        } else {
            let snapshot = collect_client_budget_snapshot_with_thread_hint(
                cfg,
                &repo_root,
                Some(&thread_id),
                None,
                None,
            )
            .await?;
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(&thread_id))
                .gate
                .gate_payload
        }
    } else {
        let CompactClientBudgetGateSurface { gate_payload, .. } =
            collect_compact_client_budget_gate_surface(cfg).await?;
        gate_payload
    };
    let payload = normalize_front_door_client_budget_gate_payload_shape(payload);
    println!("{}", serde_json::to_string(&payload)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&payload["client_budget_reply_gate"])
    {
        let action_kind =
            payload["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"]
                .as_str()
                .unwrap_or("continue_current_chat");
        let blocked_reply_hint = match action_kind {
            "wait_for_global_client_budget_recovery" => {
                "wait for global client budget recovery before replying"
            }
            "rotate_chat_for_client_budget" => "rotate into a fresh chat before replying",
            _ => "refresh the live client budget gate before replying",
        };
        return Err(anyhow!(
            "client budget reply gate blocked this reply: {blocked_reply_hint}"
        ));
    }
    Ok(())
}

pub async fn print_client_budget_root_cause(
    cfg: &AppConfig,
    enforce_reply_gate: bool,
    explicit_thread_id: Option<&str>,
) -> Result<()> {
    let compact = if let Some(payload) =
        try_fetch_local_observe_root_cause_payload_via_http(explicit_thread_id).await
    {
        payload
    } else if let Some(thread_id) = resolved_local_observe_thread_id(explicit_thread_id) {
        let repo_root = discover_repo_root(None)?;
        if let Some(materialized) =
            try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                &repo_root, &thread_id,
            )
        {
            materialized.surfaces.root_cause_payload
        } else {
            let snapshot = collect_client_budget_snapshot_with_thread_hint(
                cfg,
                &repo_root,
                Some(&thread_id),
                None,
                None,
            )
            .await?;
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(&thread_id))
                .surfaces
                .root_cause_payload
        }
    } else {
        collect_compact_client_budget_surfaces(cfg)
            .await?
            .root_cause_payload
    };
    println!("{}", serde_json::to_string(&compact)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&compact["client_budget_reply_gate"])
    {
        let action_kind =
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"]
                .as_str()
                .unwrap_or("continue_current_chat");
        let blocked_reply_hint = match action_kind {
            "wait_for_global_client_budget_recovery" => {
                "wait for global client budget recovery before replying"
            }
            "rotate_chat_for_client_budget" => "rotate into a fresh chat before replying",
            _ => "refresh the live client budget gate before replying",
        };
        return Err(anyhow!(
            "client budget reply gate blocked this reply: {blocked_reply_hint}"
        ));
    }
    Ok(())
}

fn client_budget_guard_blocks_reply(guard: &Value) -> bool {
    working_state::client_budget_guard_blocks_reply(guard)
}

#[derive(Debug, Clone)]
struct CompactClientBudgetSurfaces {
    root_cause_payload: Value,
    guard_payload: Value,
    guard: Value,
}

#[derive(Debug, Clone)]
struct CompactClientBudgetGateSurface {
    gate_payload: Value,
}

#[derive(Debug, Clone)]
struct MaterializedCompactClientBudgetSurfaces {
    surfaces: CompactClientBudgetSurfaces,
    gate: CompactClientBudgetGateSurface,
}

fn current_epoch_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

fn local_observe_http_base_url() -> String {
    let observe_bind =
        std::env::var("AMI_OBSERVE_BIND").unwrap_or_else(|_| "0.0.0.0:9464".to_string());
    let (raw_host, raw_port) = observe_bind
        .rsplit_once(':')
        .map(|(host, port)| (host.trim(), port.trim()))
        .unwrap_or(("0.0.0.0", "9464"));
    let host = match raw_host {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        value if value.starts_with('[') && value.ends_with(']') && value.len() > 2 => {
            value[1..value.len() - 1].to_string()
        }
        value => value.to_string(),
    };
    let port = if raw_port.is_empty() {
        "9464"
    } else {
        raw_port
    };
    if host.contains(':') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

fn local_observe_thread_id_from_env() -> Option<String> {
    std::env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolved_local_observe_thread_id(explicit_thread_id: Option<&str>) -> Option<String> {
    explicit_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(local_observe_thread_id_from_env)
}

fn client_budget_local_observe_http_timeout(thread_bound: bool) -> Duration {
    let fallback_ms = if thread_bound { 7000 } else { 1500 };
    let override_ms = std::env::var("AMI_CLIENT_BUDGET_OBSERVE_HTTP_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(200, 20_000));
    Duration::from_millis(override_ms.unwrap_or(fallback_ms))
}

async fn try_fetch_local_observe_gate_payload_via_http(
    explicit_thread_id: Option<&str>,
) -> Option<Value> {
    let thread_id = resolved_local_observe_thread_id(explicit_thread_id);
    let repo_root = discover_repo_root(None).ok()?;
    let now_epoch_ms = current_epoch_ms_u64();
    if let Some(cached) =
        load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms, thread_id.as_deref())
    {
        if compact_thread_bound_client_budget_gate_payload_is_consistent(
            thread_id.as_deref(),
            &cached.gate,
        ) {
            return Some(cached.gate);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(client_budget_local_observe_http_timeout(
            thread_id.is_some(),
        ))
        .build()
        .ok()?;
    let base_url = local_observe_http_base_url();
    let root_cause_request = client.get(format!("{base_url}/api/client-budget-root-cause"));
    let root_cause = (if let Some(thread_id) = thread_id.as_deref() {
        root_cause_request.query(&[("thread_id", thread_id)])
    } else {
        root_cause_request
    })
    .send()
    .await
    .ok()?
    .error_for_status()
    .ok()?
    .json::<Value>()
    .await
    .ok()?;
    if let Some(gate) = compact_cli_client_budget_gate_from_root_cause_payload(&root_cause)
        && compact_thread_bound_client_budget_gate_payload_is_consistent(
            thread_id.as_deref(),
            &gate,
        )
    {
        return Some(gate);
    }
    let gate_request = client.get(format!("{base_url}/api/client-budget-gate"));
    let response = (if let Some(thread_id) = thread_id.as_deref() {
        gate_request.query(&[("thread_id", thread_id)])
    } else {
        gate_request
    })
    .send()
    .await
    .ok()?;
    let gate = response
        .error_for_status()
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    compact_thread_bound_client_budget_gate_payload_is_consistent(thread_id.as_deref(), &gate)
        .then_some(gate)
}

async fn try_fetch_local_observe_root_cause_payload_via_http(
    explicit_thread_id: Option<&str>,
) -> Option<Value> {
    let thread_id = resolved_local_observe_thread_id(explicit_thread_id);
    let repo_root = discover_repo_root(None).ok()?;
    let now_epoch_ms = current_epoch_ms_u64();
    if let Some(cached) =
        load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms, thread_id.as_deref())
    {
        if compact_thread_bound_client_budget_root_cause_payload_is_consistent(
            thread_id.as_deref(),
            &cached.root_cause,
        ) {
            return Some(cached.root_cause);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(client_budget_local_observe_http_timeout(
            thread_id.is_some(),
        ))
        .build()
        .ok()?;
    let base_url = local_observe_http_base_url();
    let request = client.get(format!("{base_url}/api/client-budget-root-cause"));
    let payload = (if let Some(thread_id) = thread_id.as_deref() {
        request.query(&[("thread_id", thread_id)])
    } else {
        request
    })
    .send()
    .await
    .ok()?
    .error_for_status()
    .ok()?
    .json::<Value>()
    .await
    .ok()?;
    compact_thread_bound_client_budget_root_cause_payload_is_consistent(
        thread_id.as_deref(),
        &payload,
    )
    .then_some(payload)
}

async fn try_fetch_local_observe_budget_snapshot_preview_via_http() -> Option<Value> {
    let thread_id = local_observe_thread_id_from_env();
    let repo_root = discover_repo_root(None).ok()?;
    if let Some(snapshot) = load_shared_budget_snapshot_preview(&repo_root, thread_id.as_deref()) {
        return Some(compact_budget_snapshot_preview_payload(&snapshot));
    }
    let client = reqwest::Client::builder()
        .timeout(client_budget_local_observe_http_timeout(
            thread_id.is_some(),
        ))
        .build()
        .ok()?;
    let base_url = local_observe_http_base_url();
    let request = client.get(format!("{base_url}/api/client-budget-snapshot-preview"));
    (if let Some(thread_id) = thread_id.as_deref() {
        request.query(&[("thread_id", thread_id)])
    } else {
        request
    })
    .send()
    .await
    .ok()?
    .error_for_status()
    .ok()?
    .json::<Value>()
    .await
    .ok()
}

fn observe_cache_thread_suffix(thread_id: &str) -> String {
    thread_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn client_budget_surfaces_shared_cache_path(repo_root: &Path, thread_id: Option<&str>) -> PathBuf {
    if let Some(thread_id) = thread_id.map(str::trim).filter(|value| !value.is_empty()) {
        return repo_root.join(format!(
            "state/observe/client_budget_surfaces_cache.thread-{}.json",
            observe_cache_thread_suffix(thread_id)
        ));
    }
    repo_root.join(CLIENT_BUDGET_SURFACES_SHARED_CACHE_RELATIVE_PATH)
}

fn build_compact_client_budget_surfaces_cache(
    root_cause_payload: &Value,
    gate_payload: &Value,
    guard_payload: &Value,
    thread_id: Option<&str>,
) -> PersistedClientBudgetSurfacesCache {
    PersistedClientBudgetSurfacesCache {
        cache_version: CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.map(str::to_string),
        root_cause: root_cause_payload.clone(),
        gate: gate_payload.clone(),
        guard: guard_payload.clone(),
    }
}

fn client_budget_gate_shared_cache_path(repo_root: &Path, thread_id: Option<&str>) -> PathBuf {
    if let Some(thread_id) = thread_id.map(str::trim).filter(|value| !value.is_empty()) {
        return repo_root.join(format!(
            "state/observe/client_budget_gate_cache.thread-{}.json",
            observe_cache_thread_suffix(thread_id)
        ));
    }
    repo_root.join(CLIENT_BUDGET_GATE_SHARED_CACHE_RELATIVE_PATH)
}

fn active_thread_hint_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH)
}

fn thread_bound_snapshot_invalidation_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_snapshot_invalidation.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

fn thread_bound_budget_snapshot_shared_cache_path(repo_root: &Path, thread_id: &str) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_budget_snapshot.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

fn load_shared_active_thread_hint(repo_root: &Path, now_epoch_ms: u64) -> Option<String> {
    let path = active_thread_hint_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedActiveThreadHint = serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION {
        return None;
    }
    let thread_id = persisted.thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    if now_epoch_ms.saturating_sub(persisted.updated_at_epoch_ms) > ACTIVE_THREAD_HINT_MAX_AGE_MS {
        return None;
    }
    Some(thread_id.to_string())
}

fn write_shared_active_thread_hint(repo_root: &Path, thread_id: &str) -> Result<()> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok(());
    }
    let path = active_thread_hint_shared_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedActiveThreadHint {
        cache_version: ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION.to_string(),
        updated_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.to_string(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn load_shared_thread_bound_snapshot_invalidation(
    repo_root: &Path,
    thread_id: &str,
) -> Option<u64> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    let path = thread_bound_snapshot_invalidation_shared_cache_path(repo_root, thread_id);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedThreadBoundSnapshotInvalidation =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION {
        return None;
    }
    if persisted.thread_id.trim() != thread_id {
        return None;
    }
    Some(persisted.invalidated_at_epoch_ms)
}

fn write_shared_thread_bound_snapshot_invalidation(
    repo_root: &Path,
    thread_id: &str,
) -> Result<()> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok(());
    }
    let path = thread_bound_snapshot_invalidation_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedThreadBoundSnapshotInvalidation {
        cache_version: THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION.to_string(),
        invalidated_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.to_string(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn load_shared_thread_bound_budget_snapshot(
    repo_root: &Path,
    now_epoch_ms: u64,
    thread_id: &str,
) -> Option<Value> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    let path = thread_bound_budget_snapshot_shared_cache_path(repo_root, thread_id);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedThreadBoundBudgetSnapshotCache = serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION {
        return None;
    }
    if persisted.thread_id.trim() != thread_id {
        return None;
    }
    if now_epoch_ms.saturating_sub(persisted.fetched_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return None;
    }
    if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id).is_some_and(
        |invalidated_at_epoch_ms| invalidated_at_epoch_ms >= persisted.fetched_at_epoch_ms,
    ) {
        return None;
    }
    if !thread_bound_budget_snapshot_has_fresh_exact_limit_surfaces(
        &persisted.snapshot,
        now_epoch_ms,
    ) {
        return None;
    }
    Some(persisted.snapshot)
}

fn write_shared_thread_bound_budget_snapshot(
    repo_root: &Path,
    thread_id: &str,
    snapshot: &Value,
) -> Result<()> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok(());
    }
    let path = thread_bound_budget_snapshot_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedThreadBoundBudgetSnapshotCache {
        cache_version: THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.to_string(),
        snapshot: snapshot.clone(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn thread_bound_budget_snapshot_has_fresh_exact_limit_surfaces(
    snapshot: &Value,
    now_epoch_ms: u64,
) -> bool {
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let hourly_burn = &report["client_limit_hourly_burn"];
    if hourly_burn["status"].as_str() != Some("observed") {
        return false;
    }
    let Some(hourly_burn_observed_at_epoch_ms) =
        hourly_burn["latest_observed_at_epoch_ms"].as_u64()
    else {
        return false;
    };
    if now_epoch_ms.saturating_sub(hourly_burn_observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return false;
    }
    let status_bar_rate_limits = &report["client_live_meter"]["status_bar_rate_limits"];
    if status_bar_rate_limits["status"].as_str() != Some("observed") {
        return false;
    }
    let Some(status_bar_observed_at_epoch_ms) = status_bar_rate_limits["observed_at_epoch_ms"]
        .as_u64()
        .or_else(|| status_bar_rate_limits["ended_at_epoch_ms"].as_u64())
    else {
        return false;
    };
    if now_epoch_ms.saturating_sub(status_bar_observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return false;
    }
    let current_live_turn = &report["current_live_turn"];
    if !current_live_turn.is_object() {
        return false;
    }
    let status = current_live_turn["status"].as_str().unwrap_or_default();
    let exact_pair_available = current_live_turn["exact_pair_available"].as_bool() == Some(true);
    let observed_activity = current_live_turn["matched_events_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
        || current_live_turn["retrieval_context_pack_count"]
            .as_u64()
            .unwrap_or(0)
            > 0;
    if observed_activity {
        return status == "exact_pair_materialized" && exact_pair_available;
    }
    matches!(
        status,
        "no_amai_activity_in_current_live_turn" | "exact_pair_materialized"
    ) && exact_pair_available
}

fn build_compact_client_budget_gate_cache(
    gate_payload: &Value,
    guard_payload: &Value,
    thread_id: Option<&str>,
) -> PersistedClientBudgetGateCache {
    PersistedClientBudgetGateCache {
        cache_version: CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.map(str::to_string),
        gate: gate_payload.clone(),
        guard: guard_payload.clone(),
    }
}

fn shared_client_budget_cache_matches_thread(
    cached_thread_id: Option<&str>,
    expected_thread_id: Option<&str>,
) -> bool {
    match (
        cached_thread_id
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        expected_thread_id
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(cached), Some(expected)) => cached == expected,
        (None, None) => true,
        _ => false,
    }
}

fn load_shared_compact_client_budget_gate(
    repo_root: &Path,
    now_epoch_ms: u64,
    expected_thread_id: Option<&str>,
) -> Option<PersistedClientBudgetGateCache> {
    let path = client_budget_gate_shared_cache_path(repo_root, expected_thread_id);
    let payload = fs::read_to_string(path).ok()?;
    let cached: PersistedClientBudgetGateCache = serde_json::from_str(&payload).ok()?;
    if cached.cache_version != CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION {
        return None;
    }
    if !shared_client_budget_cache_matches_thread(cached.thread_id.as_deref(), expected_thread_id) {
        return None;
    }
    if now_epoch_ms.saturating_sub(cached.fetched_at_epoch_ms)
        > CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS
    {
        return None;
    }
    let observed_at_epoch_ms = cached.gate["client_budget_reply_gate"]["observed_at_epoch_ms"]
        .as_u64()
        .or_else(|| cached.guard["observed_at_epoch_ms"].as_u64())?;
    if now_epoch_ms.saturating_sub(observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return None;
    }
    if let Some(thread_id) = expected_thread_id {
        if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id).is_some_and(
            |invalidated_at_epoch_ms| invalidated_at_epoch_ms >= cached.fetched_at_epoch_ms,
        ) {
            return None;
        }
    }
    if !compact_thread_bound_client_budget_gate_payload_is_consistent(
        expected_thread_id,
        &cached.gate,
    ) {
        return None;
    }
    Some(cached)
}

fn write_shared_compact_client_budget_gate(
    repo_root: &Path,
    thread_id: Option<&str>,
    cache: &PersistedClientBudgetGateCache,
) -> Result<()> {
    let path = client_budget_gate_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec(cache)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn load_shared_compact_client_budget_surfaces(
    repo_root: &Path,
    now_epoch_ms: u64,
    expected_thread_id: Option<&str>,
) -> Option<PersistedClientBudgetSurfacesCache> {
    let path = client_budget_surfaces_shared_cache_path(repo_root, expected_thread_id);
    let payload = fs::read_to_string(path).ok()?;
    let cached: PersistedClientBudgetSurfacesCache = serde_json::from_str(&payload).ok()?;
    if cached.cache_version != CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION {
        return None;
    }
    if !shared_client_budget_cache_matches_thread(cached.thread_id.as_deref(), expected_thread_id) {
        return None;
    }
    if now_epoch_ms.saturating_sub(cached.fetched_at_epoch_ms)
        > CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS
    {
        return None;
    }
    let observed_at_epoch_ms =
        cached.root_cause["client_budget_reply_gate"]["observed_at_epoch_ms"]
            .as_u64()
            .or_else(|| cached.gate["client_budget_reply_gate"]["observed_at_epoch_ms"].as_u64())
            .or_else(|| cached.guard["observed_at_epoch_ms"].as_u64())?;
    if now_epoch_ms.saturating_sub(observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return None;
    }
    if let Some(thread_id) = expected_thread_id {
        if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id).is_some_and(
            |invalidated_at_epoch_ms| invalidated_at_epoch_ms >= cached.fetched_at_epoch_ms,
        ) {
            return None;
        }
    }
    if !compact_thread_bound_client_budget_root_cause_payload_is_consistent(
        expected_thread_id,
        &cached.root_cause,
    ) {
        return None;
    }
    Some(cached)
}

fn other_thread_feedback_confirmation_is_inconsistent(
    action_kind: Option<&str>,
    must_confirm_feedback: bool,
    feedback_pending: bool,
    effect_verdict: Option<&str>,
) -> bool {
    effect_verdict == Some("other_thread")
        && (feedback_pending
            || must_confirm_feedback
            || action_kind == Some("confirm_same_thread_host_control_feedback"))
}

fn compact_thread_bound_client_budget_gate_payload_is_consistent(
    expected_thread_id: Option<&str>,
    payload: &Value,
) -> bool {
    if expected_thread_id.is_none() {
        return true;
    }
    let gate = &payload["client_budget_reply_gate"]["reply_execution_gate"];
    !other_thread_feedback_confirmation_is_inconsistent(
        gate["action_kind"].as_str(),
        gate["must_confirm_same_thread_host_control_feedback_before_reply"].as_bool() == Some(true),
        gate["action_bundle"]["host_current_thread_control"]["feedback_pending"].as_bool()
            == Some(true),
        gate["action_bundle"]["host_current_thread_control"]["effect_verdict"].as_str(),
    )
}

fn compact_thread_bound_client_budget_root_cause_payload_is_consistent(
    expected_thread_id: Option<&str>,
    payload: &Value,
) -> bool {
    if expected_thread_id.is_none() {
        return true;
    }
    if payload["thread_binding_state"].as_str() != Some("current_thread_bound")
        || payload["current_live_turn"]["status"].as_str() == Some("current_thread_unbound")
    {
        return false;
    }
    let gate = &payload["client_budget_reply_gate"]["reply_execution_gate"];
    !other_thread_feedback_confirmation_is_inconsistent(
        gate["action_kind"].as_str(),
        gate["must_confirm_same_thread_host_control_feedback_before_reply"].as_bool() == Some(true),
        gate["action_bundle"]["host_current_thread_control"]["feedback_pending"].as_bool()
            == Some(true),
        payload["host_current_thread_control_effect"]["effect_verdict"].as_str(),
    )
}

fn write_shared_compact_client_budget_surfaces(
    repo_root: &Path,
    thread_id: Option<&str>,
    cache: &PersistedClientBudgetSurfacesCache,
) -> Result<()> {
    let path = client_budget_surfaces_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec(cache)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

async fn collect_compact_client_budget_gate_surface(
    cfg: &AppConfig,
) -> Result<CompactClientBudgetGateSurface> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms = current_epoch_ms_u64();
    if let Some(cached) = load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms, None) {
        return Ok(CompactClientBudgetGateSurface {
            gate_payload: cached.gate,
        });
    }
    if let Some(cached) = load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms, None)
    {
        let gate_cache = build_compact_client_budget_gate_cache(&cached.gate, &cached.guard, None);
        let _ = write_shared_compact_client_budget_gate(&repo_root, None, &gate_cache);
        return Ok(CompactClientBudgetGateSurface {
            gate_payload: cached.gate,
        });
    }
    let snapshot = collect_client_budget_snapshot(cfg, &repo_root).await?;
    Ok(compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, None).gate)
}

async fn collect_compact_client_budget_surfaces(
    cfg: &AppConfig,
) -> Result<CompactClientBudgetSurfaces> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms = current_epoch_ms_u64();
    if let Some(cached) = load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms, None)
    {
        return Ok(CompactClientBudgetSurfaces {
            root_cause_payload: cached.root_cause,
            guard_payload: cached.guard.clone(),
            guard: cached.guard,
        });
    }
    let snapshot = collect_client_budget_snapshot(cfg, &repo_root).await?;
    Ok(compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, None).surfaces)
}

async fn prewarm_thread_bound_client_budget_surfaces_for_thread(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: &AppConfig,
    thread_id: &str,
) -> Result<()> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms_value = current_epoch_ms_u64();
    if load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms_value, Some(thread_id))
        .is_some()
        && let Some(cached_gate) =
            load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms_value, Some(thread_id))
    {
        let maybe_launched = maybe_auto_launch_same_thread_host_control_from_gate(
            cfg,
            &repo_root,
            thread_id,
            &cached_gate.gate,
        )
        .await?;
        if maybe_launched.is_none() {
            return Ok(());
        }
        return Ok(());
    }
    if let Some(snapshot) =
        load_shared_thread_bound_budget_snapshot(&repo_root, now_epoch_ms_value, thread_id)
    {
        let materialized =
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(thread_id));
        let _ = maybe_auto_launch_same_thread_host_control_from_gate(
            cfg,
            &repo_root,
            thread_id,
            &materialized.gate.gate_payload,
        )
        .await?;
        populate_thread_bound_client_budget_surfaces_from_snapshot(
            cache, &repo_root, thread_id, snapshot,
        )
        .await;
        return Ok(());
    }

    let (latest_repo_restore_override, base_report_override) = {
        let state = cache.read().await;
        (
            cached_latest_repo_working_state_restore_snapshot(&state),
            cached_token_budget_report_snapshot(&state),
        )
    };
    let snapshot = collect_client_budget_snapshot_with_thread_hint(
        cfg,
        &repo_root,
        Some(thread_id),
        base_report_override.as_ref(),
        latest_repo_restore_override.as_ref(),
    )
    .await?;
    let materialized =
        compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(thread_id));
    let _ = maybe_auto_launch_same_thread_host_control_from_gate(
        cfg,
        &repo_root,
        thread_id,
        &materialized.gate.gate_payload,
    )
    .await?;
    populate_thread_bound_client_budget_surfaces_from_snapshot(
        cache, &repo_root, thread_id, snapshot,
    )
    .await;
    Ok(())
}

async fn prewarm_active_thread_bound_client_budget_surfaces(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: &AppConfig,
) -> Result<()> {
    let (snapshot_thread_id, activity) = {
        let state = cache.read().await;
        let snapshot = state.snapshot.as_ref();
        (
            match snapshot {
                Some(value) => strict_auto_thread_binding_hint_from_snapshot(value.clone()),
                None => None,
            },
            snapshot.map(|item| item["agent_scope_activity"].clone()),
        )
    };
    if let Some(activity) = activity.as_ref() {
        let thread_ids = token_budget::active_agent_thread_ids_from_activity(
            activity,
            current_epoch_ms_u64() as i64,
        );
        if !thread_ids.is_empty() {
            return prewarm_active_agent_thread_bound_client_budget_surfaces(cache, cfg, activity)
                .await;
        }
    }
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms_value = current_epoch_ms_u64();
    let Some(thread_id) = snapshot_thread_id
        .or_else(|| load_shared_active_thread_hint(&repo_root, now_epoch_ms_value))
    else {
        return Ok(());
    };
    prewarm_thread_bound_client_budget_surfaces_for_thread(cache, cfg, &thread_id).await?;
    Ok(())
}

async fn prewarm_active_agent_thread_bound_client_budget_surfaces(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: &AppConfig,
    activity: &Value,
) -> Result<()> {
    let thread_ids = token_budget::active_agent_thread_ids_from_activity(
        activity,
        current_epoch_ms_u64() as i64,
    );
    for thread_id in thread_ids {
        prewarm_thread_bound_client_budget_surfaces_for_thread(cache.clone(), cfg, &thread_id)
            .await?;
    }
    Ok(())
}

async fn collect_client_budget_snapshot(cfg: &AppConfig, repo_root: &Path) -> Result<Value> {
    collect_client_budget_snapshot_with_thread_hint(cfg, repo_root, None, None, None).await
}

async fn collect_client_budget_snapshot_with_thread_hint(
    cfg: &AppConfig,
    repo_root: &Path,
    thread_id_hint: Option<&str>,
    base_report_override: Option<&Value>,
    latest_repo_restore_override: Option<&Value>,
) -> Result<Value> {
    if let Ok(db) = postgres::connect_app(cfg).await {
        if let Ok(snapshot) = collect_client_budget_snapshot_from_db(
            &db,
            repo_root,
            thread_id_hint,
            base_report_override,
            latest_repo_restore_override,
        )
        .await
        {
            return Ok(snapshot);
        }
    }

    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    collect_client_budget_snapshot_from_db(
        &db,
        repo_root,
        thread_id_hint,
        base_report_override,
        latest_repo_restore_override,
    )
    .await
}

async fn collect_client_budget_snapshot_from_db(
    db: &Client,
    repo_root: &Path,
    thread_id_hint: Option<&str>,
    base_report_override: Option<&Value>,
    latest_repo_restore_override: Option<&Value>,
) -> Result<Value> {
    let latest_repo_restore_raw = latest_repo_restore_override
        .cloned()
        .or(latest_repo_working_state_restore_payload(&db, repo_root).await?);
    working_state::maintain_same_thread_execctl_active_lease_for_guard(
        db,
        latest_repo_restore_raw.as_ref(),
        thread_id_hint,
    )
    .await?;
    let report =
        token_budget::collect_dashboard_current_session_budget_report_with_thread_hint_and_base(
            &db,
            base_report_override,
            thread_id_hint,
        )
        .await?;
    let agent_scope_activity = token_budget::collect_agent_scope_activity(db).await?;
    let active_agent_budget = token_budget::collect_active_agent_live_budget_surface(
        db,
        repo_root,
        &agent_scope_activity,
    )
    .await?;
    let latest_repo_working_state_restore =
        latest_repo_working_state_restore_payload(&db, repo_root)
            .await?
            .map(|value| compact_latest_repo_working_state_restore_for_budget(&value))
            .unwrap_or_else(|| json!({ "working_state_restore": {} }));
    Ok(json!({
        "token_budget_report": {
            "token_budget_report": report["token_budget_report"].clone(),
        },
        "active_agent_budget": active_agent_budget,
        "latest_repo_working_state_restore": latest_repo_working_state_restore,
    }))
}

fn compact_latest_repo_working_state_restore_for_budget(payload: &Value) -> Value {
    json!({
        "working_state_restore": compact_working_state_restore_for_budget(
            &payload["working_state_restore"]
        )
    })
}

fn compact_working_state_restore_for_budget(restore: &Value) -> Value {
    if !restore.is_object() {
        return json!({});
    }

    let recent_actions = restore["recent_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| {
            action["source_kind"].as_str()
                == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND)
                && action["host_current_thread_control_feedback"].is_object()
        })
        .map(|action| {
            json!({
                "source_kind": action["source_kind"].clone(),
                "summary": action["summary"].clone(),
                "recorded_at_epoch_ms": action["recorded_at_epoch_ms"].clone(),
                "host_current_thread_control_feedback": {
                    "feedback_kind": action["host_current_thread_control_feedback"]["feedback_kind"].clone(),
                    "command_id": action["host_current_thread_control_feedback"]["command_id"].clone(),
                    "feedback_snapshot": {
                        "thread_id": action["host_current_thread_control_feedback"]["feedback_snapshot"]["thread_id"].clone(),
                        "client_live_meter": {
                            "client_turn_total_tokens":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]["client_turn_total_tokens"].clone(),
                            "context_used_percent":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]["context_used_percent"].clone(),
                            "primary_limit_used_percent":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]["primary_limit_used_percent"].clone()
                        },
                        "host_context_compaction": {
                            "compaction_count":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["compaction_count"].clone(),
                            "growth_since_compaction_tokens":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["growth_since_compaction_tokens"].clone(),
                            "compacted_at_epoch_ms":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["compacted_at_epoch_ms"].clone(),
                            "stage":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["stage"].clone()
                        }
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    json!({
        "client_budget_target_percent": restore["client_budget_target_percent"].clone(),
        "thread_id": restore["thread_id"].clone(),
        "current_goal": restore["current_goal"].clone(),
        "next_step": restore["next_step"].clone(),
        "execctl_resume_state": restore["execctl_resume_state"].clone(),
        "project": {
            "code": restore["project"]["code"].clone(),
            "repo_root": restore["project"]["repo_root"].clone()
        },
        "namespace": {
            "code": restore["namespace"]["code"].clone()
        },
        "state_lineage": {
            "authoritative_event_id": restore["state_lineage"]["authoritative_event_id"].clone(),
            "authoritative_source_kind": restore["state_lineage"]["authoritative_source_kind"].clone(),
            "authoritative_local_path": restore["state_lineage"]["authoritative_local_path"].clone()
        },
        "recent_actions": recent_actions
    })
}

fn compact_current_session_for_budget_snapshot_preview(current_session: &Value) -> Value {
    if !current_session.is_object() {
        return Value::Null;
    }
    json!({
        "started_at_epoch_ms": current_session["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": current_session["ended_at_epoch_ms"].clone(),
        "counted_events": current_session["counted_events"].clone(),
        "live_events_count": current_session["live_events_count"].clone(),
        "total_saved_tokens": current_session["total_saved_tokens"].clone(),
        "savings_percent": rounded_json_number(&current_session["savings_percent"], 2),
        "effective_savings_pct": rounded_json_number(&current_session["effective_savings_pct"], 2),
        "observed_client_prompt_tokens": current_session["observed_client_prompt_tokens"].clone(),
        "observed_tool_overhead_tokens": current_session["observed_tool_overhead_tokens"].clone(),
        "observed_continuity_restore_tokens": current_session["observed_continuity_restore_tokens"].clone(),
        "observed_assistant_generation_tokens": current_session["observed_assistant_generation_tokens"].clone(),
        "age_ms_since_latest": current_session["age_ms_since_latest"].clone()
    })
}

fn compact_client_live_meter_for_budget_snapshot_preview(client_live_meter: &Value) -> Value {
    if !client_live_meter.is_object() {
        return Value::Null;
    }
    json!({
        "status": client_live_meter["status"].clone(),
        "thread_binding_state": client_live_meter["thread_binding_state"].clone(),
        "current_thread_bound": client_live_meter["current_thread_bound"].clone(),
        "thread_id": client_live_meter["thread_id"].clone(),
        "turn_id": client_live_meter["turn_id"].clone(),
        "started_at_epoch_ms": client_live_meter["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": client_live_meter["ended_at_epoch_ms"].clone(),
        "client_turn_total_tokens": client_live_meter["client_turn_total_tokens"].clone(),
        "context_used_percent": rounded_json_number(&client_live_meter["context_used_percent"], 2),
        "primary_limit_used_percent": client_live_meter["primary_limit_used_percent"].clone(),
        "secondary_limit_used_percent": client_live_meter["secondary_limit_used_percent"].clone(),
        "rollout_jsonl_tolerance_summary": client_live_meter["rollout_jsonl_tolerance_summary"].clone(),
        "rollout_jsonl_tolerated_skips_present": client_live_meter["rollout_jsonl_tolerated_skips_present"].clone(),
        "rollout_jsonl_malformed_objects_fail_closed": client_live_meter["rollout_jsonl_malformed_objects_fail_closed"].clone()
    })
}

fn compact_current_live_turn_for_budget_snapshot_preview(current_live_turn: &Value) -> Value {
    if !current_live_turn.is_object() {
        return Value::Null;
    }
    json!({
        "status": current_live_turn["status"].clone(),
        "scope_code": current_live_turn["scope_code"].clone(),
        "thread_binding_state": current_live_turn["thread_binding_state"].clone(),
        "current_thread_bound": current_live_turn["current_thread_bound"].clone(),
        "thread_id": current_live_turn["thread_id"].clone(),
        "turn_id": current_live_turn["turn_id"].clone(),
        "started_at_epoch_ms": current_live_turn["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": current_live_turn["ended_at_epoch_ms"].clone(),
        "exact_pair_available": current_live_turn["exact_pair_available"].clone(),
        "exact_pair": current_live_turn["exact_pair"].clone(),
        "matched_events_count": current_live_turn["matched_events_count"].clone(),
        "matched_context_pack_ids_count": current_live_turn["matched_context_pack_ids_count"].clone(),
        "retrieval_context_pack_count": current_live_turn["retrieval_context_pack_count"].clone()
    })
}

fn compact_client_limit_hourly_burn_for_budget_snapshot_preview(
    client_limit_hourly_burn: &Value,
) -> Value {
    if !client_limit_hourly_burn.is_object() {
        return Value::Null;
    }
    json!({
        "status": client_limit_hourly_burn["status"].clone(),
        "reply_prefix": client_limit_hourly_burn["reply_prefix"].clone(),
        "kpi_percent": rounded_json_number(&client_limit_hourly_burn["kpi_percent"], 2),
        "actual_used_percent": rounded_json_number(&client_limit_hourly_burn["actual_used_percent"], 2),
        "actual_remaining_percent":
            rounded_json_number(&client_limit_hourly_burn["actual_remaining_percent"], 2),
        "ideal_used_percent": rounded_json_number(&client_limit_hourly_burn["ideal_used_percent"], 2),
        "ideal_remaining_percent":
            rounded_json_number(&client_limit_hourly_burn["ideal_remaining_percent"], 2),
        "projected_primary_used_per_hour_percent": rounded_json_number(
            &client_limit_hourly_burn["projected_primary_used_per_hour_percent"],
            2
        ),
        "ideal_primary_used_per_hour_percent": rounded_json_number(
            &client_limit_hourly_burn["ideal_primary_used_per_hour_percent"],
            2
        ),
        "latest_observed_at_epoch_ms": client_limit_hourly_burn["latest_observed_at_epoch_ms"].clone(),
        "latest_live_age_seconds":
            rounded_json_number(&client_limit_hourly_burn["latest_live_age_seconds"], 2)
    })
}

fn compact_client_limit_meter_alignment_for_budget_snapshot_preview(alignment: &Value) -> Value {
    if !alignment.is_object() {
        return Value::Null;
    }
    let baseline_component_semantics = alignment["baseline_equivalence"]["component_semantics"]
        .as_array()
        .map(|items| {
            Value::Array(
                items
                    .iter()
                    .map(|item| {
                        json!({
                            "code": item["code"].clone(),
                            "baseline_measured_tokens": item["baseline_measured_tokens"].clone(),
                            "observed_tokens": item["observed_tokens"].clone(),
                            "whole_cycle_observed_complete":
                                item["whole_cycle_observed_complete"].clone()
                        })
                    })
                    .collect(),
            )
        })
        .unwrap_or(Value::Null);
    json!({
        "alignment_state": alignment["alignment_state"].clone(),
        "same_meter_as_client_limit": alignment["same_meter_as_client_limit"].clone(),
        "exact_pair_status": alignment["exact_pair_status"].clone(),
        "strict_client_meter_slice": {
            "lower_bound_tokens": alignment["strict_client_meter_slice"]["lower_bound_tokens"].clone()
        },
        "baseline_equivalence": {
            "measured_baseline_tokens_lower_bound":
                alignment["baseline_equivalence"]["measured_baseline_tokens_lower_bound"].clone(),
            "component_semantics": baseline_component_semantics
        },
        "blocking_reasons": alignment["blocking_reasons"].clone(),
        "measured_components": alignment["measured_components"].clone(),
        "missing_components": alignment["missing_components"].clone(),
        "not_applicable_components": alignment["not_applicable_components"].clone()
    })
}

fn compact_budget_snapshot_preview_payload(snapshot: &Value) -> Value {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    json!({
        "latest_repo_working_state_restore": compact_latest_repo_working_state_restore_for_budget(
            &snapshot["latest_repo_working_state_restore"]
        ),
        "token_budget_report": {
            "token_budget_report": {
                "surface": report["surface"].clone(),
                "client_budget_target_percent": report["client_budget_target_percent"].clone(),
                "current_session":
                    compact_current_session_for_budget_snapshot_preview(&report["current_session"]),
                "statement_previews": {
                    "current_session": {
                        "observed_whole_cycle_with_amai_tokens":
                            report["statement_previews"]["current_session"]["observed_whole_cycle_with_amai_tokens"].clone(),
                        "verified_observed_whole_cycle_with_amai_tokens":
                            report["statement_previews"]["current_session"]["verified_observed_whole_cycle_with_amai_tokens"].clone(),
                        "with_amai_measured_tokens":
                            report["statement_previews"]["current_session"]["with_amai_measured_tokens"].clone(),
                        "verified_with_amai_measured_tokens":
                            report["statement_previews"]["current_session"]["verified_with_amai_measured_tokens"].clone(),
                        "client_limit_meter_alignment":
                            compact_client_limit_meter_alignment_for_budget_snapshot_preview(
                                &report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                            )
                    }
                },
                "client_limit_hourly_burn": compact_client_limit_hourly_burn_for_budget_snapshot_preview(
                    &report["client_limit_hourly_burn"]
                ),
                "client_live_meter":
                    compact_client_live_meter_for_budget_snapshot_preview(&report["client_live_meter"]),
                "current_live_turn":
                    compact_current_live_turn_for_budget_snapshot_preview(&report["current_live_turn"])
            }
        }
    })
}

fn compact_client_budget_surfaces_from_snapshot(
    repo_root: &Path,
    snapshot: &Value,
    thread_id: Option<&str>,
) -> MaterializedCompactClientBudgetSurfaces {
    let guard = dashboard::current_session_budget_guard(snapshot);
    let root_cause_payload =
        dashboard::client_budget_root_cause_payload_with_guard(snapshot, &guard);
    let compact_root_cause =
        compact_client_budget_root_cause_payload(&root_cause_payload, Some(&guard));
    let compact_gate =
        front_door_client_budget_gate_payload(compact_cli_client_budget_gate_payload(&guard));
    let compact_guard = compact_current_session_budget_guard_payload(&guard);
    let surfaces_cache = build_compact_client_budget_surfaces_cache(
        &compact_root_cause,
        &compact_gate,
        &compact_guard,
        thread_id,
    );
    let _ = write_shared_compact_client_budget_surfaces(repo_root, thread_id, &surfaces_cache);
    let gate_cache =
        build_compact_client_budget_gate_cache(&compact_gate, &compact_guard, thread_id);
    let _ = write_shared_compact_client_budget_gate(repo_root, thread_id, &gate_cache);
    MaterializedCompactClientBudgetSurfaces {
        surfaces: CompactClientBudgetSurfaces {
            root_cause_payload: compact_root_cause,
            guard_payload: compact_guard.clone(),
            guard: compact_guard.clone(),
        },
        gate: CompactClientBudgetGateSurface {
            gate_payload: compact_gate,
        },
    }
}

fn try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
    repo_root: &Path,
    thread_id: &str,
) -> Option<MaterializedCompactClientBudgetSurfaces> {
    let now_epoch_ms = current_epoch_ms_u64();
    if let Some(cached) =
        load_shared_compact_client_budget_surfaces(repo_root, now_epoch_ms, Some(thread_id))
    {
        return Some(MaterializedCompactClientBudgetSurfaces {
            surfaces: CompactClientBudgetSurfaces {
                root_cause_payload: cached.root_cause,
                guard_payload: cached.guard.clone(),
                guard: cached.guard.clone(),
            },
            gate: CompactClientBudgetGateSurface {
                gate_payload: cached.gate,
            },
        });
    }
    let snapshot = load_shared_budget_snapshot_preview(repo_root, Some(thread_id))?;
    Some(compact_client_budget_surfaces_from_snapshot(
        repo_root,
        &snapshot,
        Some(thread_id),
    ))
}

fn compact_current_session_budget_guard_payload(guard: &Value) -> Value {
    json!({
        "status_label": guard["status_label"].clone(),
        "full_turn_savings_proven": guard["full_turn_savings_proven"].clone(),
        "full_turn_savings_percent": guard["full_turn_savings_percent"].clone(),
        "should_rotate_chat_now": guard["should_rotate_chat_now"].clone(),
        "should_rotate_chat_soon": guard["should_rotate_chat_soon"].clone(),
        "requires_global_budget_recovery_before_reply":
            guard["requires_global_budget_recovery_before_reply"].clone(),
        "next_action": guard["next_action"].clone(),
        "last_request": guard["last_request"].clone(),
        "client_limits": guard["client_limits"].clone(),
        "tracked_slice": guard["tracked_slice"].clone(),
        "tracked_slice_truth": guard["tracked_slice_truth"].clone(),
        "client_live_meter_current_thread_bound":
            guard["client_live_meter_current_thread_bound"].clone(),
        "client_live_meter_thread_binding_state":
            guard["client_live_meter_thread_binding_state"].clone(),
        "observed_at_epoch_ms": guard["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": guard["max_guard_age_seconds"].clone(),
        "reply_execution_gate": compact_reply_execution_gate(&guard["reply_execution_gate"]),
    })
}

fn compact_client_budget_gate_payload(guard: &Value) -> Value {
    let reply_execution_gate = &guard["reply_execution_gate"];
    json!({
        "status_label": guard["status_label"].clone(),
        "reply_prefix": guard["reply_prefix"].clone(),
        "global_reply_prefix": guard["global_reply_prefix"].clone(),
        "reply_prefix_source": guard["reply_prefix_source"].clone(),
        "host_context_compaction": guard["host_context_compaction"].clone(),
        "observed_at_epoch_ms": guard["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": guard["max_guard_age_seconds"].clone(),
        "reply_execution_gate": compact_reply_execution_gate(reply_execution_gate),
    })
}

fn compact_host_context_compaction_for_cli(host_context_compaction: &Value) -> Value {
    json!({
        "stage": host_context_compaction["stage"].clone(),
        "current_thread_bound": host_context_compaction["current_thread_bound"].clone(),
        "current_turn_total_tokens": host_context_compaction["current_turn_total_tokens"].clone(),
        "growth_since_compaction_tokens":
            host_context_compaction["growth_since_compaction_tokens"].clone(),
        "regrowth_of_recovered_surface_ratio":
            host_context_compaction["regrowth_of_recovered_surface_ratio"].clone(),
        "critical_regrowth_active":
            host_context_compaction["critical_regrowth_active"].clone(),
        "preserve_active": host_context_compaction["preserve_active"].clone(),
    })
}

fn compact_host_context_compaction_for_root_cause_cli(host_context_compaction: &Value) -> Value {
    json!({
        "stage": host_context_compaction["stage"].clone(),
        "growth_since_compaction_tokens":
            host_context_compaction["growth_since_compaction_tokens"].clone(),
        "regrowth_of_recovered_surface_ratio":
            rounded_json_number(&host_context_compaction["regrowth_of_recovered_surface_ratio"], 2),
    })
}

fn compact_current_live_meter_for_root_cause_cli(current_live_meter: &Value) -> Value {
    json!({
        "client_turn_total_tokens": current_live_meter["client_turn_total_tokens"].clone(),
        "context_used_percent": rounded_json_number(&current_live_meter["context_used_percent"], 2),
    })
}

fn compact_current_live_turn_for_root_cause_cli(current_live_turn: &Value) -> Value {
    json!({
        "saved_pct": rounded_json_number(&current_live_turn["saved_pct"], 2),
        "status": current_live_turn["status"].clone(),
    })
}

fn compact_same_meter_economics_for_root_cause_cli(same_meter_economics: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    for field in [
        "strict_lower_bound_tokens",
        "same_meter_without_amai_tokens",
        "same_meter_with_amai_tokens",
        "same_meter_saved_tokens",
        "continuity_restore_baseline_tokens",
        "continuity_restore_observed_tokens",
        "continuity_restore_delta_tokens",
        "full_turn_overhang_tokens",
        "dominant_cost_surface",
    ] {
        if !same_meter_economics[field].is_null() {
            compact.insert(field.to_string(), same_meter_economics[field].clone());
        }
    }
    for (field, value) in [
        (
            "same_meter_saved_pct",
            rounded_json_number(&same_meter_economics["same_meter_saved_pct"], 2),
        ),
        (
            "full_turn_vs_strict_ratio",
            rounded_json_number(&same_meter_economics["full_turn_vs_strict_ratio"], 2),
        ),
    ] {
        if !value.is_null() {
            compact.insert(field.to_string(), value);
        }
    }
    Value::Object(compact)
}

fn compact_guard_for_root_cause_cli(guard: &Value) -> Value {
    json!({
        "should_rotate_chat_now": guard["should_rotate_chat_now"].clone(),
    })
}

fn rounded_json_number(value: &Value, decimals: u32) -> Value {
    let Some(number) = value.as_f64() else {
        return value.clone();
    };
    let scale = 10f64.powi(decimals as i32);
    serde_json::Number::from_f64((number * scale).round() / scale)
        .map(Value::Number)
        .unwrap_or_else(|| value.clone())
}

fn compact_host_current_thread_control_effect_for_cli(effect: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    for field in [
        "command_id",
        "surface_label",
        "thread_id",
        "current_thread_id",
        "current_stage",
        "recorded_at_epoch_ms",
        "elapsed_ms",
        "elapsed_label",
        "measurement_pending",
        "measurement_sufficient",
        "feedback_kind",
        "retry_allowed",
        "effect_verdict",
        "full_scale_client_burn_worsened",
        "rotate_fallback_recommended",
        "overlay_trial_recommended",
        "verified_host_compaction_observed_after_feedback",
        "compaction_count_delta",
        "primary_limit_used_percent_point_delta",
        "primary_limit_ideal_percent_point_delta",
        "primary_limit_used_overrun_percent_points",
        "turn_token_delta",
        "context_used_percent_point_delta",
        "regrowth_since_feedback_tokens",
    ] {
        if !effect[field].is_null() {
            compact.insert(field.to_string(), effect[field].clone());
        }
    }
    if effect["surface_exhausted_after_verified_failure"].as_bool() == Some(true) {
        compact.insert(
            "surface_exhausted_after_verified_failure".to_string(),
            json!(true),
        );
    }
    Value::Object(compact)
}

fn compact_client_budget_reply_gate_for_root_cause(guard: &Value) -> Value {
    let operator_flow = &guard["reply_execution_gate"]["action_bundle"]["operator_flow"];
    let mut compact_reply_execution_gate = serde_json::Map::from_iter([
        (
            "action_kind".to_string(),
            guard["reply_execution_gate"]["action_kind"].clone(),
        ),
        (
            "blocking".to_string(),
            guard["reply_execution_gate"]["blocking"].clone(),
        ),
        (
            "must_rotate_before_reply".to_string(),
            guard["reply_execution_gate"]["must_rotate_before_reply"].clone(),
        ),
        (
            "must_wait_for_budget_recovery_before_reply".to_string(),
            guard["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"].clone(),
        ),
        (
            "reply_budget_mode".to_string(),
            guard["reply_execution_gate"]["reply_budget_mode"].clone(),
        ),
        (
            "reply_prefix".to_string(),
            guard["reply_execution_gate"]["reply_prefix"].clone(),
        ),
        (
            "global_reply_prefix".to_string(),
            guard["global_reply_prefix"].clone(),
        ),
        (
            "reply_prefix_source".to_string(),
            guard["reply_prefix_source"].clone(),
        ),
    ]);
    compact_reply_execution_gate.extend(compact_reply_budget_pressure_hints(
        &guard["reply_execution_gate"],
    ));
    let mut compact_action_bundle = serde_json::Map::new();
    if operator_flow.is_object() {
        let mut compact_operator_flow = serde_json::Map::new();
        for field in [
            "primary_command_kind",
            "primary_command",
            "rotate_helper_command",
            "host_current_thread_control_launch_command",
        ] {
            if !operator_flow[field].is_null() {
                compact_operator_flow.insert(field.to_string(), operator_flow[field].clone());
            }
        }
        if !compact_operator_flow.is_empty() {
            compact_action_bundle.insert(
                "operator_flow".to_string(),
                Value::Object(compact_operator_flow),
            );
        }
    }
    if guard["reply_execution_gate"]["action_bundle"]["host_current_thread_control"].is_object() {
        compact_action_bundle.insert(
            "host_current_thread_control".to_string(),
            working_state::compact_host_current_thread_control_surface_for_runtime(
                &guard["reply_execution_gate"]["action_bundle"]["host_current_thread_control"],
            ),
        );
    }
    if !compact_action_bundle.is_empty() {
        compact_reply_execution_gate.insert(
            "action_bundle".to_string(),
            Value::Object(compact_action_bundle),
        );
    }
    json!({
        "observed_at_epoch_ms": guard["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": guard["max_guard_age_seconds"].clone(),
        "global_reply_prefix": guard["global_reply_prefix"].clone(),
        "reply_prefix_source": guard["reply_prefix_source"].clone(),
        "reply_execution_gate": Value::Object(compact_reply_execution_gate),
    })
}

fn compact_reply_budget_pressure_hints(
    reply_execution_gate: &Value,
) -> serde_json::Map<String, Value> {
    let contract = &reply_execution_gate["reply_budget_contract"];
    let mut compact = serde_json::Map::new();
    for field in [
        "must_confirm_same_thread_host_control_feedback_before_reply",
        "must_wait_for_same_thread_effect_measurement_before_reply",
        "host_context_compaction_inactive_target_pressure_active",
        "current_live_turn_no_amai_activity",
        "same_meter_pure_burn_turn_active",
        "must_prefer_short_paragraphs",
        "must_avoid_commentary_only_updates",
        "must_batch_all_tool_reads_before_reply",
        "must_wait_for_meaningful_result_before_progress_reply",
        "must_require_material_delta_before_next_reply",
        "must_avoid_progress_reply_when_only_guard_changed",
        "must_avoid_new_tool_turn_without_specific_delta_goal",
        "max_bullets_soft",
        "max_sentences_soft",
        "max_tool_roundtrips_soft",
    ] {
        let value = if !reply_execution_gate[field].is_null() {
            reply_execution_gate[field].clone()
        } else {
            contract[field].clone()
        };
        if !value.is_null() {
            compact.insert(field.to_string(), value);
        }
    }
    compact
}

fn compact_client_budget_root_cause_payload(payload: &Value, guard: Option<&Value>) -> Value {
    let mut compact = serde_json::Map::new();
    if !payload["thread_binding_state"].is_null() {
        compact.insert(
            "thread_binding_state".to_string(),
            payload["thread_binding_state"].clone(),
        );
    }
    if payload["current_live_meter"].is_object() {
        compact.insert(
            "current_live_meter".to_string(),
            compact_current_live_meter_for_root_cause_cli(&payload["current_live_meter"]),
        );
    }
    if payload["current_live_turn"].is_object() {
        compact.insert(
            "current_live_turn".to_string(),
            compact_current_live_turn_for_root_cause_cli(&payload["current_live_turn"]),
        );
    }
    if payload["same_meter_economics"].is_object() {
        compact.insert(
            "same_meter_economics".to_string(),
            compact_same_meter_economics_for_root_cause_cli(&payload["same_meter_economics"]),
        );
    }
    let exact_pair_state = payload["exact_pair_status"]["state"].as_str();
    let current_live_turn_status = payload["current_live_turn"]["status"].as_str();
    let exact_pair_status_redundant_for_live_turn = matches!(
        (exact_pair_state, current_live_turn_status),
        (
            Some("not_applicable_current_live_turn_has_no_amai_activity"),
            Some("no_amai_activity_in_current_live_turn")
        )
    );
    if !payload["exact_pair_status"].is_null() && !exact_pair_status_redundant_for_live_turn {
        compact.insert(
            "exact_pair_status".to_string(),
            payload["exact_pair_status"].clone(),
        );
    }
    if payload["host_context_compaction"].is_object() {
        compact.insert(
            "host_context_compaction".to_string(),
            compact_host_context_compaction_for_root_cause_cli(&payload["host_context_compaction"]),
        );
    }
    if let Some(exact_pair_status) = compact.get_mut("exact_pair_status")
        && exact_pair_status.is_object()
    {
        *exact_pair_status = json!({
            "state": exact_pair_status["state"].clone()
        });
    }
    if payload["host_current_thread_control_effect"].is_object() {
        compact.insert(
            "host_current_thread_control_effect".to_string(),
            compact_host_current_thread_control_effect_for_cli(
                &payload["host_current_thread_control_effect"],
            ),
        );
    }
    if payload["guard"].is_object() {
        compact.insert(
            "guard".to_string(),
            compact_guard_for_root_cause_cli(&payload["guard"]),
        );
    }
    if let Some(guard) = guard {
        compact.insert(
            "client_budget_reply_gate".to_string(),
            compact_client_budget_reply_gate_for_root_cause(guard),
        );
    }
    for field in [
        "missing_components",
        "partially_measured_components",
        "blocking_reasons",
    ] {
        if payload[field]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            compact.insert(field.to_string(), payload[field].clone());
        }
    }
    Value::Object(compact)
}

fn compact_cli_client_budget_gate_payload(guard: &Value) -> Value {
    let compact_gate = compact_client_budget_gate_payload(guard);
    let mut compact_action_bundle = serde_json::Map::new();
    for field in [
        "measurement_before_retry_required",
        "feedback_confirmation_before_retry_required",
    ] {
        if !compact_gate["reply_execution_gate"]["action_bundle"][field].is_null() {
            compact_action_bundle.insert(
                field.to_string(),
                compact_gate["reply_execution_gate"]["action_bundle"][field].clone(),
            );
        }
    }
    if compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"].is_object() {
        let mut operator_flow =
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]
                .as_object()
                .cloned()
                .unwrap_or_default();
        operator_flow.remove("startup_command");
        if !operator_flow.is_empty() {
            compact_action_bundle.insert("operator_flow".to_string(), Value::Object(operator_flow));
        }
    }
    if compact_gate["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
        .is_object()
    {
        compact_action_bundle.insert(
            "host_current_thread_control".to_string(),
            working_state::compact_host_current_thread_control_surface_for_runtime(
                &compact_gate["reply_execution_gate"]["action_bundle"]
                    ["host_current_thread_control"],
            ),
        );
    }
    let mut compact_reply_execution_gate = serde_json::Map::from_iter([
        (
            "action_kind".to_string(),
            compact_gate["reply_execution_gate"]["action_kind"].clone(),
        ),
        (
            "blocking".to_string(),
            compact_gate["reply_execution_gate"]["blocking"].clone(),
        ),
        (
            "must_rotate_before_reply".to_string(),
            compact_gate["reply_execution_gate"]["must_rotate_before_reply"].clone(),
        ),
        (
            "must_wait_for_budget_recovery_before_reply".to_string(),
            compact_gate["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"]
                .clone(),
        ),
        (
            "reply_budget_mode".to_string(),
            compact_gate["reply_execution_gate"]["reply_budget_mode"].clone(),
        ),
        (
            "reply_prefix".to_string(),
            compact_gate["reply_execution_gate"]["reply_prefix"].clone(),
        ),
        (
            "global_reply_prefix".to_string(),
            compact_gate["reply_execution_gate"]["global_reply_prefix"].clone(),
        ),
        (
            "reply_prefix_source".to_string(),
            compact_gate["reply_execution_gate"]["reply_prefix_source"].clone(),
        ),
        (
            "host_context_compaction_stage".to_string(),
            compact_gate["reply_execution_gate"]["host_context_compaction_stage"].clone(),
        ),
        (
            "host_context_compaction_preserve_active".to_string(),
            compact_gate["reply_execution_gate"]["host_context_compaction_preserve_active"]
                .clone(),
        ),
        (
            "host_context_compaction_critical_regrowth_active".to_string(),
            compact_gate["reply_execution_gate"]
                ["host_context_compaction_critical_regrowth_active"]
                .clone(),
        ),
        (
            "preserves_return_obligation".to_string(),
            compact_gate["reply_execution_gate"]["preserves_return_obligation"].clone(),
        ),
        ("action_bundle".to_string(), Value::Object(compact_action_bundle)),
    ]);
    for field in [
        "host_context_compaction_inactive_target_pressure_active",
        "current_live_turn_no_amai_activity",
        "same_meter_pure_burn_turn_active",
        "must_prefer_short_paragraphs",
        "must_avoid_commentary_only_updates",
        "must_batch_all_tool_reads_before_reply",
        "must_wait_for_meaningful_result_before_progress_reply",
        "must_require_material_delta_before_next_reply",
        "must_avoid_progress_reply_when_only_guard_changed",
        "must_avoid_new_tool_turn_without_specific_delta_goal",
        "max_bullets_soft",
        "max_sentences_soft",
        "max_tool_roundtrips_soft",
    ] {
        if !compact_gate["reply_execution_gate"][field].is_null() {
            compact_reply_execution_gate.insert(
                field.to_string(),
                compact_gate["reply_execution_gate"][field].clone(),
            );
        }
    }
    json!({
        "status_label": compact_gate["status_label"].clone(),
        "reply_prefix": compact_gate["reply_prefix"].clone(),
        "global_reply_prefix": compact_gate["global_reply_prefix"].clone(),
        "reply_prefix_source": compact_gate["reply_prefix_source"].clone(),
        "observed_at_epoch_ms": compact_gate["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": compact_gate["max_guard_age_seconds"].clone(),
        "reply_execution_gate": Value::Object(compact_reply_execution_gate),
    })
}

fn front_door_client_budget_gate_payload(gate: Value) -> Value {
    json!({
        "reply_prefix": gate["reply_execution_gate"]["reply_prefix"].clone(),
        "global_reply_prefix": gate["reply_execution_gate"]["global_reply_prefix"].clone(),
        "reply_prefix_source": gate["reply_execution_gate"]["reply_prefix_source"].clone(),
        "status_label": gate["status_label"].clone(),
        "observed_at_epoch_ms": gate["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": gate["max_guard_age_seconds"].clone(),
        "client_budget_reply_gate": gate,
    })
}

fn normalize_front_door_client_budget_gate_payload_shape(payload: Value) -> Value {
    if payload["reply_prefix"].is_null() && payload["client_budget_reply_gate"].is_object() {
        return front_door_client_budget_gate_payload(payload["client_budget_reply_gate"].clone());
    }
    payload
}

fn same_thread_host_control_auto_launch_args_from_gate(
    repo_root: &Path,
    thread_id: &str,
    payload: &Value,
) -> Option<ObserveClientBudgetHostControlLaunchArgs> {
    let reply_execution_gate = &payload["client_budget_reply_gate"]["reply_execution_gate"];
    let host_current_thread_control =
        &reply_execution_gate["action_bundle"]["host_current_thread_control"];
    if reply_execution_gate["action_kind"].as_str()
        != Some("compact_current_thread_for_client_budget")
        || reply_execution_gate["same_meter_pure_burn_turn_active"].as_bool() != Some(true)
        || reply_execution_gate["must_avoid_new_tool_turn_without_specific_delta_goal"].as_bool()
            != Some(true)
        || reply_execution_gate["max_tool_roundtrips_soft"].as_i64() != Some(0)
        || host_current_thread_control["automation_ready"].as_bool() != Some(true)
        || host_current_thread_control["retry_allowed"].as_bool() != Some(true)
        || host_current_thread_control["measurement_pending"].as_bool() == Some(true)
        || host_current_thread_control["feedback_pending"].as_bool() == Some(true)
    {
        return None;
    }
    let command_id = host_current_thread_control["command_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let surface_thread_id = host_current_thread_control["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(thread_id);
    if surface_thread_id != thread_id {
        return None;
    }
    Some(ObserveClientBudgetHostControlLaunchArgs {
        thread_id: thread_id.to_string(),
        compact_window: command_id == working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID,
        command_id: if command_id == working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID {
            None
        } else {
            Some(command_id.to_string())
        },
        project: None,
        repo_root: Some(repo_root.to_path_buf()),
        namespace: default_continuity_namespace(),
    })
}

async fn maybe_auto_launch_same_thread_host_control_from_gate(
    cfg: &AppConfig,
    repo_root: &Path,
    thread_id: &str,
    payload: &Value,
) -> Result<Option<Value>> {
    let Some(args) =
        same_thread_host_control_auto_launch_args_from_gate(repo_root, thread_id, payload)
    else {
        return Ok(None);
    };
    let launch_payload = client_budget_host_control_launch_payload(cfg, &args).await?;
    Ok(Some(front_door_client_budget_gate_payload(
        launch_payload["client_budget_host_control_launch"]["client_budget_reply_gate"].clone(),
    )))
}

fn compact_cli_client_budget_gate_from_root_cause_payload(payload: &Value) -> Option<Value> {
    let gate = payload.get("client_budget_reply_gate")?.clone();
    if gate["reply_execution_gate"]["action_kind"].is_null() {
        return None;
    }
    Some(front_door_client_budget_gate_payload(gate))
}

fn compact_host_control_client_budget_reply_gate(guard: &Value) -> Value {
    let compact_gate = compact_client_budget_gate_payload(guard);
    let host_context_compaction =
        compact_host_context_compaction_for_cli(&compact_gate["host_context_compaction"]);
    let compact_operator_flow = json!({
        "primary_command_kind":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"].clone(),
        "same_thread_effect_measurement_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_effect_measurement_required"].clone(),
        "same_thread_effect_measurement_summary":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_effect_measurement_summary"].clone(),
        "same_thread_feedback_confirmation_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"].clone(),
        "same_thread_feedback_confirmation_summary":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_summary"].clone(),
    });
    let compact_action_bundle = json!({
        "bundle_version":
            compact_gate["reply_execution_gate"]["action_bundle"]["bundle_version"].clone(),
        "ready_for_automation":
            compact_gate["reply_execution_gate"]["action_bundle"]["ready_for_automation"].clone(),
        "preserves_return_obligation":
            compact_gate["reply_execution_gate"]["action_bundle"]["preserves_return_obligation"].clone(),
        "measurement_before_retry_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["measurement_before_retry_required"].clone(),
        "feedback_confirmation_before_retry_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["feedback_confirmation_before_retry_required"].clone(),
        "order": compact_gate["reply_execution_gate"]["action_bundle"]["order"].clone(),
        "host_current_thread_control": working_state::compact_host_current_thread_control_surface_for_runtime(
            &compact_gate["reply_execution_gate"]["action_bundle"]["host_current_thread_control"],
        ),
        "operator_flow": compact_operator_flow,
    });
    json!({
        "status_label": compact_gate["status_label"].clone(),
        "reply_prefix": compact_gate["reply_prefix"].clone(),
        "global_reply_prefix": compact_gate["global_reply_prefix"].clone(),
        "reply_prefix_source": compact_gate["reply_prefix_source"].clone(),
        "host_context_compaction": host_context_compaction,
        "reply_execution_gate": {
            "action_kind": compact_gate["reply_execution_gate"]["action_kind"].clone(),
            "blocking": compact_gate["reply_execution_gate"]["blocking"].clone(),
            "must_rotate_before_reply":
                compact_gate["reply_execution_gate"]["must_rotate_before_reply"].clone(),
            "must_wait_for_budget_recovery_before_reply":
                compact_gate["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"].clone(),
            "reply_budget_mode": compact_gate["reply_execution_gate"]["reply_budget_mode"].clone(),
            "reply_prefix": compact_gate["reply_execution_gate"]["reply_prefix"].clone(),
            "global_reply_prefix":
                compact_gate["reply_execution_gate"]["global_reply_prefix"].clone(),
            "reply_prefix_source":
                compact_gate["reply_execution_gate"]["reply_prefix_source"].clone(),
            "host_context_compaction_stage":
                compact_gate["reply_execution_gate"]["host_context_compaction_stage"].clone(),
            "host_context_compaction_preserve_active":
                compact_gate["reply_execution_gate"]["host_context_compaction_preserve_active"].clone(),
            "host_context_compaction_critical_regrowth_active":
                compact_gate["reply_execution_gate"]["host_context_compaction_critical_regrowth_active"].clone(),
            "preserves_return_obligation":
                compact_gate["reply_execution_gate"]["preserves_return_obligation"].clone(),
            "action_bundle": compact_action_bundle,
        },
    })
}

fn compact_reply_execution_gate(reply_execution_gate: &Value) -> Value {
    let preserves_return_obligation = reply_execution_gate["preserves_return_obligation"]
        .as_bool()
        .map(Value::from)
        .unwrap_or_else(|| {
            reply_execution_gate["action_bundle"]["preserves_return_obligation"].clone()
        });
    let action_bundle =
        compact_reply_execution_action_bundle(&reply_execution_gate["action_bundle"]);
    let mut compact = serde_json::Map::from_iter([
        (
            "action_kind".to_string(),
            reply_execution_gate["action_kind"].clone(),
        ),
        (
            "blocking".to_string(),
            reply_execution_gate["blocking"].clone(),
        ),
        (
            "must_rotate_before_reply".to_string(),
            reply_execution_gate["must_rotate_before_reply"].clone(),
        ),
        (
            "must_wait_for_budget_recovery_before_reply".to_string(),
            reply_execution_gate["must_wait_for_budget_recovery_before_reply"].clone(),
        ),
        (
            "reply_budget_mode".to_string(),
            reply_execution_gate["reply_budget_mode"].clone(),
        ),
        (
            "reply_prefix".to_string(),
            reply_execution_gate["reply_prefix"].clone(),
        ),
        (
            "global_reply_prefix".to_string(),
            reply_execution_gate["global_reply_prefix"].clone(),
        ),
        (
            "reply_prefix_source".to_string(),
            reply_execution_gate["reply_prefix_source"].clone(),
        ),
        (
            "host_context_compaction_stage".to_string(),
            reply_execution_gate["host_context_compaction_stage"].clone(),
        ),
        (
            "host_context_compaction_preserve_active".to_string(),
            reply_execution_gate["host_context_compaction_preserve_active"].clone(),
        ),
        (
            "host_context_compaction_critical_regrowth_active".to_string(),
            reply_execution_gate["host_context_compaction_critical_regrowth_active"].clone(),
        ),
        (
            "preserves_return_obligation".to_string(),
            preserves_return_obligation,
        ),
        ("action_bundle".to_string(), action_bundle),
    ]);
    compact.extend(compact_reply_budget_pressure_hints(reply_execution_gate));
    Value::Object(compact)
}

fn normalize_compact_cli_command(command: &str) -> String {
    command
        .replace('\'', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_reply_execution_action_bundle(action_bundle: &Value) -> Value {
    let Some(bundle) = action_bundle.as_object() else {
        return Value::Null;
    };
    let mut compact = serde_json::Map::new();
    for field in [
        "bundle_version",
        "ready_for_automation",
        "preserves_return_obligation",
        "measurement_before_retry_required",
        "feedback_confirmation_before_retry_required",
        "order",
    ] {
        if !bundle.get(field).unwrap_or(&Value::Null).is_null() {
            compact.insert(field.to_string(), action_bundle[field].clone());
        }
    }
    if action_bundle["host_current_thread_control"].is_object() {
        compact.insert(
            "host_current_thread_control".to_string(),
            action_bundle["host_current_thread_control"].clone(),
        );
    }
    if action_bundle["operator_flow"].is_object() {
        let mut operator_flow = serde_json::Map::new();
        for field in [
            "primary_command_kind",
            "same_thread_effect_measurement_required",
            "same_thread_effect_measurement_summary",
            "same_thread_feedback_confirmation_required",
            "same_thread_feedback_confirmation_summary",
        ] {
            if !action_bundle["operator_flow"][field].is_null() {
                operator_flow.insert(
                    field.to_string(),
                    action_bundle["operator_flow"][field].clone(),
                );
            }
        }
        for field in [
            "primary_command",
            "host_current_thread_control_launch_command",
            "rotate_helper_command",
            "startup_command",
            "startup_after_recovery_command",
        ] {
            if let Some(command) = action_bundle["operator_flow"][field]
                .as_str()
                .map(normalize_compact_cli_command)
                .filter(|value| !value.is_empty())
            {
                operator_flow.insert(field.to_string(), Value::from(command));
            }
        }
        if operator_flow
            .get("primary_command_kind")
            .and_then(Value::as_str)
            == Some("rotate_helper_command")
            && operator_flow.get("primary_command") == operator_flow.get("rotate_helper_command")
        {
            operator_flow.remove("primary_command");
        }
        if operator_flow
            .get("primary_command_kind")
            .and_then(Value::as_str)
            == Some("same_thread_host_control_launch_command")
            && operator_flow.get("primary_command")
                == operator_flow.get("host_current_thread_control_launch_command")
        {
            operator_flow.remove("host_current_thread_control_launch_command");
        }
        if !operator_flow.is_empty() {
            compact.insert("operator_flow".to_string(), Value::Object(operator_flow));
        }
    }
    if compact.contains_key("operator_flow") {
        Value::Object(compact)
    } else {
        Value::Null
    }
}

pub async fn print_retention_cleanup(
    cfg: &AppConfig,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let summary = run_retention_cleanup(cfg, apply, limit).await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub async fn print_artifact_cleanup(
    _cfg: &AppConfig,
    apply: bool,
    limit: Option<usize>,
    aggressive: bool,
    target: Option<&str>,
) -> Result<()> {
    let repo_root = discover_repo_root(None)?;
    let summary =
        collect_artifact_cleanup_summary(&repo_root, apply, false, limit, aggressive, target)?;
    let _ = artifact_cleanup::write_latest_summary(&repo_root, &summary)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub async fn serve_metrics(cfg: &AppConfig, bind: &str) -> Result<()> {
    let profile = load_profile()?;
    maybe_cleanup_observability_snapshots(cfg).await?;
    maybe_cleanup_local_artifacts().await?;
    let bootstrap_db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&bootstrap_db, cfg).await?;
    let cache = Arc::new(RwLock::new(ObserveCache::default()));
    let cleanup_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SNAPSHOT_RETENTION_SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = maybe_cleanup_observability_snapshots(&cleanup_cfg).await {
                eprintln!("observability retention cleanup failed: {error:#}");
            }
        }
    });
    let artifact_cleanup_interval = artifact_cleanup::sweep_interval()?;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(artifact_cleanup_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = maybe_cleanup_local_artifacts().await {
                eprintln!("artifact cleanup failed: {error:#}");
            }
        }
    });
    let trend_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLIENT_LIMIT_TREND_ANALYSIS_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = persist_periodic_client_limit_trend_analysis(&trend_cfg).await {
                eprintln!("client limit trend analysis refresh failed: {error:#}");
            }
        }
    });
    let quarantine_resolution_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(IMPORT_PACKET_QUARANTINE_RESOLUTION_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) =
                persist_periodic_import_packet_quarantine_resolution(&quarantine_resolution_cfg)
                    .await
            {
                eprintln!("import packet quarantine resolution failed: {error:#}");
            }
        }
    });
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid observe bind address: {bind}"))?;
    let app = Router::new()
        .route("/", get(dashboard_page_handler))
        .route("/dashboard", get(dashboard_page_handler))
        .route("/help/grafana-password", get(grafana_password_help_handler))
        .route("/brand/amai_mark.svg", get(brand_mark_handler))
        .route("/brand/amai_lockup.svg", get(brand_lockup_handler))
        .route("/favicon.ico", get(favicon_handler))
        .route("/api/dashboard", get(dashboard_api_handler))
        .route(
            "/api/dashboard-live-summary",
            get(dashboard_live_summary_api_handler),
        )
        .route(
            "/api/client-budget-live",
            get(client_budget_live_api_handler),
        )
        .route(
            "/api/active-agent-budget-live",
            get(active_agent_budget_live_api_handler),
        )
        .route(
            "/api/client-budget-snapshot-preview",
            get(client_budget_snapshot_preview_api_handler),
        )
        .route(
            "/api/client-budget-root-cause",
            get(client_budget_root_cause_api_handler),
        )
        .route(
            "/api/client-budget-gate",
            get(client_budget_gate_api_handler),
        )
        .route(
            "/api/client-limit-hourly-burn",
            get(client_limit_hourly_burn_api_handler),
        )
        .route(
            "/api/client-budget-target",
            post(client_budget_target_update_api_handler),
        )
        .route(
            "/api/client-budget-compact-chat",
            post(client_budget_compact_chat_api_handler),
        )
        .route(
            "/api/continuity-handoff",
            post(continuity_handoff_api_handler),
        )
        .route(
            "/api/client-budget-host-control-launch",
            post(client_budget_host_control_launch_api_handler),
        )
        .route(
            "/api/client-budget-host-control-feedback",
            post(client_budget_host_control_feedback_api_handler),
        )
        .route(
            "/api/agent-display-name",
            post(agent_display_name_update_api_handler),
        )
        .route("/api/snapshot", get(snapshot_api_handler))
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(healthz_handler))
        .with_state(ObserveState {
            dashboard_refresh_ms: profile.dashboard.refresh_ms,
            cfg: cfg.clone(),
            bind: bind.to_string(),
            cache: cache.clone(),
        });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind observe exporter on {bind}"))?;
    let base_url = human_dashboard_base_url(bind);
    println!("Amai human dashboard: {base_url}/");
    println!("Amai dashboard JSON: {base_url}/api/dashboard");
    println!("Amai live client budget JSON: {base_url}/api/client-budget-live");
    println!("Amai raw snapshot JSON: {base_url}/api/snapshot");
    println!("Amai health JSON: {base_url}/healthz");
    println!("Amai Prometheus metrics: {base_url}/metrics");
    axum::serve(listener, app)
        .await
        .context("observe exporter stopped unexpectedly")
}

async fn persist_periodic_client_limit_trend_analysis(cfg: &AppConfig) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let _ = token_budget::collect_exact_client_limit_trend_analysis(
        &db,
        300,
        10,
        token_budget::DEFAULT_CLIENT_LIMIT_TREND_ANALYSIS_LOOKBACK_MINUTES,
        true,
    )
    .await?;
    Ok(())
}

async fn persist_periodic_import_packet_quarantine_resolution(cfg: &AppConfig) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let summary = postgres::reconcile_import_packet_quarantines(&db, true, Some(64)).await?;
    if summary.released > 0 || summary.rejected > 0 {
        eprintln!(
            "Amai import packet quarantine resolver: released={}, rejected={}, held={}, scanned={}",
            summary.released, summary.rejected, summary.held, summary.scanned
        );
    }
    Ok(())
}

const PROCEDURAL_BENCHMARK_HISTORY_LIMIT: i64 = 12;
const IMPORT_PACKET_QUARANTINE_RESOLUTION_INTERVAL: Duration = Duration::from_secs(60);

fn history_snapshot_captured_at_epoch_ms(
    snapshot: &postgres::ObservabilitySnapshotRecord,
    payload_root: &str,
) -> u64 {
    snapshot.payload[payload_root]["captured_at_epoch_ms"]
        .as_u64()
        .or_else(|| snapshot.payload["_observability"]["captured_at_epoch_ms"].as_u64())
        .unwrap_or(snapshot.created_at_epoch_ms.max(0) as u64)
}

async fn procedural_benchmark_history_surface(db: &Client) -> Result<Value> {
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        "procedural_benchmark",
        "amai",
        "benchmark",
        Some(PROCEDURAL_BENCHMARK_HISTORY_LIMIT),
    )
    .await?;
    let history_rows: Vec<Value> = snapshots
        .into_iter()
        .rev()
        .map(|snapshot| {
            let payload = &snapshot.payload["procedural_benchmark"];
            let with_summary = &payload["benchmark_line_summaries"]["with_amai"];
            let without_summary = &payload["benchmark_line_summaries"]["without_amai_but_measuring"];
            json!({
                "benchmark_run_id": payload["benchmark_run_id"].clone(),
                "captured_at_epoch_ms": history_snapshot_captured_at_epoch_ms(&snapshot, "procedural_benchmark"),
                "benchmark_run_state": payload["benchmark_run_state"].clone(),
                "benchmark_run_state_ru": payload["benchmark_run_state_ru"].clone(),
                "with_amai_pass_percent": with_summary["pass_percent"].clone(),
                "without_amai_pass_percent": without_summary["pass_percent"].clone(),
                "with_amai_point_count": with_summary["point_count"].clone(),
                "without_amai_point_count": without_summary["point_count"].clone(),
                "without_amai_series_available": payload["summary"]["without_amai_series_available"].clone()
            })
        })
        .collect();
    let with_amai_pass_percent_series: Vec<Value> = history_rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "benchmark_run_id": row["benchmark_run_id"].clone(),
                "captured_at_epoch_ms": row["captured_at_epoch_ms"].clone(),
                "pass_percent": row["with_amai_pass_percent"].as_f64()?
            }))
        })
        .collect();
    let without_amai_pass_percent_series: Vec<Value> = history_rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "benchmark_run_id": row["benchmark_run_id"].clone(),
                "captured_at_epoch_ms": row["captured_at_epoch_ms"].clone(),
                "pass_percent": row["without_amai_pass_percent"].as_f64()?
            }))
        })
        .collect();
    Ok(json!({
        "snapshot_kind": "procedural_benchmark",
        "scope_project_code": "amai",
        "scope_namespace_code": "benchmark",
        "history_limit": PROCEDURAL_BENCHMARK_HISTORY_LIMIT,
        "history_count": history_rows.len(),
        "with_amai_history_count": with_amai_pass_percent_series.len(),
        "without_amai_history_count": without_amai_pass_percent_series.len(),
        "history_rows": history_rows,
        "with_amai_pass_percent_series": with_amai_pass_percent_series,
        "without_amai_pass_percent_series": without_amai_pass_percent_series
    }))
}

#[derive(Debug, Serialize)]
struct GuardrailCheck {
    name: &'static str,
    status: &'static str,
    details: Value,
}

async fn collect_guardrail_report(db: &Client, prefix: &str) -> Result<Value> {
    let mut checks = Vec::new();
    checks.push(prove_direct_sql_working_state_event_id(db, prefix).await?);
    checks.push(prove_direct_sql_benchmark_contamination_block(db, prefix).await?);
    checks.push(prove_idempotent_replay_counter(db, prefix).await?);
    checks.push(prove_newer_divergent_payload_is_anti_replay(db, prefix).await?);
    checks.push(prove_immutable_snapshot_update_is_blocked(db, prefix).await?);
    Ok(json!({
        "status": "pass",
        "guardrails": checks,
    }))
}

async fn cleanup_guardrail_rows(db: &Client, prefix: &str) -> Result<()> {
    let like = format!("{prefix}%");
    db.execute(
        r#"
        DELETE FROM ami.observability_snapshots
        WHERE event_key LIKE $1
           OR COALESCE(source_event_id, '') LIKE $1
        "#,
        &[&like],
    )
    .await
    .context("failed to cleanup observability guardrail proof rows")?;
    Ok(())
}

async fn prove_direct_sql_working_state_event_id(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-working-state");
    let payload = json!({
        "working_state_event": {
            "event_id": event_id,
            "context_pack_id": format!("{prefix}-legacy-context-pack"),
            "source_kind": "context_pack",
            "project": {
                "code": "amai"
            },
            "namespace": {
                "code": "default"
            },
            "recorded_at_epoch_ms": 101
        }
    });
    let row = db
        .query_one(
            r#"
            INSERT INTO ami.observability_snapshots(snapshot_kind, payload)
            VALUES ($1, $2)
            RETURNING snapshot_id, event_key, source_event_id
            "#,
            &[&"working_state_event", &payload],
        )
        .await
        .context("failed to insert direct-SQL working_state proof row")?;
    let snapshot_id: Uuid = row.get(0);
    let event_key: String = row.get(1);
    let source_event_id: Option<String> = row.get(2);
    if event_key != event_id || source_event_id.as_deref() != Some(event_id.as_str()) {
        return Err(anyhow!(
            "working_state direct SQL proof expected event_id={} but stored event_key={} source_event_id={:?}",
            event_id,
            event_key,
            source_event_id
        ));
    }
    Ok(GuardrailCheck {
        name: "direct_sql_working_state_event_id",
        status: "pass",
        details: json!({
            "snapshot_id": snapshot_id,
            "event_key": event_key,
            "source_event_id": source_event_id,
        }),
    })
}

async fn prove_direct_sql_benchmark_contamination_block(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-contamination");
    let payload = json!({
        "_observability": {
            "source_event_id": event_id
        },
        "load_verification": {
            "project": "amai",
            "namespace": "default",
            "captured_at_epoch_ms": 202,
            "record_live_context": true,
            "publish_benchmark_snapshot": false
        }
    });
    let error = db
        .execute(
            r#"
            INSERT INTO ami.observability_snapshots(snapshot_kind, payload)
            VALUES ($1, $2)
            "#,
            &[&"retrieval_load_hot", &payload],
        )
        .await
        .expect_err("contaminated benchmark insert must fail");
    let message = postgres_error_message(&error);
    if !message.contains("benchmark lane contamination blocked") {
        return Err(anyhow!(
            "unexpected benchmark contamination error: {message}"
        ));
    }
    Ok(GuardrailCheck {
        name: "direct_sql_benchmark_contamination_block",
        status: "pass",
        details: json!({
            "error": message,
        }),
    })
}

fn postgres_error_message(error: &tokio_postgres::Error) -> String {
    if let Some(db_error) = error.as_db_error() {
        let mut message = db_error.message().to_string();
        if let Some(detail) = db_error.detail() {
            message.push_str(&format!(" | detail: {detail}"));
        }
        if let Some(hint) = db_error.hint() {
            message.push_str(&format!(" | hint: {hint}"));
        }
        return message;
    }
    error.to_string()
}

async fn prove_idempotent_replay_counter(db: &Client, prefix: &str) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-replay");
    let payload = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 303,
            "p95_ms": 0.5
        }
    });
    let first_snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &payload).await?;
    let replay_snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &payload).await?;
    let row = db
        .query_one(
            r#"
            SELECT replay_count
            FROM ami.observability_snapshots
            WHERE snapshot_id = $1
            "#,
            &[&first_snapshot_id],
        )
        .await
        .context("failed to fetch replay_count for observability proof row")?;
    let replay_count: i64 = row.get(0);
    if replay_snapshot_id != first_snapshot_id || replay_count != 1 {
        return Err(anyhow!(
            "idempotent replay proof expected same snapshot_id with replay_count=1, got first={} replay={} replay_count={}",
            first_snapshot_id,
            replay_snapshot_id,
            replay_count
        ));
    }
    Ok(GuardrailCheck {
        name: "idempotent_replay_counter",
        status: "pass",
        details: json!({
            "snapshot_id": first_snapshot_id,
            "replay_count": replay_count,
        }),
    })
}

async fn prove_newer_divergent_payload_is_anti_replay(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-anti-replay");
    let older = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 404,
            "p95_ms": 0.4
        }
    });
    let newer = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 405,
            "p95_ms": 0.9
        }
    });
    let snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &older).await?;
    let error = postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &newer)
        .await
        .expect_err("newer divergent payload must trigger anti-replay");
    let message = format!("{error:#}");
    if !message.contains("observability anti-replay blocked newer divergent payload") {
        return Err(anyhow!("unexpected anti-replay error: {message}"));
    }
    Ok(GuardrailCheck {
        name: "newer_divergent_payload_is_anti_replay",
        status: "pass",
        details: json!({
            "snapshot_id": snapshot_id,
            "error": message,
        }),
    })
}

async fn prove_immutable_snapshot_update_is_blocked(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-immutable");
    let original = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 505,
            "p95_ms": 0.6
        }
    });
    let mut updated = original.clone();
    updated["benchmark"]["p95_ms"] = json!(1.2);
    let snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &original).await?;
    let error = postgres::update_observability_snapshot_payload(db, &snapshot_id, &updated)
        .await
        .expect_err("immutable benchmark snapshot update must fail");
    let message = format!("{error:#}");
    if !message.contains("observability snapshot is immutable and cannot be updated") {
        return Err(anyhow!("unexpected immutable update error: {message}"));
    }
    Ok(GuardrailCheck {
        name: "immutable_snapshot_update_is_blocked",
        status: "pass",
        details: json!({
            "snapshot_id": snapshot_id,
            "error": message,
        }),
    })
}

pub fn human_dashboard_base_url(bind: &str) -> String {
    dashboard::browser_base_url(bind)
}

pub async fn collect_snapshot(cfg: &AppConfig) -> Result<Value> {
    build_snapshot(cfg, true).await
}

pub async fn collect_snapshot_preview(cfg: &AppConfig) -> Result<Value> {
    build_snapshot(cfg, false).await
}

async fn collect_budget_snapshot_preview(cfg: &AppConfig) -> Result<Value> {
    let repo_root = discover_repo_root(None)?;
    if let Some(thread_id) = codex_threads::current_thread_id() {
        if let Some(snapshot) = load_shared_budget_snapshot_preview(&repo_root, Some(&thread_id)) {
            return Ok(snapshot);
        }
    }
    collect_client_budget_snapshot_with_thread_hint(
        cfg,
        &repo_root,
        codex_threads::current_thread_id().as_deref(),
        None,
        None,
    )
    .await
}

fn load_shared_budget_snapshot_preview(repo_root: &Path, thread_id: Option<&str>) -> Option<Value> {
    let thread_id = thread_id.map(str::trim).filter(|value| !value.is_empty())?;
    load_shared_thread_bound_budget_snapshot(repo_root, current_epoch_ms_u64(), thread_id)
}

async fn latest_repo_working_state_restore_payload(
    db: &Client,
    repo_root: &Path,
) -> Result<Option<Value>> {
    let repo_root_string = repo_root.display().to_string();
    let project = match postgres::get_project_by_repo_root(db, &repo_root_string).await {
        Ok(project) => project,
        Err(_) => return Ok(None),
    };
    let latest_snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    let Some(snapshot_payload) = latest_snapshot else {
        return Ok(None);
    };
    let mut snapshot_payload = snapshot_payload;
    working_state::ensure_runtime_workspace_restore_pack(&mut snapshot_payload);
    let namespace_code = snapshot_payload["working_state_restore"]["namespace"]["code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(namespace_code) = namespace_code else {
        return Ok(Some(snapshot_payload));
    };
    let namespace =
        match postgres::get_namespace_by_code(db, project.project_id, namespace_code).await {
            Ok(namespace) => namespace,
            Err(_) => return Ok(Some(snapshot_payload)),
        };
    let Some(bundle) =
        working_state::load_recent_restore_bundle_without_live_guard(db, &project, &namespace)
            .await?
    else {
        return Ok(Some(snapshot_payload));
    };
    Ok(Some(json!({
        "working_state_restore": bundle["working_state_restore"].clone()
    })))
}

async fn continuity_restore_bundle_for_repo_root(
    db: &Client,
    repo_root: &Path,
) -> Result<Option<Value>> {
    let repo_root_string = repo_root.display().to_string();
    let project = match postgres::get_project_by_repo_root(db, &repo_root_string).await {
        Ok(project) => project,
        Err(_) => return Ok(None),
    };
    let latest_snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    if let Some(mut snapshot_payload) = latest_snapshot {
        working_state::ensure_runtime_workspace_restore_pack(&mut snapshot_payload);
        return Ok(Some(json!({
            "working_state_restore": snapshot_payload["working_state_restore"].clone()
        })));
    }
    let Some(namespace) =
        postgres::find_namespace_by_code(db, project.project_id, "continuity").await?
    else {
        return Ok(None);
    };
    working_state::load_recent_restore_bundle_without_live_guard(db, &project, &namespace).await
}

async fn reconcile_visible_recent_thread_execctl_activity(db: &Client) -> Result<()> {
    let mut latest_visible_thread_by_repo_root: std::collections::BTreeMap<
        String,
        codex_threads::RecentClientThreadRecord,
    > = std::collections::BTreeMap::new();
    for thread in codex_threads::recent_client_thread_records(30 * 60)?
        .into_iter()
        .filter(observe_user_visible_client_thread)
    {
        let key = thread.cwd.trim().to_string();
        if key.is_empty() {
            continue;
        }
        match latest_visible_thread_by_repo_root.get(&key) {
            Some(existing) if existing.updated_at_epoch_s >= thread.updated_at_epoch_s => {}
            _ => {
                latest_visible_thread_by_repo_root.insert(key, thread);
            }
        }
    }
    let mut recent_threads = latest_visible_thread_by_repo_root
        .into_values()
        .collect::<Vec<_>>();
    recent_threads.sort_by_key(|thread| thread.updated_at_epoch_s);
    for thread in &recent_threads {
        let repo_root = Path::new(&thread.cwd);
        let restore = continuity_restore_bundle_for_repo_root(db, repo_root).await?;
        working_state::maintain_same_thread_execctl_active_lease_for_guard(
            db,
            restore.as_ref(),
            Some(thread.thread_id.as_str()),
        )
        .await?;
    }
    Ok(())
}

async fn build_snapshot(cfg: &AppConfig, persist_snapshot: bool) -> Result<Value> {
    let profile = load_profile()?;
    let repo_root = discover_repo_root(None)?;
    let db = postgres::connect_admin(cfg).await?;
    if persist_snapshot {
        return with_postgres_advisory_lock(
            &db,
            OBSERVE_SYSTEM_SNAPSHOT_PERSIST_ADVISORY_LOCK_KEY,
            "failed to acquire observe system snapshot advisory lock",
            "failed to release observe system snapshot advisory lock",
            || async {
                build_snapshot_with_connected_admin_db(cfg, &profile, &repo_root, &db, true).await
            },
        )
        .await;
    }
    build_snapshot_with_connected_admin_db(cfg, &profile, &repo_root, &db, false).await
}

async fn build_snapshot_with_connected_admin_db(
    cfg: &AppConfig,
    profile: &ObservabilityProfile,
    repo_root: &Path,
    db: &Client,
    persist_snapshot: bool,
) -> Result<Value> {
    let snapshot_started = Instant::now();
    if persist_snapshot {
        maybe_cleanup_observability_snapshots_with_db(db).await?;
    }
    let mut observe_refresh_stage_ms = serde_json::Map::new();
    let previous = timed_future(
        &mut observe_refresh_stage_ms,
        "previous_system_snapshot",
        postgres::latest_observability_snapshot(&db, "system_snapshot"),
    )
    .await?;
    let http = http_client()?;
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    timed_future(
        &mut observe_refresh_stage_ms,
        "reconcile_recent_visible_thread_execctl_activity",
        reconcile_visible_recent_thread_execctl_activity(db),
    )
    .await?;

    let mut postgres_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_postgres_live",
        collect_postgres_live(&db, &profile.snapshot),
    )
    .await?;
    if let Some(object) = postgres_live.as_object_mut() {
        object.insert(
            "captured_at_epoch_ms".to_string(),
            Value::from(captured_at_epoch_ms),
        );
    }
    let qdrant_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_qdrant_live",
        collect_qdrant_live(cfg, &http),
    )
    .await?;
    let benchmark_qdrant_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_benchmark_qdrant_live",
        collect_optional_benchmark_qdrant_live(cfg, &http),
    )
    .await;
    let nats_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_nats_live",
        collect_nats_live(cfg, &http, &profile.snapshot),
    )
    .await?;
    let s3_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_s3_live",
        collect_s3_live(cfg),
    )
    .await?;
    let compatibility_report = timed_future(
        &mut observe_refresh_stage_ms,
        "compatibility_check",
        compatibility::check(cfg),
    )
    .await?;

    let latest_hot = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_hot",
        postgres::latest_observability_snapshot(&db, "retrieval_benchmark_hot"),
    )
    .await?;
    let latest_cold = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_cold",
        postgres::latest_observability_snapshot(&db, "retrieval_benchmark_cold"),
    )
    .await?;
    let latest_index = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_index_project",
        postgres::latest_observability_snapshot(&db, "index_project"),
    )
    .await?;
    let latest_accuracy = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_accuracy",
        postgres::latest_observability_snapshot(&db, "retrieval_accuracy"),
    )
    .await?;
    let (latest_load_hot, latest_load_hot_raw) = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_load_hot",
        latest_clean_benchmark_snapshot(&db, "retrieval_load_hot", "load_verification"),
    )
    .await?;
    let (latest_load_cold, latest_load_cold_raw) = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_load_cold",
        latest_clean_benchmark_snapshot(&db, "retrieval_load_cold", "load_verification"),
    )
    .await?;
    let latest_token_benchmark = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_token_benchmark",
        postgres::latest_observability_snapshot(&db, "token_benchmark"),
    )
    .await?;
    let latest_procedural_benchmark = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_procedural_benchmark",
        postgres::latest_observability_snapshot(&db, "procedural_benchmark"),
    )
    .await?;
    let procedural_benchmark_history = timed_future(
        &mut observe_refresh_stage_ms,
        "procedural_benchmark_history",
        procedural_benchmark_history_surface(&db),
    )
    .await?;
    let latest_memory_benchmark_score = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_memory_benchmark_score",
        postgres::latest_observability_snapshot(&db, "memory_benchmark_score"),
    )
    .await?;
    let latest_cold_path_benchmark = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_cold_path_benchmark",
        latest_dashboard_cold_benchmark_snapshot(&db),
    )
    .await?;
    let cold_path_benchmark_progress = read_live_cold_benchmark_progress(&repo_root);
    let cold_path_benchmark_progress = timed_future(
        &mut observe_refresh_stage_ms,
        "cold_path_benchmark_progress",
        enrich_live_cold_benchmark_progress(&db, cold_path_benchmark_progress),
    )
    .await?;
    let latest_working_state_restore = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_working_state_restore",
        postgres::latest_observability_snapshot(&db, "working_state_restore"),
    )
    .await?;
    let latest_repo_working_state_restore = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_repo_working_state_restore",
        latest_repo_working_state_restore_payload(&db, &repo_root),
    )
    .await?;
    let agent_scope_activity = timed_future(
        &mut observe_refresh_stage_ms,
        "agent_scope_activity",
        token_budget::collect_agent_scope_activity(&db),
    )
    .await?;
    let active_agent_budget = timed_future(
        &mut observe_refresh_stage_ms,
        "active_agent_budget",
        token_budget::collect_active_agent_live_budget_surface(
            &db,
            &repo_root,
            &agent_scope_activity,
        ),
    )
    .await?;
    let latest_degradation_verification = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_degradation_verification",
        postgres::latest_observability_snapshot(&db, "degradation_verification"),
    )
    .await?;
    let latest_continuity_verification = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_continuity_verification",
        postgres::latest_observability_snapshot(&db, "continuity_verification"),
    )
    .await?;
    let token_budget_report = if !persist_snapshot {
        if let Some(thread_id) = codex_threads::current_thread_id()
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(thread_bound_snapshot) = load_shared_thread_bound_budget_snapshot(
                repo_root,
                current_epoch_ms_u64(),
                thread_id,
            ) {
                thread_bound_snapshot["token_budget_report"].clone()
            } else {
                timed_future(
                    &mut observe_refresh_stage_ms,
                    "token_budget_dashboard_report",
                    token_budget::collect_dashboard_report(&db),
                )
                .await?
            }
        } else {
            timed_future(
                &mut observe_refresh_stage_ms,
                "token_budget_dashboard_report",
                token_budget::collect_dashboard_report(&db),
            )
            .await?
        }
    } else {
        timed_future(
            &mut observe_refresh_stage_ms,
            "token_budget_dashboard_report",
            token_budget::collect_dashboard_report(&db),
        )
        .await?
    };
    let artifact_cleanup_summary = timed_future(
        &mut observe_refresh_stage_ms,
        "artifact_cleanup_summary",
        async { artifact_cleanup::read_latest_summary(&repo_root) },
    )
    .await?
    .unwrap_or_else(|| {
        json!({
            "artifact_cleanup": {
                "status": "ещё нет данных"
            }
        })
    });

    let payload = json!({
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "stack_name": cfg.stack_name,
        "thresholds": profile_thresholds_json(&profile),
        "postgres": with_postgres_rates(&postgres_live, previous.as_ref()),
        "qdrant": qdrant_live,
        "benchmark_qdrant": benchmark_qdrant_live,
        "nats": nats_live,
        "s3": s3_live,
        "compatibility": compatibility::report_json(&compatibility_report),
        "latest_index_project": latest_index,
        "latest_retrieval_hot": latest_hot,
        "latest_retrieval_cold": latest_cold,
        "latest_retrieval_accuracy": latest_accuracy,
        "latest_retrieval_load_hot": latest_load_hot,
        "latest_retrieval_load_hot_raw": latest_load_hot_raw,
        "latest_retrieval_load_cold": latest_load_cold,
        "latest_retrieval_load_cold_raw": latest_load_cold_raw,
        "latest_token_benchmark": latest_token_benchmark,
        "latest_procedural_benchmark": latest_procedural_benchmark,
        "procedural_benchmark_history": procedural_benchmark_history,
        "latest_memory_benchmark_score": latest_memory_benchmark_score,
        "latest_cold_path_benchmark": latest_cold_path_benchmark,
        "cold_path_benchmark_progress": cold_path_benchmark_progress,
        "latest_working_state_restore": latest_working_state_restore,
        "latest_repo_working_state_restore": latest_repo_working_state_restore,
        "agent_scope_activity": agent_scope_activity,
        "active_agent_budget": active_agent_budget,
        "latest_degradation_verification": latest_degradation_verification,
        "latest_continuity_verification": latest_continuity_verification,
        "token_budget_report": token_budget_report,
        "artifact_cleanup": artifact_cleanup_summary["artifact_cleanup"].clone(),
    });
    let governance_surface = timed_future(
        &mut observe_refresh_stage_ms,
        "governance_surface",
        collect_governance_surface(db),
    )
    .await?;
    let degradation_model = build_degradation_model(&payload)?;
    let continuity_correctness_model = build_continuity_correctness_model(&payload)?;
    let sla = evaluate_sla(&payload, &profile);
    let snapshot = json!({
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "stack_name": cfg.stack_name,
        "thresholds": payload["thresholds"].clone(),
        "postgres": payload["postgres"].clone(),
        "qdrant": payload["qdrant"].clone(),
        "benchmark_qdrant": payload["benchmark_qdrant"].clone(),
        "nats": payload["nats"].clone(),
        "s3": payload["s3"].clone(),
        "compatibility": payload["compatibility"].clone(),
        "latest_index_project": payload["latest_index_project"].clone(),
        "latest_retrieval_hot": payload["latest_retrieval_hot"].clone(),
        "latest_retrieval_cold": payload["latest_retrieval_cold"].clone(),
        "latest_retrieval_accuracy": payload["latest_retrieval_accuracy"].clone(),
        "latest_retrieval_load_hot": payload["latest_retrieval_load_hot"].clone(),
        "latest_retrieval_load_hot_raw": payload["latest_retrieval_load_hot_raw"].clone(),
        "latest_retrieval_load_cold": payload["latest_retrieval_load_cold"].clone(),
        "latest_retrieval_load_cold_raw": payload["latest_retrieval_load_cold_raw"].clone(),
        "latest_token_benchmark": payload["latest_token_benchmark"].clone(),
        "latest_procedural_benchmark": payload["latest_procedural_benchmark"].clone(),
        "procedural_benchmark_history": payload["procedural_benchmark_history"].clone(),
        "latest_memory_benchmark_score": payload["latest_memory_benchmark_score"].clone(),
        "latest_cold_path_benchmark": payload["latest_cold_path_benchmark"].clone(),
        "cold_path_benchmark_progress": payload["cold_path_benchmark_progress"].clone(),
        "latest_working_state_restore": payload["latest_working_state_restore"].clone(),
        "latest_repo_working_state_restore": payload["latest_repo_working_state_restore"].clone(),
        "agent_scope_activity": payload["agent_scope_activity"].clone(),
        "active_agent_budget": payload["active_agent_budget"].clone(),
        "latest_degradation_verification": payload["latest_degradation_verification"].clone(),
        "latest_continuity_verification": payload["latest_continuity_verification"].clone(),
        "token_budget_report": payload["token_budget_report"].clone(),
        "client_budget_guard": dashboard::current_session_budget_guard(&payload),
        "artifact_cleanup": payload["artifact_cleanup"].clone(),
        "observe_refresh": {
            "total_ms": snapshot_started.elapsed().as_millis() as u64,
            "stage_ms": observe_refresh_stage_ms,
        },
        "degradation_model": degradation_model,
        "continuity_correctness_model": continuity_correctness_model,
        "governance_surface": governance_surface,
        "sla": sla,
    });
    if persist_snapshot {
        let _ = postgres::insert_observability_snapshot(&db, "system_snapshot", &snapshot).await?;
    }
    Ok(snapshot)
}

async fn collect_governance_surface(db: &Client) -> Result<Value> {
    let open_conflicts: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_conflicts
            WHERE conflict_state = 'open'
            "#,
            &[],
        )
        .await
        .context("governance: count open conflicts")?
        .get(0);

    let active_quarantine: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.quarantine_items
            WHERE quarantine_state = 'active'
            "#,
            &[],
        )
        .await
        .context("governance: count active quarantine items")?
        .get(0);

    let poisoned_provenance: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_provenance
            WHERE details->>'poisoned' = 'true'
               OR details->'safety'->>'poisoned' = 'true'
            "#,
            &[],
        )
        .await
        .context("governance: count poisoned provenance")?
        .get(0);

    let disputed_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE trust_state = 'disputed'
            "#,
            &[],
        )
        .await
        .context("governance: count disputed memory items")?
        .get(0);

    let quarantined_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE trust_state = 'quarantined'
            "#,
            &[],
        )
        .await
        .context("governance: count quarantined memory items")?
        .get(0);

    let stale_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE consolidation_status IN ('archived', 'pruned')
            "#,
            &[],
        )
        .await
        .context("governance: count stale (archived/pruned) memory items")?
        .get(0);

    let total_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            "#,
            &[],
        )
        .await
        .context("governance: count total memory items")?
        .get(0);

    let active_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE consolidation_status = 'active'
            "#,
            &[],
        )
        .await
        .context("governance: count active memory items")?
        .get(0);

    let duplicate_fact_triples: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0) FROM (
                SELECT fact_subject, fact_predicate, fact_object
                FROM ami.memory_cards
                WHERE truth_state = 'current'
                  AND status = 'active'
                  AND fact_subject IS NOT NULL
                  AND fact_predicate IS NOT NULL
                  AND fact_object IS NOT NULL
                  AND superseded_by_memory_card_id IS NULL
                GROUP BY fact_subject, fact_predicate, fact_object
                HAVING COUNT(*) > 1
            ) dup
            "#,
            &[],
        )
        .await
        .context("governance: count duplicate active truth fact triples")?
        .get(0);

    let scope_override_events_count: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.scope_override_events
            "#,
            &[],
        )
        .await
        .context("governance: count scope override events")?
        .get(0);

    let forgetting_actions_count: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.forgetting_audit_log
            "#,
            &[],
        )
        .await
        .context("governance: count forgetting audit log entries")?
        .get(0);

    let forgetting_action_breakdown_rows = db
        .query(
            r#"
            SELECT action, COUNT(*)::bigint
            FROM ami.forgetting_audit_log
            GROUP BY action
            ORDER BY action ASC
            "#,
            &[],
        )
        .await
        .context("governance: forgetting action breakdown")?;

    let quarantine_breakdown_rows = db
        .query(
            r#"
            SELECT
                COALESCE(quarantine_reason, 'unknown') AS quarantine_reason,
                COALESCE(entity_kind, 'unknown') AS entity_kind,
                COALESCE(source_kind, 'unknown') AS source_kind,
                COUNT(*)::bigint AS item_count
            FROM ami.quarantine_items
            WHERE quarantine_state = 'active'
            GROUP BY 1, 2, 3
            ORDER BY item_count DESC, quarantine_reason ASC, entity_kind ASC, source_kind ASC
            LIMIT 5
            "#,
            &[],
        )
        .await
        .context("governance: quarantine breakdown")?;

    let conflict_breakdown_rows = db
        .query(
            r#"
            SELECT
                COALESCE(summary, 'unknown') AS summary,
                COALESCE(source_kind, 'unknown') AS source_kind,
                COUNT(*)::bigint AS item_count
            FROM ami.memory_conflicts
            WHERE conflict_state = 'open'
            GROUP BY 1, 2
            ORDER BY item_count DESC, summary ASC, source_kind ASC
            LIMIT 5
            "#,
            &[],
        )
        .await
        .context("governance: conflict breakdown")?;

    let mut prune_ttl_expired_count = 0_i64;
    let mut prune_low_utility_count = 0_i64;
    let mut archive_cold_tier_count = 0_i64;
    let mut revalidate_stale_count = 0_i64;
    let mut dedup_compacted_count = 0_i64;
    let mut forgetting_action_breakdown = serde_json::Map::new();
    for row in forgetting_action_breakdown_rows {
        let action: String = row.get(0);
        let count: i64 = row.get(1);
        match action.as_str() {
            "prune_ttl_expired" => prune_ttl_expired_count = count,
            "prune_low_utility" => prune_low_utility_count = count,
            "archive_cold_tier" => archive_cold_tier_count = count,
            "revalidate_stale" => revalidate_stale_count = count,
            "dedup_compacted" => dedup_compacted_count = count,
            _ => {}
        }
        forgetting_action_breakdown.insert(action, json!(count));
    }

    let forgetting_job_breakdown = json!({
        "de_duplication_job": dedup_compacted_count,
        "summarization_job": 0,
        "compaction_job": dedup_compacted_count,
        "pruning_job": prune_ttl_expired_count + prune_low_utility_count,
        "cold_archive_job": archive_cold_tier_count,
        "revalidation_job": revalidate_stale_count
    });

    let quarantine_breakdown: Vec<Value> = quarantine_breakdown_rows
        .into_iter()
        .map(|row| {
            let reason: String = row.get(0);
            let entity_kind: String = row.get(1);
            let source_kind: String = row.get(2);
            let item_count: i64 = row.get(3);
            json!({
                "quarantine_reason": reason,
                "entity_kind": entity_kind,
                "source_kind": source_kind,
                "item_count": item_count
            })
        })
        .collect();

    let conflict_breakdown: Vec<Value> = conflict_breakdown_rows
        .into_iter()
        .map(|row| {
            let summary: String = row.get(0);
            let source_kind: String = row.get(1);
            let item_count: i64 = row.get(2);
            json!({
                "summary": summary,
                "source_kind": source_kind,
                "item_count": item_count
            })
        })
        .collect();

    let stale_memory_error_rate = if total_memory_items > 0 {
        stale_memory_items as f64 / total_memory_items as f64
    } else {
        0.0
    };

    let duplicate_branch_rate = if total_memory_items > 0 {
        duplicate_fact_triples as f64 / total_memory_items as f64
    } else {
        0.0
    };

    Ok(json!({
        "governance_surface_version": "governance-surface-v2",
        "wrong_link_rate": {
            "open_conflict_count": open_conflicts,
            "note": "wrong-link rate is proxied by open memory_conflicts with kind='scope' or 'truth'"
        },
        "duplicate_branch_rate": {
            "duplicate_active_truth_fact_triples": duplicate_fact_triples,
            "rate": duplicate_branch_rate,
            "note": "duplicate truth branches: same fact triple active as current without supersession"
        },
        "stale_memory_error_rate": {
            "stale_items_archived_or_pruned": stale_memory_items,
            "total_memory_items": total_memory_items,
            "active_memory_items": active_memory_items,
            "rate": stale_memory_error_rate,
            "note": "ratio of archived/pruned items to total — higher means more aggressive cleanup"
        },
        "cross_project_leak_rate": {
            "note": "surfaced via degradation_model.cross_project_scope and latest_retrieval_accuracy.accuracy_verification.cross_project_leakage"
        },
        "poisoning_alert_count": {
            "poisoned_provenance_count": poisoned_provenance,
            "active_quarantine_items": active_quarantine,
            "quarantined_memory_items": quarantined_memory_items,
            "active_quarantine_breakdown": quarantine_breakdown,
            "note": "sum of poisoned provenance marks and active quarantine items"
        },
        "open_conflict_breakdown": conflict_breakdown,
        "abstention_quality": {
            "note": "surfaced via continuity_correctness_model and latest_memory_benchmark_score.memory_benchmark_score.capability_breakdown.longmemeval_abstention_accuracy"
        },
        "recovery_quality": {
            "note": "surfaced via continuity_correctness_model.summary.recovered_useful"
        },
        "trust_state_distribution": {
            "disputed_memory_items": disputed_memory_items,
            "quarantined_memory_items": quarantined_memory_items
        },
        "human_override_audit": {
            "scope_override_events_total": scope_override_events_count,
            "forgetting_audit_log_entries_total": forgetting_actions_count
        },
        "forgetting_job_breakdown": forgetting_job_breakdown,
        "forgetting_action_breakdown": forgetting_action_breakdown
    }))
}

async fn with_postgres_advisory_lock<T, F, Fut>(
    db: &Client,
    key: i64,
    acquire_error: &'static str,
    release_error: &'static str,
    f: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    db.query_one("SELECT pg_advisory_lock($1)", &[&key])
        .await
        .context(acquire_error)?;
    let result = f().await;
    let unlock_result = db
        .query_one("SELECT pg_advisory_unlock($1)", &[&key])
        .await
        .context(release_error);
    match (result, unlock_result) {
        (Ok(value), Ok(_)) => Ok(value),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(unlock_error)) => Err(unlock_error),
        (Err(error), Err(unlock_error)) => Err(anyhow!(
            "{error:#}\nsecondary unlock failure: {unlock_error:#}"
        )),
    }
}

async fn timed_future<T, F>(
    timings: &mut serde_json::Map<String, Value>,
    label: &str,
    future: F,
) -> T
where
    F: Future<Output = T>,
{
    let started = Instant::now();
    let value = future.await;
    timings.insert(
        label.to_string(),
        Value::from(started.elapsed().as_millis() as u64),
    );
    value
}

fn build_continuity_correctness_model(payload: &Value) -> Result<Value> {
    let verification = &payload["latest_continuity_verification"]["continuity_verification"];
    if !verification.is_object() {
        return Ok(json!({
            "summary": {
                "status": "unknown",
                "probe_count": 0,
                "verified_probes": 0,
                "failed_probes": 0,
                "recovered_useful": 0,
                "fail_closed": 0,
                "evidence_gap": true,
            },
            "failed_probe_names": [],
            "last_evidence_at_epoch_ms": Value::Null,
        }));
    }

    let canonical_eval = &verification["canonical_eval"];
    let probe_count = verification["probe_count"].as_u64().unwrap_or_else(|| {
        canonical_eval["probes"]
            .as_array()
            .map_or(0, |items| items.len() as u64)
    });
    let failed_probe_names = verification["failed_probes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let failed_probes = failed_probe_names.len() as u64;
    let verified_probes = verification["verified_probes"]
        .as_u64()
        .unwrap_or_else(|| probe_count.saturating_sub(failed_probes));
    let verification_status =
        verification["verification_status"]
            .as_str()
            .unwrap_or(if failed_probes > 0 {
                "critical"
            } else {
                "pass"
            });
    Ok(json!({
        "summary": {
            "status": verification_status,
            "probe_count": probe_count,
            "verified_probes": verified_probes,
            "failed_probes": failed_probes,
            "recovered_useful": canonical_eval["verdict_counts"]["recovered_useful"].as_u64().unwrap_or(0),
            "fail_closed": canonical_eval["verdict_counts"]["hit_correct_target"].as_u64().unwrap_or(0),
            "evidence_gap": false,
        },
        "failed_probe_names": failed_probe_names,
        "last_evidence_at_epoch_ms": verification["captured_at_epoch_ms"].clone(),
    }))
}

fn build_degradation_model(payload: &Value) -> Result<Value> {
    let entries = retrieval_science::degradation_matrix_entries()?;
    let matrix_json = retrieval_science::degradation_matrix_json()?;
    let truth_ranking = matrix_json
        .get("truth_ranking")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let classes = entries
        .into_iter()
        .map(|(class_key, entry)| evaluate_degradation_class(payload, &class_key, &entry))
        .collect::<Vec<_>>();

    let fail_closed_total = classes
        .iter()
        .filter(|item| item["mode"].as_str() == Some("fail_closed"))
        .count() as u64;
    let graceful_total = classes
        .iter()
        .filter(|item| item["mode"].as_str() == Some("graceful_fallback"))
        .count() as u64;
    let pass = classes
        .iter()
        .filter(|item| item["status"].as_str() == Some("pass"))
        .count() as u64;
    let critical = classes
        .iter()
        .filter(|item| item["status"].as_str() == Some("critical"))
        .count() as u64;
    let unknown = classes
        .iter()
        .filter(|item| item["status"].as_str() == Some("unknown"))
        .count() as u64;
    let evidence_gaps = classes
        .iter()
        .filter(|item| item["evidence_gap"].as_bool() == Some(true))
        .count() as u64;
    let overall_status = if critical > 0 {
        "critical"
    } else if unknown > 0 {
        "unknown"
    } else {
        "pass"
    };

    Ok(json!({
        "policy_version": matrix_json["policy_version"].clone(),
        "truth_ranking": truth_ranking,
        "summary": {
            "status": overall_status,
            "pass": pass,
            "critical": critical,
            "unknown": unknown,
            "fail_closed_total": fail_closed_total,
            "graceful_fallback_total": graceful_total,
            "evidence_gaps": evidence_gaps,
        },
        "classes": classes,
    }))
}

fn evaluate_degradation_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
) -> Value {
    match class_key {
        "cross_project_scope" => evaluate_accuracy_degradation_class(
            payload,
            class_key,
            entry,
            "cross_project_leakage",
            &[
                "strict_local_visible_projects_only",
                "strict_local_hits_do_not_leak_projects",
                "hostile_mixed_query_fail_closed",
                "hostile_mixed_query_visible_projects_only",
                "hostile_mixed_query_hits_do_not_leak_projects",
            ],
            "Последний accuracy / isolation прогон подтвердил zero leakage между проектами.",
        ),
        "cross_namespace_scope" => evaluate_accuracy_degradation_class(
            payload,
            class_key,
            entry,
            "cross_namespace_leakage",
            &[
                "strict_local_visible_namespaces_only",
                "strict_local_hits_do_not_leak_namespaces",
                "hostile_mixed_query_visible_namespaces_only",
                "hostile_mixed_query_hits_do_not_leak_namespaces",
                "namespace_strict_visible_projects_only",
                "namespace_strict_hits_do_not_leak_namespaces",
                "namespace_strict_fail_closed",
            ],
            "Последний accuracy / isolation прогон подтвердил zero leakage между namespace.",
        ),
        "cross_agent_scope"
        | "corrupt_scope_metadata"
        | "partial_refresh"
        | "qdrant_unavailable"
        | "stale_cache"
        | "partial_thread_index"
        | "empty_embeddings"
        | "stale_handoff"
        | "working_state_conflict" => {
            evaluate_degradation_verification_class(payload, class_key, entry)
        }
        _ => evaluate_policy_gap_class(payload, class_key, entry),
    }
}

fn evaluate_accuracy_degradation_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
    leakage_key: &str,
    invariant_names: &[&str],
    success_reason: &str,
) -> Value {
    let accuracy = &payload["latest_retrieval_accuracy"]["accuracy_verification"];
    if !accuracy.is_object() {
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            "Свежий accuracy / isolation verification ещё не записан.",
            None,
            None,
            true,
        );
    }
    let captured_at = accuracy["captured_at_epoch_ms"].as_u64();
    let leakage = accuracy[leakage_key]
        .as_f64()
        .or_else(|| accuracy[leakage_key].as_u64().map(|value| value as f64))
        .unwrap_or(0.0);
    let invariants = invariant_names
        .iter()
        .map(|name| {
            (
                *name,
                accuracy["formal_invariants"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|item| item["name"].as_str() == Some(*name))
                    .and_then(|item| item["pass"].as_bool()),
            )
        })
        .collect::<Vec<_>>();
    let missing = invariants
        .iter()
        .filter(|(_, pass)| pass.is_none())
        .map(|(name, _)| (*name).to_string())
        .collect::<Vec<_>>();
    let failed = invariants
        .iter()
        .filter_map(|(name, pass)| {
            if pass == &Some(false) {
                Some((*name).to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if leakage > 0.0 || !failed.is_empty() {
        let mut reasons = Vec::new();
        if leakage > 0.0 {
            reasons.push(format!("observed leakage = {}", leakage));
        }
        if !failed.is_empty() {
            reasons.push(format!("formal invariants failed: {}", failed.join(", ")));
        }
        return degradation_class_value(
            class_key,
            entry,
            "critical",
            &format!("Последний proof поймал нарушение: {}.", reasons.join("; ")),
            Some("retrieval_accuracy"),
            captured_at,
            false,
        );
    }

    if !missing.is_empty() {
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            &format!(
                "Последний accuracy proof неполный: не хватает formal invariants {}.",
                missing.join(", ")
            ),
            Some("retrieval_accuracy"),
            captured_at,
            true,
        );
    }

    degradation_class_value(
        class_key,
        entry,
        "pass",
        success_reason,
        Some("retrieval_accuracy"),
        captured_at,
        false,
    )
}

fn evaluate_policy_gap_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
) -> Value {
    let working_state = &payload["latest_working_state_restore"]["working_state_restore"];
    if class_key == "stale_handoff" && working_state.is_object() {
        let freshness = working_state["restore_freshness_state"]
            .as_str()
            .unwrap_or("ещё нет данных");
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            &format!(
                "Текущий рабочий снимок уже умеет помечать freshness = {freshness}, но отдельный degradation proof для этого класса ещё не записан."
            ),
            Some("working_state_restore"),
            working_state["captured_at_epoch_ms"].as_u64(),
            true,
        );
    }

    if class_key == "working_state_conflict" && working_state.is_object() {
        let confidence = working_state["restore_confidence"]
            .as_str()
            .unwrap_or("ещё нет данных");
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            &format!(
                "Текущий рабочий снимок уже даёт confidence = {confidence}, но отдельный conflict-proof для этого класса ещё не записан."
            ),
            Some("working_state_restore"),
            working_state["captured_at_epoch_ms"].as_u64(),
            true,
        );
    }

    degradation_class_value(
        class_key,
        entry,
        "unknown",
        &format!(
            "Этот класс уже описан в policy, но свежий machine-readable proof через '{}' пока не materialized.",
            entry.evidence_source
        ),
        None,
        None,
        true,
    )
}

fn evaluate_degradation_verification_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
) -> Value {
    let verification = &payload["latest_degradation_verification"]["degradation_verification"];
    if !verification.is_object() {
        return evaluate_policy_gap_class(payload, class_key, entry);
    }
    let scenario = verification["scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["class_key"].as_str() == Some(class_key));
    let Some(scenario) = scenario else {
        return evaluate_policy_gap_class(payload, class_key, entry);
    };

    degradation_class_value(
        class_key,
        entry,
        scenario["status"].as_str().unwrap_or("unknown"),
        scenario["reason"].as_str().unwrap_or("ещё нет деталей"),
        Some("degradation_verification"),
        verification["captured_at_epoch_ms"].as_u64(),
        scenario["status"].as_str() != Some("pass"),
    )
}

fn degradation_class_value(
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
    status: &str,
    reason: &str,
    last_evidence_kind: Option<&str>,
    last_evidence_at_epoch_ms: Option<u64>,
    evidence_gap: bool,
) -> Value {
    json!({
        "class_key": class_key,
        "title": entry.title,
        "mode": entry.mode,
        "summary": entry.summary,
        "expected_behavior": entry.expected_behavior,
        "user_signal": entry.user_signal,
        "evidence_source": entry.evidence_source,
        "runbook": entry.runbook,
        "status": status,
        "reason": reason,
        "last_evidence_kind": last_evidence_kind,
        "last_evidence_at_epoch_ms": last_evidence_at_epoch_ms,
        "evidence_gap": evidence_gap,
    })
}

async fn latest_clean_benchmark_snapshot(
    db: &tokio_postgres::Client,
    snapshot_kind: &str,
    expected_root: &str,
) -> Result<(Option<Value>, Option<Value>)> {
    let latest_raw = postgres::latest_observability_snapshot(db, snapshot_kind).await?;
    let latest_clean =
        postgres::latest_clean_benchmark_snapshot_payload(db, snapshot_kind, expected_root).await?;
    Ok((latest_clean, latest_raw))
}

async fn latest_dashboard_cold_benchmark_snapshot(
    db: &tokio_postgres::Client,
) -> Result<Option<Value>> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["cold_path_benchmark"], Some(64))
            .await?;
    let payloads: Vec<Value> = rows.into_iter().map(|row| row.payload).collect();
    Ok(select_latest_dashboard_cold_benchmark_snapshot(&payloads))
}

#[cfg(test)]
fn select_latest_clean_benchmark_snapshot(
    payloads: &[Value],
    expected_root: &str,
) -> Option<Value> {
    payloads
        .iter()
        .find(|payload| benchmark_payload_contaminated(payload, expected_root) == Some(false))
        .cloned()
}

fn select_latest_dashboard_cold_benchmark_snapshot(payloads: &[Value]) -> Option<Value> {
    payloads
        .iter()
        .find(|payload| {
            benchmark_payload_contaminated(payload, "cold_benchmark") == Some(false)
                && cold_benchmark_dashboard_scope(payload) == Some("canonical")
        })
        .cloned()
        .or_else(|| {
            payloads
                .iter()
                .find(|payload| {
                    benchmark_payload_contaminated(payload, "cold_benchmark") == Some(false)
                })
                .cloned()
        })
}

fn cold_benchmark_dashboard_scope(payload: &Value) -> Option<&str> {
    let root = payload.get("cold_benchmark")?;
    if let Some(scope) = root["dashboard_scope"]["class"].as_str() {
        return Some(scope);
    }
    let profile_name = root["profile"]["display_name"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if profile_name.contains("proof") {
        return Some("proof");
    }
    let sample_count = root["machine_readable_summary"]["sample_count"].as_u64()?;
    let repo_count = root["machine_readable_summary"]["repo_count"].as_u64()?;
    let query_slice_count = root["machine_readable_summary"]["query_slice_count"].as_u64()?;
    let min_sample_count = root["profile"]["min_sample_count"].as_u64()?;
    let min_repo_count = root["profile"]["min_repo_count"].as_u64()?;
    let min_query_slice_count = root["profile"]["min_query_slice_count"].as_u64()?;
    if sample_count >= min_sample_count
        && repo_count >= min_repo_count
        && query_slice_count >= min_query_slice_count
    {
        Some("canonical")
    } else {
        Some("smoke")
    }
}

fn profile_thresholds_json(profile: &ObservabilityProfile) -> Value {
    let observability_policy = observability_policy::policy_json()
        .unwrap_or_else(|error| json!({ "error": format!("{error:#}") }));
    json!({
        "postgres": {
            "query_probe_p95_ms": {
                "target": profile.postgres.target_query_probe_p95_ms,
                "alert": profile.postgres.alert_query_probe_p95_ms,
                "critical": profile.postgres.critical_query_probe_p95_ms,
            },
            "connection_usage_ratio": {
                "target": profile.postgres.target_connection_usage_ratio,
                "alert": profile.postgres.alert_connection_usage_ratio,
                "critical": profile.postgres.critical_connection_usage_ratio,
            },
        },
        "qdrant": {
            "optimize_queue": {
                "target": profile.qdrant.target_index_optimize_queue,
                "alert": profile.qdrant.alert_index_optimize_queue,
                "critical": profile.qdrant.critical_index_optimize_queue,
            },
            "update_queue_length": {
                "target": profile.qdrant.target_update_queue_length,
                "alert": profile.qdrant.alert_update_queue_length,
                "critical": profile.qdrant.critical_update_queue_length,
            },
            "search_p95_ms": {
                "target": profile.qdrant.target_search_p95_ms,
                "alert": profile.qdrant.alert_search_p95_ms,
                "critical": profile.qdrant.critical_search_p95_ms,
            },
        },
        "nats": {
            "publish_probe_p95_ms": {
                "target": profile.nats.target_publish_p95_ms,
                "alert": profile.nats.alert_publish_p95_ms,
                "critical": profile.nats.critical_publish_p95_ms,
            },
            "consumer_lag_msgs": {
                "target": profile.nats.target_consumer_lag_msgs,
                "alert": profile.nats.alert_consumer_lag_msgs,
                "critical": profile.nats.critical_consumer_lag_msgs,
            },
            "jetstream_disk_usage_ratio": {
                "target": profile.nats.target_jetstream_disk_usage_ratio,
                "alert": profile.nats.alert_jetstream_disk_usage_ratio,
                "critical": profile.nats.critical_jetstream_disk_usage_ratio,
            },
        },
        "retrieval": {
            "cold_live_p95_ms": {
                "target": profile.retrieval.target_p95_ms,
                "alert": profile.retrieval.alert_p95_ms,
                "critical": profile.retrieval.critical_p95_ms,
            },
            "hot_live_p95_ms": {
                "target": profile.retrieval.target_hot_p95_ms,
                "alert": profile.retrieval.alert_p95_ms,
                "critical": profile.retrieval.critical_p95_ms,
                "stretch": profile.retrieval.stretch_hot_p95_ms,
            },
            "cold_live_table": {
                "target_p50_ms": 2.0,
                "target_p95_ms": 4.0,
                "target_p99_ms": 6.0,
                "target_max_ms": 10.0,
                "live_readiness_sample_count": 100,
                "benchmark_sample_count": profile.retrieval.target_cold_sample_count,
                "target_sample_count": profile.retrieval.target_cold_sample_count,
            },
            "hot_live_table": {
                "target_p50_ms": 1.0,
                "target_p95_ms": 2.0,
                "target_p99_ms": 3.0,
                "target_max_ms": 5.0,
                "live_readiness_sample_count": 100,
                "benchmark_sample_count": profile.retrieval.target_hot_sample_count,
                "target_sample_count": profile.retrieval.target_hot_sample_count,
            },
            "hot_benchmark_table": {
                "target_iterations": profile.retrieval.target_hot_benchmark_iterations,
                "target_warmup": profile.retrieval.target_hot_benchmark_warmup,
            },
        },
        "accuracy": {
            "symbol_precision": {
                "target": profile.accuracy.target_symbol_precision,
                "alert": profile.accuracy.alert_symbol_precision,
                "critical": profile.accuracy.critical_symbol_precision,
            },
            "semantic_precision": {
                "target": profile.accuracy.target_semantic_precision,
                "alert": profile.accuracy.alert_semantic_precision,
                "critical": profile.accuracy.critical_semantic_precision,
            },
            "cross_project_leakage": {
                "target": 0.0,
                "alert": 0.0,
                "critical": 0.0,
            },
        },
        "load": {
            "hot_qps": {
                "target": profile.load.target_hot_qps,
                "alert": profile.load.alert_hot_qps,
                "critical": profile.load.critical_hot_qps,
            },
            "hot_benchmark_table": {
                "target_p50_ms": profile.load.target_hot_p50_ms,
                "target_p95_ms": profile.load.target_hot_p95_ms,
                "target_p99_ms": profile.load.target_hot_p99_ms,
                "target_max_ms": profile.load.target_hot_max_ms,
                "target_workers": profile.load.target_hot_workers,
                "target_sample_count": profile.load.target_hot_sample_count,
            },
            "hot_error_rate": {
                "target": profile.load.target_hot_error_rate,
                "alert": profile.load.alert_hot_error_rate,
                "critical": profile.load.critical_hot_error_rate,
            },
        },
        "dashboard": {
            "refresh_ms": profile.dashboard.refresh_ms,
            "exact_client_limit_prewarm_seconds": profile.dashboard.exact_client_limit_prewarm_seconds,
            "compact_client_budget_prewarm_seconds": profile.dashboard.compact_client_budget_prewarm_seconds,
            "timing_format": {
                "switch_to_nanoseconds_below_ms": profile.dashboard.timing_format.switch_to_nanoseconds_below_ms,
                "switch_to_microseconds_below_ms": profile.dashboard.timing_format.switch_to_microseconds_below_ms,
                "switch_to_seconds_at_or_above_ms": profile.dashboard.timing_format.switch_to_seconds_at_or_above_ms,
                "non_positive_floor_label": profile.dashboard.timing_format.non_positive_floor_label,
                "seconds_suffix": profile.dashboard.timing_format.seconds_suffix,
                "milliseconds_suffix": profile.dashboard.timing_format.milliseconds_suffix,
                "microseconds_suffix": profile.dashboard.timing_format.microseconds_suffix,
                "nanoseconds_suffix": profile.dashboard.timing_format.nanoseconds_suffix,
                "seconds_decimals": profile.dashboard.timing_format.seconds_decimals,
                "milliseconds_decimals": profile.dashboard.timing_format.milliseconds_decimals,
                "microseconds_decimals": profile.dashboard.timing_format.microseconds_decimals,
                "nanoseconds_decimals": profile.dashboard.timing_format.nanoseconds_decimals,
            },
        },
        "observability": observability_policy,
    })
}

async fn maybe_cleanup_observability_snapshots(cfg: &AppConfig) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    maybe_cleanup_observability_snapshots_with_db(&db).await
}

async fn maybe_cleanup_observability_snapshots_with_db(db: &Client) -> Result<()> {
    let summary = run_retention_cleanup_with_db(db, true, Some(2048)).await?;
    let cleanup = &summary["observability_retention_cleanup"];
    let deleted = cleanup["deleted"].as_u64().unwrap_or(0);
    let expired = cleanup["expired"].as_u64().unwrap_or(0);
    if deleted > 0 || expired > 0 {
        println!(
            "Amai observability retention cleanup: deleted={}, expired={}, scanned={}",
            deleted,
            expired,
            cleanup["scanned"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
}

async fn maybe_cleanup_local_artifacts() -> Result<()> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms = current_epoch_ms_u64();
    let min_interval_ms = artifact_cleanup::sweep_interval()?
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    if let Some(summary) = artifact_cleanup::read_latest_summary(&repo_root)?
        .filter(|summary| artifact_cleanup_summary_is_fresh(summary, now_epoch_ms, min_interval_ms))
    {
        let _ = summary;
        return Ok(());
    }
    let summary = collect_artifact_cleanup_summary(&repo_root, true, true, None, false, None)?;
    let _ = artifact_cleanup::write_latest_summary(&repo_root, &summary)?;
    let cleanup = &summary["artifact_cleanup"];
    let deleted = cleanup["deleted"].as_u64().unwrap_or(0);
    let reclaimed_bytes = cleanup["reclaimed_bytes"].as_u64().unwrap_or(0);
    if deleted > 0 || reclaimed_bytes > 0 {
        eprintln!(
            "Amai artifact cleanup: deleted={}, expired={}, reclaimed_bytes={}",
            deleted,
            cleanup["expired"].as_u64().unwrap_or(0),
            reclaimed_bytes
        );
    }
    Ok(())
}

fn artifact_cleanup_summary_captured_at_epoch_ms(summary: &Value) -> Option<u64> {
    summary
        .get("artifact_cleanup")?
        .get("captured_at_epoch_ms")?
        .as_u64()
}

fn artifact_cleanup_summary_is_fresh(
    summary: &Value,
    now_epoch_ms: u64,
    min_interval_ms: u64,
) -> bool {
    artifact_cleanup_summary_captured_at_epoch_ms(summary).is_some_and(|captured_at_epoch_ms| {
        now_epoch_ms.saturating_sub(captured_at_epoch_ms) <= min_interval_ms
    })
}

fn collect_artifact_cleanup_summary(
    repo_root: &Path,
    apply: bool,
    auto_only: bool,
    limit: Option<usize>,
    aggressive: bool,
    target: Option<&str>,
) -> Result<Value> {
    let existing_last_apply = artifact_cleanup::read_latest_summary(repo_root)?
        .and_then(|summary| extract_last_artifact_cleanup_apply(&summary));
    if !apply {
        let mut current =
            artifact_cleanup::run_cleanup(repo_root, false, auto_only, limit, aggressive, target)?;
        if let Some(last_apply) = existing_last_apply {
            if let Some(object) = current["artifact_cleanup"].as_object_mut() {
                object.insert("last_apply".to_string(), last_apply);
            }
        }
        return Ok(current);
    }

    let applied =
        artifact_cleanup::run_cleanup(repo_root, true, auto_only, limit, aggressive, target)?;
    let mut current =
        artifact_cleanup::run_cleanup(repo_root, false, auto_only, None, false, target)?;
    let applied_cleanup = &applied["artifact_cleanup"];
    let last_apply = if applied_cleanup["reclaimed_bytes"].as_u64().unwrap_or(0) > 0
        || applied_cleanup["deleted"].as_u64().unwrap_or(0) > 0
    {
        json!({
            "captured_at_epoch_ms": applied_cleanup["captured_at_epoch_ms"].clone(),
            "mode": applied_cleanup["mode"].clone(),
            "auto_only": applied_cleanup["auto_only"].clone(),
            "deleted": applied_cleanup["deleted"].clone(),
            "reclaimed_bytes": applied_cleanup["reclaimed_bytes"].clone(),
            "selected": applied_cleanup["selected"].clone(),
        })
    } else {
        existing_last_apply.unwrap_or(Value::Null)
    };
    if let Some(object) = current["artifact_cleanup"].as_object_mut() {
        if !last_apply.is_null() {
            object.insert("last_apply".to_string(), last_apply);
        }
    }
    Ok(current)
}

fn extract_last_artifact_cleanup_apply(summary: &Value) -> Option<Value> {
    let cleanup = summary.get("artifact_cleanup")?;
    if let Some(last_apply) = cleanup.get("last_apply").filter(|value| value.is_object()) {
        return Some(last_apply.clone());
    }
    if cleanup.get("apply").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let reclaimed_bytes = cleanup
        .get("reclaimed_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let deleted = cleanup.get("deleted").and_then(Value::as_u64).unwrap_or(0);
    if reclaimed_bytes == 0 && deleted == 0 {
        return None;
    }
    Some(json!({
        "captured_at_epoch_ms": cleanup["captured_at_epoch_ms"].clone(),
        "mode": cleanup["mode"].clone(),
        "auto_only": cleanup["auto_only"].clone(),
        "deleted": cleanup["deleted"].clone(),
        "reclaimed_bytes": cleanup["reclaimed_bytes"].clone(),
        "selected": cleanup["selected"].clone(),
    }))
}

async fn run_retention_cleanup(cfg: &AppConfig, apply: bool, limit: Option<i64>) -> Result<Value> {
    let db = postgres::connect_admin(cfg).await?;
    run_retention_cleanup_with_db(&db, apply, limit).await
}

async fn run_retention_cleanup_with_db(
    db: &Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<Value> {
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let minimum_retention_hours = observability_policy::minimum_retention_hours()?.unwrap_or(24);
    let cutoff_epoch_ms = now_epoch_ms.saturating_sub(minimum_retention_hours * 3_600_000) as i64;
    let candidates =
        postgres::list_observability_snapshots_older_than(&db, cutoff_epoch_ms, limit).await?;
    let expired = expired_retention_candidates(&candidates, now_epoch_ms)?;
    let expired_snapshot_ids: Vec<_> = expired
        .iter()
        .filter_map(|candidate| candidate["snapshot_id"].as_str())
        .filter_map(|snapshot_id| uuid::Uuid::parse_str(snapshot_id).ok())
        .collect();
    let deleted = if apply {
        postgres::delete_observability_snapshots_by_ids(&db, &expired_snapshot_ids).await?
    } else {
        0
    };
    Ok(json!({
        "observability_retention_cleanup": {
            "apply": apply,
            "minimum_retention_hours": minimum_retention_hours,
            "cutoff_epoch_ms": cutoff_epoch_ms,
            "scanned": candidates.len(),
            "expired": expired.len(),
            "deleted": deleted,
            "candidates": expired,
        }
    }))
}

fn expired_retention_candidates(
    candidates: &[postgres::ObservabilityRetentionCandidate],
    now_epoch_ms: u64,
) -> Result<Vec<Value>> {
    let mut expired = Vec::new();
    for candidate in candidates {
        let rule = observability_policy::retention_rule(
            &candidate.snapshot_kind,
            &candidate.payload,
            &candidate.source_kind,
            &candidate.source_class,
        )?;
        let Some(ttl_hours) = rule.ttl_hours else {
            continue;
        };
        let basis_epoch_ms = candidate
            .captured_at_epoch_ms
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or_else(|| candidate.created_at_epoch_ms.max(0) as u64);
        let age_ms = now_epoch_ms.saturating_sub(basis_epoch_ms);
        let ttl_ms = ttl_hours.saturating_mul(3_600_000);
        if age_ms < ttl_ms {
            continue;
        }
        expired.push(json!({
            "snapshot_id": candidate.snapshot_id.to_string(),
            "snapshot_kind": candidate.snapshot_kind,
            "source_kind": candidate.source_kind,
            "source_class": candidate.source_class,
            "retention_class": rule.retention_class,
            "retention_ttl_hours": ttl_hours,
            "immutable_snapshot": rule.immutable_snapshot,
            "age_hours": age_ms as f64 / 3_600_000.0,
            "created_at_epoch_ms": candidate.created_at_epoch_ms,
            "captured_at_epoch_ms": candidate.captured_at_epoch_ms,
        }));
    }
    Ok(expired)
}

async fn metrics_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    let snapshot = async {
        match cached_snapshot_with_meta(&state).await {
            Ok(snapshot) => Ok(snapshot),
            Err(_) => {
                refresh_observe_cache(
                    state.cache.clone(),
                    state.cfg.clone(),
                    state.bind.clone(),
                    state.dashboard_refresh_ms,
                )
                .await?;
                cached_snapshot_with_meta(&state).await
            }
        }
    }
    .await;
    match snapshot {
        Ok(snapshot) => {
            let body = render_prometheus_metrics(&snapshot);
            let headers = [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
            )];
            (StatusCode::OK, headers, body).into_response()
        }
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("observe exporter failed to read cached snapshot: {error:#}"),
        )
            .into_response(),
    }
}

async fn dashboard_page_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    spawn_client_live_meter_refresh(&state).await;
    let bootstrap_payload = cached_dashboard_payload(&state).await.ok();
    let html = dashboard::render_html(state.dashboard_refresh_ms, bootstrap_payload.as_ref());
    (no_store_headers("text/html; charset=utf-8"), Html(html)).into_response()
}

async fn grafana_password_help_handler() -> impl IntoResponse {
    let repo_root =
        discover_repo_root(None).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let env_path = repo_root.join(".env");
    let html = format!(
        r#"<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Где менять пароль Grafana</title>
  <style>
    body {{
      margin: 0;
      padding: 32px 24px 48px;
      background: #0f171b;
      color: #edf3f5;
      font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
      line-height: 1.55;
    }}
    main {{
      max-width: 860px;
      margin: 0 auto;
      background: rgba(255, 255, 255, 0.04);
      border-radius: 18px;
      padding: 28px 28px 32px;
      box-shadow: 0 18px 42px rgba(0, 0, 0, 0.24);
    }}
    h1 {{ margin: 0 0 18px; font-size: 30px; }}
    p {{ margin: 0 0 14px; }}
    code {{
      background: rgba(255, 255, 255, 0.08);
      padding: 2px 6px;
      border-radius: 6px;
      font-family: "IBM Plex Mono", "SFMono-Regular", monospace;
      font-size: 0.95em;
    }}
    ol {{ margin: 0; padding-left: 22px; }}
    li {{ margin: 0 0 10px; }}
    a {{ color: #8de4da; }}
  </style>
</head>
<body>
  <main>
    <h1>Где менять пароль Grafana</h1>
    <p>Пароль Grafana задаётся не в самой карточке dashboard, а в локальном файле окружения проекта.</p>
    <ol>
      <li>Откройте файл <code>{}</code>.</li>
      <li>Найдите строку <code>AMI_GRAFANA_ADMIN_PASSWORD=...</code>.</li>
      <li>Поставьте новый пароль.</li>
      <li>Примените изменение: <code>./scripts/monitoring_up.sh</code>.</li>
    </ol>
    <p>Дополнительный контур: <code>AMI_GRAFANA_ADMIN_USER</code> задаёт логин администратора.</p>
    <p><a href="/dashboard">Вернуться в Amai dashboard</a></p>
  </main>
</body>
</html>"#,
        env_path.display()
    );
    Html(html).into_response()
}

async fn brand_mark_handler() -> impl IntoResponse {
    let headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    )];
    (StatusCode::OK, headers, dashboard::brand_mark_svg()).into_response()
}

async fn brand_lockup_handler() -> impl IntoResponse {
    let headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    )];
    (StatusCode::OK, headers, dashboard::brand_lockup_svg()).into_response()
}

async fn favicon_handler() -> impl IntoResponse {
    let headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/x-icon"),
    )];
    (StatusCode::OK, headers, dashboard::favicon_ico()).into_response()
}

async fn dashboard_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    spawn_client_live_meter_refresh(&state).await;
    let response: Result<Value> = async {
        let payload = if let Some(thread_id_hint) =
            normalized_thread_id_hint(query.thread_id.as_deref())
        {
            let snapshot = merged_thread_bound_snapshot_with_meta(&state, &thread_id_hint).await?;
            dashboard::build_payload(
                &state.cfg,
                &snapshot,
                &state.bind,
                state.dashboard_refresh_ms,
            )?
        } else {
            let snapshot = match cached_snapshot_with_meta(&state).await {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    refresh_observe_cache(
                        state.cache.clone(),
                        state.cfg.clone(),
                        state.bind.clone(),
                        state.dashboard_refresh_ms,
                    )
                    .await?;
                    cached_snapshot_with_meta(&state).await?
                }
            };
            dashboard::build_payload(
                &state.cfg,
                &snapshot,
                &state.bind,
                state.dashboard_refresh_ms,
            )?
        };
        let cache = state.cache.read().await;
        Ok(attach_observe_cache_to_dashboard_payload(
            payload,
            &cache,
            state.dashboard_refresh_ms,
        ))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => dashboard_api_error_response(&state, &error).await,
    }
}

async fn dashboard_live_summary_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    let response =
        dashboard_live_summary_payload_for_request(&state, query.thread_id.as_deref()).await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => dashboard_live_summary_error_response(&state, &error).await,
    }
}

async fn client_budget_live_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    let resolved_thread_id = resolved_request_thread_hint(&state, query.thread_id.as_deref()).await;
    {
        let cache = state.cache.read().await;
        let same_thread =
            cache.client_budget_live_thread_id.as_deref() == resolved_thread_id.as_deref();
        let cache_fresh = cache
            .client_budget_live_completed_epoch_ms
            .is_some_and(|completed_at| {
                now_epoch_ms().saturating_sub(completed_at)
                    <= CLIENT_BUDGET_LIVE_PAYLOAD_CACHE_TTL_MS
            });
        if same_thread
            && cache_fresh
            && let Some(payload) = cache.client_budget_live_payload.clone()
        {
            return (
                StatusCode::OK,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
                .into_response();
        }
    }
    if let Some(thread_id) = resolved_thread_id.as_deref() {
        if let Ok(repo_root) = discover_repo_root(None) {
            if let Some(snapshot) = load_shared_budget_snapshot_preview(&repo_root, Some(thread_id))
            {
                let payload = dashboard::client_budget_live_payload(&snapshot);
                let mut cache = state.cache.write().await;
                cache.client_budget_live_payload = Some(payload.clone());
                cache.client_budget_live_thread_id = Some(thread_id.to_string());
                cache.client_budget_live_completed_epoch_ms = Some(now_epoch_ms());
                return (
                    StatusCode::OK,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                )
                    .into_response();
            }
        }
    }
    spawn_client_live_meter_refresh(&state).await;
    let response =
        compact_client_budget_snapshot_for_request(&state, resolved_thread_id.as_deref()).await;
    match response {
        Ok(snapshot) => {
            let payload = dashboard::client_budget_live_payload(&snapshot);
            let mut cache = state.cache.write().await;
            cache.client_budget_live_payload = Some(payload.clone());
            cache.client_budget_live_thread_id = resolved_thread_id.clone();
            cache.client_budget_live_completed_epoch_ms = Some(now_epoch_ms());
            (
                StatusCode::OK,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
                .into_response()
        }
        Err(error) => client_budget_live_error_response(&state, &error).await,
    }
}

async fn client_budget_live_error_response(
    state: &ObserveState,
    error: &anyhow::Error,
) -> Response {
    let (refresh_in_progress, snapshot_age_ms) = {
        let cache = state.cache.read().await;
        (cache.refresh_in_progress, cache_snapshot_age_ms(&cache))
    };
    if refresh_in_progress {
        return (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "warming_up",
                "rows": [],
                "reply_prefix": Value::Null,
                "global_reply_prefix": Value::Null,
                "reply_prefix_source": "warmup_pending",
                "thread_binding_state": "warmup_pending",
                "current_thread_bound": false,
                "ended_at_epoch_ms": Value::Null,
                "warmup_pending": true,
                "snapshot_age_ms": snapshot_age_ms,
            }))
            .unwrap_or_default(),
        )
            .into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        no_store_headers("application/json; charset=utf-8"),
        serde_json::to_string_pretty(&json!({
            "status": "down",
            "error": format!("{error:#}"),
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

async fn dashboard_api_error_response(state: &ObserveState, error: &anyhow::Error) -> Response {
    let (refresh_in_progress, snapshot_age_ms) = {
        let cache = state.cache.read().await;
        (cache.refresh_in_progress, cache_snapshot_age_ms(&cache))
    };
    if refresh_in_progress {
        return (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&dashboard_warmup_payload(snapshot_age_ms))
                .unwrap_or_default(),
        )
            .into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        no_store_headers("application/json; charset=utf-8"),
        serde_json::to_string_pretty(&json!({
            "status": "down",
            "error": format!("{error:#}"),
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

async fn dashboard_live_summary_error_response(
    state: &ObserveState,
    error: &anyhow::Error,
) -> Response {
    let (refresh_in_progress, snapshot_age_ms) = {
        let cache = state.cache.read().await;
        (cache.refresh_in_progress, cache_snapshot_age_ms(&cache))
    };
    if refresh_in_progress {
        return (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&dashboard_live_summary_warmup_payload(snapshot_age_ms))
                .unwrap_or_default(),
        )
            .into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        no_store_headers("application/json; charset=utf-8"),
        serde_json::to_string_pretty(&json!({
            "status": "down",
            "error": format!("{error:#}"),
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

fn dashboard_warmup_payload(snapshot_age_ms: Option<u64>) -> Value {
    json!({
        "meta": {
            "package_version": env!("CARGO_PKG_VERSION"),
            "cache_stale": true,
            "cache_snapshot_age_ms": snapshot_age_ms,
            "observe_refresh_total_ms": Value::Null,
            "observe_refresh_slowest_stage": Value::Null,
            "observe_refresh_slowest_stage_ms": Value::Null,
            "cache_refresh_completed_at_label": Value::Null,
            "cache_refresh_duration_ms": Value::Null,
        },
        "headline": {
            "status": "waiting",
            "status_label": "идёт прогрев",
            "status_tooltip": "Observe cache ещё materialize-ится. Панель вернёт полный live snapshot после завершения первого refresh.",
            "status_reason": "Первый observe refresh ещё не завершён.",
            "token_value": "ещё нет данных",
            "token_scope": Value::Null,
        },
        "links": [],
        "hero_cards": [],
        "top_cards": [],
        "benchmark_cards": [],
        "service_cards": [],
        "machine_cards": [],
        "warnings": [
            "Панель ещё прогревается: первый live snapshot не готов."
        ],
        "glossary": []
    })
}

fn dashboard_live_summary_warmup_payload(snapshot_age_ms: Option<u64>) -> Value {
    json!({
        "meta": {
            "package_version": env!("CARGO_PKG_VERSION"),
            "cache_stale": true,
            "cache_snapshot_age_ms": snapshot_age_ms,
            "observe_refresh_total_ms": Value::Null,
            "observe_refresh_slowest_stage": Value::Null,
            "observe_refresh_slowest_stage_ms": Value::Null,
            "cache_refresh_completed_at_label": Value::Null,
            "cache_refresh_duration_ms": Value::Null,
        },
        "headline": {
            "status": "waiting",
            "status_label": "идёт прогрев",
            "status_tooltip": "Live summary ещё не готов: observe cache materialize-ится.",
            "status_reason": "Первый live summary snapshot ещё не готов.",
            "token_value": "ещё нет данных",
            "token_scope": Value::Null,
        },
        "active_agent_card": Value::Null,
        "top_cards": [],
        "warmup_pending": true
    })
}

async fn active_agent_budget_live_api_handler(
    State(state): State<ObserveState>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    spawn_client_live_meter_refresh(&state).await;
    let response = live_active_agent_budget_card_payload(&state).await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "down",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

async fn client_budget_snapshot_preview_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    let resolved_thread_id = resolved_request_thread_hint(&state, query.thread_id.as_deref()).await;
    let response =
        compact_client_budget_snapshot_for_request(&state, resolved_thread_id.as_deref()).await;
    match response {
        Ok(snapshot) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&compact_budget_snapshot_preview_payload(&snapshot))
                .unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

async fn client_budget_root_cause_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    let resolved_thread_id = resolved_request_thread_hint(&state, query.thread_id.as_deref()).await;
    let response: Result<Value> = async {
        if let Some(thread_id) = resolved_thread_id.as_deref() {
            let repo_root = discover_repo_root(None)?;
            if let Some(materialized) =
                try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                    &repo_root, thread_id,
                )
            {
                return Ok(materialized.surfaces.root_cause_payload);
            }
            refresh_client_live_meter_on_request(&state).await;
            let snapshot = thread_bound_snapshot_with_meta(&state, &thread_id).await?;
            return Ok(compact_client_budget_surfaces_from_snapshot(
                &repo_root,
                &snapshot,
                Some(thread_id),
            )
            .surfaces
            .root_cause_payload);
        }
        refresh_client_live_meter_on_request(&state).await;
        Ok(collect_compact_client_budget_surfaces(&state.cfg)
            .await?
            .root_cause_payload)
    }
    .await;
    match response {
        Ok(compact) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&compact).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

async fn client_budget_gate_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    let response: Result<Value> = async {
        if let Some(thread_id) = normalized_thread_id_hint(query.thread_id.as_deref()) {
            let repo_root = discover_repo_root(None)?;
            let now_epoch_ms = current_epoch_ms_u64();
            if let Some(cached) =
                load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms, Some(thread_id))
            {
                if let Some(launched_gate) = maybe_auto_launch_same_thread_host_control_from_gate(
                    &state.cfg,
                    &repo_root,
                    thread_id,
                    &cached.gate,
                )
                .await?
                {
                    return Ok(launched_gate);
                }
                return Ok(cached.gate);
            }
            if let Some(materialized) =
                try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                    &repo_root, thread_id,
                )
            {
                if let Some(launched_gate) = maybe_auto_launch_same_thread_host_control_from_gate(
                    &state.cfg,
                    &repo_root,
                    thread_id,
                    &materialized.gate.gate_payload,
                )
                .await?
                {
                    return Ok(launched_gate);
                }
                return Ok(materialized.gate.gate_payload);
            }
            refresh_client_live_meter_on_request(&state).await;
            let snapshot = thread_bound_snapshot_with_meta(&state, &thread_id).await?;
            let materialized = compact_client_budget_surfaces_from_snapshot(
                &repo_root,
                &snapshot,
                Some(thread_id),
            );
            if let Some(launched_gate) = maybe_auto_launch_same_thread_host_control_from_gate(
                &state.cfg,
                &repo_root,
                thread_id,
                &materialized.gate.gate_payload,
            )
            .await?
            {
                return Ok(launched_gate);
            }
            return Ok(materialized.gate.gate_payload);
        }
        refresh_client_live_meter_on_request(&state).await;
        let surfaces = collect_compact_client_budget_surfaces(&state.cfg).await?;
        compact_cli_client_budget_gate_from_root_cause_payload(&surfaces.root_cause_payload)
            .ok_or_else(|| anyhow!("compact client-budget root-cause payload missing gate"))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&normalize_front_door_client_budget_gate_payload_shape(
                payload,
            ))
            .unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

async fn client_limit_hourly_burn_api_handler(
    State(state): State<ObserveState>,
) -> impl IntoResponse {
    let response: Result<Value> = async {
        let db = postgres::connect_admin(&state.cfg).await?;
        postgres::bootstrap_schema(&db, &state.cfg).await?;
        token_budget::collect_default_client_limit_hourly_burn_surface(&db).await
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

fn normalize_agent_display_name_input(raw: &str) -> Result<String> {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(anyhow!("agent display_name must not be empty"));
    }
    if normalized.chars().count() > 120 {
        return Err(anyhow!("agent display_name must be at most 120 characters"));
    }
    Ok(normalized)
}

async fn agent_display_name_update_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<AgentDisplayNameUpdateRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let response: Result<Value> = async {
        let agent_scope = request.agent_scope.trim();
        if agent_scope.is_empty() {
            return Err(anyhow!("agent_scope is required"));
        }
        let display_name = normalize_agent_display_name_input(&request.display_name)?;
        let db = postgres::connect_admin(&state.cfg).await?;
        postgres::bootstrap_schema(&db, &state.cfg).await?;
        postgres::upsert_agent_display_name_by_code(&db, agent_scope, &display_name).await?;
        refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await?;
        let live_summary =
            dashboard_live_summary_payload_for_request(&state, query.thread_id.as_deref()).await?;
        Ok(json!({
            "status": "ok",
            "agent_display_name_update": {
                "agent_scope": agent_scope,
                "display_name": display_name,
            },
            "dashboard_live_summary": live_summary,
            "chat_notice": {
                "kind": "agent_display_name_updated",
                "thread_id": query.thread_id.clone(),
                "message_text": format!("Имя агента сохранено: {display_name}."),
                "agent_scope": agent_scope,
                "display_name": display_name,
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&json!({
                "status": "error",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

async fn client_budget_target_update_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetTargetUpdateRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let repo_root =
        match resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()).await {
            Ok(path) => path,
            Err(error) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&json!({
                        "status": "down",
                        "error": format!("{error:#}"),
                    }))
                    .unwrap_or_default(),
                )
                    .into_response();
            }
        };
    let response: Result<Value> = async {
        let args = ContinuityClientBudgetTargetArgs {
            project: request.project.clone(),
            repo_root: Some(repo_root),
            namespace: request.namespace.clone(),
            percent: request.percent,
            json: true,
        };
        let update = continuity::client_budget_target_payload(
            &state.cfg,
            &args,
            query.thread_id.as_deref(),
        )
        .await?;
        refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await?;
        let snapshot = if let Some(thread_id_hint) =
            resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
        {
            thread_bound_snapshot_with_meta(&state, &thread_id_hint).await?
        } else {
            cached_snapshot_with_meta(&state).await?
        };
        Ok(json!({
            "status": "ok",
            "client_budget_target_update": update["client_budget_target_update"].clone(),
            "client_budget_live": dashboard::client_budget_live_payload(&snapshot),
            "chat_notice": {
                "kind": "client_budget_target_changed",
                "thread_id": query.thread_id.clone(),
                "message_text": update["client_budget_target_update"]["operator_notice"]["message_text"].clone(),
                "reply_prefix": update["client_budget_target_update"]["client_budget_guard"]["reply_prefix"].clone(),
                "exact_chat_command": update["client_budget_target_update"]["operator_notice"]["exact_chat_command"].clone(),
                "target_percent": update["client_budget_target_update"]["target_percent"].clone(),
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "down",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

async fn client_budget_compact_chat_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetCompactChatRequest>,
) -> impl IntoResponse {
    let refresh_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = tokio::time::timeout(
            std::time::Duration::from_secs(6),
            refresh_client_live_meter_on_request(&refresh_state),
        )
        .await
        {
            eprintln!("client_budget_compact_chat preflight refresh timed out: {error:#}");
        }
    });
    let repo_root = match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()),
    )
    .await
    {
        Ok(Ok(path)) => path,
        Ok(Err(error)) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&json!({
                    "status": "down",
                    "error": format!("{error:#}"),
                }))
                .unwrap_or_default(),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&json!({
                    "status": "down",
                    "error": format!("client_budget_compact_chat repo_root timed out: {error:#}"),
                }))
                .unwrap_or_default(),
            )
                .into_response();
        }
    };
    let response: Result<Value> = async {
        let args = ContinuityCompactChatArgs {
            project: request.project.clone(),
            repo_root: Some(repo_root),
            namespace: request.namespace.clone(),
            headline: None,
            next_step: None,
            details_file: None,
            launch_host: request.launch_host,
            runtime_fallback: true,
            skip_handoff: !request.refresh_handoff,
            json: true,
        };
        let mut update = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            continuity::compact_chat_payload(&state.cfg, &args, query.thread_id.as_deref()),
        )
        .await
        .map_err(|_| anyhow!("client_budget_compact_chat timed out"))??;
        tokio::time::timeout(
            std::time::Duration::from_secs(6),
            continuity::maybe_launch_compact_chat_host(&mut update, request.launch_host, false),
        )
        .await
        .map_err(|_| anyhow!("client_budget_compact_chat launch timed out"))??;
        tokio::spawn({
            let cache = state.cache.clone();
            let cfg = state.cfg.clone();
            let bind = state.bind.clone();
            let refresh_ms = state.dashboard_refresh_ms;
            async move {
                if let Err(error) =
                    refresh_observe_cache(cache, cfg, bind, refresh_ms).await
                {
                    eprintln!("client_budget_compact_chat refresh failed: {error:#}");
                }
            }
        });
        let snapshot = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            cached_snapshot_with_meta(&state),
        )
        .await
        .map_err(|_| anyhow!("client_budget_compact_chat snapshot timed out"))??;
        let host_launch_status = update["continuity_compact_chat"]["host_launch"]["status"]
            .as_str()
            .unwrap_or("unknown");
        let notice_kind = match host_launch_status {
            "requested" => "client_budget_compact_chat_launch_requested",
            "bridge_unavailable" => "client_budget_compact_chat_bridge_unavailable",
            "launch_failed" => "client_budget_compact_chat_launch_failed",
            "available_not_requested" => "client_budget_compact_chat_launch_not_requested",
            _ => "client_budget_compact_chat_requested",
        };
        let compact_chat_summary = compact_chat_api_summary(&update["continuity_compact_chat"]);
        Ok(json!({
            "status": "ok",
            "continuity_compact_chat": compact_chat_summary,
            "client_budget_live": dashboard::client_budget_live_payload(&snapshot),
            "chat_notice": {
                "kind": notice_kind,
                "thread_id": query.thread_id.clone(),
                "message_text": update["continuity_compact_chat"]["operator_notice"]["message_text"].clone(),
                "reply_prefix": update["continuity_compact_chat"]["operator_notice"]["reply_prefix"].clone(),
                "exact_chat_command": update["continuity_compact_chat"]["operator_notice"]["exact_chat_command"].clone(),
                "prompt_text": update["continuity_compact_chat"]["chat_start_restore"]["prompt_text"].clone(),
                "prompt_file": update["continuity_compact_chat"]["operator_notice"]["prompt_file"].clone(),
                "client_surface": update["continuity_compact_chat"]["client_surface"].clone(),
                "required_host_action": update["continuity_compact_chat"]["operator_notice"]["required_host_action"].clone(),
                "note": update["continuity_compact_chat"]["operator_notice"]["note"].clone(),
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => {
            let message = format!("{error:#}");
            let status = if message.contains("timed out") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_REQUEST
            };
            let snapshot = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                cached_snapshot_with_meta(&state),
            )
            .await
            .ok()
            .and_then(|result| result.ok());
            let fallback_budget_live = snapshot.as_ref().map(dashboard::client_budget_live_payload);
            (
                status,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string(&json!({
                    "status": "error",
                    "error": message,
                    "client_budget_live": fallback_budget_live,
                }))
                .unwrap_or_default(),
            )
                .into_response()
        }
    }
}

fn compact_chat_api_summary(payload: &Value) -> Value {
    json!({
        "project": payload["project"].clone(),
        "namespace": payload["namespace"].clone(),
        "chat_start_restore": {
            "headline": payload["chat_start_restore"]["headline"].clone(),
            "next_step": payload["chat_start_restore"]["next_step"].clone(),
            "prompt_text": payload["chat_start_restore"]["prompt_text"].clone(),
        },
        "operator_notice": {
            "message_text": payload["operator_notice"]["message_text"].clone(),
            "reply_prefix": payload["operator_notice"]["reply_prefix"].clone(),
            "exact_chat_command": payload["operator_notice"]["exact_chat_command"].clone(),
            "prompt_file": payload["operator_notice"]["prompt_file"].clone(),
            "launch_clean_chat_command": payload["operator_notice"]["launch_clean_chat_command"].clone(),
            "launch_clean_chat_fallback_command": payload["operator_notice"]["launch_clean_chat_fallback_command"].clone(),
            "launch_clean_chat_command_kind": payload["operator_notice"]["launch_clean_chat_command_kind"].clone(),
            "manual_fallback_steps": payload["operator_notice"]["manual_fallback_steps"].clone(),
            "required_host_action": payload["operator_notice"]["required_host_action"].clone(),
            "note": payload["operator_notice"]["note"].clone(),
        },
        "client_surface": payload["client_surface"].clone(),
        "host_launch": payload["host_launch"].clone(),
    })
}

async fn resolve_continuity_project_and_namespace(
    db: &Client,
    repo_root_string: &str,
    project_code: Option<&str>,
    namespace_code: &str,
) -> Result<(postgres::ProjectRecord, postgres::NamespaceRecord)> {
    let project = if let Some(project_code) = project_code.map(str::trim) {
        if project_code.is_empty() {
            postgres::get_project_by_repo_root(db, repo_root_string).await?
        } else {
            let project = postgres::get_project_by_code(db, project_code).await?;
            if project.repo_root != repo_root_string {
                return Err(anyhow!(
                    "project {project_code} is not bound to repo_root {repo_root_string}"
                ));
            }
            project
        }
    } else {
        postgres::get_project_by_repo_root(db, repo_root_string).await?
    };
    let namespace = postgres::ensure_namespace(
        db,
        project.project_id,
        namespace_code,
        Some("Continuity"),
        "local_strict",
    )
    .await?;
    Ok((project, namespace))
}

fn observe_recent_thread_record_has_connected_model(
    thread: &codex_threads::RecentClientThreadRecord,
) -> bool {
    thread
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

fn observe_proof_like_runtime_marker(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            let lower = value.to_ascii_lowercase();
            value.starts_with("proof-")
                || value.starts_with("proof_")
                || value.starts_with("turn-proof-")
                || value.starts_with("turn_proof_")
                || value.contains("::proof_")
                || value.contains("::proof-")
                || lower.contains("proof_execctl_restore")
                || lower.contains("proof-execctl-restore")
                || lower.contains("execctl_restore_stress")
                || lower.contains("execctl restore stress")
        })
}

fn observe_user_visible_client_thread(thread: &codex_threads::RecentClientThreadRecord) -> bool {
    observe_recent_thread_record_has_connected_model(thread)
        && ![
            Some(thread.thread_id.as_str()),
            Some(thread.title.as_str()),
            thread.agent_nickname.as_deref(),
            thread.agent_role.as_deref(),
        ]
        .into_iter()
        .any(observe_proof_like_runtime_marker)
}

fn observe_thread_record_matches_repo_root(
    thread: &codex_threads::RecentClientThreadRecord,
    repo_root: &Path,
) -> bool {
    let repo_root = repo_root.display().to_string();
    thread.cwd == repo_root || thread.cwd.starts_with(&format!("{repo_root}/"))
}

fn evaluate_host_current_thread_control_window_targeting(
    repo_root: &Path,
    target_thread_id: &str,
    recent_threads: &[codex_threads::RecentClientThreadRecord],
) -> Value {
    let visible_threads = recent_threads
        .iter()
        .filter(|thread| observe_user_visible_client_thread(thread))
        .map(|thread| {
            json!({
                "thread_id": thread.thread_id,
                "cwd": thread.cwd,
                "title": thread.title,
                "model": thread.model,
                "updated_at_epoch_ms": thread.updated_at_epoch_s.saturating_mul(1000),
            })
        })
        .collect::<Vec<_>>();
    let visible_count = visible_threads.len();
    let target_thread = visible_threads.iter().find(|thread| {
        thread["thread_id"]
            .as_str()
            .map(str::trim)
            .is_some_and(|value| value == target_thread_id)
    });
    let target_visible = target_thread.is_some();
    let target_repo_root_match = target_thread.is_some_and(|thread| {
        let Some(thread_id) = thread["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return false;
        };
        let record = recent_threads
            .iter()
            .find(|candidate| candidate.thread_id == thread_id);
        record.is_some_and(|record| observe_thread_record_matches_repo_root(record, repo_root))
    });
    let denial_reason = if visible_count == 0 {
        Some("no_visible_model_bound_threads")
    } else if visible_count > 1 {
        Some("ambiguous_multi_window_recent_threads")
    } else if !target_visible {
        Some("target_thread_not_visible_in_recent_threads")
    } else if !target_repo_root_match {
        Some("target_thread_repo_root_mismatch")
    } else {
        None
    };
    json!({
        "status": if denial_reason.is_none() { "allowed" } else { "denied" },
        "allowed": denial_reason.is_none(),
        "target_thread_id": target_thread_id,
        "visible_model_bound_thread_count": visible_count,
        "recent_window_minutes": 30,
        "target_thread_visible": target_visible,
        "target_thread_repo_root_match": target_repo_root_match,
        "denial_reason": denial_reason,
        "visible_model_bound_threads": visible_threads,
    })
}

fn host_current_thread_control_window_targeting_summary(
    repo_root: &Path,
    target_thread_id: &str,
) -> Result<Value> {
    let recent_threads = codex_threads::recent_client_thread_records(30 * 60)?;
    Ok(evaluate_host_current_thread_control_window_targeting(
        repo_root,
        target_thread_id,
        &recent_threads,
    ))
}

async fn execute_host_current_thread_control_launch(surface: &Value) -> Result<Value> {
    let external_launch = surface["external_uri_launch"]
        .as_object()
        .ok_or_else(|| anyhow!("host current-thread control surface missing external launch"))?;
    let uri = external_launch
        .get("uri")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("host current-thread control launch uri is unavailable"))?;
    if !external_launch
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "host current-thread control external launch surface is unavailable"
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            ProcessCommand::new("xdg-open").arg(uri).output(),
        )
        .await
        .context("timed out waiting for xdg-open to return")?
        .context("failed to run xdg-open for same-thread host control")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let status = output
                .status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "terminated-by-signal".to_string());
            let detail = if stderr.is_empty() {
                format!("xdg-open exited with status {status}")
            } else {
                format!("xdg-open exited with status {status}: {stderr}")
            };
            return Err(anyhow!(detail));
        }
        return Ok(json!({
            "launched": true,
            "launch_method": "xdg_open",
            "uri": uri,
            "exit_status": output.status.code(),
            "verification_state": "launch_command_executed_exit_zero",
        }));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = uri;
        Err(anyhow!(
            "server-side same-thread overlay launch is unavailable on this platform"
        ))
    }
}

async fn continuity_handoff_api_handler(
    State(state): State<ObserveState>,
    Json(request): Json<ContinuityHandoffRequest>,
) -> impl IntoResponse {
    let response: Result<Value> = async {
        let project_code = request
            .project
            .clone()
            .ok_or_else(|| anyhow!("project is required for continuity handoff API"))?;
        let mut db = postgres::connect_admin(&state.cfg).await?;
        let payload = continuity::handoff_payload_from_parts_with_db(
            &mut db,
            &state.cfg,
            &project_code,
            &request.namespace,
            &request.headline,
            &request.next_step,
            request.details.as_deref().unwrap_or_default(),
            request.resolve_current_goal,
            &request.resolved_headlines,
            &request.resolved_task_ids,
        )
        .await?;
        Ok(json!({
            "status": "ok",
            "continuity_handoff": payload["continuity_handoff"].clone(),
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "down",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

pub async fn client_budget_host_control_launch_payload(
    cfg: &AppConfig,
    args: &ObserveClientBudgetHostControlLaunchArgs,
) -> Result<Value> {
    let repo_root = match args.repo_root.as_deref() {
        Some(path) => path.to_path_buf(),
        None => resolve_request_repo_root_for_project(cfg, args.project.as_deref()).await?,
    };
    let thread_id = args.thread_id.trim();
    if thread_id.is_empty() {
        return Err(anyhow!(
            "thread_id is required for same-thread host control launch"
        ));
    }
    let command_id = if args.compact_window {
        Some(working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID)
    } else {
        args.command_id.as_deref()
    };
    let launch_targeting =
        host_current_thread_control_window_targeting_summary(&repo_root, thread_id)?;
    if !launch_targeting["allowed"].as_bool().unwrap_or(false) {
        let denial_reason = launch_targeting["denial_reason"]
            .as_str()
            .unwrap_or("ambiguous_host_launch_target");
        return Err(anyhow!(
            "same-thread host control launch refused: {denial_reason}"
        ));
    }
    let surface = working_state::build_host_current_thread_control_surface_for_thread_and_command(
        Some(thread_id),
        command_id,
    );
    let launch = execute_host_current_thread_control_launch(&surface).await?;
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let repo_root_string = repo_root.display().to_string();
    let (project, namespace) = resolve_continuity_project_and_namespace(
        &db,
        &repo_root_string,
        args.project.as_deref(),
        &args.namespace,
    )
    .await?;
    let command_id = surface["command_id"]
        .as_str()
        .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID);
    let launched_feedback_kind = working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED;
    working_state::record_host_current_thread_control_feedback_with_thread_hint(
        &db,
        &project,
        &namespace,
        launched_feedback_kind,
        Some(command_id),
        Some(thread_id),
    )
    .await?;
    let _ = write_shared_thread_bound_snapshot_invalidation(&repo_root, thread_id);
    let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
    if let Ok(snapshot) = collect_client_budget_snapshot_from_db(
        &db,
        &repo_root,
        Some(thread_id),
        None,
        restore.as_ref(),
    )
    .await
    {
        materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
            &repo_root, thread_id, &snapshot,
        );
    }
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
    let client_budget_reply_gate =
        compact_host_control_client_budget_reply_gate(&client_budget_guard);
    let message_text = surface["external_uri_launch"]["observe_api_launch_summary"]
        .as_str()
        .or_else(|| surface["requested_message_text"].as_str())
        .unwrap_or("Запрошен same-thread host control.");
    Ok(json!({
        "status": "ok",
        "client_budget_host_control_launch": {
            "project": {
                "code": project.code.clone(),
                "display_name": project.display_name.clone(),
                "repo_root": project.repo_root.clone(),
            },
            "namespace": {
                "code": namespace.code.clone(),
                "display_name": namespace.display_name.clone(),
            },
            "thread_id": thread_id,
            "command_id": command_id,
            "launch_targeting": launch_targeting,
            "host_current_thread_control": surface,
            "launch": launch,
            "client_budget_reply_gate": client_budget_reply_gate,
            "operator_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "message_text": message_text,
                "feedback_kind": launched_feedback_kind,
                "command_id": command_id,
                "thread_id": thread_id,
            }
        }
    }))
}

pub async fn print_client_budget_host_control_launch(
    cfg: &AppConfig,
    args: &ObserveClientBudgetHostControlLaunchArgs,
) -> Result<()> {
    let payload = client_budget_host_control_launch_payload(cfg, args).await?;
    println!("{}", serde_json::to_string(&payload)?);
    Ok(())
}

async fn client_budget_host_control_launch_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetHostControlLaunchRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let repo_root =
        match resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()).await {
            Ok(path) => path,
            Err(error) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&json!({
                        "status": "down",
                        "error": format!("{error:#}"),
                    }))
                    .unwrap_or_default(),
                )
                    .into_response();
            }
        };
    let response: Result<Value> = async {
        let thread_id = query
            .thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("thread_id is required for same-thread host control launch"))?;
        let payload = client_budget_host_control_launch_payload(
            &state.cfg,
            &ObserveClientBudgetHostControlLaunchArgs {
                thread_id: thread_id.to_string(),
                compact_window: false,
                command_id: request.command_id.clone(),
                project: request.project.clone(),
                repo_root: Some(repo_root.clone()),
                namespace: request.namespace.clone(),
            },
        )
        .await?;
        let message_text = payload["client_budget_host_control_launch"]["operator_notice"]
            ["message_text"]
            .as_str()
            .unwrap_or("Запрошен same-thread host control.");
        let launch_summary =
            client_budget_host_control_launch_api_summary(&payload["client_budget_host_control_launch"]);
        Ok(json!({
            "status": "ok",
            "client_budget_host_control_launch": launch_summary,
            "chat_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "thread_id": query.thread_id.clone(),
                "message_text": message_text,
                "reply_prefix":
                    payload["client_budget_host_control_launch"]["client_budget_reply_gate"]["reply_prefix"].clone(),
                "feedback_kind": working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED,
                "command_id":
                    payload["client_budget_host_control_launch"]["command_id"].clone(),
                "thread_id_hint": thread_id,
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&json!({
                "status": "error",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

fn client_budget_host_control_launch_api_summary(payload: &Value) -> Value {
    json!({
        "project": payload["project"].clone(),
        "namespace": payload["namespace"].clone(),
        "thread_id": payload["thread_id"].clone(),
        "command_id": payload["command_id"].clone(),
        "launch_targeting": {
            "status": payload["launch_targeting"]["status"].clone(),
            "summary": payload["launch_targeting"]["summary"].clone(),
        },
        "client_budget_reply_gate": {
            "reply_prefix": payload["client_budget_reply_gate"]["reply_prefix"].clone(),
        },
        "operator_notice": payload["operator_notice"].clone(),
    })
}

async fn client_budget_host_control_feedback_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetHostControlFeedbackRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let repo_root =
        match resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()).await {
            Ok(path) => path,
            Err(error) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&json!({
                        "status": "down",
                        "error": format!("{error:#}"),
                    }))
                    .unwrap_or_default(),
                )
                    .into_response();
            }
        };
    let response: Result<Value> = async {
        let db = postgres::connect_admin(&state.cfg).await?;
        postgres::bootstrap_schema(&db, &state.cfg).await?;
        let repo_root_string = repo_root.display().to_string();
        let (project, namespace) = resolve_continuity_project_and_namespace(
            &db,
            &repo_root_string,
            request.project.as_deref(),
            &request.namespace,
        )
        .await?;
        let feedback_kind = working_state::normalize_host_current_thread_control_feedback_kind(
            &request.feedback_kind,
        )
        .ok_or_else(|| {
            anyhow!("host current-thread control feedback must be one of requested, opened, failed")
        })?;
        let command_id = working_state::normalize_host_current_thread_control_command_id(
            request
                .command_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        );
        working_state::record_host_current_thread_control_feedback_with_thread_hint(
            &db,
            &project,
            &namespace,
            feedback_kind,
            Some(command_id),
            query.thread_id.as_deref(),
        )
        .await?;
        if let Some(thread_id) = query
            .thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let _ = write_shared_thread_bound_snapshot_invalidation(&repo_root, thread_id);
            let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
            if let Ok(snapshot) = collect_client_budget_snapshot_from_db(
                &db,
                &repo_root,
                Some(thread_id),
                None,
                restore.as_ref(),
            )
            .await
            {
                materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
                    &repo_root, thread_id, &snapshot,
                );
            }
            let client_budget_guard =
                token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref())
                    .await?;
            let client_budget_reply_gate =
                compact_host_control_client_budget_reply_gate(&client_budget_guard);
            let message_text =
                working_state::host_current_thread_control_feedback_notice_text_for_command(
                    feedback_kind,
                    Some(command_id),
                );
            return Ok(json!({
                "status": "ok",
                "client_budget_host_control_feedback": {
                    "project": {
                        "code": project.code.clone(),
                        "display_name": project.display_name.clone(),
                        "repo_root": project.repo_root.clone(),
                    },
                    "namespace": {
                        "code": namespace.code.clone(),
                        "display_name": namespace.display_name.clone(),
                    },
                    "thread_id": thread_id,
                    "command_id": command_id,
                    "feedback_kind": feedback_kind,
                    "message_text": message_text,
                    "client_budget_reply_gate": client_budget_reply_gate,
                }
            }));
        }
        let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
        let client_budget_guard =
            token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
        let client_budget_reply_gate =
            compact_host_control_client_budget_reply_gate(&client_budget_guard);
        let message_text =
            working_state::host_current_thread_control_feedback_notice_text_for_command(
                feedback_kind,
                Some(command_id),
            );
        Ok(json!({
            "status": "ok",
            "client_budget_host_control_feedback": {
                "project": {
                    "code": project.code.clone(),
                    "display_name": project.display_name.clone(),
                    "repo_root": project.repo_root.clone(),
                },
                "namespace": {
                    "code": namespace.code.clone(),
                    "display_name": namespace.display_name.clone(),
                },
                "feedback_kind": feedback_kind,
                "command_id": command_id,
                "client_budget_reply_gate": client_budget_reply_gate.clone(),
                "operator_notice": {
                    "kind": format!("host_current_thread_control_feedback_{feedback_kind}"),
                    "message_text": message_text,
                    "feedback_kind": feedback_kind,
                    "command_id": command_id,
                }
            },
            "chat_notice": {
                "kind": format!("host_current_thread_control_feedback_{feedback_kind}"),
                "thread_id": query.thread_id.clone(),
                "message_text": message_text,
                "reply_prefix": client_budget_reply_gate["reply_prefix"].clone(),
                "feedback_kind": feedback_kind,
                "command_id": command_id,
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "error",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

async fn snapshot_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    spawn_client_live_meter_refresh(&state).await;
    let response = if let Some(thread_id_hint) =
        resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
    {
        merged_thread_bound_snapshot_with_meta(&state, &thread_id_hint).await
    } else {
        live_active_agent_snapshot_for_request(&state).await
    };
    match response {
        Ok(snapshot) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&snapshot).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

async fn healthz_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    if let Err(error) = maybe_refresh_stale_observe_cache_for_healthz(&state).await {
        eprintln!("observe healthz refresh recovery failed: {error:#}");
    }
    let snapshot = cached_snapshot_with_meta(&state).await;
    match snapshot {
        Ok(snapshot) => {
            let summary = &snapshot["sla"]["summary"];
            let critical = summary["critical"].as_u64().unwrap_or(0);
            let unknown = summary["unknown"].as_u64().unwrap_or(0);
            let cache_stale = snapshot["observe_cache"]["stale"].as_bool().unwrap_or(true);
            let status = if critical == 0 && unknown == 0 && !cache_stale {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            let status_label = if status == StatusCode::OK {
                "up"
            } else {
                "down"
            };
            let headers = no_store_headers("application/json; charset=utf-8");
            let payload = json!({
                "status": status_label,
                "critical": critical,
                "unknown": unknown,
                "cache_stale": cache_stale,
                "refresh_in_progress": snapshot["observe_cache"]["refresh_in_progress"].clone(),
                "snapshot_age_ms": snapshot["observe_cache"]["snapshot_age_ms"].clone(),
                "last_refresh_completed_epoch_ms": snapshot["observe_cache"]["last_refresh_completed_epoch_ms"].clone(),
                "last_error": snapshot["observe_cache"]["last_error"].clone(),
            });
            (
                status,
                headers,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

fn no_store_headers(content_type: &'static str) -> [(header::HeaderName, HeaderValue); 4] {
    [
        (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
        ),
        (header::PRAGMA, HeaderValue::from_static("no-cache")),
        (header::EXPIRES, HeaderValue::from_static("0")),
    ]
}

async fn mark_observe_http_activity(state: &ObserveState) {
    let mut cache = state.cache.write().await;
    cache.last_http_request_epoch_ms = Some(now_epoch_ms());
}

async fn observe_recent_http_activity(cache: Arc<RwLock<ObserveCache>>) -> bool {
    let cache = cache.read().await;
    let Some(last_http_request_epoch_ms) = cache.last_http_request_epoch_ms else {
        return false;
    };
    now_epoch_ms().saturating_sub(last_http_request_epoch_ms)
        <= OBSERVE_BACKGROUND_REFRESH_IDLE_GRACE_MS
}

async fn refresh_observe_cache(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: AppConfig,
    bind: String,
    refresh_ms: u64,
) -> Result<()> {
    let started_epoch_ms = now_epoch_ms();
    {
        let mut state = cache.write().await;
        if state.refresh_in_progress {
            if !observe_refresh_stuck(&state) {
                return Ok(());
            }
            state.refresh_in_progress = false;
            state.last_error = Some(
                "previous observe refresh was declared stuck and recovered by watchdog".to_string(),
            );
        }
        state.last_refresh_started_epoch_ms = Some(started_epoch_ms);
        state.refresh_in_progress = true;
    }
    let cache_clone = cache.clone();
    let cfg_clone = cfg.clone();
    let bind_clone = bind.clone();
    let refresh_task = tokio::spawn(async move {
        let started = Instant::now();
        let result =
            match tokio::time::timeout(Duration::from_millis(OBSERVE_REFRESH_TIMEOUT_MS), async {
                build_snapshot(&cfg_clone, false)
                    .await
                    .and_then(|snapshot| {
                        dashboard::build_payload(&cfg_clone, &snapshot, &bind_clone, refresh_ms)
                            .map(|payload| (snapshot, payload))
                    })
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow!(
                    "observe refresh exceeded timeout of {} ms",
                    OBSERVE_REFRESH_TIMEOUT_MS
                )),
            };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let completed_epoch_ms = now_epoch_ms();
        let mut thread_id_to_prewarm = None;
        let outcome = {
            let mut state = cache_clone.write().await;
            state.refresh_in_progress = false;
            match result {
                Ok((snapshot, payload)) => {
                    state.last_refresh_completed_epoch_ms = Some(completed_epoch_ms);
                    state.last_refresh_duration_ms = Some(elapsed_ms);
                    state.snapshot = Some(snapshot);
                    state.dashboard_payload = Some(payload);
                    state.last_error = None;
                    thread_id_to_prewarm = match state.snapshot.as_ref() {
                        Some(value) => strict_auto_thread_binding_hint_from_snapshot(value.clone()),
                        None => None,
                    };
                    Ok(())
                }
                Err(error) => {
                    state.last_refresh_duration_ms = Some(elapsed_ms);
                    state.last_error = Some(format!("{error:#}"));
                    Err(error)
                }
            }
        };
        if let Some(thread_id) = thread_id_to_prewarm {
            if let Err(error) = prewarm_thread_bound_client_budget_surfaces_for_thread(
                cache_clone,
                &cfg_clone,
                &thread_id,
            )
            .await
            {
                eprintln!("refresh-triggered active thread prewarm failed: {error:#}");
            }
        }
        outcome
    });
    match refresh_task.await {
        Ok(outcome) => outcome,
        Err(error) => {
            let error_message = format!("{error:#}");
            let error_message_for_task = error_message.clone();
            let cache_cleanup = cache.clone();
            tokio::spawn(async move {
                let mut state = cache_cleanup.write().await;
                if state.refresh_in_progress {
                    state.refresh_in_progress = false;
                    state.last_error = Some(format!(
                        "observe refresh task aborted before completion: {error_message_for_task}"
                    ));
                }
            });
            Err(anyhow!("observe refresh task aborted: {error_message}"))
        }
    }
}

async fn maybe_refresh_stale_observe_cache_for_healthz(state: &ObserveState) -> Result<()> {
    let should_refresh = {
        let cache = state.cache.read().await;
        let cache_stale = observe_cache_stale(&cache, state.dashboard_refresh_ms);
        let refresh_stuck = observe_refresh_stuck(&cache);
        cache_stale && (!cache.refresh_in_progress || refresh_stuck)
    };
    if !should_refresh {
        return Ok(());
    }
    refresh_observe_cache(
        state.cache.clone(),
        state.cfg.clone(),
        state.bind.clone(),
        state.dashboard_refresh_ms,
    )
    .await
}

async fn cached_dashboard_payload(state: &ObserveState) -> Result<Value> {
    let cache = state.cache.read().await;
    let mut payload = cache
        .dashboard_payload
        .clone()
        .ok_or_else(|| anyhow!("dashboard cache not ready"))?;
    if payload.get("client_budget_live").is_none() {
        if let Some(snapshot) = cache.snapshot.as_ref() {
            if let Some(root) = payload.as_object_mut() {
                root.insert(
                    "client_budget_live".to_string(),
                    dashboard::client_budget_live_payload(snapshot),
                );
            }
        }
    }
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn cached_snapshot_with_meta(state: &ObserveState) -> Result<Value> {
    let cache = state.cache.read().await;
    let snapshot = cache
        .snapshot
        .clone()
        .ok_or_else(|| anyhow!("snapshot cache not ready"))?;
    Ok(attach_observe_cache_to_snapshot(
        snapshot,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn live_active_agent_snapshot_for_request(state: &ObserveState) -> Result<Value> {
    let snapshot = if let Some(thread_id) = auto_thread_binding_hint_from_cache(state).await {
        merged_thread_bound_snapshot_with_meta(state, &thread_id).await?
    } else {
        cached_snapshot_with_meta(state).await?
    };
    let cached_activity = snapshot["agent_scope_activity"].clone();
    if !cached_activity.is_object() {
        return Ok(snapshot);
    }
    let db = postgres::connect_admin(&state.cfg).await?;
    postgres::bootstrap_schema(&db, &state.cfg).await?;
    let repo_root = discover_repo_root(None)?;
    let active_agent_budget =
        token_budget::collect_active_agent_live_budget_surface(&db, &repo_root, &cached_activity)
            .await?;
    Ok(overlay_live_active_agent_surfaces(
        snapshot,
        cached_activity,
        active_agent_budget,
    ))
}

fn active_agent_budget_card_payload_from_snapshot(snapshot: &Value) -> Result<Value> {
    let surface = &snapshot["active_agent_budget"];
    let card = dashboard::build_active_agent_budget_session_card_from_surface(surface)
        .ok_or_else(|| anyhow!("active agent budget card not ready"))?;
    Ok(json!({
        "card": card,
        "captured_at_epoch_ms": surface["captured_at_epoch_ms"].clone(),
        "source": surface["source"].clone(),
    }))
}

async fn live_active_agent_budget_card_payload(state: &ObserveState) -> Result<Value> {
    let snapshot = live_dashboard_summary_snapshot_for_request(state, None).await?;
    active_agent_budget_card_payload_from_snapshot(&snapshot)
}

async fn live_active_agent_dashboard_payload(state: &ObserveState) -> Result<Value> {
    let snapshot = live_active_agent_snapshot_for_request(state).await?;
    let mut payload = dashboard::build_payload(
        &state.cfg,
        &snapshot,
        &state.bind,
        state.dashboard_refresh_ms,
    )?;
    if payload.get("client_budget_live").is_none() {
        if let Some(root) = payload.as_object_mut() {
            root.insert(
                "client_budget_live".to_string(),
                dashboard::client_budget_live_payload(&snapshot),
            );
        }
    }
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn dashboard_live_summary_payload_for_request(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Result<Value> {
    let resolved_thread_id = resolved_request_thread_hint(state, explicit_thread_id).await;
    {
        let cache = state.cache.read().await;
        let cache_age_ms = cache
            .dashboard_live_summary_completed_epoch_ms
            .map(|completed_at| now_epoch_ms().saturating_sub(completed_at));
        if cache.dashboard_live_summary_refresh_in_progress {
            if let Some(payload) = cache.dashboard_live_summary_payload.clone() {
                return Ok(attach_observe_cache_to_dashboard_payload(
                    payload,
                    &cache,
                    state.dashboard_refresh_ms,
                ));
            }
            return Ok(dashboard_live_summary_warmup_payload(
                cache_snapshot_age_ms(&cache),
            ));
        }
        if let Some(payload) = cache.dashboard_live_summary_payload.clone() {
            let cache_stale = cache_age_ms
                .map(|age_ms| age_ms > DASHBOARD_LIVE_SUMMARY_CACHE_TTL_MS)
                .unwrap_or(true);
            let refresh_needed = cache_stale && !cache.dashboard_live_summary_refresh_in_progress;
            drop(cache);
            if refresh_needed {
                spawn_dashboard_live_summary_refresh(state, resolved_thread_id.clone()).await;
            }
            let cache = state.cache.read().await;
            return Ok(attach_observe_cache_to_dashboard_payload(
                payload,
                &cache,
                state.dashboard_refresh_ms,
            ));
        }
    }

    let payload = refresh_dashboard_live_summary_cache(state, resolved_thread_id.clone()).await?;
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn spawn_dashboard_live_summary_refresh(
    state: &ObserveState,
    resolved_thread_id: Option<String>,
) {
    {
        let mut cache = state.cache.write().await;
        if cache.dashboard_live_summary_refresh_in_progress {
            return;
        }
        cache.dashboard_live_summary_refresh_in_progress = true;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = refresh_dashboard_live_summary_cache(&state, resolved_thread_id).await {
            eprintln!("dashboard live summary refresh failed: {error:#}");
            let mut cache = state.cache.write().await;
            cache.dashboard_live_summary_refresh_in_progress = false;
        }
    });
}

async fn refresh_dashboard_live_summary_cache(
    state: &ObserveState,
    resolved_thread_id: Option<String>,
) -> Result<Value> {
    let snapshot =
        live_dashboard_summary_snapshot_for_request(state, resolved_thread_id.as_deref()).await?;
    let payload = dashboard::build_live_summary_payload(
        &state.cfg,
        &snapshot,
        &state.bind,
        state.dashboard_refresh_ms,
    )?;
    let mut cache = state.cache.write().await;
    cache.dashboard_live_summary_payload = Some(payload.clone());
    cache.dashboard_live_summary_thread_id = resolved_thread_id;
    cache.dashboard_live_summary_completed_epoch_ms = Some(now_epoch_ms());
    cache.dashboard_live_summary_refresh_in_progress = false;
    Ok(payload)
}

fn overlay_live_active_agent_surfaces(
    mut snapshot: Value,
    activity: Value,
    active_agent_budget: Value,
) -> Value {
    if let Some(root) = snapshot.as_object_mut() {
        root.insert("agent_scope_activity".to_string(), activity);
        root.insert("active_agent_budget".to_string(), active_agent_budget);
    }
    snapshot
}

fn overlay_dashboard_live_summary_surfaces(
    mut snapshot: Value,
    activity: Value,
    latest_repo_working_state_restore: Value,
    active_agent_budget: Value,
) -> Value {
    if let Some(root) = snapshot.as_object_mut() {
        root.insert("agent_scope_activity".to_string(), activity);
        root.insert(
            "latest_repo_working_state_restore".to_string(),
            latest_repo_working_state_restore,
        );
        root.insert("active_agent_budget".to_string(), active_agent_budget);
    }
    snapshot
}

async fn live_dashboard_summary_snapshot_for_request(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Result<Value> {
    let base_snapshot =
        if let Some(thread_id) = resolved_request_thread_hint(state, explicit_thread_id).await {
            merged_thread_bound_snapshot_with_meta(state, &thread_id).await?
        } else {
            cached_snapshot_with_meta(state).await?
        };
    let db = postgres::connect_admin(&state.cfg).await?;
    postgres::bootstrap_schema(&db, &state.cfg).await?;
    let repo_root = discover_repo_root(None)?;
    let agent_scope_activity = token_budget::collect_agent_scope_activity(&db).await?;
    let active_agent_budget = token_budget::collect_active_agent_live_budget_surface(
        &db,
        &repo_root,
        &agent_scope_activity,
    )
    .await?;
    let latest_repo_working_state_restore =
        latest_repo_working_state_restore_payload(&db, &repo_root)
            .await?
            .unwrap_or_else(|| json!({ "working_state_restore": {} }));
    Ok(overlay_dashboard_live_summary_surfaces(
        base_snapshot,
        agent_scope_activity,
        latest_repo_working_state_restore,
        active_agent_budget,
    ))
}

fn normalized_thread_id_hint(thread_id: Option<&str>) -> Option<&str> {
    thread_id.map(str::trim).filter(|value| !value.is_empty())
}

async fn resolved_request_thread_hint(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Option<String> {
    if let Some(thread_id) = normalized_thread_id_hint(explicit_thread_id) {
        Some(thread_id.to_string())
    } else {
        auto_thread_binding_hint_from_cache(state).await
    }
}

async fn auto_thread_binding_hint_from_cache(state: &ObserveState) -> Option<String> {
    let cache = state.cache.read().await;
    let snapshot = cache.snapshot.clone()?;
    strict_auto_thread_binding_hint_from_snapshot(snapshot)
}

fn strict_auto_thread_binding_hint_from_snapshot(snapshot: Value) -> Option<String> {
    strict_auto_thread_binding_hint_from_agent_scope_activity(
        snapshot["agent_scope_activity"].clone(),
    )
}

fn strict_auto_thread_binding_hint_from_agent_scope_activity(activity: Value) -> Option<String> {
    let recent_thread_ids = activity["client_recent_threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item["thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    if recent_thread_ids.is_empty() {
        return None;
    }

    let active_thread_ids = activity["active_now_scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item["owner_thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    if active_thread_ids.len() != 1 {
        return None;
    }

    let thread_id = active_thread_ids.into_iter().next()?;
    recent_thread_ids.contains(&thread_id).then_some(thread_id)
}

async fn thread_bound_dashboard_payload(state: &ObserveState, thread_id: &str) -> Result<Value> {
    let snapshot = merged_thread_bound_snapshot_with_meta(state, thread_id).await?;
    let mut payload = dashboard::build_payload(
        &state.cfg,
        &snapshot,
        &state.bind,
        state.dashboard_refresh_ms,
    )?;
    if payload.get("client_budget_live").is_none() {
        if let Some(root) = payload.as_object_mut() {
            root.insert(
                "client_budget_live".to_string(),
                dashboard::client_budget_live_payload(&snapshot),
            );
        }
    }
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn merged_thread_bound_snapshot_with_meta(
    state: &ObserveState,
    thread_id: &str,
) -> Result<Value> {
    let thread_bound_snapshot = thread_bound_snapshot_with_meta(state, thread_id).await?;
    let base_snapshot = match cached_snapshot_with_meta(state).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            refresh_observe_cache(
                state.cache.clone(),
                state.cfg.clone(),
                state.bind.clone(),
                state.dashboard_refresh_ms,
            )
            .await?;
            cached_snapshot_with_meta(state).await?
        }
    };
    let snapshot = merge_thread_bound_client_budget_snapshot_into_base_snapshot(
        &base_snapshot,
        &thread_bound_snapshot,
    );
    Ok(snapshot)
}

fn merge_thread_bound_client_budget_snapshot_into_base_snapshot(
    base_snapshot: &Value,
    thread_bound_snapshot: &Value,
) -> Value {
    let mut merged = base_snapshot.clone();
    let token_budget_report = &thread_bound_snapshot["token_budget_report"];
    if token_budget_report.is_object() {
        let base_outer = &base_snapshot["token_budget_report"];
        if base_outer.is_object()
            && base_outer["token_budget_report"].is_object()
            && token_budget_report["token_budget_report"].is_object()
        {
            let mut merged_outer = base_outer.clone();
            let mut merged_inner = base_outer["token_budget_report"].clone();
            if let Some(merged_inner_object) = merged_inner.as_object_mut() {
                for key in [
                    "current_session",
                    "live_response_latency",
                    "client_live_meter",
                    "personal_agent_kpi",
                    "client_limit_hourly_burn",
                    "current_live_turn",
                    "client_budget_target_percent",
                    "filters",
                ] {
                    if token_budget_report["token_budget_report"][key].is_null() {
                        continue;
                    }
                    if key == "live_response_latency"
                        && should_keep_base_live_response_latency(
                            &base_outer["token_budget_report"][key],
                            &token_budget_report["token_budget_report"][key],
                        )
                    {
                        continue;
                    }
                    merged_inner_object.insert(
                        key.to_string(),
                        token_budget_report["token_budget_report"][key].clone(),
                    );
                }
                if base_outer["token_budget_report"]["statement_previews"].is_object()
                    || token_budget_report["token_budget_report"]["statement_previews"].is_object()
                {
                    let mut merged_statement_previews =
                        base_outer["token_budget_report"]["statement_previews"].clone();
                    if let Some(statement_previews_object) =
                        merged_statement_previews.as_object_mut()
                    {
                        if token_budget_report["token_budget_report"]["statement_previews"]
                            ["current_session"]
                            .is_object()
                        {
                            statement_previews_object.insert(
                                "current_session".to_string(),
                                token_budget_report["token_budget_report"]["statement_previews"]
                                    ["current_session"]
                                    .clone(),
                            );
                        }
                    }
                    merged_inner_object
                        .insert("statement_previews".to_string(), merged_statement_previews);
                }
            }
            if let Some(merged_outer_object) = merged_outer.as_object_mut() {
                merged_outer_object.insert("token_budget_report".to_string(), merged_inner);
            }
            merged["token_budget_report"] = merged_outer;
        } else {
            merged["token_budget_report"] = token_budget_report.clone();
        }
    }
    let latest_repo_restore = &thread_bound_snapshot["latest_repo_working_state_restore"];
    if latest_repo_restore.is_object() {
        merged["latest_repo_working_state_restore"] = latest_repo_restore.clone();
    }
    merged
}

fn live_response_latency_total_sample_count(surface: &Value) -> u64 {
    ["current_session", "rolling_window"]
        .into_iter()
        .filter_map(|scope| surface[scope]["sample_count"].as_u64())
        .sum()
}

fn should_keep_base_live_response_latency(base_surface: &Value, thread_surface: &Value) -> bool {
    let base_count = live_response_latency_total_sample_count(base_surface);
    let thread_count = live_response_latency_total_sample_count(thread_surface);
    base_count > 0 && thread_count == 0
}

async fn thread_bound_snapshot_with_meta(state: &ObserveState, thread_id: &str) -> Result<Value> {
    let repo_root = discover_repo_root(None)?;
    let _ = write_shared_active_thread_hint(&repo_root, thread_id);
    if let Some(snapshot) = cached_thread_bound_snapshot_with_meta(state, thread_id).await {
        return Ok(snapshot);
    }
    let (latest_repo_restore_override, base_report_override) = {
        let cache = state.cache.read().await;
        (
            cached_latest_repo_working_state_restore_snapshot(&cache),
            cached_token_budget_report_snapshot(&cache),
        )
    };
    let snapshot = collect_client_budget_snapshot_with_thread_hint(
        &state.cfg,
        &repo_root,
        Some(thread_id),
        base_report_override.as_ref(),
        latest_repo_restore_override.as_ref(),
    )
    .await?;
    let _ = write_shared_thread_bound_budget_snapshot(&repo_root, thread_id, &snapshot);
    let cached_snapshot = {
        let mut cache = state.cache.write().await;
        cache.thread_bound_snapshot = Some(snapshot);
        cache.thread_bound_snapshot_thread_id = Some(thread_id.to_string());
        cache.thread_bound_snapshot_completed_epoch_ms = Some(now_epoch_ms());
        cache.thread_bound_snapshot.clone().unwrap_or(Value::Null)
    };
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_snapshot(
        cached_snapshot,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

fn materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
    repo_root: &Path,
    thread_id: &str,
    snapshot: &Value,
) {
    let _ = write_shared_thread_bound_budget_snapshot(repo_root, thread_id, snapshot);
    let guard = dashboard::current_session_budget_guard(snapshot);
    let root_cause_payload =
        dashboard::client_budget_root_cause_payload_with_guard(snapshot, &guard);
    let compact_root_cause =
        compact_client_budget_root_cause_payload(&root_cause_payload, Some(&guard));
    let compact_gate =
        front_door_client_budget_gate_payload(compact_cli_client_budget_gate_payload(&guard));
    let compact_guard = compact_current_session_budget_guard_payload(&guard);
    let surfaces_cache = build_compact_client_budget_surfaces_cache(
        &compact_root_cause,
        &compact_gate,
        &compact_guard,
        Some(thread_id),
    );
    let _ =
        write_shared_compact_client_budget_surfaces(repo_root, Some(thread_id), &surfaces_cache);
    let gate_cache =
        build_compact_client_budget_gate_cache(&compact_gate, &compact_guard, Some(thread_id));
    let _ = write_shared_compact_client_budget_gate(repo_root, Some(thread_id), &gate_cache);
}

async fn populate_thread_bound_client_budget_surfaces_from_snapshot(
    cache: Arc<RwLock<ObserveCache>>,
    repo_root: &Path,
    thread_id: &str,
    snapshot: Value,
) {
    materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
        repo_root, thread_id, &snapshot,
    );

    let completed_epoch_ms = now_epoch_ms();
    let mut state = cache.write().await;
    state.thread_bound_snapshot = Some(snapshot);
    state.thread_bound_snapshot_thread_id = Some(thread_id.to_string());
    state.thread_bound_snapshot_completed_epoch_ms = Some(completed_epoch_ms);
}

async fn cached_thread_bound_snapshot_with_meta(
    state: &ObserveState,
    thread_id: &str,
) -> Option<Value> {
    let repo_root = discover_repo_root(None).ok()?;
    {
        let cache = state.cache.read().await;
        if let (Some(cached_thread_id), Some(completed_at), Some(snapshot)) = (
            cache.thread_bound_snapshot_thread_id.as_deref(),
            cache.thread_bound_snapshot_completed_epoch_ms,
            cache.thread_bound_snapshot.clone(),
        ) {
            let now_epoch_ms_value = now_epoch_ms();
            if cached_thread_id == thread_id
                && now_epoch_ms_value.saturating_sub(completed_at)
                    <= COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
                && thread_bound_budget_snapshot_has_fresh_exact_limit_surfaces(
                    &snapshot,
                    now_epoch_ms_value,
                )
                && !load_shared_thread_bound_snapshot_invalidation(&repo_root, thread_id)
                    .is_some_and(|invalidated_at_epoch_ms| invalidated_at_epoch_ms >= completed_at)
            {
                return Some(attach_observe_cache_to_snapshot(
                    snapshot,
                    &cache,
                    state.dashboard_refresh_ms,
                ));
            }
        }
    }
    let now_epoch_ms_value = current_epoch_ms_u64();
    if let Some(snapshot) =
        load_shared_thread_bound_budget_snapshot(&repo_root, now_epoch_ms_value, thread_id)
    {
        let mut cache = state.cache.write().await;
        cache.thread_bound_snapshot = Some(snapshot.clone());
        cache.thread_bound_snapshot_thread_id = Some(thread_id.to_string());
        cache.thread_bound_snapshot_completed_epoch_ms = Some(now_epoch_ms_value);
        return Some(attach_observe_cache_to_snapshot(
            snapshot,
            &cache,
            state.dashboard_refresh_ms,
        ));
    }
    None
}

fn cached_latest_repo_working_state_restore_snapshot(cache: &ObserveCache) -> Option<Value> {
    let snapshot = cache.snapshot.as_ref()?;
    let latest_repo_restore = snapshot["latest_repo_working_state_restore"].clone();
    latest_repo_restore
        .get("working_state_restore")
        .is_some()
        .then_some(latest_repo_restore)
}

fn cached_token_budget_report_snapshot(cache: &ObserveCache) -> Option<Value> {
    let snapshot = cache.snapshot.as_ref()?;
    let report = snapshot["token_budget_report"]["token_budget_report"].clone();
    report["current_session"].is_object().then_some(report)
}

async fn compact_client_budget_snapshot_for_request(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Result<Value> {
    refresh_compact_client_budget_snapshot_on_request(state).await?;
    if let Some(thread_id) = normalized_thread_id_hint(explicit_thread_id) {
        thread_bound_snapshot_with_meta(state, thread_id).await
    } else {
        cached_snapshot_with_meta(state).await
    }
}

async fn refresh_compact_client_budget_snapshot_on_request(state: &ObserveState) -> Result<()> {
    let (snapshot_present, snapshot_age_ms, refresh_in_progress) = {
        let cache = state.cache.read().await;
        (
            cache.snapshot.is_some(),
            cache_snapshot_age_ms(&cache),
            cache.refresh_in_progress,
        )
    };

    let cache_too_old = compact_client_budget_snapshot_cache_too_old(snapshot_age_ms);
    if !snapshot_present || cache_too_old {
        if refresh_in_progress {
            return Err(anyhow!(
                "compact client-budget snapshot cache is unavailable or too stale while refresh is still in progress"
            ));
        }
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    }

    Ok(())
}

fn compact_client_budget_snapshot_cache_too_old(snapshot_age_ms: Option<u64>) -> bool {
    snapshot_age_ms
        .map(|age_ms| age_ms > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS)
        .unwrap_or(true)
}

async fn refresh_client_live_meter_on_request(state: &ObserveState) {
    if let Err(error) = maybe_refresh_client_live_meter(state).await {
        eprintln!("observe request-side client meter refresh failed: {error:#}");
    }
    let mut cache = state.cache.write().await;
    cache.client_live_meter_refresh_in_progress = false;
}

async fn spawn_client_live_meter_refresh(state: &ObserveState) {
    let now = now_epoch_ms();
    {
        let mut cache = state.cache.write().await;
        if cache.client_live_meter_refresh_in_progress {
            return;
        }
        if cache
            .client_live_meter_refresh_started_epoch_ms
            .is_some_and(|started_at| {
                now.saturating_sub(started_at) < CLIENT_LIMIT_LIVE_SOURCE_TTL_MS
            })
        {
            return;
        }
        cache.client_live_meter_refresh_in_progress = true;
        cache.client_live_meter_refresh_started_epoch_ms = Some(now);
    }
    let state = state.clone();
    tokio::spawn(async move {
        refresh_client_live_meter_on_request(&state).await;
    });
}

async fn maybe_refresh_client_live_meter(state: &ObserveState) -> Result<()> {
    let cache_snapshot = {
        let cache = state.cache.read().await;
        Some((
            cache.snapshot.clone(),
            cache.last_refresh_completed_epoch_ms,
            cache.refresh_in_progress,
            observe_cache_stale(&cache, state.dashboard_refresh_ms),
        ))
    };

    let Some((snapshot, _last_refresh_completed_epoch_ms, refresh_in_progress, cache_stale)) =
        cache_snapshot
    else {
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    };

    if cache_stale {
        if refresh_in_progress {
            return Ok(());
        }
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    }

    let Some(snapshot) = snapshot else {
        if refresh_in_progress {
            return Ok(());
        }
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    };

    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if cached_exact_client_limit_refresh_needed(
        &snapshot,
        now_epoch_ms,
        CLIENT_LIMIT_LIVE_SOURCE_TTL_MS,
    ) {
        if refresh_in_progress {
            return Ok(());
        }
        return prewarm_active_thread_bound_client_budget_surfaces(state.cache.clone(), &state.cfg)
            .await;
    }

    if active_agent_budget_refresh_needed(&snapshot, ACTIVE_AGENT_CARD_MAX_SOURCE_DRIFT_MS)? {
        if refresh_in_progress {
            return Ok(());
        }
        return prewarm_active_thread_bound_client_budget_surfaces(state.cache.clone(), &state.cfg)
            .await;
    }

    let cached_meter = cached_client_live_meter_state(&snapshot);
    let preferred_thread_id = codex_threads::current_thread_id()
        .or_else(|| cached_meter.working_state_thread_id.clone())
        .or_else(|| cached_meter.thread_id.clone());
    let Some(thread_id) = preferred_thread_id else {
        return Ok(());
    };
    let latest_rollout =
        codex_threads::latest_rollout_client_meter_observation_for_thread(&thread_id)?;
    if !client_live_meter_refresh_needed(&cached_meter, latest_rollout.as_ref()) {
        return Ok(());
    }
    if refresh_in_progress {
        return Ok(());
    }
    prewarm_thread_bound_client_budget_surfaces_for_thread(
        state.cache.clone(),
        &state.cfg,
        &thread_id,
    )
    .await
}

fn active_agent_budget_refresh_needed(snapshot: &Value, max_source_drift_ms: i64) -> Result<bool> {
    let Some(agents) = snapshot["active_agent_budget"]["agents"].as_array() else {
        return Ok(false);
    };

    for agent in agents {
        let Some(thread_id) = agent["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let card_ended_at_epoch_ms = agent["client_live_meter"]["ended_at_epoch_ms"]
            .as_i64()
            .unwrap_or_default();
        let latest_rollout =
            codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)?;
        let Some(latest_rollout) = latest_rollout else {
            continue;
        };
        if active_agent_card_refresh_needed_against_rollout(
            card_ended_at_epoch_ms,
            &latest_rollout,
            max_source_drift_ms,
        ) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn active_agent_card_refresh_needed_against_rollout(
    card_ended_at_epoch_ms: i64,
    rollout: &codex_threads::RolloutClientMeterObservation,
    max_source_drift_ms: i64,
) -> bool {
    if card_ended_at_epoch_ms <= 0 {
        return true;
    }
    rollout.ended_at_epoch_ms > card_ended_at_epoch_ms.saturating_add(max_source_drift_ms.max(0))
}

fn cached_exact_client_limit_refresh_needed(
    snapshot: &Value,
    now_epoch_ms: u64,
    max_source_age_ms: u64,
) -> bool {
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let hourly_burn = &report["client_limit_hourly_burn"];
    if hourly_burn["status"].as_str() != Some("observed") {
        return true;
    }
    let Some(observed_at_epoch_ms) = hourly_burn["latest_observed_at_epoch_ms"].as_u64() else {
        return true;
    };
    now_epoch_ms.saturating_sub(observed_at_epoch_ms) > max_source_age_ms.max(1)
}

fn cached_client_live_meter_state(snapshot: &Value) -> CachedClientLiveMeterState {
    let meter = &snapshot["token_budget_report"]["client_live_meter"];
    let working_state_thread_id =
        snapshot["latest_repo_working_state_restore"]["working_state_restore"]["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    CachedClientLiveMeterState {
        working_state_thread_id,
        thread_id: meter["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        turn_id: meter["turn_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        ended_at_epoch_ms: meter["ended_at_epoch_ms"].as_i64(),
        client_turn_total_tokens: meter["client_turn_total_tokens"].as_u64(),
        primary_limit_used_percent: meter["primary_limit_used_percent"].as_u64(),
        secondary_limit_used_percent: meter["secondary_limit_used_percent"].as_u64(),
    }
}

fn client_live_meter_refresh_needed(
    cached: &CachedClientLiveMeterState,
    rollout: Option<&codex_threads::RolloutClientMeterObservation>,
) -> bool {
    if let Some(working_state_thread_id) = cached.working_state_thread_id.as_deref() {
        if cached.thread_id.as_deref() != Some(working_state_thread_id) {
            return true;
        }
    }

    let Some(rollout) = rollout else {
        return cached.thread_id.is_none();
    };

    cached.thread_id.as_deref() != Some(rollout.thread_id.as_str())
        || cached.turn_id.as_deref() != Some(rollout.turn_id.as_str())
        || cached.ended_at_epoch_ms.unwrap_or_default() < rollout.ended_at_epoch_ms
        || cached.client_turn_total_tokens != Some(rollout.client_turn_total_tokens)
        || cached.primary_limit_used_percent != Some(rollout.latest_primary_limit_used_percent)
        || cached.secondary_limit_used_percent != Some(rollout.latest_secondary_limit_used_percent)
}

fn attach_observe_cache_to_dashboard_payload(
    mut payload: Value,
    cache: &ObserveCache,
    refresh_ms: u64,
) -> Value {
    let cache_meta = observe_cache_meta(cache, refresh_ms);
    if let Some(root) = payload.as_object_mut() {
        root.insert("observe_cache".to_string(), cache_meta.clone());
    }
    if let Some(meta) = payload["meta"].as_object_mut() {
        if let Some(started_at) = cache.last_refresh_started_epoch_ms {
            meta.insert(
                "cache_refresh_started_at_epoch_ms".to_string(),
                Value::from(started_at),
            );
        }
        if let Some(completed_at) = cache.last_refresh_completed_epoch_ms {
            meta.insert(
                "cache_refresh_completed_at_epoch_ms".to_string(),
                Value::from(completed_at),
            );
            meta.insert(
                "cache_refresh_completed_at_label".to_string(),
                Value::from(completed_at.to_string()),
            );
        }
        if let Some(duration_ms) = cache.last_refresh_duration_ms {
            meta.insert(
                "cache_refresh_duration_ms".to_string(),
                Value::from(duration_ms),
            );
        }
        meta.insert(
            "cache_snapshot_age_ms".to_string(),
            Value::from(cache_snapshot_age_ms(cache).unwrap_or_default()),
        );
        meta.insert(
            "cache_stale".to_string(),
            Value::Bool(observe_cache_stale(cache, refresh_ms)),
        );
        if let Some(error) = &cache.last_error {
            meta.insert("cache_last_error".to_string(), Value::from(error.clone()));
        }
    }
    payload
}

fn attach_observe_cache_to_snapshot(
    mut snapshot: Value,
    cache: &ObserveCache,
    refresh_ms: u64,
) -> Value {
    if let Some(root) = snapshot.as_object_mut() {
        root.insert(
            "observe_cache".to_string(),
            observe_cache_meta(cache, refresh_ms),
        );
    }
    snapshot
}

fn observe_cache_meta(cache: &ObserveCache, refresh_ms: u64) -> Value {
    let age_ms = cache_snapshot_age_ms(cache);
    json!({
        "refresh_ms": refresh_ms,
        "last_refresh_started_epoch_ms": cache.last_refresh_started_epoch_ms,
        "last_refresh_completed_epoch_ms": cache.last_refresh_completed_epoch_ms,
        "last_refresh_completed_label": cache
            .last_refresh_completed_epoch_ms
            .map(|epoch_ms| epoch_ms.to_string()),
        "last_refresh_duration_ms": cache.last_refresh_duration_ms,
        "refresh_in_progress": cache.refresh_in_progress,
        "snapshot_age_ms": age_ms,
        "stale": observe_cache_stale(cache, refresh_ms),
        "last_error": cache.last_error.clone(),
    })
}

fn observe_refresh_stuck(cache: &ObserveCache) -> bool {
    if !cache.refresh_in_progress {
        return false;
    }
    let Some(started_at_epoch_ms) = cache.last_refresh_started_epoch_ms else {
        return true;
    };
    now_epoch_ms().saturating_sub(started_at_epoch_ms)
        > OBSERVE_REFRESH_TIMEOUT_MS.saturating_add(OBSERVE_REFRESH_STUCK_GRACE_MS)
}

fn cache_snapshot_age_ms(cache: &ObserveCache) -> Option<u64> {
    cache
        .last_refresh_completed_epoch_ms
        .map(|completed_at| now_epoch_ms().saturating_sub(completed_at))
}

fn observe_cache_stale(cache: &ObserveCache, refresh_ms: u64) -> bool {
    if cache.snapshot.is_none() {
        return true;
    }
    let max_age_ms = refresh_ms.max(1000).saturating_mul(3).max(
        cache
            .last_refresh_duration_ms
            .unwrap_or_default()
            .saturating_add(refresh_ms.max(1000).saturating_mul(2)),
    );
    cache_snapshot_age_ms(cache)
        .map(|age_ms| age_ms > max_age_ms)
        .unwrap_or(true)
}

async fn collect_postgres_live(
    db: &tokio_postgres::Client,
    profile: &SnapshotProfile,
) -> Result<Value> {
    let max_connections = db
        .query_one("SHOW max_connections", &[])
        .await?
        .get::<_, String>(0)
        .parse::<u64>()
        .context("failed to parse postgres max_connections")?;
    let active_connections = db
        .query_one("SELECT COUNT(*)::bigint FROM pg_stat_activity", &[])
        .await?
        .get::<_, i64>(0) as u64;
    let row = db
        .query_one(
            r#"
            SELECT
                COALESCE(numbackends, 0)::bigint,
                COALESCE(xact_commit + xact_rollback, 0)::bigint,
                COALESCE(deadlocks, 0)::bigint
            FROM pg_stat_database
            WHERE datname = current_database()
            "#,
            &[],
        )
        .await?;
    let numbackends = row.get::<_, i64>(0) as u64;
    let transactions_total = row.get::<_, i64>(1) as u64;
    let deadlocks_total = row.get::<_, i64>(2) as u64;
    let wal_bytes_total = db
        .query_one(
            "SELECT COALESCE(wal_bytes::bigint, 0) FROM pg_stat_wal",
            &[],
        )
        .await?
        .get::<_, i64>(0) as u64;
    let replica_lag_seconds = db
        .query_one(
            r#"
            SELECT COALESCE(
                MAX(EXTRACT(EPOCH FROM COALESCE(replay_lag, flush_lag, write_lag))),
                0
            )::double precision
            FROM pg_stat_replication
            "#,
            &[],
        )
        .await?
        .get::<_, f64>(0);

    let mut probe_samples = Vec::with_capacity(profile.postgres_query_probe_iterations);
    for _ in 0..profile.postgres_query_probe_iterations {
        let started = Instant::now();
        db.query_one("SELECT 1", &[]).await?;
        probe_samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let query_probe_p95_ms = percentile_f64(&probe_samples, 95);

    Ok(json!({
        "max_connections": max_connections,
        "active_connections": active_connections,
        "numbackends": numbackends,
        "connection_usage_ratio": ratio(active_connections, max_connections),
        "transactions_total": transactions_total,
        "deadlocks_total": deadlocks_total,
        "wal_bytes_total": wal_bytes_total,
        "replica_lag_seconds": replica_lag_seconds,
        "query_probe_p95_ms": query_probe_p95_ms,
        "query_probe_samples_ms": probe_samples,
    }))
}

fn with_postgres_rates(current: &Value, previous: Option<&Value>) -> Value {
    let captured_at = current["captured_at_epoch_ms"].as_u64().unwrap_or_default();
    let prev_captured_at = previous.and_then(|value| value["captured_at_epoch_ms"].as_u64());
    let dt_ms = prev_captured_at
        .and_then(|prev| captured_at.checked_sub(prev))
        .unwrap_or_default();
    let dt_s = if dt_ms == 0 {
        None
    } else {
        Some(dt_ms as f64 / 1000.0)
    };

    let tx_per_sec = dt_s.and_then(|dt| {
        delta_rate(
            current["transactions_total"].as_f64().unwrap_or(0.0),
            previous.and_then(|value| value["postgres"]["transactions_total"].as_f64()),
            dt,
        )
    });
    let deadlocks_delta = counter_delta(
        current["deadlocks_total"].as_f64().unwrap_or(0.0),
        previous.and_then(|value| value["postgres"]["deadlocks_total"].as_f64()),
    );
    let deadlocks_per_sec = dt_s.and_then(|dt| {
        delta_rate(
            current["deadlocks_total"].as_f64().unwrap_or(0.0),
            previous.and_then(|value| value["postgres"]["deadlocks_total"].as_f64()),
            dt,
        )
    });
    let wal_bytes_per_sec = dt_s.and_then(|dt| {
        delta_rate(
            current["wal_bytes_total"].as_f64().unwrap_or(0.0),
            previous.and_then(|value| value["postgres"]["wal_bytes_total"].as_f64()),
            dt,
        )
    });

    let mut value = current.clone();
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "transactions_per_sec".to_string(),
            tx_per_sec.map_or(Value::Null, Value::from),
        );
        object.insert(
            "deadlocks_delta".to_string(),
            deadlocks_delta.map_or(Value::Null, Value::from),
        );
        object.insert(
            "deadlocks_per_sec".to_string(),
            deadlocks_per_sec.map_or(Value::Null, Value::from),
        );
        object.insert(
            "wal_bytes_per_sec".to_string(),
            wal_bytes_per_sec.map_or(Value::Null, Value::from),
        );
    }
    value
}

async fn collect_qdrant_live_from(
    qdrant_http_url: &str,
    collection_code: &str,
    http: &reqwest::Client,
) -> Result<Value> {
    let metrics_text = http
        .get(format!("{}/metrics", qdrant_http_url))
        .send()
        .await
        .context("failed to query qdrant metrics endpoint")?
        .text()
        .await
        .context("failed to read qdrant metrics response")?;
    let metrics = parse_prometheus_sums(&metrics_text);
    let (resolved_collection_code, collection) =
        resolve_qdrant_collection_live(qdrant_http_url, collection_code, http).await?;
    let result = &collection["result"];
    Ok(json!({
        "collections_total": metric_value(&metrics, "collections_total"),
        "collections_vector_total": metric_value(&metrics, "collections_vector_total"),
        "index_optimize_queue": metric_value(&metrics, "collection_update_queue_length"),
        "running_optimizations": metric_value(&metrics, "collection_running_optimizations"),
        "update_queue_length": metric_value(&metrics, "collection_update_queue_length"),
        "memory_resident_bytes": metric_value_optional(&metrics, "memory_resident_bytes"),
        "optimizer_status": result["optimizer_status"].clone(),
        "indexed_vectors_count": result["indexed_vectors_count"].clone(),
        "points_count": result["points_count"].clone(),
        "segments_count": result["segments_count"].clone(),
        "effective_collection_code": resolved_collection_code,
    }))
}

async fn resolve_qdrant_collection_live(
    qdrant_http_url: &str,
    collection_code: &str,
    http: &reqwest::Client,
) -> Result<(String, Value)> {
    if let Some(collection) =
        fetch_qdrant_collection_json(qdrant_http_url, collection_code, http).await?
    {
        return Ok((collection_code.to_string(), collection));
    }
    let Some(discovered_collection_code) =
        discover_single_qdrant_collection_code(qdrant_http_url, http).await?
    else {
        bail!(
            "qdrant collection {} is unavailable and no single fallback collection could be discovered",
            collection_code
        );
    };
    let discovered_collection =
        fetch_qdrant_collection_json(qdrant_http_url, &discovered_collection_code, http)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "qdrant fallback collection {} disappeared before it could be queried",
                    discovered_collection_code
                )
            })?;
    Ok((discovered_collection_code, discovered_collection))
}

async fn fetch_qdrant_collection_json(
    qdrant_http_url: &str,
    collection_code: &str,
    http: &reqwest::Client,
) -> Result<Option<Value>> {
    let response = http
        .get(format!(
            "{}/collections/{}",
            qdrant_http_url, collection_code
        ))
        .send()
        .await
        .context("failed to query qdrant collection endpoint")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        bail!(
            "qdrant collection endpoint {} returned HTTP {}",
            collection_code,
            response.status()
        );
    }
    let collection = response
        .json()
        .await
        .context("failed to decode qdrant collection response")?;
    Ok(Some(collection))
}

async fn discover_single_qdrant_collection_code(
    qdrant_http_url: &str,
    http: &reqwest::Client,
) -> Result<Option<String>> {
    let response = http
        .get(format!("{}/collections", qdrant_http_url))
        .send()
        .await
        .context("failed to query qdrant collections endpoint")?;
    if !response.status().is_success() {
        bail!(
            "qdrant collections endpoint returned HTTP {}",
            response.status()
        );
    }
    let collections: Value = response
        .json()
        .await
        .context("failed to decode qdrant collections response")?;
    let mut names = collections["result"]["collections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["name"].as_str())
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.len() == 1 {
        Ok(names.into_iter().next())
    } else {
        Ok(None)
    }
}

async fn collect_qdrant_live(cfg: &AppConfig, http: &reqwest::Client) -> Result<Value> {
    collect_qdrant_live_from(&cfg.qdrant_http_url, &cfg.qdrant_collection_code, http).await
}

async fn collect_optional_benchmark_qdrant_live(cfg: &AppConfig, http: &reqwest::Client) -> Value {
    let Some(qdrant_http_url) = cfg.benchmark_qdrant_http_url.as_deref() else {
        return json!({
            "available": false,
            "configured": false,
            "reason": "missing benchmark qdrant config",
        });
    };
    let Some(collection_code) = cfg.benchmark_qdrant_collection_code.as_deref() else {
        return json!({
            "available": false,
            "configured": false,
            "reason": "missing benchmark qdrant collection config",
        });
    };
    let benchmark_active = discover_repo_root(None)
        .ok()
        .and_then(|repo_root| {
            external_benchmark::benchmark_run_active_for_qdrant_http_url(
                &repo_root,
                qdrant_http_url,
            )
        })
        .unwrap_or(false);
    let benchmark_run_summary = discover_repo_root(None).ok().and_then(|repo_root| {
        external_benchmark::benchmark_run_summary_for_qdrant_http_url(&repo_root, qdrant_http_url)
    });
    match collect_qdrant_live_from(qdrant_http_url, collection_code, http).await {
        Ok(mut value) => {
            if let Some(object) = value.as_object_mut() {
                object.insert("available".to_string(), Value::Bool(true));
                object.insert("configured".to_string(), Value::Bool(true));
                object.insert("active".to_string(), Value::Bool(benchmark_active));
                object.insert("from_last_success".to_string(), Value::Bool(false));
                object.insert(
                    "http_url".to_string(),
                    Value::String(qdrant_http_url.to_string()),
                );
                object.insert(
                    "collection_code".to_string(),
                    Value::String(collection_code.to_string()),
                );
                object.insert(
                    "captured_at_epoch_ms".to_string(),
                    Value::from(now_epoch_ms()),
                );
                if let Some(run_summary) = benchmark_run_summary.clone() {
                    let mut run_summary = run_summary;
                    if let Ok(repo_root) = discover_repo_root(None) {
                        external_benchmark::enrich_untracked_ann_run_summary(
                            &repo_root,
                            &mut run_summary,
                        );
                    }
                    object.insert("run_summary".to_string(), run_summary);
                }
            }
            persist_last_successful_benchmark_qdrant_snapshot(&value);
            value
        }
        Err(_error) => load_last_successful_benchmark_qdrant_snapshot()
            .map(|mut cached| {
                if let Some(object) = cached.as_object_mut() {
                    object.insert("available".to_string(), Value::Bool(false));
                    object.insert("configured".to_string(), Value::Bool(true));
                    object.insert("active".to_string(), Value::Bool(false));
                    object.insert("from_last_success".to_string(), Value::Bool(true));
                    object.insert(
                        "http_url".to_string(),
                        Value::String(qdrant_http_url.to_string()),
                    );
                    object.insert(
                        "collection_code".to_string(),
                        Value::String(collection_code.to_string()),
                    );
                    if let Some(run_summary) = benchmark_run_summary.clone() {
                        let mut run_summary = run_summary;
                        if let Ok(repo_root) = discover_repo_root(None) {
                            external_benchmark::enrich_untracked_ann_run_summary(
                                &repo_root,
                                &mut run_summary,
                            );
                        }
                        object.insert("run_summary".to_string(), run_summary);
                    }
                }
                cached
            })
            .unwrap_or_else(|| {
                json!({
                    "available": false,
                    "configured": true,
                    "active": false,
                    "from_last_success": false,
                    "http_url": qdrant_http_url,
                    "collection_code": collection_code,
                    "run_summary": benchmark_run_summary,
                })
            }),
    }
}

fn benchmark_qdrant_cache_path() -> Option<PathBuf> {
    let repo_root = discover_repo_root(None).ok()?;
    Some(
        repo_root
            .join("state")
            .join("observe")
            .join("benchmark_qdrant_last_success.json"),
    )
}

fn cold_benchmark_live_progress_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join("state")
        .join("cold-benchmark")
        .join("live_progress.json")
}

fn read_live_cold_benchmark_progress(repo_root: &Path) -> Option<Value> {
    let path = cold_benchmark_live_progress_path(repo_root);
    let raw = fs::read_to_string(&path).ok()?;
    let payload: Value = serde_json::from_str(&raw).ok()?;
    let progress = &payload["cold_benchmark_progress"];
    if progress["state"].as_str() != Some("running") {
        let _ = fs::remove_file(path);
        return None;
    }
    let pid = progress["pid"].as_u64()? as u32;
    if !cold_benchmark_pid_is_live(pid) {
        let _ = fs::remove_file(path);
        return None;
    }
    Some(payload)
}

async fn enrich_live_cold_benchmark_progress(
    db: &Client,
    progress: Option<Value>,
) -> Result<Option<Value>> {
    let Some(mut payload) = progress else {
        return Ok(None);
    };
    let current_repo_code = payload["cold_benchmark_progress"]["current_repo_code"]
        .as_str()
        .map(str::to_string);
    if let Some(project_code) = current_repo_code {
        let indexed_files = postgres::count_documents_for_project_namespace_codes(
            db,
            &project_code,
            "cold_benchmark",
        )
        .await?;
        if let Some(progress_object) =
            payload["cold_benchmark_progress"]["progress"].as_object_mut()
        {
            progress_object.insert(
                "current_repo_indexed_files".to_string(),
                Value::from(indexed_files),
            );
        }
    }
    Ok(Some(payload))
}

#[cfg(target_os = "linux")]
fn cold_benchmark_pid_is_live(pid: u32) -> bool {
    let proc_dir = PathBuf::from("/proc").join(pid.to_string());
    if !proc_dir.exists() {
        return false;
    }
    let cmdline = fs::read(proc_dir.join("cmdline")).ok();
    cmdline
        .map(|bytes| String::from_utf8_lossy(&bytes).contains("cold-path"))
        .unwrap_or(true)
}

#[cfg(not(target_os = "linux"))]
fn cold_benchmark_pid_is_live(_pid: u32) -> bool {
    true
}

fn persist_last_successful_benchmark_qdrant_snapshot(value: &Value) {
    let Some(path) = benchmark_qdrant_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(value) else {
        return;
    };
    let _ = fs::write(path, text);
}

fn load_last_successful_benchmark_qdrant_snapshot() -> Option<Value> {
    let path = benchmark_qdrant_cache_path()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn collect_nats_live(
    cfg: &AppConfig,
    http: &reqwest::Client,
    profile: &SnapshotProfile,
) -> Result<Value> {
    let varz: Value = http
        .get(format!("{}/varz", cfg.nats_http_url))
        .send()
        .await
        .context("failed to query nats /varz")?
        .json()
        .await
        .context("failed to decode nats /varz")?;
    let jsz: Value = http
        .get(format!("{}/jsz?streams=1&consumers=1", cfg.nats_http_url))
        .send()
        .await
        .context("failed to query nats /jsz")?
        .json()
        .await
        .context("failed to decode nats /jsz")?;

    let client = nats::connect(cfg).await?;
    let mut publish_samples = Vec::with_capacity(profile.nats_publish_probe_iterations);
    for index in 0..profile.nats_publish_probe_iterations {
        let started = Instant::now();
        client
            .publish(
                "ami.event.observe.probe",
                format!("probe-{index}").into_bytes().into(),
            )
            .await
            .context("failed to publish nats probe message")?;
        client
            .flush()
            .await
            .context("failed to flush nats probe publish")?;
        publish_samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    let jetstream_storage = jsz["storage"].as_f64().unwrap_or(0.0);
    let jetstream_max_storage = jsz["config"]["max_storage"].as_f64().unwrap_or(0.0);
    Ok(json!({
        "version": varz["version"].clone(),
        "connections": varz["connections"].clone(),
        "slow_consumers": varz["slow_consumers"].clone(),
        "in_msgs": varz["in_msgs"].clone(),
        "out_msgs": varz["out_msgs"].clone(),
        "jetstream_storage_bytes": jetstream_storage,
        "jetstream_max_storage_bytes": jetstream_max_storage,
        "jetstream_disk_usage_ratio": ratio_f64(jetstream_storage, jetstream_max_storage),
        "consumer_lag_msgs": extract_nats_consumer_lag(&jsz),
        "publish_probe_p95_ms": percentile_f64(&publish_samples, 95),
        "publish_probe_samples_ms": publish_samples,
    }))
}

async fn collect_s3_live(cfg: &AppConfig) -> Result<Value> {
    let client = s3::connect(cfg).await?;
    let started = Instant::now();
    let buckets = s3::status_bucket_names(&client).await?;
    let list_buckets_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(json!({
        "bucket_count": buckets.len(),
        "context_bucket_available": buckets.iter().any(|bucket| bucket == &cfg.s3_bucket_context),
        "artifacts_bucket_available": buckets.iter().any(|bucket| bucket == &cfg.s3_bucket_artifacts),
        "transcripts_bucket_available": buckets.iter().any(|bucket| bucket == &cfg.s3_bucket_transcripts),
        "list_buckets_ms": list_buckets_ms,
    }))
}

fn evaluate_sla(snapshot: &Value, profile: &ObservabilityProfile) -> Value {
    let mut checks = vec![
        max_check(
            "postgres.connection_usage_ratio",
            snapshot["postgres"]["connection_usage_ratio"].as_f64(),
            profile.postgres.target_connection_usage_ratio,
            profile.postgres.alert_connection_usage_ratio,
            profile.postgres.critical_connection_usage_ratio,
        ),
        max_check(
            "postgres.query_probe_p95_ms",
            snapshot["postgres"]["query_probe_p95_ms"].as_f64(),
            profile.postgres.target_query_probe_p95_ms,
            profile.postgres.alert_query_probe_p95_ms,
            profile.postgres.critical_query_probe_p95_ms,
        ),
        max_check(
            "postgres.replica_lag_seconds",
            snapshot["postgres"]["replica_lag_seconds"].as_f64(),
            profile.postgres.target_replica_lag_seconds,
            profile.postgres.alert_replica_lag_seconds,
            profile.postgres.critical_replica_lag_seconds,
        ),
        max_check(
            "qdrant.index_optimize_queue",
            snapshot["qdrant"]["index_optimize_queue"].as_f64(),
            profile.qdrant.target_index_optimize_queue,
            profile.qdrant.alert_index_optimize_queue,
            profile.qdrant.critical_index_optimize_queue,
        ),
        max_check(
            "qdrant.update_queue_length",
            snapshot["qdrant"]["update_queue_length"].as_f64(),
            profile.qdrant.target_update_queue_length,
            profile.qdrant.alert_update_queue_length,
            profile.qdrant.critical_update_queue_length,
        ),
        max_check(
            "qdrant.search_stage_p95_ms",
            snapshot["latest_retrieval_cold"]["retrieval_runtime"]["stage_p95_ms"]
                ["semantic_search_ms"]
                .as_f64(),
            profile.qdrant.target_search_p95_ms,
            profile.qdrant.alert_search_p95_ms,
            profile.qdrant.critical_search_p95_ms,
        ),
        max_check(
            "nats.publish_probe_p95_ms",
            snapshot["nats"]["publish_probe_p95_ms"].as_f64(),
            profile.nats.target_publish_p95_ms,
            profile.nats.alert_publish_p95_ms,
            profile.nats.critical_publish_p95_ms,
        ),
        max_check(
            "nats.consumer_lag_msgs",
            snapshot["nats"]["consumer_lag_msgs"].as_f64(),
            profile.nats.target_consumer_lag_msgs,
            profile.nats.alert_consumer_lag_msgs,
            profile.nats.critical_consumer_lag_msgs,
        ),
        max_check(
            "nats.jetstream_disk_usage_ratio",
            snapshot["nats"]["jetstream_disk_usage_ratio"].as_f64(),
            profile.nats.target_jetstream_disk_usage_ratio,
            profile.nats.alert_jetstream_disk_usage_ratio,
            profile.nats.critical_jetstream_disk_usage_ratio,
        ),
        max_check(
            "retrieval.cold_p95_ms",
            snapshot["latest_retrieval_cold"]["benchmark"]["p95_ms"].as_f64(),
            profile.retrieval.target_p95_ms,
            profile.retrieval.alert_p95_ms,
            profile.retrieval.critical_p95_ms,
        ),
        max_check(
            "retrieval.hot_p95_ms",
            snapshot["latest_retrieval_hot"]["benchmark"]["p95_ms"].as_f64(),
            profile.retrieval.target_hot_p95_ms,
            profile.retrieval.alert_p95_ms,
            profile.retrieval.critical_p95_ms,
        ),
        min_check(
            "parser.coverage_ratio",
            snapshot["latest_index_project"]["index_project"]["parser_coverage_ratio"].as_f64(),
            profile.parser.target_coverage_ratio,
            profile.parser.alert_coverage_ratio,
            profile.parser.critical_coverage_ratio,
        ),
    ];

    if let Some(check) = optional_zero_check(
        "postgres.deadlocks_delta",
        snapshot["postgres"]["deadlocks_delta"].as_f64(),
    ) {
        checks.push(check);
    }

    if let Some(check) = optional_zero_check(
        "accuracy.cross_project_leakage",
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["cross_project_leakage"]
            .as_f64(),
    ) {
        checks.push(check);
    }
    if let Some(check) = optional_min_check(
        "accuracy.symbol_precision",
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["symbol_precision"].as_f64(),
        profile.accuracy.target_symbol_precision,
        profile.accuracy.alert_symbol_precision,
        profile.accuracy.critical_symbol_precision,
    ) {
        checks.push(check);
    }
    if let Some(check) = optional_min_check(
        "accuracy.semantic_precision",
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["semantic_precision"]
            .as_f64(),
        profile.accuracy.target_semantic_precision,
        profile.accuracy.alert_semantic_precision,
        profile.accuracy.critical_semantic_precision,
    ) {
        checks.push(check);
    }
    if let Some(check) = optional_min_check(
        "load.hot_qps",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["qps"].as_f64(),
        profile.load.target_hot_qps,
        profile.load.alert_hot_qps,
        profile.load.critical_hot_qps,
    ) {
        checks.push(check);
    }
    if let Some(check) = optional_max_check(
        "load.hot_error_rate",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["error_rate"].as_f64(),
        profile.load.target_hot_error_rate,
        profile.load.alert_hot_error_rate,
        profile.load.critical_hot_error_rate,
    ) {
        checks.push(check);
    }
    if let Some(check) = optional_zero_check(
        "observability.benchmark_contamination",
        benchmark_contamination_value(snapshot),
    ) {
        checks.push(check);
    }

    let mut pass = 0_u64;
    let mut alert = 0_u64;
    let mut critical = 0_u64;
    let mut unknown = 0_u64;
    for check in &checks {
        match check["status"].as_str().unwrap_or("unknown") {
            "pass" => pass += 1,
            "alert" => alert += 1,
            "critical" => critical += 1,
            _ => unknown += 1,
        }
    }

    json!({
        "checks": checks,
        "summary": {
            "pass": pass,
            "alert": alert,
            "critical": critical,
            "unknown": unknown,
        }
    })
}

fn benchmark_contamination_value(snapshot: &Value) -> Option<f64> {
    let checks = [
        benchmark_payload_contaminated(
            contamination_probe_payload(
                snapshot,
                "latest_retrieval_load_hot_raw",
                "latest_retrieval_load_hot",
            ),
            "load_verification",
        ),
        benchmark_payload_contaminated(
            contamination_probe_payload(
                snapshot,
                "latest_retrieval_load_cold_raw",
                "latest_retrieval_load_cold",
            ),
            "load_verification",
        ),
        benchmark_payload_contaminated(&snapshot["latest_retrieval_hot"], "benchmark"),
        benchmark_payload_contaminated(&snapshot["latest_retrieval_cold"], "benchmark"),
        benchmark_payload_contaminated(&snapshot["latest_cold_path_benchmark"], "cold_benchmark"),
    ];
    let mut saw_payload = false;
    for contaminated in checks.into_iter().flatten() {
        saw_payload = true;
        if contaminated {
            return Some(1.0);
        }
    }
    if saw_payload { Some(0.0) } else { None }
}

fn contamination_probe_payload<'a>(
    snapshot: &'a Value,
    raw_key: &str,
    selected_key: &str,
) -> &'a Value {
    let raw = &snapshot[raw_key];
    if raw.is_null() {
        &snapshot[selected_key]
    } else {
        raw
    }
}

fn benchmark_payload_contaminated(payload: &Value, expected_root: &str) -> Option<bool> {
    if payload.is_null() {
        return None;
    }
    let root = payload.get(expected_root)?;
    if root.is_null() || !root.is_object() {
        return Some(true);
    }
    if root["record_live_context"].as_bool() == Some(true)
        || root["publish_benchmark_snapshot"].as_bool() == Some(false)
    {
        return Some(true);
    }
    if let Some(source_class) = payload["_observability"]["source_class"].as_str() {
        return Some(source_class != "benchmark");
    }
    Some(false)
}

fn max_check(metric: &str, value: Option<f64>, target: f64, alert: f64, critical: f64) -> Value {
    match value {
        Some(value) if value <= target => {
            threshold_json(metric, value, target, alert, critical, "pass")
        }
        Some(value) if value <= alert => {
            threshold_json(metric, value, target, alert, critical, "alert")
        }
        Some(value) => threshold_json(
            metric,
            value,
            target,
            alert,
            critical,
            if value > critical {
                "critical"
            } else {
                "alert"
            },
        ),
        None => threshold_json(metric, Value::Null, target, alert, critical, "unknown"),
    }
}

fn min_check(metric: &str, value: Option<f64>, target: f64, alert: f64, critical: f64) -> Value {
    match value {
        Some(value) if value >= target => {
            threshold_json(metric, value, target, alert, critical, "pass")
        }
        Some(value) if value >= alert => {
            threshold_json(metric, value, target, alert, critical, "alert")
        }
        Some(value) => threshold_json(
            metric,
            value,
            target,
            alert,
            critical,
            if value < critical {
                "critical"
            } else {
                "alert"
            },
        ),
        None => threshold_json(metric, Value::Null, target, alert, critical, "unknown"),
    }
}

fn zero_check(metric: &str, value: Option<f64>) -> Value {
    match value {
        Some(value) if value == 0.0 => {
            json!({"metric": metric, "value": value, "status": "pass", "target": 0})
        }
        Some(value) => json!({"metric": metric, "value": value, "status": "critical", "target": 0}),
        None => json!({"metric": metric, "value": Value::Null, "status": "unknown", "target": 0}),
    }
}

fn optional_max_check(
    metric: &str,
    value: Option<f64>,
    target: f64,
    alert: f64,
    critical: f64,
) -> Option<Value> {
    value.map(|value| max_check(metric, Some(value), target, alert, critical))
}

fn optional_min_check(
    metric: &str,
    value: Option<f64>,
    target: f64,
    alert: f64,
    critical: f64,
) -> Option<Value> {
    value.map(|value| min_check(metric, Some(value), target, alert, critical))
}

fn optional_zero_check(metric: &str, value: Option<f64>) -> Option<Value> {
    value.map(|value| zero_check(metric, Some(value)))
}

fn threshold_json(
    metric: &str,
    value: impl Into<Value>,
    target: f64,
    alert: f64,
    critical: f64,
    status: &str,
) -> Value {
    json!({
        "metric": metric,
        "value": value.into(),
        "target": target,
        "alert": alert,
        "critical": critical,
        "status": status,
    })
}

fn load_profile() -> Result<ObservabilityProfile> {
    let path = profile_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read observability profile {}", path.display()))?;
    toml::from_str(&content).context("failed to parse observability profile")
}

fn profile_path() -> PathBuf {
    let cwd_path = Path::new("config/observability.toml");
    if cwd_path.exists() {
        cwd_path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join("observability.toml")
    }
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .context("failed to build observe HTTP client")
}

fn parse_prometheus_sums(body: &str) -> HashMap<String, f64> {
    let mut values = HashMap::new();
    for line in body.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(metric) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        let name = metric.split('{').next().unwrap_or(metric).to_string();
        let parsed = value.parse::<f64>().unwrap_or(0.0);
        *values.entry(name).or_insert(0.0) += parsed;
    }
    values
}

fn metric_value(metrics: &HashMap<String, f64>, key: &str) -> f64 {
    metrics.get(key).copied().unwrap_or(0.0)
}

fn metric_value_optional(metrics: &HashMap<String, f64>, key: &str) -> Option<f64> {
    metrics.get(key).copied()
}

fn extract_nats_consumer_lag(jsz: &Value) -> u64 {
    jsz["account_details"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|account| account["stream_detail"].as_array().into_iter().flatten())
        .flat_map(|stream| {
            stream["consumer_detail"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|consumer| consumer["num_pending"].as_u64())
        })
        .max()
        .unwrap_or(0)
}

fn percentile_f64(samples: &[f64], percentile: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (percentile.min(100) * sorted.len()).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64
}

fn ratio_f64(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        return 0.0;
    }
    numerator / denominator
}

fn delta_rate(current: f64, previous: Option<f64>, dt_s: f64) -> Option<f64> {
    let previous = previous?;
    if dt_s <= 0.0 || current < previous {
        return None;
    }
    Some((current - previous) / dt_s)
}

fn counter_delta(current: f64, previous: Option<f64>) -> Option<f64> {
    let previous = previous?;
    if current < previous {
        return None;
    }
    Some(current - previous)
}

fn render_prometheus_metrics(snapshot: &Value) -> String {
    let mut output = String::new();

    push_metric(
        &mut output,
        "amai_postgres_connection_usage_ratio",
        "PostgreSQL connection usage ratio.",
        snapshot["postgres"]["connection_usage_ratio"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_postgres_query_probe_p95_ms",
        "PostgreSQL SELECT 1 probe latency p95 in milliseconds.",
        snapshot["postgres"]["query_probe_p95_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_postgres_transactions_per_sec",
        "PostgreSQL transactions per second between the latest system snapshots.",
        snapshot["postgres"]["transactions_per_sec"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_postgres_deadlocks_total",
        "PostgreSQL deadlocks total for the active database.",
        snapshot["postgres"]["deadlocks_total"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_postgres_deadlocks_delta",
        "PostgreSQL deadlock counter delta between the latest system snapshots.",
        snapshot["postgres"]["deadlocks_delta"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_postgres_wal_bytes_per_sec",
        "PostgreSQL WAL bytes per second between the latest system snapshots.",
        snapshot["postgres"]["wal_bytes_per_sec"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_postgres_replica_lag_seconds",
        "PostgreSQL replica lag in seconds.",
        snapshot["postgres"]["replica_lag_seconds"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_qdrant_index_optimize_queue",
        "Qdrant index optimization queue length.",
        snapshot["qdrant"]["index_optimize_queue"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_qdrant_update_queue_length",
        "Qdrant update queue length.",
        snapshot["qdrant"]["update_queue_length"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_qdrant_search_stage_p95_ms",
        "Qdrant semantic search stage p95 from the latest cold retrieval benchmark.",
        snapshot["latest_retrieval_cold"]["retrieval_runtime"]["stage_p95_ms"]["semantic_search_ms"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_qdrant_memory_resident_bytes",
        "Qdrant resident memory in bytes.",
        snapshot["qdrant"]["memory_resident_bytes"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_nats_publish_probe_p95_ms",
        "NATS publish+flush probe latency p95 in milliseconds.",
        snapshot["nats"]["publish_probe_p95_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_nats_consumer_lag_msgs",
        "JetStream consumer lag in messages.",
        snapshot["nats"]["consumer_lag_msgs"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_nats_jetstream_disk_usage_ratio",
        "JetStream disk usage ratio.",
        snapshot["nats"]["jetstream_disk_usage_ratio"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_retrieval_hot_p95_ms",
        "Hot retrieval benchmark p95 in milliseconds.",
        snapshot["latest_retrieval_hot"]["benchmark"]["p95_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_retrieval_cold_p95_ms",
        "Cold retrieval benchmark p95 in milliseconds.",
        snapshot["latest_retrieval_cold"]["benchmark"]["p95_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_p50_ms",
        "Latest end-to-end cold contour p50 in milliseconds.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]["p50"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_p95_ms",
        "Latest end-to-end cold contour p95 in milliseconds.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]["p95"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_p99_ms",
        "Latest end-to-end cold contour p99 in milliseconds.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]["p99"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_max_ms",
        "Latest end-to-end cold contour max in milliseconds.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]["max"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_precision",
        "Latest end-to-end cold contour precision.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]
            ["precision"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_recall",
        "Latest end-to-end cold contour recall.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]["recall"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_hit_rate",
        "Latest end-to-end cold contour target hit rate.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]
            ["hit_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_fallback_rate",
        "Latest end-to-end cold contour fallback rate.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]
            ["fallback_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_cold_contour_target_met",
        "Latest end-to-end cold contour target_met as 1 or 0.",
        snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["machine_readable_summary"]
            ["target_met"]
            .as_bool()
            .map(|value| if value { 1.0 } else { 0.0 }),
    );
    for (state, display) in [("mixed", "mix"), ("hot", "hot"), ("cold", "cold")] {
        push_metric(
            &mut output,
            &format!("amai_retrieval_live_{display}_current_ms"),
            &format!("Current live {display} retrieval latency in milliseconds."),
            live_latency_value(snapshot, state, "current_latency_ms"),
        );
        push_metric(
            &mut output,
            &format!("amai_retrieval_live_{display}_p50_ms"),
            &format!("Live {display} retrieval latency p50 in milliseconds."),
            live_latency_value(snapshot, state, "p50_latency_ms"),
        );
        push_metric(
            &mut output,
            &format!("amai_retrieval_live_{display}_p95_ms"),
            &format!("Live {display} retrieval latency p95 in milliseconds."),
            live_latency_value(snapshot, state, "p95_latency_ms"),
        );
        push_metric(
            &mut output,
            &format!("amai_retrieval_live_{display}_p99_ms"),
            &format!("Live {display} retrieval latency p99 in milliseconds."),
            live_latency_value(snapshot, state, "p99_latency_ms"),
        );
        push_metric(
            &mut output,
            &format!("amai_retrieval_live_{display}_max_ms"),
            &format!("Live {display} retrieval latency max in milliseconds."),
            live_latency_value(snapshot, state, "max_latency_ms"),
        );
        push_metric(
            &mut output,
            &format!("amai_retrieval_live_{display}_sample_count"),
            &format!("Sample count for live {display} retrieval latency."),
            live_latency_value(snapshot, state, "sample_count"),
        );
    }
    push_metric(
        &mut output,
        "amai_index_files_per_min",
        "Latest indexing throughput in files per minute.",
        snapshot["latest_index_project"]["index_project"]["files_per_min"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_parser_coverage_ratio",
        "Latest parser coverage ratio.",
        snapshot["latest_index_project"]["index_project"]["parser_coverage_ratio"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_accuracy_cross_project_leakage",
        "Cross-project leakage count from the latest accuracy verification.",
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["cross_project_leakage"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_accuracy_symbol_precision",
        "Symbol precision from the latest accuracy verification.",
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["symbol_precision"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_accuracy_semantic_precision",
        "Semantic precision from the latest accuracy verification.",
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["semantic_precision"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_qps",
        "Concurrent hot retrieval QPS from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["qps"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_p50_ms",
        "Hot benchmark p50 latency from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["p50_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_p95_ms",
        "Hot benchmark p95 latency from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["p95_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_p99_ms",
        "Hot benchmark p99 latency from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["p99_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_max_ms",
        "Hot benchmark max latency from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["max_ms"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_error_rate",
        "Concurrent hot retrieval error rate from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["error_rate"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_observability_benchmark_contamination",
        "Whether benchmark-facing observability snapshots were contaminated by live-context payloads.",
        benchmark_contamination_value(snapshot),
    );
    push_metric(
        &mut output,
        "amai_load_hot_workers",
        "Parallel worker count from the latest hot load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["workers"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_load_hot_sample_count",
        "Total sample count from the latest hot load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["success_count"]
            .as_f64()
            .zip(snapshot["latest_retrieval_load_hot"]["load_verification"]["error_count"].as_f64())
            .map(|(success, errors)| success + errors),
    );
    push_metric(
        &mut output,
        "amai_tokens_naive_scope_total",
        "Naive visible-scope token count from the latest token benchmark.",
        snapshot["latest_token_benchmark"]["token_benchmark"]["naive_scope"]["tokens"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_context_pack_total",
        "Context-pack token count from the latest token benchmark.",
        snapshot["latest_token_benchmark"]["token_benchmark"]["context_pack_render"]["tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_saved_total",
        "Saved tokens from the latest token benchmark.",
        snapshot["latest_token_benchmark"]["token_benchmark"]["savings"]["saved_tokens"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_factor",
        "Naive-scope to context-pack token reduction factor from the latest token benchmark.",
        snapshot["latest_token_benchmark"]["token_benchmark"]["savings"]["savings_factor"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent",
        "Token savings percent from the latest token benchmark.",
        snapshot["latest_token_benchmark"]["token_benchmark"]["savings"]["savings_percent"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_saved_session_total",
        "Verified effective saved tokens accumulated in the current live token-usage session.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]
            ["verified_effective_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_saved_window_total",
        "Verified effective saved tokens accumulated in the current live budget window.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]
            ["verified_effective_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_saved_lifetime_total",
        "Verified effective saved tokens accumulated across all live recorded token-usage events.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]
            ["verified_effective_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent_session",
        "Verified effective savings percent accumulated in the current live token-usage session.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]
            ["verified_effective_savings_pct"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent_window",
        "Verified effective savings percent accumulated in the current live budget window.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]
            ["verified_effective_savings_pct"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent_lifetime",
        "Verified effective savings percent accumulated across all live recorded token-usage events.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]
            ["verified_effective_savings_pct"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_raw_saved_session_total",
        "Raw saved tokens accumulated in the current session before quality gating.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]
            ["total_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_raw_saved_window_total",
        "Raw saved tokens accumulated in the current budget window before quality gating.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["total_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_raw_saved_lifetime_total",
        "Raw saved tokens accumulated across all recorded token-usage events before quality gating.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["total_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_raw_savings_percent_session",
        "Raw savings percent accumulated in the current session before quality gating.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]["savings_percent"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_raw_savings_percent_window",
        "Raw savings percent accumulated in the current budget window before quality gating.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["savings_percent"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_raw_savings_percent_lifetime",
        "Raw savings percent accumulated across all recorded token-usage events before quality gating.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["savings_percent"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_quality_ok_rate_session",
        "Share of current-session live events that passed the quality gate.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]["quality_ok_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_quality_ok_rate_window",
        "Share of current-window live events that passed the quality gate.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["quality_ok_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_quality_ok_rate_lifetime",
        "Share of lifetime live events that passed the quality gate.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["quality_ok_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_fallback_rate_session",
        "Share of current-session live events that needed fallback or follow-up.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]["fallback_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_fallback_rate_window",
        "Share of current-window live events that needed fallback or follow-up.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["fallback_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_fallback_rate_lifetime",
        "Share of lifetime live events that needed fallback or follow-up.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["fallback_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_answer_like_rate_session",
        "Share of current-session live events that already reached the stricter answer-like contour.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]["answer_like_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_answer_like_rate_window",
        "Share of current-window live events that already reached the stricter answer-like contour.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["answer_like_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_answer_like_rate_lifetime",
        "Share of lifetime live events that already reached the stricter answer-like contour.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["answer_like_rate"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_sla_pass_total",
        "Count of SLA checks currently passing.",
        snapshot["sla"]["summary"]["pass"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_sla_alert_total",
        "Count of SLA checks currently in alert state.",
        snapshot["sla"]["summary"]["alert"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_sla_critical_total",
        "Count of SLA checks currently in critical state.",
        snapshot["sla"]["summary"]["critical"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_sla_unknown_total",
        "Count of SLA checks currently unknown.",
        snapshot["sla"]["summary"]["unknown"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_degradation_pass_total",
        "Count of degradation classes with fresh passing evidence.",
        snapshot["degradation_model"]["summary"]["pass"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_degradation_critical_total",
        "Count of degradation classes currently failing their last known proof.",
        snapshot["degradation_model"]["summary"]["critical"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_degradation_unknown_total",
        "Count of degradation classes without fresh machine-readable proof.",
        snapshot["degradation_model"]["summary"]["unknown"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_degradation_fail_closed_total",
        "Count of fail-closed degradation classes in the current policy.",
        snapshot["degradation_model"]["summary"]["fail_closed_total"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_degradation_graceful_fallback_total",
        "Count of graceful-fallback degradation classes in the current policy.",
        snapshot["degradation_model"]["summary"]["graceful_fallback_total"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_degradation_evidence_gaps_total",
        "Count of degradation classes that still lack fresh machine-readable proof.",
        snapshot["degradation_model"]["summary"]["evidence_gaps"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_continuity_verified_probes_total",
        "Count of continuity verification probes currently confirmed by the last known proof.",
        snapshot["continuity_correctness_model"]["summary"]["verified_probes"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_continuity_failed_probes_total",
        "Count of continuity verification probes currently failing the last known proof.",
        snapshot["continuity_correctness_model"]["summary"]["failed_probes"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_continuity_recovered_useful_total",
        "Count of continuity verification probes that recovered useful working context.",
        snapshot["continuity_correctness_model"]["summary"]["recovered_useful"].as_f64(),
    );
    push_metric(
        &mut output,
        "amai_continuity_fail_closed_total",
        "Count of continuity verification probes that fail-closed instead of substituting a wrong chat or time slice.",
        snapshot["continuity_correctness_model"]["summary"]["fail_closed"].as_f64(),
    );

    output
}

fn live_latency_value(snapshot: &Value, state: &str, field: &str) -> Option<f64> {
    snapshot["token_budget_report"]["token_budget_report"]["current_session"]["latency_slices"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|slice| slice["state"].as_str() == Some(state))
        .and_then(|slice| slice[field].as_f64())
        .or_else(|| {
            snapshot["token_budget_report"]["token_budget_report"]["current_session"]
                ["latency_slices"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|slice| slice["state"].as_str() == Some(state))
                .and_then(|slice| slice[field].as_u64())
                .map(|value| value as f64)
        })
}

fn push_metric(output: &mut String, name: &str, help: &str, value: Option<f64>) {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return;
    };
    output.push_str("# HELP ");
    output.push_str(name);
    output.push(' ');
    output.push_str(help);
    output.push('\n');
    output.push_str("# TYPE ");
    output.push_str(name);
    output.push_str(" gauge\n");
    output.push_str(name);
    output.push(' ');
    output.push_str(&value.to_string());
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::{
        COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS, ObserveCache, ObserveState,
        benchmark_contamination_value, build_continuity_correctness_model, build_degradation_model,
        cache_snapshot_age_ms, cached_client_live_meter_state, client_live_meter_refresh_needed,
        compact_client_budget_snapshot_cache_too_old, dashboard_live_summary_api_handler,
        dashboard_page_handler, evaluate_sla, expired_retention_candidates, load_profile,
        merge_thread_bound_client_budget_snapshot_into_base_snapshot, normalized_thread_id_hint,
        profile_thresholds_json, render_prometheus_metrics, select_latest_clean_benchmark_snapshot,
    };
    use crate::codex_threads::{
        RecentClientThreadRecord, RolloutClientMeterObservation, RolloutJsonlToleranceSummary,
    };
    use crate::config::AppConfig;
    use crate::postgres::ObservabilityRetentionCandidate;
    use crate::working_state;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use reqwest::Client;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn test_config() -> AppConfig {
        AppConfig {
            stack_name: "amai".to_string(),
            pg_db: "amai".to_string(),
            app_db_user: "amai".to_string(),
            app_db_password: "amai".to_string(),
            postgres_dsn: "postgres://127.0.0.1:1/unused".to_string(),
            app_postgres_dsn: "postgres://127.0.0.1:1/unused".to_string(),
            qdrant_url: "http://127.0.0.1:1".to_string(),
            qdrant_http_url: "http://127.0.0.1:1".to_string(),
            qdrant_collection_code: "test".to_string(),
            benchmark_qdrant_http_url: None,
            benchmark_qdrant_collection_code: None,
            qdrant_alias_code: "test".to_string(),
            qdrant_collection_memory: "memory".to_string(),
            qdrant_alias_memory: "memory".to_string(),
            qdrant_code_dim: 384,
            qdrant_memory_dim: 384,
            qdrant_distance: "Cosine".to_string(),
            s3_endpoint: "http://127.0.0.1:1".to_string(),
            s3_region: "us-east-1".to_string(),
            s3_access_key: "test".to_string(),
            s3_secret_key: "test".to_string(),
            s3_bucket_artifacts: "artifacts".to_string(),
            s3_bucket_transcripts: "transcripts".to_string(),
            s3_bucket_context: "context".to_string(),
            nats_url: "nats://127.0.0.1:1".to_string(),
            nats_http_url: "http://127.0.0.1:1".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            edge_cache_path: "/tmp/observe-test-edge-cache.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            local_fast_cache_ttl_ms: 5_000,
        }
    }

    #[tokio::test]
    async fn collect_qdrant_live_falls_back_to_single_discovered_collection() {
        async fn metrics_handler() -> &'static str {
            "# HELP collections_total number of collections\n# TYPE collections_total gauge\ncollections_total 1\n# HELP collections_vector_total total number of vectors in all collections\n# TYPE collections_vector_total gauge\ncollections_vector_total 860484\n# HELP memory_resident_bytes resident memory\n# TYPE memory_resident_bytes gauge\nmemory_resident_bytes 12345\n"
        }

        async fn collections_handler() -> axum::Json<Value> {
            axum::Json(json!({
                "result": {
                    "collections": [
                        {"name": "ann_benchmarks_test"}
                    ]
                },
                "status": "ok"
            }))
        }

        async fn missing_collection_handler() -> (StatusCode, axum::Json<Value>) {
            (
                StatusCode::NOT_FOUND,
                axum::Json(json!({
                    "status": "error",
                    "result": null
                })),
            )
        }

        async fn discovered_collection_handler() -> axum::Json<Value> {
            axum::Json(json!({
                "result": {
                    "optimizer_status": "ok",
                    "indexed_vectors_count": 0,
                    "points_count": 859520,
                    "segments_count": 30
                },
                "status": "ok"
            }))
        }

        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/collections", get(collections_handler))
            .route(
                "/collections/QdrantLocalCollection",
                get(missing_collection_handler),
            )
            .route(
                "/collections/ann_benchmarks_test",
                get(discovered_collection_handler),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let http = Client::new();
        let live = super::collect_qdrant_live_from(
            &format!("http://{}", addr),
            "QdrantLocalCollection",
            &http,
        )
        .await
        .unwrap();

        assert_eq!(
            live["effective_collection_code"].as_str(),
            Some("ann_benchmarks_test")
        );
        assert_eq!(live["points_count"].as_u64(), Some(859520));
        assert_eq!(live["segments_count"].as_u64(), Some(30));
        assert_eq!(live["collections_vector_total"].as_f64(), Some(860484.0));
        server.abort();
    }

    #[test]
    fn merge_keeps_richer_base_live_response_latency_when_thread_surface_is_empty() {
        let base_snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "live_response_latency": {
                        "current_session": {
                            "sample_count": 91,
                            "latency_slices": [
                                {"state": "hot", "sample_count": 47},
                                {"state": "cold", "sample_count": 44}
                            ]
                        },
                        "rolling_window": {
                            "sample_count": 92,
                            "latency_slices": [
                                {"state": "hot", "sample_count": 48},
                                {"state": "cold", "sample_count": 44}
                            ]
                        }
                    }
                }
            }
        });
        let thread_bound_snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "live_response_latency": {
                        "current_session": {
                            "sample_count": 0,
                            "latency_slices": []
                        },
                        "rolling_window": {
                            "sample_count": 0,
                            "latency_slices": []
                        }
                    }
                }
            }
        });

        let merged = merge_thread_bound_client_budget_snapshot_into_base_snapshot(
            &base_snapshot,
            &thread_bound_snapshot,
        );

        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["live_response_latency"]
                ["current_session"]["sample_count"]
                .as_u64(),
            Some(91)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["live_response_latency"]
                ["rolling_window"]["sample_count"]
                .as_u64(),
            Some(92)
        );
    }

    fn test_observe_state(cache: ObserveCache) -> ObserveState {
        ObserveState {
            dashboard_refresh_ms: 1000,
            cfg: test_config(),
            bind: "127.0.0.1:9464".to_string(),
            cache: Arc::new(RwLock::new(cache)),
        }
    }

    #[test]
    fn host_current_thread_control_window_targeting_allows_single_visible_target_thread() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let threads = vec![RecentClientThreadRecord {
            thread_id: "thread-amai".to_string(),
            cwd: "/home/art/agent-memory-index".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            title: "Amai".to_string(),
            agent_nickname: None,
            agent_role: None,
            model_provider: Some("openai".to_string()),
            model: Some("gpt-5.4".to_string()),
            reasoning_effort: Some("medium".to_string()),
            updated_at_epoch_s: 1_775_118_000,
        }];

        let summary = super::evaluate_host_current_thread_control_window_targeting(
            repo_root,
            "thread-amai",
            &threads,
        );

        assert_eq!(summary["status"], json!("allowed"));
        assert_eq!(summary["allowed"], json!(true));
        assert_eq!(summary["visible_model_bound_thread_count"], json!(1));
        assert_eq!(summary["target_thread_visible"], json!(true));
        assert_eq!(summary["target_thread_repo_root_match"], json!(true));
        assert!(summary["denial_reason"].is_null());
    }

    #[tokio::test]
    async fn dashboard_page_route_keeps_live_summary_contract_without_cached_bootstrap_payload() {
        let payload = json!({
            "meta": {
                "package_version": "0.1.0"
            },
            "headline": {
                "status": "pass",
                "status_label": "ok",
                "status_reason": "cached payload",
                "token_value": "5ч KPI: 1:1",
                "token_scope": ""
            },
            "hero_cards": [],
            "top_cards": [],
            "benchmark_cards": [],
            "machine_cards": [],
            "service_cards": [],
            "warnings": [],
            "glossary": [],
            "links": []
        });
        let mut cache = ObserveCache::default();
        cache.dashboard_payload = Some(payload);
        let app = Router::new()
            .route("/", get(dashboard_page_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let html = String::from_utf8(body.to_vec()).expect("html");
        assert!(html.contains("/api/dashboard-live-summary"));
        assert!(html.contains("syncDashboardLiveSummary"));
        assert!(!html.contains("/api/active-agent-budget-live"));
        assert!(!html.contains("syncActiveAgentBudgetLiveCard"));
        assert!(!html.contains("fetchActiveAgentBudgetLivePayload"));
        assert!(html.contains("const DASHBOARD_BOOTSTRAP_PAYLOAD = null;"));
        assert!(!html.contains("cached payload"));
    }

    #[tokio::test]
    async fn dashboard_live_summary_route_is_mounted_and_not_404() {
        let app = Router::new()
            .route(
                "/api/dashboard-live-summary",
                get(dashboard_live_summary_api_handler),
            )
            .with_state(test_observe_state(ObserveCache::default()));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard-live-summary")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let text = String::from_utf8(body.to_vec()).expect("body text");
        assert!(text.contains("\"status\":\"down\""));
    }

    #[tokio::test]
    async fn client_budget_live_route_returns_warmup_envelope_while_refresh_in_progress() {
        let mut cache = ObserveCache::default();
        cache.refresh_in_progress = true;

        let app = Router::new()
            .route(
                "/api/client-budget-live",
                get(super::client_budget_live_api_handler),
            )
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/client-budget-live")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("warmup payload");
        assert_eq!(payload["status"], json!("warming_up"));
        assert_eq!(payload["warmup_pending"], json!(true));
        assert_eq!(payload["rows"].as_array().map(Vec::len), Some(0));
    }

    #[tokio::test]
    async fn dashboard_live_summary_route_returns_warmup_envelope_while_refresh_in_progress() {
        let mut cache = ObserveCache::default();
        cache.refresh_in_progress = true;

        let app = Router::new()
            .route(
                "/api/dashboard-live-summary",
                get(super::dashboard_live_summary_api_handler),
            )
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard-live-summary")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("warmup payload");
        assert_eq!(payload["headline"]["status"], json!("waiting"));
        assert_eq!(payload["warmup_pending"], json!(true));
        assert_eq!(payload["top_cards"], json!([]));
    }

    #[tokio::test]
    async fn dashboard_route_returns_warmup_envelope_while_refresh_in_progress() {
        let mut cache = ObserveCache::default();
        cache.refresh_in_progress = true;

        let app = Router::new()
            .route("/api/dashboard", get(super::dashboard_api_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("warmup payload");
        assert_eq!(payload["headline"]["status"], json!("waiting"));
        assert_eq!(payload["service_cards"], json!([]));
        assert_eq!(
            payload["warnings"][0],
            json!("Панель ещё прогревается: первый live snapshot не готов.")
        );
    }

    #[tokio::test]
    async fn healthz_route_returns_up_envelope_for_ready_cache() {
        let mut cache = ObserveCache::default();
        cache.snapshot = Some(json!({
            "sla": {
                "summary": {
                    "critical": 0,
                    "unknown": 0
                }
            }
        }));
        cache.last_refresh_completed_epoch_ms = Some(super::now_epoch_ms());

        let app = Router::new()
            .route("/healthz", get(super::healthz_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("health payload");
        assert_eq!(payload["status"], json!("up"));
        assert_eq!(payload["critical"], json!(0));
        assert_eq!(payload["unknown"], json!(0));
        assert_eq!(payload["cache_stale"], json!(false));
    }

    #[tokio::test]
    async fn healthz_route_returns_down_envelope_when_cache_is_stale() {
        let mut cache = ObserveCache::default();
        cache.snapshot = Some(json!({
            "sla": {
                "summary": {
                    "critical": 0,
                    "unknown": 0
                }
            }
        }));

        let app = Router::new()
            .route("/healthz", get(super::healthz_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("health payload");
        assert_eq!(payload["status"], json!("down"));
        assert_eq!(payload["cache_stale"], json!(true));
    }

    #[tokio::test]
    async fn agent_display_name_route_rejects_empty_scope_before_db_lookup() {
        let app = Router::new()
            .route(
                "/api/agent-display-name",
                post(super::agent_display_name_update_api_handler),
            )
            .with_state(test_observe_state(ObserveCache::default()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agent-display-name")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"agent_scope":"   ","display_name":"Новый агент"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let text = String::from_utf8(body.to_vec()).expect("body text");
        assert!(text.contains("agent_scope is required"));
    }

    #[test]
    fn legacy_active_agent_payload_matches_live_summary_bundle_on_same_snapshot() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "observe_refresh": {
                "total_ms": 321u64,
                "stage_ms": {
                    "active_agent_budget": 44u64
                }
            },
            "sla": {
                "summary": {
                    "pass": 19,
                    "alert": 0,
                    "critical": 0,
                    "unknown": 0
                }
            },
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 1.0,
                        "target_p99_ms": 2.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 12.0,
                        "target_p99_ms": 13.0,
                        "target_max_ms": 15.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "headline": {
                        "title": "global fallback",
                        "value_percent": 12.0,
                        "scope_label": "fallback"
                    },
                    "current_session": {
                        "latency_slices": []
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": null
            },
            "agent_scope_activity": {
                "client_recent_window_minutes": 30,
                "client_recent_thread_count": 2,
                "client_recent_threads": [],
                "active_now_count": 2,
                "active_now_scopes": [],
                "recent_scope_window_hours": 24,
                "recent_scope_count": 2,
                "recent_scopes": []
            },
            "active_agent_budget": {
                "captured_at_epoch_ms": 1774239286000u64,
                "source": "live_active_agent_budget_surface",
                "headline": {
                    "title": "Средний KPI активных агентов",
                    "value_text": "5ч KPI: экономия 40.00%",
                    "scope_label": "среднее по 2 активным агентам"
                },
                "aggregate": {
                    "status": "observed",
                    "classification": "saving",
                    "reply_prefix": "5ч KPI: экономия 40.00%"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "thread_title": "Amai dashboard",
                        "cwd": "/home/art/agent-memory-index",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 60.00%",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 43.00%, 7д остаётся 72.00%",
                            "tooltip": "personal limit one"
                        }
                    },
                    {
                        "agent_label": "Hunter",
                        "agent_scope": "bug_bounty::continuity::default",
                        "thread_title": "Bug bounty",
                        "cwd": "/home/art/Bug-Bounty",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 20.00%",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 88.00%, 7д остаётся 91.00%",
                            "tooltip": "personal limit two"
                        }
                    }
                ]
            }
        });

        let legacy_payload = super::active_agent_budget_card_payload_from_snapshot(&snapshot)
            .expect("legacy active agent payload");
        let live_summary = crate::dashboard::build_live_summary_payload(
            &test_config(),
            &snapshot,
            "127.0.0.1:9464",
            1000,
        )
        .expect("live summary payload");

        assert_eq!(
            legacy_payload["card"]["value"].as_str(),
            live_summary["active_agent_card"]["value"].as_str()
        );
        assert_eq!(
            legacy_payload["card"]["agent_blocks"],
            live_summary["active_agent_card"]["agent_blocks"]
        );
    }

    #[test]
    fn host_current_thread_control_window_targeting_denies_ambiguous_multi_window_state() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let threads = vec![
            RecentClientThreadRecord {
                thread_id: "thread-amai".to_string(),
                cwd: "/home/art/agent-memory-index".to_string(),
                rollout_path: "/tmp/rollout-amai.jsonl".to_string(),
                title: "Amai".to_string(),
                agent_nickname: None,
                agent_role: None,
                model_provider: Some("openai".to_string()),
                model: Some("gpt-5.4".to_string()),
                reasoning_effort: Some("medium".to_string()),
                updated_at_epoch_s: 1_775_118_000,
            },
            RecentClientThreadRecord {
                thread_id: "thread-bounty".to_string(),
                cwd: "/home/art/Bug-Bounty".to_string(),
                rollout_path: "/tmp/rollout-bounty.jsonl".to_string(),
                title: "Bug Bounty".to_string(),
                agent_nickname: None,
                agent_role: None,
                model_provider: Some("openai".to_string()),
                model: Some("gpt-5.4".to_string()),
                reasoning_effort: Some("medium".to_string()),
                updated_at_epoch_s: 1_775_118_001,
            },
        ];

        let summary = super::evaluate_host_current_thread_control_window_targeting(
            repo_root,
            "thread-amai",
            &threads,
        );

        assert_eq!(summary["status"], json!("denied"));
        assert_eq!(summary["allowed"], json!(false));
        assert_eq!(summary["visible_model_bound_thread_count"], json!(2));
        assert_eq!(
            summary["denial_reason"],
            json!("ambiguous_multi_window_recent_threads")
        );
    }

    #[test]
    fn prometheus_token_rollups_export_verified_values_by_default() {
        let snapshot = json!({
            "latest_token_benchmark": {
                "token_benchmark": {
                    "naive_scope": { "token_count": 100.0 },
                    "context_pack": { "token_count": 20.0 },
                    "savings": {
                        "saved_tokens": 80.0,
                        "savings_factor": 5.0,
                        "savings_percent": 80.0
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "verified_effective_saved_tokens": 60.0,
                        "verified_effective_savings_pct": 75.0,
                        "total_saved_tokens": 90.0,
                        "savings_percent": 90.0,
                        "quality_ok_rate": 70.0,
                        "fallback_rate": 20.0,
                        "answer_like_rate": 50.0
                    },
                    "rolling_window": {
                        "verified_effective_saved_tokens": 120.0,
                        "verified_effective_savings_pct": 66.0,
                        "total_saved_tokens": 180.0,
                        "savings_percent": 88.0,
                        "quality_ok_rate": 80.0,
                        "fallback_rate": 10.0,
                        "answer_like_rate": 40.0
                    },
                    "lifetime": {
                        "verified_effective_saved_tokens": 240.0,
                        "verified_effective_savings_pct": 55.0,
                        "total_saved_tokens": 300.0,
                        "savings_percent": 77.0,
                        "quality_ok_rate": 90.0,
                        "fallback_rate": 5.0,
                        "answer_like_rate": 35.0
                    }
                }
            },
            "sla": {
                "summary": {
                    "pass": 1.0,
                    "alert": 0.0,
                    "critical": 0.0,
                    "unknown": 0.0
                }
            },
            "degradation_model": {
                "summary": {
                    "pass": 2.0,
                    "critical": 0.0,
                    "unknown": 9.0,
                    "fail_closed_total": 5.0,
                    "graceful_fallback_total": 6.0,
                    "evidence_gaps": 9.0
                }
            },
            "continuity_correctness_model": {
                "summary": {
                    "verified_probes": 9.0,
                    "failed_probes": 0.0,
                    "recovered_useful": 7.0,
                    "fail_closed": 2.0
                }
            }
        });

        let output = render_prometheus_metrics(&snapshot);
        assert!(output.contains("amai_tokens_saved_session_total 60"));
        assert!(output.contains("amai_tokens_savings_percent_window 66"));
        assert!(output.contains("amai_tokens_raw_saved_session_total 90"));
        assert!(output.contains("amai_tokens_quality_ok_rate_lifetime 90"));
        assert!(output.contains("amai_tokens_answer_like_rate_window 40"));
        assert!(output.contains("amai_degradation_unknown_total 9"));
        assert!(output.contains("amai_degradation_fail_closed_total 5"));
        assert!(output.contains("amai_degradation_graceful_fallback_total 6"));
        assert!(output.contains("amai_continuity_verified_probes_total 9"));
        assert!(output.contains("amai_continuity_recovered_useful_total 7"));
        assert!(output.contains("amai_continuity_fail_closed_total 2"));
    }

    #[test]
    fn merge_thread_bound_snapshot_preserves_full_dashboard_thresholds() {
        let base_snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001
                    }
                }
            },
            "sla": {
                "summary": {
                    "critical": 0
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "headline": {
                        "title": "5ч KPI: экономия",
                        "value_percent": 42.0
                    },
                    "current_session": {
                        "savings_percent": 25.0
                    },
                    "rolling_window": {
                        "verified_effective_saved_tokens": 7154
                    },
                    "lifetime": {
                        "verified_effective_saved_tokens": 13844711
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "same_meter_equivalent"
                            }
                        },
                        "rolling_window": {
                            "scope_label": "окно"
                        },
                        "lifetime": {
                            "scope_label": "всё время"
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "current_goal": "full snapshot"
                }
            }
        });
        let thread_bound_snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "current_session": {
                        "savings_percent": 70.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "same_meter_equivalent"
                            }
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "current_goal": "thread bound snapshot"
                }
            }
        });

        let merged = merge_thread_bound_client_budget_snapshot_into_base_snapshot(
            &base_snapshot,
            &thread_bound_snapshot,
        );

        assert_eq!(
            merged["thresholds"]["dashboard"]["timing_format"]["switch_to_nanoseconds_below_ms"]
                .as_f64(),
            Some(0.001)
        );
        assert_eq!(merged["sla"]["summary"]["critical"].as_u64(), Some(0));
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["current_session"]
                ["savings_percent"]
                .as_f64(),
            Some(70.0)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["headline"]["value_percent"]
                .as_f64(),
            Some(42.0)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["rolling_window"]
                ["verified_effective_saved_tokens"]
                .as_i64(),
            Some(7154)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["lifetime"]
                ["verified_effective_saved_tokens"]
                .as_i64(),
            Some(13844711)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["statement_previews"]
                ["rolling_window"]["scope_label"]
                .as_str(),
            Some("окно")
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["statement_previews"]["lifetime"]
                ["scope_label"]
                .as_str(),
            Some("всё время")
        );
        assert_eq!(
            merged["latest_repo_working_state_restore"]["working_state_restore"]["current_goal"]
                .as_str(),
            Some("thread bound snapshot")
        );
    }

    #[test]
    fn overlay_live_active_agent_surfaces_replaces_cached_agent_card_data() {
        let snapshot = json!({
            "agent_scope_activity": {
                "active_now_scopes": []
            },
            "active_agent_budget": {
                "agents": [
                    {
                        "agent_label": "Amai",
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 44.00%, 7д остаётся 23.00%"
                        }
                    }
                ]
            }
        });

        let merged = super::overlay_live_active_agent_surfaces(
            snapshot,
            json!({
                "active_now_scopes": [
                    { "owner_thread_id": "thread-amai" }
                ]
            }),
            json!({
                "agents": [
                    {
                        "agent_label": "Amai",
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 42.00%, 7д остаётся 22.00%"
                        }
                    }
                ]
            }),
        );

        assert_eq!(
            merged["active_agent_budget"]["agents"][0]["personal_client_limit"]["value_text"]
                .as_str(),
            Some("5ч остаётся 42.00%, 7д остаётся 22.00%")
        );
        assert_eq!(
            merged["agent_scope_activity"]["active_now_scopes"][0]["owner_thread_id"].as_str(),
            Some("thread-amai")
        );
    }

    #[test]
    fn normalized_thread_id_hint_rejects_empty_values() {
        assert_eq!(normalized_thread_id_hint(None), None);
        assert_eq!(normalized_thread_id_hint(Some("")), None);
        assert_eq!(normalized_thread_id_hint(Some("   ")), None);
    }

    #[test]
    fn normalized_thread_id_hint_trims_value() {
        assert_eq!(
            normalized_thread_id_hint(Some("  thread-123  ")),
            Some("thread-123")
        );
    }

    #[test]
    fn strict_auto_thread_binding_hint_uses_unique_active_thread() {
        let snapshot = json!({
            "agent_scope_activity": {
                "client_recent_threads": [
                    { "thread_id": "thread-live" },
                    { "thread_id": "thread-other" }
                ],
                "active_now_scopes": [
                    { "owner_thread_id": "thread-live" },
                    { "owner_thread_id": "thread-live" }
                ]
            }
        });

        assert_eq!(
            super::strict_auto_thread_binding_hint_from_snapshot(snapshot).as_deref(),
            Some("thread-live")
        );
    }

    #[test]
    fn strict_auto_thread_binding_hint_rejects_ambiguous_active_threads() {
        let snapshot = json!({
            "agent_scope_activity": {
                "client_recent_threads": [
                    { "thread_id": "thread-a" },
                    { "thread_id": "thread-b" }
                ],
                "active_now_scopes": [
                    { "owner_thread_id": "thread-a" },
                    { "owner_thread_id": "thread-b" }
                ]
            }
        });

        assert_eq!(
            super::strict_auto_thread_binding_hint_from_snapshot(snapshot),
            None
        );
    }

    #[test]
    fn strict_auto_thread_binding_hint_requires_matching_raw_client_thread() {
        let snapshot = json!({
            "agent_scope_activity": {
                "client_recent_threads": [
                    { "thread_id": "thread-other" }
                ],
                "active_now_scopes": [
                    { "owner_thread_id": "thread-live" }
                ]
            }
        });

        assert_eq!(
            super::strict_auto_thread_binding_hint_from_snapshot(snapshot),
            None
        );
    }

    #[test]
    fn continuity_correctness_model_reports_passing_probe_counts() {
        let payload = json!({
            "latest_continuity_verification": {
                "continuity_verification": {
                    "captured_at_epoch_ms": 4242,
                    "verification_status": "pass",
                    "probe_count": 9,
                    "verified_probes": 9,
                    "failed_probes": [],
                    "canonical_eval": {
                        "verdict_counts": {
                            "recovered_useful": 7,
                            "hit_correct_target": 2
                        }
                    }
                }
            }
        });

        let model = build_continuity_correctness_model(&payload).expect("continuity model");
        assert_eq!(model["summary"]["status"], json!("pass"));
        assert_eq!(model["summary"]["probe_count"], json!(9));
        assert_eq!(model["summary"]["verified_probes"], json!(9));
        assert_eq!(model["summary"]["failed_probes"], json!(0));
        assert_eq!(model["summary"]["recovered_useful"], json!(7));
        assert_eq!(model["summary"]["fail_closed"], json!(2));
        assert_eq!(model["summary"]["evidence_gap"], json!(false));
    }

    #[test]
    fn continuity_correctness_model_reports_gap_without_snapshot() {
        let payload = json!({
            "latest_continuity_verification": null
        });

        let model = build_continuity_correctness_model(&payload).expect("continuity model");
        assert_eq!(model["summary"]["status"], json!("unknown"));
        assert_eq!(model["summary"]["probe_count"], json!(0));
        assert_eq!(model["summary"]["evidence_gap"], json!(true));
    }

    #[test]
    fn client_live_meter_refresh_needed_when_rollout_is_newer_than_cache() {
        let snapshot = json!({
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "thread_id": "thread-1"
                }
            },
            "token_budget_report": {
                "client_live_meter": {
                    "thread_id": "thread-1",
                    "turn_id": "turn-1",
                    "ended_at_epoch_ms": 1000,
                    "client_turn_total_tokens": 120000,
                    "primary_limit_used_percent": 91,
                    "secondary_limit_used_percent": 28
                }
            }
        });
        let cached = cached_client_live_meter_state(&snapshot);
        let rollout = RolloutClientMeterObservation {
            thread_id: "thread-1".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_epoch_ms: 900,
            ended_at_epoch_ms: 1100,
            first_assistant_response_at_epoch_ms: None,
            client_turn_total_tokens: 122000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 93,
            latest_secondary_limit_used_percent: 29,
            latest_primary_window_duration_mins: None,
            latest_primary_resets_at_epoch_seconds: None,
            latest_secondary_window_duration_mins: None,
            latest_secondary_resets_at_epoch_seconds: None,
            rollout_jsonl_tolerance_summary: RolloutJsonlToleranceSummary::default(),
            observation_source: "codex_rollout_client_meter_v1".to_string(),
        };

        assert!(client_live_meter_refresh_needed(&cached, Some(&rollout)));
    }

    #[test]
    fn client_live_meter_refresh_needed_when_cached_thread_drifted_from_working_state() {
        let snapshot = json!({
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "thread_id": "thread-2"
                }
            },
            "token_budget_report": {
                "client_live_meter": {
                    "thread_id": "thread-1",
                    "turn_id": "turn-1",
                    "ended_at_epoch_ms": 1000,
                    "client_turn_total_tokens": 120000,
                    "primary_limit_used_percent": 91,
                    "secondary_limit_used_percent": 28
                }
            }
        });
        let cached = cached_client_live_meter_state(&snapshot);

        assert!(client_live_meter_refresh_needed(&cached, None));
    }

    #[test]
    fn active_agent_card_refresh_needed_when_rollout_drift_exceeds_10_seconds() {
        let rollout = RolloutClientMeterObservation {
            thread_id: "thread-bounty".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            turn_id: "turn-2".to_string(),
            started_at_epoch_ms: 1_000,
            ended_at_epoch_ms: 25_001,
            first_assistant_response_at_epoch_ms: None,
            client_turn_total_tokens: 150_000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200_000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 74,
            latest_secondary_limit_used_percent: 22,
            latest_primary_window_duration_mins: Some(300),
            latest_primary_resets_at_epoch_seconds: Some(123),
            latest_secondary_window_duration_mins: Some(10080),
            latest_secondary_resets_at_epoch_seconds: Some(456),
            rollout_jsonl_tolerance_summary: RolloutJsonlToleranceSummary::default(),
            observation_source: "codex_rollout_client_meter_v2".to_string(),
        };

        assert!(super::active_agent_card_refresh_needed_against_rollout(
            15_000, &rollout, 10_000,
        ));
    }

    #[test]
    fn active_agent_card_refresh_not_needed_within_10_second_drift_budget() {
        let rollout = RolloutClientMeterObservation {
            thread_id: "thread-amai".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            turn_id: "turn-2".to_string(),
            started_at_epoch_ms: 1_000,
            ended_at_epoch_ms: 25_000,
            first_assistant_response_at_epoch_ms: None,
            client_turn_total_tokens: 150_000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200_000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 55,
            latest_secondary_limit_used_percent: 77,
            latest_primary_window_duration_mins: Some(300),
            latest_primary_resets_at_epoch_seconds: Some(123),
            latest_secondary_window_duration_mins: Some(10080),
            latest_secondary_resets_at_epoch_seconds: Some(456),
            rollout_jsonl_tolerance_summary: RolloutJsonlToleranceSummary::default(),
            observation_source: "codex_rollout_client_meter_v2".to_string(),
        };

        assert!(!super::active_agent_card_refresh_needed_against_rollout(
            15_000, &rollout, 10_000,
        ));
    }

    #[test]
    fn degradation_model_reports_proven_and_gap_classes_honestly() {
        let payload = json!({
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 100,
                    "cross_project_leakage": 0,
                    "cross_namespace_leakage": 0,
                    "formal_invariants": [
                        { "name": "strict_local_visible_projects_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_projects", "pass": true },
                        { "name": "hostile_mixed_query_fail_closed", "pass": true },
                        { "name": "hostile_mixed_query_visible_projects_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_projects", "pass": true },
                        { "name": "strict_local_visible_namespaces_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "hostile_mixed_query_visible_namespaces_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_visible_projects_only", "pass": true },
                        { "name": "namespace_strict_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_fail_closed", "pass": true }
                    ]
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 200,
                    "restore_freshness_state": "fresh",
                    "restore_confidence": "medium"
                }
            }
        });

        let model = build_degradation_model(&payload).expect("degradation model");
        assert_eq!(model["summary"]["pass"], json!(2));
        assert_eq!(model["summary"]["unknown"], json!(9));
        assert_eq!(model["summary"]["evidence_gaps"], json!(9));

        let classes = model["classes"].as_array().expect("classes");
        let cross_project = classes
            .iter()
            .find(|item| item["class_key"].as_str() == Some("cross_project_scope"))
            .expect("cross_project_scope");
        assert_eq!(cross_project["status"], json!("pass"));

        let stale_handoff = classes
            .iter()
            .find(|item| item["class_key"].as_str() == Some("stale_handoff"))
            .expect("stale_handoff");
        assert_eq!(stale_handoff["status"], json!("unknown"));
        assert_eq!(
            stale_handoff["last_evidence_kind"],
            json!("working_state_restore")
        );
        assert_eq!(stale_handoff["evidence_gap"], json!(true));
    }

    #[test]
    fn degradation_model_promotes_classes_after_degradation_verification() {
        let payload = json!({
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 100,
                    "cross_project_leakage": 0,
                    "cross_namespace_leakage": 0,
                    "formal_invariants": [
                        { "name": "strict_local_visible_projects_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_projects", "pass": true },
                        { "name": "hostile_mixed_query_fail_closed", "pass": true },
                        { "name": "hostile_mixed_query_visible_projects_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_projects", "pass": true },
                        { "name": "strict_local_visible_namespaces_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "hostile_mixed_query_visible_namespaces_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_visible_projects_only", "pass": true },
                        { "name": "namespace_strict_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_fail_closed", "pass": true }
                    ]
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 200,
                    "restore_freshness_state": "fresh",
                    "restore_confidence": "medium"
                }
            },
            "latest_degradation_verification": {
                "degradation_verification": {
                    "captured_at_epoch_ms": 300,
                    "scenarios": [
                        {
                            "class_key": "cross_agent_scope",
                            "status": "pass",
                            "reason": "cross agent proof passed"
                        },
                        {
                            "class_key": "stale_handoff",
                            "status": "pass",
                            "reason": "stale handoff proof passed"
                        },
                        {
                            "class_key": "working_state_conflict",
                            "status": "pass",
                            "reason": "conflict proof passed"
                        }
                    ]
                }
            }
        });

        let model = build_degradation_model(&payload).expect("degradation model");
        assert_eq!(model["summary"]["pass"], json!(5));
        assert_eq!(model["summary"]["unknown"], json!(6));

        let classes = model["classes"].as_array().expect("classes");
        let cross_agent = classes
            .iter()
            .find(|item| item["class_key"].as_str() == Some("cross_agent_scope"))
            .expect("cross_agent_scope");
        assert_eq!(cross_agent["status"], json!("pass"));
        assert_eq!(
            cross_agent["last_evidence_kind"],
            json!("degradation_verification")
        );
        assert_eq!(cross_agent["evidence_gap"], json!(false));
    }

    #[test]
    fn contamination_check_flags_live_context_payload_in_benchmark_lane() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "_observability": {
                    "source_class": "live_context"
                },
                "load_verification": {
                    "record_live_context": true,
                    "publish_benchmark_snapshot": false
                }
            },
            "latest_retrieval_load_cold": null,
            "latest_retrieval_hot": null,
            "latest_retrieval_cold": null,
            "latest_cold_path_benchmark": null
        });
        assert_eq!(benchmark_contamination_value(&snapshot), Some(1.0));
    }

    #[test]
    fn contamination_check_stays_clean_for_benchmark_payload() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "_observability": {
                    "source_class": "benchmark"
                },
                "load_verification": {
                    "record_live_context": false,
                    "publish_benchmark_snapshot": true
                }
            },
            "latest_retrieval_load_cold": null,
            "latest_retrieval_hot": null,
            "latest_retrieval_cold": null,
            "latest_cold_path_benchmark": null
        });
        assert_eq!(benchmark_contamination_value(&snapshot), Some(0.0));
    }

    #[test]
    fn contamination_check_prefers_raw_lane_when_dashboard_falls_back_to_clean_snapshot() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "_observability": {
                    "source_class": "benchmark"
                },
                "load_verification": {
                    "record_live_context": false,
                    "publish_benchmark_snapshot": true
                }
            },
            "latest_retrieval_load_hot_raw": {
                "_observability": {
                    "source_class": "live_context"
                },
                "load_verification": {
                    "record_live_context": true,
                    "publish_benchmark_snapshot": false
                }
            },
            "latest_retrieval_load_cold": null,
            "latest_retrieval_load_cold_raw": null,
            "latest_retrieval_hot": null,
            "latest_retrieval_cold": null,
            "latest_cold_path_benchmark": null
        });
        assert_eq!(benchmark_contamination_value(&snapshot), Some(1.0));
    }

    #[test]
    fn select_latest_clean_benchmark_snapshot_skips_contaminated_latest_payload() {
        let contaminated = json!({
            "_observability": { "source_class": "live_context" },
            "load_verification": {
                "record_live_context": true,
                "publish_benchmark_snapshot": false,
                "qps": 2.0
            }
        });
        let clean = json!({
            "_observability": { "source_class": "benchmark" },
            "load_verification": {
                "record_live_context": false,
                "publish_benchmark_snapshot": true,
                "qps": 62000.0
            }
        });

        let selected = select_latest_clean_benchmark_snapshot(
            &[contaminated.clone(), clean.clone()],
            "load_verification",
        );
        assert_eq!(selected, Some(clean));
    }

    #[test]
    fn select_latest_dashboard_cold_benchmark_prefers_canonical_over_newer_proof() {
        let proof = json!({
            "_observability": { "source_class": "benchmark" },
            "cold_benchmark": {
                "profile": {
                    "display_name": "Local Proof Cold Contour",
                    "min_sample_count": 1000,
                    "min_repo_count": 75,
                    "min_query_slice_count": 200
                },
                "machine_readable_summary": {
                    "sample_count": 9,
                    "repo_count": 4,
                    "query_slice_count": 9
                }
            }
        });
        let canonical = json!({
            "_observability": { "source_class": "benchmark" },
            "cold_benchmark": {
                "profile": {
                    "display_name": "Large Real-Repos Cold Contour",
                    "min_sample_count": 1000,
                    "min_repo_count": 75,
                    "min_query_slice_count": 200
                },
                "machine_readable_summary": {
                    "sample_count": 1105,
                    "repo_count": 75,
                    "query_slice_count": 221
                }
            }
        });

        let selected =
            super::select_latest_dashboard_cold_benchmark_snapshot(&[proof, canonical.clone()]);
        assert_eq!(selected, Some(canonical));
    }

    #[test]
    fn retrieval_hot_sla_uses_configured_critical_threshold() {
        let profile = load_profile().expect("profile");
        let snapshot = json!({
            "postgres": {
                "connection_usage_ratio": 0.1,
                "query_probe_p95_ms": 1.0,
                "replica_lag_seconds": 0.0,
                "deadlocks_total": 0.0,
                "deadlocks_delta": 0.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            },
            "nats": {
                "publish_probe_p95_ms": 0.1,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.01
            },
            "latest_retrieval_cold": {
                "benchmark": {
                    "p95_ms": 2.0
                },
                "retrieval_runtime": {
                    "stage_p95_ms": {
                        "semantic_search_ms": 0.0
                    }
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "p95_ms": 7.0
                }
            },
            "latest_index_project": {
                "index_project": {
                    "parser_coverage_ratio": 1.0
                }
            }
        });

        let sla = evaluate_sla(&snapshot, &profile);
        let hot_check = sla["checks"]
            .as_array()
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|check| check["metric"].as_str() == Some("retrieval.hot_p95_ms"))
            })
            .expect("hot retrieval SLA check");
        assert_eq!(hot_check["target"].as_f64(), Some(1.0));
        assert_eq!(hot_check["alert"].as_f64(), Some(6.0));
        assert_eq!(hot_check["critical"].as_f64(), Some(10.0));
        assert_eq!(hot_check["status"].as_str(), Some("alert"));
    }

    #[test]
    fn postgres_historical_deadlock_total_without_fresh_delta_does_not_fail_sla() {
        let profile = load_profile().expect("profile");
        let snapshot = json!({
            "postgres": {
                "connection_usage_ratio": 0.1,
                "query_probe_p95_ms": 1.0,
                "replica_lag_seconds": 0.0,
                "deadlocks_total": 1.0,
                "deadlocks_delta": 0.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            },
            "nats": {
                "publish_probe_p95_ms": 0.1,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.01
            },
            "latest_retrieval_cold": {
                "benchmark": {
                    "p95_ms": 2.0
                },
                "retrieval_runtime": {
                    "stage_p95_ms": {
                        "semantic_search_ms": 0.0
                    }
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "p95_ms": 2.0
                }
            },
            "latest_index_project": {
                "index_project": {
                    "parser_coverage_ratio": 1.0
                }
            }
        });

        let sla = evaluate_sla(&snapshot, &profile);
        let deadlock_check = sla["checks"]
            .as_array()
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|check| check["metric"].as_str() == Some("postgres.deadlocks_delta"))
            })
            .expect("deadlock delta check");
        assert_eq!(deadlock_check["value"].as_f64(), Some(0.0));
        assert_eq!(deadlock_check["status"].as_str(), Some("pass"));
        assert_eq!(sla["summary"]["critical"].as_u64(), Some(0));
    }

    #[test]
    fn shared_client_budget_surfaces_cache_reuses_fresh_bundle() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-surfaces-cache-{}",
            Uuid::new_v4()
        ));
        let root_cause = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 10.00%"
                }
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500,
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "compact_current_thread_for_client_budget"
                }
            }
        });
        let cache =
            super::build_compact_client_budget_surfaces_cache(&root_cause, &gate, &guard, None);
        super::write_shared_compact_client_budget_surfaces(&temp_root, None, &cache)
            .expect("write shared cache");
        let loaded = super::load_shared_compact_client_budget_surfaces(&temp_root, 10_500, None)
            .expect("fresh shared cache");
        assert_eq!(
            loaded.cache_version,
            super::CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION
        );
        assert_eq!(
            loaded.root_cause["client_budget_reply_gate"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 10.00%")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn try_load_fast_thread_bound_materialized_compact_client_budget_surfaces_uses_cached_surfaces()
    {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-fast-thread-bound-surfaces-cache-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let root_cause = json!({
            "thread_binding_state": "current_thread_bound",
            "current_live_turn": {
                "status": "observed"
            },
            "host_current_thread_control_effect": {
                "effect_verdict": "same_thread_host_control_not_requested"
            },
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": observed_at_epoch_ms,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 10.00%"
                }
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": observed_at_epoch_ms,
                "reply_execution_gate": {
                    "action_kind": "rotate_chat_for_client_budget"
                }
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": observed_at_epoch_ms,
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "rotate_chat_for_client_budget"
                }
            }
        });
        let cache = super::build_compact_client_budget_surfaces_cache(
            &root_cause,
            &gate,
            &guard,
            Some("thread-current"),
        );
        super::write_shared_compact_client_budget_surfaces(
            &temp_root,
            Some("thread-current"),
            &cache,
        )
        .expect("write shared cache");
        let loaded = super::try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
            &temp_root,
            "thread-current",
        )
        .expect("fast thread-bound surfaces");
        assert_eq!(
            loaded.surfaces.root_cause_payload["client_budget_reply_gate"]["reply_execution_gate"]
                ["reply_prefix"],
            json!("5ч KPI: переплата 10.00%")
        );
        assert_eq!(
            loaded.gate.gate_payload["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_client_budget_surfaces_cache_fail_closed_when_stale() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-surfaces-cache-stale-{}",
            Uuid::new_v4()
        ));
        let root_cause = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache =
            super::build_compact_client_budget_surfaces_cache(&root_cause, &gate, &guard, None);
        super::write_shared_compact_client_budget_surfaces(&temp_root, None, &cache)
            .expect("write shared cache");
        let cache_path = super::client_budget_surfaces_shared_cache_path(&temp_root, None);
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(1u64);
        persisted["root_cause"]["client_budget_reply_gate"]["observed_at_epoch_ms"] = json!(1u64);
        persisted["gate"]["client_budget_reply_gate"]["observed_at_epoch_ms"] = json!(1u64);
        persisted["guard"]["observed_at_epoch_ms"] = json!(1u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite stale cache");
        assert!(
            super::load_shared_compact_client_budget_surfaces(
                &temp_root,
                super::CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS + 2,
                None,
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_snapshot_invalidation_roundtrips() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-snapshot-invalidation-{}",
            Uuid::new_v4()
        ));
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidated_at =
            super::load_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
                .expect("load invalidation");
        assert!(invalidated_at > 0);
        assert!(
            super::load_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-other")
                .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_roundtrips_when_fresh() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 0,
                            "with_amai_tokens": 0,
                            "saved_tokens": 0,
                            "saved_pct": 0.0
                        },
                        "matched_events_count": 0,
                        "retrieval_context_pack_count": 0
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let loaded = super::load_shared_thread_bound_budget_snapshot(
            &temp_root,
            super::current_epoch_ms_u64(),
            "thread-current",
        )
        .expect("load thread-bound snapshot");
        assert_eq!(
            loaded["token_budget_report"]["token_budget_report"]["surface"],
            json!("dashboard_current_session_budget_only")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_respects_invalidation() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-invalidated-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 0,
                            "with_amai_tokens": 0,
                            "saved_tokens": 0,
                            "saved_pct": 0.0
                        },
                        "matched_events_count": 0,
                        "retrieval_context_pack_count": 0
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let cache_path =
            super::thread_bound_budget_snapshot_shared_cache_path(&temp_root, "thread-current");
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(9_600u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidation_path = super::thread_bound_snapshot_invalidation_shared_cache_path(
            &temp_root,
            "thread-current",
        );
        let mut invalidation: serde_json::Value =
            serde_json::from_slice(&fs::read(&invalidation_path).expect("read invalidation"))
                .expect("parse invalidation");
        invalidation["invalidated_at_epoch_ms"] = json!(9_700u64);
        fs::write(
            &invalidation_path,
            serde_json::to_vec(&invalidation).expect("serialize invalidation"),
        )
        .expect("rewrite invalidation");
        assert!(
            super::load_shared_thread_bound_budget_snapshot(&temp_root, 10_000, "thread-current")
                .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn load_shared_budget_snapshot_preview_uses_thread_bound_snapshot_cache() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-budget-snapshot-preview-cache-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let loaded = super::load_shared_budget_snapshot_preview(&temp_root, Some("thread-current"))
            .expect("load shared budget snapshot preview");
        assert_eq!(
            loaded["token_budget_report"]["token_budget_report"]["surface"],
            json!("dashboard_current_session_budget_only")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_rejects_stale_exact_limit_surfaces() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-stale-exact-limits-{}",
            Uuid::new_v4()
        ));
        let stale_observed_at_epoch_ms =
            20_000u64.saturating_sub(super::COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS + 1);
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": stale_observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": stale_observed_at_epoch_ms
                        }
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let cache_path =
            super::thread_bound_budget_snapshot_shared_cache_path(&temp_root, "thread-current");
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(20_000u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");

        assert!(
            super::load_shared_thread_bound_budget_snapshot(&temp_root, 20_000, "thread-current")
                .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_rejects_unmaterialized_current_live_turn_exact_pair() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-exact-pair-gap-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "activity_observed_exact_pair_unavailable",
                        "exact_pair_available": false,
                        "matched_events_count": 1,
                        "retrieval_context_pack_count": 1
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        assert!(
            super::load_shared_thread_bound_budget_snapshot(
                &temp_root,
                super::current_epoch_ms_u64(),
                "thread-current",
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_accepts_no_amai_activity_exact_pair() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-no-amai-activity-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 0,
                            "with_amai_tokens": 0,
                            "saved_tokens": 0,
                            "saved_pct": 0.0
                        },
                        "matched_events_count": 0,
                        "retrieval_context_pack_count": 0
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let loaded = super::load_shared_thread_bound_budget_snapshot(
            &temp_root,
            super::current_epoch_ms_u64(),
            "thread-current",
        )
        .expect("load thread-bound snapshot");
        assert_eq!(
            loaded["token_budget_report"]["token_budget_report"]["current_live_turn"]["status"],
            json!("no_amai_activity_in_current_live_turn")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_client_budget_surfaces_cache_respects_invalidation() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-surfaces-cache-invalidated-{}",
            Uuid::new_v4()
        ));
        let root_cause = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache = super::build_compact_client_budget_surfaces_cache(
            &root_cause,
            &gate,
            &guard,
            Some("thread-current"),
        );
        super::write_shared_compact_client_budget_surfaces(
            &temp_root,
            Some("thread-current"),
            &cache,
        )
        .expect("write thread-bound shared cache");
        let cache_path =
            super::client_budget_surfaces_shared_cache_path(&temp_root, Some("thread-current"));
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(9_600u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidation_path = super::thread_bound_snapshot_invalidation_shared_cache_path(
            &temp_root,
            "thread-current",
        );
        let mut invalidation: serde_json::Value =
            serde_json::from_slice(&fs::read(&invalidation_path).expect("read invalidation"))
                .expect("parse invalidation");
        invalidation["invalidated_at_epoch_ms"] = json!(9_700u64);
        fs::write(
            &invalidation_path,
            serde_json::to_vec(&invalidation).expect("serialize invalidation"),
        )
        .expect("rewrite invalidation");
        assert!(
            super::load_shared_compact_client_budget_surfaces(
                &temp_root,
                10_000,
                Some("thread-current"),
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_active_thread_hint_roundtrips_when_fresh() {
        let temp_root =
            std::env::temp_dir().join(format!("amai-active-thread-hint-cache-{}", Uuid::new_v4()));
        super::write_shared_active_thread_hint(&temp_root, "thread-current")
            .expect("write active thread hint");
        let loaded =
            super::load_shared_active_thread_hint(&temp_root, super::current_epoch_ms_u64())
                .expect("fresh active thread hint");
        assert_eq!(loaded, "thread-current");
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_active_thread_hint_fails_closed_when_stale() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-active-thread-hint-cache-stale-{}",
            Uuid::new_v4()
        ));
        super::write_shared_active_thread_hint(&temp_root, "thread-current")
            .expect("write active thread hint");
        let cache_path = super::active_thread_hint_shared_cache_path(&temp_root);
        let mut persisted: super::PersistedActiveThreadHint =
            serde_json::from_slice(&fs::read(&cache_path).expect("read active thread hint"))
                .expect("decode active thread hint");
        persisted.updated_at_epoch_ms = 1;
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("encode active thread hint"),
        )
        .expect("rewrite active thread hint");
        assert!(
            super::load_shared_active_thread_hint(
                &temp_root,
                super::ACTIVE_THREAD_HINT_MAX_AGE_MS + 2,
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_client_budget_gate_cache_reuses_fresh_bundle() {
        let temp_root =
            std::env::temp_dir().join(format!("amai-client-budget-gate-cache-{}", Uuid::new_v4()));
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 10.00%"
                }
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget"
            }
        });
        let cache = super::build_compact_client_budget_gate_cache(&gate, &guard, None);
        super::write_shared_compact_client_budget_gate(&temp_root, None, &cache)
            .expect("write gate cache");
        let loaded = super::load_shared_compact_client_budget_gate(&temp_root, 10_500, None)
            .expect("gate cache");
        assert_eq!(
            loaded.cache_version,
            super::CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION
        );
        assert_eq!(
            loaded.gate["client_budget_reply_gate"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 10.00%")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn thread_bound_gate_payload_rejects_other_thread_feedback_confirmation() {
        let payload = json!({
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "confirm_same_thread_host_control_feedback",
                    "must_confirm_same_thread_host_control_feedback_before_reply": true,
                    "action_bundle": {
                        "host_current_thread_control": {
                            "effect_verdict": "other_thread",
                            "feedback_pending": true
                        }
                    }
                }
            }
        });

        assert!(
            !super::compact_thread_bound_client_budget_gate_payload_is_consistent(
                Some("thread-current"),
                &payload,
            )
        );
        assert!(
            super::compact_thread_bound_client_budget_gate_payload_is_consistent(None, &payload,)
        );
    }

    #[test]
    fn thread_bound_root_cause_payload_rejects_other_thread_feedback_confirmation() {
        let payload = json!({
            "thread_binding_state": "current_thread_bound",
            "current_live_turn": {
                "status": "no_amai_activity_in_current_live_turn"
            },
            "host_current_thread_control_effect": {
                "effect_verdict": "other_thread"
            },
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "confirm_same_thread_host_control_feedback",
                    "must_confirm_same_thread_host_control_feedback_before_reply": true,
                    "action_bundle": {
                        "host_current_thread_control": {
                            "feedback_pending": true
                        }
                    }
                }
            }
        });

        assert!(
            !super::compact_thread_bound_client_budget_root_cause_payload_is_consistent(
                Some("thread-current"),
                &payload,
            )
        );
        assert!(
            super::compact_thread_bound_client_budget_root_cause_payload_is_consistent(
                None, &payload,
            )
        );
    }

    #[test]
    fn shared_client_budget_gate_cache_fail_closed_when_stale() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-gate-cache-stale-{}",
            Uuid::new_v4()
        ));
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache = super::build_compact_client_budget_gate_cache(&gate, &guard, None);
        super::write_shared_compact_client_budget_gate(&temp_root, None, &cache)
            .expect("write gate cache");
        let cache_path = super::client_budget_gate_shared_cache_path(&temp_root, None);
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(1u64);
        persisted["gate"]["client_budget_reply_gate"]["observed_at_epoch_ms"] = json!(1u64);
        persisted["guard"]["observed_at_epoch_ms"] = json!(1u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite stale cache");
        assert!(
            super::load_shared_compact_client_budget_gate(
                &temp_root,
                super::CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS + 2,
                None,
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_client_budget_gate_cache_respects_invalidation() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-gate-cache-invalidated-{}",
            Uuid::new_v4()
        ));
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache =
            super::build_compact_client_budget_gate_cache(&gate, &guard, Some("thread-current"));
        super::write_shared_compact_client_budget_gate(&temp_root, Some("thread-current"), &cache)
            .expect("write gate cache");
        let cache_path =
            super::client_budget_gate_shared_cache_path(&temp_root, Some("thread-current"));
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(9_600u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidation_path = super::thread_bound_snapshot_invalidation_shared_cache_path(
            &temp_root,
            "thread-current",
        );
        let mut invalidation: serde_json::Value =
            serde_json::from_slice(&fs::read(&invalidation_path).expect("read invalidation"))
                .expect("parse invalidation");
        invalidation["invalidated_at_epoch_ms"] = json!(9_700u64);
        fs::write(
            &invalidation_path,
            serde_json::to_vec(&invalidation).expect("serialize invalidation"),
        )
        .expect("rewrite invalidation");
        assert!(
            super::load_shared_compact_client_budget_gate(
                &temp_root,
                10_000,
                Some("thread-current"),
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn postgres_fresh_deadlock_delta_trips_sla() {
        let profile = load_profile().expect("profile");
        let snapshot = json!({
            "postgres": {
                "connection_usage_ratio": 0.1,
                "query_probe_p95_ms": 1.0,
                "replica_lag_seconds": 0.0,
                "deadlocks_total": 2.0,
                "deadlocks_delta": 1.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            },
            "nats": {
                "publish_probe_p95_ms": 0.1,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.01
            },
            "latest_retrieval_cold": {
                "benchmark": {
                    "p95_ms": 2.0
                },
                "retrieval_runtime": {
                    "stage_p95_ms": {
                        "semantic_search_ms": 0.0
                    }
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "p95_ms": 2.0
                }
            },
            "latest_index_project": {
                "index_project": {
                    "parser_coverage_ratio": 1.0
                }
            }
        });

        let sla = evaluate_sla(&snapshot, &profile);
        let deadlock_check = sla["checks"]
            .as_array()
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|check| check["metric"].as_str() == Some("postgres.deadlocks_delta"))
            })
            .expect("deadlock delta check");
        assert_eq!(deadlock_check["value"].as_f64(), Some(1.0));
        assert_eq!(deadlock_check["status"].as_str(), Some("critical"));
        assert_eq!(sla["summary"]["critical"].as_u64(), Some(1));
    }

    #[test]
    fn retention_cleanup_marks_old_verify_token_events_as_expired() {
        let snapshot_id = Uuid::parse_str("00000000-0000-0000-0000-000000000042").expect("uuid");
        let candidates = vec![ObservabilityRetentionCandidate {
            snapshot_id,
            snapshot_kind: "token_budget_event".to_string(),
            payload: json!({
                "token_budget_event": {
                    "traffic_class": "verify"
                }
            }),
            source_kind: "verify_token_benchmark".to_string(),
            source_class: "operational".to_string(),
            created_at_epoch_ms: 0,
            captured_at_epoch_ms: Some(0),
        }];
        let expired =
            expired_retention_candidates(&candidates, 720_u64.saturating_mul(3_600_000) + 1)
                .expect("expired candidates");
        assert_eq!(expired.len(), 1);
        assert_eq!(
            expired[0]["retention_class"].as_str(),
            Some("synthetic_token_event")
        );
    }

    #[test]
    fn thresholds_surface_observability_policy_versions() {
        let profile = load_profile().expect("profile");
        let thresholds = profile_thresholds_json(&profile);
        assert_eq!(
            thresholds["observability"]["classification_rules_version"].as_str(),
            Some("observability-source-class-v2")
        );
        assert_eq!(
            thresholds["observability"]["schema_version"].as_u64(),
            Some(2)
        );
    }

    #[test]
    fn client_budget_guard_keeps_rotate_requirements_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "must_rotate_before_reply": true
            }
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_allows_reply_when_rotate_flags_are_clear() {
        let guard = json!({
            "reply_execution_gate": {
                "must_rotate_before_reply": false,
                "blocking": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": false
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_allows_reply_for_rotate_soon_advisory_only() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "must_rotate_before_reply": false,
                "blocking": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_keeps_global_budget_wait_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true,
                "must_wait_for_budget_recovery_before_reply": true
            },
            "requires_global_budget_recovery_before_reply": true
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_only_gate_fields() {
        let guard = json!({
            "status": "critical",
            "status_label": "глобальный лимит клиента почти исчерпан",
            "reply_prefix": "5ч KPI: переплата 2.46%",
            "reason": "heavy human explanation",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": false,
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "reason": "client_budget_guard_global_exhaustion",
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 2.46%",
                "reply_budget_contract": {
                    "host_context_compaction_inactive_target_pressure_active": true,
                    "same_meter_pure_burn_turn_active": true,
                    "must_prefer_short_paragraphs": true,
                    "must_avoid_commentary_only_updates": true,
                    "must_batch_all_tool_reads_before_reply": true,
                    "must_wait_for_meaningful_result_before_progress_reply": true,
                    "must_require_material_delta_before_next_reply": true,
                    "must_avoid_progress_reply_when_only_guard_changed": true,
                    "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                    "max_bullets_soft": 0,
                    "max_sentences_soft": 1,
                    "max_tool_roundtrips_soft": 0
                },
                "rotate_now": false,
                "rotate_soon": false,
                "blocking_reply_contract": {
                    "active": true,
                    "contract_version": working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION,
                    "response_kind": working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND,
                    "max_sentences": working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES,
                    "template": working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE,
                },
                "action_bundle": {
                    "preserves_return_obligation": false,
                }
            },
            "last_request": "heavy row",
            "tracked_slice": "heavy row",
            "client_limits": "heavy row"
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["status_label"].as_str(),
            Some("глобальный лимит клиента почти исчерпан")
        );
        assert_eq!(
            payload["reply_execution_gate"]["reply_budget_mode"].as_str(),
            Some("compact_high_signal")
        );
        assert_eq!(
            payload["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 2.46%")
        );
        assert_eq!(
            payload["reply_execution_gate"]["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 2.46%")
        );
        assert_eq!(
            payload["reply_execution_gate"]["preserves_return_obligation"].as_bool(),
            Some(false)
        );
        assert_eq!(
            payload["reply_execution_gate"]["same_meter_pure_burn_turn_active"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_prefer_short_paragraphs"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_avoid_commentary_only_updates"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_bullets_soft"],
            json!(0)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_sentences_soft"],
            json!(1)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_tool_roundtrips_soft"],
            json!(0)
        );
        assert!(payload.get("last_request").is_none());
        assert!(payload.get("tracked_slice").is_none());
        assert!(payload.get("client_limits").is_none());
        assert!(payload.get("reason").is_none());
        assert!(
            payload
                .get("requires_global_budget_recovery_before_reply")
                .is_none()
        );
        assert!(payload["reply_execution_gate"]["reply_budget_contract"].is_null());
        assert!(payload["reply_execution_gate"]["action_bundle"].is_null());
        assert!(payload["reply_execution_gate"]["blocking_reply_contract"].is_null());
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_blocking_contract_null_when_not_blocking() {
        let guard = json!({
            "status": "alert",
            "status_label": "цель >90% не достигнута",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": false,
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "reason": "client_budget_guard_clear",
                "action_kind": "continue_current_chat",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": "compact_high_signal",
                "rotate_now": false,
                "rotate_soon": false,
                "preserves_return_obligation": true,
                "blocking_reply_contract": {
                    "active": false,
                    "template": "should not leak"
                }
            }
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert!(payload["reply_execution_gate"]["blocking_reply_contract"].is_null());
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_compact_rotate_action_bundle() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: экономия 39.49%",
            "observed_at_epoch_ms": 1774789858128u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "critical_regrowth",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": true,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: экономия 39.49%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "host_current_thread_control": {
                        "available": true,
                        "control_kind": "hotkey_window_open_current",
                        "command_id": "hotkey-window-open-current",
                        "automation_ready": false
                    },
                    "operator_flow": {
                        "primary_command_kind": "same_thread_host_control_launch_command",
                        "primary_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--compact-window' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--compact-window' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'",
                        "rotate_helper_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'",
                        "startup_command": "'amai' 'continuity' 'startup' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index' '--token-source-kind' 'live_continuity_startup' '--json'"
                    }
                }
            }
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["bundle_version"],
            json!("rotate-chat-action-bundle-v1")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("same_thread_host_control_launch_command")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            payload["reply_execution_gate"]["host_context_compaction_stage"],
            json!("critical_regrowth")
        );
        assert_eq!(
            payload["reply_execution_gate"]["host_context_compaction_critical_regrowth_active"],
            json!(true)
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command"]
                .as_str()
                .unwrap_or_default()
                .contains("ctl-launch")
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]
                .get("host_current_thread_control_launch_command")
                .is_none()
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["startup_command"]
                .as_str()
                .unwrap_or_default()
                .contains("live_continuity_startup")
        );
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_same_thread_wait_metadata() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 32.34%",
            "observed_at_epoch_ms": 1774831991851u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "critical_regrowth",
                "must_confirm_same_thread_host_control_feedback_before_reply": true,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": true,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 32.34%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "feedback_confirmation_before_retry_required": true,
                    "order": [
                        "confirm_same_thread_host_control_feedback",
                        "measure_existing_same_thread_effect",
                        "fallback_rotate_chat"
                    ],
                    "host_current_thread_control": {
                        "available": true,
                        "control_kind": "thread_overlay_open_current",
                        "command_id": "thread-overlay-open-current",
                        "retry_allowed": false
                    },
                    "operator_flow": {
                        "primary_command_kind": "confirm_same_thread_host_control_feedback",
                        "primary_command": null,
                        "same_thread_feedback_confirmation_required": true,
                        "same_thread_feedback_confirmation_summary": "Requested same-thread overlay launch via host current-thread control.",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'"
                    }
                }
            }
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["feedback_confirmation_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["order"][0],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_confirm_same_thread_host_control_feedback_before_reply"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"],
            json!(true)
        );
    }

    #[test]
    fn compact_host_control_client_budget_reply_gate_stays_small_and_machine_readable() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 12.34%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true
            },
            "observed_at_epoch_ms": 1774850926868u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 12.34%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "host_current_thread_control": {
                        "available": true,
                        "command_id": "hotkey-window-open-current"
                    },
                    "operator_flow": {
                        "primary_command_kind": "same_thread_host_control_launch_command",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--compact-window'"
                    }
                }
            },
            "long_prose_field": "this should not survive"
        });
        let payload = super::compact_host_control_client_budget_reply_gate(&guard);
        assert_eq!(payload["reply_prefix"], json!("5ч KPI: переплата 12.34%"));
        assert_eq!(payload["status_label"], json!("сожми текущий чат сейчас"));
        assert_eq!(
            payload["reply_execution_gate"]["action_kind"],
            json!("compact_current_thread_for_client_budget")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert!(payload.get("observed_at_epoch_ms").is_none());
        assert!(payload.get("max_guard_age_seconds").is_none());
        assert!(payload.get("long_prose_field").is_none());
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_trims_same_thread_surface_but_keeps_commands() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 11.11%",
            "host_context_compaction": {
                "stage": "critical_regrowth",
                "current_thread_bound": true,
                "current_turn_total_tokens": 120000,
                "growth_since_compaction_tokens": 70000,
                "regrowth_of_recovered_surface_ratio": 0.42,
                "critical_regrowth_active": true,
                "preserve_active": true,
                "long_note": "drop me"
            },
            "observed_at_epoch_ms": 1774852000000u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "critical_regrowth",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": true,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 11.11%",
                "reply_budget_contract": {
                    "must_prefer_short_paragraphs": true,
                    "must_avoid_commentary_only_updates": true,
                    "max_bullets_soft": 0,
                    "max_sentences_soft": 1,
                    "max_tool_roundtrips_soft": 0
                },
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "feedback_confirmation_before_retry_required": true,
                    "order": [
                        "confirm_same_thread_host_control_feedback",
                        "measure_existing_same_thread_effect",
                        "fallback_rotate_chat"
                    ],
                    "host_current_thread_control": {
                        "available": true,
                        "automation_ready": true,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "critical_regrowth",
                        "feedback_pending": true,
                        "measurement_pending": true,
                        "retry_allowed": false,
                        "retry_blocked_reason": "pending feedback",
                        "effect_verdict": "measurement_pending",
                        "effect_summary": "summary",
                        "selection_reason": "protect_recent_host_compaction_gain",
                        "external_uri_launch": {
                            "uri": "vscode://too-big"
                        },
                        "alternate_controls": [
                            {
                                "button_label": "Open thread overlay",
                                "command_id": "thread-overlay-open-current",
                                "control_kind": "thread_overlay_open_current",
                                "note": "drop me"
                            }
                        ]
                    },
                    "operator_flow": {
                        "primary_command_kind": "confirm_same_thread_host_control_feedback",
                        "host_current_thread_control_launch_command": "launch",
                        "rotate_helper_command": "rotate"
                    }
                }
            }
        });
        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        assert_eq!(payload["status_label"], json!("сожми текущий чат сейчас"));
        assert!(payload.get("reply_prefix").is_none());
        assert!(payload.get("host_context_compaction").is_none());
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["rotate_helper_command"],
            json!("rotate")
        );
        assert_eq!(
            payload["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 11.11%")
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_prefer_short_paragraphs"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_avoid_commentary_only_updates"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_bullets_soft"],
            json!(0)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_sentences_soft"],
            json!(1)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_tool_roundtrips_soft"],
            json!(0)
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                ["host_current_thread_control"]["command_id"]
                .as_str()
                == Some("hotkey-window-open-current")
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                .get("order")
                .is_none()
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_drops_long_same_thread_details_after_verified_failure()
     {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 204.20%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true,
                "current_turn_total_tokens": 77335,
                "growth_since_compaction_tokens": 28228,
                "regrowth_of_recovered_surface_ratio": 0.15,
                "critical_regrowth_active": false,
                "preserve_active": true
            },
            "observed_at_epoch_ms": 1774854192111u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 204.20%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "order": [
                        "run_rotate_helper",
                        "open_fresh_chat",
                        "run_continuity_startup"
                    ],
                    "host_current_thread_control": {
                        "available": false,
                        "automation_ready": false,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "preserve",
                        "feedback_pending": false,
                        "measurement_pending": false,
                        "retry_allowed": false,
                        "retry_blocked_reason": "Same-thread compact window локально меняет thread, но полный 5ч burn всё ещё идёт хуже идеального темпа на +12.68 п.п.; rotate fallback should become primary.",
                        "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                        "effect_summary": "Очень длинный exhausted summary, который больше не должен тащиться в CLI payload после verified failure.",
                        "selection_reason": "protect_recent_host_compaction_gain",
                        "availability_state": "exhausted_after_verified_failure",
                        "surface_exhausted_after_verified_failure": true,
                        "alternate_controls": [
                            {
                                "button_label": "Open thread overlay",
                                "command_id": "thread-overlay-open-current",
                                "control_kind": "thread_overlay_open_current"
                            }
                        ]
                    },
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command",
                        "rotate_helper_command": "rotate"
                    }
                }
            }
        });
        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["availability_state"],
            json!("exhausted_after_verified_failure")
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
                .get("effect_summary")
                .is_none()
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_normalizes_commands_and_deduplicates_primary_rotate_command()
     {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 50.00%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true,
                "current_turn_total_tokens": 50000,
                "growth_since_compaction_tokens": 12000,
                "regrowth_of_recovered_surface_ratio": 0.2,
                "critical_regrowth_active": false,
                "preserve_active": true
            },
            "observed_at_epoch_ms": 1774854192111u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 50.00%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "order": [
                        "run_rotate_helper",
                        "open_fresh_chat",
                        "run_continuity_startup"
                    ],
                    "host_current_thread_control": {
                        "available": false,
                        "automation_ready": false,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "preserve",
                        "feedback_pending": false,
                        "measurement_pending": false,
                        "retry_allowed": false,
                        "retry_blocked_reason": "rotate fallback already primary",
                        "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                        "selection_reason": "protect_recent_host_compaction_gain"
                    },
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command",
                        "primary_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai'",
                        "rotate_helper_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai'",
                        "startup_command": "'amai' 'continuity' 'startup' '--project' 'amai' '--runtime-state-json'"
                    }
                }
            }
        });

        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        let operator_flow = &payload["reply_execution_gate"]["action_bundle"]["operator_flow"];
        assert!(operator_flow.get("primary_command").is_none());
        assert!(operator_flow.get("handoff_command").is_none());
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                .get("order")
                .is_none()
        );
        assert_eq!(
            operator_flow["rotate_helper_command"],
            json!("amai continuity rotate-chat --project amai")
        );
        assert!(operator_flow.get("startup_command").is_none());
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_deduplicates_same_thread_primary_command() {
        let guard = json!({
            "status": "alert",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 10.00%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true,
                "current_turn_total_tokens": 50000,
                "growth_since_compaction_tokens": 12000,
                "regrowth_of_recovered_surface_ratio": 0.2,
                "critical_regrowth_active": false,
                "preserve_active": true
            },
            "observed_at_epoch_ms": 1774854192111u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 10.00%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "order": [
                        "run_same_thread_host_control",
                        "confirm_surface_effect",
                        "fallback_rotate_chat"
                    ],
                    "host_current_thread_control": {
                        "available": true,
                        "automation_ready": true,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "preserve",
                        "feedback_pending": false,
                        "measurement_pending": false,
                        "retry_allowed": true,
                        "effect_verdict": "measurement_pending",
                        "selection_reason": "protect_recent_host_compaction_gain"
                    },
                    "operator_flow": {
                        "primary_command_kind": "same_thread_host_control_launch_command",
                        "primary_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current'",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current'",
                        "rotate_helper_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai'",
                        "startup_command": "'amai' 'continuity' 'startup' '--project' 'amai' '--runtime-state-json'"
                    }
                }
            }
        });

        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        let operator_flow = &payload["reply_execution_gate"]["action_bundle"]["operator_flow"];
        assert_eq!(
            operator_flow["primary_command"],
            json!("amai observe ctl-launch --thread-id thread-current")
        );
        assert!(
            payload["reply_execution_gate"]
                .get("must_confirm_same_thread_host_control_feedback_before_reply")
                .is_none()
        );
        assert!(
            operator_flow
                .get("host_current_thread_control_launch_command")
                .is_none()
        );
        assert!(operator_flow.get("handoff_command").is_none());
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                .get("order")
                .is_none()
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
    }

    #[test]
    fn compact_current_session_budget_guard_payload_drops_prose_and_operator_flow() {
        let guard = json!({
            "status_label": "сожми текущий чат сейчас",
            "status_tooltip": "long prose",
            "reason": "long prose",
            "note": "long prose",
            "full_turn_savings_proven": true,
            "full_turn_savings_percent": "0.00%",
            "should_rotate_chat_now": true,
            "should_rotate_chat_soon": true,
            "requires_global_budget_recovery_before_reply": false,
            "next_action": "rotate now",
            "last_request": "154048 из 258400",
            "client_limits": "5ч остаётся 69%",
            "tracked_slice": "экономия 490",
            "tracked_slice_truth": "учтённая часть",
            "client_live_meter_current_thread_bound": true,
            "client_live_meter_thread_binding_state": "current_thread_bound",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "reason": "client_budget_guard_pressure",
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": true,
                "must_rotate_before_reply": true,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": "compact_high_signal",
                "rotate_now": true,
                "rotate_soon": true,
                "reply_budget_contract": {
                    "active": true
                },
                "action_bundle": {
                    "operator_flow": {
                        "copy_paste_ready": true
                    },
                    "preserves_return_obligation": false
                }
            }
        });
        let payload = super::compact_current_session_budget_guard_payload(&guard);
        assert_eq!(
            payload["status_label"].as_str(),
            Some("сожми текущий чат сейчас")
        );
        assert_eq!(payload["full_turn_savings_percent"].as_str(), Some("0.00%"));
        assert_eq!(payload["last_request"].as_str(), Some("154048 из 258400"));
        assert_eq!(payload["client_limits"].as_str(), Some("5ч остаётся 69%"));
        assert!(payload.get("status_tooltip").is_none());
        assert!(payload.get("reason").is_none());
        assert!(payload.get("note").is_none());
        assert!(payload["reply_execution_gate"]["reply_budget_contract"].is_null());
        assert!(payload["reply_execution_gate"]["action_bundle"].is_null());
    }

    #[test]
    fn compact_client_budget_root_cause_payload_trims_long_same_thread_effect_fields() {
        let payload = json!({
            "status": "observed",
            "reply_prefix": "5ч KPI: переплата 182.29%",
            "thread_binding_state": "current_thread_bound",
            "current_thread_bound": true,
            "current_live_meter": {
                "client_turn_total_tokens": 169873,
                "context_used_percent": 65.74
            },
            "current_live_turn": {
                "status": "no_amai_activity_in_current_live_turn",
                "saved_pct": 0.0
            },
            "same_meter_economics": {
                "strict_lower_bound_tokens": 182,
                "same_meter_without_amai_tokens": 182,
                "same_meter_with_amai_tokens": 72,
                "same_meter_saved_tokens": 110,
                "same_meter_saved_pct": 60.43956043956044,
                "continuity_restore_baseline_tokens": 178,
                "continuity_restore_observed_tokens": 68,
                "continuity_restore_delta_tokens": -110,
                "full_turn_overhang_tokens": 169691,
                "full_turn_vs_strict_ratio": 933.3681318681319,
                "dominant_cost_surface": "giant_thread_context_outside_same_meter_slice"
            },
            "exact_pair_status": {
                "state": "not_applicable_current_live_turn_has_no_amai_activity"
            },
            "guard": {
                "status_label": "сожми текущий чат сейчас",
                "action_kind": "rotate_chat_for_client_budget"
            },
            "host_context_compaction": {
                "stage": "critical_regrowth",
                "current_thread_bound": true,
                "current_turn_total_tokens": 169873,
                "growth_since_compaction_tokens": 120766,
                "regrowth_of_recovered_surface_ratio": 0.65,
                "critical_regrowth_active": true,
                "preserve_active": true,
                "long_note": "drop me"
            },
            "host_current_thread_control_effect": {
                "command_id": "hotkey-window-open-current",
                "surface_label": "compact window",
                "current_stage": "critical_regrowth",
                "thread_id": "thread-current",
                "measurement_pending": false,
                "measurement_sufficient": true,
                "retry_allowed": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "full_scale_client_burn_worsened": true,
                "rotate_fallback_recommended": true,
                "verified_host_compaction_observed_after_feedback": true,
                "compaction_count_delta": 2,
                "primary_limit_used_overrun_percent_points": 12.68,
                "turn_token_delta": -58073,
                "context_used_percent_point_delta": -22.47,
                "regrowth_since_feedback_tokens": -59781,
                "summary": "Long prose summary that should not survive compact CLI root-cause output.",
                "note": "Long prose note that should also be dropped."
            },
            "measured_components": ["client_prompt"],
            "missing_components": [],
            "blocking_reasons": []
        });

        let guard = json!({
            "status_label": "сожми текущий чат сейчас",
            "observed_at_epoch_ms": 1774862518426u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL,
                "reply_prefix": "5ч KPI: переплата 182.29%",
                "host_context_compaction_stage": "critical_regrowth",
                "host_context_compaction_preserve_active": true,
                "host_context_compaction_critical_regrowth_active": true,
                "reply_budget_contract": {
                    "host_context_compaction_inactive_target_pressure_active": true,
                    "same_meter_pure_burn_turn_active": true,
                    "must_prefer_short_paragraphs": true,
                    "must_avoid_commentary_only_updates": true,
                    "must_batch_all_tool_reads_before_reply": true,
                    "must_wait_for_meaningful_result_before_progress_reply": true,
                    "must_require_material_delta_before_next_reply": true,
                    "must_avoid_progress_reply_when_only_guard_changed": true,
                    "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                    "max_bullets_soft": 0,
                    "max_sentences_soft": 1,
                    "max_tool_roundtrips_soft": 0
                },
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "host_current_thread_control": {
                        "command_id": "hotkey-window-open-current",
                        "button_label": "Open compact window",
                        "automation_ready": true,
                        "retry_allowed": false,
                        "summary": "Same-thread compact window",
                        "note": "internal host surface"
                    },
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command",
                        "rotate_helper_command": "amai continuity rotate-chat",
                        "startup_command": "./scripts/continuity_startup.sh --json"
                    }
                }
            }
        });

        let compact = super::compact_client_budget_root_cause_payload(&payload, Some(&guard));

        assert_eq!(
            compact["host_current_thread_control_effect"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            compact["host_context_compaction"]["stage"],
            json!("critical_regrowth")
        );
        assert_eq!(
            compact["host_context_compaction"]["regrowth_of_recovered_surface_ratio"],
            json!(0.65)
        );
        assert!(
            compact["host_context_compaction"]
                .get("current_turn_total_tokens")
                .is_none()
        );
        assert_eq!(
            compact["current_live_meter"]["context_used_percent"],
            json!(65.74)
        );
        assert!(
            compact["current_live_turn"]
                .get("exact_pair_available")
                .is_none()
        );
        assert_eq!(
            compact["same_meter_economics"]["strict_lower_bound_tokens"],
            json!(182)
        );
        assert_eq!(
            compact["same_meter_economics"]["same_meter_saved_pct"],
            json!(60.44)
        );
        assert_eq!(
            compact["same_meter_economics"]["full_turn_vs_strict_ratio"],
            json!(933.37)
        );
        assert_eq!(
            compact["same_meter_economics"]["dominant_cost_surface"],
            json!("giant_thread_context_outside_same_meter_slice")
        );
        assert!(compact.get("exact_pair_status").is_none());
        assert!(
            compact["current_live_meter"]
                .get("ended_at_epoch_ms")
                .is_none()
        );
        assert!(compact["guard"].get("action_kind").is_none());
        assert_eq!(compact["guard"]["should_rotate_chat_now"], json!(null));
        assert!(
            compact["host_current_thread_control_effect"]
                .get("summary")
                .is_none()
        );
        assert!(
            compact["host_current_thread_control_effect"]
                .get("note")
                .is_none()
        );
        assert!(
            compact["host_context_compaction"]
                .get("long_note")
                .is_none()
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert!(
            compact["client_budget_reply_gate"]["status_label"].is_null()
                || compact["client_budget_reply_gate"]
                    .get("status_label")
                    .is_none()
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["operator_flow"]
                ["primary_command_kind"],
            json!("rotate_helper_command")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["operator_flow"]
                ["rotate_helper_command"],
            json!("amai continuity rotate-chat")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
                ["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
                ["button_label"],
            json!("Open compact window")
        );
        assert!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["operator_flow"]
                .get("startup_command")
                .is_none()
        );
        assert!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]
                .get("host_context_compaction_stage")
                .is_none()
        );
        assert!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]
                .get("preserves_return_obligation")
                .is_none()
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["same_meter_pure_burn_turn_active"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["must_prefer_short_paragraphs"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["must_avoid_commentary_only_updates"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["max_bullets_soft"],
            json!(0)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["max_sentences_soft"],
            json!(1)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["max_tool_roundtrips_soft"],
            json!(0)
        );
    }

    #[test]
    fn compact_working_state_restore_for_budget_keeps_only_budget_critical_fields() {
        let restore = json!({
            "client_budget_target_percent": 50,
            "thread_id": "thread-1",
            "current_goal": "goal",
            "next_step": "next",
            "execctl_resume_state": "pending_return_queue_present",
            "project": {
                "code": "amai",
                "repo_root": "/home/art/agent-memory-index",
                "display_name": "Amai"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "state_lineage": {
                "authoritative_event_id": "handoff-1",
                "authoritative_source_kind": "continuity_handoff",
                "authoritative_local_path": "/tmp/HANDOFF.md",
                "drop_me": "nope"
            },
            "recent_actions": [
                {
                    "source_kind": working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND,
                    "summary": "feedback summary",
                    "recorded_at_epoch_ms": 123,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED,
                        "command_id": working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID,
                        "feedback_snapshot": {
                            "thread_id": "thread-1",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100,
                                "context_used_percent": 12.5,
                                "primary_limit_used_percent": 7,
                                "secondary_limit_used_percent": 9
                            },
                            "host_context_compaction": {
                                "compaction_count": 2,
                                "growth_since_compaction_tokens": 30,
                                "compacted_at_epoch_ms": 456,
                                "stage": "preserve",
                                "note": "drop me"
                            },
                            "unrelated": "drop me"
                        }
                    },
                    "query": "drop me"
                },
                {
                    "source_kind": "continuity_handoff",
                    "summary": "unrelated"
                }
            ],
            "open_questions": ["drop me"],
            "materialized_notes": ["drop me"]
        });

        let compact = super::compact_working_state_restore_for_budget(&restore);

        assert_eq!(compact["client_budget_target_percent"], json!(50));
        assert_eq!(compact["thread_id"], json!("thread-1"));
        assert_eq!(compact["project"]["code"], json!("amai"));
        assert_eq!(
            compact["project"]["repo_root"],
            json!("/home/art/agent-memory-index")
        );
        assert!(compact["project"].get("display_name").is_none());
        assert_eq!(compact["namespace"]["code"], json!("continuity"));
        assert!(compact["namespace"].get("display_name").is_none());
        assert_eq!(
            compact["state_lineage"]["authoritative_event_id"],
            json!("handoff-1")
        );
        assert_eq!(
            compact["state_lineage"]["authoritative_source_kind"],
            json!("continuity_handoff")
        );
        assert_eq!(
            compact["state_lineage"]["authoritative_local_path"],
            json!("/tmp/HANDOFF.md")
        );
        assert!(compact["state_lineage"].get("drop_me").is_none());
        assert_eq!(compact["recent_actions"].as_array().unwrap().len(), 1);
        assert_eq!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]["feedback_snapshot"]
                ["client_live_meter"]["client_turn_total_tokens"],
            json!(100)
        );
        assert!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]["feedback_snapshot"]
                .get("unrelated")
                .is_none()
        );
        assert!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]
                .get("secondary_limit_used_percent")
                .is_none()
        );
        assert!(compact.get("open_questions").is_none());
        assert!(compact.get("materialized_notes").is_none());
    }

    #[test]
    fn compact_budget_snapshot_preview_payload_trims_hourly_burn_and_alignment_to_essential_fields()
    {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_budget_target_percent": 50,
                    "current_session": {
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "counted_events": 3,
                        "live_events_count": 4,
                        "total_saved_tokens": 5,
                        "savings_percent": 12.3456,
                        "effective_savings_pct": 23.4567,
                        "observed_client_prompt_tokens": 10,
                        "observed_tool_overhead_tokens": 11,
                        "observed_continuity_restore_tokens": 12,
                        "observed_assistant_generation_tokens": 13,
                        "age_ms_since_latest": 14,
                        "model_version": "drop me"
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "same_meter_equivalent",
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": "exact_pair_materialized",
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 182
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 182,
                                    "component_semantics": [
                                        {
                                            "code": "client_prompt",
                                            "baseline_measured_tokens": 4,
                                            "observed_tokens": 4,
                                            "whole_cycle_observed_complete": true,
                                            "note": "drop me"
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 178,
                                            "observed_tokens": 68,
                                            "whole_cycle_observed_complete": true,
                                            "note": "drop me"
                                        }
                                    ]
                                },
                                "blocking_reasons": ["x"],
                                "measured_components": ["client_prompt"],
                                "missing_components": ["tool_overhead"],
                                "not_applicable_components": ["retrieval_quality"],
                                "note": "drop me",
                                "model_version": "drop me",
                                "surface_kind": "drop me"
                            },
                            "observed_whole_cycle_with_amai_tokens": 72,
                            "verified_observed_whole_cycle_with_amai_tokens": 70,
                            "with_amai_measured_tokens": 72,
                            "verified_with_amai_measured_tokens": 70
                        }
                    },
                    "client_limit_hourly_burn": {
                        "status": "overspend",
                        "reply_prefix": "5ч KPI: переплата 12.34%",
                        "kpi_percent": 12.3456,
                        "actual_used_percent": 67.891,
                        "actual_remaining_percent": 32.109,
                        "ideal_used_percent": 44.444,
                        "ideal_remaining_percent": 55.556,
                        "projected_primary_used_per_hour_percent": 14.789,
                        "ideal_primary_used_per_hour_percent": 10.111,
                        "latest_observed_at_epoch_ms": 100,
                        "latest_live_age_seconds": 1.987,
                        "summary": "drop me",
                        "status_bar_correlated": true,
                        "window_minutes": 123
                    },
                    "client_live_meter": {
                        "status": "live",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "thread-1",
                        "turn_id": "turn-1",
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 11,
                        "client_turn_total_tokens": 12,
                        "context_used_percent": 34.567,
                        "primary_limit_used_percent": 35,
                        "secondary_limit_used_percent": 36,
                        "note": "drop me"
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "scope_code": "same_meter",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "thread-1",
                        "turn_id": "turn-1",
                        "started_at_epoch_ms": 20,
                        "ended_at_epoch_ms": 21,
                        "exact_pair_available": false,
                        "exact_pair": null,
                        "matched_events_count": 0,
                        "matched_context_pack_ids_count": 0,
                        "retrieval_context_pack_count": 0,
                        "note": "drop me"
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "client_budget_target_percent": 50,
                    "thread_id": "thread-1",
                    "current_goal": "goal",
                    "next_step": "next",
                    "execctl_resume_state": "pending_return_queue_present",
                    "project": {
                        "code": "amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity"
                    },
                    "recent_actions": []
                }
            }
        });

        let compact = super::compact_budget_snapshot_preview_payload(&snapshot);
        let report = &compact["token_budget_report"]["token_budget_report"];
        assert_eq!(
            report["client_limit_hourly_burn"]["reply_prefix"],
            json!("5ч KPI: переплата 12.34%")
        );
        assert_eq!(
            report["client_limit_hourly_burn"]["kpi_percent"],
            json!(12.35)
        );
        assert!(report["client_limit_hourly_burn"].get("summary").is_none());
        assert!(
            report["client_limit_hourly_burn"]
                .get("status_bar_correlated")
                .is_none()
        );
        assert!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                .get("note")
                .is_none()
        );
        assert!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                .get("model_version")
                .is_none()
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["alignment_state"],
            json!("same_meter_equivalent")
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["strict_client_meter_slice"]
                ["lower_bound_tokens"],
            json!(182)
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]
                ["measured_baseline_tokens_lower_bound"],
            json!(182)
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]
                ["component_semantics"][1]["code"],
            json!("continuity_restore_outside_retrieval")
        );
        assert!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]["component_semantics"][1]
                .get("note")
                .is_none()
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["observed_whole_cycle_with_amai_tokens"],
            json!(72)
        );
    }

    #[test]
    fn cached_exact_client_limit_refresh_needed_when_hourly_burn_is_missing() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_limit_hourly_burn": {
                        "status": "missing"
                    }
                }
            }
        });

        assert!(super::cached_exact_client_limit_refresh_needed(
            &snapshot, 10_000, 3_000
        ));
    }

    #[test]
    fn compact_cli_client_budget_gate_from_root_cause_payload_extracts_gate() {
        let payload = json!({
            "client_budget_reply_gate": {
                "status_label": "ok",
                "observed_at_epoch_ms": 1234,
                "max_guard_age_seconds": 10,
                "global_reply_prefix": "5ч KPI: экономия 8.00%",
                "reply_prefix_source": "personal_agent_5h_kpi",
                "reply_execution_gate": {
                    "action_kind": "rotate_chat_for_client_budget",
                    "reply_prefix": "5ч KPI: переплата 1.00%",
                    "global_reply_prefix": "5ч KPI: экономия 8.00%",
                    "reply_prefix_source": "personal_agent_5h_kpi"
                }
            }
        });
        let compact = super::compact_cli_client_budget_gate_from_root_cause_payload(&payload)
            .expect("gate payload");
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert_eq!(compact["reply_prefix"], json!("5ч KPI: переплата 1.00%"));
        assert_eq!(
            compact["global_reply_prefix"],
            json!("5ч KPI: экономия 8.00%")
        );
        assert_eq!(
            compact["reply_prefix_source"],
            json!("personal_agent_5h_kpi")
        );
    }

    #[test]
    fn front_door_client_budget_gate_payload_surfaces_top_level_reply_prefix_fields() {
        let gate = json!({
            "status_label": "ok",
            "observed_at_epoch_ms": 1234,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "reply_prefix": "5ч KPI: переплата 7.00%",
                "global_reply_prefix": "5ч KPI: переплата 7.00%",
                "reply_prefix_source": "global_client_limit_hourly_burn"
            }
        });

        let payload = super::front_door_client_budget_gate_payload(gate.clone());
        assert_eq!(payload["reply_prefix"], json!("5ч KPI: переплата 7.00%"));
        assert_eq!(
            payload["global_reply_prefix"],
            json!("5ч KPI: переплата 7.00%")
        );
        assert_eq!(
            payload["reply_prefix_source"],
            json!("global_client_limit_hourly_burn")
        );
        assert_eq!(payload["client_budget_reply_gate"], gate);
    }

    #[test]
    fn normalize_front_door_client_budget_gate_payload_shape_backfills_top_level_fields() {
        let payload = json!({
            "client_budget_reply_gate": {
                "status_label": "ok",
                "observed_at_epoch_ms": 1234,
                "max_guard_age_seconds": 10,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 7.00%",
                    "global_reply_prefix": "5ч KPI: переплата 7.00%",
                    "reply_prefix_source": "global_client_limit_hourly_burn"
                }
            }
        });

        let normalized = super::normalize_front_door_client_budget_gate_payload_shape(payload);
        assert_eq!(normalized["reply_prefix"], json!("5ч KPI: переплата 7.00%"));
        assert_eq!(
            normalized["global_reply_prefix"],
            json!("5ч KPI: переплата 7.00%")
        );
        assert_eq!(
            normalized["reply_prefix_source"],
            json!("global_client_limit_hourly_burn")
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_from_root_cause_payload_fails_closed_without_action_kind() {
        let payload = json!({
            "client_budget_reply_gate": {
                "reply_execution_gate": {}
            }
        });
        assert!(super::compact_cli_client_budget_gate_from_root_cause_payload(&payload).is_none());
    }

    #[test]
    fn compact_chat_api_summary_drops_heavy_runtime_fields() {
        let payload = json!({
            "project": {
                "code": "amai",
                "display_name": "Amai",
                "repo_root": "/home/art/agent-memory-index"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT",
                "project_task_tree": {"drop_me": true},
                "pending_return_queue": [{"drop_me": true}]
            },
            "operator_notice": {
                "message_text": "message",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "note",
                "launch_clean_chat_command": "code chat ...",
                "launch_clean_chat_fallback_command": "code chat --reuse-window ...",
                "launch_clean_chat_command_kind": "vscode_code_chat_cli",
                "manual_fallback_steps": ["Codex: New Thread in Codex Sidebar (chatgpt.newChat)"]
            },
            "client_surface": {
                "client_key": "vscode"
            },
            "host_launch": {
                "status": "available_not_requested"
            },
            "host_current_thread_control": {
                "command_id": "thread-overlay-open-current"
            },
            "client_budget_guard": {
                "drop_me": true
            }
        });

        let compact = super::compact_chat_api_summary(&payload);

        assert_eq!(compact["project"]["code"], json!("amai"));
        assert_eq!(
            compact["chat_start_restore"]["prompt_text"],
            json!("PROMPT")
        );
        assert_eq!(
            compact["operator_notice"]["exact_chat_command"],
            json!("компакт_чат")
        );
        assert_eq!(compact["client_surface"]["client_key"], json!("vscode"));
        assert_eq!(
            compact["host_launch"]["status"],
            json!("available_not_requested")
        );
        assert!(compact.get("host_current_thread_control").is_none());
        assert!(compact.get("client_budget_guard").is_none());
        assert_eq!(
            compact["operator_notice"]["launch_clean_chat_command"],
            json!("code chat ...")
        );
        assert_eq!(
            compact["operator_notice"]["launch_clean_chat_fallback_command"],
            json!("code chat --reuse-window ...")
        );
        assert_eq!(
            compact["operator_notice"]["launch_clean_chat_command_kind"],
            json!("vscode_code_chat_cli")
        );
        assert!(
            compact["operator_notice"]["manual_fallback_steps"]
                .as_array()
                .is_some()
        );
        assert!(
            compact["chat_start_restore"]
                .get("project_task_tree")
                .is_none()
        );
        assert!(
            compact["chat_start_restore"]
                .get("pending_return_queue")
                .is_none()
        );
    }

    #[test]
    fn client_budget_host_control_launch_api_summary_drops_launch_detail_echo() {
        let payload = json!({
            "project": {
                "code": "amai",
                "display_name": "Amai",
                "repo_root": "/home/art/agent-memory-index"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "thread_id": "thread-current",
            "command_id": "hotkey-window-open-current",
            "launch_targeting": {
                "status": "ok",
                "summary": "single target",
                "window_count": 1
            },
            "host_current_thread_control": {
                "command_id": "hotkey-window-open-current"
            },
            "launch": {
                "status": "opened"
            },
            "client_budget_reply_gate": {
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "blocking": false
            },
            "operator_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "message_text": "opened",
                "feedback_kind": "opened",
                "command_id": "hotkey-window-open-current",
                "thread_id": "thread-current"
            }
        });

        let compact = super::client_budget_host_control_launch_api_summary(&payload);

        assert_eq!(
            compact["launch_targeting"]["summary"],
            json!("single target")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert_eq!(
            compact["operator_notice"]["kind"],
            json!("host_current_thread_control_launch_opened")
        );
        assert!(compact["launch_targeting"].get("window_count").is_none());
        assert!(compact.get("host_current_thread_control").is_none());
        assert!(compact.get("launch").is_none());
        assert!(
            compact["client_budget_reply_gate"]
                .get("blocking")
                .is_none()
        );
    }

    #[test]
    fn thread_bound_host_control_feedback_payload_stays_summary_first() {
        let payload = json!({
            "status": "ok",
            "client_budget_host_control_feedback": {
                "project": {
                    "code": "amai",
                    "display_name": "Amai",
                    "repo_root": "/home/art/agent-memory-index"
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                },
                "thread_id": "thread-current",
                "command_id": "hotkey-window-open-current",
                "feedback_kind": "opened",
                "message_text": "Подтверждено: same-thread host control открылся.",
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 1.00%"
                }
            }
        });

        let feedback = &payload["client_budget_host_control_feedback"];
        assert_eq!(feedback["thread_id"], json!("thread-current"));
        assert_eq!(feedback["feedback_kind"], json!("opened"));
        assert_eq!(
            feedback["client_budget_reply_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert!(payload.get("chat_notice").is_none());
        assert!(feedback.get("operator_notice").is_none());
        assert!(feedback.get("host_current_thread_control").is_none());
        assert!(feedback.get("launch").is_none());
        assert!(feedback.get("launch_targeting").is_none());
    }

    #[test]
    fn non_thread_host_control_feedback_payload_stays_summary_first() {
        let payload = json!({
            "status": "ok",
            "client_budget_host_control_feedback": {
                "project": {
                    "code": "amai",
                    "display_name": "Amai",
                    "repo_root": "/home/art/agent-memory-index"
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                },
                "feedback_kind": "failed",
                "command_id": "hotkey-window-open-current",
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 1.00%"
                },
                "operator_notice": {
                    "kind": "host_current_thread_control_feedback_failed",
                    "message_text": "Зафиксировано: same-thread host control не открылся.",
                    "feedback_kind": "failed",
                    "command_id": "hotkey-window-open-current"
                }
            },
            "chat_notice": {
                "kind": "host_current_thread_control_feedback_failed",
                "thread_id": null,
                "message_text": "Зафиксировано: same-thread host control не открылся.",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "feedback_kind": "failed",
                "command_id": "hotkey-window-open-current"
            }
        });

        let feedback = &payload["client_budget_host_control_feedback"];
        assert_eq!(feedback["feedback_kind"], json!("failed"));
        assert_eq!(
            feedback["operator_notice"]["kind"],
            json!("host_current_thread_control_feedback_failed")
        );
        assert_eq!(
            payload["chat_notice"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert!(feedback.get("message_text").is_none());
        assert!(feedback.get("host_current_thread_control").is_none());
        assert!(feedback.get("launch").is_none());
        assert!(feedback.get("launch_targeting").is_none());
        assert!(payload["chat_notice"].get("host_launch").is_none());
        assert!(
            payload["chat_notice"]
                .get("host_current_thread_control")
                .is_none()
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_keeps_personal_kpi_metadata() {
        let guard = json!({
            "status_label": "ok",
            "reply_prefix": "5ч KPI: н/д",
            "global_reply_prefix": "5ч KPI: экономия 8.00%",
            "reply_prefix_source": "personal_agent_5h_kpi",
            "observed_at_epoch_ms": 1_000,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "continue_current_chat",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: н/д",
                "global_reply_prefix": "5ч KPI: экономия 8.00%",
                "reply_prefix_source": "personal_agent_5h_kpi",
                "host_context_compaction_stage": "preserve",
                "host_context_compaction_preserve_active": true,
                "host_context_compaction_critical_regrowth_active": false,
                "preserves_return_obligation": false,
                "same_meter_pure_burn_turn_active": true,
                "action_bundle": {
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command"
                    },
                    "host_current_thread_control": {
                        "command_id": "thread-overlay-open-current",
                        "button_label": "Open thread overlay"
                    }
                }
            }
        });

        let compact = super::compact_cli_client_budget_gate_payload(&guard);
        assert_eq!(compact["reply_prefix"], json!("5ч KPI: н/д"));
        assert_eq!(
            compact["global_reply_prefix"],
            json!("5ч KPI: экономия 8.00%")
        );
        assert_eq!(
            compact["reply_prefix_source"],
            json!("personal_agent_5h_kpi")
        );
        assert_eq!(
            compact["reply_execution_gate"]["global_reply_prefix"],
            json!("5ч KPI: экономия 8.00%")
        );
        assert_eq!(
            compact["reply_execution_gate"]["reply_prefix_source"],
            json!("personal_agent_5h_kpi")
        );
    }

    #[test]
    fn cached_exact_client_limit_refresh_needed_when_hourly_burn_is_stale() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": 1_000
                    }
                }
            }
        });

        assert!(super::cached_exact_client_limit_refresh_needed(
            &snapshot, 5_001, 3_000
        ));
        assert!(!super::cached_exact_client_limit_refresh_needed(
            &snapshot, 4_000, 3_000
        ));
    }

    #[test]
    fn compact_client_budget_request_uses_fresh_cached_snapshot() {
        let cache = ObserveCache {
            snapshot: Some(json!({"status": "ok"})),
            last_refresh_completed_epoch_ms: Some(
                super::now_epoch_ms()
                    .saturating_sub(COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS / 2),
            ),
            ..ObserveCache::default()
        };

        assert!(
            cache_snapshot_age_ms(&cache)
                .is_some_and(|age| age < COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS)
        );
        assert!(!compact_client_budget_snapshot_cache_too_old(
            cache_snapshot_age_ms(&cache)
        ));
    }

    #[test]
    fn compact_client_budget_request_rejects_stale_cached_snapshot() {
        let cache = ObserveCache {
            snapshot: Some(json!({"status": "ok"})),
            last_refresh_completed_epoch_ms: Some(
                super::now_epoch_ms()
                    .saturating_sub(COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS + 1),
            ),
            ..ObserveCache::default()
        };

        assert!(compact_client_budget_snapshot_cache_too_old(
            cache_snapshot_age_ms(&cache)
        ));
    }

    #[test]
    fn observe_refresh_stuck_when_refresh_in_progress_exceeds_timeout() {
        let cache = ObserveCache {
            refresh_in_progress: true,
            last_refresh_started_epoch_ms: Some(super::now_epoch_ms().saturating_sub(
                super::OBSERVE_REFRESH_TIMEOUT_MS + super::OBSERVE_REFRESH_STUCK_GRACE_MS + 1,
            )),
            ..ObserveCache::default()
        };

        assert!(super::observe_refresh_stuck(&cache));
    }

    #[test]
    fn observe_refresh_not_stuck_when_not_in_progress() {
        let cache = ObserveCache {
            last_refresh_started_epoch_ms: Some(super::now_epoch_ms().saturating_sub(
                super::OBSERVE_REFRESH_TIMEOUT_MS + super::OBSERVE_REFRESH_STUCK_GRACE_MS + 1,
            )),
            ..ObserveCache::default()
        };

        assert!(!super::observe_refresh_stuck(&cache));
    }

    #[test]
    fn artifact_cleanup_summary_is_fresh_within_interval() {
        let summary = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 9_500
            }
        });
        assert!(super::artifact_cleanup_summary_is_fresh(
            &summary, 10_000, 1_000
        ));
        assert!(!super::artifact_cleanup_summary_is_fresh(
            &summary, 10_501, 1_000
        ));
    }

    #[test]
    fn artifact_cleanup_summary_is_not_fresh_without_capture_timestamp() {
        let summary = json!({
            "artifact_cleanup": {}
        });
        assert!(!super::artifact_cleanup_summary_is_fresh(
            &summary, 10_000, 1_000
        ));
    }
}
