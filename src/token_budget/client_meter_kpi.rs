use serde_json::{Value, json};

pub(super) fn signed_kpi_classification(kpi_percent: f64) -> &'static str {
    if kpi_percent > 0.5 {
        "saving"
    } else if kpi_percent < -0.5 {
        "overspend"
    } else {
        "one_to_one"
    }
}

pub(super) fn reply_prefix_for_signed_kpi_percent(kpi_percent: f64) -> String {
    match signed_kpi_classification(kpi_percent) {
        "saving" => format!("5ч KPI: экономия {kpi_percent:.2}%"),
        "overspend" => format!("5ч KPI: переплата {:.2}%", kpi_percent.abs()),
        _ => "5ч KPI: 1:1".to_string(),
    }
}

pub(super) fn damp_signed_kpi_percent_for_window_progress(
    raw_signed_kpi_percent: f64,
    elapsed_window_minutes: f64,
    minimum_stable_window_minutes: u64,
) -> (f64, &'static str, f64) {
    let minimum_stable_window_minutes = minimum_stable_window_minutes as f64;
    if minimum_stable_window_minutes <= f64::EPSILON
        || elapsed_window_minutes >= minimum_stable_window_minutes
    {
        return (raw_signed_kpi_percent, "stable", 1.0);
    }
    let progress_ratio = (elapsed_window_minutes / minimum_stable_window_minutes).clamp(0.0, 1.0);
    (
        raw_signed_kpi_percent * progress_ratio,
        "preliminary",
        progress_ratio,
    )
}

fn percent_value(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|value| value as f64))
}

#[derive(Debug, Clone)]
pub(super) struct PreferredOnlineLimitSurface {
    pub(super) observed_at_epoch_ms: u64,
    pub(super) primary_used_percent: f64,
    pub(super) primary_remaining_percent: f64,
    pub(super) secondary_used_percent: f64,
    pub(super) secondary_remaining_percent: f64,
    pub(super) window_duration_minutes: u64,
    pub(super) primary_resets_at_epoch_seconds: Option<u64>,
    pub(super) source_label: &'static str,
    pub(super) source_kind: &'static str,
}

pub(super) fn preferred_online_limit_surface(
    client_live_meter: &Value,
) -> Option<PreferredOnlineLimitSurface> {
    if client_live_meter["status"].as_str() == Some("observed")
        && client_live_meter["current_thread_bound"]
            .as_bool()
            .unwrap_or_else(|| {
                client_live_meter["thread_binding_state"].as_str() == Some("current_thread_bound")
            })
    {
        if let (Some(observed_at_epoch_ms), Some(primary_used_percent)) = (
            client_live_meter["observed_at_epoch_ms"]
                .as_u64()
                .or_else(|| client_live_meter["ended_at_epoch_ms"].as_u64()),
            percent_value(&client_live_meter["primary_limit_used_percent"]),
        ) {
            let primary_remaining_percent =
                percent_value(&client_live_meter["primary_limit_remaining_percent"])
                    .unwrap_or((100.0 - primary_used_percent).max(0.0));
            let secondary_used_percent =
                percent_value(&client_live_meter["secondary_limit_used_percent"]).unwrap_or(0.0);
            let secondary_remaining_percent =
                percent_value(&client_live_meter["secondary_limit_remaining_percent"])
                    .unwrap_or((100.0 - secondary_used_percent).max(0.0));
            return Some(PreferredOnlineLimitSurface {
                observed_at_epoch_ms,
                primary_used_percent,
                primary_remaining_percent,
                secondary_used_percent,
                secondary_remaining_percent,
                window_duration_minutes: client_live_meter["primary_window_duration_mins"]
                    .as_u64()
                    .unwrap_or(300)
                    .max(1),
                primary_resets_at_epoch_seconds:
                    client_live_meter["primary_resets_at_epoch_seconds"].as_u64(),
                source_label: "thread-local VS Code rollout token_count/rate_limits contour",
                source_kind: "thread_local_rollout",
            });
        }
    }
    let status_bar = &client_live_meter["status_bar_rate_limits"];
    if status_bar["status"].as_str() == Some("observed") {
        if let (Some(observed_at_epoch_ms), Some(primary_used_percent)) = (
            status_bar["observed_at_epoch_ms"]
                .as_u64()
                .or_else(|| status_bar["ended_at_epoch_ms"].as_u64()),
            percent_value(&status_bar["primary_limit_used_percent"]),
        ) {
            let primary_remaining_percent =
                percent_value(&status_bar["primary_limit_remaining_percent"])
                    .unwrap_or((100.0 - primary_used_percent).max(0.0));
            let secondary_used_percent =
                percent_value(&status_bar["secondary_limit_used_percent"]).unwrap_or(0.0);
            let secondary_remaining_percent =
                percent_value(&status_bar["secondary_limit_remaining_percent"])
                    .unwrap_or((100.0 - secondary_used_percent).max(0.0));
            return Some(PreferredOnlineLimitSurface {
                observed_at_epoch_ms,
                primary_used_percent,
                primary_remaining_percent,
                secondary_used_percent,
                secondary_remaining_percent,
                window_duration_minutes: status_bar["primary_window_duration_mins"]
                    .as_u64()
                    .unwrap_or(300)
                    .max(1),
                primary_resets_at_epoch_seconds: status_bar["primary_resets_at_epoch_seconds"]
                    .as_u64(),
                source_label: "codex app-server status-bar rateLimits/read contour",
                source_kind: "status_bar_exact",
            });
        }
    }
    None
}

