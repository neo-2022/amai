use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio_postgres::Client;

const CAPACITY_FORECAST_VERSION: &str = "capacity-forecast-v1";
const OBSERVE_SCOPE_NAMESPACE: &str = "observe";
const HISTORY_LIMIT: i64 = 64;
const MIN_WINDOW_SAMPLES: usize = 2;
const WINDOW_KEYS: [(&str, u64); 2] = [("1m", 60), ("5m", 300)];

#[derive(Debug, Clone, Copy)]
struct NatsHistoryPoint {
    captured_at_epoch_ms: u64,
    in_msgs: f64,
    consumer_lag_msgs: f64,
}

pub async fn build_capacity_forecast(db: &Client, snapshot: &Value) -> Result<Value> {
    let (history_scope, mut points) = load_nats_system_snapshot_history(db, snapshot).await?;
    if let Some(current) = nats_point_from_current_snapshot(snapshot) {
        points.push(current);
    }
    points.sort_by_key(|point| point.captured_at_epoch_ms);
    points.dedup_by_key(|point| point.captured_at_epoch_ms);

    let family = build_nats_family_report(&points, &history_scope);
    let family_status = family["status"].as_str().unwrap_or("unknown");
    let measured_families = usize::from(family_status == "measured");
    let insufficient_families = usize::from(family_status == "insufficient_sample");
    Ok(json!({
        "model_version": CAPACITY_FORECAST_VERSION,
        "surface_role": "read_only_capacity_forecast",
        "summary": {
            "status": if measured_families > 0 { "pass" } else { "unknown" },
            "family_count": 1,
            "measured_families": measured_families,
            "insufficient_families": insufficient_families,
        },
        "history_scope": history_scope,
        "families": [family],
        "guardrails": {
            "runtime_authority": false,
            "routing_authority": false,
            "truth_authority": false,
        }
    }))
}

async fn load_nats_system_snapshot_history(
    db: &Client,
    snapshot: &Value,
) -> Result<(Value, Vec<NatsHistoryPoint>)> {
    let project_code = snapshot["_observability"]["scope_project_code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let namespace_code = snapshot["_observability"]["scope_namespace_code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| OBSERVE_SCOPE_NAMESPACE.to_string());

    if let Some(project_code) = project_code {
        let scoped = query_nats_history_points_scoped(
            db,
            &project_code,
            &namespace_code,
            Some(HISTORY_LIMIT),
        )
        .await?;
        if !scoped.is_empty() {
            return Ok((
                json!({
                    "mode": "project_scoped_observe_history",
                    "snapshot_kind": "system_snapshot",
                    "project_code": project_code,
                    "namespace_code": namespace_code,
                    "history_points": scoped.len(),
                }),
                scoped,
            ));
        }
        let fallback = query_nats_history_points_global(db, Some(HISTORY_LIMIT)).await?;
        return Ok((
            json!({
                "mode": "global_unscoped_fallback",
                "snapshot_kind": "system_snapshot",
                "project_code": project_code,
                "namespace_code": namespace_code,
                "history_points": fallback.len(),
            }),
            fallback,
        ));
    }

    let fallback = query_nats_history_points_global(db, Some(HISTORY_LIMIT)).await?;
    Ok((
        json!({
            "mode": "global_unscoped_fallback",
            "snapshot_kind": "system_snapshot",
            "project_code": Value::Null,
            "namespace_code": Value::Null,
            "history_points": fallback.len(),
        }),
        fallback,
    ))
}

async fn query_nats_history_points_scoped(
    db: &Client,
    project_code: &str,
    namespace_code: &str,
    limit: Option<i64>,
) -> Result<Vec<NatsHistoryPoint>> {
    let limit = limit.unwrap_or(HISTORY_LIMIT);
    let rows = db
        .query(
            r#"
            SELECT
                COALESCE(captured_at_epoch_ms, (EXTRACT(EPOCH FROM created_at) * 1000)::bigint) AS captured_at_epoch_ms,
                NULLIF(payload #>> '{nats,in_msgs}', '')::double precision AS in_msgs,
                NULLIF(payload #>> '{nats,consumer_lag_msgs}', '')::double precision AS consumer_lag_msgs
            FROM ami.observability_snapshots
            WHERE snapshot_kind = 'system_snapshot'
              AND scope_project_code = $1
              AND scope_namespace_code = $2
            ORDER BY COALESCE(captured_at_epoch_ms, (EXTRACT(EPOCH FROM created_at) * 1000)::bigint) DESC,
                     created_at DESC
            LIMIT $3
            "#,
            &[&project_code, &namespace_code, &limit],
        )
        .await
        .with_context(|| {
            format!(
                "failed to query scoped nats history points for {}::{}",
                project_code, namespace_code
            )
        })?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(NatsHistoryPoint {
                captured_at_epoch_ms: row.get::<_, Option<i64>>(0)? as u64,
                in_msgs: row.get::<_, Option<f64>>(1)?,
                consumer_lag_msgs: row.get::<_, Option<f64>>(2)?,
            })
        })
        .collect())
}

