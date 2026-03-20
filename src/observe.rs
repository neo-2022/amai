use crate::config::AppConfig;
use crate::{dashboard, nats, postgres, s3, token_budget};
use anyhow::{Context, Result, anyhow};
use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct ObserveState {
    cfg: AppConfig,
    bind: String,
    dashboard_refresh_ms: u64,
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
    target_p95_ms: f64,
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
    alert_hot_qps: f64,
    critical_hot_qps: f64,
    target_hot_error_rate: f64,
    alert_hot_error_rate: f64,
    critical_hot_error_rate: f64,
}

pub async fn print_snapshot(cfg: &AppConfig) -> Result<()> {
    let snapshot = collect_snapshot(cfg).await?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

pub async fn run_sla_check(cfg: &AppConfig) -> Result<()> {
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

pub async fn serve_metrics(cfg: &AppConfig, bind: &str) -> Result<()> {
    let profile = load_profile()?;
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid observe bind address: {bind}"))?;
    let app = Router::new()
        .route("/", get(dashboard_page_handler))
        .route("/dashboard", get(dashboard_page_handler))
        .route("/api/dashboard", get(dashboard_api_handler))
        .route("/api/snapshot", get(snapshot_api_handler))
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(healthz_handler))
        .with_state(ObserveState {
            cfg: cfg.clone(),
            bind: bind.to_string(),
            dashboard_refresh_ms: profile.dashboard.refresh_ms,
        });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind observe exporter on {bind}"))?;
    let base_url = human_dashboard_base_url(bind);
    println!("Amai human dashboard: {base_url}/");
    println!("Amai dashboard JSON: {base_url}/api/dashboard");
    println!("Amai raw snapshot JSON: {base_url}/api/snapshot");
    println!("Amai health JSON: {base_url}/healthz");
    println!("Amai Prometheus metrics: {base_url}/metrics");
    axum::serve(listener, app)
        .await
        .context("observe exporter stopped unexpectedly")
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
    let profile = load_profile()?;
    let db = postgres::connect_admin(cfg).await?;
    let previous = postgres::latest_observability_snapshot(&db, "system_snapshot").await?;
    let http = http_client()?;
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;

    let mut postgres_live = collect_postgres_live(&db, &profile.snapshot).await?;
    if let Some(object) = postgres_live.as_object_mut() {
        object.insert(
            "captured_at_epoch_ms".to_string(),
            Value::from(captured_at_epoch_ms),
        );
    }
    let qdrant_live = collect_qdrant_live(cfg, &http).await?;
    let nats_live = collect_nats_live(cfg, &http, &profile.snapshot).await?;
    let s3_live = collect_s3_live(cfg).await?;

    let latest_hot =
        postgres::latest_observability_snapshot(&db, "retrieval_benchmark_hot").await?;
    let latest_cold =
        postgres::latest_observability_snapshot(&db, "retrieval_benchmark_cold").await?;
    let latest_index = postgres::latest_observability_snapshot(&db, "index_project").await?;
    let latest_accuracy =
        postgres::latest_observability_snapshot(&db, "retrieval_accuracy").await?;
    let latest_load_hot =
        postgres::latest_observability_snapshot(&db, "retrieval_load_hot").await?;
    let latest_load_cold =
        postgres::latest_observability_snapshot(&db, "retrieval_load_cold").await?;
    let latest_token_benchmark =
        postgres::latest_observability_snapshot(&db, "token_benchmark").await?;
    let token_budget_report = token_budget::collect_default_report(&db).await?;

    let payload = json!({
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "stack_name": cfg.stack_name,
        "postgres": with_postgres_rates(&postgres_live, previous.as_ref()),
        "qdrant": qdrant_live,
        "nats": nats_live,
        "s3": s3_live,
        "latest_index_project": latest_index,
        "latest_retrieval_hot": latest_hot,
        "latest_retrieval_cold": latest_cold,
        "latest_retrieval_accuracy": latest_accuracy,
        "latest_retrieval_load_hot": latest_load_hot,
        "latest_retrieval_load_cold": latest_load_cold,
        "latest_token_benchmark": latest_token_benchmark,
        "token_budget_report": token_budget_report,
    });
    let sla = evaluate_sla(&payload, &profile);
    let snapshot = json!({
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "stack_name": cfg.stack_name,
        "postgres": payload["postgres"].clone(),
        "qdrant": payload["qdrant"].clone(),
        "nats": payload["nats"].clone(),
        "s3": payload["s3"].clone(),
        "latest_index_project": payload["latest_index_project"].clone(),
        "latest_retrieval_hot": payload["latest_retrieval_hot"].clone(),
        "latest_retrieval_cold": payload["latest_retrieval_cold"].clone(),
        "latest_retrieval_accuracy": payload["latest_retrieval_accuracy"].clone(),
        "latest_retrieval_load_hot": payload["latest_retrieval_load_hot"].clone(),
        "latest_retrieval_load_cold": payload["latest_retrieval_load_cold"].clone(),
        "latest_token_benchmark": payload["latest_token_benchmark"].clone(),
        "token_budget_report": payload["token_budget_report"].clone(),
        "sla": sla,
    });
    if persist_snapshot {
        let _ = postgres::insert_observability_snapshot(&db, "system_snapshot", &snapshot).await?;
    }
    Ok(snapshot)
}

async fn metrics_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    match build_snapshot(&state.cfg, false).await {
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
            format!("observe exporter failed to collect snapshot: {error:#}"),
        )
            .into_response(),
    }
}