pub(super) fn preferred_active_agent_limit_surface(
    client_live_meter: &Value,
) -> Option<PreferredOnlineLimitSurface> {
    let status_bar = &client_live_meter["status_bar_rate_limits"];
    if status_bar["status"].as_str() == Some("observed") {
        if let (Some(observed_at_epoch_ms), Some(primary_used_percent)) = (
            status_bar["observed_at_epoch_ms"]
                .as_u64()
                .or_else(|| status_bar["ended_at_epoch_ms"].as_u64()),
            percent_value(&status_bar["primary_limit_used_percent"]),
        ) {
            let primary_remaining_percent =
                percent_value(&status_bar["primary_limit_remaining_percent"])
                    .unwrap_or((100.0 - primary_used_percent).max(0.0));
            let secondary_used_percent =
                percent_value(&status_bar["secondary_limit_used_percent"]).unwrap_or(0.0);
            let secondary_remaining_percent =
                percent_value(&status_bar["secondary_limit_remaining_percent"])
                    .unwrap_or((100.0 - secondary_used_percent).max(0.0));
            return Some(PreferredOnlineLimitSurface {
                observed_at_epoch_ms,
                primary_used_percent,
                primary_remaining_percent,
                secondary_used_percent,
                secondary_remaining_percent,
                window_duration_minutes: status_bar["primary_window_duration_mins"]
                    .as_u64()
                    .unwrap_or(300)
                    .max(1),
                primary_resets_at_epoch_seconds: status_bar["primary_resets_at_epoch_seconds"]
                    .as_u64(),
                source_label: "codex app-server status-bar rateLimits/read contour",
                source_kind: "status_bar_exact",
            });
        }
    }
    preferred_online_limit_surface(client_live_meter)
}

