use crate::codex_threads;
use crate::config::{self, AppConfig};
use crate::continuity;
use crate::dashboard_format::*;
use crate::hardware_telemetry::{AcceleratorSummary, MachineSummary, collect_machine_summary};
use crate::onboarding;
use crate::working_state;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

mod dashboard_benchmark_cards;
mod dashboard_card_support;
mod dashboard_client_budget_diagnostics;
mod dashboard_client_budget_support;
mod dashboard_client_limit_alignment;
mod dashboard_context;
mod dashboard_current_session_budget_guard;
mod dashboard_hero_cards;
mod dashboard_live_latency_compare;
mod dashboard_payload;
mod dashboard_renderer;
mod dashboard_runtime_support;
mod dashboard_service_cards;
mod dashboard_working_state_card;
use self::dashboard_benchmark_cards::build_benchmark_cards;
pub use self::dashboard_card_support::monitoring_url;
use self::dashboard_card_support::{
    card, tcp_port_is_open, with_extra_class, with_status, with_status_label, with_status_tooltip,
    with_table_orientation,
};
use self::dashboard_client_budget_diagnostics::*;
#[allow(unused_imports)]
pub(crate) use self::dashboard_client_budget_diagnostics::{
    client_budget_root_cause_payload, client_budget_root_cause_payload_with_guard,
};
pub(crate) use self::dashboard_client_budget_support::client_budget_live_payload;
use self::dashboard_client_budget_support::*;
use self::dashboard_client_limit_alignment::*;
pub use self::dashboard_context::browser_base_url;
#[cfg(test)]
use self::dashboard_current_session_budget_guard::build_client_budget_reply_execution_gate_with_primary_command;
pub use self::dashboard_current_session_budget_guard::current_session_budget_guard;
pub(crate) use self::dashboard_hero_cards::build_active_agent_budget_session_card_from_surface;
use self::dashboard_hero_cards::{
    build_active_agent_budget_session_card, build_hero_cards,
    humanize_tracked_slice_exactness_value, humanize_tracked_slice_savings_value,
};
use self::dashboard_live_latency_compare::{
    live_latency_compare_card, live_latency_compare_status,
};
pub use self::dashboard_payload::{build_live_summary_payload, build_payload};
pub use self::dashboard_renderer::render_html;
#[cfg(test)]
use self::dashboard_runtime_support::artifact_cleanup_warning;
use self::dashboard_runtime_support::{
    build_glossary, build_governance_card, build_links, build_machine_cards, build_warnings,
};
#[cfg(test)]
use self::dashboard_service_cards::benchmark_qdrant_live_card;
use self::dashboard_service_cards::build_service_cards;
use self::dashboard_working_state_card::*;

pub use crate::dashboard_assets::{brand_lockup_svg, brand_mark_svg, favicon_ico};

#[cfg(test)]
fn compact_chat_selector_client_surface(restore_context: &Value) -> Value {
    dashboard_client_budget_support::compact_chat_selector_client_surface(restore_context)
}

fn slowest_observe_refresh_stage(snapshot: &Value) -> (Option<String>, Option<u64>) {
    let mut slowest: Option<(&str, u64)> = None;
    for (label, value) in snapshot["observe_refresh"]["stage_ms"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let Some(duration_ms) = value.as_u64() else {
            continue;
        };
        match slowest {
            Some((_, current_max)) if current_max >= duration_ms => {}
            _ => slowest = Some((label.as_str(), duration_ms)),
        }
    }
    slowest
        .map(|(label, duration_ms)| (Some(label.to_string()), Some(duration_ms)))
        .unwrap_or((None, None))
}

fn build_headline(snapshot: &Value, captured_at_epoch_ms: u64) -> Value {
    let pass = snapshot["sla"]["summary"]["pass"].as_u64().unwrap_or(0);
    let alert = snapshot["sla"]["summary"]["alert"].as_u64().unwrap_or(0);
    let critical = snapshot["sla"]["summary"]["critical"].as_u64().unwrap_or(0);
    let unknown = snapshot["sla"]["summary"]["unknown"].as_u64().unwrap_or(0);
    let token_headline = &snapshot["token_budget_report"]["token_budget_report"]["headline"];
    let active_agent_headline = &snapshot["active_agent_budget"]["headline"];
    let sla_status = if critical > 0 {
        "critical"
    } else if alert > 0 {
        "alert"
    } else if unknown > 0 {
        "unknown"
    } else {
        "pass"
    };
    let live_status = live_latency_compare_status(snapshot);
    let status = combine_headline_statuses(sla_status, live_status);
    json!({
        "status": status,
        "status_label": headline_status_label(status),
        "status_reason": headline_status_reason(pass, alert, critical, unknown, live_status),
        "captured_at": human_timestamp(captured_at_epoch_ms),
        "summary": format!("SLA сейчас: pass={pass}, alert={alert}, critical={critical}, unknown={unknown}"),
        "token_title": active_agent_headline["title"]
            .as_str()
            .or_else(|| token_headline["title"].as_str())
            .unwrap_or("ещё нет данных"),
        "token_value": active_agent_headline["value_text"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format_percent(token_headline["value_percent"].as_f64())),
        "token_scope": if active_agent_headline.is_object() { "" } else { token_headline["scope_label"].as_str().unwrap_or("") },
    })
}

fn build_top_cards(snapshot: &Value) -> Vec<Value> {
    vec![
        live_latency_compare_card(snapshot),
        working_state_live_card(snapshot),
    ]
}