async fn dashboard_page_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    let html = dashboard::render_html(state.dashboard_refresh_ms);
    Html(html).into_response()
}

async fn dashboard_api_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    match build_snapshot(&state.cfg, false)
        .await
        .and_then(|snapshot| {
            dashboard::build_payload(
                &state.cfg,
                &snapshot,
                &state.bind,
                state.dashboard_refresh_ms,
            )
        }) {
        Ok(payload) => {
            let headers = [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            )];
            (
                StatusCode::OK,
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

async fn snapshot_api_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    match build_snapshot(&state.cfg, false).await {
        Ok(snapshot) => {
            let headers = [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            )];
            (
                StatusCode::OK,
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

async fn healthz_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    match build_snapshot(&state.cfg, false).await {
        Ok(snapshot) => {
            let summary = &snapshot["sla"]["summary"];
            let critical = summary["critical"].as_u64().unwrap_or(0);
            let unknown = summary["unknown"].as_u64().unwrap_or(0);
            let status = if critical == 0 && unknown == 0 {
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
            "wal_bytes_per_sec".to_string(),
            wal_bytes_per_sec.map_or(Value::Null, Value::from),
        );
    }
    value
}

async fn collect_qdrant_live(cfg: &AppConfig, http: &reqwest::Client) -> Result<Value> {
    let metrics_text = http
        .get(format!("{}/metrics", cfg.qdrant_http_url))
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
            cfg.qdrant_http_url, cfg.qdrant_collection_code
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
        zero_check(
            "postgres.deadlocks_total",
            snapshot["postgres"]["deadlocks_total"].as_f64(),
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
            profile.retrieval.stretch_hot_p95_ms,
            profile.retrieval.target_p95_ms,
            profile.retrieval.alert_p95_ms,
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
        "amai_load_hot_error_rate",
        "Concurrent hot retrieval error rate from the latest load verification.",
        snapshot["latest_retrieval_load_hot"]["load_verification"]["error_rate"].as_f64(),
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
        "Saved tokens accumulated in the current token-usage session.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]
            ["total_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_saved_window_total",
        "Saved tokens accumulated in the current budget window.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["total_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_saved_lifetime_total",
        "Saved tokens accumulated across all recorded token-usage events.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["total_saved_tokens"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent_session",
        "Savings percent accumulated in the current token-usage session.",
        snapshot["token_budget_report"]["token_budget_report"]["current_session"]
            ["savings_percent"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent_window",
        "Savings percent accumulated in the current budget window.",
        snapshot["token_budget_report"]["token_budget_report"]["rolling_window"]["savings_percent"]
            .as_f64(),
    );
    push_metric(
        &mut output,
        "amai_tokens_savings_percent_lifetime",
        "Savings percent accumulated across all recorded token-usage events.",
        snapshot["token_budget_report"]["token_budget_report"]["lifetime"]["savings_percent"]
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

    output
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
