use super::ObservabilityProfile;
use crate::observability_policy;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn profile_thresholds_json(profile: &ObservabilityProfile) -> Value {
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

pub(super) fn evaluate_sla(snapshot: &Value, profile: &ObservabilityProfile) -> Value {
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

pub(super) fn benchmark_contamination_value(snapshot: &Value) -> Option<f64> {
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

pub(super) fn benchmark_payload_contaminated(payload: &Value, expected_root: &str) -> Option<bool> {
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

pub(super) fn load_profile() -> Result<ObservabilityProfile> {
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

pub(super) fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .context("failed to build observe HTTP client")
}

pub(super) fn parse_prometheus_sums(body: &str) -> HashMap<String, f64> {
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

pub(super) fn metric_value(metrics: &HashMap<String, f64>, key: &str) -> f64 {
    metrics.get(key).copied().unwrap_or(0.0)
}

pub(super) fn metric_value_optional(metrics: &HashMap<String, f64>, key: &str) -> Option<f64> {
    metrics.get(key).copied()
}

pub(super) fn extract_nats_consumer_lag(jsz: &Value) -> u64 {
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

pub(super) fn percentile_f64(samples: &[f64], percentile: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (percentile.min(100) * sorted.len()).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

pub(super) fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64
}

pub(super) fn ratio_f64(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        return 0.0;
    }
    numerator / denominator
}

pub(super) fn delta_rate(current: f64, previous: Option<f64>, dt_s: f64) -> Option<f64> {
    let previous = previous?;
    if dt_s <= 0.0 || current < previous {
        return None;
    }
    Some((current - previous) / dt_s)
}

pub(super) fn counter_delta(current: f64, previous: Option<f64>) -> Option<f64> {
    let previous = previous?;
    if current < previous {
        return None;
    }
    Some(current - previous)
}

pub(super) fn render_prometheus_metrics(snapshot: &Value) -> String {
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