async fn query_nats_history_points_global(
    db: &Client,
    limit: Option<i64>,
) -> Result<Vec<NatsHistoryPoint>> {
    let limit = limit.unwrap_or(HISTORY_LIMIT);
    let rows = db
        .query(
            r#"
            SELECT
                COALESCE(captured_at_epoch_ms, (EXTRACT(EPOCH FROM created_at) * 1000)::bigint) AS captured_at_epoch_ms,
                NULLIF(payload #>> '{nats,in_msgs}', '')::double precision AS in_msgs,
                NULLIF(payload #>> '{nats,consumer_lag_msgs}', '')::double precision AS consumer_lag_msgs
            FROM ami.observability_snapshots
            WHERE snapshot_kind = 'system_snapshot'
            ORDER BY COALESCE(captured_at_epoch_ms, (EXTRACT(EPOCH FROM created_at) * 1000)::bigint) DESC,
                     created_at DESC
            LIMIT $1
            "#,
            &[&limit],
        )
        .await
        .context("failed to query global nats history points")?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(NatsHistoryPoint {
                captured_at_epoch_ms: row.get::<_, Option<i64>>(0)? as u64,
                in_msgs: row.get::<_, Option<f64>>(1)?,
                consumer_lag_msgs: row.get::<_, Option<f64>>(2)?,
            })
        })
        .collect())
}

fn nats_point_from_current_snapshot(snapshot: &Value) -> Option<NatsHistoryPoint> {
    Some(NatsHistoryPoint {
        captured_at_epoch_ms: snapshot["captured_at_epoch_ms"].as_u64()?,
        in_msgs: snapshot["nats"]["in_msgs"].as_f64()?,
        consumer_lag_msgs: snapshot["nats"]["consumer_lag_msgs"].as_f64()?,
    })
}

fn build_nats_family_report(points: &[NatsHistoryPoint], history_scope: &Value) -> Value {
    let windows = WINDOW_KEYS
        .into_iter()
        .map(|(window_key, window_seconds)| build_window_report(points, window_key, window_seconds))
        .collect::<Vec<_>>();
    let measured_windows = windows
        .iter()
        .filter(|window| window["status"].as_str() == Some("measured"))
        .count();
    json!({
        "family_key": "nats_events",
        "title": "NATS events",
        "status": if measured_windows > 0 { "measured" } else { "insufficient_sample" },
        "source_bucket_definition": {
            "snapshot_kind": "system_snapshot",
            "arrival_counter": "nats.in_msgs",
            "backlog_gauge": "nats.consumer_lag_msgs",
            "service_rate_formula": "service_rate = (arrivals - delta_backlog) / observed_span_seconds",
            "history_scope_mode": history_scope["mode"].clone(),
        },
        "windows": windows,
    })
}