pub(super) fn personal_agent_online_kpi_from_client_live_meter(
    client_live_meter: &Value,
    scope_kind: &str,
    scope_label: &str,
) -> Option<Value> {
    let preferred_limits = preferred_online_limit_surface(client_live_meter)?;
    let observed_at_epoch_ms = preferred_limits.observed_at_epoch_ms;
    let primary_used_percent = preferred_limits.primary_used_percent;
    let window_duration_minutes = preferred_limits.window_duration_minutes;
    let primary_resets_at_epoch_seconds = preferred_limits.primary_resets_at_epoch_seconds?;
    let reset_at_epoch_ms = primary_resets_at_epoch_seconds.saturating_mul(1000);
    let remaining_window_minutes = if reset_at_epoch_ms <= observed_at_epoch_ms {
        0.0
    } else {
        (reset_at_epoch_ms - observed_at_epoch_ms) as f64 / 60_000.0
    };
    let elapsed_window_minutes =
        (window_duration_minutes as f64 - remaining_window_minutes).max(0.0);
    let ideal_remaining_percent =
        (remaining_window_minutes * 100.0 / window_duration_minutes as f64).clamp(0.0, 100.0);
    let ideal_used_percent = (100.0 - ideal_remaining_percent).clamp(0.0, 100.0);
    let raw_signed_kpi_percent = if ideal_used_percent <= 0.01 {
        if primary_used_percent <= 0.01 {
            0.0
        } else {
            -100.0
        }
    } else {
        (1.0 - primary_used_percent / ideal_used_percent) * 100.0
    };
    let (signed_kpi_percent, window_progress_state, window_progress_ratio) =
        damp_signed_kpi_percent_for_window_progress(
            raw_signed_kpi_percent,
            elapsed_window_minutes,
            super::DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MIN_HISTORY_SPAN_MINUTES,
        );
    let classification = signed_kpi_classification(signed_kpi_percent);
    let reply_prefix = reply_prefix_for_signed_kpi_percent(signed_kpi_percent);
    Some(json!({
        "status": "observed",
        "confidence": "online_limit_contour",
        "scope_kind": scope_kind,
        "scope_label": scope_label,
        "classification": classification,
        "kpi_percent": signed_kpi_percent.abs(),
        "signed_kpi_percent": signed_kpi_percent,
        "raw_kpi_percent": raw_signed_kpi_percent.abs(),
        "raw_signed_kpi_percent": raw_signed_kpi_percent,
        "window_progress_state": window_progress_state,
        "window_progress_ratio": window_progress_ratio,
        "elapsed_window_minutes": elapsed_window_minutes,
        "minimum_elapsed_window_minutes": super::DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MIN_HISTORY_SPAN_MINUTES,
        "reply_prefix": reply_prefix,
        "window_hours": super::PERSONAL_AGENT_KPI_WINDOW_HOURS,
        "events_total": 0,
        "counted_events": 0,
        "summary": match classification {
            "saving" => format!(
                "Личный 5ч KPI текущего active thread идёт в экономии {:.2}% по {}{}.",
                signed_kpi_percent,
                preferred_limits.source_label
                ,
                if window_progress_state == "preliminary" {
                    " (раннее окно сглажено)"
                } else {
                    ""
                }
            ),
            "overspend" => format!(
                "Личный 5ч KPI текущего active thread идёт в переплате {:.2}% по {}{}.",
                signed_kpi_percent.abs(),
                preferred_limits.source_label,
                if window_progress_state == "preliminary" {
                    " (раннее окно сглажено)"
                } else {
                    ""
                }
            ),
            _ => format!(
                "Личный 5ч KPI текущего active thread идёт примерно 1:1 по {}{}.",
                preferred_limits.source_label,
                if window_progress_state == "preliminary" {
                    " (раннее окно сглажено)"
                } else {
                    ""
                }
            ),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn personal_agent_online_kpi_from_client_live_meter_uses_raw_thread_limit_contour() {
        let value = personal_agent_online_kpi_from_client_live_meter(
            &json!({
                "status": "observed",
                "current_thread_bound": true,
                "ended_at_epoch_ms": 1775056740000u64,
                "primary_limit_used_percent": 14.0,
                "primary_window_duration_mins": 300,
                "primary_resets_at_epoch_seconds": 1775063220u64,
                "status_bar_rate_limits": {
                    "status": "observed",
                    "observed_at_epoch_ms": 1775056740000u64,
                    "primary_limit_used_percent": 14.0,
                    "primary_window_duration_mins": 300,
                    "primary_resets_at_epoch_seconds": 1775063220u64
                }
            }),
            "personal_thread_scope",
            "thread-bounty",
        )
        .expect("online kpi");
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(value["confidence"].as_str(), Some("online_limit_contour"));
        assert_eq!(value["scope_kind"].as_str(), Some("personal_thread_scope"));
        assert_eq!(value["scope_label"].as_str(), Some("thread-bounty"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 78.12%")
        );
        assert!(
            value["summary"]
                .as_str()
                .is_some_and(|summary| summary.contains("thread-local"))
        );
    }

    #[test]
    fn personal_agent_online_kpi_from_client_live_meter_prefers_thread_local_contour_over_status_bar()
     {
        let value = personal_agent_online_kpi_from_client_live_meter(
            &json!({
                "status": "observed",
                "current_thread_bound": true,
                "ended_at_epoch_ms": 1775056740000u64,
                "primary_limit_used_percent": 93.0,
                "primary_limit_remaining_percent": 7.0,
                "secondary_limit_used_percent": 40.0,
                "secondary_limit_remaining_percent": 60.0,
                "primary_window_duration_mins": 300,
                "primary_resets_at_epoch_seconds": 1775063220u64,
                "status_bar_rate_limits": {
                    "status": "observed",
                    "observed_at_epoch_ms": 1775056740000u64,
                    "primary_limit_used_percent": 7.0,
                    "primary_limit_remaining_percent": 93.0,
                    "secondary_limit_used_percent": 2.0,
                    "secondary_limit_remaining_percent": 98.0,
                    "primary_window_duration_mins": 300,
                    "primary_resets_at_epoch_seconds": 1775063220u64
                }
            }),
            "personal_thread_scope",
            "thread-amai",
        )
        .expect("online kpi");
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(value["confidence"].as_str(), Some("online_limit_contour"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 45.31%")
        );
        assert!(
            value["summary"]
                .as_str()
                .is_some_and(|summary| summary.contains("thread-local"))
        );
    }

    #[test]
    fn personal_agent_online_kpi_from_client_live_meter_damps_before_min_history_span() {
        let value = personal_agent_online_kpi_from_client_live_meter(
            &json!({
                "status": "observed",
                "current_thread_bound": true,
                "ended_at_epoch_ms": 300_000u64,
                "primary_limit_used_percent": 11.0,
                "primary_window_duration_mins": 300,
                "primary_resets_at_epoch_seconds": 18_000u64,
                "status_bar_rate_limits": {
                    "status": "observed",
                    "observed_at_epoch_ms": 300_000u64,
                    "primary_limit_used_percent": 11.0,
                    "primary_window_duration_mins": 300,
                    "primary_resets_at_epoch_seconds": 18_000u64
                }
            }),
            "personal_thread_scope",
            "thread-amai",
        )
        .expect("online kpi");
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(value["confidence"].as_str(), Some("online_limit_contour"));
        assert_eq!(value["window_progress_state"].as_str(), Some("preliminary"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 50.91%")
        );
        assert_eq!(value["minimum_elapsed_window_minutes"].as_u64(), Some(55));
    }
}
