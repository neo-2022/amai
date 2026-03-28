use crate::config::{AppConfig, discover_repo_root};
use crate::{
    artifact_cleanup, codex_threads, compatibility, dashboard, external_benchmark, nats,
    observability_policy, postgres, retrieval_science, s3, token_budget, working_state,
};
use anyhow::{Context, Result, anyhow};
use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
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

#[derive(Debug, Clone, Default)]
struct ObserveCache {
    snapshot: Option<Value>,
    dashboard_payload: Option<Value>,
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
    timing_format: DashboardTimingFormatProfile,
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
    target_cold_sample_count: u64,
    target_hot_p50_ms: f64,
    target_hot_p95_ms: f64,
    target_hot_p99_ms: f64,
    target_hot_max_ms: f64,
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

pub async fn print_client_budget_guard(cfg: &AppConfig, enforce_reply_gate: bool) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot_preview(cfg).await?;
    let guard = dashboard::current_session_budget_guard(&snapshot);
    println!("{}", serde_json::to_string_pretty(&guard)?);
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

pub async fn print_client_budget_gate(cfg: &AppConfig, enforce_reply_gate: bool) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot_preview(cfg).await?;
    let guard = dashboard::current_session_budget_guard(&snapshot);
    let payload = json!({
        "client_budget_reply_gate": compact_client_budget_gate_payload(&guard)
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&payload["client_budget_reply_gate"]) {
        let action_kind = payload["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"]
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

pub async fn print_client_budget_root_cause(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot_preview(cfg).await?;
    let payload = dashboard::client_budget_root_cause_payload(&snapshot);
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn client_budget_guard_blocks_reply(guard: &Value) -> bool {
    working_state::client_budget_guard_blocks_reply(guard)
}

fn compact_client_budget_gate_payload(guard: &Value) -> Value {
    let reply_execution_gate = &guard["reply_execution_gate"];
    json!({
        "source": "client_budget_reply_gate_v1",
        "status": guard["status"].clone(),
        "status_label": guard["status_label"].clone(),
        "reason_code": reply_execution_gate["reason"].clone(),
        "observed_at_epoch_ms": guard["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": guard["max_guard_age_seconds"].clone(),
        "should_rotate_chat_now": guard["should_rotate_chat_now"].clone(),
        "should_rotate_chat_soon": guard["should_rotate_chat_soon"].clone(),
        "reply_execution_gate": compact_reply_execution_gate(reply_execution_gate),
    })
}

fn compact_reply_execution_gate(reply_execution_gate: &Value) -> Value {
    json!({
        "gate_version": reply_execution_gate["gate_version"].clone(),
        "action_kind": reply_execution_gate["action_kind"].clone(),
        "blocking": reply_execution_gate["blocking"].clone(),
        "must_rotate_before_reply": reply_execution_gate["must_rotate_before_reply"].clone(),
        "must_wait_for_budget_recovery_before_reply":
            reply_execution_gate["must_wait_for_budget_recovery_before_reply"].clone(),
        "reply_budget_mode": reply_execution_gate["reply_budget_mode"].clone(),
        "rotate_now": reply_execution_gate["rotate_now"].clone(),
        "rotate_soon": reply_execution_gate["rotate_soon"].clone(),
        "preserves_return_obligation": reply_execution_gate["action_bundle"]
            ["preserves_return_obligation"]
            .clone(),
    })
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
    let cache = Arc::new(RwLock::new(ObserveCache::default()));
    refresh_observe_cache(
        cache.clone(),
        cfg.clone(),
        bind.to_string(),
        profile.dashboard.refresh_ms,
    )
    .await?;
    let refresh_cache = cache.clone();
    let refresh_cfg = cfg.clone();
    let refresh_bind = bind.to_string();
    let refresh_interval_ms = profile.dashboard.refresh_ms.max(250);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(refresh_interval_ms)).await;
        loop {
            if let Err(error) = refresh_observe_cache(
                refresh_cache.clone(),
                refresh_cfg.clone(),
                refresh_bind.clone(),
                refresh_interval_ms,
            )
            .await
            {
                eprintln!("observe cache refresh failed: {error:#}");
            }
            tokio::time::sleep(Duration::from_millis(refresh_interval_ms)).await;
        }
    });
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
        if let Err(error) = persist_periodic_client_limit_trend_analysis(&trend_cfg).await {
            eprintln!("client limit trend analysis refresh failed: {error:#}");
        }
        let mut interval = tokio::time::interval(CLIENT_LIMIT_TREND_ANALYSIS_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(error) = persist_periodic_client_limit_trend_analysis(&trend_cfg).await {
                eprintln!("client limit trend analysis refresh failed: {error:#}");
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
            "/api/client-budget-live",
            get(client_budget_live_api_handler),
        )
        .route("/api/snapshot", get(snapshot_api_handler))
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(healthz_handler))
        .with_state(ObserveState {
            dashboard_refresh_ms: profile.dashboard.refresh_ms,
            cfg: cfg.clone(),
            bind: bind.to_string(),
            cache,
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

async fn build_snapshot(cfg: &AppConfig, persist_snapshot: bool) -> Result<Value> {
    let snapshot_started = Instant::now();
    if persist_snapshot {
        maybe_cleanup_observability_snapshots(cfg).await?;
    }
    let profile = load_profile()?;
    let repo_root = discover_repo_root(None)?;
    let db = postgres::connect_admin(cfg).await?;
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
        async {
            match postgres::get_project_by_repo_root(&db, &repo_root.display().to_string()).await {
                Ok(project) => {
                    postgres::latest_observability_snapshot_for_project(
                        &db,
                        "working_state_restore",
                        "working_state_restore",
                        &project.code,
                    )
                    .await
                }
                Err(_) => Ok(None),
            }
        },
    )
    .await?;
    let agent_scope_activity = timed_future(
        &mut observe_refresh_stage_ms,
        "agent_scope_activity",
        collect_agent_scope_activity(&db),
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
    let token_budget_report = timed_future(
        &mut observe_refresh_stage_ms,
        "token_budget_dashboard_report",
        token_budget::collect_dashboard_report(&db),
    )
    .await?;
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
        "latest_cold_path_benchmark": latest_cold_path_benchmark,
        "cold_path_benchmark_progress": cold_path_benchmark_progress,
        "latest_working_state_restore": latest_working_state_restore,
        "latest_repo_working_state_restore": latest_repo_working_state_restore,
        "agent_scope_activity": agent_scope_activity,
        "latest_degradation_verification": latest_degradation_verification,
        "latest_continuity_verification": latest_continuity_verification,
        "token_budget_report": token_budget_report,
        "artifact_cleanup": artifact_cleanup_summary["artifact_cleanup"].clone(),
    });
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
        "latest_cold_path_benchmark": payload["latest_cold_path_benchmark"].clone(),
        "cold_path_benchmark_progress": payload["cold_path_benchmark_progress"].clone(),
        "latest_working_state_restore": payload["latest_working_state_restore"].clone(),
        "latest_repo_working_state_restore": payload["latest_repo_working_state_restore"].clone(),
        "agent_scope_activity": payload["agent_scope_activity"].clone(),
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
        "sla": sla,
    });
    if persist_snapshot {
        let _ = postgres::insert_observability_snapshot(&db, "system_snapshot", &snapshot).await?;
    }
    Ok(snapshot)
}

async fn collect_agent_scope_activity(db: &Client) -> Result<Value> {
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let recent_window_hours = 24_i64;
    let recent_window_start_epoch_ms = now_epoch_ms - recent_window_hours * 60 * 60 * 1000;
    let client_recent_window_minutes = 30_i64;
    let client_recent_threads =
        codex_threads::recent_client_thread_records(client_recent_window_minutes * 60)?
            .into_iter()
            .map(|item| {
                json!({
                    "thread_id": item.thread_id,
                    "cwd": item.cwd,
                    "rollout_path": item.rollout_path,
                    "title": item.title,
                    "agent_nickname": item.agent_nickname,
                    "agent_role": item.agent_role,
                    "model_provider": item.model_provider,
                    "model": item.model,
                    "reasoning_effort": item.reasoning_effort,
                    "updated_at_epoch_ms": item.updated_at_epoch_s.saturating_mul(1000),
                })
            })
            .collect::<Vec<_>>();

    let active_rows = db
        .query(
            r#"
            SELECT
                agent_scope,
                owner_thread_id,
                heartbeat_at_epoch_ms,
                expires_at_epoch_ms
            FROM ami.execctl_task_leases
            WHERE lease_state = 'active'
              AND expires_at_epoch_ms > $1
            ORDER BY heartbeat_at_epoch_ms DESC, agent_scope ASC
            LIMIT 64
            "#,
            &[&now_epoch_ms],
        )
        .await
        .context("failed to query active execctl task leases for agent scope activity")?;
    let active_now_scopes = active_rows
        .into_iter()
        .map(|row| {
            json!({
                "agent_scope": row.get::<_, String>(0),
                "owner_thread_id": row.get::<_, Option<String>>(1),
                "heartbeat_at_epoch_ms": row.get::<_, i64>(2),
                "expires_at_epoch_ms": row.get::<_, i64>(3),
            })
        })
        .collect::<Vec<_>>();

    let recent_rows = db
        .query(
            r#"
            WITH recent AS (
                SELECT DISTINCT ON (
                    payload #>> '{working_state_restore,project,code}',
                    payload #>> '{working_state_restore,namespace,code}',
                    payload #>> '{working_state_restore,agent_scope}',
                    COALESCE(payload #>> '{working_state_restore,thread_id}', '')
                )
                    payload #>> '{working_state_restore,project,code}' AS project_code,
                    payload #>> '{working_state_restore,namespace,code}' AS namespace_code,
                    payload #>> '{working_state_restore,agent_scope}' AS agent_scope,
                    NULLIF(payload #>> '{working_state_restore,thread_id}', '') AS thread_id,
                    payload #>> '{working_state_restore,current_goal}' AS current_goal,
                    captured_at_epoch_ms
                FROM ami.observability_snapshots
                WHERE snapshot_kind = 'working_state_restore'
                  AND captured_at_epoch_ms >= $1
                ORDER BY
                    payload #>> '{working_state_restore,project,code}',
                    payload #>> '{working_state_restore,namespace,code}',
                    payload #>> '{working_state_restore,agent_scope}',
                    COALESCE(payload #>> '{working_state_restore,thread_id}', ''),
                    captured_at_epoch_ms DESC
            )
            SELECT
                project_code,
                namespace_code,
                agent_scope,
                thread_id,
                current_goal,
                captured_at_epoch_ms
            FROM recent
            ORDER BY captured_at_epoch_ms DESC
            LIMIT 64
            "#,
            &[&recent_window_start_epoch_ms],
        )
        .await
        .context("failed to query recent working_state_restore scopes for agent scope activity")?;
    let recent_scopes = recent_rows
        .into_iter()
        .map(|row| {
            json!({
                "project_code": row.get::<_, Option<String>>(0),
                "namespace_code": row.get::<_, Option<String>>(1),
                "agent_scope": row.get::<_, Option<String>>(2),
                "thread_id": row.get::<_, Option<String>>(3),
                "current_goal": row.get::<_, Option<String>>(4),
                "captured_at_epoch_ms": row.get::<_, i64>(5),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "source": "observe_agent_scope_activity_v2",
        "captured_at_epoch_ms": now_epoch_ms,
        "client_recent_window_minutes": client_recent_window_minutes,
        "client_recent_thread_count": client_recent_threads.len(),
        "client_recent_threads": client_recent_threads,
        "active_now_count": active_now_scopes.len(),
        "active_now_scopes": active_now_scopes,
        "recent_scope_window_hours": recent_window_hours,
        "recent_scope_count": recent_scopes.len(),
        "recent_scopes": recent_scopes,
    }))
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
                "target_p50_ms": profile.retrieval.target_cold_p50_ms,
                "target_p95_ms": profile.retrieval.target_p95_ms,
                "target_p99_ms": profile.retrieval.target_cold_p99_ms,
                "target_max_ms": profile.retrieval.target_cold_max_ms,
                "target_sample_count": profile.retrieval.target_cold_sample_count,
            },
            "hot_live_table": {
                "target_p50_ms": profile.retrieval.target_hot_p50_ms,
                "target_p95_ms": profile.retrieval.target_hot_p95_ms,
                "target_p99_ms": profile.retrieval.target_hot_p99_ms,
                "target_max_ms": profile.retrieval.target_hot_max_ms,
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
    let summary = run_retention_cleanup(cfg, true, Some(2048)).await?;
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
    let summary = collect_artifact_cleanup_summary(&repo_root, true, true, None, false, None)?;
    let _ = artifact_cleanup::write_latest_summary(&repo_root, &summary)?;
    let cleanup = &summary["artifact_cleanup"];
    let deleted = cleanup["deleted"].as_u64().unwrap_or(0);
    let expired = cleanup["expired"].as_u64().unwrap_or(0);
    if deleted > 0 || expired > 0 {
        eprintln!(
            "Amai artifact cleanup: deleted={}, expired={}, reclaimed_bytes={}",
            deleted,
            expired,
            cleanup["reclaimed_bytes"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
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
    match cached_snapshot_with_meta(&state).await {
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
    let html = dashboard::render_html(state.dashboard_refresh_ms);
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
    refresh_client_live_meter_on_request(&state).await;
    let response = if let Some(thread_id_hint) =
        resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
    {
        thread_bound_dashboard_payload(&state, &thread_id_hint).await
    } else {
        cached_dashboard_payload(&state).await
    };
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

async fn client_budget_live_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let response = if let Some(thread_id_hint) =
        resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
    {
        thread_bound_snapshot_with_meta(&state, &thread_id_hint).await
    } else {
        cached_snapshot_with_meta(&state).await
    };
    match response {
        Ok(snapshot) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&dashboard::client_budget_live_payload(&snapshot))
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

async fn snapshot_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let response = if let Some(thread_id_hint) =
        resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
    {
        thread_bound_snapshot_with_meta(&state, &thread_id_hint).await
    } else {
        cached_snapshot_with_meta(&state).await
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
    refresh_client_live_meter_on_request(&state).await;
    match cached_snapshot_with_meta(&state).await {
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
            let headers = [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            )];
            (
                status,
                headers,
                serde_json::to_string_pretty(&snapshot).unwrap_or_default(),
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

async fn refresh_observe_cache(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: AppConfig,
    bind: String,
    refresh_ms: u64,
) -> Result<()> {
    let started_epoch_ms = now_epoch_ms();
    {
        let mut state = cache.write().await;
        state.last_refresh_started_epoch_ms = Some(started_epoch_ms);
        state.refresh_in_progress = true;
    }
    let started = Instant::now();
    let result = build_snapshot(&cfg, false).await.and_then(|snapshot| {
        dashboard::build_payload(&cfg, &snapshot, &bind, refresh_ms)
            .map(|payload| (snapshot, payload))
    });
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let completed_epoch_ms = now_epoch_ms();
    let mut state = cache.write().await;
    state.last_refresh_completed_epoch_ms = Some(completed_epoch_ms);
    state.last_refresh_duration_ms = Some(elapsed_ms);
    state.refresh_in_progress = false;
    match result {
        Ok((snapshot, payload)) => {
            state.snapshot = Some(snapshot);
            state.dashboard_payload = Some(payload);
            state.last_error = None;
            Ok(())
        }
        Err(error) => {
            state.last_error = Some(format!("{error:#}"));
            Err(error)
        }
    }
}

async fn cached_dashboard_payload(state: &ObserveState) -> Result<Value> {
    let cache = state.cache.read().await;
    let payload = cache
        .dashboard_payload
        .clone()
        .ok_or_else(|| anyhow!("dashboard cache not ready"))?;
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
    let snapshot = cache.snapshot.as_ref()?;
    strict_auto_thread_binding_hint_from_snapshot(snapshot)
}

fn strict_auto_thread_binding_hint_from_snapshot(snapshot: &Value) -> Option<String> {
    strict_auto_thread_binding_hint_from_agent_scope_activity(&snapshot["agent_scope_activity"])
}

fn strict_auto_thread_binding_hint_from_agent_scope_activity(activity: &Value) -> Option<String> {
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
    let snapshot = thread_bound_snapshot_with_meta(state, thread_id).await?;
    let payload = dashboard::build_payload(
        &state.cfg,
        &snapshot,
        &state.bind,
        state.dashboard_refresh_ms,
    )?;
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn thread_bound_snapshot_with_meta(state: &ObserveState, thread_id: &str) -> Result<Value> {
    let snapshot = collect_snapshot_preview_for_thread_hint(thread_id).await?;
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_snapshot(
        snapshot,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

async fn collect_snapshot_preview_for_thread_hint(thread_id: &str) -> Result<Value> {
    let repo_root = discover_repo_root(None)?;
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let subprocess_binary = preferred_snapshot_preview_subprocess_binary(&repo_root, &current_exe);
    let output = ProcessCommand::new(&subprocess_binary)
        .arg("observe")
        .arg("snapshot-preview")
        .env("CODEX_THREAD_ID", thread_id)
        .current_dir(&repo_root)
        .output()
        .await
        .with_context(|| {
            format!(
                "failed to spawn snapshot-preview subprocess for thread_id={thread_id} using {}",
                subprocess_binary.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "snapshot-preview subprocess failed for thread_id={thread_id}: {}",
            stderr.trim()
        ));
    }
    serde_json::from_slice(&output.stdout).with_context(|| {
        format!("snapshot-preview subprocess returned invalid JSON for thread_id={thread_id}")
    })
}

fn preferred_snapshot_preview_subprocess_binary(repo_root: &Path, current_exe: &Path) -> PathBuf {
    let release_binary = repo_root.join("target/release/amai");
    if release_binary.is_file() {
        return release_binary;
    }

    let debug_binary = repo_root.join("target/debug/amai");
    if debug_binary.is_file() {
        return debug_binary;
    }

    current_exe.to_path_buf()
}

async fn refresh_client_live_meter_on_request(state: &ObserveState) {
    if let Err(error) = maybe_refresh_client_live_meter(state).await {
        eprintln!("observe request-side client meter refresh failed: {error:#}");
    }
}

async fn maybe_refresh_client_live_meter(state: &ObserveState) -> Result<()> {
    let cache_snapshot = {
        let cache = state.cache.read().await;
        if observe_cache_stale(&cache, state.dashboard_refresh_ms) {
            None
        } else {
            Some((
                cache.snapshot.clone(),
                cache.last_refresh_completed_epoch_ms,
                cache.refresh_in_progress,
            ))
        }
    };

    let Some((snapshot, _last_refresh_completed_epoch_ms, refresh_in_progress)) = cache_snapshot
    else {
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    };

    let Some(snapshot) = snapshot else {
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
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
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
    refresh_observe_cache(
        state.cache.clone(),
        state.cfg.clone(),
        state.bind.clone(),
        state.dashboard_refresh_ms,
    )
    .await
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
    let collection: Value = http
        .get(format!(
            "{}/collections/{}",
            qdrant_http_url, collection_code
        ))
        .send()
        .await
        .context("failed to query qdrant collection endpoint")?
        .json()
        .await
        .context("failed to decode qdrant collection response")?;
    let result = &collection["result"];
    Ok(json!({
        "collections_total": metric_value(&metrics, "collections_total"),
        "collections_vector_total": metric_value(&metrics, "collections_vector_total"),
        "index_optimize_queue": metric_value(&metrics, "collection_update_queue_length"),
        "running_optimizations": metric_value(&metrics, "collection_running_optimizations"),
        "update_queue_length": metric_value(&metrics, "collection_update_queue_length"),
        "memory_resident_bytes": metric_value(&metrics, "memory_resident_bytes"),
        "optimizer_status": result["optimizer_status"].clone(),
        "indexed_vectors_count": result["indexed_vectors_count"].clone(),
        "points_count": result["points_count"].clone(),
        "segments_count": result["segments_count"].clone(),
    }))
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
        benchmark_contamination_value, build_continuity_correctness_model, build_degradation_model,
        cached_client_live_meter_state, client_live_meter_refresh_needed, evaluate_sla,
        expired_retention_candidates, load_profile, normalized_thread_id_hint,
        profile_thresholds_json, render_prometheus_metrics, select_latest_clean_benchmark_snapshot,
    };
    use crate::codex_threads::RolloutClientMeterObservation;
    use crate::postgres::ObservabilityRetentionCandidate;
    use serde_json::json;
    use std::fs::{self, File};
    use std::path::PathBuf;
    use uuid::Uuid;

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
            super::strict_auto_thread_binding_hint_from_snapshot(&snapshot).as_deref(),
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
            super::strict_auto_thread_binding_hint_from_snapshot(&snapshot),
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
            super::strict_auto_thread_binding_hint_from_snapshot(&snapshot),
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
            client_turn_total_tokens: 122000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 93,
            latest_secondary_limit_used_percent: 29,
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
    fn client_budget_guard_blocks_reply_when_reply_execution_gate_requires_rotate() {
        let guard = json!({
            "reply_execution_gate": {
                "must_rotate_before_reply": true
            }
        });
        assert!(super::client_budget_guard_blocks_reply(&guard));
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
                "action_kind": "continue_current_chat",
                "must_rotate_before_reply": false,
                "blocking": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_blocks_reply_when_waiting_for_global_budget_recovery() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true,
                "must_wait_for_budget_recovery_before_reply": true
            },
            "requires_global_budget_recovery_before_reply": true
        });
        assert!(super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_only_gate_fields() {
        let guard = json!({
            "status": "critical",
            "status_label": "новый чат нужен сейчас",
            "reason": "heavy human explanation",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "should_rotate_chat_now": true,
            "should_rotate_chat_soon": true,
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
            payload["source"].as_str(),
            Some("client_budget_reply_gate_v1")
        );
        assert_eq!(payload["status_label"].as_str(), Some("новый чат нужен сейчас"));
        assert_eq!(
            payload["reply_execution_gate"]["reply_budget_mode"].as_str(),
            Some("compact_high_signal")
        );
        assert_eq!(
            payload["reply_execution_gate"]["preserves_return_obligation"].as_bool(),
            Some(false)
        );
        assert_eq!(
            payload["reason_code"].as_str(),
            Some("client_budget_guard_pressure")
        );
        assert!(payload.get("last_request").is_none());
        assert!(payload.get("tracked_slice").is_none());
        assert!(payload.get("client_limits").is_none());
        assert!(payload.get("reason").is_none());
        assert!(payload.get("requires_global_budget_recovery_before_reply").is_none());
        assert!(payload["reply_execution_gate"]["reply_budget_contract"].is_null());
        assert!(payload["reply_execution_gate"]["blocking_reply_contract"].is_null());
        assert!(payload["reply_execution_gate"]["action_bundle"].is_null());
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
            &snapshot,
            10_000,
            3_000
        ));
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
            &snapshot,
            5_001,
            3_000
        ));
        assert!(!super::cached_exact_client_limit_refresh_needed(
            &snapshot,
            4_000,
            3_000
        ));
    }

    #[test]
    fn snapshot_preview_subprocess_prefers_release_binary_under_repo_root() {
        let temp_root = std::env::temp_dir().join(format!("amai-observe-test-{}", Uuid::new_v4()));
        let target_release = temp_root.join("target/release");
        fs::create_dir_all(&target_release).expect("create target/release");
        let release_binary = target_release.join("amai");
        File::create(&release_binary).expect("create release binary");
        let deleted_current_exe = PathBuf::from("/tmp/amai-deleted-binary");

        let selected =
            super::preferred_snapshot_preview_subprocess_binary(&temp_root, &deleted_current_exe);

        assert_eq!(selected, release_binary);
        fs::remove_dir_all(&temp_root).expect("remove temp root");
    }
}