fn build_window_report(
    points: &[NatsHistoryPoint],
    window_key: &str,
    window_seconds: u64,
) -> Value {
    let Some(current) = points.last().copied() else {
        return insufficient_window(window_key, window_seconds, 0, 0.0, "no_history");
    };
    let window_start_epoch_ms = current
        .captured_at_epoch_ms
        .saturating_sub(window_seconds.saturating_mul(1000));
    let window_points = points
        .iter()
        .copied()
        .filter(|point| point.captured_at_epoch_ms >= window_start_epoch_ms)
        .collect::<Vec<_>>();
    if window_points.len() < MIN_WINDOW_SAMPLES {
        return insufficient_window(
            window_key,
            window_seconds,
            window_points.len(),
            observed_span_seconds(&window_points),
            "not_enough_points",
        );
    }
    let first = window_points
        .first()
        .copied()
        .expect("window_points not empty");
    let last = window_points
        .last()
        .copied()
        .expect("window_points not empty");
    let span_seconds = observed_span_seconds(&window_points);
    if span_seconds <= 0.0 || span_seconds < window_seconds as f64 * 0.5 {
        return insufficient_window(
            window_key,
            window_seconds,
            window_points.len(),
            span_seconds,
            "insufficient_observed_span",
        );
    }

    let arrivals = (last.in_msgs - first.in_msgs).max(0.0);
    let delta_backlog = last.consumer_lag_msgs - first.consumer_lag_msgs;
    let expected_arrivals = arrivals.max(0.0);
    let lambda = expected_arrivals / span_seconds;
    let observed_service_rate = ((arrivals - delta_backlog).max(0.0)) / span_seconds;
    let capacity_margin = observed_service_rate - lambda;
    let (lower, upper) = poisson_interval_95(expected_arrivals);
    json!({
        "window_key": window_key,
        "window_seconds": window_seconds,
        "status": "measured",
        "sample_count": window_points.len(),
        "observed_span_seconds": span_seconds,
        "lambda": lambda,
        "arrival_count": arrivals,
        "expected_arrivals": expected_arrivals,
        "poisson_interval_95": {
            "lower": lower,
            "upper": upper,
            "method": "normal_approx"
        },
        "observed_service_rate": observed_service_rate,
        "capacity_margin": capacity_margin,
        "starting_backlog": first.consumer_lag_msgs,
        "ending_backlog": last.consumer_lag_msgs,
    })
}

fn insufficient_window(
    window_key: &str,
    window_seconds: u64,
    sample_count: usize,
    observed_span_seconds: f64,
    reason: &str,
) -> Value {
    json!({
        "window_key": window_key,
        "window_seconds": window_seconds,
        "status": "insufficient_sample",
        "sample_count": sample_count,
        "observed_span_seconds": observed_span_seconds,
        "reason": reason,
    })
}

fn observed_span_seconds(points: &[NatsHistoryPoint]) -> f64 {
    let Some(first) = points.first() else {
        return 0.0;
    };
    let Some(last) = points.last() else {
        return 0.0;
    };
    last.captured_at_epoch_ms
        .saturating_sub(first.captured_at_epoch_ms) as f64
        / 1000.0
}

fn poisson_interval_95(expected_arrivals: f64) -> (f64, f64) {
    if expected_arrivals <= 0.0 {
        return (0.0, 0.0);
    }
    let sigma = expected_arrivals.sqrt();
    let lower = (expected_arrivals - 1.96 * sigma).max(0.0);
    let upper = (expected_arrivals + 1.96 * sigma).max(expected_arrivals);
    (lower, upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(captured_at_epoch_ms: u64, in_msgs: f64, consumer_lag_msgs: f64) -> NatsHistoryPoint {
        NatsHistoryPoint {
            captured_at_epoch_ms,
            in_msgs,
            consumer_lag_msgs,
        }
    }

    #[test]
    fn poisson_interval_never_goes_negative() {
        let (lower, upper) = poisson_interval_95(4.0);
        assert!(lower >= 0.0);
        assert!(upper >= 4.0);
    }

    #[test]
    fn nats_bucket_aggregation_uses_counter_and_lag_deltas() {
        let points = vec![point(0, 100.0, 10.0), point(60_000, 130.0, 5.0)];
        let report = build_window_report(&points, "1m", 60);
        assert_eq!(report["status"], json!("measured"));
        assert_eq!(report["arrival_count"], json!(30.0));
        assert_eq!(report["expected_arrivals"], json!(30.0));
        let observed_service_rate = report["observed_service_rate"]
            .as_f64()
            .expect("observed_service_rate f64");
        let capacity_margin = report["capacity_margin"]
            .as_f64()
            .expect("capacity_margin f64");
        assert!((observed_service_rate - (35.0 / 60.0)).abs() < 1e-9);
        assert!((capacity_margin - (5.0 / 60.0)).abs() < 1e-9);
    }

    #[test]
    fn five_minute_window_requires_enough_span() {
        let points = vec![point(0, 100.0, 10.0), point(60_000, 130.0, 5.0)];
        let report = build_window_report(&points, "5m", 300);
        assert_eq!(report["status"], json!("insufficient_sample"));
        assert_eq!(report["reason"], json!("insufficient_observed_span"));
    }
}