fn humanize_identifier(value: &str) -> String {
    value
        .split(['_', '-', '/', ':'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let head = first.to_uppercase().collect::<String>();
                    let tail = chars.as_str().to_lowercase();
                    format!("{head}{tail}")
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn hot_retrieval_benchmark_status(hot_retrieval: &Value, thresholds: &Value) -> &'static str {
    combine_statuses(&[
        status_strict_less_than(
            hot_retrieval["p50_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_p50_ms"].as_f64(),
        ),
        status_strict_less_than(
            hot_retrieval["p95_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_p95_ms"].as_f64(),
        ),
        status_strict_less_than(
            hot_retrieval["p99_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_p99_ms"].as_f64(),
        ),
        status_strict_less_than(
            hot_retrieval["max_ms"].as_f64(),
            thresholds["retrieval"]["hot_live_table"]["target_max_ms"].as_f64(),
        ),
        status_at_least_or_equal(
            hot_retrieval["iterations"].as_f64(),
            thresholds["retrieval"]["hot_benchmark_table"]["target_iterations"].as_f64(),
        ),
        status_at_least_or_equal(
            hot_retrieval["warmup"].as_f64(),
            thresholds["retrieval"]["hot_benchmark_table"]["target_warmup"].as_f64(),
        ),
    ])
}

fn hot_load_benchmark_reasons(
    snapshot: &Value,
    hot_load: &Value,
    thresholds: &Value,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let sample_count = hot_load["success_count"]
        .as_u64()
        .zip(hot_load["error_count"].as_u64())
        .map(|(success, errors)| success + errors);

    if let Some(reason) = failing_metric_reason_strict_more(
        "Burst QPS",
        hot_load["qps"].as_f64(),
        thresholds["load"]["hot_qps"]["target"].as_f64(),
        format_burst_qps_table(hot_load["qps"].as_f64()),
        format_burst_qps_threshold(thresholds["load"]["hot_qps"]["target"].as_f64(), ">"),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_at_most_or_equal(
        "Error rate",
        hot_load["error_rate"].as_f64(),
        thresholds["load"]["hot_error_rate"]["target"].as_f64(),
        format_percent(hot_load["error_rate"].as_f64()),
        format_zero_or_at_most_percent(
            thresholds["load"]["hot_error_rate"]
                .get("target")
                .and_then(Value::as_f64),
        ),
    ) {
        reasons.push(reason);
    }
    for (label, value_key, target_key) in [
        ("P50", "p50_ms", "target_p50_ms"),
        ("P95", "p95_ms", "target_p95_ms"),
        ("P99", "p99_ms", "target_p99_ms"),
        ("Max", "max_ms", "target_max_ms"),
    ] {
        if let Some(reason) = failing_metric_reason_strict_less(
            label,
            hot_load[value_key].as_f64(),
            thresholds["load"]["hot_benchmark_table"][target_key].as_f64(),
            format_ms(snapshot, hot_load[value_key].as_f64()),
            format_time_threshold(
                snapshot,
                thresholds["load"]["hot_benchmark_table"][target_key].as_f64(),
                "<",
            ),
        ) {
            reasons.push(reason);
        }
    }
    if let Some(reason) = failing_metric_reason_strict_more(
        "Workers",
        hot_load["workers"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_workers"].as_f64(),
        format_u64(hot_load["workers"].as_u64()),
        format_threshold_at_least(
            thresholds["load"]["hot_benchmark_table"]["target_workers"].as_f64(),
            "",
            0,
        ),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_strict_more(
        "Выборка",
        sample_count.map(|value| value as f64),
        thresholds["load"]["hot_benchmark_table"]["target_sample_count"].as_f64(),
        format_u64(sample_count),
        format_threshold_at_least(
            thresholds["load"]["hot_benchmark_table"]["target_sample_count"].as_f64(),
            "",
            0,
        ),
    ) {
        reasons.push(reason);
    }
    reasons
}

fn hot_retrieval_benchmark_reasons(
    snapshot: &Value,
    hot_retrieval: &Value,
    thresholds: &Value,
) -> Vec<String> {
    let mut reasons = Vec::new();
    for (label, value_key, target_key) in [
        ("P50", "p50_ms", "target_p50_ms"),
        ("P95", "p95_ms", "target_p95_ms"),
        ("P99", "p99_ms", "target_p99_ms"),
        ("Max", "max_ms", "target_max_ms"),
    ] {
        if let Some(reason) = failing_metric_reason_strict_less(
            label,
            hot_retrieval[value_key].as_f64(),
            thresholds["retrieval"]["hot_live_table"][target_key].as_f64(),
            format_ms(snapshot, hot_retrieval[value_key].as_f64()),
            format_time_threshold(
                snapshot,
                thresholds["retrieval"]["hot_live_table"][target_key].as_f64(),
                "<",
            ),
        ) {
            reasons.push(reason);
        }
    }
    if let Some(reason) = failing_metric_reason_at_least_or_equal(
        "Итерации",
        hot_retrieval["iterations"].as_f64(),
        thresholds["retrieval"]["hot_benchmark_table"]["target_iterations"].as_f64(),
        format_u64(hot_retrieval["iterations"].as_u64()),
        format_threshold_at_least_or_equal(
            thresholds["retrieval"]["hot_benchmark_table"]["target_iterations"].as_f64(),
            "",
            0,
        ),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_at_least_or_equal(
        "Warmup",
        hot_retrieval["warmup"].as_f64(),
        thresholds["retrieval"]["hot_benchmark_table"]["target_warmup"].as_f64(),
        format_u64(hot_retrieval["warmup"].as_u64()),
        format_threshold_at_least_or_equal(
            thresholds["retrieval"]["hot_benchmark_table"]["target_warmup"].as_f64(),
            "",
            0,
        ),
    ) {
        reasons.push(reason);
    }
    reasons
}

fn cold_benchmark_reasons(snapshot: &Value, cold_contour: &Value) -> Vec<String> {
    let mut reasons = Vec::new();
    let profile = &cold_contour["profile"];
    let summary = &cold_contour["machine_readable_summary"];
    for (label, value_key, target_key) in [
        ("Cold P50", "p50", "target_p50_ms"),
        ("Cold P95", "p95", "target_p95_ms"),
        ("Cold P99", "p99", "target_p99_ms"),
        ("Cold Max", "max", "target_max_ms"),
    ] {
        if let Some(reason) = failing_metric_reason_strict_less(
            label,
            summary[value_key].as_f64(),
            profile[target_key].as_f64(),
            format_ms(snapshot, summary[value_key].as_f64()),
            format_time_threshold(snapshot, profile[target_key].as_f64(), "<"),
        ) {
            reasons.push(reason);
        }
    }
    for (label, value_key, target_key) in [
        ("Precision", "precision", "min_precision"),
        ("Recall", "recall", "min_recall"),
        ("Hit rate", "hit_rate", "min_target_hit_rate"),
    ] {
        if let Some(reason) = failing_metric_reason_at_least_or_equal(
            label,
            summary[value_key].as_f64().map(|value| value * 100.0),
            profile[target_key].as_f64().map(|value| value * 100.0),
            format_ratio_percent(summary[value_key].as_f64()),
            format_threshold_value(
                profile[target_key].as_f64().map(|value| value * 100.0),
                ">=",
                "%",
                2,
            ),
        ) {
            reasons.push(reason);
        }
    }
    for (label, value_key, target_key) in [
        ("Выборка", "sample_count", "min_sample_count"),
        ("Repo count", "repo_count", "min_repo_count"),
        ("Query slices", "query_slice_count", "min_query_slice_count"),
    ] {
        if let Some(reason) = failing_metric_reason_at_least_or_equal(
            label,
            summary[value_key].as_f64(),
            profile[target_key].as_f64(),
            format_u64(summary[value_key].as_u64()),
            format_threshold_at_least_or_equal(profile[target_key].as_f64(), "", 0),
        ) {
            reasons.push(reason);
        }
    }
    if let Some(reason) = failing_metric_reason_strict_less(
        "Duration",
        summary["duration"].as_f64(),
        profile["max_duration_seconds"].as_f64(),
        format_seconds(snapshot, summary["duration"].as_f64()),
        format_threshold_rendered(
            "<",
            format_seconds(snapshot, profile["max_duration_seconds"].as_f64()),
        ),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_at_most_or_equal(
        "Leakage",
        summary["leakage"].as_f64(),
        profile["max_leakage"].as_f64(),
        format_u64(summary["leakage"].as_u64()),
        format_threshold_value(profile["max_leakage"].as_f64(), "=", "", 0),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_at_most_or_equal(
        "Error rate",
        summary["error_rate"].as_f64().map(|value| value * 100.0),
        profile["max_error_rate"]
            .as_f64()
            .map(|value| value * 100.0),
        format_percent(summary["error_rate"].as_f64()),
        format_zero_or_at_most_percent(
            profile["max_error_rate"]
                .as_f64()
                .map(|value| value * 100.0),
        ),
    ) {
        reasons.push(reason);
    }
    reasons
}

fn cold_benchmark_progress_reasons(
    snapshot: &Value,
    cold_contour: &Value,
    progress: &Value,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let completed = progress["progress"]["completed_case_count"]
        .as_u64()
        .unwrap_or(0);
    let target = progress["progress"]["target_case_count"]
        .as_u64()
        .unwrap_or(0);
    reasons.push(format!(
        "Прогон ещё не завершён: собрано {} из {} cold-case.",
        format_u64(Some(completed)),
        format_u64(Some(target))
    ));
    if let Some(phase) = progress["phase"].as_str() {
        reasons.push(format!("Текущая фаза: {phase}."));
    }
    if let Some(current_repo_code) = progress["current_repo_code"].as_str() {
        let current_repo_name = progress["current_repo_display_name"]
            .as_str()
            .unwrap_or(current_repo_code);
        let indexed = progress["progress"]["current_repo_indexed_files"].as_u64();
        let target = progress["progress"]["current_repo_target_files"].as_u64();
        if indexed.is_some() || target.is_some() {
            reasons.push(format!(
                "Сейчас индексируется репозиторий {}: {} из {} файлов уже записаны в индекс.",
                current_repo_name,
                format_u64(indexed),
                format_u64(target),
            ));
        }
    }
    if cold_contour["machine_readable_summary"]["sample_count"].as_u64() == Some(0) {
        reasons.push(
            "Пока нет ни одного завершённого cold-case, поэтому latency и quality ещё не накопились."
                .to_string(),
        );
        return reasons;
    }
    reasons.extend(cold_benchmark_reasons(snapshot, cold_contour));
    reasons
}

fn accuracy_benchmark_reasons(accuracy: &Value, thresholds: &Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(reason) = failing_metric_reason_at_most_or_equal(
        "Leakage",
        accuracy["cross_project_leakage"].as_f64(),
        Some(0.0),
        format_f64_count(accuracy["cross_project_leakage"].as_f64()),
        "0".to_string(),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_at_least_or_equal(
        "Symbol precision",
        accuracy["symbol_precision"]
            .as_f64()
            .map(|value| value * 100.0),
        thresholds["accuracy"]["symbol_precision"]["target"]
            .as_f64()
            .map(|value| value * 100.0),
        format_ratio_percent(accuracy["symbol_precision"].as_f64()),
        format_ratio_percent(thresholds["accuracy"]["symbol_precision"]["target"].as_f64()),
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = failing_metric_reason_at_least_or_equal(
        "Semantic precision",
        accuracy["semantic_precision"]
            .as_f64()
            .map(|value| value * 100.0),
        thresholds["accuracy"]["semantic_precision"]["target"]
            .as_f64()
            .map(|value| value * 100.0),
        format_ratio_percent(accuracy["semantic_precision"].as_f64()),
        format_ratio_percent(thresholds["accuracy"]["semantic_precision"]["target"].as_f64()),
    ) {
        reasons.push(reason);
    }
    reasons
}

fn sla_metric_reasons(snapshot: &Value, metrics: &[&str]) -> Vec<String> {
    let mut reasons = Vec::new();
    for metric in metrics {
        if let Some(check) = snapshot["sla"]["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|check| check["metric"].as_str() == Some(*metric))
        {
            if check["status"].as_str() != Some("pass") {
                reasons.push(humanize_check(snapshot, check));
            }
        } else {
            reasons.push(format!("Для метрики {metric} пока нет свежего SLA-среза."));
        }
    }
    reasons
}

fn token_budget_report_root<'a>(snapshot: &'a Value) -> &'a Value {
    if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    }
}

fn live_response_latency_root<'a>(snapshot: &'a Value) -> Option<&'a Value> {
    let root = token_budget_report_root(snapshot);
    if root["live_response_latency"].is_object() {
        Some(&root["live_response_latency"])
    } else if root["current_session"].is_object()
        || root["rolling_window"].is_object()
        || root["current_session_relation"].is_object()
        || root["current_session_exclusions"].is_object()
        || root["current_thread_live_file_hints"].is_object()
    {
        Some(root)
    } else {
        None
    }
}

fn live_response_latency_current_thread_file_hints(snapshot: &Value) -> Vec<String> {
    live_response_latency_root(snapshot)
        .and_then(|root| root["current_thread_live_file_hints"]["hints"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item["label"].as_str().map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn compact_dashboard_text(value: Option<&str>, max_chars: usize, fallback: &str) -> String {
    let text = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}

fn card_with_rows(
    title: &str,
    value: String,
    note: String,
    status: &str,
    source_label: Option<String>,
    title_tooltip: Option<String>,
    rows: Vec<Value>,
) -> Value {
    json!({
        "title": title,
        "value": value,
        "note": note,
        "status": status,
        "status_label": status_label(status),
        "status_tooltip": Value::Null,
        "source_label": source_label,
        "title_tooltip": title_tooltip,
        "rows": rows,
    })
}

fn metric_row(label: &str, value: String, tooltip: Option<&str>) -> Value {
    json!({
        "label": label,
        "value": value,
        "tooltip": tooltip,
    })
}

fn metric_row_with_key(key: &str, label: &str, value: String, tooltip: Option<&str>) -> Value {
    let mut row = metric_row(label, value, tooltip);
    if let Some(root) = row.as_object_mut() {
        root.insert("key".to_string(), Value::from(key));
    }
    row
}

const CLIENT_LIVE_CONTEXT_ROW_KEY: &str = "client_live_context";
const CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY: &str = "client_live_full_turn_savings";
const CLIENT_LIVE_LIMIT_ROW_KEY: &str = "client_live_limit";
const CLIENT_LIMIT_HOURLY_BURN_ROW_KEY: &str = "client_limit_hourly_burn";

fn status_reason_tooltip(status: &str, reasons: Vec<String>, fallback: &str) -> Option<String> {
    if status == "pass" {
        return None;
    }
    let intro = match status {
        "critical" => "Статус стал критичным по следующим причинам:",
        "alert" => "Статус требует внимания по следующим причинам:",
        "waiting" => "Статус пока не может считаться нормальным по следующим причинам:",
        _ => "Статус пока не может считаться нормальным по следующим причинам:",
    };
    if reasons.is_empty() {
        Some(format!("{intro}\n- {fallback}"))
    } else {
        Some(format!("{intro}\n- {}", reasons.join("\n- ")))
    }
}

fn failing_metric_reason_strict_less(
    label: &str,
    current: Option<f64>,
    target: Option<f64>,
    current_value: String,
    target_value: String,
) -> Option<String> {
    match (current, target) {
        (Some(current), Some(target)) if current < target => None,
        (Some(_), Some(_)) => Some(format!(
            "{label} вышел за эталон: сейчас {current_value}, цель {target_value}."
        )),
        _ => Some(format!(
            "{label} пока нельзя оценить: не хватает текущего значения или эталона."
        )),
    }
}

fn failing_metric_reason_strict_more(
    label: &str,
    current: Option<f64>,
    target: Option<f64>,
    current_value: String,
    target_value: String,
) -> Option<String> {
    match (current, target) {
        (Some(current), Some(target)) if current > target => None,
        (Some(_), Some(_)) => Some(format!(
            "{label} ниже эталона: сейчас {current_value}, цель {target_value}."
        )),
        _ => Some(format!(
            "{label} пока нельзя оценить: не хватает текущего значения или эталона."
        )),
    }
}

fn failing_metric_reason_at_most_or_equal(
    label: &str,
    current: Option<f64>,
    target: Option<f64>,
    current_value: String,
    target_value: String,
) -> Option<String> {
    match (current, target) {
        (Some(current), Some(target)) if current <= target => None,
        (Some(_), Some(_)) => Some(format!(
            "{label} вышел за допустимую границу: сейчас {current_value}, цель {target_value}."
        )),
        _ => Some(format!(
            "{label} пока нельзя оценить: не хватает текущего значения или эталона."
        )),
    }
}

fn failing_metric_reason_at_least_or_equal(
    label: &str,
    current: Option<f64>,
    target: Option<f64>,
    current_value: String,
    target_value: String,
) -> Option<String> {
    match (current, target) {
        (Some(current), Some(target)) if current >= target => None,
        (Some(_), Some(_)) => Some(format!(
            "{label} ниже минимально допустимого уровня: сейчас {current_value}, цель {target_value}."
        )),
        _ => Some(format!(
            "{label} пока нельзя оценить: не хватает текущего значения или эталона."
        )),
    }
}

fn hot_load_benchmark_status(hot_load: &Value, thresholds: &Value) -> &'static str {
    let qps_status = status_strict_more_than(
        hot_load["qps"].as_f64(),
        thresholds["load"]["hot_qps"]["target"].as_f64(),
    );
    let error_status = status_at_most_or_equal(
        hot_load["error_rate"].as_f64(),
        thresholds["load"]["hot_error_rate"]["target"].as_f64(),
    );
    let p50_status = status_strict_less_than(
        hot_load["p50_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_p50_ms"].as_f64(),
    );
    let p95_status = status_strict_less_than(
        hot_load["p95_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_p95_ms"].as_f64(),
    );
    let p99_status = status_strict_less_than(
        hot_load["p99_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_p99_ms"].as_f64(),
    );
    let max_status = status_strict_less_than(
        hot_load["max_ms"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_max_ms"].as_f64(),
    );
    let workers_status = status_strict_more_than(
        hot_load["workers"].as_f64(),
        thresholds["load"]["hot_benchmark_table"]["target_workers"].as_f64(),
    );
    let sample_count = hot_load["success_count"]
        .as_u64()
        .zip(hot_load["error_count"].as_u64())
        .map(|(success, errors)| (success + errors) as f64);
    let sample_status = status_strict_more_than(
        sample_count,
        thresholds["load"]["hot_benchmark_table"]["target_sample_count"].as_f64(),
    );
    combine_statuses(&[
        qps_status,
        error_status,
        p50_status,
        p95_status,
        p99_status,
        max_status,
        workers_status,
        sample_status,
    ])
}

fn status_strict_less_than(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current < target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn status_strict_more_than(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current > target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn status_at_most_or_equal(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current <= target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn status_at_least_or_equal(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current >= target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

fn compare_values(snapshot: &Value, slice: Option<&Value>, sample_count: u64) -> Vec<String> {
    if sample_count == 0 {
        return vec![
            "ещё нет данных".to_string(),
            "ещё нет данных".to_string(),
            "ещё нет данных".to_string(),
            "ещё нет данных".to_string(),
            "0".to_string(),
        ];
    }
    vec![
        format_ms(
            snapshot,
            slice.and_then(|value| value["p50_latency_ms"].as_f64()),
        ),
        format_ms(
            snapshot,
            slice.and_then(|value| value["p95_latency_ms"].as_f64()),
        ),
        format_ms(
            snapshot,
            slice.and_then(|value| value["p99_latency_ms"].as_f64()),
        ),
        format_ms(
            snapshot,
            slice.and_then(|value| value["max_latency_ms"].as_f64()),
        ),
        format_u64(Some(sample_count)),
    ]
}

#[derive(Debug, Clone, Copy)]
struct LiveLatencyTableTargets {
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    live_readiness_sample_count: u64,
    benchmark_sample_count: u64,
}

struct LiveLatencySliceAssessment {
    status: &'static str,
    note: String,
}

fn default_live_latency_table_targets(state: &str) -> LiveLatencyTableTargets {
    match state {
        "hot" => LiveLatencyTableTargets {
            p50_ms: 1.0,
            p95_ms: 2.0,
            p99_ms: 3.0,
            max_ms: 5.0,
            live_readiness_sample_count: 100,
            benchmark_sample_count: 100000,
        },
        _ => LiveLatencyTableTargets {
            p50_ms: 2.0,
            p95_ms: 4.0,
            p99_ms: 6.0,
            max_ms: 10.0,
            live_readiness_sample_count: 100,
            benchmark_sample_count: 10000,
        },
    }
}

fn live_latency_table_targets(snapshot: &Value, state: &str) -> LiveLatencyTableTargets {
    let defaults = default_live_latency_table_targets(state);
    let thresholds = if state == "hot" {
        &snapshot["thresholds"]["retrieval"]["hot_live_table"]
    } else {
        &snapshot["thresholds"]["retrieval"]["cold_live_table"]
    };
    LiveLatencyTableTargets {
        p50_ms: thresholds["target_p50_ms"]
            .as_f64()
            .filter(|value| *value > 0.0)
            .unwrap_or(defaults.p50_ms),
        p95_ms: thresholds["target_p95_ms"]
            .as_f64()
            .filter(|value| *value > 0.0)
            .unwrap_or(defaults.p95_ms),
        p99_ms: thresholds["target_p99_ms"]
            .as_f64()
            .filter(|value| *value > 0.0)
            .unwrap_or(defaults.p99_ms),
        max_ms: thresholds["target_max_ms"]
            .as_f64()
            .filter(|value| *value > 0.0)
            .unwrap_or(defaults.max_ms),
        live_readiness_sample_count: thresholds["live_readiness_sample_count"]
            .as_u64()
            .or_else(|| thresholds["target_sample_count"].as_u64())
            .filter(|value| *value > 0)
            .unwrap_or(defaults.live_readiness_sample_count),
        benchmark_sample_count: thresholds["benchmark_sample_count"]
            .as_u64()
            .or_else(|| thresholds["target_sample_count"].as_u64())
            .filter(|value| *value > 0)
            .unwrap_or(defaults.benchmark_sample_count),
    }
}

fn target_values(snapshot: &Value, targets: &LiveLatencyTableTargets) -> Vec<String> {
    vec![
        format_time_threshold(snapshot, Some(targets.p50_ms), "<="),
        format_time_threshold(snapshot, Some(targets.p95_ms), "<="),
        format_time_threshold(snapshot, Some(targets.p99_ms), "<="),
        format_time_threshold(snapshot, Some(targets.max_ms), "<="),
        format_target_u64(">=", targets.live_readiness_sample_count),
    ]
}

fn status_label(status: &str) -> &'static str {
    match status {
        "pass" => "в норме",
        "alert" => "внимание",
        "critical" => "критично",
        "waiting" => "ждём подтверждённую выборку",
        _ => "нет данных",
    }
}

fn headline_status_label(status: &str) -> &'static str {
    match status {
        "pass" => "система в норме",
        "alert" => "нужно внимание",
        "critical" => "есть критичные сигналы",
        "waiting" => "данных пока мало",
        _ => "данных пока мало",
    }
}

fn headline_status_reason(
    pass: u64,
    alert: u64,
    critical: u64,
    unknown: u64,
    live_status: &str,
) -> String {
    let mut base = if critical > 0 {
        format!("Критичных SLA-проверок: {critical}. Предупреждений: {alert}.")
    } else if alert > 0 {
        format!("SLA-предупреждений: {alert}. Критичных SLA-проверок нет.")
    } else if unknown > 0 {
        format!("Неопределённых SLA-проверок: {unknown}. Остальные зелёные: {pass}.")
    } else {
        format!("Все SLA-проверки зелёные: {pass}.")
    };

    match live_status {
        "critical" => {
            base.push_str(" Живой пользовательский поток сейчас в критичном состоянии.");
        }
        "alert" => {
            base.push_str(" Живой пользовательский поток сейчас требует внимания.");
        }
        "unknown" => {
            base.push_str(" По живому пользовательскому потоку пока недостаточно данных.");
        }
        _ => {}
    }

    base
}

fn assess_live_latency_slice(
    slice: Option<&Value>,
    targets: &LiveLatencyTableTargets,
) -> LiveLatencySliceAssessment {
    let Some(slice) = slice else {
        return LiveLatencySliceAssessment {
            status: "unknown",
            note: "В живом окне ещё не накопилась выборка для этого режима.".to_string(),
        };
    };

    let sample_count = slice["sample_count"].as_u64().unwrap_or_default();
    if sample_count == 0 {
        return LiveLatencySliceAssessment {
            status: "unknown",
            note: "В живом окне ещё не накопилась выборка для этого режима.".to_string(),
        };
    }

    let metrics = [
        ("P50", slice["p50_latency_ms"].as_f64(), targets.p50_ms),
        ("P95", slice["p95_latency_ms"].as_f64(), targets.p95_ms),
        ("P99", slice["p99_latency_ms"].as_f64(), targets.p99_ms),
        ("Max", slice["max_latency_ms"].as_f64(), targets.max_ms),
    ];

    let missing_metrics = metrics
        .iter()
        .filter_map(|(label, value, _)| value.is_none().then_some(*label))
        .collect::<Vec<_>>();
    if !missing_metrics.is_empty() {
        return LiveLatencySliceAssessment {
            status: "unknown",
            note: format!(
                "Часть живых значений ещё не собрана: {}.",
                missing_metrics.join(", ")
            ),
        };
    }

    let failed_metrics = metrics
        .iter()
        .filter_map(|(label, value, target)| {
            (!value.is_some_and(|value| value <= *target)).then_some(*label)
        })
        .collect::<Vec<_>>();
    let sample_ok = sample_count >= targets.live_readiness_sample_count;

    if !sample_ok {
        return LiveLatencySliceAssessment {
            status: "waiting",
            note: if failed_metrics.is_empty() {
                format!(
                    "По задержке всё хорошо, но живое окно ещё мало: {} из >= {}. Строгая проверочная выборка отдельно: > {}.",
                    format_u64(Some(sample_count)),
                    format_u64(Some(targets.live_readiness_sample_count)),
                    format_u64(Some(targets.benchmark_sample_count))
                )
            } else {
                format!(
                    "Пока рано делать строгий вывод: живое окно ещё мало ({} из >= {}), а текущие значения ещё не лучше эталона по {}. Строгая проверочная выборка отдельно: > {}.",
                    format_u64(Some(sample_count)),
                    format_u64(Some(targets.live_readiness_sample_count)),
                    failed_metrics.join(", "),
                    format_u64(Some(targets.benchmark_sample_count))
                )
            },
        };
    }

    if !failed_metrics.is_empty() {
        return LiveLatencySliceAssessment {
            status: "critical",
            note: format!(
                "Живой эталон уже не выполняется по {}. Живая выборка: {}. Строгая проверочная норма показывается отдельно.",
                failed_metrics.join(", "),
                format_u64(Some(sample_count))
            ),
        };
    }

    LiveLatencySliceAssessment {
        status: "pass",
        note: format!(
            "Живой эталон выдержан. Живая выборка: {}. Строгая проверочная норма показывается отдельно.",
            format_u64(Some(sample_count))
        ),
    }
}

fn combine_live_compare_status(statuses: &[&str]) -> &'static str {
    if statuses.contains(&"critical") {
        return "critical";
    }
    if statuses.contains(&"alert") {
        return "alert";
    }
    if statuses.iter().all(|status| *status == "pass") {
        return "pass";
    }
    if statuses.contains(&"waiting") {
        return "waiting";
    }
    "unknown"
}

fn combine_headline_statuses(sla_status: &str, live_status: &str) -> &'static str {
    match live_status {
        "critical" => "critical",
        "alert" => {
            if sla_status == "critical" {
                "critical"
            } else {
                "alert"
            }
        }
        _ => match sla_status {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        },
    }
}

fn cold_contour_status(snapshot: &Value) -> &'static str {
    match snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["executive_summary"]["verdict"]
        .as_str()
    {
        Some("TARGET MET") => "pass",
        Some("PARTIALLY MET") => "alert",
        Some("NOT MET") => "critical",
        _ => "unknown",
    }
}

fn reviewed_frozen_debt_export_note_sentence(alignment: &Value) -> Option<&'static str> {
    let surface = &alignment["reviewed_frozen_debt_export_surface"];
    if surface["export_ready_report_only"].as_bool() != Some(true) {
        return None;
    }
    Some(
        "Исторический frozen debt уже вынесен в отдельный report-only export contour: его можно review-ить отдельно, но он не имеет права притворяться raw exact history.",
    )
}

fn historical_frozen_debt_note_sentence(
    current_session_alignment: &Value,
    rolling_window_alignment: &Value,
    lifetime_alignment: &Value,
) -> Option<&'static str> {
    historical_frozen_debt_metric_row(
        current_session_alignment,
        rolling_window_alignment,
        lifetime_alignment,
    )?;
    Some(
        "Текущая сессия и рабочее окно уже exact: frozen debt сейчас остался только в историческом lifetime-хвосте и не выглядит как новый live drift.",
    )
}

fn historical_frozen_debt_metric_row(
    current_session_alignment: &Value,
    rolling_window_alignment: &Value,
    lifetime_alignment: &Value,
) -> Option<Value> {
    let current_exact = current_session_alignment["exact_pair_status"]["exact_pair_available"]
        .as_bool()
        == Some(true);
    let rolling_exact = rolling_window_alignment["exact_pair_status"]["exact_pair_available"]
        .as_bool()
        == Some(true);
    let frozen_gap_review_surface = &lifetime_alignment["frozen_gap_review_surface"];
    if !(current_exact
        && rolling_exact
        && frozen_gap_review_surface["state"].as_str() == Some("review_required"))
    {
        return None;
    }
    let blocker_code = frozen_gap_review_surface["blocking_component"]
        .as_str()
        .unwrap_or("unknown_blocker");
    let irrecoverable_missing_live_events =
        frozen_gap_review_surface["irrecoverable_missing_live_events"]
            .as_u64()
            .unwrap_or(0);
    let tooltip = format!(
        "Этот ряд показывает, что frozen debt сейчас уже не растёт в активных live scopes.\n- Current session: exact pair materialized\n- Working window: exact pair materialized\n- Lifetime blocker: {}\n- Lifetime irrecoverable rows: {}\n- Значит irrecoverable debt сейчас выглядит как исторический хвост, а не как новый live drift.",
        blocker_code,
        format_u64(Some(irrecoverable_missing_live_events)),
    );
    Some(metric_row(
        "Исторический frozen debt",
        format!(
            "{}: historical-only, {} rows",
            blocker_code,
            format_u64(Some(irrecoverable_missing_live_events))
        ),
        Some(tooltip.as_str()),
    ))
}

fn reviewed_frozen_debt_export_metric_row(alignment: &Value) -> Option<Value> {
    let surface = &alignment["reviewed_frozen_debt_export_surface"];
    if surface["export_ready_report_only"].as_bool() != Some(true) {
        return None;
    }
    let surface_kind = surface["surface_kind"]
        .as_str()
        .unwrap_or("reviewed_frozen_debt_report_only");
    let blocker_code = surface["blocking_component"]
        .as_str()
        .unwrap_or("unknown_blocker");
    let irrecoverable_missing_live_events = surface["irrecoverable_missing_live_events"]
        .as_u64()
        .unwrap_or(0);
    let allowed_claims = surface["allowed_claims"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let forbidden_claims = surface["forbidden_claims"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let propagated_surfaces = surface["propagated_surfaces"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let review_bundle_command = surface["review_bundle_command"]
        .as_str()
        .unwrap_or_default();
    let evidence_pack_command = surface["evidence_pack_command"]
        .as_str()
        .unwrap_or_default();
    let tooltip = format!(
        "Этот ряд показывает отдельный report-only export contour для irrecoverable historical debt.\n- Surface kind: {}\n- Blocker component: {}\n- Irrecoverable rows: {}\n- Allowed claims: {}\n- Forbidden claims: {}\n- Propagated surfaces: {}\n- Review bundle command: {}\n- Evidence pack command: {}\n- Этот contour не чинит lifetime exactness и не имеет права притворяться raw exact history.",
        surface_kind,
        blocker_code,
        format_u64(Some(irrecoverable_missing_live_events)),
        allowed_claims,
        forbidden_claims,
        propagated_surfaces,
        review_bundle_command,
        evidence_pack_command
    );
    Some(metric_row(
        "Review-only export",
        format!(
            "{}: {} irrecoverable rows",
            surface_kind,
            format_u64(Some(irrecoverable_missing_live_events))
        ),
        Some(tooltip.as_str()),
    ))
}

fn status_for_metric_prefix(snapshot: &Value, prefix: &str) -> &'static str {
    let mut current: Option<&str> = None;
    for check in snapshot["sla"]["checks"].as_array().into_iter().flatten() {
        let metric = check["metric"].as_str().unwrap_or_default();
        if !metric.starts_with(prefix) {
            continue;
        }
        let status = check["status"].as_str().unwrap_or("unknown");
        current = Some(match current {
            Some(existing) => worst_status(existing, status),
            None => match status {
                "pass" => "pass",
                "alert" => "alert",
                "critical" => "critical",
                _ => "unknown",
            },
        });
    }
    current.unwrap_or("unknown")
}

fn status_for_metric_name(snapshot: &Value, metric_name: &str) -> &'static str {
    snapshot["sla"]["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|check| check["metric"].as_str() == Some(metric_name))
        .and_then(|check| check["status"].as_str())
        .and_then(normalize_status)
        .unwrap_or("unknown")
}

fn combine_statuses(statuses: &[&str]) -> &'static str {
    statuses
        .iter()
        .copied()
        .filter_map(normalize_status)
        .reduce(worst_status)
        .unwrap_or("unknown")
}

fn normalize_status(status: &str) -> Option<&'static str> {
    match status {
        "pass" => Some("pass"),
        "alert" => Some("alert"),
        "critical" => Some("critical"),
        "unknown" => Some("unknown"),
        _ => None,
    }
}

fn worst_status(left: &str, right: &str) -> &'static str {
    if status_rank(left) >= status_rank(right) {
        match left {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        }
    } else {
        match right {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        }
    }
}

fn status_rank(status: &str) -> u8 {
    match status {
        "critical" => 4,
        "alert" => 3,
        "pass" => 2,
        "unknown" => 1,
        _ => 0,
    }
}

fn humanize_check(snapshot: &Value, check: &Value) -> String {
    let metric = check["metric"].as_str().unwrap_or("unknown.metric");
    let status = status_label(check["status"].as_str().unwrap_or("unknown"));
    let value = match check["value"].as_f64() {
        Some(number) if metric.ends_with("_ratio") => format!("{:.2}%", number * 100.0),
        Some(number) if metric.ends_with("_ms") => format_ms(snapshot, Some(number)),
        Some(number) if metric.ends_with("_seconds") => format_seconds(snapshot, Some(number)),
        Some(number) => format!("{number:.3}"),
        None => "ещё нет данных".to_string(),
    };
    let explanation = match metric {
        "postgres.connection_usage_ratio" => "PostgreSQL использует слишком много соединений.",
        "postgres.query_probe_p95_ms" => "PostgreSQL отвечает медленнее, чем должен.",
        "postgres.replica_lag_seconds" => {
            "Отставание реплики PostgreSQL вышло за допустимый контур."
        }
        "postgres.deadlocks_delta" => {
            "Между двумя последними snapshot-ами в PostgreSQL появился новый deadlock."
        }
        "qdrant.index_optimize_queue" => "У Qdrant выросла очередь оптимизации индекса.",
        "qdrant.update_queue_length" => "У Qdrant растёт очередь обновлений.",
        "qdrant.search_stage_p95_ms" => "Семантический поиск в Qdrant стал заметно тяжелее.",
        "nats.publish_probe_p95_ms" => "NATS публикует события медленнее ожидаемого.",
        "nats.consumer_lag_msgs" => "У JetStream накопилось слишком много непрочитанных сообщений.",
        "nats.jetstream_disk_usage_ratio" => "JetStream слишком близко подошёл к лимиту диска.",
        "retrieval.cold_p95_ms" => "Первый запрос после старта стал слишком медленным.",
        "retrieval.hot_p95_ms" => "Быстрый повторный запрос больше не укладывается в stretch-goal.",
        "parser.coverage_ratio" => {
            "Слишком часто приходится падать в грубый текстовый fallback вместо AST-разбора."
        }
        "accuracy.cross_project_leakage" => {
            "Один проект начал подтекать в другой, а этого быть не должно."
        }
        "accuracy.symbol_precision" => "Попадание в нужные символы стало менее точным.",
        "accuracy.semantic_precision" => {
            "Семантический поиск стал реже попадать в правильные ответы."
        }
        "load.hot_qps" => "Горячий быстрый путь держит меньше Burst QPS, чем обещано.",
        "load.hot_p50_ms" => "Обычная hot-задержка в benchmark-прогоне стала выше целевой планки.",
        "load.hot_p95_ms" => "Тяжёлый хвост hot benchmark стал выше обещанной границы.",
        "load.hot_p99_ms" => "Редкие тяжёлые выбросы в hot benchmark стали слишком большими.",
        "load.hot_max_ms" => "Самый тяжёлый запрос в hot benchmark вышел за безопасную границу.",
        "load.hot_error_rate" => "Под нагрузкой появились ошибки на быстром пути.",
        "observability.benchmark_contamination" => {
            "В benchmark-витрину подмешался live-context или другой неподходящий source."
        }
        "load.hot_workers" => "Последний hot benchmark был прогнан слишком слабой параллельностью.",
        "load.hot_sample_count" => {
            "Последний hot benchmark собран на слишком маленькой выборке, чтобы ему доверять."
        }
        _ => "Один из обязательных проверочных контуров вышел из своей нормы.",
    };
    format!("{explanation} Сейчас: {value}. Состояние: {status}.")
}

#[cfg(test)]
mod tests {
    use super::{
        artifact_cleanup_warning, benchmark_qdrant_live_card, browser_base_url,
        build_benchmark_cards, build_governance_card, build_hero_cards, build_links,
        build_live_summary_payload, build_machine_cards, build_payload, build_service_cards,
        build_top_cards, format_ms, format_time_compare_pair, human_elapsed_ms,
        live_latency_compare_card, monitoring_url, render_html, working_state_live_card,
        worst_status,
    };
    use crate::config::AppConfig;
    use crate::hardware_telemetry::{AcceleratorSummary, MachineSummary};
    use crate::working_state;
    use serde_json::{Value, json};

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
            edge_cache_path: "/tmp/edge-cache-test.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            local_fast_cache_ttl_ms: 5_000,
        }
    }

    fn synthetic_machine_summary(
        disk_available_gib: f64,
        disk_used_percent: Option<f64>,
    ) -> MachineSummary {
        MachineSummary {
            cpu_model: "Synthetic CPU".to_string(),
            logical_cpus: 8,
            physical_cpus: Some(4),
            cpu_usage_percent: Some(12.0),
            cpu_temperature_celsius: None,
            cpu_max_mhz: Some(4200.0),
            cpu_source_label: "synthetic".to_string(),
            total_memory_gib: 64.0,
            available_memory_gib: 48.0,
            used_memory_gib: 16.0,
            memory_used_percent: Some(25.0),
            memory_type: "DDR5".to_string(),
            memory_speed_label: "5600 MT/s".to_string(),
            memory_source_label: "synthetic".to_string(),
            swap_total_gib: 16.0,
            swap_used_gib: 0.0,
            disk_device: Some("/dev/nvme0n1".to_string()),
            disk_model: "Synthetic NVMe".to_string(),
            disk_kind: "NVMe SSD".to_string(),
            disk_source_label: "synthetic".to_string(),
            disk_total_gib: 1900.0,
            disk_available_gib,
            disk_used_percent,
            disk_busy_percent: None,
            disk_read_mib_per_sec: None,
            disk_write_mib_per_sec: None,
            disk_temperature_celsius: None,
            disk_firmware: "test".to_string(),
            accelerators: Vec::<AcceleratorSummary>::new(),
        }
    }

    #[test]
    fn browser_url_rewrites_unspecified_v4() {
        assert_eq!(browser_base_url("0.0.0.0:9464"), "http://127.0.0.1:9464");
    }

    #[test]
    fn dashboard_payload_exposes_live_compare_card_alias_from_top_cards() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "observe_refresh": {"total_ms": 12},
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "live_response_latency": {
                        "current_session": {
                            "sample_count": 0,
                            "latency_slices": []
                        },
                        "rolling_window": {
                            "sample_count": 1,
                            "latency_slices": [
                                {
                                    "state": "cold",
                                    "sample_count": 1,
                                    "p50_latency_ms": 2.0,
                                    "p95_latency_ms": 2.0,
                                    "p99_latency_ms": 2.0,
                                    "max_latency_ms": 2.0
                                }
                            ]
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn"
                    }
                }
            }
        });

        let payload =
            build_payload(&test_config(), &snapshot, "127.0.0.1:9464", 1000).expect("payload");

        assert_eq!(
            payload["live_compare_card"]["kind"].as_str(),
            Some("live_compare")
        );
        assert_eq!(
            payload["live_compare_card"]["title"].as_str(),
            Some("Скорость ответа")
        );
        assert!(payload["client_budget_live"].is_object());
    }

    #[test]
    fn dashboard_html_refresh_contract_is_live_on_focus_and_visibility() {
        let html = render_html(1000, None);
        assert!(html.contains("const TOOLTIP_HIDE_GRACE_MS = 220;"));
        assert!(html.contains("const DASHBOARD_BOOTSTRAP_PAYLOAD = null;"));
        assert!(html.contains("async function fetchWithTimeout(path, timeoutMs, init = {}) {"));
        assert!(html.contains(
            "renderDashboardPayload(chooseInitialDashboardPayload(DASHBOARD_BOOTSTRAP_PAYLOAD));"
        ));
        assert!(html.contains(
            "function chooseInitialDashboardPayload(bootstrapPayload) {\n      if (bootstrapPayload) {\n        return bootstrapPayload;\n      }\n      return null;\n    }"
        ));
        assert!(html.contains(
            "const DASHBOARD_PAYLOAD_CACHE_KEY = \"amai-human-dashboard-last-payload-v1\";"
        ));
        assert!(html.contains(
            "function scheduleHideTooltip(target = null, delayMs = TOOLTIP_HIDE_GRACE_MS) {"
        ));
        assert!(html.contains(
            "function isDocumentVisibleForRefresh() {\n      return document.visibilityState === \"visible\";\n    }"
        ));
        assert!(html.contains(
            "function scheduleForcedDashboardRefresh(reason = \"forced_refresh\", delayMs = 0) {"
        ));
        assert!(html.contains("document.addEventListener(\"visibilitychange\""));
        assert!(html.contains(
            "window.addEventListener(\"focus\", () => scheduleForcedDashboardRefresh(\"window_focus\"));"
        ));
        assert!(html.contains(
            "window.addEventListener(\"pageshow\", () => scheduleForcedDashboardRefresh(\"window_pageshow\"));"
        ));
        assert!(html.contains("const dashboardThreadId = new URLSearchParams(window.location.search).get(\"thread_id\");"));
        assert!(html.contains(
            "fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/client-budget-live\")"
        ));
        assert!(html.contains("scheduleForcedDashboardRefresh(\"initial_boot\");"));
        assert!(html.contains(
            "fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/dashboard-live-summary\")"
        ));
        assert!(html.contains("fetch(apiPathWithThreadHint(\"/api/client-budget-target\")"));
        assert!(html.contains("/api/client-budget-host-control-launch"));
        assert!(html.contains("/api/client-budget-host-control-feedback"));
        assert!(
            html.contains("fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/dashboard\")")
        );
        assert!(html.contains("id=\"dashboard-toast\""));
        assert!(html.contains("tooltipLayer.addEventListener(\"mouseenter\", () => {"));
        assert!(!html.contains(
            "setInterval(() => syncDashboardLiveSummary(false), DASHBOARD_LIVE_SUMMARY_REFRESH_MS);"
        ));
        assert!(html.contains(
            "setInterval(() => syncClientBudgetLiveRows(false), CLIENT_BUDGET_LIVE_REFRESH_MS);"
        ));
        assert!(!html.contains("setInterval(() => loadDashboard(false), REFRESH_MS);"));
        assert!(!html.contains("syncActiveAgentBudgetLiveCard(false)"));
        assert!(!html.contains("fetchActiveAgentBudgetLivePayload(force = false)"));
        assert!(!html.contains(
            "async function fetchClientBudgetLivePayload(force = false) {\n      if (!force && isRefreshPaused()) {"
        ));
        assert!(!html.contains("INTERACTION_HOLD_SELECTOR"));
    }

    #[test]
    fn dashboard_html_contains_agent_rename_endpoint_and_inline_tooltip_trigger() {
        let html = render_html(1000, None);
        assert!(html.contains("/api/agent-display-name"));
        assert!(html.contains("content.className = \"tooltip-inline-trigger has-tooltip\";"));
    }

    #[test]
    fn critical_status_wins() {
        assert_eq!(worst_status("pass", "critical"), "critical");
        assert_eq!(worst_status("alert", "unknown"), "alert");
        assert_eq!(worst_status("unknown", "pass"), "pass");
    }

    #[test]
    fn monitoring_url_reuses_dashboard_host() {
        assert_eq!(
            monitoring_url("http://demo-host:9464", "59090"),
            "http://demo-host:59090"
        );
    }

    #[test]
    fn elapsed_label_is_compact() {
        assert_eq!(human_elapsed_ms(30_000), "меньше минуты");
        assert_eq!(human_elapsed_ms(61_000), "1 мин.");
        assert_eq!(human_elapsed_ms(3_720_000), "1 ч. 2 мин.");
    }

    #[test]
    fn format_ms_uses_dashboard_timing_policy_from_snapshot() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.0005,
                        "switch_to_microseconds_below_ms": 2.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "below timer floor",
                        "seconds_suffix": "secs",
                        "milliseconds_suffix": "millis",
                        "microseconds_suffix": "micros",
                        "nanoseconds_suffix": "nanos",
                        "seconds_decimals": 2,
                        "milliseconds_decimals": 2,
                        "microseconds_decimals": 1,
                        "nanoseconds_decimals": 0
                    }
                }
            }
        });

        assert_eq!(format_ms(&snapshot, Some(0.0)), "below timer floor");
        assert_eq!(format_ms(&snapshot, Some(0.0004)), "400 nanos");
        assert_eq!(format_ms(&snapshot, Some(0.0015)), "1.5 micros");
        assert_eq!(format_ms(&snapshot, Some(2.3456)), "2.35 millis");
        assert_eq!(format_ms(&snapshot, Some(2345.6)), "2.35 secs");
    }

    #[test]
    fn format_ms_falls_back_to_default_dashboard_timing_policy_when_missing() {
        let snapshot = json!({});

        assert_eq!(format_ms(&snapshot, Some(0.0)), "0 ns");
        assert_eq!(format_ms(&snapshot, Some(0.0004)), "400 ns");
        assert_eq!(format_ms(&snapshot, Some(0.0015)), "1.5 µs");
        assert_eq!(format_ms(&snapshot, Some(2.3456)), "2.346 ms");
        assert_eq!(format_ms(&snapshot, Some(2345.6)), "2.346 s");
    }

    #[test]
    fn compare_time_pair_uses_one_row_unit_for_target_and_current() {
        let snapshot = json!({
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
                }
            }
        });

        assert_eq!(
            format_time_compare_pair(&snapshot, Some(1.0), Some(0.674), "<="),
            vec!["<= 1 ms".to_string(), "0.674 ms".to_string()]
        );
        assert_eq!(
            format_time_compare_pair(&snapshot, Some(0.015), Some(0.003226), "<="),
            vec!["<= 15 µs".to_string(), "3.226 µs".to_string()]
        );
        assert_eq!(
            format_time_compare_pair(&snapshot, Some(1.0), Some(0.000271), "<="),
            vec!["<= 1 ms".to_string(), "271 ns".to_string()]
        );
    }

    #[test]
    fn benchmark_qdrant_card_uses_last_success_snapshot_without_error_rows() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": false,
                "active": false,
                "from_last_success": true,
                "http_url": "http://127.0.0.1:7633",
                "memory_resident_bytes": 422123456.0,
                "points_count": 70200.0,
                "segments_count": 8.0,
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0,
                "run_summary": {
                    "benchmark_display_name": "VectorDBBench",
                    "dataset_display_name": "dbpedia-openai-1000k-angular",
                    "run_state": "finished_ok",
                    "dataset_size": 990000,
                    "latest_result": {
                        "recall": 0.9958,
                        "p95_ms": 0.0117,
                        "p99_ms": 0.0129
                    }
                }
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("последний прогон успешен")
        );
        assert_eq!(card["value"].as_str(), Some("последний прогон успешен"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("последнего успешного прогона")
        );
        let empty_rows = Vec::new();
        let labels = card["rows"]
            .as_array()
            .unwrap_or(&empty_rows)
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Прогон"));
        assert!(labels.contains(&"Последний результат"));
    }

    #[test]
    fn benchmark_qdrant_card_without_cache_shows_test_not_running_without_error_rows() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": false,
                "active": false,
                "from_last_success": false,
                "http_url": "http://127.0.0.1:7633",
                "index_optimize_queue": null,
                "update_queue_length": null,
                "memory_resident_bytes": null,
                "points_count": null,
                "segments_count": null,
                "run_summary": {
                    "benchmark_display_name": "VectorDBBench",
                    "dataset_display_name": "dbpedia-openai-1000k-angular",
                    "run_state": "not_started"
                }
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(card["status_label"].as_str(), Some("тест не запущен"));
        assert_eq!(card["value"].as_str(), Some("ещё нет данных"));
        let empty_rows = Vec::new();
        let labels = card["rows"]
            .as_array()
            .unwrap_or(&empty_rows)
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Прогон"));
        assert!(labels.contains(&"Состояние"));
    }

    #[test]
    fn benchmark_qdrant_card_marks_stopped_test_even_if_metrics_are_still_available() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": true,
                "active": false,
                "from_last_success": false,
                "http_url": "http://127.0.0.1:7633",
                "memory_resident_bytes": 219709440.0,
                "points_count": 218800.0,
                "segments_count": 8.0,
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0,
                "run_summary": {
                    "benchmark_display_name": "VectorDBBench",
                    "dataset_display_name": "dbpedia-openai-1000k-angular",
                    "run_state": "finished_error"
                }
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("alert"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("последний прогон с ошибкой")
        );
        assert_eq!(card["value"].as_str(), Some("последний прогон с ошибкой"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("завершился с ошибкой")
        );
    }

    #[test]
    fn benchmark_qdrant_card_is_waiting_while_live_run_is_still_in_progress() {
        let snapshot = json!({
            "thresholds": {
                "qdrant": {
                    "optimize_queue": { "target": 10.0 },
                    "update_queue_length": { "target": 0.0 }
                }
            },
            "benchmark_qdrant": {
                "configured": true,
                "available": true,
                "active": true,
                "from_last_success": false,
                "http_url": "http://127.0.0.1:7633",
                "memory_resident_bytes": 219709440.0,
                "points_count": 990000.0,
                "segments_count": 8.0,
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0,
                "run_summary": {
                    "benchmark_display_name": "ann-benchmarks",
                    "dataset_display_name": "dbpedia-openai-1000k-angular",
                    "run_state": "running",
                    "dataset_size": 990000,
                    "started_at_epoch_s": 1775800000,
                    "live_progress": {
                        "definition_label": "['angular', 'scalar', 32, 128]",
                        "group_current": 9,
                        "group_total": 18,
                        "processed_current": 1000,
                        "processed_total": 10000
                    }
                }
            }
        });
        let card = benchmark_qdrant_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("идёт прогон"));
        assert_eq!(card["value"].as_str(), Some("идёт прогон"));
        assert!(
            card["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Итоговый статус")
        );
    }

    #[test]
    fn live_compare_card_is_not_green_when_samples_are_missing_or_under_target() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "cold",
                                "sample_count": 14,
                                "p50_latency_ms": 2.0,
                                "p95_latency_ms": 4.0,
                                "p99_latency_ms": 4.0,
                                "max_latency_ms": 4.0
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("текущая серия ещё набирается")
        );
        assert!(
            card["metrics"][0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("пока нет ни текущей серии, ни накопленного окна")
        );
        assert!(
            card["metrics"][1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("По задержке всё хорошо")
        );
    }

    #[test]
    fn live_compare_card_is_green_only_when_both_modes_strictly_pass() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 100001,
                                "p50_latency_ms": 0.4,
                                "p95_latency_ms": 0.7,
                                "p99_latency_ms": 1.2,
                                "max_latency_ms": 2.5
                            },
                            {
                                "state": "cold",
                                "sample_count": 10001,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.1,
                                "p99_latency_ms": 3.4,
                                "max_latency_ms": 5.2
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        assert_eq!(card["status_label"].as_str(), Some("в норме"));
    }

    #[test]
    fn live_compare_card_uses_live_readiness_floor_separately_from_benchmark_floor() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "profile": {
                        "rolling_window_hours": 24
                    },
                    "live_response_latency": {
                        "current_session": {
                            "latency_slices": []
                        },
                        "rolling_window": {
                            "latency_slices": [
                                {
                                    "state": "hot",
                                    "sample_count": 24,
                                    "p50_latency_ms": 0.8,
                                    "p95_latency_ms": 0.9,
                                    "p99_latency_ms": 1.4,
                                    "max_latency_ms": 2.4
                                },
                                {
                                    "state": "cold",
                                    "sample_count": 140,
                                    "p50_latency_ms": 1.9,
                                    "p95_latency_ms": 3.9,
                                    "p99_latency_ms": 5.0,
                                    "max_latency_ms": 7.1
                                }
                            ]
                        }
                    },
                    "current_session": {
                        "latency_slices": []
                    },
                    "rolling_window": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 24,
                                "p50_latency_ms": 0.8,
                                "p95_latency_ms": 0.9,
                                "p99_latency_ms": 1.4,
                                "max_latency_ms": 2.4
                            },
                            {
                                "state": "cold",
                                "sample_count": 140,
                                "p50_latency_ms": 1.9,
                                "p95_latency_ms": 3.9,
                                "p99_latency_ms": 5.0,
                                "max_latency_ms": 7.1
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("онлайн-серия ещё набирается")
        );
        assert_eq!(
            card["table"]["rows"][0]["values"][4].as_str(),
            Some(">= 100")
        );
        assert_eq!(
            card["table"]["rows"][3]["values"][4].as_str(),
            Some(">= 100")
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Ниже рядом показаны и текущая серия")
        );
    }

    #[test]
    fn live_compare_card_falls_back_to_stable_targets_when_thresholds_are_missing() {
        let snapshot = json!({
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
                "retrieval": {}
            },
            "token_budget_report": {
                "token_budget_report": {
                    "rolling_window": {
                        "latency_slices": [
                            {
                                "state": "cold",
                                "sample_count": 1,
                                "current_latency_ms": 87.0,
                                "p50_latency_ms": 87.0,
                                "p95_latency_ms": 87.0,
                                "p99_latency_ms": 87.0,
                                "max_latency_ms": 87.0
                            }
                        ]
                    },
                    "current_session": {
                        "latency_slices": []
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(
            card["table"]["rows"][0]["values"],
            json!(["<= 1 ms", "<= 2 ms", "<= 3 ms", "<= 5 ms", ">= 100"])
        );
        assert_eq!(
            card["table"]["rows"][2]["values"],
            json!(["<= 2 ms", "<= 4 ms", "<= 6 ms", "<= 10 ms", ">= 100"])
        );
    }

    #[test]
    fn live_compare_card_keeps_stable_rows_when_hot_cold_are_absent() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "mixed",
                                "sample_count": 3,
                                "current_latency_ms": 1.7,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.4,
                                "p99_latency_ms": 2.4,
                                "max_latency_ms": 2.4
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("окно ещё набирается"));
        assert_eq!(
            card["metrics"][0]["label"].as_str(),
            Some("Повторный запрос")
        );
        assert_eq!(card["metrics"][0]["value"].as_str(), Some("ещё нет данных"));
        assert_eq!(card["metrics"][1]["label"].as_str(), Some("Новый запрос"));
        assert_eq!(card["metrics"][1]["value"].as_str(), Some("ещё нет данных"));
        assert_eq!(
            card["table"]["rows"][0]["label"].as_str(),
            Some("Повторный запрос — эталон")
        );
        assert_eq!(
            card["table"]["rows"][2]["label"].as_str(),
            Some("Новый запрос — эталон")
        );
        assert_eq!(
            card["table"]["rows"].as_array().map(|rows| rows.len()),
            Some(4)
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Последний живой ответ")
        );
    }

    #[test]
    fn live_compare_card_keeps_stable_rows_when_live_turn_is_empty() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn"
                    },
                    "current_session": {
                        "latency_slices": []
                    },
                    "rolling_window": {
                        "latency_slices": []
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(
            card["table"]["rows"][0]["values"],
            json!(["<= 1 ms", "<= 2 ms", "<= 3 ms", "<= 5 ms", ">= 100"])
        );
        assert_eq!(
            card["table"]["rows"][1]["values"],
            json!([
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "0"
            ])
        );
        assert_eq!(
            card["table"]["rows"][2]["values"],
            json!([
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "0"
            ])
        );
        assert_eq!(
            card["table"]["rows"][3]["values"],
            json!(["<= 2 ms", "<= 4 ms", "<= 6 ms", "<= 10 ms", ">= 100"])
        );
        assert_eq!(
            card["table"]["rows"][4]["values"],
            json!([
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "0"
            ])
        );
        assert_eq!(
            card["table"]["rows"][5]["values"],
            json!([
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "ещё нет данных",
                "0"
            ])
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("В текущем live-turn пока нет новых Amai-событий")
        );
    }

    #[test]
    fn live_compare_card_prefers_rolling_window_so_stats_do_not_reset_on_new_chat() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "observe_refresh": {
                "total_ms": 42
            },
            "token_budget_report": {
                "token_budget_report": {
                    "profile": {
                        "rolling_window_hours": 24
                    },
                    "current_session": {
                        "latency_slices": []
                    },
                    "rolling_window": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 120000,
                                "p50_latency_ms": 0.8,
                                "p95_latency_ms": 0.9,
                                "p99_latency_ms": 1.4,
                                "max_latency_ms": 2.2
                            },
                            {
                                "state": "cold",
                                "sample_count": 22000,
                                "p50_latency_ms": 1.9,
                                "p95_latency_ms": 3.9,
                                "p99_latency_ms": 5.0,
                                "max_latency_ms": 7.1
                            }
                        ]
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        assert_eq!(card["metrics"][0]["value"].as_str(), Some("800 µs"));
        assert_eq!(card["metrics"][1]["value"].as_str(), Some("1.9 ms"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("накопительное окно 24 часов")
        );
        assert!(
            card["title_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("задержку Amai")
        );
        assert!(
            card["table"]["columns"][1]["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("задержка Amai")
        );
    }

    #[test]
    fn live_compare_card_explains_when_current_series_is_from_previous_turn() {
        let snapshot = json!({
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn"
                    },
                    "live_response_latency": {
                        "current_session_relation": {
                            "status": "recent_same_chat_series_previous_turn",
                            "note": "Текущий live-turn уже начался, но в нём пока нет новых Amai-событий. Показанная текущая серия относится к недавним ответам этого же чата из предыдущего turn."
                        },
                        "current_session": {
                            "latency_slices": [{
                                "state": "cold",
                                "sample_count": 1,
                                "p50_latency_ms": 2.0,
                                "p95_latency_ms": 2.0,
                                "p99_latency_ms": 2.0,
                                "max_latency_ms": 2.0
                            }]
                        },
                        "rolling_window": {
                            "latency_slices": [{
                                "state": "cold",
                                "sample_count": 1,
                                "p50_latency_ms": 2.0,
                                "p95_latency_ms": 2.0,
                                "p99_latency_ms": 2.0,
                                "max_latency_ms": 2.0
                            }]
                        }
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert!(
            card["note"]
                .as_str()
                .is_some_and(|note| note.contains("из предыдущего turn"))
        );
        assert!(
            card["status_tooltip"]
                .as_str()
                .is_some_and(|note| note.contains("из предыдущего turn"))
        );
    }

    #[test]
    fn live_compare_card_ignores_end_to_end_response_window_for_amai_surface() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1_774_258_000_000u64,
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "profile": {
                        "rolling_window_hours": 24
                    },
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "cold",
                                "sample_count": 999,
                                "p50_latency_ms": 1.0,
                                "p95_latency_ms": 1.0,
                                "p99_latency_ms": 1.0,
                                "max_latency_ms": 1.0
                            }
                        ]
                    },
                    "rolling_window": {
                        "latency_slices": []
                    },
                    "live_response_latency": {
                        "current_session": {
                            "latency_slices": [
                                {
                                    "state": "hot",
                                    "sample_count": 2,
                                    "current_latency_ms": 3200.0,
                                    "p50_latency_ms": 2800.0,
                                    "p95_latency_ms": 3200.0,
                                    "p99_latency_ms": 3200.0,
                                    "max_latency_ms": 3200.0
                                }
                            ],
                            "latest_turn": {
                                "ended_at_epoch_ms": 1_774_257_999_000u64
                            }
                        },
                        "rolling_window": {
                            "latency_slices": [
                                {
                                    "state": "hot",
                                    "sample_count": 8,
                                    "current_latency_ms": 3200.0,
                                    "p50_latency_ms": 2800.0,
                                    "p95_latency_ms": 4100.0,
                                    "p99_latency_ms": 4200.0,
                                    "max_latency_ms": 4200.0
                                },
                                {
                                    "state": "cold",
                                    "sample_count": 3,
                                    "current_latency_ms": 8900.0,
                                    "p50_latency_ms": 7600.0,
                                    "p95_latency_ms": 8900.0,
                                    "p99_latency_ms": 8900.0,
                                    "max_latency_ms": 8900.0
                                }
                            ],
                            "latest_turn": {
                                "ended_at_epoch_ms": 1_774_257_999_000u64
                            }
                        }
                    }
                }
            }
        });

        let card = live_latency_compare_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("текущая серия ещё набирается")
        );
        assert_eq!(card["metrics"][0]["value"].as_str(), Some("2.8 s"));
        assert_eq!(card["metrics"][1]["value"].as_str(), Some("7.6 s"));
        assert_eq!(
            card["table"]["rows"][0]["label"].as_str(),
            Some("Повторный запрос — эталон")
        );
        assert_eq!(
            card["table"]["rows"][3]["label"].as_str(),
            Some("Новый запрос — эталон")
        );
        assert_eq!(
            card["table"]["rows"].as_array().map(|rows| rows.len()),
            Some(6)
        );
        assert!(
            card["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("live_response_latency")
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Главный сигнал теперь строится по текущей серии")
        );
    }

    #[test]
    fn top_cards_split_live_retrieval_from_real_workline() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 100001,
                                "p50_latency_ms": 0.4,
                                "p95_latency_ms": 0.7,
                                "p99_latency_ms": 1.2,
                                "max_latency_ms": 2.5
                            },
                            {
                                "state": "cold",
                                "sample_count": 10001,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.1,
                                "p99_latency_ms": 3.4,
                                "max_latency_ms": 5.2
                            }
                        ]
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Amai observability guardrail proof materialized",
                    "next_step": "Вывести guardrail verdict в dashboard/service layer.",
                    "last_command": "continuity handoff",
                    "last_results_summary": "Зафиксирован handoff для art :: continuity",
                    "latest_decision_trace": {
                        "included": [
                            {
                                "strategy": "exact_documents",
                                "count": 1,
                                "reason": "Нашлись точные document/path совпадения внутри видимого контура."
                            }
                        ],
                        "not_included": [
                            {
                                "strategy": "semantic_chunks",
                                "reason": "Semantic layer честно abstained и не добавил фрагменты."
                            }
                        ]
                    },
                    "active_files": [
                        "/home/art/agent-memory-index/src/observe.rs",
                        "/home/art/agent-memory-index/src/dashboard.rs"
                    ],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            },
            "agent_scope_activity": {
                "client_recent_window_minutes": 30,
                "client_recent_thread_count": 1,
                "client_recent_threads": [
                    {
                        "thread_id": "019d16f2-528d-7cc0-bcfe-8984f95f05c7",
                        "cwd": "/home/art/Art",
                        "rollout_path": "/home/art/.codex/sessions/2026/03/22/rollout-2026-03-22T22-07-52-019d16f2-528d-7cc0-bcfe-8984f95f05c7.jsonl",
                        "title": "продолжай по Amai continuity",
                        "agent_nickname": "Amai",
                        "agent_role": "continuity",
                        "model_provider": "openai",
                        "model": "gpt-5.4",
                        "reasoning_effort": "xhigh",
                        "updated_at_epoch_ms": 1774239285880u64
                    }
                ],
                "active_now_count": 1,
                "active_now_scopes": [
                    {
                        "agent_scope": "art::continuity::default",
                        "owner_thread_id": "019d16f2-528d-7cc0-bcfe-8984f95f05c7",
                        "heartbeat_at_epoch_ms": 1774239285880u64
                    }
                ],
                "recent_scope_window_hours": 24,
                "recent_scope_count": 3,
                "recent_scopes": [
                    {
                        "agent_scope": "art::continuity::default",
                        "captured_at_epoch_ms": 1774239285880u64
                    },
                    {
                        "agent_scope": "bug_bounty::continuity::default",
                        "captured_at_epoch_ms": 1774239200000u64
                    }
                ]
            }
        });

        let cards = build_top_cards(&snapshot);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0]["title"].as_str(), Some("Скорость ответа"));
        assert_eq!(cards[1]["title"].as_str(), Some("Текущая работа"));
        assert!(
            cards[0]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        );
        assert!(
            cards[1]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Уверенность в этом рабочем снимке пока")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая сводка по текущей работе")
        );
        assert!(cards[1]["rows"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["label"].as_str() == Some("Что дальше"))
        }));
    }

    #[test]
    fn working_state_card_hides_empty_decision_trace_rows_and_requires_repo_scoped_snapshot() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "default" },
                    "agent_scope": "amai::default::default",
                    "session_age_ms": 7u64,
                    "events_count": 1u64,
                    "current_goal": "Рабочий запрос: structural graph proof",
                    "next_step": "Уточните запрос или задайте follow-up.",
                    "last_command": "context pack",
                    "last_results_summary": "Найдено: документов 0, символов 0.",
                    "latest_decision_trace": null,
                    "active_files": [],
                    "recent_queries": ["structural graph proof"],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("ждём устойчивый снимок")
        );
        let rows = card["rows"].as_array().expect("rows");
        assert!(
            rows.iter()
                .all(|row| row["label"].as_str() != Some("Почему включено"))
        );
        assert!(
            rows.iter()
                .all(|row| row["label"].as_str() != Some("Почему не вошло"))
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая сводка по текущей работе")
        );

        let unknown_card = working_state_live_card(&json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "latest_repo_working_state_restore": null
        }));
        assert_eq!(unknown_card["status"], json!("unknown"));
        assert!(
            unknown_card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("нет свежего локального снимка")
        );
    }

    #[test]
    fn working_state_card_surfaces_current_live_turn_activity_when_exact_pair_is_ready() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "exact_pair_materialized",
                        "retrieval_context_pack_count": 1,
                        "matched_context_pack_ids_count": 1,
                        "note": "Exact full-turn pair materialized from the actual VS Code meter.",
                        "exact_pair": {
                            "saved_pct": 76.52
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Repair dashboard live-turn behavior",
                    "next_step": "Surface live turn in current work card.",
                    "last_command": "context pack",
                    "last_results_summary": "current_live_turn exact pair materialized",
                    "active_files": [
                        "/home/art/agent-memory-index/src/dashboard.rs"
                    ],
                    "recent_queries": [],
                    "restore_confidence": "medium"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        assert!(
            card["note"]
                .as_str()
                .is_some_and(|note| { note.contains("свежий живой ответ Amai") })
        );
        let rows = card["rows"].as_array().expect("rows");
        let live_turn_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Живой turn Amai"))
            .expect("live turn row");
        assert!(
            live_turn_row["value"].as_str().is_some_and(|value| {
                value.contains("1 context-pack") && value.contains("76.52%")
            })
        );
    }

    #[test]
    fn working_state_card_uses_waiting_status_when_only_live_turn_activity_is_fresh() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "exact_pair_materialized",
                        "retrieval_context_pack_count": 1,
                        "matched_context_pack_ids_count": 1,
                        "note": "Exact full-turn pair materialized from the actual VS Code meter.",
                        "exact_pair": {
                            "saved_pct": 69.64
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 0u64,
                    "current_goal": "Current live-turn now surfaces same-thread Amai activity after fresh context-pack",
                    "next_step": "Tighten current-work card so fresh exact-pair / thread activity is surfaced there too.",
                    "last_command": "continuity handoff",
                    "last_results_summary": null,
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("живой turn уже виден"));
        let rows = card["rows"].as_array().expect("rows");
        let last_result_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Последний результат"))
            .expect("last result row");
        assert!(
            last_result_row["value"]
                .as_str()
                .is_some_and(|value| { value.contains("Exact full-turn pair materialized") })
        );
        let last_command_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Последняя команда"))
            .expect("last command row");
        assert_eq!(
            last_command_row["value"].as_str(),
            Some("Amai context pack")
        );
    }

    #[test]
    fn preliminary_handoff_command_is_overridden_by_fresh_live_turn_command() {
        assert!(super::should_override_last_command_with_live_turn(
            "сохранена рабочая сводка",
            "preliminary",
            0,
        ));
        assert!(!super::should_override_last_command_with_live_turn(
            "сохранена рабочая сводка",
            "high",
            0,
        ));
        assert!(!super::should_override_last_command_with_live_turn(
            "сохранена рабочая сводка",
            "preliminary",
            2,
        ));
    }

    #[test]
    fn live_file_hints_restore_last_command_when_new_turn_is_still_empty() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "current_thread_bound": true,
                        "retrieval_context_pack_count": 0,
                        "matched_context_pack_ids_count": 0
                    },
                    "live_response_latency": {
                        "current_session_relation": {
                            "status": "recent_same_chat_series_previous_turn"
                        },
                        "current_thread_live_file_hints": {
                            "hints": [
                                {"label": "dashboard.rs", "query": "./src/dashboard.rs"}
                            ]
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 0u64,
                    "current_goal": "Current live-turn now surfaces same-thread Amai activity after fresh context-pack",
                    "next_step": "Tighten current-work card so fresh exact-pair / thread activity is surfaced there too.",
                    "last_command": null,
                    "last_results_summary": null,
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        let rows = card["rows"].as_array().expect("rows");
        let last_command_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Последняя команда"))
            .expect("last command row");
        assert_eq!(
            last_command_row["value"].as_str(),
            Some("Amai context pack")
        );
    }

    #[test]
    fn working_state_card_falls_back_to_live_turn_when_working_state_is_missing() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "thread_activity_observed_turn_open",
                        "retrieval_context_pack_count": 2,
                        "matched_context_pack_ids_count": 1,
                        "note": "Observed new retrieval_context_pack after the last completed turn."
                    }
                }
            },
            "latest_repo_working_state_restore": null
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("живой turn уже виден"));
        assert!(card["note"].as_str().is_some_and(|note| {
            note.contains("текущий chat turn уже видит свежую активность Amai")
        }));
        let rows = card["rows"].as_array().expect("rows");
        let live_turn_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Живой turn Amai"))
            .expect("live turn row");
        assert_eq!(
            live_turn_row["value"].as_str(),
            Some("2 context-pack • turn ещё открыт")
        );
    }

    #[test]
    fn working_state_card_surfaces_open_turn_without_amai_answer_yet() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775420265000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "current_thread_bound": true,
                        "thread_id": "thread-live",
                        "note": "В текущем live-turn не наблюдалось ни одного retrieval_context_pack от Amai."
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "next_step": "Wait for the next real Amai answer in this chat.",
                    "current_goal": "Observe the next online answer",
                    "events_count": 0u64,
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("ждём ответ Amai"));
        let rows = card["rows"].as_array().expect("rows");
        let live_turn_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Живой turn Amai"))
            .expect("live turn row");
        assert_eq!(
            live_turn_row["value"].as_str(),
            Some("turn открыт • ответов Amai ещё нет")
        );
        assert!(
            card["status_tooltip"]
                .as_str()
                .is_some_and(|tooltip| tooltip.contains("Amai в нём ещё не ответила"))
        );
    }

    #[test]
    fn working_state_card_uses_live_file_hints_when_active_files_are_empty() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775420265000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "current_thread_bound": true,
                        "thread_id": "thread-live"
                    },
                    "live_response_latency": {
                        "current_thread_live_file_hints": {
                            "hints": [
                                { "label": "dashboard.rs", "query": "./src/dashboard.rs" },
                                { "label": "token_budget.rs", "query": "./src/token_budget.rs" }
                            ]
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "next_step": "Add live file hints.",
                    "current_goal": "Observe the next online answer",
                    "events_count": 0u64,
                    "restore_confidence": "preliminary",
                    "active_files": []
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        let rows = card["rows"].as_array().expect("rows");
        let active_files_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Активные файлы"))
            .expect("active files row");
        assert_eq!(
            active_files_row["value"].as_str(),
            Some("2 • dashboard.rs, token_budget.rs")
        );
    }

    #[test]
    fn summarize_working_state_next_step_humanizes_live_card_reconciliation_text() {
        assert_eq!(
            super::summarize_working_state_next_step(Some(
                "If user continues, refine operator-facing copy or expand the same reconciliation pattern to other live cards."
            )),
            "уточнить операторский текст в live-карточках"
        );
        assert_eq!(
            super::summarize_working_state_goal(
                Some(
                    "If user continues, refine operator-facing copy or expand the same reconciliation pattern to other live cards."
                ),
                None
            ),
            "доработка live-карточек панели"
        );
        assert_eq!(
            super::summarize_working_state_next_step(Some(
                "If user continues, enrich current-work card with live-thread active files or replace generic next-step text."
            )),
            "добавить в карточку текущей работы живые подсказки по активным файлам"
        );
        assert_eq!(
            super::summarize_working_state_next_step(Some(
                "Optionally continue by filling last-command placeholder from the same live-turn source so the card is fully operator-readable before working-state catches up."
            )),
            "заполнить в карточке текущей работы последнюю команду из живого Amai-turn"
        );
    }

    #[test]
    fn current_session_card_explains_raw_savings_vs_client_budget() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 2,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "total_naive_tokens": 920432,
                        "total_context_tokens": 94,
                        "effective_savings_pct": 99.98978740417543
                    },
                    "rolling_window": {},
                    "lifetime": {},
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        let note = cards[0]["note"].as_str().unwrap_or_default();
        assert!(note.contains("Короткая карточка только с проверяемыми цифрами по текущей сессии"));
        assert!(note.contains("реальная экономия на полной шкале клиента пока не доказана"));
        let rows = cards[0]["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]["label"].as_str(),
            Some("Экономия на учтённой части")
        );
    }

    #[test]
    fn hero_cards_explain_scope_and_strict_verified_fraction() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 120,
                        "verified_effective_savings_pct": 25.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 25.0,
                        "answer_like_counted_events": 1,
                        "verified_answer_like_savings_pct": 25.0,
                        "verified_baseline_tokens": 200,
                        "verified_delivered_tokens": 80,
                        "verified_recovery_tokens": 0,
                        "excluded_events_count": 3,
                        "excluded_effective_saved_tokens": 50,
                        "excluded_baseline_tokens": 400,
                        "excluded_delivered_tokens": 350,
                        "excluded_recovery_tokens": 0,
                        "total_naive_tokens": 600,
                        "total_context_tokens": 430,
                        "effective_savings_pct": 28.33,
                        "total_effective_saved_tokens": 170,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 12,
                        "counted_events": 6,
                        "verified_effective_saved_tokens": 38622,
                        "verified_effective_savings_pct": 83.29,
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 20,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 33.33,
                        "answer_like_counted_events": 6,
                        "verified_answer_like_savings_pct": 83.29
                    },
                    "lifetime": {
                        "events_total": 56,
                        "counted_events": 22,
                        "verified_effective_saved_tokens": 4824306,
                        "verified_effective_savings_pct": 99.14,
                        "started_at_epoch_ms": 100,
                        "ended_at_epoch_ms": 200,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 39.29,
                        "answer_like_counted_events": 22,
                        "verified_answer_like_savings_pct": 99.14
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("pass"));
        assert_eq!(
            cards[0]["title_tooltip"].as_str(),
            Some(
                "Показывает только проверяемые цифры по текущей сессии: реальную долю Amai на полной живой шкале turn, текущий лимит клиента и точность учтённой части."
            )
        );
        assert!(cards[1]["title_tooltip"].as_str().is_some_and(|value| {
            value.contains("только проверяемые цифры по рабочему окну")
        }));
        assert!(cards[2]["title_tooltip"].as_str().is_some_and(|value| {
            value.contains("только подтверждённые цифры за всё время")
        }));
        assert!(
            cards[1]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("подтверждённый хвост прошлых стартов")
        );
        assert!(
            cards[2]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("старый долг точности")
        );
    }

    #[test]
    fn hero_session_card_uses_waiting_status_before_verified_sample_exists() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 1,
                        "excluded_effective_saved_tokens": 243216,
                        "excluded_baseline_tokens": 243300,
                        "excluded_delivered_tokens": 84,
                        "excluded_recovery_tokens": 0,
                        "total_naive_tokens": 243300,
                        "total_context_tokens": 84,
                        "effective_savings_pct": 99.97,
                        "total_effective_saved_tokens": 243216,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "lifetime": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("waiting"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("ждём подтверждённую выборку")
        );
        assert!(
            cards[0]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("ни один из них ещё не подтвердился")
        );
        assert!(
            cards[0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая карточка только с проверяемыми цифрами по текущей сессии")
        );
    }

    #[test]
    fn hero_cards_surface_client_limit_alignment_when_preview_is_present() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 4,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 1200,
                        "total_context_tokens": 900,
                        "effective_savings_pct": 25.0,
                        "total_effective_saved_tokens": 300,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 7,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0
                    },
                    "lifetime": {
                        "events_total": 12,
                        "counted_events": 3,
                        "verified_effective_saved_tokens": 900,
                        "verified_effective_savings_pct": 75.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "only_non_live_scope_activity",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 0,
                                "non_live_events_count": 4,
                                "blocking_reasons": [
                                    "client_prompt_unmeasured",
                                    "no_live_usage_in_scope",
                                    "non_live_events_present_in_scope"
                                ]
                            }
                        },
                        "rolling_window": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "live_usage_unconfirmed_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 2,
                                "non_live_events_count": 5,
                                "blocking_reasons": [
                                    "client_prompt_unmeasured",
                                    "no_confirmed_live_usage_in_scope"
                                ]
                            }
                        },
                        "lifetime": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "partial_lower_bound_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 12,
                                "non_live_events_count": 0,
                                "blocking_reasons": [
                                    "client_prompt_unmeasured",
                                    "assistant_generation_unmeasured"
                                ]
                            }
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        for card in &cards {
            assert!(
                card["rows"]
                    .as_array()
                    .expect("rows")
                    .iter()
                    .all(|row| row["label"].as_str() != Some("Связь с лимитом клиента"))
            );
        }
        assert!(
            cards[0]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("живая шкала клиента")
        );
    }

    #[test]
    fn hero_cards_alert_when_continuity_startup_burn_dominates_live_window() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 0,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 0,
                        "total_context_tokens": 0,
                        "effective_savings_pct": 0.0,
                        "total_effective_saved_tokens": 0,
                        "total_recovery_tokens": 0,
                        "observed_continuity_restore_tokens": 817
                    },
                    "rolling_window": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "observed_continuity_restore_tokens": 817
                    },
                    "lifetime": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_without_amai_measured_tokens": 4,
                            "verified_with_amai_measured_tokens": 4,
                            "verified_measured_saved_tokens": 0,
                            "verified_measured_saved_pct": 0.0,
                            "client_limit_meter_alignment": {
                                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 1,
                                "non_live_events_count": 0,
                                "blocking_reasons": ["same_meter_baseline_explicit_boundary"],
                                "strict_client_meter_slice": {
                                    "same_meter_equivalent_for_slice": true,
                                    "lower_bound_tokens": 4,
                                    "components": ["client_prompt"]
                                },
                                "explicit_boundary_surface": {
                                    "state": "amai_continuity_boundary",
                                    "components": ["continuity_restore_outside_retrieval"],
                                    "note": "Continuity boundary."
                                },
                                "continuity_boundary_rollup": {
                                    "state": "amai_continuity_boundary_observed",
                                    "observed_tokens": 817
                                },
                                "baseline_equivalence": {
                                    "state": "baseline_component_semantics_explicit_boundary",
                                    "measured_baseline_components": ["client_prompt"],
                                    "explicitly_unmodeled_baseline_components": ["continuity_restore_outside_retrieval"]
                                }
                            }
                        },
                        "rolling_window": {
                            "verified_without_amai_measured_tokens": 4,
                            "verified_with_amai_measured_tokens": 4,
                            "verified_measured_saved_tokens": 0,
                            "verified_measured_saved_pct": 0.0,
                            "client_limit_meter_alignment": {
                                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 1,
                                "non_live_events_count": 0,
                                "blocking_reasons": ["same_meter_baseline_explicit_boundary"],
                                "strict_client_meter_slice": {
                                    "same_meter_equivalent_for_slice": true,
                                    "lower_bound_tokens": 4,
                                    "components": ["client_prompt"]
                                },
                                "explicit_boundary_surface": {
                                    "state": "amai_continuity_boundary",
                                    "components": ["continuity_restore_outside_retrieval"],
                                    "note": "Continuity boundary."
                                },
                                "continuity_boundary_rollup": {
                                    "state": "amai_continuity_boundary_observed",
                                    "observed_tokens": 817
                                },
                                "baseline_equivalence": {
                                    "state": "baseline_component_semantics_explicit_boundary",
                                    "measured_baseline_components": ["client_prompt"],
                                    "explicitly_unmodeled_baseline_components": ["continuity_restore_outside_retrieval"]
                                }
                            }
                        },
                        "lifetime": {
                            "verified_without_amai_measured_tokens": 8,
                            "verified_with_amai_measured_tokens": 8,
                            "verified_measured_saved_tokens": 0,
                            "verified_measured_saved_pct": 0.0,
                            "client_limit_meter_alignment": {
                                "alignment_state": "partial_lower_bound_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 1,
                                "non_live_events_count": 0,
                                "blocking_reasons": ["client_prompt_unmeasured"]
                            }
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("alert"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("burn в continuity startup")
        );
        assert!(
            cards[0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая карточка только с проверяемыми цифрами по текущей сессии")
        );
        let model_row = cards[0]["rows"]
            .as_array()
            .expect("session rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Экономия на учтённой части"))
            .expect("model-token row");
        assert!(
            model_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("Точного процента пока нет")
        );
    }

    #[test]
    fn exact_model_component_delta_row_surfaces_top_same_meter_driver() {
        let alignment = json!({
            "baseline_equivalence": {
                "component_semantics": [
                    {
                        "code": "client_prompt",
                        "baseline_measured_tokens": 48,
                        "observed_tokens": 48,
                        "whole_cycle_observed_complete": true
                    },
                    {
                        "code": "continuity_restore_outside_retrieval",
                        "baseline_measured_tokens": 8228,
                        "observed_tokens": 8456,
                        "whole_cycle_observed_complete": true
                    }
                ]
            }
        });

        let row = super::exact_model_component_delta_metric_row(&alignment).expect("row");
        assert_eq!(row["label"].as_str(), Some("Главный драйвер exact-пары"));
        assert_eq!(
            row["value"].as_str(),
            Some("continuity-restore overhead вне retrieval: 8228 -> 8456 (+228 к расходу)")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("исходный запрос клиента: 48 -> 48 (без разницы)")
        );
    }

    #[test]
    fn rolling_window_card_surfaces_historical_startup_drag_when_current_session_is_profitable() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 265,
                        "verified_effective_savings_pct": 60.91954022988506,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 0,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 0,
                        "total_context_tokens": 0,
                        "effective_savings_pct": 0.0,
                        "total_effective_saved_tokens": 0,
                        "total_recovery_tokens": 0,
                        "observed_continuity_restore_tokens": 170
                    },
                    "rolling_window": {
                        "events_total": 15,
                        "counted_events": 15,
                        "verified_effective_saved_tokens": -181,
                        "verified_effective_savings_pct": -1.8986677855869087,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "observed_continuity_restore_tokens": 9714
                    },
                    "lifetime": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_without_amai_measured_tokens": 435,
                            "verified_with_amai_measured_tokens": 0,
                            "verified_observed_whole_cycle_with_amai_tokens": 174,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "continuity_boundary_rollup": {
                                    "observed_tokens": 0
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 439
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 439,
                                    "component_semantics": [
                                        {
                                            "code": "client_prompt",
                                            "baseline_measured_tokens": 4,
                                            "observed_tokens": 4,
                                            "whole_cycle_observed_complete": true
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 435,
                                            "observed_tokens": 170,
                                            "whole_cycle_observed_complete": true
                                        }
                                    ]
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        },
                        "rolling_window": {
                            "verified_without_amai_measured_tokens": 9533,
                            "verified_with_amai_measured_tokens": 0,
                            "verified_observed_whole_cycle_with_amai_tokens": 9774,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "continuity_boundary_rollup": {
                                    "observed_tokens": 0
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 9593
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 9593,
                                    "component_semantics": [
                                        {
                                            "code": "client_prompt",
                                            "baseline_measured_tokens": 60,
                                            "observed_tokens": 60,
                                            "whole_cycle_observed_complete": true
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 9533,
                                            "observed_tokens": 9714,
                                            "whole_cycle_observed_complete": true
                                        }
                                    ]
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        },
                        "lifetime": {
                            "verified_without_amai_measured_tokens": 8,
                            "verified_with_amai_measured_tokens": 8,
                            "verified_observed_whole_cycle_with_amai_tokens": 8,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "continuity_boundary_rollup": {
                                    "observed_tokens": 0
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 8
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 8
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[1]["status"].as_str(), Some("alert"));
        assert_eq!(
            cards[1]["status_label"].as_str(),
            Some("исторический startup drag")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая карточка только с проверяемыми цифрами по рабочему окну.")
        );
        let row = cards[1]["rows"]
            .as_array()
            .expect("rolling rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Хвост от прошлых стартов"))
            .expect("historical startup drag row");
        assert_eq!(
            row["value"].as_str(),
            Some("вне текущей сессии: без Amai 9154, с Amai 9600, +446 к расходу")
        );
        assert!(row["tooltip"].as_str().unwrap_or_default().contains("9544"));
    }

    #[test]
    fn exact_pair_status_override_marks_irrecoverable_gap_as_alert() {
        let alignment = json!({
            "exact_pair_status": {
                "state": "exact_pair_blocked",
                "blockers": [{
                    "code": "tool_overhead_outside_retrieval",
                    "missing_live_events": 13,
                    "irrecoverable_missing_live_events": 13
                }]
            }
        });

        let (status, label, tooltip) =
            super::exact_pair_card_status_override(&alignment).expect("status override");
        assert_eq!(status, "alert");
        assert_eq!(label, "есть старый долг точности");
        assert!(tooltip.contains("Не хватает строк: 13"));
        assert!(tooltip.contains("Потеряно без восстановления: 13"));
    }

    #[test]
    fn exact_pair_status_override_marks_recoverable_gap_as_waiting() {
        let alignment = json!({
            "exact_pair_status": {
                "state": "exact_pair_blocked",
                "blockers": [{
                    "code": "tool_overhead_outside_retrieval",
                    "missing_live_events": 7,
                    "irrecoverable_missing_live_events": 0
                }]
            }
        });

        let (status, label, tooltip) =
            super::exact_pair_card_status_override(&alignment).expect("status override");
        assert_eq!(status, "waiting");
        assert_eq!(label, "ждём полного совпадения");
        assert!(tooltip.contains("совпадение с реальной шкалой лимита модели ещё не собрано"));
    }

    #[test]
    fn exact_pair_status_metric_row_surfaces_frozen_debt_review() {
        let alignment = json!({
            "exact_pair_status": {
                "state": "exact_pair_blocked",
                "exact_pair_available": false,
                "blockers": [{
                    "code": "tool_overhead_outside_retrieval",
                    "frozen_gap_candidate": true,
                    "missing_live_events": 13,
                    "irrecoverable_missing_live_events": 13
                }]
            }
        });

        let row = super::exact_pair_status_metric_row(&alignment).expect("exact pair row");
        assert_eq!(row["label"], "Совпадение с реальным лимитом");
        assert_eq!(
            row["value"].as_str(),
            Some("цифра пока не полностью точная: в старой истории потеряно 13 строк")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("старой исторической потери данных")
        );
    }

    #[test]
    fn exact_pair_status_metric_row_surfaces_exact_materialized() {
        let alignment = json!({
            "exact_pair_status": {
                "state": "exact_pair_materialized",
                "exact_pair_available": true
            }
        });

        let row = super::exact_pair_status_metric_row(&alignment).expect("exact pair row");
        assert_eq!(
            row["value"].as_str(),
            Some("цифра точная: полностью совпадает со шкалой лимита модели")
        );
    }

    #[test]
    fn exact_pair_frozen_debt_metric_row_surfaces_resolution_law() {
        let alignment = json!({
            "frozen_gap_review_surface": {
                "state": "review_required",
                "blocking_component": "tool_overhead_outside_retrieval",
                "missing_live_events": 13,
                "irrecoverable_missing_live_events": 13,
                "recoverable_missing_live_events": 0,
                "resolution_condition": "freeze_irrecoverable_gap_or_keep_exact_pair_unavailable"
            },
            "exact_pair_status": {
                "state": "exact_pair_blocked",
                "blockers": [{
                    "code": "tool_overhead_outside_retrieval",
                    "frozen_gap_candidate": true,
                    "missing_live_events": 13,
                    "irrecoverable_missing_live_events": 13,
                    "recoverable_missing_live_events": 0,
                    "resolution_condition": "freeze_irrecoverable_gap_or_keep_exact_pair_unavailable"
                }]
            }
        });

        let row = super::exact_pair_frozen_debt_metric_row(&alignment).expect("frozen debt row");
        assert_eq!(row["label"], "Frozen debt exact-пары");
        assert_eq!(
            row["value"].as_str(),
            Some("tool_overhead_outside_retrieval: 13 irrecoverable rows")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("freeze_irrecoverable_gap_or_keep_exact_pair_unavailable")
        );
    }

    #[test]
    fn historical_frozen_debt_metric_row_surfaces_historical_only_tail() {
        let current_session_alignment = json!({
            "exact_pair_status": {
                "exact_pair_available": true
            }
        });
        let rolling_window_alignment = json!({
            "exact_pair_status": {
                "exact_pair_available": true
            }
        });
        let lifetime_alignment = json!({
            "frozen_gap_review_surface": {
                "state": "review_required",
                "blocking_component": "tool_overhead_outside_retrieval",
                "irrecoverable_missing_live_events": 13
            }
        });

        let row = super::historical_frozen_debt_metric_row(
            &current_session_alignment,
            &rolling_window_alignment,
            &lifetime_alignment,
        )
        .expect("historical tail row");
        assert_eq!(row["label"], "Исторический frozen debt");
        assert_eq!(
            row["value"].as_str(),
            Some("tool_overhead_outside_retrieval: historical-only, 13 rows")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Current session: exact pair materialized")
        );
    }

    #[test]
    fn client_full_turn_savings_metric_row_surfaces_full_turn_share() {
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 35534
        });
        let row = super::client_full_turn_savings_metric_row(&meter, Some((550, 127, 423, 76.91)))
            .expect("full turn row");
        assert_eq!(
            row["key"].as_str(),
            Some(super::CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
        );
        assert_eq!(row["label"], "Amai в полном live-turn");
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("без Amai 35957")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("замедлением расхода шкалы VS Code")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("rollout token_count.last_token_usage.total_tokens")
        );
    }

    #[test]
    fn client_full_turn_savings_metric_row_hides_percent_until_exact_turn_pair_exists() {
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "current_thread_bound",
            "current_thread_bound": true,
            "client_turn_total_tokens": 35534
        });
        let row = super::client_full_turn_savings_metric_row(&meter, None).expect("full turn row");
        assert_eq!(
            row["key"].as_str(),
            Some(super::CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
        );
        assert_eq!(row["label"], "Amai в полном live-turn");
        assert_eq!(
            row["value"].as_str(),
            Some("точный процент по шкале VS Code пока не доказан")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("единственный процент")
        );
    }

    #[test]
    fn client_full_turn_savings_metric_row_surfaces_unbound_meter_as_unproven() {
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "no_current_thread_binding",
            "current_thread_bound": false,
            "client_turn_total_tokens": 35534
        });
        let row = super::client_full_turn_savings_metric_row(&meter, None).expect("full turn row");
        assert_eq!(
            row["key"].as_str(),
            Some(super::CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
        );
        assert_eq!(row["label"], "Amai в полном live-turn");
        assert_eq!(
            row["value"].as_str(),
            Some("точный процент по шкале VS Code пока не доказан")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("current-thread binding")
        );
    }

    #[test]
    fn client_live_limit_metric_row_surfaces_remaining_budget() {
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "current_thread_bound",
            "current_thread_bound": true,
            "primary_limit_remaining_percent": 31,
            "primary_limit_used_percent": 69,
            "secondary_limit_remaining_percent": 79,
            "secondary_limit_used_percent": 21,
            "ended_at_epoch_ms": 1774625102000u64
        });
        let row = super::client_live_limit_metric_row(&meter).expect("limit row");
        assert_eq!(row["key"].as_str(), Some("client_live_limit"));
        assert_eq!(row["label"], "Лимит клиента сейчас");
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("5ч остаётся 31.00%")
        );
        assert!(row["value"].as_str().unwrap_or_default().contains("raw"));
    }

    #[test]
    fn client_live_limit_metric_row_prefers_exact_status_bar_source() {
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "no_current_thread_binding",
            "current_thread_bound": false,
            "primary_limit_remaining_percent": 31,
            "primary_limit_used_percent": 69,
            "secondary_limit_remaining_percent": 79,
            "secondary_limit_used_percent": 21,
            "ended_at_epoch_ms": 1774625102000u64,
            "status_bar_rate_limits": {
                "status": "observed",
                "source": "codex_app_server_account_rate_limits_read_v1",
                "status_bar_correlated": true,
                "observed_at_epoch_ms": 1774682249000u64,
                "primary_limit_used_percent": 38.0,
                "primary_limit_remaining_percent": 62.0,
                "secondary_limit_used_percent": 41.0,
                "secondary_limit_remaining_percent": 59.0
            }
        });
        let row = super::client_live_limit_metric_row(&meter).expect("limit row");
        assert_eq!(row["key"].as_str(), Some("client_live_limit"));
        assert_eq!(row["label"], "Лимит клиента сейчас");
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("5ч остаётся 62.00%, 7д остаётся 59.00%")
        );
        assert!(row["value"].as_str().unwrap_or_default().contains("live"));
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("codex app-server account/rateLimits/read")
        );
    }

    #[test]
    fn current_live_turn_exact_pair_surfaces_zero_pair() {
        let current_live_turn = json!({
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            }
        });
        assert_eq!(
            super::current_live_turn_exact_pair(&current_live_turn),
            Some((0, 0, 0, 0.0))
        );
    }

    #[test]
    fn client_full_turn_savings_metric_row_surfaces_zero_percent_when_no_amai_activity() {
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "current_thread_bound",
            "current_thread_bound": true,
            "client_turn_total_tokens": 35534
        });
        let row = super::client_full_turn_savings_metric_row(&meter, Some((0, 0, 0, 0.0)))
            .expect("full turn row");
        assert!(row["value"].as_str().unwrap_or_default().contains("0.00%"));
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("delta 0")
        );
    }

    #[test]
    fn client_live_context_metric_row_uses_last_request_window_pressure() {
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "current_thread_bound",
            "current_thread_bound": true,
            "client_turn_total_tokens": 133419,
            "latest_model_context_window": 258400,
            "context_used_percent": 51.633359133126934,
            "ended_at_epoch_ms": 1774625102000u64
        });
        let row = super::client_live_context_metric_row(&meter).expect("context row");
        assert_eq!(row["key"].as_str(), Some("client_live_context"));
        assert_eq!(row["label"].as_str(), Some("Последний запрос клиента"));
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("133419 из 258400")
        );
        assert!(row["value"].as_str().unwrap_or_default().contains("raw"));
    }

    #[test]
    fn client_turn_pressure_guard_requires_current_thread_binding() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "thread_binding_state": "no_current_thread_binding",
            "current_thread_bound": false,
            "client_turn_total_tokens": 140921,
            "latest_model_context_window": 258400,
            "context_used_percent": 54.54,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0,
            "ended_at_epoch_ms": 1774622949000u64
        });
        assert!(
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .is_none()
        );
        assert!(super::client_live_context_metric_row(&meter).is_none());
        let row = super::client_live_limit_metric_row(&meter).expect("limit row");
        assert_eq!(row["label"], "Последний observed лимит клиента");
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("последнее observed:")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("latest observed")
        );
    }

    #[test]
    fn client_budget_live_payload_surfaces_only_available_rows() {
        let snapshot = json!({
            "token_budget_report": {
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "no_current_thread_binding",
                    "current_thread_bound": false,
                    "primary_limit_remaining_percent": 78,
                    "primary_limit_used_percent": 22,
                    "secondary_limit_remaining_percent": 63,
                    "secondary_limit_used_percent": 37,
                    "ended_at_epoch_ms": 1774683538000u64
                },
                "client_limit_hourly_burn": {
                    "status": "insufficient_history",
                    "reply_prefix": "5ч KPI: н/д"
                }
            }
        });
        let payload = super::client_budget_live_payload(&snapshot);
        let rows = payload["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0]["key"].as_str(),
            Some("client_live_full_turn_savings")
        );
        assert_eq!(rows[1]["key"].as_str(), Some("client_live_limit"));
        assert_eq!(rows[2]["key"].as_str(), Some("client_limit_hourly_burn"));
        assert_eq!(
            rows[1]["label"].as_str(),
            Some("Последний observed лимит клиента")
        );
    }

    #[test]
    fn client_budget_live_payload_surfaces_exact_live_limit_without_rollout_meter() {
        let snapshot = json!({
            "token_budget_report": {
                "client_live_meter": {
                    "status": "missing",
                    "status_bar_rate_limits": {
                        "status": "observed",
                        "source": "codex_app_server_account_rate_limits_read_v1",
                        "status_bar_correlated": true,
                        "observed_at_epoch_ms": 1774682249000u64,
                        "primary_limit_used_percent": 39.0,
                        "primary_limit_remaining_percent": 61.0,
                        "secondary_limit_used_percent": 42.0,
                        "secondary_limit_remaining_percent": 58.0
                    }
                },
                "client_limit_hourly_burn": {
                    "status": "insufficient_history",
                    "reply_prefix": "5ч KPI: н/д"
                }
            }
        });
        let payload = super::client_budget_live_payload(&snapshot);
        let rows = payload["rows"].as_array().expect("rows array");
        assert_eq!(payload["status"], json!("observed"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["key"].as_str(), Some("client_live_limit"));
        assert_eq!(rows[0]["label"].as_str(), Some("Лимит клиента сейчас"));
        assert!(
            rows[0]["value"]
                .as_str()
                .unwrap_or_default()
                .contains("5ч остаётся 61.00%, 7д остаётся 58.00%")
        );
        assert_eq!(rows[1]["key"].as_str(), Some("client_limit_hourly_burn"));
    }

    #[test]
    fn client_budget_live_payload_surfaces_current_live_turn_numeric_row() {
        let snapshot = json!({
            "token_budget_report": {
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "client_turn_total_tokens": 35534,
                    "primary_limit_remaining_percent": 61,
                    "secondary_limit_remaining_percent": 58,
                    "ended_at_epoch_ms": 1774682249000u64
                },
                "current_live_turn": {
                    "exact_pair_available": true,
                    "exact_pair": {
                        "without_amai_tokens": 0,
                        "with_amai_tokens": 0,
                        "saved_tokens": 0,
                        "saved_pct": 0.0
                    }
                }
            }
        });
        let payload = super::client_budget_live_payload(&snapshot);
        let rows = payload["rows"].as_array().expect("rows array");
        assert_eq!(
            rows[0]["key"].as_str(),
            Some("client_live_full_turn_savings")
        );
        assert!(
            rows[0]["value"]
                .as_str()
                .unwrap_or_default()
                .contains("0.00%")
        );
    }

    #[test]
    fn client_budget_root_cause_payload_stays_compact_and_surfaces_primary_blocker() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "client_turn_total_tokens": 3210,
                        "context_used_percent": 64.0,
                        "ended_at_epoch_ms": 2000,
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": 2000
                        }
                    },
                    "current_live_turn": {
                        "status": "same_meter_pending",
                        "exact_pair_available": false,
                        "observed_client_prompt_tokens": 22,
                        "observed_assistant_generation_tokens": 0,
                        "observed_continuity_restore_tokens": 144,
                        "observed_tool_overhead_tokens": 311,
                        "observed_whole_cycle_with_amai_tokens": 477
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "reply_prefix": "5ч KPI: переплата 20.00%"
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "exact_pair_status": {
                                    "state": "exact_pair_blocked",
                                    "exact_pair_available": false,
                                    "primary_blocking_reason": "assistant_generation_unmeasured",
                                    "blockers": [
                                        {
                                            "code": "assistant_generation",
                                            "blocker_kind": "generic_alignment_gap",
                                            "blocking_reason": "assistant_generation_unmeasured",
                                            "missing_live_events": 1,
                                            "irrecoverable_missing_live_events": 0
                                        }
                                    ]
                                },
                                "measured_components": ["client_prompt", "continuity_restore_outside_retrieval"],
                                "missing_components": ["assistant_generation", "tool_overhead_outside_retrieval"],
                                "partially_measured_components": ["tool_overhead_outside_retrieval"],
                                "blocking_reasons": ["assistant_generation_unmeasured"]
                            }
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {}
            }
        });

        let payload = super::client_budget_root_cause_payload(&snapshot);
        assert_eq!(
            payload["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 20.00%")
        );
        assert_eq!(
            payload["exact_pair_status"]["primary_blocker_code"].as_str(),
            Some("assistant_generation")
        );
        assert_eq!(
            payload["exact_pair_status"]["note"].as_str(),
            Some(
                "Exact pair сейчас удерживает assistant-generation baseline semantics: observed output tokens уже видны, но deduplicated same-meter baseline для этого scope ещё не materialized."
            )
        );
        assert_eq!(
            payload["guard"]["reply_budget_mode"].as_str(),
            Some(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert!(
            serde_json::to_string(&payload)
                .expect("compact payload")
                .len()
                < 2500
        );
    }

    #[test]
    fn client_budget_root_cause_payload_omits_zero_activity_noise() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "client_turn_total_tokens": 172361,
                        "context_used_percent": 66.7,
                        "ended_at_epoch_ms": 2000,
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": 2000
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
                        "observed_client_prompt_tokens": null,
                        "observed_assistant_generation_tokens": null,
                        "observed_continuity_restore_tokens": null,
                        "observed_tool_overhead_tokens": null,
                        "observed_whole_cycle_with_amai_tokens": null,
                        "verified_observed_whole_cycle_with_amai_tokens": null
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "reply_prefix": "5ч KPI: переплата 75.41%"
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "exact_pair_status": {
                                    "state": "exact_pair_materialized",
                                    "exact_pair_available": true,
                                    "primary_blocking_reason": null,
                                    "blockers": []
                                },
                                "measured_components": ["retrieval_payload", "followup_recovery", "client_prompt", "continuity_restore_outside_retrieval"],
                                "missing_components": [],
                                "partially_measured_components": [],
                                "blocking_reasons": []
                            }
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {}
            }
        });

        let payload = super::client_budget_root_cause_payload(&snapshot);
        assert_eq!(
            payload["current_live_turn"]["status"].as_str(),
            Some("no_amai_activity_in_current_live_turn")
        );
        assert_eq!(payload["current_live_turn"]["saved_pct"], json!(0.0));
        assert!(payload["current_live_turn"]["exact_pair"].is_null());
        assert!(payload["current_live_turn"]["observed_client_prompt_tokens"].is_null());
        assert_eq!(
            payload["exact_pair_status"]["state"].as_str(),
            Some("not_applicable_current_live_turn_has_no_amai_activity")
        );
        assert_eq!(
            payload["exact_pair_status"]["note"].as_str(),
            Some(
                "В текущем live-turn у Amai нет активности, поэтому exact-pair blocker surface здесь не про missing measurement, а про нулевой вклад: для этого turn Amai честно даёт 0.00% same-meter savings."
            )
        );
        assert!(payload["exact_pair_status"]["primary_blocker_code"].is_null());
        assert!(payload["exact_pair_status"]["missing_live_events"].is_null());
        assert!(payload["missing_components"].is_null());
        assert!(payload["partially_measured_components"].is_null());
        assert!(payload["blocking_reasons"].is_null());
        assert!(
            serde_json::to_string(&payload)
                .expect("compact payload")
                .len()
                < 1600
        );
    }

    #[test]
    fn client_budget_root_cause_payload_surfaces_same_meter_economics_for_giant_thread_overhang() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "client_turn_total_tokens": 87356,
                        "context_used_percent": 33.45,
                        "ended_at_epoch_ms": 2000,
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": 2000
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
                        }
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "reply_prefix": "5ч KPI: переплата 1988.49%"
                    },
                    "statement_previews": {
                        "current_session": {
                            "observed_whole_cycle_with_amai_tokens": 72,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
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
                                            "whole_cycle_observed_complete": true
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 178,
                                            "observed_tokens": 68,
                                            "whole_cycle_observed_complete": true
                                        }
                                    ]
                                },
                                "exact_pair_status": {
                                    "state": "exact_pair_materialized",
                                    "exact_pair_available": true,
                                    "primary_blocking_reason": null,
                                    "blockers": []
                                },
                                "measured_components": [
                                    "client_prompt",
                                    "continuity_restore_outside_retrieval"
                                ],
                                "missing_components": [],
                                "partially_measured_components": [],
                                "blocking_reasons": []
                            }
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {}
            }
        });

        let payload = super::client_budget_root_cause_payload(&snapshot);
        assert_eq!(
            payload["same_meter_economics"]["strict_lower_bound_tokens"],
            json!(182)
        );
        assert_eq!(
            payload["same_meter_economics"]["same_meter_saved_tokens"],
            json!(110)
        );
        assert_eq!(
            payload["same_meter_economics"]["continuity_restore_baseline_tokens"],
            json!(178)
        );
        assert_eq!(
            payload["same_meter_economics"]["continuity_restore_observed_tokens"],
            json!(68)
        );
        assert_eq!(
            payload["same_meter_economics"]["continuity_restore_delta_tokens"],
            json!(-110)
        );
        assert_eq!(
            payload["same_meter_economics"]["full_turn_overhang_tokens"],
            json!(87174)
        );
        assert_eq!(
            payload["same_meter_economics"]["dominant_cost_surface"],
            json!("giant_thread_context_outside_same_meter_slice")
        );
    }

    #[test]
    fn client_turn_pressure_guard_triggers_on_large_thread_with_weak_full_turn_share() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 2,
                        "verified_effective_saved_tokens": 445,
                        "verified_effective_savings_pct": 78.76,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 50.0,
                        "answer_like_counted_events": 2,
                        "verified_answer_like_savings_pct": 78.76,
                        "verified_baseline_tokens": 565,
                        "verified_delivered_tokens": 120,
                        "verified_recovery_tokens": 0,
                        "excluded_events_count": 2,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 565,
                        "total_context_tokens": 120,
                        "effective_savings_pct": 78.76,
                        "total_effective_saved_tokens": 445,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "lifetime": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_observed_whole_cycle_with_amai_tokens": 206274,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": {
                                    "exact_pair_available": true
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 206719
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        }
                    },
                    "statement_export_previews": {
                        "lifetime": {}
                    },
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "019d4eb1-3e92-75e3-b22b-2bdf21f13885",
                        "client_turn_total_tokens": 206274,
                        "latest_model_context_window": 258400,
                        "context_used_percent": 79.82739938080495,
                        "primary_limit_remaining_percent": 28.0,
                        "secondary_limit_remaining_percent": 78.0
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("critical"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("сожми текущий чат сейчас")
        );
        assert!(
            cards[0]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("внешний лимит клиента уже горит быстрее")
        );
        let row = cards[0]["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Следующее действие"))
            .expect("next action row");
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("проверь effect")
        );
    }

    #[test]
    fn client_turn_pressure_guard_stays_off_for_light_live_turn() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 95.0
        });
        let current_live_turn = json!({
            "status": "observed",
            "retrieval_context_pack_count": 1
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 22000,
            "latest_model_context_window": 258400,
            "context_used_percent": 8.51,
            "primary_limit_remaining_percent": 74.0,
            "secondary_limit_remaining_percent": 91.0
        });
        assert!(
            super::client_turn_pressure_guard(
                &meter,
                Some((22420, 22000, 420, 1.87)),
                &hourly_burn,
                &current_live_turn
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_triggers_on_nearly_exhausted_primary_limit_even_below_70pct() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 178971,
            "latest_model_context_window": 258400,
            "context_used_percent": 69.26,
            "primary_limit_remaining_percent": 3.0,
            "secondary_limit_remaining_percent": 71.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((179416, 178971, 445, 0.25)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_triggers_early_when_exact_full_turn_pair_is_missing() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 118116,
            "latest_model_context_window": 258400,
            "context_used_percent": 45.71,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard =
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert!(
            super::client_turn_pressure_tooltip(guard, None, false,).contains("слишком раздут")
        );
    }

    #[test]
    fn client_turn_pressure_tooltip_surfaces_same_thread_host_control_when_present() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 118116,
            "latest_model_context_window": 258400,
            "context_used_percent": 45.71,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard =
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .expect("pressure guard");
        let bundle = json!({
            "host_current_thread_control": working_state::build_host_current_thread_control_surface()
        });
        let tooltip = super::client_turn_pressure_tooltip(guard, Some(&bundle), false);
        assert!(tooltip.contains("same-thread host surface"));
        assert!(tooltip.contains("thread-overlay-open-current"));
    }

    #[test]
    fn client_turn_pressure_guard_triggers_when_exact_full_turn_savings_are_tiny() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 140921,
            "latest_model_context_window": 258400,
            "context_used_percent": 54.54,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((141366, 140921, 445, 0.31)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_blocks_earlier_for_negligible_exact_savings() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 90000,
            "latest_model_context_window": 258400,
            "context_used_percent": 34.83,
            "primary_limit_remaining_percent": 82.0,
            "secondary_limit_remaining_percent": 95.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((90452, 90000, 452, 0.50)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_even_earlier_for_small_negligible_gain() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 65000,
            "latest_model_context_window": 258400,
            "context_used_percent": 25.15,
            "primary_limit_remaining_percent": 94.0,
            "secondary_limit_remaining_percent": 97.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((65320, 65000, 320, 0.49)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
    }

    #[test]
    fn client_turn_pressure_guard_escalates_to_critical_when_primary_budget_is_nearly_burned() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 88241,
            "latest_model_context_window": 258400,
            "context_used_percent": 34.15,
            "primary_limit_remaining_percent": 8.0,
            "secondary_limit_remaining_percent": 72.0
        });
        let guard =
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_early_when_5h_kpi_overspends_without_amai_activity() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 36.59
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 18200,
            "latest_model_context_window": 258400,
            "context_used_percent": 7.04,
            "primary_limit_remaining_percent": 64.0,
            "secondary_limit_remaining_percent": 82.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((0, 0, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_when_5h_kpi_overspends_with_weak_live_gain() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 111.87
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 85681,
                "with_amai_tokens": 84456,
                "saved_tokens": 1225,
                "saved_pct": 1.4297218753282526
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 84456,
            "latest_model_context_window": 258400,
            "context_used_percent": 32.68421052631579,
            "primary_limit_remaining_percent": 75.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((85681, 84456, 1225, 1.4297218753282526)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_overspend_large_thread_even_with_fresh_budget() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 48.53
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 128709,
                "with_amai_tokens": 127509,
                "saved_tokens": 1200,
                "saved_pct": 0.9323357284223408
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 127509,
            "latest_model_context_window": 258400,
            "context_used_percent": 50.65,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 51.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((128709, 127509, 1200, 0.9323357284223408)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_huge_overspend_thread_before_primary_limit_softens()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 47.59
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 150842,
                "with_amai_tokens": 150104,
                "saved_tokens": 738,
                "saved_pct": 0.4892536561435144
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 150104,
            "latest_model_context_window": 258400,
            "context_used_percent": 58.09,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 29.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((150842, 150104, 738, 0.4892536561435144)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_huge_no_amai_thread_without_hourly_burn_surface()
    {
        let hourly_burn = json!({});
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 130000,
            "latest_model_context_window": 258400,
            "context_used_percent": 50.31,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((0, 0, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert!(guard.no_amai_activity_in_current_live_turn);
        assert_eq!(guard.hourly_burn_classification, None);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_large_no_amai_thread_without_hourly_burn_surface()
     {
        let hourly_burn = json!({});
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 76000,
            "latest_model_context_window": 258400,
            "context_used_percent": 30.44,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((0, 0, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert!(guard.no_amai_activity_in_current_live_turn);
        assert_eq!(guard.hourly_burn_classification, None);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_huge_no_amai_thread_when_exact_5h_kpi_is_below_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 14.07
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 85.0,
            "secondary_limit_remaining_percent": 5.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((196009, 196009, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_weak_exact_pair_thread_when_5h_kpi_is_below_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 42.89
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 87076,
                "with_amai_tokens": 85435,
                "saved_tokens": 1641,
                "saved_pct": 1.8845606137167532
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 85435,
            "latest_model_context_window": 258400,
            "context_used_percent": 33.06308049535604,
            "primary_limit_remaining_percent": 82.0,
            "secondary_limit_remaining_percent": 91.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((87076, 85435, 1641, 1.8845606137167532)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_high_context_thread_when_exact_pair_is_far_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 20.19
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 507742,
                "with_amai_tokens": 177981,
                "saved_tokens": 329761,
                "saved_pct": 64.94656735113503
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 177981,
            "latest_model_context_window": 258400,
            "context_used_percent": 68.8780959752322,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((507742, 177981, 329761, 64.94656735113503)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_high_context_thread_when_exact_pair_is_below_90_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 88.4
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 180000,
                "with_amai_tokens": 25000,
                "saved_tokens": 155000,
                "saved_pct": 86.11111111111111
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 25000,
            "latest_model_context_window": 258400,
            "context_used_percent": 55.0,
            "primary_limit_remaining_percent": 92.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((180000, 25000, 155000, 86.11111111111111)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_early_when_exact_pair_is_below_90_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 82.5
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 90000,
                "with_amai_tokens": 18000,
                "saved_tokens": 72000,
                "saved_pct": 80.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 45000,
            "latest_model_context_window": 258400,
            "context_used_percent": 18.2,
            "primary_limit_remaining_percent": 94.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((90000, 18000, 72000, 80.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_on_small_thread_when_exact_pair_and_5h_kpi_are_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 87.1
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 62000,
                "with_amai_tokens": 10200,
                "saved_tokens": 51800,
                "saved_pct": 83.54838709677419
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 10200,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.05,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((62000, 10200, 51800, 83.54838709677419)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_on_moderate_thread_when_exact_pair_and_5h_kpi_are_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 84.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 110000,
                "with_amai_tokens": 19000,
                "saved_tokens": 91000,
                "saved_pct": 82.72727272727273
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 19000,
            "latest_model_context_window": 258400,
            "context_used_percent": 7.35,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((110000, 19000, 91000, 82.72727272727273)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_respects_custom_50_percent_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 62.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 30000,
                "with_amai_tokens": 12000,
                "saved_tokens": 18000,
                "saved_pct": 60.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 12000,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.65,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((30000, 12000, 18000, 60.0)),
                &hourly_burn,
                &current_live_turn,
                50,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_zero_target_disables_target_only_pressure() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 87.1
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 62000,
                "with_amai_tokens": 10200,
                "saved_tokens": 51800,
                "saved_pct": 83.54838709677419
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 10200,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.05,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((62000, 10200, 51800, 83.54838709677419)),
                &hourly_burn,
                &current_live_turn,
                0,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_small_no_amai_thread_when_5h_kpi_is_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 88.2
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            },
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 10500,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.1,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((10500, 10500, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_stays_off_for_huge_no_amai_thread_when_exact_5h_kpi_is_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 94.07
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 85.0,
            "secondary_limit_remaining_percent": 5.0
        });
        assert!(
            super::client_turn_pressure_guard(
                &meter,
                Some((196009, 196009, 0, 0.0)),
                &hourly_burn,
                &current_live_turn,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_moderate_no_amai_thread_when_5h_kpi_is_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 84.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            },
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 19000,
            "latest_model_context_window": 258400,
            "context_used_percent": 7.4,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((19000, 19000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_early_no_amai_thread_when_5h_kpi_is_below_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 86.2
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            },
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 47000,
            "latest_model_context_window": 258400,
            "context_used_percent": 18.6,
            "primary_limit_remaining_percent": 94.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((47000, 47000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_huge_no_amai_thread_when_exact_5h_kpi_is_one_to_one()
    {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "one_to_one",
            "kpi_percent": 0.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 85.0,
            "secondary_limit_remaining_percent": 5.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((196009, 196009, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_keeps_critical_primary_limit_even_when_exact_5h_kpi_is_saving() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 14.07
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 18.0,
            "secondary_limit_remaining_percent": 5.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((196009, 196009, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn build_headline_prefers_active_agent_budget_average() {
        let snapshot = json!({
            "sla": {
                "summary": {
                    "pass": 19,
                    "alert": 0,
                    "critical": 0,
                    "unknown": 0
                }
            },
            "active_agent_budget": {
                "headline": {
                    "title": "Средний KPI активных агентов",
                    "value_text": "5ч KPI: экономия 40.00%",
                    "scope_label": "среднее по 2 активным агентам"
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "headline": {
                        "title": "global fallback",
                        "value_percent": 12.0,
                        "scope_label": "fallback"
                    }
                }
            }
        });
        let headline = super::build_headline(&snapshot, 1775039106398);
        assert_eq!(
            headline["token_title"].as_str(),
            Some("Средний KPI активных агентов")
        );
        assert_eq!(
            headline["token_value"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(headline["token_scope"].as_str(), Some(""));
    }

    #[test]
    fn live_summary_payload_keeps_headline_and_active_agent_card_on_one_surface() {
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
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
                        "latency_slices": [
                            {
                                "state": "mixed",
                                "sample_count": 3,
                                "current_latency_ms": 1.7,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.4,
                                "p99_latency_ms": 2.4,
                                "max_latency_ms": 2.4
                            }
                        ]
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Dashboard live summary poller keeps headline and top cards fresh",
                    "next_step": "Keep headline and hero card on one live surface.",
                    "last_command": "context pack",
                    "last_results_summary": "Найдено: документов 0, символов 0.",
                    "latest_decision_trace": null,
                    "active_files": [],
                    "recent_queries": ["dashboard live summary"],
                    "restore_confidence": "preliminary"
                }
            },
            "agent_scope_activity": {
                "client_recent_window_minutes": 30,
                "client_recent_thread_count": 2,
                "client_recent_threads": [],
                "active_now_count": 2,
                "active_now_scopes": [
                    {
                        "agent_scope": "amai::continuity::default",
                        "owner_thread_id": "thread-a",
                        "heartbeat_at_epoch_ms": 1774239285880u64
                    },
                    {
                        "agent_scope": "bug_bounty::continuity::default",
                        "owner_thread_id": "thread-b",
                        "heartbeat_at_epoch_ms": 1774239200000u64
                    }
                ],
                "recent_scope_window_hours": 24,
                "recent_scope_count": 2,
                "recent_scopes": [
                    {
                        "agent_scope": "amai::continuity::default",
                        "captured_at_epoch_ms": 1774239285880u64
                    },
                    {
                        "agent_scope": "bug_bounty::continuity::default",
                        "captured_at_epoch_ms": 1774239200000u64
                    }
                ]
            },
            "active_agent_budget": {
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

        let payload = build_live_summary_payload(&test_config(), &snapshot, "127.0.0.1:9464", 1000)
            .expect("payload");
        assert_eq!(
            payload["headline"]["token_value"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(
            payload["active_agent_card"]["value"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(
            payload["active_agent_card"]["presentation_variant"].as_str(),
            Some("active_agent_budget_grouped_v3")
        );
        let top_cards = payload["top_cards"].as_array().expect("top cards");
        assert_eq!(top_cards.len(), 2);
        assert_eq!(top_cards[0]["title"].as_str(), Some("Скорость ответа"));
        assert_eq!(top_cards[1]["title"].as_str(), Some("Текущая работа"));
    }

    #[test]
    fn current_session_budget_guard_surfaces_machine_readable_rotate_flags() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "019d4eb1-3e92-75e3-b22b-2bdf21f13885",
                    "client_turn_total_tokens": 140921,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 54.54,
                    "primary_limit_remaining_percent": 61.0,
                    "secondary_limit_remaining_percent": 88.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                "current_live_turn": {
                    "status": "no_amai_activity_in_current_live_turn",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "019d4eb1-3e92-75e3-b22b-2bdf21f13885",
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(
            guard["source"],
            json!("dashboard_current_session_budget_guard_v2")
        );
        assert_eq!(guard["full_turn_savings_proven"], json!(false));
        assert_eq!(guard["should_rotate_chat_now"], json!(true));
        assert_eq!(guard["should_rotate_chat_soon"], json!(true));
        assert_eq!(guard["status_label"], json!("сожми текущий чат сейчас"));
        assert_eq!(
            guard["reply_execution_gate"]["gate_version"],
            json!("client-reply-budget-gate-v1")
        );
        assert_eq!(guard["reply_execution_gate"]["blocking"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["must_rotate_before_reply"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("compact_current_thread_for_client_budget")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["contract_version"],
            json!(working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["response_kind"],
            json!(working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["max_sentences"],
            json!(working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["bundle_version"],
            json!("rotate-chat-action-bundle-v1")
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["run_continuity_startup"]["project"],
            json!("amai")
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["recommended_handoff"]["headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["operator_flow"]["copy_paste_ready"],
            json!(true)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("same_thread_host_control_launch_command")
        );
        assert!(
            guard["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert!(
            guard["reply_execution_gate"]["action_bundle"]["operator_flow"]["rotate_helper_command"]
                .as_str()
                .unwrap_or_default()
                .contains("rotate-chat")
        );
        assert_eq!(guard["max_guard_age_seconds"], json!(10));
        assert_eq!(guard["observed_at_epoch_ms"], json!(1774622949000u64));
        assert!(
            guard["last_request"]
                .as_str()
                .unwrap_or_default()
                .contains("140921 из 258400, остаётся 45.46%")
        );
        assert!(
            guard["last_request"]
                .as_str()
                .unwrap_or_default()
                .contains("raw")
        );
        assert!(
            guard["client_limits"]
                .as_str()
                .unwrap_or_default()
                .contains("5ч остаётся 61.00%, 7д остаётся 88.00%")
        );
        assert!(
            guard["client_limits"]
                .as_str()
                .unwrap_or_default()
                .contains("raw")
        );
        assert!(
            guard["tracked_slice"]
                .as_str()
                .unwrap_or_default()
                .contains("без Amai 240, с Amai 106, экономия 134")
        );
        assert!(
            guard["next_action"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
        assert_eq!(guard["client_live_meter_current_thread_bound"], json!(true));
        assert_eq!(
            guard["client_live_meter_thread_binding_state"],
            json!("current_thread_bound")
        );
    }

    #[test]
    fn client_turn_pressure_display_status_label_prefers_same_thread_copy() {
        assert_eq!(
            super::client_turn_pressure_display_status_label("новый чат нужен сейчас", true),
            "сожми текущий чат сейчас"
        );
        assert_eq!(
            super::client_turn_pressure_display_status_label("новый чат рекомендован", true),
            "сожми текущий чат"
        );
        assert_eq!(
            super::client_turn_pressure_display_status_label("реальная экономия не доказана", true),
            "реальная экономия не доказана"
        );
    }

    #[test]
    fn selected_host_current_thread_control_state_prefers_same_thread_surface_when_thread_is_available_even_in_inactive_stage()
     {
        let thread_id = "019d4eb1-3e92-75e3-b22b-2bdf21f13885";
        let report = json!({
            "client_live_meter": {
                "status": "observed",
                "thread_id": thread_id,
                "client_turn_total_tokens": 91234,
                "latest_model_context_window": 258400,
                "current_thread_bound": true
            }
        });
        let restore_context = json!({});
        let host_context_compaction = json!({
            "stage": "inactive"
        });
        let (surface, _effect, preferred) = super::selected_host_current_thread_control_state(
            &report,
            &restore_context,
            &report["client_live_meter"],
            &host_context_compaction,
        );
        assert_eq!(surface["available"], json!(true));
        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
        );
        assert!(preferred);
    }

    #[test]
    fn current_session_budget_guard_ignores_foreign_thread_feedback_for_same_thread_confirmation() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_budget_target_percent": 50,
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 216,
                        "verified_effective_savings_pct": 76.86,
                        "started_at_epoch_ms": 1774984250772u64,
                        "ended_at_epoch_ms": 1774984250772u64,
                        "verified_baseline_tokens": 281,
                        "verified_observed_whole_cycle_with_amai_tokens": 69
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_observed_whole_cycle_with_amai_tokens": 69,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": {
                                    "exact_pair_available": true,
                                    "state": "exact_pair_materialized",
                                    "blockers": []
                                },
                                "strict_client_meter_slice": {"lower_bound_tokens": 285},
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        }
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "classification": "overspend",
                        "reply_prefix": "5ч KPI: переплата 100.82%",
                        "kpi_percent": 100.81540973161394,
                        "latest_observed_at_epoch_ms": 1774984496711u64,
                        "projected_primary_used_per_hour_percent": 40.163081946322826,
                        "remaining_window_minutes": 286.5548166666667,
                        "actual_remaining_percent": 91.0,
                        "ideal_remaining_percent": 95.51827222222222,
                        "projected_reset_delta_minutes": -150.60907407407424
                    },
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "thread_id": "019d4549-89d6-7640-a6e3-589979f08d20",
                        "current_thread_bound": true,
                        "client_turn_total_tokens": 150940,
                        "latest_model_context_window": 258400,
                        "context_used_percent": 58.413312693498455,
                        "primary_limit_remaining_percent": 93,
                        "primary_limit_used_percent": 7,
                        "secondary_limit_remaining_percent": 83,
                        "secondary_limit_used_percent": 17,
                        "started_at_epoch_ms": 1774984228000u64,
                        "ended_at_epoch_ms": 1774984490000u64,
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "source": "codex_app_server_account_rate_limits_read_v1",
                            "status_bar_correlated": true,
                            "observed_at_epoch_ms": 1774984496711u64,
                            "primary_limit_used_percent": 9.0,
                            "primary_limit_remaining_percent": 91.0,
                            "secondary_limit_used_percent": 3.0,
                            "secondary_limit_remaining_percent": 97.0
                        }
                    },
                    "current_live_turn": {
                        "status": "exact_pair_materialized",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 151437,
                            "with_amai_tokens": 150940,
                            "saved_tokens": 497,
                            "saved_pct": 0.3281892800306398
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity"
                    },
                    "client_budget_target_percent": 50,
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "same-thread ctl-launch now materializes fresh thread-bound budget surfaces immediately",
                    "next_step": "First continue from a fresh chat via continuity startup to satisfy the required return task, then continue reducing remaining current-session/live-turn cost and giant-thread burn.",
                    "thread_id": "",
                    "recent_actions": [{
                        "source_kind": "host_current_thread_control_feedback",
                        "recorded_at_epoch_ms": 1774983785445u64,
                        "summary": "Requested same-thread compact window launch via host current-thread control.",
                        "host_current_thread_control_feedback": {
                            "feedback_kind": "requested",
                            "command_id": "hotkey-window-open-current",
                            "feedback_snapshot": {
                                "thread_id": "019d38ab-7c35-7553-b1c0-ae83c5eabf3f",
                                "client_live_meter": {
                                    "client_turn_total_tokens": 186324,
                                    "context_used_percent": 72.10681114551083,
                                    "primary_limit_used_percent": 0
                                },
                                "host_context_compaction": {
                                    "compacted_at_epoch_ms": 1774981093000u64,
                                    "compaction_count": 74,
                                    "growth_since_compaction_tokens": 97713,
                                    "stage": null
                                }
                            }
                        }
                    }]
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);

        assert_ne!(
            guard["reply_execution_gate"]["action_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["feedback_pending"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["effect_verdict"],
            json!("other_thread")
        );
    }

    #[test]
    fn current_session_budget_guard_keeps_rotate_soon_as_advisory_only() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 320,
                    "verified_effective_savings_pct": 49.0,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 65320,
                    "verified_observed_whole_cycle_with_amai_tokens": 65000
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 65000,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 65320},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "019d4eb1-3e92-75e3-b22b-2bdf21f13885",
                    "client_turn_total_tokens": 65000,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 25.15,
                    "primary_limit_remaining_percent": 94.0,
                    "secondary_limit_remaining_percent": 97.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(true));
        assert_eq!(guard["status_label"], json!("сожми текущий чат"));
        assert_eq!(guard["reply_execution_gate"]["blocking"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["must_rotate_before_reply"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("compact_current_thread_for_client_budget")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_contract"]["contract_version"],
            json!(working_state::CLIENT_REPLY_BUDGET_CONTRACT_VERSION)
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_contract"]["must_avoid_unrequested_recaps"],
            json!(true)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["preserves_return_obligation"],
            json!(true)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["bundle_version"],
            json!("rotate-chat-action-bundle-v1")
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("same_thread_host_control_launch_command")
        );
    }

    #[test]
    fn current_session_budget_guard_uses_compact_mode_for_thread_bound_one_to_one_hourly_burn() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "thread-current",
                    "client_turn_total_tokens": 30240,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 11.70,
                    "primary_limit_remaining_percent": 90.0,
                    "secondary_limit_remaining_percent": 95.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                "client_limit_hourly_burn": {
                    "status": "observed",
                    "classification": "one_to_one",
                    "kpi_percent": 0.41,
                    "reply_prefix": "5ч KPI: 1:1"
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("continue_current_chat")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(guard["reply_prefix"], json!("5ч KPI: 1:1"));
        assert_eq!(
            guard["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: 1:1")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_contract"]["must_avoid_unrequested_recaps"],
            json!(true)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["preserves_return_obligation"],
            json!(true)
        );
        assert!(guard["reply_execution_gate"]["action_bundle"].is_null());
    }

    #[test]
    fn current_session_budget_guard_prefers_live_personal_agent_reply_prefix() {
        let snapshot = json!({
        "active_agent_budget": {
            "aggregate": {
                "status": "observed",
                "reply_prefix": "5ч KPI: экономия 28.49%"
            }
        },
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "thread-current",
                    "client_turn_total_tokens": 30240,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 11.70,
                    "primary_limit_remaining_percent": 90.0,
                    "secondary_limit_remaining_percent": 95.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                "personal_agent_kpi": {
                    "status": "observed",
                    "reply_prefix": "5ч KPI: экономия 61.25%"
                },
                "client_limit_hourly_burn": {
                    "status": "observed",
                    "classification": "one_to_one",
                    "kpi_percent": 0.41,
                    "reply_prefix": "5ч KPI: 1:1"
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["reply_prefix"], json!("5ч KPI: 1:1"));
        assert_eq!(guard["global_reply_prefix"], json!("5ч KPI: 1:1"));
        assert_eq!(
            guard["reply_prefix_source"],
            json!("global_client_limit_hourly_burn")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: 1:1")
        );
    }

    #[test]
    fn current_session_budget_guard_marks_online_personal_kpi_source() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "reply_prefix": "5ч KPI: экономия 10.00%"
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 138,
                        "verified_effective_savings_pct": 56.56
                    },
                    "rolling_window": {"events_total": 0, "counted_events": 0},
                    "lifetime": {"events_total": 0, "counted_events": 0},
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": {"exact_pair_available": true},
                                "strict_client_meter_slice": {"lower_bound_tokens": 240},
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        }
                    },
                    "statement_export_previews": {"lifetime": {}},
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "thread-current",
                        "client_turn_total_tokens": 30240,
                        "latest_model_context_window": 258400,
                        "context_used_percent": 11.70,
                        "primary_limit_remaining_percent": 90.0,
                        "secondary_limit_remaining_percent": 95.0,
                        "started_at_epoch_ms": 1774622174000u64,
                        "ended_at_epoch_ms": 1774622949000u64
                    },
                    "personal_agent_kpi": {
                        "status": "observed",
                        "confidence": "online_limit_contour",
                        "reply_prefix": "5ч KPI: экономия 78.12%"
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "classification": "one_to_one",
                        "kpi_percent": 0.41,
                        "reply_prefix": "5ч KPI: 1:1"
                    },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    }
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["reply_prefix"], json!("5ч KPI: экономия 78.12%"));
        assert_eq!(
            guard["reply_prefix_source"],
            json!("personal_agent_online_limit_contour")
        );
    }

    #[test]
    fn current_session_budget_guard_uses_compact_mode_for_saving_below_target_hourly_burn() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "thread-current",
                    "client_turn_total_tokens": 30240,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 11.70,
                    "primary_limit_remaining_percent": 90.0,
                    "secondary_limit_remaining_percent": 95.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                "client_limit_hourly_burn": {
                    "status": "observed",
                    "classification": "saving",
                    "kpi_percent": 12.37
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("continue_current_chat")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["preserves_return_obligation"],
            json!(true)
        );
        assert!(guard["reply_execution_gate"]["action_bundle"].is_null());
    }

    #[test]
    fn current_session_budget_guard_uses_compact_mode_for_live_turn_below_target_even_when_hourly_kpi_is_healthy()
     {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "thread-current",
                    "client_turn_total_tokens": 30240,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 11.70,
                    "primary_limit_remaining_percent": 90.0,
                    "secondary_limit_remaining_percent": 95.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                "current_live_turn": {
                    "status": "exact_pair_materialized",
                    "exact_pair_available": true,
                    "exact_pair": {
                        "without_amai_tokens": 30479,
                        "with_amai_tokens": 30240,
                        "saved_tokens": 239,
                        "saved_pct": 0.7834896158010433
                    }
                },
                "client_limit_hourly_burn": {
                    "status": "observed",
                    "classification": "saving",
                    "kpi_percent": 95.0
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("continue_current_chat")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["preserves_return_obligation"],
            json!(true)
        );
        assert!(guard["reply_execution_gate"]["action_bundle"].is_null());
    }

    #[test]
    fn current_session_budget_guard_keeps_normal_mode_when_saving_above_target() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "current_thread_bound",
                    "current_thread_bound": true,
                    "thread_id": "thread-current",
                    "client_turn_total_tokens": 2000,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 0.77,
                    "primary_limit_remaining_percent": 90.0,
                    "secondary_limit_remaining_percent": 95.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                "current_live_turn": {
                    "status": "exact_pair_materialized",
                    "exact_pair_available": true,
                    "exact_pair": {
                        "without_amai_tokens": 30240,
                        "with_amai_tokens": 2000,
                        "saved_tokens": 28240,
                        "saved_pct": 93.38624338624338
                    }
                },
                "client_limit_hourly_burn": {
                    "status": "observed",
                    "classification": "saving",
                    "kpi_percent": 95.0
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("continue_current_chat")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_NORMAL)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["preserves_return_obligation"],
            json!(true)
        );
        assert!(guard["reply_execution_gate"]["action_bundle"].is_null());
    }

    #[test]
    fn current_session_budget_guard_ignores_unbound_previous_thread_meter() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "no_current_thread_binding",
                    "current_thread_bound": false,
                    "thread_id": "thread-previous",
                    "client_turn_total_tokens": 140921,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 54.54,
                    "primary_limit_remaining_percent": 61.0,
                    "secondary_limit_remaining_percent": 88.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("continue_current_chat")
        );
        assert_eq!(
            guard["reply_execution_gate"]["reply_budget_mode"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_NORMAL)
        );
        assert_eq!(
            guard["reply_execution_gate"]["must_rotate_before_reply"],
            json!(false)
        );
        assert_eq!(guard["last_request"], Value::Null);
        assert!(
            guard["client_limits"]
                .as_str()
                .unwrap_or_default()
                .contains("5ч остаётся 61.00%, 7д остаётся 88.00%")
        );
        assert!(
            guard["client_limits"]
                .as_str()
                .unwrap_or_default()
                .contains("latest observed")
        );
        assert_eq!(guard["observed_at_epoch_ms"], Value::Null);
        assert_eq!(
            guard["client_live_meter_current_thread_bound"],
            json!(false)
        );
        assert_eq!(
            guard["client_live_meter_thread_binding_state"],
            json!("no_current_thread_binding")
        );
        assert_eq!(
            guard["global_client_limit_source"]["source_kind"],
            json!(working_state::GLOBAL_CLIENT_LIMIT_SOURCE_KIND)
        );
        assert_eq!(
            guard["global_client_limit_source"]["truly_global_source_materialized"],
            json!(false)
        );
        assert!(
            guard["global_client_limit_source"]["summary"]
                .as_str()
                .unwrap_or_default()
                .contains("последнее observed значение client limits")
        );
    }

    #[test]
    fn current_session_budget_guard_blocks_global_budget_exhaustion_without_thread_binding() {
        let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": {
                "current_session": {
                    "events_total": 1,
                    "counted_events": 1,
                    "verified_effective_saved_tokens": 138,
                    "verified_effective_savings_pct": 56.56,
                    "started_at_epoch_ms": 1774622516860u64,
                    "ended_at_epoch_ms": 1774622516860u64,
                    "verified_baseline_tokens": 240,
                    "verified_observed_whole_cycle_with_amai_tokens": 106
                },
                "rolling_window": {"events_total": 0, "counted_events": 0},
                "lifetime": {"events_total": 0, "counted_events": 0},
                "statement_previews": {
                    "current_session": {
                        "verified_observed_whole_cycle_with_amai_tokens": 106,
                        "client_limit_meter_alignment": {
                            "same_meter_as_client_limit": true,
                            "exact_pair_status": {"exact_pair_available": true},
                            "strict_client_meter_slice": {"lower_bound_tokens": 240},
                            "explicit_boundary_surface": {
                                "blocks_full_same_meter_equivalence": false
                            }
                        }
                    },
                    "rolling_window": {},
                    "lifetime": {}
                },
                "statement_export_previews": {"lifetime": {}},
                "client_live_meter": {
                    "status": "observed",
                    "thread_binding_state": "no_current_thread_binding",
                    "current_thread_bound": false,
                    "thread_id": "thread-previous",
                    "client_turn_total_tokens": 140921,
                    "latest_model_context_window": 258400,
                    "context_used_percent": 54.54,
                    "primary_limit_remaining_percent": 5.0,
                    "secondary_limit_remaining_percent": 71.0,
                    "started_at_epoch_ms": 1774622174000u64,
                    "ended_at_epoch_ms": 1774622949000u64
                },
                    "profile": {"display_name": "Обычная рабочая машина"}
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "display_name": "Amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity",
                        "display_name": "Continuity"
                    },
                    "execctl_resume_state": "pending_return_queue_present",
                    "current_goal": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                }
            }
        });

        let guard = super::current_session_budget_guard(&snapshot);
        assert_eq!(guard["should_rotate_chat_now"], json!(false));
        assert_eq!(guard["should_rotate_chat_soon"], json!(false));
        assert_eq!(
            guard["requires_global_budget_recovery_before_reply"],
            json!(true)
        );
        assert_eq!(
            guard["status_label"],
            json!("глобальный лимит клиента почти исчерпан")
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_kind"],
            json!("wait_for_global_client_budget_recovery")
        );
        assert_eq!(guard["reply_execution_gate"]["blocking"], json!(false));
        assert_eq!(
            guard["reply_execution_gate"]["must_rotate_before_reply"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["blocking_reply_contract"]["active"],
            json!(false)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["bundle_version"],
            json!("wait-client-budget-action-bundle-v1")
        );
        assert_eq!(
            guard["global_client_limit_source"]["source_kind"],
            json!(working_state::GLOBAL_CLIENT_LIMIT_SOURCE_KIND)
        );
        assert_eq!(
            guard["reply_execution_gate"]["action_bundle"]["budget_source"]["source_kind"],
            json!(working_state::GLOBAL_CLIENT_LIMIT_SOURCE_KIND)
        );
        assert_eq!(guard["last_request"], Value::Null);
        assert!(
            guard["client_limits"]
                .as_str()
                .unwrap_or_default()
                .contains("latest observed")
        );
        assert_eq!(guard["observed_at_epoch_ms"], json!(1774622949000u64));
    }

    #[test]
    fn reviewed_frozen_debt_export_metric_row_surfaces_report_only_path() {
        let alignment = json!({
            "reviewed_frozen_debt_export_surface": {
                "export_ready_report_only": true,
                "surface_kind": "reviewed_frozen_debt_report_only",
                "blocking_component": "tool_overhead_outside_retrieval",
                "irrecoverable_missing_live_events": 13,
                "allowed_claims": [
                    "reviewed_frozen_debt_report_only",
                    "historical_source_loss_disclosed_non_exact"
                ],
                "forbidden_claims": [
                    "claim_raw_exact_history",
                    "claim_exact_same_meter_pair_materialized"
                ],
                "propagated_surfaces": [
                    "statement_export_preview",
                    "settlement_report_preview",
                    "contractual_evidence_pack"
                ],
                "review_bundle_command": "./scripts/amai_exec.sh observe token-statement-export --scope lifetime",
                "evidence_pack_command": "./scripts/amai_exec.sh observe token-evidence-pack --scope lifetime"
            }
        });

        let row = super::reviewed_frozen_debt_export_metric_row(&alignment).expect("export row");
        assert_eq!(row["label"], "Review-only export");
        assert_eq!(
            row["value"].as_str(),
            Some("reviewed_frozen_debt_report_only: 13 irrecoverable rows")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("claim_raw_exact_history")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("token-statement-export --scope lifetime")
        );
    }

    #[test]
    fn build_links_groups_api_and_monitoring_entries() {
        let links = build_links("http://127.0.0.1:9464");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0]["label"].as_str(), Some(""));
        assert_eq!(
            links[0]["items"].as_array().map(|items| items.len()),
            Some(4)
        );
        assert_eq!(links[1]["label"].as_str(), Some(""));
        assert_eq!(links[1]["note"].as_str(), Some(""));
        assert_eq!(
            links[1]["items"].as_array().map(|items| items.len()),
            Some(2)
        );
    }

    #[test]
    fn machine_cards_include_artifact_cleanup_visibility() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 0,
                "policy_retained_targets": [],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "expired": 0,
                "kept_latest": 3,
                "protected": 1,
                "targets_scanned": 7,
                "aggressive_preview_selected": 4,
                "aggressive_preview_reclaimable_bytes": 35_604_527_338u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 30,
                    "reclaimed_bytes": 50_424_092_586u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 230_200_000_000u64,
                    "cleanup_scope_bytes": 29_960_520_424u64,
                    "out_of_policy_bytes": 200_239_479_576u64,
                    "unmanaged_alert_triggered": true,
                    "large_unmanaged_roots": [
                        {
                            "path": "output/windows-vm-lab",
                            "unmanaged_bytes": 199_715_979_264u64
                        }
                    ],
                    "manual_only_targets": [
                        {
                            "path": "output/windows-vm-lab",
                            "ttl_hours": 168,
                            "keep_latest": 2,
                            "total_bytes": 199_715_979_264u64
                        }
                    ],
                    "unreadable_paths_count": 1
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("alert"));
        assert_eq!(
            cleanup_card["value"].as_str(),
            Some("186.49 GiB вне policy")
        );
        assert_eq!(
            cleanup_card["rows"][0]["value"].as_str(),
            Some("214.39 GiB")
        );
        assert_eq!(cleanup_card["rows"][1]["value"].as_str(), Some("27.90 GiB"));
        assert_eq!(
            cleanup_card["rows"][2]["value"].as_str(),
            Some("186.49 GiB")
        );
        assert_eq!(cleanup_card["rows"][4]["value"].as_str(), Some("33.16 GiB"));
        assert_eq!(
            cleanup_card["rows"][7]["value"].as_str(),
            Some("46.96 GiB (30, aggressive)")
        );
        assert_eq!(
            cleanup_card["rows"][11]["value"].as_str(),
            Some("output/windows-vm-lab (186.00 GiB)")
        );
        assert_eq!(
            cleanup_card["rows"][12]["value"].as_str(),
            Some("output/windows-vm-lab (186.00 GiB, ttl 168h, keep_latest 2)")
        );
    }

    #[test]
    fn artifact_cleanup_warning_surfaces_large_unmanaged_root() {
        let snapshot = json!({
            "artifact_cleanup": {
                "selected_reclaimable_bytes": 0,
                "aggressive_preview_reclaimable_bytes": 0,
                "repo_inventory": {
                    "out_of_policy_bytes": 200_239_479_576u64,
                    "unmanaged_alert_triggered": true,
                    "large_unmanaged_roots": [
                        {
                            "path": "output/windows-vm-lab",
                            "unmanaged_bytes": 199_715_979_264u64
                        }
                    ],
                    "manual_only_targets": [
                        {
                            "path": "output/windows-vm-lab"
                        }
                    ]
                }
            }
        });
        let warning = artifact_cleanup_warning(&snapshot, None).expect("warning");
        assert!(warning.contains("вне cleanup policy"));
        assert!(warning.contains("output/windows-vm-lab"));
        assert!(
            warning.contains("observe cleanup-artifacts --target output/windows-vm-lab --apply")
        );
    }

    #[test]
    fn artifact_cleanup_card_surfaces_policy_retained_hot_storage_as_waiting() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 18_460_613_632u64,
                "policy_retained_targets": [
                    {
                        "path": "target/debug",
                        "ttl_hours": 168,
                        "keep_latest": 3,
                        "aggressive_preview_reclaimable_bytes": 16_254_702_590u64
                    }
                ],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "expired": 0,
                "kept_latest": 13,
                "protected": 0,
                "targets_scanned": 8,
                "aggressive_preview_selected": 19,
                "aggressive_preview_reclaimable_bytes": 32_577_450_367u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 1,
                    "reclaimed_bytes": 28_888_311_035u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 35_728_482_155u64,
                    "cleanup_scope_bytes": 32_698_373_188u64,
                    "out_of_policy_bytes": 3_030_108_967u64,
                    "unmanaged_alert_triggered": false,
                    "large_unmanaged_roots": [],
                    "manual_only_targets": [
                        {
                            "path": "output/windows-vm-lab",
                            "ttl_hours": 24,
                            "keep_latest": 2,
                            "total_bytes": 15_079_381u64
                        }
                    ],
                    "unreadable_paths_count": 1
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("waiting"));
        assert_eq!(cleanup_card["value"].as_str(), Some("17.19 GiB ждёт TTL"));
        let operator_row = cleanup_card["rows"]
            .as_array()
            .expect("cleanup rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Operator reclaim next"))
            .expect("operator reclaim row");
        assert!(
            operator_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("observe cleanup-artifacts --target target/debug --aggressive --apply")
        );
        let warning = artifact_cleanup_warning(&snapshot, None).expect("warning");
        assert!(warning.contains("policy-covered"));
        assert!(warning.contains("TTL/keep-latest"));
        assert!(warning.contains("target/debug"));
        assert!(warning.contains("--aggressive --apply"));
    }

    #[test]
    fn artifact_cleanup_card_escalates_policy_retained_hot_storage_under_disk_pressure() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 18_460_613_632u64,
                "policy_retained_targets": [
                    {
                        "path": "target/debug",
                        "ttl_hours": 168,
                        "keep_latest": 3,
                        "aggressive_preview_reclaimable_bytes": 16_254_702_590u64
                    }
                ],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "disk_pressure_thresholds": {
                    "alert_used_percent": 85.0,
                    "critical_used_percent": 92.0,
                    "alert_available_gib": 150.0,
                    "critical_available_gib": 60.0
                },
                "expired": 0,
                "kept_latest": 13,
                "protected": 0,
                "targets_scanned": 8,
                "aggressive_preview_selected": 19,
                "aggressive_preview_reclaimable_bytes": 32_577_450_367u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 1,
                    "reclaimed_bytes": 28_888_311_035u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 35_728_482_155u64,
                    "cleanup_scope_bytes": 32_698_373_188u64,
                    "out_of_policy_bytes": 3_030_108_967u64,
                    "unmanaged_alert_triggered": false,
                    "large_unmanaged_roots": [],
                    "manual_only_targets": [],
                    "unreadable_paths_count": 1
                }
            }
        });
        let machine = synthetic_machine_summary(48.0, Some(94.0));
        let cards = build_machine_cards(&snapshot, Some(&machine), None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("critical"));
        let warning = artifact_cleanup_warning(&snapshot, Some(&machine)).expect("warning");
        assert!(warning.contains("давление"));
        assert!(warning.contains("target/debug"));
        assert!(warning.contains("--aggressive --apply"));
    }

    #[test]
    fn artifact_cleanup_card_surfaces_unreadable_samples_as_best_effort_note() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 18_460_613_632u64,
                "policy_retained_targets": [
                    {
                        "path": "target/debug",
                        "ttl_hours": 168,
                        "keep_latest": 3,
                        "aggressive_preview_reclaimable_bytes": 16_254_702_590u64
                    }
                ],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "expired": 0,
                "kept_latest": 13,
                "protected": 0,
                "targets_scanned": 8,
                "aggressive_preview_selected": 19,
                "aggressive_preview_reclaimable_bytes": 32_577_450_367u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 1,
                    "reclaimed_bytes": 28_888_311_035u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 35_728_482_155u64,
                    "cleanup_scope_bytes": 32_698_373_188u64,
                    "out_of_policy_bytes": 3_030_108_967u64,
                    "unmanaged_alert_triggered": false,
                    "large_unmanaged_roots": [],
                    "manual_only_targets": [],
                    "unreadable_paths_count": 1,
                    "unreadable_paths_sample": [
                        "/home/art/agent-memory-index/state/postgres/pgdata"
                    ]
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert!(
            cleanup_card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("best-effort lower bound")
        );
        let unreadable_row = cleanup_card["rows"]
            .as_array()
            .expect("cleanup rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Unreadable sample"))
            .expect("unreadable sample row");
        assert_eq!(
            unreadable_row["value"].as_str(),
            Some("/home/art/agent-memory-index/state/postgres/pgdata")
        );
    }

    #[test]
    fn governance_card_surfaces_forgetting_job_breakdown() {
        let snapshot = json!({
            "governance_surface": {
                "human_override_audit": {
                    "scope_override_events_total": 2,
                    "forgetting_audit_log_entries_total": 17
                },
                "wrong_link_rate": {
                    "open_conflict_count": 0
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 0
                },
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.125
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 7,
                    "cold_archive_job": 3,
                    "revalidation_job": 4,
                    "de_duplication_job": 2,
                    "summarization_job": 0
                }
            }
        });

        let card = build_governance_card(&snapshot);
        assert_eq!(card["title"], json!("Жизненный цикл памяти"));
        assert_eq!(card["status"], json!("pass"));
        assert_eq!(card["rows"][0]["value"], json!("7"));
        assert_eq!(card["rows"][1]["value"], json!("3"));
        assert_eq!(card["rows"][2]["value"], json!("4"));
        assert_eq!(card["rows"][3]["value"], json!("2"));
        assert_eq!(card["rows"][4]["value"], json!("0"));
    }

    #[test]
    fn governance_card_alert_headline_surfaces_quarantine_and_conflicts() {
        let snapshot = json!({
            "governance_surface": {
                "human_override_audit": {
                    "forgetting_audit_log_entries_total": 18
                },
                "wrong_link_rate": {
                    "open_conflict_count": 135
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 66,
                    "active_quarantine_breakdown": [
                        {
                            "quarantine_reason": "proof quarantine",
                            "entity_kind": "import_packet",
                            "source_kind": "import_packet_override",
                            "item_count": 60
                        }
                    ]
                },
                "open_conflict_breakdown": [
                    {
                        "summary": "truth conflict detected",
                        "source_kind": "verification_conflict_runtime",
                        "item_count": 120
                    }
                ],
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.0095
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 6,
                    "cold_archive_job": 6,
                    "revalidation_job": 6,
                    "de_duplication_job": 0,
                    "summarization_job": 0
                }
            }
        });

        let card = build_governance_card(&snapshot);
        assert_eq!(card["status"], json!("alert"));
        assert_eq!(card["value"], json!("66 в quarantine • 135 конфликтов"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("главный quarantine-класс")
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("главный конфликт")
        );
        assert_eq!(card["rows"][6]["label"], json!("Quarantine"));
        assert_eq!(card["rows"][6]["value"], json!("66"));
        assert_eq!(card["rows"][7]["label"], json!("Спорные"));
        assert_eq!(card["rows"][8]["label"], json!("Открытые конфликты"));
    }

    #[test]
    fn governance_card_uses_correct_russian_count_forms_in_alert_headline() {
        let snapshot = json!({
            "governance_surface": {
                "human_override_audit": {
                    "forgetting_audit_log_entries_total": 1
                },
                "wrong_link_rate": {
                    "open_conflict_count": 1
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 0,
                    "active_quarantine_breakdown": []
                },
                "open_conflict_breakdown": [
                    {
                        "summary": "cli get conflict 1",
                        "source_kind": "runtime_cli",
                        "item_count": 1
                    }
                ],
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.0
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 0,
                    "cold_archive_job": 0,
                    "revalidation_job": 0,
                    "de_duplication_job": 0,
                    "summarization_job": 0
                }
            }
        });

        let card = build_governance_card(&snapshot);
        assert_eq!(card["value"], json!("1 конфликт"));
    }

    #[test]
    fn service_cards_keep_only_live_operator_cards() {
        let snapshot = json!({
            "postgres": {
                "query_probe_p95_ms": 1.5,
                "connection_usage_ratio": 0.2,
                "replica_lag_seconds": 0.0,
                "deadlocks_delta": 0.0,
                "transactions_per_sec": 12.0,
                "wal_bytes_per_sec": 4096.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0,
                "memory_resident_bytes": 1024.0,
                "points_count": 10.0,
                "segments_count": 2.0
            },
            "nats": {
                "publish_probe_p95_ms": 1.0,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.1
            },
            "thresholds": {
                "postgres": {
                    "query_probe_p95_ms": { "target": 5.0 },
                    "connection_usage_ratio": { "target": 0.8 }
                },
                "qdrant": {
                    "optimize_queue": { "target": 0.0 },
                    "update_queue_length": { "target": 0.0 }
                },
                "nats": {
                    "publish_probe_p95_ms": { "target": 5.0 },
                    "consumer_lag_msgs": { "target": 0.0 },
                    "jetstream_disk_usage_ratio": { "target": 0.8 }
                }
            },
            "governance_surface": {
                "human_override_audit": {
                    "forgetting_audit_log_entries_total": 4
                },
                "wrong_link_rate": {
                    "open_conflict_count": 0
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 0
                },
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.05
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 1,
                    "cold_archive_job": 1,
                    "revalidation_job": 1,
                    "de_duplication_job": 1,
                    "summarization_job": 0
                }
            },
            "benchmark_external_summary": {}
        });

        let cards = build_service_cards(&snapshot);
        let titles: Vec<&str> = cards
            .iter()
            .filter_map(|card| card["title"].as_str())
            .collect();

        assert!(titles.contains(&"PostgreSQL"));
        assert!(titles.contains(&"Qdrant Amai live"));
        assert!(titles.contains(&"Qdrant внешнего бенча"));
        assert!(titles.contains(&"NATS / JetStream"));
        assert!(titles.contains(&"Жизненный цикл памяти"));
        assert!(!titles.contains(&"Поведение при сбоях"));
        assert!(!titles.contains(&"Правильное продолжение"));
    }

    #[test]
    fn benchmark_cards_name_lanes_explicitly() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "load_verification": {
                    "captured_at_epoch_ms": 1,
                    "project": "project_alpha",
                    "namespace": "review",
                    "query": "alpha_only_token",
                    "execution_mode": "hot_cache_only",
                    "qps": 1224682.0,
                    "p50_ms": 0.007,
                    "p95_ms": 0.010,
                    "p99_ms": 0.015,
                    "max_ms": 0.439,
                    "error_rate": 0.0,
                    "workers": 17,
                    "success_count": 10013,
                    "error_count": 0
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "captured_at_epoch_ms": 2,
                    "project": "project_alpha",
                    "namespace": "default",
                    "query": "alpha_runtime_summary",
                    "disable_cache": false,
                    "qps": 1661.13,
                    "p50_ms": 0.000211,
                    "p95_ms": 0.000271,
                    "p99_ms": 0.000280,
                    "max_ms": 0.000280,
                    "iterations": 20,
                    "warmup": 3
                }
            },
            "latest_cold_path_benchmark": {
                "cold_benchmark": {
                    "captured_at_epoch_ms": 3,
                    "executive_summary": { "verdict": "TARGET MET" },
                    "profile": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 12.0,
                        "target_p99_ms": 13.0,
                        "target_max_ms": 15.0,
                        "min_precision": 0.9,
                        "min_recall": 0.9,
                        "min_target_hit_rate": 0.9,
                        "min_sample_count": 100.0,
                        "min_repo_count": 75.0,
                        "min_query_slice_count": 200.0,
                        "max_duration_seconds": 120.0,
                        "max_leakage": 0.0,
                        "max_error_rate": 0.0
                    },
                    "machine_readable_summary": {
                        "p50": 1.0,
                        "p95": 2.0,
                        "p99": 3.0,
                        "max": 4.0,
                        "precision": 1.0,
                        "recall": 1.0,
                        "hit_rate": 1.0,
                        "sample_count": 1000,
                        "repo_count": 75,
                        "query_slice_count": 200,
                        "duration": 10.0,
                        "leakage": 0,
                        "error_rate": 0.0
                    }
                }
            },
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 4,
                    "cross_project_leakage": 0.0,
                    "symbol_precision": 1.0,
                    "semantic_precision": 1.0
                }
            },
            "latest_procedural_benchmark": {
                "captured_at_epoch_ms": 5,
                "procedural_benchmark": {
                    "benchmark_run_state": "dual_line_materialized",
                    "benchmark_run_state_ru": "обе benchmark-линии materialized",
                    "benchmark_metric_kind": "procedural_skill_metrics",
                    "benchmark_with_amai_series": [
                        { "metric_key": "reuse_quality", "value": 1.0 },
                        { "metric_key": "bad_skill_suppression", "value": 1.0 },
                        { "metric_key": "stale_skill_suppression", "value": 1.0 },
                        { "metric_key": "shadow_to_verified_uplift", "value": 1.0 },
                        { "metric_key": "evaluator_correctness", "value": 1.0 }
                    ],
                    "benchmark_without_amai_series": [
                        { "metric_key": "reuse_quality", "value": 0.0 },
                        { "metric_key": "bad_skill_suppression", "value": 1.0 },
                        { "metric_key": "stale_skill_suppression", "value": 1.0 },
                        { "metric_key": "shadow_to_verified_uplift", "value": 0.0 },
                        { "metric_key": "evaluator_correctness", "value": 1.0 }
                    ],
                    "benchmark_line_summaries": {
                        "with_amai": {
                            "line_code": "with_amai",
                            "state": "materialized",
                            "point_count": 5,
                            "pass_percent": 100.0
                        },
                        "without_amai_but_measuring": {
                            "line_code": "without_amai_but_measuring",
                            "state": "materialized",
                            "point_count": 5,
                            "pass_percent": 60.0,
                            "reason_ru": "Amai не помогает, но benchmark продолжает измерять procedural lane."
                        }
                    },
                    "benchmark_run_passport": {
                        "multi_platform_runtime_contract": "platform-neutral benchmark snapshot"
                    },
                    "summary": {
                        "total_metrics": 5,
                        "passed_metrics": 5,
                        "pass_percent": 100.0,
                        "without_amai_series_available": true
                    },
                    "procedural_metrics": [
                        {
                            "metric_key": "reuse_quality",
                            "label_ru": "Reuse quality",
                            "tooltip_ru": "Skill reuse quality",
                            "passed": true
                        },
                        {
                            "metric_key": "bad_skill_suppression",
                            "label_ru": "Bad-skill suppression",
                            "tooltip_ru": "Bad skill suppression",
                            "passed": true
                        },
                        {
                            "metric_key": "stale_skill_suppression",
                            "label_ru": "Stale-skill suppression",
                            "tooltip_ru": "Stale skill suppression",
                            "passed": true
                        },
                        {
                            "metric_key": "shadow_to_verified_uplift",
                            "label_ru": "Shadow-to-verified uplift",
                            "tooltip_ru": "Shadow uplift",
                            "passed": true
                        },
                        {
                            "metric_key": "evaluator_correctness",
                            "label_ru": "Evaluator correctness",
                            "tooltip_ru": "Evaluator correctness",
                            "passed": true
                        }
                    ]
                }
            },
            "latest_memory_benchmark_score": {
                "_observability": {
                    "captured_at_epoch_ms": 6
                },
                "memory_benchmark_score": {
                    "bench": "longmemeval",
                    "dataset": "longmemeval_s_cleaned",
                    "note": "Baseline scorer: exact/contains match + abstention heuristics. Official upstream scoring not yet implemented.",
                    "capability_breakdown": {
                        "longmemeval_overall_accuracy": 0.02,
                        "longmemeval_abstention_accuracy": 0.0,
                        "longmemeval_false_answer_rate_on_abstention": 1.0
                    },
                    "summary": {
                        "total": 500,
                        "missing_prediction": 490,
                        "abstention_expected": 32
                    }
                }
            },
            "procedural_benchmark_history": {
                "history_count": 2,
                "with_amai_history_count": 2,
                "without_amai_history_count": 2,
                "history_rows": [
                    {
                        "benchmark_run_id": "procedural-benchmark-1",
                        "captured_at_epoch_ms": 4,
                        "benchmark_run_state": "dual_line_materialized",
                        "with_amai_pass_percent": 80.0,
                        "without_amai_pass_percent": 40.0
                    },
                    {
                        "benchmark_run_id": "procedural-benchmark-2",
                        "captured_at_epoch_ms": 5,
                        "benchmark_run_state": "dual_line_materialized",
                        "with_amai_pass_percent": 100.0,
                        "without_amai_pass_percent": 60.0
                    }
                ],
                "with_amai_pass_percent_series": [
                    { "benchmark_run_id": "procedural-benchmark-1", "captured_at_epoch_ms": 4, "pass_percent": 80.0 },
                    { "benchmark_run_id": "procedural-benchmark-2", "captured_at_epoch_ms": 5, "pass_percent": 100.0 }
                ],
                "without_amai_pass_percent_series": [
                    { "benchmark_run_id": "procedural-benchmark-1", "captured_at_epoch_ms": 4, "pass_percent": 40.0 },
                    { "benchmark_run_id": "procedural-benchmark-2", "captured_at_epoch_ms": 5, "pass_percent": 60.0 }
                ]
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
                "load": {
                    "hot_qps": { "target": 1200000.0 },
                    "hot_error_rate": { "target": 0.0 },
                    "hot_benchmark_table": {
                        "target_p50_ms": 0.012,
                        "target_p95_ms": 0.015,
                        "target_p99_ms": 0.020,
                        "target_max_ms": 0.500,
                        "target_workers": 16.0,
                        "target_sample_count": 10000.0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0
                    },
                    "hot_benchmark_table": {
                        "target_iterations": 20.0,
                        "target_warmup": 3.0
                    }
                },
                "accuracy": {
                    "symbol_precision": { "target": 0.99 },
                    "semantic_precision": { "target": 0.98 }
                }
            },
            "sla": {
                "checks": [
                    { "metric": "accuracy.cross_project_leakage", "status": "pass" },
                    { "metric": "accuracy.symbol_precision", "status": "pass" },
                    { "metric": "accuracy.semantic_precision", "status": "pass" }
                ]
            }
        });

        let cards = build_benchmark_cards(&snapshot);
        let titles: Vec<&str> = cards
            .iter()
            .filter_map(|card| card["title"].as_str())
            .collect();
        assert_eq!(cards[0]["title"].as_str(), Some("Нагрузка после прогрева"));
        assert_eq!(cards[1]["title"].as_str(), Some("Повторный запрос"));
        assert!(
            cards[0]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Он не равен retrieval.hot_p95_ms")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("источник SLA-метрики retrieval.hot_p95_ms")
        );
        assert_eq!(cards[1]["headline_value"].as_str(), Some("271 ns"));
        assert_eq!(
            cards[0]["table"]["rows"][0]["values"][0].as_str(),
            Some("> 1200000\nBurst QPS")
        );
        assert_eq!(
            cards[0]["table"]["rows"][0]["values"][1].as_str(),
            Some("1224682\nBurst QPS")
        );
        assert_eq!(
            cards[0]["table"]["rows"][5]["values"][0].as_str(),
            Some("= 0.00%")
        );
        assert_eq!(
            cards[1]["table"]["rows"][0]["values"][0].as_str(),
            Some("нет SLA-порога")
        );
        assert_eq!(
            cards[1]["table"]["rows"][1]["values"][1].as_str(),
            Some("211 ns")
        );
        assert_eq!(
            cards[1]["table"]["rows"][2]["values"][1].as_str(),
            Some("271 ns")
        );
        assert_eq!(
            cards[1]["table"]["rows"][3]["values"][1].as_str(),
            Some("280 ns")
        );
        assert_eq!(
            cards[1]["table"]["rows"][4]["values"][1].as_str(),
            Some("280 ns")
        );
        assert_eq!(
            cards[1]["table"]["rows"][5]["values"][0].as_str(),
            Some(">= 20")
        );
        assert_eq!(
            cards[1]["table"]["rows"][6]["values"][0].as_str(),
            Some(">= 3")
        );
        assert_eq!(
            cards[2]["table"]["rows"][8]["values"][0].as_str(),
            Some(">= 75")
        );
        assert_eq!(
            cards[3]["table"]["rows"][1]["values"][0].as_str(),
            Some("99.00%")
        );
        assert_eq!(
            cards[3]["table"]["rows"][2]["values"][0].as_str(),
            Some("98.00%")
        );
        assert_eq!(
            cards[3]["headline_value"].as_str(),
            Some("утечки 0 • symbol 100.00% • semantic 100.00%")
        );
        assert_eq!(
            cards[3]["extra_class"].as_str(),
            Some("benchmark-span-full")
        );
        assert_eq!(cards[3]["table_orientation"].as_str(), Some("transposed"));
        assert_eq!(cards[4]["title"].as_str(), Some("Память и изоляция"));
        assert_eq!(cards[4]["status"].as_str(), Some("critical"));
        assert_eq!(
            cards[4]["headline_value"].as_str(),
            Some("500 кейсов • overall 2.00% • abstention 0.00%")
        );
        assert_eq!(
            cards[4]["table"]["rows"][0]["values"][1].as_str(),
            Some("longmemeval")
        );
        assert_eq!(
            cards[4]["table"]["rows"][2]["values"][1].as_str(),
            Some("500")
        );
        assert_eq!(
            cards[4]["table"]["rows"][5]["values"][1].as_str(),
            Some("100.00%")
        );
        assert_eq!(
            cards[4]["table"]["rows"][6]["values"][1].as_str(),
            Some("490")
        );
        assert!(
            cards[4]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Missing predictions")
        );
        assert_eq!(cards[5]["title"].as_str(), Some("Навыки и память действий"));
        assert_eq!(
            cards[5]["headline_value"].as_str(),
            Some(
                "5 из 5 skill-метрик подтверждены с Amai (100.00%); линия без Amai materialized отдельно"
            )
        );
        assert_eq!(cards[5]["status"].as_str(), Some("pass"));
        assert!(
            cards[5]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("generic memory score запрещён")
        );
        assert!(
            cards[5]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Линия без Amai materialized отдельно")
        );
        assert_eq!(
            cards[5]["benchmark_metric_kind"].as_str(),
            Some("procedural_skill_metrics")
        );
        assert_eq!(
            cards[5]["benchmark_run_state"].as_str(),
            Some("dual_line_materialized")
        );
        assert_eq!(
            cards[5]["without_amai_series_available"].as_bool(),
            Some(true)
        );
        assert_eq!(
            cards[5]["table"]["rows"][0]["label"].as_str(),
            Some("Metric kind")
        );
        assert_eq!(
            cards[5]["table"]["rows"][0]["values"][1].as_str(),
            Some("procedural_skill_metrics")
        );
        assert_eq!(
            cards[5]["table"]["rows"][1]["values"][1].as_str(),
            Some("dual_line_materialized (обе benchmark-линии materialized)")
        );
        assert_eq!(
            cards[5]["table"]["rows"][3]["values"][1].as_str(),
            Some("materialized")
        );
        assert_eq!(
            cards[5]["table"]["rows"][4]["values"][1].as_str(),
            Some("5")
        );
        assert_eq!(
            cards[5]["table"]["rows"][5]["values"][1].as_str(),
            Some("materialized")
        );
        assert_eq!(
            cards[5]["table"]["rows"][6]["values"][1].as_str(),
            Some("platform-neutral benchmark snapshot")
        );
        assert_eq!(
            cards[5]["table"]["rows"][7]["label"].as_str(),
            Some("История benchmark")
        );
        assert_eq!(
            cards[5]["table"]["rows"][7]["values"][1].as_str(),
            Some("2")
        );
        assert_eq!(
            cards[5]["table"]["rows"][8]["values"][1].as_str(),
            Some("2")
        );
        assert_eq!(
            cards[5]["table"]["rows"][9]["values"][1].as_str(),
            Some("2")
        );
        assert_eq!(
            cards[5]["table"]["rows"][10]["label"].as_str(),
            Some("Reuse quality")
        );
        assert_eq!(
            cards[5]["table"]["rows"][14]["values"][1].as_str(),
            Some("pass")
        );
        assert_eq!(
            cards[5]["benchmark_with_amai_history_series"]
                .as_array()
                .map(|items| items.len()),
            Some(2)
        );
        assert_eq!(
            cards[5]["benchmark_without_amai_history_series"]
                .as_array()
                .map(|items| items.len()),
            Some(2)
        );
        assert!(titles.contains(&"Память и изоляция"));
    }

    #[test]
    fn cold_benchmark_card_switches_to_live_progress_when_run_is_active() {
        let snapshot = json!({
            "captured_at_epoch_ms": 120_000u64,
            "cold_path_benchmark_progress": {
                "cold_benchmark_progress": {
                    "state": "running",
                    "captured_at_epoch_ms": 10,
                    "started_at_epoch_ms": 0,
                    "phase": "running",
                    "progress": {
                        "completed_case_count": 128,
                        "target_case_count": 442,
                        "current_repo_indexed_files": 512,
                        "current_repo_target_files": 800
                    },
                    "current_repo_code": "amai",
                    "current_repo_display_name": "Amai",
                    "profile": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 5.0,
                        "target_p99_ms": 10.0,
                        "target_max_ms": 15.0,
                        "min_precision": 0.997,
                        "min_recall": 0.997,
                        "min_target_hit_rate": 0.997,
                        "min_sample_count": 1000.0,
                        "min_repo_count": 75.0,
                        "min_query_slice_count": 200.0,
                        "max_duration_seconds": 10.0,
                        "max_leakage": 0.0,
                        "max_error_rate": 0.0
                    },
                    "machine_readable_summary": {
                        "p50": 1.345,
                        "p95": 1.777,
                        "p99": 2.307,
                        "max": 6.529,
                        "precision": 1.0,
                        "recall": 1.0,
                        "hit_rate": 1.0,
                        "sample_count": 128,
                        "repo_count": 32,
                        "query_slice_count": 64,
                        "duration": 9.5,
                        "run_wall_clock_duration": 312.0,
                        "leakage": 0,
                        "error_rate": 0.0
                    }
                }
            },
            "latest_retrieval_load_hot": {
                "load_verification": { "success_count": 0, "error_count": 0 }
            },
            "latest_retrieval_hot": {
                "benchmark": {}
            },
            "latest_cold_path_benchmark": {
                "cold_benchmark": {
                    "captured_at_epoch_ms": 3,
                    "executive_summary": { "verdict": "NOT MET" },
                    "profile": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 5.0,
                        "target_p99_ms": 10.0,
                        "target_max_ms": 15.0,
                        "min_precision": 0.997,
                        "min_recall": 0.997,
                        "min_target_hit_rate": 0.997,
                        "min_sample_count": 1000.0,
                        "min_repo_count": 75.0,
                        "min_query_slice_count": 200.0,
                        "max_duration_seconds": 10.0,
                        "max_leakage": 0.0,
                        "max_error_rate": 0.0
                    },
                    "machine_readable_summary": {
                        "p50": 9.0,
                        "p95": 11.0,
                        "p99": 13.0,
                        "max": 18.0,
                        "precision": 0.5,
                        "recall": 0.5,
                        "hit_rate": 0.5,
                        "sample_count": 9,
                        "repo_count": 4,
                        "query_slice_count": 9,
                        "duration": 999.0,
                        "leakage": 1,
                        "error_rate": 0.1
                    }
                }
            },
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 4,
                    "cross_project_leakage": 0.0,
                    "symbol_precision": 1.0,
                    "semantic_precision": 1.0
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
                "load": {
                    "hot_qps": { "target": 1200000.0 },
                    "hot_error_rate": { "target": 0.0 },
                    "hot_benchmark_table": {
                        "target_p50_ms": 0.012,
                        "target_p95_ms": 0.015,
                        "target_p99_ms": 0.020,
                        "target_max_ms": 0.500,
                        "target_workers": 16.0,
                        "target_sample_count": 10000.0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0
                    },
                    "hot_benchmark_table": {
                        "target_iterations": 20.0,
                        "target_warmup": 3.0
                    }
                },
                "accuracy": {
                    "symbol_precision": { "target": 0.99 },
                    "semantic_precision": { "target": 0.98 }
                }
            },
            "sla": {
                "checks": [
                    { "metric": "accuracy.cross_project_leakage", "status": "pass" },
                    { "metric": "accuracy.symbol_precision", "status": "pass" },
                    { "metric": "accuracy.semantic_precision", "status": "pass" }
                ]
            }
        });

        let cards = build_benchmark_cards(&snapshot);
        let cold_card = &cards[2];
        assert_eq!(cold_card["status"].as_str(), Some("waiting"));
        assert_eq!(cold_card["status_label"].as_str(), Some("идёт прогон"));
        assert!(
            cold_card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("обновляются по мере прогона")
        );
        assert_eq!(
            cold_card["table"]["columns"][2]["label"].as_str(),
            Some("Онлайн\nсейчас")
        );
        assert_eq!(
            cold_card["table"]["rows"][0]["label"].as_str(),
            Some("Прогресс")
        );
        assert_eq!(
            cold_card["table"]["rows"][0]["values"][1].as_str(),
            Some("128 из 442")
        );
        assert_eq!(
            cold_card["table"]["rows"][1]["values"][1].as_str(),
            Some("120 s")
        );
        assert_eq!(
            cold_card["table"]["rows"][2]["values"][0].as_str(),
            Some("Amai")
        );
        assert_eq!(
            cold_card["table"]["rows"][2]["values"][1].as_str(),
            Some("512 из 800")
        );
        assert_eq!(
            cold_card["table"]["rows"][4]["values"][1].as_str(),
            Some("1.777 ms")
        );
        assert_eq!(
            cold_card["table"]["rows"][13]["values"][1].as_str(),
            Some("9.5 s")
        );
        assert!(
            cold_card["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сейчас индексируется репозиторий Amai")
        );
    }

    #[test]
    fn client_budget_live_payload_surfaces_hourly_burn_reply_prefix() {
        let snapshot = json!({
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "client_budget_target_percent": 50
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "client_live_meter": {
                        "status": "observed",
                        "current_thread_bound": true,
                        "thread_binding_state": "current_thread_bound",
                        "ended_at_epoch_ms": 1000,
                        "client_turn_total_tokens": 1000,
                        "latest_model_context_window": 2000,
                        "context_used_percent": 50.0,
                        "primary_limit_used_percent": 57.0,
                        "primary_limit_remaining_percent": 43.0,
                        "secondary_limit_used_percent": 77.0,
                        "secondary_limit_remaining_percent": 23.0,
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "source": "codex_app_server_account_rate_limits_read_v1",
                            "observed_at_epoch_ms": 2000,
                            "primary_limit_used_percent": 57.0,
                            "primary_limit_remaining_percent": 43.0,
                            "secondary_limit_used_percent": 77.0,
                            "secondary_limit_remaining_percent": 23.0
                        }
                    },
                    "current_live_turn": {
                        "exact_pair_available": false
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "classification": "saving",
                        "reply_prefix": "5ч KPI: экономия 50.00%",
                        "projected_primary_used_per_hour_percent": 10.0,
                        "kpi_percent": 50.0,
                        "remaining_window_minutes": 30.0,
                        "actual_remaining_percent": 75.0,
                        "ideal_remaining_percent": 50.0,
                        "latest_observed_at_epoch_ms": 2000,
                        "projected_reset_delta_minutes": 30.0
                    }
                }
            }
        });

        let payload = super::client_budget_live_payload(&snapshot);
        assert_eq!(
            payload["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 50.00%")
        );
        let rows = payload["rows"].as_array().expect("rows");
        assert!(
            rows.iter().any(|row| {
                row["key"].as_str() == Some(super::CLIENT_LIMIT_HOURLY_BURN_ROW_KEY)
            })
        );
        let hourly_row = rows
            .iter()
            .find(|row| row["key"].as_str() == Some(super::CLIENT_LIMIT_HOURLY_BURN_ROW_KEY))
            .expect("hourly burn row");
        assert_eq!(
            hourly_row["target_selector"]["current_target_percent"],
            json!(50)
        );
        assert_eq!(
            hourly_row["target_selector"]["selected_chat_command"],
            json!("экономия_50%")
        );
    }

    #[test]
    fn client_budget_live_payload_prefers_live_personal_agent_reply_prefix() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "reply_prefix": "5ч KPI: экономия 28.49%"
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "client_budget_target_percent": 50
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "client_live_meter": {
                        "status": "observed",
                        "current_thread_bound": true,
                        "thread_binding_state": "current_thread_bound",
                        "ended_at_epoch_ms": 1000,
                        "client_turn_total_tokens": 1000,
                        "latest_model_context_window": 2000,
                        "context_used_percent": 50.0,
                        "primary_limit_used_percent": 57.0,
                        "primary_limit_remaining_percent": 43.0,
                        "secondary_limit_used_percent": 77.0,
                        "secondary_limit_remaining_percent": 23.0,
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "source": "codex_app_server_account_rate_limits_read_v1",
                            "observed_at_epoch_ms": 2000,
                            "primary_limit_used_percent": 57.0,
                            "primary_limit_remaining_percent": 43.0,
                            "secondary_limit_used_percent": 77.0,
                            "secondary_limit_remaining_percent": 23.0
                        }
                    },
                    "current_live_turn": {
                        "exact_pair_available": false
                    },
                    "personal_agent_kpi": {
                        "status": "observed",
                        "reply_prefix": "5ч KPI: экономия 61.25%"
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "classification": "saving",
                        "reply_prefix": "5ч KPI: экономия 50.00%",
                        "projected_primary_used_per_hour_percent": 10.0,
                        "kpi_percent": 50.0,
                        "remaining_window_minutes": 30.0,
                        "actual_remaining_percent": 75.0,
                        "ideal_remaining_percent": 50.0,
                        "latest_observed_at_epoch_ms": 2000,
                        "projected_reset_delta_minutes": 30.0
                    }
                }
            }
        });

        let payload = super::client_budget_live_payload(&snapshot);
        assert_eq!(
            payload["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 50.00%")
        );
        assert_eq!(
            payload["global_reply_prefix"].as_str(),
            Some("5ч KPI: экономия 50.00%")
        );
        assert_eq!(
            payload["reply_prefix_source"],
            json!("global_client_limit_hourly_burn")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_embeds_same_thread_host_control_in_selector() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "saving",
                "reply_prefix": "5ч KPI: экономия 50.00%",
                "projected_primary_used_per_hour_percent": 10.0,
                "kpi_percent": 50.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 75.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 2000,
                "projected_reset_delta_minutes": 30.0
            }),
            50,
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "summary": "Operator confirmed same-thread overlay opened.",
                    "recorded_at_epoch_ms": 3000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "opened",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 12000,
                                "context_used_percent": 4.65,
                                "primary_limit_used_percent": 21
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 2000
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 6000,
                "client_turn_total_tokens": 14500,
                "context_used_percent": 5.61,
                "primary_limit_used_percent": 23
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 4500
            }),
            &working_state::build_host_current_thread_control_surface_for_thread(Some(
                "thread-current",
            )),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["host_current_thread_control"]["command_id"],
            json!("thread-overlay-open-current")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_button_label"],
            json!("Open thread overlay")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control"]["external_uri_launch"]["uri"],
            json!("vscode://openai.chatgpt/thread-overlay/thread-current")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_last_feedback_kind"],
            json!("opened")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_last_feedback_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("Operator confirmed same-thread overlay opened.")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_effect_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("thread overlay")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_uses_surface_driven_compact_window_text() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "saving",
                "reply_prefix": "5ч KPI: экономия 50.00%",
                "projected_primary_used_per_hour_percent": 10.0,
                "kpi_percent": 50.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 75.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 2000,
                "projected_reset_delta_minutes": 30.0
            }),
            50,
            &json!({}),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 6000,
                "client_turn_total_tokens": 15000,
                "context_used_percent": 5.8,
                "primary_limit_used_percent": 24
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 4800
            }),
            &working_state::build_host_current_thread_control_surface_for_thread_and_stage(
                Some("thread-current"),
                working_state::HostContextCompactionStage::Preserve,
            ),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_button_label"],
            json!("Open compact window")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_intro"]
                .as_str()
                .unwrap_or_default()
                .contains("compact-window")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_notice_message"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_ack_intro"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_embeds_compact_chat_client_surface_assist() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "saving",
                "reply_prefix": "5ч KPI: экономия 50.00%",
                "projected_primary_used_per_hour_percent": 10.0,
                "kpi_percent": 50.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 75.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 2000,
                "projected_reset_delta_minutes": 30.0
            }),
            50,
            &json!({
                "project": {
                    "repo_root": env!("CARGO_MANIFEST_DIR")
                }
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 6000,
                "client_turn_total_tokens": 15000,
                "context_used_percent": 5.8,
                "primary_limit_used_percent": 24
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 4800
            }),
            &working_state::build_host_current_thread_control_surface_for_thread_and_stage(
                Some("thread-current"),
                working_state::HostContextCompactionStage::Preserve,
            ),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["compact_chat_required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            row["target_selector"]["compact_chat_note"]
                .as_str()
                .unwrap_or_default()
                .contains("clean")
        );
        assert!(
            row["target_selector"]["compact_chat_assist_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("./scripts/reconnect_local.sh --client")
        );
        assert!(
            row["target_selector"]["compact_chat_reconnect_bootstrap_command"]
                .as_str()
                .unwrap_or_default()
                .contains("./scripts/amai_exec.sh bootstrap reconnect --client")
        );
    }

    #[test]
    fn compact_chat_selector_client_surface_falls_back_to_discovered_repo_root() {
        let surface = super::compact_chat_selector_client_surface(&json!({}));
        assert!(
            surface["display_name"]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty())
        );
        assert!(
            surface["reconnect_shell_command"]
                .as_str()
                .unwrap_or_default()
                .contains("./scripts/reconnect_local.sh --client")
        );
    }

    #[test]
    fn host_current_thread_control_effect_recommends_rotate_fallback_after_failed_compact_window() {
        let effect = super::host_current_thread_control_effect_payload(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 3000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9000,
                "client_turn_total_tokens": 160500,
                "context_used_percent": 53.5,
                "primary_limit_used_percent": 66
            }),
            &json!({
                "stage": "critical_regrowth",
                "growth_since_compaction_tokens": 52000
            }),
        );
        assert_eq!(
            effect["effect_verdict"],
            json!("full_scale_client_burn_worsened_rotate_fallback_recommended")
        );
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(false));
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("полный 5ч burn")
        );
    }

    #[test]
    fn host_current_thread_control_effect_recommends_overlay_trial_during_critical_regrowth() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 3000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9000,
                "client_turn_total_tokens": 106500,
                "context_used_percent": 42.6,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "critical_regrowth",
                "growth_since_compaction_tokens": 26530
            }),
            Some("hotkey-window-open-current"),
        );
        assert_eq!(
            effect["effect_verdict"],
            json!("critical_regrowth_overlay_trial_recommended")
        );
        assert_eq!(effect["overlay_trial_recommended"], json!(true));
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("overlay")
        );
    }

    #[test]
    fn host_current_thread_control_effect_marks_recent_baseline_as_measurement_pending() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 8_500,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9_000,
                "client_turn_total_tokens": 101000,
                "context_used_percent": 40.2,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 20800
            }),
            Some("hotkey-window-open-current"),
        );
        assert_eq!(effect["measurement_pending"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(false));
        assert_eq!(effect["effect_verdict"], json!("measurement_pending"));
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("дождись измеримого effect")
        );
    }

    #[test]
    fn host_current_thread_control_effect_clears_measurement_pending_after_short_idle_window() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "opened",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "inactive"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 31_500,
                "client_turn_total_tokens": 100000,
                "context_used_percent": 40.0,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "inactive",
                "growth_since_compaction_tokens": 20000
            }),
            Some("thread-overlay-open-current"),
        );
        assert_eq!(effect["measurement_pending"], json!(false));
        assert_eq!(effect["measurement_sufficient"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(true));
        assert_eq!(
            effect["effect_verdict"],
            json!("opened_overlay_surface_observed")
        );
    }

    #[test]
    fn host_current_thread_control_effect_marks_verified_compaction_after_requested_feedback() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "compaction_count": 2,
                                "compacted_at_epoch_ms": 900,
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 300_000,
                "client_turn_total_tokens": 101000,
                "context_used_percent": 40.2,
                "primary_limit_used_percent": 61
            }),
            &json!({
                "stage": "preserve",
                "compaction_count": 3,
                "compacted_at_epoch_ms": 200_000,
                "growth_since_compaction_tokens": 20800
            }),
            Some("thread-overlay-open-current"),
        );
        assert_eq!(
            effect["verified_host_compaction_observed_after_feedback"],
            json!(true)
        );
        assert_eq!(effect["compaction_count_delta"], json!(1));
    }

    #[test]
    fn host_current_thread_control_effect_recommends_rotate_when_full_scale_burn_worsens() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 165_000,
                                "context_used_percent": 63.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 601_000,
                "client_turn_total_tokens": 93_000,
                "context_used_percent": 36.0,
                "primary_limit_used_percent": 40
            }),
            &json!({
                "stage": "critical_regrowth",
                "growth_since_compaction_tokens": 0
            }),
            Some("thread-overlay-open-current"),
        );
        assert_eq!(
            effect["effect_verdict"],
            json!("full_scale_client_burn_worsened_rotate_fallback_recommended")
        );
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["material_compaction_gain_observed"], json!(false));
        assert_eq!(effect["retry_allowed"], json!(false));
        assert!(
            effect["summary"]
                .as_str()
                .unwrap_or_default()
                .contains("против идеального темпа")
        );
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("полный 5ч burn")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_blocks_same_thread_retry_while_measurement_pending() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "overspend",
                "reply_prefix": "5ч KPI: переплата 10.00%",
                "projected_primary_used_per_hour_percent": 12.0,
                "kpi_percent": 10.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 40.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 9_000,
                "projected_reset_delta_minutes": -10.0
            }),
            90,
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 8_500,
                    "summary": "Requested compact window launch via host current-thread control.",
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9_000,
                "client_turn_total_tokens": 101000,
                "context_used_percent": 40.2,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 20800
            }),
            &working_state::build_host_current_thread_control_surface_for_thread_and_stage(
                Some("thread-current"),
                working_state::HostContextCompactionStage::Preserve,
            ),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["host_current_thread_control_retry_allowed"],
            json!(false)
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_measurement_pending"],
            json!(true)
        );
        assert!(
            row["target_selector"]["host_current_thread_control_retry_blocked_reason"]
                .as_str()
                .unwrap_or_default()
                .contains("Requested compact window launch")
        );
    }

    #[test]
    fn reply_execution_gate_waits_for_same_thread_effect_when_retry_is_blocked() {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "сожми текущий чат сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
            false,
            Some("thread-current"),
            Some("thread-overlay-open-current"),
            &json!({
                "retry_allowed": false,
                "retry_blocked_reason": "Requested same-thread overlay launch via host current-thread control.",
                "measurement_pending": false,
                "effect_verdict": "requested_overlay_surface_observed",
                "summary": "Overlay request is still active."
            }),
            false,
            Some("Requested same-thread overlay launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_bundle"]["host_current_thread_control"]["retry_allowed"],
            json!(false)
        );
        assert_eq!(
            gate["action_kind"],
            json!("wait_for_same_thread_effect_measurement")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("wait_for_same_thread_effect_measurement")
        );
        assert_eq!(
            gate["must_wait_for_same_thread_effect_measurement_before_reply"],
            json!(true)
        );
        assert!(gate["action_bundle"]["operator_flow"]["primary_command"].is_null());
        assert_eq!(
            gate["action_bundle"]["measurement_before_retry_required"],
            json!(true)
        );
    }

    #[test]
    fn reply_execution_gate_requests_feedback_confirmation_when_retry_is_blocked_by_pending_feedback()
     {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "сожми текущий чат сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
            false,
            Some("thread-current"),
            Some("thread-overlay-open-current"),
            &json!({
                "retry_allowed": false,
                "retry_blocked_reason": "Requested same-thread overlay launch via host current-thread control.",
                "measurement_pending": false,
                "effect_verdict": "requested_overlay_surface_observed",
                "summary": "Overlay request is still active."
            }),
            true,
            Some("Requested same-thread overlay launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["must_confirm_same_thread_host_control_feedback_before_reply"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["feedback_confirmation_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"],
            json!(true)
        );
    }

    #[test]
    fn reply_execution_gate_skips_feedback_confirmation_after_verified_host_compaction() {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "сожми текущий чат сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
            false,
            Some("thread-current"),
            Some("thread-overlay-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "verified_host_compaction_observed_after_feedback": true,
                "summary": "Real host compaction already observed after baseline."
            }),
            false,
            Some("Requested same-thread overlay launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_bundle"]["feedback_confirmation_before_retry_required"],
            Value::Null
        );
        assert_eq!(
            gate["action_bundle"]["measurement_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("wait_for_same_thread_effect_measurement")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"],
            Value::Null
        );
    }

    #[test]
    fn reply_execution_gate_keeps_rotate_order_when_same_thread_retry_is_disallowed_after_rotate_selection()
     {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "новый чат нужен сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            false,
            true,
            false,
            Some("thread-current"),
            Some("hotkey-window-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "verified_host_compaction_observed_after_feedback": true,
                "summary": "Surface already failed; rotate is primary."
            }),
            false,
            Some("Requested same-thread compact window launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("rotate_helper_command")
        );
        assert_eq!(
            gate["action_bundle"]["measurement_before_retry_required"],
            Value::Null
        );
        assert_eq!(
            gate["action_bundle"]["order"],
            json!([
                "run_rotate_helper",
                "open_fresh_chat",
                "run_continuity_startup"
            ])
        );
    }

    #[test]
    fn reply_execution_gate_requests_feedback_confirmation_before_rotate_when_same_thread_feedback_is_pending()
     {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "новый чат нужен сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            false,
            true,
            false,
            Some("thread-current"),
            Some("hotkey-window-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "requested_compact_surface_observed",
                "summary": "Requested compact window launch via host current-thread control."
            }),
            true,
            Some("Requested same-thread compact window launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["must_confirm_same_thread_host_control_feedback_before_reply"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["feedback_confirmation_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["order"],
            json!([
                "confirm_same_thread_host_control_feedback",
                "run_rotate_helper",
                "open_fresh_chat",
                "run_continuity_startup"
            ])
        );
    }

    #[test]
    fn reply_execution_gate_hard_blocks_rotate_now_for_pure_burn_critical_regrowth() {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "новый чат нужен сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            true,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            false,
            true,
            true,
            Some("thread-current"),
            Some("hotkey-window-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "verified_host_compaction_observed_after_feedback": true,
                "summary": "Surface already failed; rotate is primary."
            }),
            false,
            None,
        );
        assert_eq!(gate["action_kind"], json!("rotate_chat_for_client_budget"));
        assert_eq!(gate["blocking"], json!(true));
        assert_eq!(gate["must_rotate_before_reply"], json!(true));
        assert_eq!(
            gate["blocking_reply_contract"]["response_kind"],
            json!(working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND)
        );
        assert_eq!(
            gate["reason"],
            json!("client_budget_guard_pure_burn_rotate_now")
        );
    }

    #[test]
    fn selected_host_current_thread_control_state_prefers_lower_regrowth_rate_in_critical_stage() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 120000,
                                "context_used_percent": 48.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 32000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9000,
            "client_turn_total_tokens": 124000,
            "context_used_percent": 49.2,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 36000
        });
        let (surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(surface["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(
            surface["selection_reason"],
            json!("critical_regrowth_try_overlay")
        );
        assert_eq!(effect["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(same_thread_preferred, true);
    }

    #[test]
    fn selected_host_current_thread_control_state_drops_same_thread_preference_only_after_verified_failure_on_both_surfaces()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 170_000,
                                "context_used_percent": 65.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 80_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 1_500
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 2_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 160_000,
                                "context_used_percent": 62.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 2_500
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 602_000,
            "client_turn_total_tokens": 188_000,
            "context_used_percent": 72.5,
            "primary_limit_used_percent": 40
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 95_000,
            "compaction_count": 2,
            "compacted_at_epoch_ms": 3_000
        });
        let (_surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, false);
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(false));
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_same_thread_preference_when_verified_compaction_has_material_gain()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 170_000,
                                "context_used_percent": 65.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 80_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 1_500
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 2_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 160_000,
                                "context_used_percent": 62.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 2_500
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 602_000,
            "client_turn_total_tokens": 92_000,
            "context_used_percent": 35.5,
            "primary_limit_used_percent": 40
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 0,
            "compaction_count": 2,
            "compacted_at_epoch_ms": 3_000
        });
        let (_surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, true);
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["material_compaction_gain_observed"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(true));
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_same_thread_preference_when_both_surfaces_only_have_observational_rotate_fallback()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 170_000,
                                "context_used_percent": 65.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 80_000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 2_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 160_000,
                                "context_used_percent": 62.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 602_000,
            "client_turn_total_tokens": 92_000,
            "context_used_percent": 35.5,
            "primary_limit_used_percent": 40
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 0
        });
        let (_surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, true);
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(
            effect["verified_host_compaction_observed_after_feedback"],
            json!(false)
        );
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_same_thread_preference_for_oversized_critical_regrowth_without_verified_failure()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": []
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9_000,
            "client_turn_total_tokens": 190_000,
            "context_used_percent": 81.0,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 80_000,
            "regrowth_of_recovered_surface_ratio": 0.8
        });
        let (_surface, _effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, true);
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_pending_feedback_command_selected() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 110000,
                                "context_used_percent": 46.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 26000,
                                "stage": "inactive"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9_000,
            "client_turn_total_tokens": 111000,
            "context_used_percent": 46.2,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "inactive",
            "growth_since_compaction_tokens": 26200
        });

        let (surface, effect, _) = super::selected_host_current_thread_control_state(
            &report,
            &restore,
            &client_live_meter,
            &host_context_compaction,
        );

        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID)
        );
        assert_eq!(effect["command_id"], json!("hotkey-window-open-current"));
        assert_eq!(effect["effect_verdict"], json!("measurement_pending"));
    }

    #[test]
    fn selected_host_current_thread_control_state_ignores_pending_feedback_from_other_thread() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-old",
                            "client_live_meter": {
                                "client_turn_total_tokens": 110000,
                                "context_used_percent": 46.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 26000,
                                "stage": "inactive"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9_000,
            "client_turn_total_tokens": 111000,
            "context_used_percent": 46.2,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "inactive",
            "growth_since_compaction_tokens": 26200
        });

        let (surface, effect, _) = super::selected_host_current_thread_control_state(
            &report,
            &restore,
            &client_live_meter,
            &host_context_compaction,
        );

        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
        );
        assert_eq!(effect["command_id"], Value::Null);
    }

    #[test]
    fn selected_host_current_thread_control_state_prefers_newer_pending_feedback_command() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 9_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 120000,
                                "context_used_percent": 48.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 30000,
                                "stage": "inactive"
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 110000,
                                "context_used_percent": 46.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 26000,
                                "stage": "inactive"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 10_000,
            "client_turn_total_tokens": 120400,
            "context_used_percent": 48.1,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "inactive",
            "growth_since_compaction_tokens": 30100
        });

        let (surface, effect, _) = super::selected_host_current_thread_control_state(
            &report,
            &restore,
            &client_live_meter,
            &host_context_compaction,
        );

        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
        );
        assert_eq!(effect["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(effect["effect_verdict"], json!("measurement_pending"));
    }
}
