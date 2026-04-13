use crate::codex_threads;
use crate::config::{self, AppConfig};
use crate::continuity;
use crate::dashboard_format::*;
use crate::hardware_telemetry::{AcceleratorSummary, MachineSummary, collect_machine_summary};
use crate::onboarding;
use crate::working_state;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::env;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

mod dashboard_benchmark_cards;
mod dashboard_card_support;
mod dashboard_client_budget_support;
mod dashboard_context;
mod dashboard_payload;
mod dashboard_renderer;
mod dashboard_service_cards;
mod dashboard_working_state_card;
use self::dashboard_benchmark_cards::build_benchmark_cards;
pub use self::dashboard_card_support::monitoring_url;
use self::dashboard_card_support::{
    card, tcp_port_is_open, with_extra_class, with_status, with_status_label, with_status_tooltip,
    with_table_orientation,
};
pub(crate) use self::dashboard_client_budget_support::client_budget_live_payload;
use self::dashboard_client_budget_support::*;
pub use self::dashboard_context::browser_base_url;
pub use self::dashboard_payload::{build_live_summary_payload, build_payload};
pub use self::dashboard_renderer::render_html;
#[cfg(test)]
use self::dashboard_service_cards::benchmark_qdrant_live_card;
use self::dashboard_service_cards::build_service_cards;
use self::dashboard_working_state_card::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HistoricalStartupDrag {
    older_without_amai_tokens: u64,
    older_with_amai_tokens: u64,
    older_delta_tokens: i64,
    current_continuity_tokens: u64,
    older_continuity_tokens: u64,
}

pub use crate::dashboard_assets::{brand_lockup_svg, brand_mark_svg, favicon_ico};

#[cfg(test)]
fn compact_chat_selector_client_surface(restore_context: &Value) -> Value {
    dashboard_client_budget_support::compact_chat_selector_client_surface(restore_context)
}

pub fn current_session_budget_guard(snapshot: &Value) -> Value {
    current_session_budget_guard_with_restore_context(
        snapshot,
        &snapshot["token_budget_report"]["token_budget_report"],
        &snapshot["latest_repo_working_state_restore"]["working_state_restore"],
    )
}

fn current_agent_reply_prefix_fields<'a>(
    snapshot: &'a Value,
    report: &'a Value,
) -> (Option<&'a str>, Option<&'a str>, &'static str) {
    let global_reply_prefix = report["client_limit_hourly_burn"]["reply_prefix"].as_str();
    let personal_reply_prefix = report["personal_agent_kpi"]["reply_prefix"].as_str();
    let personal_confidence = report["personal_agent_kpi"]["confidence"].as_str();
    let aggregate_reply_prefix =
        snapshot["active_agent_budget"]["aggregate"]["reply_prefix"].as_str();
    let personal_reply_prefix = personal_reply_prefix.or(aggregate_reply_prefix);

    if personal_confidence == Some("online_limit_contour") {
        (
            personal_reply_prefix.or(global_reply_prefix),
            global_reply_prefix,
            "personal_agent_online_limit_contour",
        )
    } else {
        (
            global_reply_prefix,
            global_reply_prefix,
            "global_client_limit_hourly_burn",
        )
    }
}

fn current_session_budget_guard_with_restore_context(
    snapshot: &Value,
    report: &Value,
    restore_context: &Value,
) -> Value {
    let client_budget_target_percent =
        client_budget_target_percent_from_inputs(report, restore_context);
    let client_budget_target_active = client_budget_target_active(client_budget_target_percent);
    let client_budget_target_percent_f64 =
        client_budget_target_percent_f64(client_budget_target_percent);
    let client_live_meter = &report["client_live_meter"];
    let current_thread_bound = current_session_client_live_meter_available(client_live_meter);
    let current_session_summary = &report["current_session"];
    let current_session_statement = &report["statement_previews"]["current_session"];
    let current_session_alignment = &current_session_statement["client_limit_meter_alignment"];
    let current_session_exact_pair =
        exact_model_token_pair(current_session_summary, current_session_alignment);
    let session_events_total = current_session_summary["events_total"]
        .as_u64()
        .unwrap_or(0);
    let session_events = current_session_summary["counted_events"]
        .as_u64()
        .unwrap_or(0);
    let session_saved = current_session_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| current_session_summary["verified_effective_saved_tokens"].as_i64());
    let session_live_turn_exact_pair = live_turn_exact_pair(
        current_session_summary,
        client_live_meter,
        current_session_exact_pair,
    );
    let session_live_turn_exact_pair =
        current_live_turn_exact_pair(&report["current_live_turn"]).or(session_live_turn_exact_pair);
    let session_full_turn_savings_pct =
        full_turn_savings_pct_from_live_meter(client_live_meter, session_live_turn_exact_pair);
    let last_request_row = client_live_context_metric_row(client_live_meter);
    let client_limits_row = client_live_limit_metric_row(client_live_meter);
    let last_request = last_request_row
        .as_ref()
        .and_then(|row| row["value"].as_str().map(str::to_string));
    let client_limits = client_limits_row
        .as_ref()
        .and_then(|row| row["value"].as_str().map(str::to_string));
    let global_limit_source = global_client_limit_source(client_live_meter);
    let global_limit_guard = global_client_limit_guard(client_live_meter);
    let (reply_prefix, global_reply_prefix, reply_prefix_source) =
        current_agent_reply_prefix_fields(snapshot, report);
    let observed_at_epoch_ms = if current_session_client_live_meter_available(client_live_meter)
        || global_limit_guard.is_some()
    {
        preferred_client_limit_observed_at_epoch_ms(client_live_meter)
    } else {
        None
    };
    let max_guard_age_seconds = 10_u64;
    let hourly_burn = &report["client_limit_hourly_burn"];
    let host_context_compaction = latest_host_context_compaction_payload(report, restore_context);
    let host_context_compaction_stage =
        host_context_compaction_stage_from_payload(&host_context_compaction);
    let (
        host_current_thread_control,
        host_current_thread_control_effect,
        same_thread_compaction_preferred,
    ) = selected_host_current_thread_control_state(
        report,
        restore_context,
        client_live_meter,
        &host_context_compaction,
    );
    let current_live_turn_no_amai_activity = report["current_live_turn"]["status"].as_str()
        == Some("no_amai_activity_in_current_live_turn");
    let selected_host_current_thread_control_command_id =
        host_current_thread_control["command_id"].as_str();
    let selected_host_current_thread_control_feedback_kind =
        latest_host_current_thread_control_feedback_kind_for_command(
            restore_context,
            selected_host_current_thread_control_command_id,
        );
    let selected_host_current_thread_control_feedback_summary =
        latest_host_current_thread_control_feedback_summary_for_command(
            restore_context,
            selected_host_current_thread_control_command_id,
        );
    let selected_host_current_thread_control_feedback_pending =
        host_current_thread_control_feedback_pending_from_effect(
            selected_host_current_thread_control_feedback_kind,
            &host_current_thread_control_effect,
        );
    let selected_host_current_thread_control_measurement_pending =
        host_current_thread_control_effect["measurement_pending"].as_bool() == Some(true);
    let selected_host_current_thread_control_retry_allowed =
        host_current_thread_control_effect["retry_allowed"]
            .as_bool()
            .unwrap_or(true)
            && !selected_host_current_thread_control_feedback_pending;
    let host_current_thread_control = decorate_host_current_thread_control_surface(
        &host_current_thread_control,
        &host_current_thread_control_effect,
        selected_host_current_thread_control_feedback_pending,
        selected_host_current_thread_control_feedback_summary.as_deref(),
    );
    let recommended_headline = restore_context["current_goal"]
        .as_str()
        .filter(|value| !value.is_empty());
    let recommended_next_step = restore_context["next_step"]
        .as_str()
        .filter(|value| !value.is_empty());
    let preserves_return_obligation = restore_context["execctl_resume_state"]
        .as_str()
        .is_some_and(|value| value != "clear");
    let session_rotate_bundle = restore_context.is_object().then(|| {
        working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
            restore_context["project"]["code"].as_str(),
            restore_context["namespace"]["code"].as_str(),
            restore_context["project"]["repo_root"].as_str(),
            preserves_return_obligation,
            recommended_headline,
            recommended_next_step,
            host_context_compaction_stage,
            same_thread_compaction_preferred,
            host_current_thread_control["thread_id"].as_str(),
            host_current_thread_control["command_id"].as_str(),
        )
    });
    let session_client_turn_pressure = client_turn_pressure_guard_with_target(
        client_live_meter,
        session_live_turn_exact_pair,
        hourly_burn,
        &report["current_live_turn"],
        client_budget_target_percent,
    );
    let session_boundary_pressure =
        continuity_boundary_pressure(current_session_summary, current_session_alignment);
    let tracked_slice_row =
        model_token_savings_metric_row(current_session_summary, current_session_alignment);
    let tracked_slice = tracked_slice_row["value"]
        .as_str()
        .map(humanize_tracked_slice_savings_value);
    let tracked_slice_truth =
        exact_pair_status_metric_row(current_session_alignment).and_then(|row| {
            row["value"]
                .as_str()
                .map(humanize_tracked_slice_exactness_value)
        });
    let tracked_slice_tooltip = tracked_slice_row["tooltip"].as_str().map(str::to_string);
    let next_action = client_turn_pressure_metric_row(
        session_client_turn_pressure,
        session_rotate_bundle.as_ref(),
        same_thread_compaction_preferred,
    )
    .and_then(|row| row["value"].as_str().map(str::to_string));
    let mut compact_status = if let Some(guard) = session_client_turn_pressure {
        guard.severity.to_string()
    } else if session_boundary_pressure.is_some_and(|(boundary_tokens, strict_tokens)| {
        continuity_boundary_pressure_is_alert(session_saved, boundary_tokens, strict_tokens)
    }) {
        "alert".to_string()
    } else if client_budget_target_active
        && session_full_turn_savings_pct
            .is_some_and(|value| value < client_budget_target_percent_f64)
    {
        "alert".to_string()
    } else {
        savings_status(session_saved, session_events, session_events_total).to_string()
    };
    let mut compact_status_label = if let Some(guard) = session_client_turn_pressure {
        Some(guard.status_label.to_string())
    } else if session_full_turn_savings_pct.is_none()
        && current_session_client_live_meter_available(client_live_meter)
    {
        Some("реальная экономия не доказана".to_string())
    } else if session_full_turn_savings_pct.is_some_and(|value| {
        client_budget_target_active && value < client_budget_target_percent_f64
    }) {
        Some(client_budget_target_alert_label(
            client_budget_target_percent,
        ))
    } else if session_boundary_pressure.is_some_and(|(boundary_tokens, strict_tokens)| {
        continuity_boundary_pressure_is_alert(session_saved, boundary_tokens, strict_tokens)
    }) {
        Some("burn в continuity startup".to_string())
    } else {
        None
    };
    let mut compact_status_tooltip = if let Some(guard) = session_client_turn_pressure {
        Some(client_turn_pressure_tooltip(
            guard,
            session_rotate_bundle.as_ref(),
            same_thread_compaction_preferred,
        ))
    } else if session_full_turn_savings_pct.is_none()
        && current_session_client_live_meter_available(client_live_meter)
    {
        Some(
            "Статус требует внимания по следующим причинам:\n- Для текущего живого turn ещё нет доказанной same-turn пары `без Amai / с Amai`.\n- Значит реальную экономию на полной шкале клиента пока нельзя честно показать числом.\n- Пока эта пара не materialized, нижняя строка про учтённую часть остаётся внутренним Amai-срезом, а не полным client spend.\n- Чтобы получить реальную экономию, нужно быстрее фиксировать exact pair на коротком live turn и для этого сначала сжать текущий giant thread через same-thread compact window, а не расширять его новыми ходами."
                .to_string(),
        )
    } else if let Some(full_turn_savings_pct) = session_full_turn_savings_pct
        .filter(|value| client_budget_target_active && *value < client_budget_target_percent_f64)
    {
        Some(format!(
            "Статус требует внимания по следующим причинам:\n- Реальная экономия на полной шкале клиента сейчас всего {}.\n- {}\n- Значит текущий thread пока жжёт почти весь полный client turn/context, а Amai экономит только малую долю.\n- Чтобы реально улучшить картину без потери точности, нужно дальше уменьшать полный размер turn и жёстко удерживать same-thread compact surface, чтобы следующий exact pair materialized на коротком live turn.",
            format_percent(Some(full_turn_savings_pct)),
            client_budget_target_sentence(client_budget_target_percent)
        ))
    } else if let Some((boundary_tokens, strict_tokens)) =
        session_boundary_pressure.filter(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(session_saved, *boundary_tokens, *strict_tokens)
        })
    {
        Some(format!(
            "Статус требует внимания по следующим причинам:\n- В этой сессии savings-KPI пока не показывает положительную подтверждённую экономию.\n- При этом observed continuity startup уже сжёг {} токенов.\n- Strict same-meter slice по клиентскому запросу пока даёт только {} токенов.\n- Значит лимит сейчас уходит главным образом в continuity restore, а не в retrieval/workflow effect.",
            format_u64(Some(boundary_tokens)),
            format_u64(Some(strict_tokens))
        ))
    } else if session_events_total > 0 && session_events == 0 {
        Some(
            "Статус пока не может считаться нормальным по следующим причинам:\n- В этой сессии уже были живые запросы.\n- Но пока ни один из них ещё не подтвердился как полезный без потери качества.\n- Как только появится первый такой случай, главный итог этой карточки начнёт считаться."
                .to_string(),
        )
    } else if session_events > 0 && session_saved.unwrap_or_default() < 0 {
        Some(format!(
            "Статус требует внимания по следующим причинам:\n- В подтверждённой части текущей сессии экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai вышел тяжелее обычного пути без Amai.\n- Нижние строки со всем живым потоком показаны отдельно и не отменяют этот итог.",
            format_signed_count(session_saved)
        ))
    } else {
        None
    };
    if compact_status == "pass" {
        if let Some((status, label, tooltip)) =
            exact_pair_card_status_override(current_session_alignment)
        {
            compact_status = status.to_string();
            compact_status_label = Some(label.to_string());
            compact_status_tooltip = Some(tooltip);
        }
    }
    let full_turn_savings_percent =
        session_full_turn_savings_pct.map(|value| format_percent(Some(value)));
    let compact_note = if let Some(guard) = session_client_turn_pressure {
        client_turn_pressure_note_sentence_for_preference(
            Some(guard),
            same_thread_compaction_preferred,
        )
        .unwrap_or_default()
    } else if let Some(value) = full_turn_savings_percent.as_deref() {
        format!("Полная шкала клиента сейчас даёт {value}.")
    } else {
        "Реальная экономия на полной шкале клиента пока не доказана для текущего turn.".to_string()
    };
    let compact_status_label = compact_status_label.unwrap_or_else(|| compact_status.clone());
    let should_rotate_chat_now = compact_status_label == "новый чат нужен сейчас";
    let should_rotate_chat_soon =
        compact_status_label == "новый чат рекомендован" || should_rotate_chat_now;
    let hourly_burn_kpi_percent = hourly_burn["kpi_percent"].as_f64();
    let hourly_burn_below_target = client_budget_target_active
        && hourly_burn["status"].as_str() == Some("observed")
        && hourly_burn_kpi_percent.unwrap_or(0.0) < client_budget_target_percent_f64;
    let live_turn_below_target = client_budget_target_active
        && session_full_turn_savings_pct
            .is_some_and(|value| value < client_budget_target_percent_f64);
    let target_pressure_active =
        should_rotate_chat_soon || hourly_burn_below_target || live_turn_below_target;
    let same_thread_compaction_advisory =
        should_rotate_chat_soon && same_thread_compaction_preferred;
    let host_context_compaction_preserve_active =
        host_context_compaction["preserve_active"].as_bool() == Some(true);
    let compact_reply_required = !should_rotate_chat_now
        && (target_pressure_active || host_context_compaction_preserve_active);
    let requires_global_budget_recovery_before_reply =
        global_limit_guard.is_some_and(|guard| guard.severity == "critical");
    let status = global_limit_guard
        .map(|guard| guard.severity)
        .unwrap_or(compact_status.as_str());
    let status_label = global_limit_guard
        .map(|guard| guard.status_label)
        .unwrap_or_else(|| {
            client_turn_pressure_display_status_label(
                compact_status_label.as_str(),
                same_thread_compaction_advisory,
            )
        });
    let global_limit_note = global_limit_guard.map(|guard| {
        global_client_limit_guard_note(
            guard,
            client_limits.as_deref(),
            preferred_client_limit_meter_is_exact(client_live_meter),
        )
    });
    let status_tooltip = global_limit_note.clone().or(compact_status_tooltip.clone());
    let note = global_limit_note
        .clone()
        .unwrap_or_else(|| compact_note.clone());
    let reply_execution_gate = build_client_budget_reply_execution_gate_with_primary_command(
        status,
        status_label,
        reply_prefix,
        global_reply_prefix,
        reply_prefix_source,
        observed_at_epoch_ms,
        max_guard_age_seconds,
        should_rotate_chat_now,
        should_rotate_chat_soon,
        compact_reply_required,
        requires_global_budget_recovery_before_reply,
        preserves_return_obligation,
        restore_context["project"]["code"].as_str(),
        restore_context["namespace"]["code"].as_str(),
        restore_context["project"]["repo_root"].as_str(),
        recommended_headline,
        recommended_next_step,
        client_budget_target_percent,
        host_context_compaction_stage,
        same_thread_compaction_preferred,
        target_pressure_active,
        current_live_turn_no_amai_activity,
        host_current_thread_control["thread_id"].as_str(),
        selected_host_current_thread_control_command_id,
        &host_current_thread_control_effect,
        selected_host_current_thread_control_feedback_pending,
        selected_host_current_thread_control_feedback_summary.as_deref(),
    );
    json!({
        "source": "dashboard_current_session_budget_guard_v2",
        "status": status,
        "status_label": status_label,
        "reply_prefix": reply_prefix,
        "global_reply_prefix": global_reply_prefix,
        "reply_prefix_source": reply_prefix_source,
        "status_tooltip": status_tooltip,
        "full_turn_savings_proven": session_full_turn_savings_pct.is_some(),
        "full_turn_savings_percent": full_turn_savings_percent,
        "should_rotate_chat_now": should_rotate_chat_now,
        "should_rotate_chat_soon": should_rotate_chat_soon,
        "requires_global_budget_recovery_before_reply": requires_global_budget_recovery_before_reply,
        "next_action": next_action,
        "last_request": last_request,
        "client_limits": client_limits,
        "global_client_limit_source": global_limit_source.unwrap_or(Value::Null),
        "observed_at_epoch_ms": observed_at_epoch_ms,
        "client_live_meter_current_thread_bound": current_thread_bound,
        "client_live_meter_thread_binding_state": client_live_meter["thread_binding_state"]
            .as_str()
            .unwrap_or(if current_thread_bound {
                "current_thread_bound"
            } else {
                "missing_rollout_client_meter"
            }),
        "client_budget_target_percent": client_budget_target_percent,
        "max_guard_age_seconds": max_guard_age_seconds,
        "reply_execution_gate": reply_execution_gate,
        "host_context_compaction": host_context_compaction,
        "host_current_thread_control_effect": host_current_thread_control_effect,
        "same_thread_compaction_preferred": same_thread_compaction_preferred,
        "selected_host_current_thread_control_feedback_kind":
            selected_host_current_thread_control_feedback_kind,
        "selected_host_current_thread_control_feedback_summary":
            selected_host_current_thread_control_feedback_summary,
        "selected_host_current_thread_control_feedback_pending":
            selected_host_current_thread_control_feedback_pending,
        "selected_host_current_thread_control_measurement_pending":
            selected_host_current_thread_control_measurement_pending,
        "selected_host_current_thread_control_retry_allowed":
            selected_host_current_thread_control_retry_allowed,
        "host_current_thread_control": host_current_thread_control,
        "tracked_slice": tracked_slice,
        "tracked_slice_truth": tracked_slice_truth,
        "tracked_slice_tooltip": tracked_slice_tooltip,
        "reason": note.clone(),
        "note": note,
    })
}

fn build_client_budget_reply_execution_gate_with_primary_command(
    status: &str,
    status_label: &str,
    reply_prefix: Option<&str>,
    global_reply_prefix: Option<&str>,
    reply_prefix_source: &str,
    observed_at_epoch_ms: Option<u64>,
    max_guard_age_seconds: u64,
    should_rotate_chat_now: bool,
    should_rotate_chat_soon: bool,
    compact_reply_required: bool,
    requires_global_budget_recovery_before_reply: bool,
    preserves_return_obligation: bool,
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    recommended_headline: Option<&str>,
    recommended_next_step: Option<&str>,
    client_budget_target_percent: u64,
    host_context_compaction_stage: working_state::HostContextCompactionStage,
    same_thread_compaction_preferred: bool,
    target_pressure_active: bool,
    current_live_turn_no_amai_activity: bool,
    same_thread_thread_id: Option<&str>,
    same_thread_primary_command_id: Option<&str>,
    host_current_thread_control_effect: &Value,
    selected_host_current_thread_control_feedback_pending: bool,
    selected_host_current_thread_control_feedback_summary: Option<&str>,
) -> Value {
    let rotate_advisory = should_rotate_chat_soon;
    let same_thread_compaction_advisory = rotate_advisory && same_thread_compaction_preferred;
    let pure_burn_rotate_hard_block = !requires_global_budget_recovery_before_reply
        && should_rotate_chat_now
        && !same_thread_compaction_advisory
        && host_context_compaction_stage.critical_regrowth_active()
        && current_live_turn_no_amai_activity;
    let blocking = pure_burn_rotate_hard_block;
    let reply_budget_mode = if requires_global_budget_recovery_before_reply
        || pure_burn_rotate_hard_block
        || compact_reply_required
        || rotate_advisory
    {
        working_state::ClientReplyBudgetMode::CompactHighSignal
    } else {
        working_state::ClientReplyBudgetMode::Normal
    };
    let (
        action_kind,
        reason,
        must_rotate_before_reply,
        save_handoff_before_rotate,
        fresh_chat_requires_continuity_startup,
        blocking_reply_mode,
        action_bundle,
        preserves_return_obligation_field,
    ) = if requires_global_budget_recovery_before_reply {
        (
            "wait_for_global_client_budget_recovery",
            "client_budget_guard_global_exhaustion",
            false,
            false,
            false,
            working_state::ClientBudgetBlockingReplyMode::Inactive,
            working_state::build_wait_for_global_client_budget_action_bundle(
                project_code,
                namespace_code,
                repo_root,
                preserves_return_obligation,
                recommended_headline,
                recommended_next_step,
            ),
            preserves_return_obligation,
        )
    } else if pure_burn_rotate_hard_block {
        (
            "rotate_chat_for_client_budget",
            "client_budget_guard_pure_burn_rotate_now",
            true,
            true,
            true,
            working_state::ClientBudgetBlockingReplyMode::RotateChatOnly,
            working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
                project_code,
                namespace_code,
                repo_root,
                preserves_return_obligation,
                recommended_headline,
                recommended_next_step,
                host_context_compaction_stage,
                same_thread_compaction_preferred,
                same_thread_thread_id,
                same_thread_primary_command_id,
            ),
            preserves_return_obligation,
        )
    } else if same_thread_compaction_advisory {
        (
            "compact_current_thread_for_client_budget",
            "client_budget_guard_same_thread_compaction",
            false,
            false,
            false,
            working_state::ClientBudgetBlockingReplyMode::Inactive,
            working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
                project_code,
                namespace_code,
                repo_root,
                preserves_return_obligation,
                recommended_headline,
                recommended_next_step,
                host_context_compaction_stage,
                same_thread_compaction_preferred,
                same_thread_thread_id,
                same_thread_primary_command_id,
            ),
            preserves_return_obligation,
        )
    } else if rotate_advisory {
        (
            "rotate_chat_for_client_budget",
            "client_budget_guard_pressure",
            false,
            true,
            true,
            working_state::ClientBudgetBlockingReplyMode::Inactive,
            working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
                project_code,
                namespace_code,
                repo_root,
                preserves_return_obligation,
                recommended_headline,
                recommended_next_step,
                host_context_compaction_stage,
                same_thread_compaction_preferred,
                same_thread_thread_id,
                same_thread_primary_command_id,
            ),
            preserves_return_obligation,
        )
    } else {
        (
            "continue_current_chat",
            "client_budget_guard_clear",
            false,
            false,
            false,
            working_state::ClientBudgetBlockingReplyMode::Inactive,
            Value::Null,
            preserves_return_obligation,
        )
    };
    let gate_fresh_until_epoch_ms =
        observed_at_epoch_ms.map(|value| value.saturating_add(max_guard_age_seconds * 1000));
    let reply_prefix = reply_prefix
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let global_reply_prefix = global_reply_prefix
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut gate = json!({
        "gate_version": "client-reply-budget-gate-v1",
        "status": status,
        "status_label": status_label,
        "reply_prefix": reply_prefix,
        "global_reply_prefix": global_reply_prefix,
        "reply_prefix_source": reply_prefix_source,
        "action_kind": action_kind,
        "reason": reason,
        "blocking": blocking,
        "must_rotate_before_reply": must_rotate_before_reply,
        "must_wait_for_budget_recovery_before_reply": false,
        "unrelated_reply_allowed": !blocking,
        "reply_budget_mode": match reply_budget_mode {
            working_state::ClientReplyBudgetMode::Normal => working_state::CLIENT_REPLY_BUDGET_MODE_NORMAL,
            working_state::ClientReplyBudgetMode::CompactHighSignal => working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL,
        },
        "client_budget_target_percent": client_budget_target_percent,
        "reply_budget_contract": working_state::build_client_reply_budget_contract_with_target(
            reply_budget_mode,
            client_budget_target_percent,
            host_context_compaction_stage,
            target_pressure_active,
            current_live_turn_no_amai_activity,
        ),
        "host_context_compaction_stage": host_context_compaction_stage.as_str(),
        "host_context_compaction_preserve_active": host_context_compaction_stage.preserve_active(),
        "host_context_compaction_critical_regrowth_active":
            host_context_compaction_stage.critical_regrowth_active(),
        "save_handoff_before_rotate": save_handoff_before_rotate,
        "fresh_chat_requires_continuity_startup": fresh_chat_requires_continuity_startup,
        "full_scale_client_truth_required": true,
        "guard_observed_at_epoch_ms": observed_at_epoch_ms,
        "max_guard_age_seconds": max_guard_age_seconds,
        "guard_fresh_until_epoch_ms": gate_fresh_until_epoch_ms,
        "rotate_now": should_rotate_chat_now,
        "rotate_soon": should_rotate_chat_soon,
        "preserves_return_obligation": preserves_return_obligation_field,
        "blocking_reply_contract": working_state::build_client_budget_blocking_reply_contract(
            blocking_reply_mode,
        ),
        "action_bundle": action_bundle,
    });
    let mut gate_action_kind_override: Option<&'static str> = None;
    let mut gate_reason_override: Option<&'static str> = None;
    let mut gate_requires_feedback_confirmation = false;
    let mut gate_requires_effect_measurement = false;
    let mut gate_unrelated_reply_allowed_override: Option<bool> = None;
    if let Some(action_bundle) = gate["action_bundle"].as_object_mut()
        && let Some(surface) = action_bundle.get("host_current_thread_control").cloned()
    {
        let decorated_surface = decorate_host_current_thread_control_surface(
            &surface,
            host_current_thread_control_effect,
            selected_host_current_thread_control_feedback_pending,
            selected_host_current_thread_control_feedback_summary,
        );
        let retry_allowed = decorated_surface["retry_allowed"].as_bool().unwrap_or(true);
        let retry_blocked_reason = decorated_surface["retry_blocked_reason"]
            .as_str()
            .map(str::to_string);
        action_bundle.insert("host_current_thread_control".to_string(), decorated_surface);
        if !retry_allowed {
            let primary_command_kind = action_bundle
                .get("operator_flow")
                .and_then(Value::as_object)
                .and_then(|operator_flow| operator_flow["primary_command_kind"].as_str());
            let same_thread_primary_selected =
                primary_command_kind == Some("same_thread_host_control_launch_command");
            let feedback_pending = selected_host_current_thread_control_feedback_pending;
            if same_thread_primary_selected {
                if feedback_pending {
                    action_bundle.insert(
                        "feedback_confirmation_before_retry_required".to_string(),
                        json!(true),
                    );
                    action_bundle.insert(
                        "order".to_string(),
                        json!([
                            "confirm_same_thread_host_control_feedback",
                            "measure_existing_same_thread_effect",
                            "fallback_rotate_chat"
                        ]),
                    );
                } else {
                    action_bundle
                        .insert("measurement_before_retry_required".to_string(), json!(true));
                    action_bundle.insert(
                        "order".to_string(),
                        json!([
                            "measure_existing_same_thread_effect",
                            "reuse_latest_live_diagnostics",
                            "fallback_rotate_chat"
                        ]),
                    );
                }
            }
            if let Some(operator_flow) = action_bundle
                .get_mut("operator_flow")
                .and_then(Value::as_object_mut)
                && operator_flow["primary_command_kind"].as_str()
                    == Some("same_thread_host_control_launch_command")
            {
                if feedback_pending {
                    gate_action_kind_override = Some("confirm_same_thread_host_control_feedback");
                    gate_reason_override = Some("same_thread_host_control_feedback_pending");
                    gate_unrelated_reply_allowed_override = Some(false);
                    gate_requires_feedback_confirmation = true;
                } else {
                    gate_action_kind_override = Some("wait_for_same_thread_effect_measurement");
                    gate_reason_override = Some("same_thread_effect_measurement_pending");
                    gate_unrelated_reply_allowed_override = Some(false);
                    gate_requires_effect_measurement = true;
                }
                operator_flow.insert(
                    "primary_command_kind".to_string(),
                    if feedback_pending {
                        json!("confirm_same_thread_host_control_feedback")
                    } else {
                        json!("wait_for_same_thread_effect_measurement")
                    },
                );
                operator_flow.insert("primary_command".to_string(), Value::Null);
                if feedback_pending {
                    operator_flow.insert(
                        "same_thread_feedback_confirmation_required".to_string(),
                        json!(true),
                    );
                    operator_flow.insert(
                        "same_thread_feedback_confirmation_summary".to_string(),
                        retry_blocked_reason
                            .clone()
                            .map_or(Value::Null, Value::String),
                    );
                } else {
                    operator_flow.insert(
                        "same_thread_effect_measurement_required".to_string(),
                        json!(true),
                    );
                    operator_flow.insert(
                        "same_thread_effect_measurement_summary".to_string(),
                        retry_blocked_reason
                            .clone()
                            .map_or(Value::Null, Value::String),
                    );
                }
            }
            if feedback_pending {
                action_bundle.insert(
                    "feedback_confirmation_before_retry_required".to_string(),
                    json!(true),
                );
                action_bundle.insert(
                    "order".to_string(),
                    json!([
                        "confirm_same_thread_host_control_feedback",
                        "run_rotate_helper",
                        "open_fresh_chat",
                        "run_continuity_startup"
                    ]),
                );
            }
            if feedback_pending
                && let Some(operator_flow) = action_bundle
                    .get_mut("operator_flow")
                    .and_then(Value::as_object_mut)
                && operator_flow["primary_command_kind"].as_str() == Some("rotate_helper_command")
            {
                gate_action_kind_override = Some("confirm_same_thread_host_control_feedback");
                gate_reason_override = Some("same_thread_host_control_feedback_pending");
                gate_unrelated_reply_allowed_override = Some(false);
                gate_requires_feedback_confirmation = true;
                operator_flow.insert(
                    "primary_command_kind".to_string(),
                    json!("confirm_same_thread_host_control_feedback"),
                );
                operator_flow.insert("primary_command".to_string(), Value::Null);
                operator_flow.insert(
                    "same_thread_feedback_confirmation_required".to_string(),
                    json!(true),
                );
                operator_flow.insert(
                    "same_thread_feedback_confirmation_summary".to_string(),
                    retry_blocked_reason
                        .clone()
                        .map_or(Value::Null, Value::String),
                );
            }
            if let Some(run_same_thread_host_control) = action_bundle
                .get_mut("run_same_thread_host_control")
                .and_then(Value::as_object_mut)
            {
                run_same_thread_host_control
                    .insert("preferred_before_rotate".to_string(), json!(false));
            }
        }
        if action_bundle["operator_flow"]["primary_command_kind"].as_str()
            == Some("rotate_helper_command")
        {
            action_bundle.remove("measurement_before_retry_required");
            action_bundle.remove("feedback_confirmation_before_retry_required");
            action_bundle.insert(
                "order".to_string(),
                json!([
                    "run_rotate_helper",
                    "open_fresh_chat",
                    "run_continuity_startup"
                ]),
            );
        }
    }
    if let Some(action_kind) = gate_action_kind_override {
        gate["action_kind"] = json!(action_kind);
    }
    if let Some(reason) = gate_reason_override {
        gate["reason"] = json!(reason);
    }
    if let Some(unrelated_reply_allowed) = gate_unrelated_reply_allowed_override {
        gate["unrelated_reply_allowed"] = json!(unrelated_reply_allowed);
    }
    if gate_requires_feedback_confirmation {
        gate["must_confirm_same_thread_host_control_feedback_before_reply"] = json!(true);
    }
    if gate_requires_effect_measurement {
        gate["must_wait_for_same_thread_effect_measurement_before_reply"] = json!(true);
    }
    gate
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

fn active_agent_budget_card_status(surface: &Value) -> (&'static str, &'static str, String) {
    let aggregate = &surface["aggregate"];
    let status = aggregate["status"].as_str().unwrap_or("missing");
    let classification = aggregate["classification"].as_str().unwrap_or("missing");
    match (status, classification) {
        ("observed", "overspend") => (
            "alert",
            "активные агенты жгут лимит",
            "Среднее по активным агентам сейчас в переплате, поэтому карточка требует внимания."
                .to_string(),
        ),
        ("observed", _) => (
            "pass",
            "только активные агенты",
            "Карточка показывает только личный 5ч KPI и текущий лимит клиента для реально активных агентов."
                .to_string(),
        ),
        ("partial", _) => (
            "waiting",
            "не у всех KPI materialized",
            "Не у каждого активного агента уже есть measured личный 5ч KPI, поэтому среднее fail-closed не посчитано."
                .to_string(),
        ),
        _ => (
            "waiting",
            "активных агентов сейчас нет",
            "Сейчас нет active lease, поэтому карточка не показывает персональные KPI."
                .to_string(),
        ),
    }
}

pub(crate) fn build_active_agent_budget_session_card_from_surface(
    surface: &Value,
) -> Option<Value> {
    let agents = surface["agents"].as_array()?;
    let (status, _status_label, _status_tooltip) = active_agent_budget_card_status(surface);
    let aggregate_value = surface["aggregate"]["reply_prefix"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("5ч KPI: н/д")
        .to_string();
    let mut agent_blocks = Vec::new();
    for agent in agents {
        let agent_label =
            compact_dashboard_text(agent["agent_label"].as_str(), 72, "Активный агент");
        let kpi_prefix = agent["personal_agent_kpi"]["reply_prefix"]
            .as_str()
            .unwrap_or("5ч KPI: н/д");
        let agent_tooltip = agent["thread_title"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(|thread_title| {
                format!(
                    "{}\n- {}\n- {}",
                    agent["agent_scope"]
                        .as_str()
                        .unwrap_or("scope ещё нет данных"),
                    compact_dashboard_text(Some(thread_title), 88, thread_title),
                    agent["cwd"].as_str().unwrap_or("cwd ещё нет данных"),
                )
            })
            .unwrap_or_else(|| {
                agent["agent_scope"]
                    .as_str()
                    .unwrap_or("scope ещё нет данных")
                    .to_string()
            });
        let limit_label = active_agent_online_limit_label(agent);
        let (limit_value, limit_tooltip) = active_agent_online_limit_field(agent);
        let (pressure_value, pressure_tooltip) = active_agent_live_pressure_field(agent)
            .map(|(value, tooltip)| (Some(value), Some(tooltip)))
            .unwrap_or((None, None));
        let kpi_tooltip = agent["personal_agent_kpi"]["summary"]
            .as_str()
            .map(str::to_string);
        agent_blocks.push(json!({
            "agent_scope": agent["agent_scope"].clone(),
            "agent_label": agent_label,
            "agent_tooltip": agent_tooltip,
            "limit_label": limit_label,
            "limit_value": limit_value,
            "limit_tooltip": limit_tooltip,
            "pressure_label": pressure_value
                .as_ref()
                .map(|_| "Последний запрос:"),
            "pressure_value": pressure_value,
            "pressure_tooltip": pressure_tooltip,
            "kpi_value": kpi_prefix,
            "kpi_tooltip": kpi_tooltip,
        }));
    }
    let shared_limit = shared_active_agent_limit(&agent_blocks);
    if shared_limit.is_some() {
        for block in agent_blocks.iter_mut() {
            if let Some(root) = block.as_object_mut() {
                root.remove("limit_label");
                root.remove("limit_value");
                root.remove("limit_tooltip");
            }
        }
    }
    let mut legacy_rows = Vec::new();
    if let Some((label, value, tooltip)) = shared_limit.as_ref() {
        legacy_rows.push(metric_row(label, value.to_string(), tooltip.as_deref()));
    }
    for block in &agent_blocks {
        legacy_rows.push(metric_row(
            "Агент:",
            block["agent_label"]
                .as_str()
                .unwrap_or("Активный агент")
                .to_string(),
            block["agent_tooltip"].as_str(),
        ));
        if let Some(limit_value) = block["limit_value"].as_str() {
            legacy_rows.push(metric_row(
                block["limit_label"]
                    .as_str()
                    .unwrap_or("Лимит клиента сейчас:"),
                limit_value.to_string(),
                block["limit_tooltip"].as_str(),
            ));
        }
        legacy_rows.push(metric_row(
            "KPI:",
            block["kpi_value"]
                .as_str()
                .unwrap_or("5ч KPI: н/д")
                .to_string(),
            block["kpi_tooltip"].as_str(),
        ));
        if let Some(pressure_value) = block["pressure_value"].as_str() {
            legacy_rows.push(metric_row(
                block["pressure_label"]
                    .as_str()
                    .unwrap_or("Последний запрос:"),
                pressure_value.to_string(),
                block["pressure_tooltip"].as_str(),
            ));
        }
    }
    let mut card = card_with_rows(
        "Экономия токенов за текущую сессию",
        aggregate_value,
        String::new(),
        status,
        None,
        None,
        legacy_rows,
    );
    if let Some(root) = card.as_object_mut() {
        root.insert(
            "presentation_variant".to_string(),
            Value::from("active_agent_budget_grouped_v3"),
        );
        root.insert("status_label".to_string(), Value::from(String::new()));
        root.insert("status_tooltip".to_string(), Value::Null);
        root.insert("agent_blocks".to_string(), Value::from(agent_blocks));
        if let Some((label, value, tooltip)) = shared_limit {
            root.insert("shared_limit_label".to_string(), Value::from(label));
            root.insert("shared_limit_value".to_string(), Value::from(value));
            root.insert(
                "shared_limit_tooltip".to_string(),
                tooltip.map(Value::from).unwrap_or(Value::Null),
            );
        }
    }
    Some(card)
}

fn build_active_agent_budget_session_card(snapshot: &Value) -> Option<Value> {
    build_active_agent_budget_session_card_from_surface(&snapshot["active_agent_budget"])
}

fn active_agent_online_limit_field(agent: &Value) -> (String, Option<String>) {
    let value = agent["personal_client_limit"]["value_text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("н/д")
        .to_string();
    let tooltip = agent["personal_client_limit"]["tooltip"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            Some("Личный online limit surface для этого агента ещё не materialized.".to_string())
        });
    (value, tooltip)
}

fn active_agent_online_limit_label(agent: &Value) -> &str {
    agent["personal_client_limit"]["label_text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("Лимит клиента сейчас:")
}

fn active_agent_live_pressure_field(agent: &Value) -> Option<(String, String)> {
    let client_live_meter = &agent["client_live_meter"];
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .filter(|value| *value > 0)?;
    let model_context_window = client_live_meter["latest_model_context_window"]
        .as_u64()
        .filter(|value| *value > 0)?;
    let context_used_percent = client_live_meter["context_used_percent"]
        .as_f64()
        .unwrap_or_else(|| (turn_total_tokens as f64 * 100.0) / model_context_window as f64);
    let observed_at = client_live_meter["ended_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp);
    let observed_at_short = client_live_meter["ended_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp_clock);
    let pressure_note = if context_used_percent >= 70.0 {
        "Это giant-thread pressure: почти всё окно клиента уже занято одним live-turn, поэтому 5ч burn сейчас идёт главным образом размером самого запроса."
    } else if context_used_percent >= 50.0 {
        "Это тяжёлый live-turn: заметная часть burn сейчас приходит от размера текущего клиентского запроса, а не только от Amai-side delta."
    } else {
        "Это текущий observed client turn этого агента. Он помогает отличить реальный burn от UI/агрегационного drift."
    };
    let tooltip = format!(
        "Этот ряд показывает последний observed client turn именно этого active agent из rollout token_count.\n- Последний запрос: {} из {}\n- Окно занято: {}\n- Источник: rollout token_count.last_token_usage.total_tokens / model_context_window{}\n- {}\n- Снято из raw token_count: {}",
        format_u64(Some(turn_total_tokens)),
        format_u64(Some(model_context_window)),
        format_percent(Some(context_used_percent)),
        observed_at_short
            .as_ref()
            .map(|stamp| format!(" ({stamp})"))
            .unwrap_or_default(),
        pressure_note,
        observed_at.unwrap_or_else(|| "ещё нет данных".to_string()),
    );
    Some((
        format!(
            "{} из {} · окно занято {}",
            format_u64(Some(turn_total_tokens)),
            format_u64(Some(model_context_window)),
            format_percent(Some(context_used_percent)),
        ),
        tooltip,
    ))
}

fn shared_active_agent_limit(agent_blocks: &[Value]) -> Option<(String, String, Option<String>)> {
    let first = agent_blocks.first()?;
    let label = first["limit_label"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let value = first["limit_value"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if label != "Лимит клиента сейчас:" {
        return None;
    }
    let tooltip = first["limit_tooltip"].as_str().map(str::to_string);
    let same_across_blocks = agent_blocks.iter().all(|block| {
        block["limit_label"].as_str().map(str::trim) == Some(label)
            && block["limit_value"].as_str().map(str::trim) == Some(value)
            && block["limit_tooltip"].as_str().map(str::to_string) == tooltip
    });
    if !same_across_blocks {
        return None;
    }
    Some((label.to_string(), value.to_string(), tooltip))
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

fn live_latency_compare_status_tooltip(
    overall_status: &str,
    hot_assessment: &LiveLatencySliceAssessment,
    cold_assessment: &LiveLatencySliceAssessment,
) -> Option<String> {
    let mut reasons = Vec::new();
    if hot_assessment.status != "pass" {
        reasons.push(format!("Повторный запрос: {}", hot_assessment.note));
    }
    if cold_assessment.status != "pass" {
        reasons.push(format!("Новый запрос: {}", cold_assessment.note));
    }
    status_reason_tooltip(
        overall_status,
        reasons,
        "Живой срез ещё не даёт устойчивой картины по обоим пользовательским режимам. Строгие проверочные прогоны показываются отдельно.",
    )
}

#[allow(dead_code)]
fn build_current_session_hero_card(snapshot: &Value) -> Value {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    let current_session = &report["current_session"];
    let current_session_statement = &report["statement_previews"]["current_session"];
    let client_live_meter = &report["client_live_meter"];
    let current_session_alignment = &current_session_statement["client_limit_meter_alignment"];
    let current_session_exact_pair =
        exact_model_token_pair(current_session_statement, current_session_alignment);
    let session_events_total = current_session["events_total"].as_u64().unwrap_or(0);
    let session_events = current_session["counted_events"].as_u64().unwrap_or(0);
    let session_saved = current_session_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| current_session["verified_effective_saved_tokens"].as_i64());
    let session_percent = current_session_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| current_session["verified_effective_savings_pct"].as_f64());
    let session_started = current_session["started_at_epoch_ms"].as_u64();
    let session_ended = current_session["ended_at_epoch_ms"].as_u64();
    let session_raw_baseline = current_session["total_naive_tokens"]
        .as_u64()
        .or_else(|| current_session["baseline_tokens"].as_u64());
    let session_raw_delivered = current_session["total_context_tokens"]
        .as_u64()
        .or_else(|| current_session["delivered_tokens"].as_u64());
    let session_raw_percent = current_session["effective_savings_pct"].as_f64();
    let session_recovery = current_session["median_recovery_tokens"].as_f64();
    let session_answer_rate = current_session["answer_like_rate"].as_f64();
    let session_answer_count = current_session["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let session_answer_percent = current_session["verified_answer_like_savings_pct"].as_f64();

    let mut session_note = if session_events > 0 {
        format!(
            "Текущая сессия — это непрерывная работа без паузы дольше 30 минут. Длительность: {}. В главный итог уже вошли {} из {} живых запросов. Проверенная экономия по ним: {}. {}",
            elapsed_since_epoch_label(session_started, session_ended),
            format_u64(Some(session_events)),
            format_u64(Some(session_events_total)),
            format_percent(session_percent),
            recovery_sentence(session_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от всей выборки, экономия по ним: {}.",
            format_count_with_word(session_answer_count, "случай", "случая", "случаев"),
            format_percent(session_answer_rate),
            format_percent(session_answer_percent)
        ) + if current_session_exact_pair.is_some() {
            " Нижние строки ниже разделяют внутренний retrieval-KPI Amai и exact model-meter breakdown."
        } else {
            " Подробные цифры по главному итогу, всему живому потоку и тому, что пока вне главного итога, вынесены в нижние строки."
        }
    } else if session_events_total > 0 {
        format!(
            "В этой сессии уже есть Amai-запросы: {}. Но пока ни один случай ещё не подтвердился как полезный без потери качества. Поэтому главный итог по сессии ещё не накоплен.",
            format_u64(Some(session_events_total)),
        ) + &format!(
            " {} {}",
            raw_savings_sentence(
                session_raw_baseline,
                session_raw_delivered,
                session_raw_percent
            ),
            client_budget_disclaimer()
        )
    } else {
        "В текущей непрерывной сессии Amai ещё не накопил ни одного учтённого запроса, поэтому реальную экономию пока рано показывать.".to_string()
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(current_session_alignment) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(current_session_statement, current_session_alignment)
    {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(current_session_alignment) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_live_turn_exact_pair = live_turn_exact_pair(
        current_session,
        client_live_meter,
        current_session_exact_pair,
    );
    let session_live_turn_exact_pair =
        current_live_turn_exact_pair(&report["current_live_turn"]).or(session_live_turn_exact_pair);
    let restore_context = &snapshot["latest_repo_working_state_restore"]["working_state_restore"];
    let client_budget_target_percent =
        client_budget_target_percent_from_inputs(report, restore_context);
    let client_budget_target_active = client_budget_target_active(client_budget_target_percent);
    let client_budget_target_percent_f64 =
        client_budget_target_percent_f64(client_budget_target_percent);
    let host_context_compaction = latest_host_context_compaction_payload(report, restore_context);
    let host_context_compaction_stage =
        host_context_compaction_stage_from_payload(&host_context_compaction);
    let (
        host_current_thread_control,
        _host_current_thread_control_effect,
        same_thread_compaction_preferred,
    ) = selected_host_current_thread_control_state(
        report,
        restore_context,
        client_live_meter,
        &host_context_compaction,
    );
    let session_rotate_bundle = restore_context.is_object().then(|| {
        working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
            restore_context["project"]["code"].as_str(),
            restore_context["namespace"]["code"].as_str(),
            restore_context["project"]["repo_root"].as_str(),
            restore_context["execctl_resume_state"]
                .as_str()
                .is_some_and(|value| value != "clear"),
            restore_context["current_goal"].as_str(),
            restore_context["next_step"].as_str(),
            host_context_compaction_stage,
            same_thread_compaction_preferred,
            host_current_thread_control["thread_id"].as_str(),
            host_current_thread_control["command_id"].as_str(),
        )
    });
    if let Some(sentence) =
        client_live_meter_note_sentence(client_live_meter, session_live_turn_exact_pair)
    {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_full_turn_savings_pct =
        full_turn_savings_pct_from_live_meter(client_live_meter, session_live_turn_exact_pair);
    let session_client_turn_pressure = client_turn_pressure_guard_with_target(
        client_live_meter,
        session_live_turn_exact_pair,
        &report["client_limit_hourly_burn"],
        &report["current_live_turn"],
        client_budget_target_percent,
    );
    if let Some(sentence) = client_turn_pressure_note_sentence_for_preference(
        session_client_turn_pressure,
        same_thread_compaction_preferred,
    ) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_boundary_pressure =
        continuity_boundary_pressure(current_session, current_session_alignment);
    if let Some((boundary_tokens, strict_tokens)) = session_boundary_pressure {
        session_note.push(' ');
        session_note.push_str(&continuity_boundary_pressure_sentence(
            boundary_tokens,
            strict_tokens,
        ));
    }
    let mut session_rows =
        current_session_lane_rows(current_session, current_session_exact_pair.is_some());
    if let Some(row) =
        client_full_turn_savings_metric_row(client_live_meter, session_live_turn_exact_pair)
    {
        session_rows.push(row);
    }
    session_rows.push(model_token_savings_metric_row(
        current_session_statement,
        current_session_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_live_context_metric_row(client_live_meter) {
        session_rows.push(row);
    }
    if let Some(row) = client_live_limit_metric_row(client_live_meter) {
        session_rows.push(row);
    }
    if let Some(row) = client_turn_pressure_metric_row(
        session_client_turn_pressure,
        session_rotate_bundle.as_ref(),
        same_thread_compaction_preferred,
    ) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    let session_status = if let Some(guard) = session_client_turn_pressure {
        guard.severity
    } else if session_boundary_pressure.is_some_and(|(boundary_tokens, strict_tokens)| {
        continuity_boundary_pressure_is_alert(session_saved, boundary_tokens, strict_tokens)
    }) {
        "alert"
    } else if client_budget_target_active
        && session_full_turn_savings_pct
            .is_some_and(|value| value < client_budget_target_percent_f64)
    {
        "alert"
    } else {
        savings_status(session_saved, session_events, session_events_total)
    };
    let mut session_card = card_with_rows(
        "Экономия токенов за текущую сессию",
        session_full_turn_savings_pct
            .map(|value| format_percent(Some(value)))
            .unwrap_or_else(|| "не доказано".to_string()),
        session_note,
        session_status,
        None,
        Some("Эта карточка показывает, сколько токенов Amai сэкономил в текущем непрерывном заходе работы. Новый заход начинается после паузы дольше 30 минут. В главный итог попадают только те живые запросы, которые уже подтвердились как полезные без потери качества. Нижние строки нужны, чтобы показать разницу между главным итогом и всем живым потоком.".to_string()),
        session_rows,
    );
    if let Some(guard) = session_client_turn_pressure {
        session_card = with_status_label(
            session_card,
            client_turn_pressure_display_status_label(
                guard.status_label,
                same_thread_compaction_preferred,
            ),
        );
        session_card = with_status_tooltip(
            session_card,
            &client_turn_pressure_tooltip(
                guard,
                session_rotate_bundle.as_ref(),
                same_thread_compaction_preferred,
            ),
        );
    } else if session_full_turn_savings_pct.is_none()
        && current_session_client_live_meter_available(client_live_meter)
    {
        session_card = with_status(session_card, "alert");
        session_card = with_status_label(session_card, "реальная экономия не доказана");
        session_card = with_status_tooltip(
            session_card,
            "Статус требует внимания по следующим причинам:\n- Для текущего живого turn ещё нет доказанной same-turn пары `без Amai / с Amai`.\n- Значит реальную экономию на полной шкале клиента пока нельзя честно показать числом.\n- Пока эта пара не materialized, нижняя строка про учтённую часть остаётся внутренним Amai-срезом, а не полным client spend.\n- Чтобы получить реальную экономию, нужно быстрее фиксировать exact pair на коротком live turn и для этого сначала сжать текущий giant thread через same-thread compact window, а не расширять его новыми ходами.",
        );
    } else if let Some(full_turn_savings_pct) = session_full_turn_savings_pct
        .filter(|value| client_budget_target_active && *value < client_budget_target_percent_f64)
    {
        session_card = with_status_label(
            session_card,
            &client_budget_target_alert_label(client_budget_target_percent),
        );
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- Реальная экономия на полной шкале клиента сейчас всего {}.\n- {}\n- Значит текущий thread пока жжёт почти весь полный client turn/context, а Amai экономит только малую долю.\n- Чтобы реально улучшить картину без потери точности, нужно дальше уменьшать полный размер turn и жёстко удерживать same-thread compact surface, чтобы следующий exact pair materialized на коротком live turn.",
                format_percent(Some(full_turn_savings_pct)),
                client_budget_target_sentence(client_budget_target_percent)
            ),
        );
    } else if let Some((boundary_tokens, strict_tokens)) =
        session_boundary_pressure.filter(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(session_saved, *boundary_tokens, *strict_tokens)
        })
    {
        session_card = with_status_label(session_card, "burn в continuity startup");
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В этой сессии savings-KPI пока не показывает положительную подтверждённую экономию.\n- При этом observed continuity startup уже сжёг {} токенов.\n- Strict same-meter slice по клиентскому запросу пока даёт только {} токенов.\n- Значит лимит сейчас уходит главным образом в continuity restore, а не в retrieval/workflow effect.",
                format_u64(Some(boundary_tokens)),
                format_u64(Some(strict_tokens))
            ),
        );
    } else if session_events_total > 0 && session_events == 0 {
        session_card = with_status_tooltip(
            session_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В этой сессии уже были живые запросы.\n- Но пока ни один из них ещё не подтвердился как полезный без потери качества.\n- Как только появится первый такой случай, главный итог этой карточки начнёт считаться.",
        );
    } else if session_events > 0 && session_saved.unwrap_or_default() < 0 {
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В подтверждённой части текущей сессии экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai вышел тяжелее обычного пути без Amai.\n- Нижние строки со всем живым потоком показаны отдельно и не отменяют этот итог.",
                format_signed_count(session_saved)
            ),
        );
    }
    if session_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) =
            exact_pair_card_status_override(current_session_alignment)
        {
            session_card = with_status(session_card, status);
            session_card = with_status_label(session_card, label);
            session_card = with_status_tooltip(session_card, &tooltip);
        }
    }
    session_card
}

fn build_hero_cards(snapshot: &Value) -> Vec<Value> {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    let current_session = &report["current_session"];
    let lifetime = &report["lifetime"];
    let rolling_window = &report["rolling_window"];
    let current_session_statement = &report["statement_previews"]["current_session"];
    let rolling_window_statement = &report["statement_previews"]["rolling_window"];
    let lifetime_statement = &report["statement_previews"]["lifetime"];
    let lifetime_statement_export = &report["statement_export_previews"]["lifetime"];
    let client_live_meter = &report["client_live_meter"];
    let current_session_alignment = &current_session_statement["client_limit_meter_alignment"];
    let rolling_window_alignment = &rolling_window_statement["client_limit_meter_alignment"];
    let lifetime_alignment = &lifetime_statement["client_limit_meter_alignment"];
    let current_session_exact_pair =
        exact_model_token_pair(current_session_statement, current_session_alignment);
    let rolling_window_exact_pair =
        exact_model_token_pair(rolling_window_statement, rolling_window_alignment);
    let lifetime_exact_pair = exact_model_token_pair(lifetime_statement, lifetime_alignment);
    let session_events_total = current_session["events_total"].as_u64().unwrap_or(0);
    let session_events = current_session["counted_events"].as_u64().unwrap_or(0);
    let session_saved = current_session_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| current_session["verified_effective_saved_tokens"].as_i64());
    let session_percent = current_session_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| current_session["verified_effective_savings_pct"].as_f64());
    let session_started = current_session["started_at_epoch_ms"].as_u64();
    let session_ended = current_session["ended_at_epoch_ms"].as_u64();
    let session_raw_baseline = current_session["total_naive_tokens"]
        .as_u64()
        .or_else(|| current_session["baseline_tokens"].as_u64());
    let session_raw_delivered = current_session["total_context_tokens"]
        .as_u64()
        .or_else(|| current_session["delivered_tokens"].as_u64());
    let session_raw_percent = current_session["effective_savings_pct"].as_f64();
    let lifetime_events_total = lifetime["events_total"].as_u64().unwrap_or(0);
    let lifetime_events = lifetime["counted_events"].as_u64().unwrap_or(0);
    let lifetime_saved = lifetime_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| lifetime["verified_effective_saved_tokens"].as_i64());
    let lifetime_percent = lifetime_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| lifetime["verified_effective_savings_pct"].as_f64());
    let lifetime_started = lifetime["started_at_epoch_ms"].as_u64();
    let lifetime_ended = lifetime["ended_at_epoch_ms"].as_u64();
    let rolling_events_total = rolling_window["events_total"].as_u64().unwrap_or(0);
    let rolling_events = rolling_window["counted_events"].as_u64().unwrap_or(0);
    let rolling_saved = rolling_window_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| rolling_window["verified_effective_saved_tokens"].as_i64());
    let rolling_percent = rolling_window_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| rolling_window["verified_effective_savings_pct"].as_f64());
    let rolling_started = rolling_window["started_at_epoch_ms"].as_u64();
    let rolling_ended = rolling_window["ended_at_epoch_ms"].as_u64();
    let rolling_window_label = report["profile"]["display_name"]
        .as_str()
        .unwrap_or("рабочее окно");
    let rolling_recovery = rolling_window["median_recovery_tokens"].as_f64();
    let session_recovery = current_session["median_recovery_tokens"].as_f64();
    let lifetime_recovery = lifetime["median_recovery_tokens"].as_f64();
    let session_answer_rate = current_session["answer_like_rate"].as_f64();
    let session_answer_count = current_session["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let session_answer_percent = current_session["verified_answer_like_savings_pct"].as_f64();
    let rolling_answer_rate = rolling_window["answer_like_rate"].as_f64();
    let rolling_answer_count = rolling_window["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let rolling_answer_percent = rolling_window["verified_answer_like_savings_pct"].as_f64();
    let lifetime_answer_rate = lifetime["answer_like_rate"].as_f64();
    let lifetime_answer_count = lifetime["answer_like_counted_events"].as_u64().unwrap_or(0);
    let lifetime_answer_percent = lifetime["verified_answer_like_savings_pct"].as_f64();

    let mut session_note = if session_events > 0 {
        format!(
            "Текущая сессия — это непрерывная работа без паузы дольше 30 минут. Длительность: {}. В главный итог уже вошли {} из {} живых запросов. Проверенная экономия по ним: {}. {}",
            elapsed_since_epoch_label(session_started, session_ended),
            format_u64(Some(session_events)),
            format_u64(Some(session_events_total)),
            format_percent(session_percent),
            recovery_sentence(session_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от всей выборки, экономия по ним: {}.",
            format_count_with_word(session_answer_count, "случай", "случая", "случаев"),
            format_percent(session_answer_rate),
            format_percent(session_answer_percent)
        ) + if current_session_exact_pair.is_some() {
            " Нижние строки ниже разделяют внутренний retrieval-KPI Amai и exact model-meter breakdown."
        } else {
            " Подробные цифры по главному итогу, всему живому потоку и тому, что пока вне главного итога, вынесены в нижние строки."
        }
    } else if session_events_total > 0 {
        format!(
            "В этой сессии уже есть Amai-запросы: {}. Но пока ни один случай ещё не подтвердился как полезный без потери качества. Поэтому главный итог по сессии ещё не накоплен.",
            format_u64(Some(session_events_total)),
        ) + &format!(
            " {} {}",
            raw_savings_sentence(
                session_raw_baseline,
                session_raw_delivered,
                session_raw_percent
            ),
            client_budget_disclaimer()
        )
    } else {
        "В текущей непрерывной сессии Amai ещё не накопил ни одного учтённого запроса, поэтому реальную экономию пока рано показывать.".to_string()
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(current_session_alignment) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(current_session_statement, current_session_alignment)
    {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(current_session_alignment) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_live_turn_exact_pair = live_turn_exact_pair(
        current_session,
        client_live_meter,
        current_session_exact_pair,
    );
    let session_live_turn_exact_pair =
        current_live_turn_exact_pair(&report["current_live_turn"]).or(session_live_turn_exact_pair);
    let restore_context = &snapshot["latest_repo_working_state_restore"]["working_state_restore"];
    let client_budget_target_percent =
        client_budget_target_percent_from_inputs(report, restore_context);
    let client_budget_target_active = client_budget_target_active(client_budget_target_percent);
    let client_budget_target_percent_f64 =
        client_budget_target_percent_f64(client_budget_target_percent);
    let host_context_compaction = latest_host_context_compaction_payload(report, restore_context);
    let host_context_compaction_stage =
        host_context_compaction_stage_from_payload(&host_context_compaction);
    let (
        host_current_thread_control,
        _host_current_thread_control_effect,
        same_thread_compaction_preferred,
    ) = selected_host_current_thread_control_state(
        report,
        restore_context,
        client_live_meter,
        &host_context_compaction,
    );
    let session_rotate_bundle = restore_context.is_object().then(|| {
        working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
            restore_context["project"]["code"].as_str(),
            restore_context["namespace"]["code"].as_str(),
            restore_context["project"]["repo_root"].as_str(),
            restore_context["execctl_resume_state"]
                .as_str()
                .is_some_and(|value| value != "clear"),
            restore_context["current_goal"].as_str(),
            restore_context["next_step"].as_str(),
            host_context_compaction_stage,
            same_thread_compaction_preferred,
            host_current_thread_control["thread_id"].as_str(),
            host_current_thread_control["command_id"].as_str(),
        )
    });
    if let Some(sentence) =
        client_live_meter_note_sentence(client_live_meter, session_live_turn_exact_pair)
    {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_full_turn_savings_pct =
        full_turn_savings_pct_from_live_meter(client_live_meter, session_live_turn_exact_pair);
    let session_client_turn_pressure = client_turn_pressure_guard_with_target(
        client_live_meter,
        session_live_turn_exact_pair,
        &report["client_limit_hourly_burn"],
        &report["current_live_turn"],
        client_budget_target_percent,
    );
    if let Some(sentence) = client_turn_pressure_note_sentence_for_preference(
        session_client_turn_pressure,
        same_thread_compaction_preferred,
    ) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_boundary_pressure =
        continuity_boundary_pressure(current_session, current_session_alignment);
    if let Some((boundary_tokens, strict_tokens)) = session_boundary_pressure {
        session_note.push(' ');
        session_note.push_str(&continuity_boundary_pressure_sentence(
            boundary_tokens,
            strict_tokens,
        ));
    }
    let mut session_rows =
        current_session_lane_rows(current_session, current_session_exact_pair.is_some());
    if let Some(row) =
        client_full_turn_savings_metric_row(client_live_meter, session_live_turn_exact_pair)
    {
        session_rows.push(row);
    }
    session_rows.push(model_token_savings_metric_row(
        current_session_statement,
        current_session_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_live_context_metric_row(client_live_meter) {
        session_rows.push(row);
    }
    if let Some(row) = client_live_limit_metric_row(client_live_meter) {
        session_rows.push(row);
    }
    if let Some(row) = client_turn_pressure_metric_row(
        session_client_turn_pressure,
        session_rotate_bundle.as_ref(),
        same_thread_compaction_preferred,
    ) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    let session_status = if let Some(guard) = session_client_turn_pressure {
        guard.severity
    } else if session_boundary_pressure.is_some_and(|(boundary_tokens, strict_tokens)| {
        continuity_boundary_pressure_is_alert(session_saved, boundary_tokens, strict_tokens)
    }) {
        "alert"
    } else if client_budget_target_active
        && session_full_turn_savings_pct
            .is_some_and(|value| value < client_budget_target_percent_f64)
    {
        "alert"
    } else {
        savings_status(session_saved, session_events, session_events_total)
    };
    let mut session_card = card_with_rows(
        "Экономия токенов за текущую сессию",
        session_full_turn_savings_pct
            .map(|value| format_percent(Some(value)))
            .unwrap_or_else(|| "не доказано".to_string()),
        session_note,
        session_status,
        None,
        Some("Эта карточка показывает, сколько токенов Amai сэкономил в текущем непрерывном заходе работы. Новый заход начинается после паузы дольше 30 минут. В главный итог попадают только те живые запросы, которые уже подтвердились как полезные без потери качества. Нижние строки нужны, чтобы показать разницу между главным итогом и всем живым потоком.".to_string()),
        session_rows,
    );
    if let Some(guard) = session_client_turn_pressure {
        session_card = with_status_label(
            session_card,
            client_turn_pressure_display_status_label(
                guard.status_label,
                same_thread_compaction_preferred,
            ),
        );
        session_card = with_status_tooltip(
            session_card,
            &client_turn_pressure_tooltip(
                guard,
                session_rotate_bundle.as_ref(),
                same_thread_compaction_preferred,
            ),
        );
    } else if session_full_turn_savings_pct.is_none()
        && current_session_client_live_meter_available(client_live_meter)
    {
        session_card = with_status(session_card, "alert");
        session_card = with_status_label(session_card, "реальная экономия не доказана");
        session_card = with_status_tooltip(
            session_card,
            "Статус требует внимания по следующим причинам:\n- Для текущего живого turn ещё нет доказанной same-turn пары `без Amai / с Amai`.\n- Значит реальную экономию на полной шкале клиента пока нельзя честно показать числом.\n- Пока эта пара не materialized, нижняя строка про учтённую часть остаётся внутренним Amai-срезом, а не полным client spend.\n- Чтобы получить реальную экономию, нужно быстрее фиксировать exact pair на коротком live turn и для этого сначала сжать текущий giant thread через same-thread compact window, а не расширять его новыми ходами.",
        );
    } else if let Some(full_turn_savings_pct) = session_full_turn_savings_pct
        .filter(|value| client_budget_target_active && *value < client_budget_target_percent_f64)
    {
        session_card = with_status_label(
            session_card,
            &client_budget_target_alert_label(client_budget_target_percent),
        );
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- Реальная экономия на полной шкале клиента сейчас всего {}.\n- {}\n- Значит текущий thread пока жжёт почти весь полный client turn/context, а Amai экономит только малую долю.\n- Чтобы реально улучшить картину без потери точности, нужно дальше уменьшать полный размер turn и жёстко удерживать same-thread compact surface, чтобы следующий exact pair materialized на коротком live turn.",
                format_percent(Some(full_turn_savings_pct)),
                client_budget_target_sentence(client_budget_target_percent)
            ),
        );
    } else if let Some((boundary_tokens, strict_tokens)) =
        session_boundary_pressure.filter(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(session_saved, *boundary_tokens, *strict_tokens)
        })
    {
        session_card = with_status_label(session_card, "burn в continuity startup");
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В этой сессии savings-KPI пока не показывает положительную подтверждённую экономию.\n- При этом observed continuity startup уже сжёг {} токенов.\n- Strict same-meter slice по клиентскому запросу пока даёт только {} токенов.\n- Значит лимит сейчас уходит главным образом в continuity restore, а не в retrieval/workflow effect.",
                format_u64(Some(boundary_tokens)),
                format_u64(Some(strict_tokens))
            ),
        );
    } else if session_events_total > 0 && session_events == 0 {
        session_card = with_status_tooltip(
            session_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В этой сессии уже были живые запросы.\n- Но пока ни один из них ещё не подтвердился как полезный без потери качества.\n- Как только появится первый такой случай, главный итог этой карточки начнёт считаться.",
        );
    } else if session_events > 0 && session_saved.unwrap_or_default() < 0 {
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В подтверждённой части текущей сессии экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai вышел тяжелее обычного пути без Amai.\n- Нижние строки со всем живым потоком показаны отдельно и не отменяют этот итог.",
                format_signed_count(session_saved)
            ),
        );
    }
    if session_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) =
            exact_pair_card_status_override(current_session_alignment)
        {
            session_card = with_status(session_card, status);
            session_card = with_status_label(session_card, label);
            session_card = with_status_tooltip(session_card, &tooltip);
        }
    }

    let mut rolling_note = if rolling_events > 0 {
        format!(
            "Это текущее рабочее окно профиля {} за {}. В главный итог окна уже вошли {} из {} живых запросов. Проверенная экономия: {}. {}",
            rolling_window_label,
            elapsed_since_epoch_label(rolling_started, rolling_ended),
            format_u64(Some(rolling_events)),
            format_u64(Some(rolling_events_total)),
            format_percent(rolling_percent),
            recovery_sentence(rolling_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от окна, экономия по ним: {}.",
            format_count_with_word(rolling_answer_count, "случай", "случая", "случаев"),
            format_percent(rolling_answer_rate),
            format_percent(rolling_answer_percent)
        )
    } else if rolling_events_total > 0 {
        format!(
            "В текущем рабочем окне уже есть Amai-запросы: {}. Но пока ни один случай ещё не подтвердился как полезный без потери качества. Поэтому итог по окну пока рано считать устойчивым.",
            format_u64(Some(rolling_events_total))
        )
    } else {
        "В текущем рабочем окне Amai ещё не накопил учтённых запросов, поэтому здесь пока нет подтверждённой живой статистики.".to_string()
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(rolling_window_alignment) {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(rolling_window_statement, rolling_window_alignment)
    {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(rolling_window_alignment) {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    let rolling_historical_startup_drag = historical_startup_drag(
        current_session_exact_pair,
        rolling_window_exact_pair,
        current_session,
        rolling_window,
    );
    if let Some(sentence) = historical_startup_drag_note_sentence(rolling_historical_startup_drag) {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    let rolling_boundary_pressure =
        continuity_boundary_pressure(rolling_window, rolling_window_alignment);
    if let Some((boundary_tokens, strict_tokens)) = rolling_boundary_pressure {
        rolling_note.push(' ');
        rolling_note.push_str(&continuity_boundary_pressure_sentence(
            boundary_tokens,
            strict_tokens,
        ));
    }
    let mut rolling_rows = Vec::new();
    rolling_rows.push(model_token_savings_metric_row(
        rolling_window_statement,
        rolling_window_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = historical_startup_drag_metric_row(rolling_historical_startup_drag) {
        rolling_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    let rolling_status =
        if rolling_boundary_pressure.is_some_and(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(rolling_saved, boundary_tokens, strict_tokens)
        }) {
            "alert"
        } else {
            savings_status(rolling_saved, rolling_events, rolling_events_total)
        };
    let mut rolling_card = card_with_rows(
        "Экономия токенов за рабочее окно",
        format_signed_count(rolling_saved),
        rolling_note,
        rolling_status,
        None,
        Some(format!(
            "Эта карточка показывает не одну сессию, а текущее скользящее рабочее окно профиля {}. Окно может захватывать несколько заходов работы подряд и нужно для недавнего тренда, а не только для последнего непрерывного сеанса. В главный итог здесь тоже попадают только те живые запросы, которые уже подтвердились как полезные без потери качества.",
            rolling_window_label
        )),
        rolling_rows,
    );
    if let Some((boundary_tokens, strict_tokens)) =
        rolling_boundary_pressure.filter(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(rolling_saved, *boundary_tokens, *strict_tokens)
        })
    {
        rolling_card = with_status_label(rolling_card, "burn в continuity startup");
        rolling_card = with_status_tooltip(
            rolling_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В рабочем окне savings-KPI пока не показывает положительную подтверждённую экономию.\n- При этом observed continuity startup уже сжёг {} токенов.\n- Strict same-meter slice по клиентскому запросу пока даёт только {} токенов.\n- Значит недавний live budget уходит главным образом в continuity restore, а не в retrieval/workflow effect.",
                format_u64(Some(boundary_tokens)),
                format_u64(Some(strict_tokens))
            ),
        );
    } else if let Some(drag) = rolling_historical_startup_drag {
        rolling_card = with_status_label(rolling_card, "исторический startup drag");
        rolling_card = with_status_tooltip(
            rolling_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- Свежая текущая сессия уже profitable и не объясняет минус окна.\n- Вне текущей сессии в рабочем окне остаётся исторический continuity-startup cohort: без Amai было {}, с Amai стало {}, это +{} токенов к расходу.\n- Из observed continuity-restore в окне {} токенов приходятся на этот старший хвост, а на текущую сессию — только {}.",
                format_u64(Some(drag.older_without_amai_tokens)),
                format_u64(Some(drag.older_with_amai_tokens)),
                format_u64(Some(drag.older_delta_tokens as u64)),
                format_u64(Some(drag.older_continuity_tokens)),
                format_u64(Some(drag.current_continuity_tokens))
            ),
        );
    } else if rolling_events_total > 0 && rolling_events == 0 {
        rolling_card = with_status_tooltip(
            rolling_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В текущем рабочем окне уже есть живые запросы.\n- Но пока ни один случай ещё не подтвердился как полезный без потери качества.\n- Поэтому окно ещё копит подтверждённую выборку.",
        );
    } else if rolling_events > 0 && rolling_saved.unwrap_or_default() < 0 {
        rolling_card = with_status_tooltip(
            rolling_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В подтверждённой части рабочего окна экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai вышел тяжелее обычного пути без Amai.",
                format_signed_count(rolling_saved)
            ),
        );
    }
    if rolling_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) =
            exact_pair_card_status_override(rolling_window_alignment)
        {
            rolling_card = with_status(rolling_card, status);
            rolling_card = with_status_label(rolling_card, label);
            rolling_card = with_status_tooltip(rolling_card, &tooltip);
        }
    }

    let mut lifetime_note = if lifetime_events > 0 {
        format!(
            "Это накопительный итог с первого записанного запроса Amai в этой установке за {}. В главный итог уже вошли {} из {} живых запросов. Проверенная экономия: {}. {}",
            elapsed_since_epoch_label(lifetime_started, lifetime_ended),
            format_u64(Some(lifetime_events)),
            format_u64(Some(lifetime_events_total)),
            format_percent(lifetime_percent),
            recovery_sentence(lifetime_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от всей выборки, экономия по ним: {}.",
            format_count_with_word(lifetime_answer_count, "случай", "случая", "случаев"),
            format_percent(lifetime_answer_rate),
            format_percent(lifetime_answer_percent)
        )
    } else if lifetime_events_total > 0 {
        format!(
            "После установки уже накоплены Amai-запросы: {}. Но пока ни один случай ещё не подтвердился как полезный без потери качества. Поэтому главный итог пока не считается надёжным.",
            format_u64(Some(lifetime_events_total)),
        )
    } else {
        "После установки Amai ещё не накопил учтённых запросов, поэтому здесь пока нет итоговой живой статистики.".to_string()
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(lifetime_alignment) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(lifetime_statement, lifetime_alignment)
    {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(lifetime_alignment) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) = reviewed_frozen_debt_export_note_sentence(lifetime_statement_export) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) = historical_frozen_debt_note_sentence(
        current_session_alignment,
        rolling_window_alignment,
        lifetime_alignment,
    ) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    let mut lifetime_rows = Vec::new();
    lifetime_rows.push(model_token_savings_metric_row(
        lifetime_statement,
        lifetime_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = historical_frozen_debt_metric_row(
        current_session_alignment,
        rolling_window_alignment,
        lifetime_alignment,
    ) {
        lifetime_rows.push(row);
    }
    if let Some(row) = reviewed_frozen_debt_export_metric_row(lifetime_statement_export) {
        lifetime_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    let mut lifetime_card = card_with_rows(
        "Экономия токенов за всё время записи",
        format_signed_count(lifetime_saved),
        lifetime_note,
        savings_status(lifetime_saved, lifetime_events, lifetime_events_total),
        None,
        Some("Эта карточка показывает накопительный итог с первого записанного запроса Amai в текущей установке. Это не процент от лимита чата и не вся история всех внешних клиентов навсегда. В главный итог попадают только те живые запросы, которые уже подтвердились как полезные без потери качества; проверочные прогоны и другой инженерный трафик сюда не подмешиваются.".to_string()),
        lifetime_rows,
    );
    if lifetime_events_total > 0 && lifetime_events == 0 {
        lifetime_card = with_status_tooltip(
            lifetime_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В истории уже есть живые запросы.\n- Но пока ещё нет ни одного подтверждённого случая без потери качества.\n- Поэтому накопительный итог ещё не может считаться надёжным.",
        );
    } else if lifetime_events > 0 && lifetime_saved.unwrap_or_default() < 0 {
        lifetime_card = with_status_tooltip(
            lifetime_card,
            &format!(
                "Статус требует внимания по следующим причинам:\n- В подтверждённой части всей истории экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai пока выходит тяжелее обычного пути без Amai.",
                format_signed_count(lifetime_saved)
            ),
        );
    }
    if lifetime_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) = exact_pair_card_status_override(lifetime_alignment)
        {
            lifetime_card = with_status(lifetime_card, status);
            lifetime_card = with_status_label(lifetime_card, label);
            lifetime_card = with_status_tooltip(lifetime_card, &tooltip);
        }
    }

    if let Some(active_agent_budget_card) = build_active_agent_budget_session_card(snapshot) {
        session_card = active_agent_budget_card;
    }

    vec![
        compact_token_hero_card(session_card),
        compact_token_hero_card(rolling_card),
        compact_token_hero_card(lifetime_card),
    ]
}

fn compact_token_hero_card(mut card: Value) -> Value {
    if matches!(
        card["presentation_variant"].as_str(),
        Some(
            "active_agent_budget_v1"
                | "active_agent_budget_minimal_v2"
                | "active_agent_budget_grouped_v3"
        )
    ) {
        return card;
    }
    let title = card["title"].as_str().unwrap_or_default().to_string();
    if let Some(rows) = card["rows"].as_array_mut() {
        let allowed = truth_only_token_card_labels(&title);
        rows.retain(|row| {
            row["label"]
                .as_str()
                .is_some_and(|label| allowed.iter().any(|allowed_label| label == *allowed_label))
        });
        for row in rows {
            if let Some(label) = row["label"].as_str() {
                row["label"] =
                    Value::String(humanize_token_card_row_label(&title, label).to_string());
            }
            if let (Some(label), Some(value)) = (row["label"].as_str(), row["value"].as_str()) {
                row["value"] =
                    Value::String(humanize_token_card_row_value(&title, label, value).to_string());
            }
        }
    }
    if let Some(source_label) = truth_only_token_card_source_label(&card) {
        card["source_label"] = Value::String(source_label);
    }
    card["note"] = Value::String(truth_only_token_card_note(&card));
    if let Some(title_tooltip) = truth_only_token_card_title_tooltip(&title) {
        card["title_tooltip"] = Value::String(title_tooltip);
    }
    card
}

fn truth_only_token_card_labels(title: &str) -> &'static [&'static str] {
    match title {
        "Экономия токенов за текущую сессию" => &[
            "Amai в полном live-turn",
            "Экономия токенов модели",
            "Главный драйвер exact-пары",
            "Совпадение с реальным лимитом",
            "Последний запрос клиента",
            "Лимит клиента сейчас",
            "Последний observed лимит клиента",
            "Следующее действие",
        ],
        "Экономия токенов за рабочее окно" => &[
            "Экономия токенов модели",
            "Совпадение с реальным лимитом",
            "Исторический startup-хвост",
            "Следующее действие",
        ],
        "Экономия токенов за всё время записи" => &[
            "Экономия токенов модели",
            "Совпадение с реальным лимитом",
            "Исторический frozen debt",
            "Review-only export",
        ],
        _ => &[],
    }
}

fn humanize_token_card_row_label<'a>(title: &str, label: &'a str) -> &'a str {
    match (title, label) {
        ("Экономия токенов за текущую сессию", "Amai в полном live-turn") => {
            "Экономия на реальной шкале"
        }
        ("Экономия токенов за текущую сессию", "Экономия токенов модели")
        | ("Экономия токенов за рабочее окно", "Экономия токенов модели")
        | ("Экономия токенов за всё время записи", "Экономия токенов модели") => {
            "Экономия на учтённой части"
        }
        ("Экономия токенов за текущую сессию", "Главный драйвер exact-пары") => {
            "Что именно посчитано"
        }
        (_, "Совпадение с реальным лимитом") => "Точность учтённой части",
        (_, "Последний запрос клиента") => "Последний запрос в модель",
        (_, "Исторический startup-хвост") => "Хвост от прошлых стартов",
        (_, "Исторический frozen debt") => "Исторический долг точности",
        (_, "Review-only export") => "Отчёт для ручной сверки",
        _ => label,
    }
}

fn humanize_token_card_row_value(title: &str, label: &str, value: &str) -> String {
    match (title, label) {
        ("Экономия токенов за текущую сессию", "Экономия на реальной шкале") => {
            humanize_full_turn_savings_value(value)
        }
        ("Экономия токенов за текущую сессию", "Экономия на учтённой части")
        | ("Экономия токенов за рабочее окно", "Экономия на учтённой части")
        | ("Экономия токенов за всё время записи", "Экономия на учтённой части") => {
            humanize_tracked_slice_savings_value(value)
        }
        (_, "Точность учтённой части") => {
            humanize_tracked_slice_exactness_value(value)
        }
        (_, "Исторический долг точности") => {
            if let Some((_, rows)) = value.rsplit_once(", ") {
                return format!(
                    "старый исторический хвост: {}",
                    humanize_history_row_count(rows)
                );
            }
            if let Some((_, rows)) = value.rsplit_once(": ") {
                return format!(
                    "старый исторический хвост: {}",
                    humanize_history_row_count(rows)
                );
            }
            "старый исторический хвост".to_string()
        }
        (_, "Отчёт для ручной сверки") => {
            if let Some((_, rows)) = value.rsplit_once(": ") {
                return format!(
                    "есть отдельный отчёт для ручной сверки: {}",
                    humanize_review_row_count(rows)
                );
            }
            "есть отдельный отчёт для ручной сверки".to_string()
        }
        _ => value.to_string(),
    }
}

fn humanize_full_turn_savings_value(value: &str) -> String {
    let normalized = value.replace("delta ", "экономия ");
    if let Some((pct, rest)) = normalized.split_once(": ") {
        if pct.trim_start().starts_with('-') {
            return format!(
                "На полной шкале Amai пока добавил расход {}: {}",
                pct,
                rest.replace("экономия -", "перерасход ")
            );
        }
        return format!("На полной шкале Amai сэкономил {}: {rest}", pct);
    }
    normalized
}

fn humanize_tracked_slice_savings_value(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("Предварительный учтённый same-meter срез: ")
    {
        return format!("На учтённой части пока предварительно: {rest}");
    }
    if let Some(rest) = value.strip_prefix("Учтённый same-meter срез: ") {
        return format!("На учтённой части: {rest}");
    }
    if let Some(rest) = value.strip_prefix("Точного процента пока нет; ") {
        return format!("По полной шкале точного процента пока нет; на учтённой части {rest}");
    }
    value.to_string()
}

fn humanize_tracked_slice_exactness_value(value: &str) -> String {
    if value == "цифра точная: полностью совпадает со шкалой лимита модели"
    {
        return "учтённая часть посчитана точно по той же шкале клиента".to_string();
    }
    if let Some(rest) = value.strip_prefix("цифра пока не полностью точная: ")
    {
        return format!("учтённая часть пока не сведена полностью: {rest}");
    }
    if let Some(rest) = value.strip_prefix("цифра пока предварительная: ") {
        return format!("учтённая часть пока предварительная: {rest}");
    }
    value.to_string()
}

fn humanize_history_row_count(value: &str) -> String {
    value.replace(" rows", " строк")
}

fn humanize_review_row_count(value: &str) -> String {
    value
        .replace(" irrecoverable rows", " строк без восстановления")
        .replace(" rows", " строк")
}

fn truth_only_token_card_title_tooltip(title: &str) -> Option<String> {
    let text = match title {
        "Экономия токенов за текущую сессию" => {
            "Показывает только проверяемые цифры по текущей сессии: реальную долю Amai на полной живой шкале turn, текущий лимит клиента и точность учтённой части."
        }
        "Экономия токенов за рабочее окно" => {
            "Показывает только проверяемые цифры по рабочему окну. Процент здесь относится к подтверждённой учтённой части, а не ко всему полному расходу модели за окно."
        }
        "Экономия токенов за всё время записи" => {
            "Показывает только подтверждённые цифры за всё время записи. Процент здесь относится к подтверждённой учтённой части, а старый исторический хвост вынесен отдельно."
        }
        _ => return None,
    };
    Some(text.to_string())
}

fn truth_only_token_card_source_label(card: &Value) -> Option<String> {
    let title = card["title"].as_str()?;
    let source = match title {
        "Экономия токенов за текущую сессию" => {
            "Источник: живая шкала клиента из rollout token_count и отдельно сведённая учтённая часть Amai по strict same-meter компонентам."
        }
        "Экономия токенов за рабочее окно" => {
            "Источник: подтверждённая учтённая часть окна и подтверждённый хвост прошлых стартов. Это не весь полный расход клиента за окно."
        }
        "Экономия токенов за всё время записи" => {
            "Источник: подтверждённая учтённая история плюс отдельно отмеченный старый долг точности. Это не полный raw spend всей истории."
        }
        _ => return None,
    };
    Some(source.to_string())
}

fn truth_only_token_card_note(card: &Value) -> String {
    let title = card["title"].as_str().unwrap_or_default();
    let status_label = card["status_label"]
        .as_str()
        .unwrap_or(card["status"].as_str().unwrap_or("неизвестно"));
    match title {
        "Экономия токенов за текущую сессию" => {
            match card["value"].as_str() {
                Some("не доказано") => format!(
                    "Короткая карточка только с проверяемыми цифрами по текущей сессии: реальная экономия на полной шкале клиента пока не доказана, ниже остаётся только точная учтённая часть. Единственный процент, который должен напрямую совпадать с замедлением шкалы VS Code, живёт в строке «Экономия на реальной шкале» и показывается только после exact full-turn pair. Строка «Экономия на учтённой части» относится только к strict same-meter срезу уже учтённых компонентов; если она помечена как preliminary, это ещё не вся сессия. Статус: {status_label}."
                ),
                _ => format!(
                    "Короткая карточка только с проверяемыми цифрами по текущей сессии: сверху реальная доля Amai на полной шкале текущего turn, ниже точность учтённой части. Единственный процент, который должен напрямую совпадать с замедлением шкалы VS Code, живёт в строке «Экономия на реальной шкале». Строка «Экономия на учтённой части» относится только к strict same-meter срезу уже учтённых компонентов; если она помечена как preliminary, это ещё не вся сессия. Статус: {status_label}."
                ),
            }
        }
        "Экономия токенов за рабочее окно" => {
            format!(
                "Короткая карточка только с проверяемыми цифрами по рабочему окну. Процент здесь относится к подтверждённой учтённой части, а не ко всему полному расходу модели за окно. Статус: {status_label}."
            )
        }
        "Экономия токенов за всё время записи" => {
            format!(
                "Короткая карточка только с подтверждёнными цифрами за всё время записи. Процент здесь относится к подтверждённой учтённой части, а старый долг точности вынесен отдельно. Статус: {status_label}."
            )
        }
        _ => return card["note"].as_str().unwrap_or_default().to_string(),
    }
}

fn build_machine_cards(
    snapshot: &Value,
    machine: Option<&MachineSummary>,
    install_state: Option<&dashboard_context::InstallState>,
) -> Vec<Value> {
    let mut cards = Vec::new();
    if let Some(machine) = machine {
        cards.push(card_with_rows(
            "CPU",
            format!("{} потоков", machine.logical_cpus),
            match machine.physical_cpus {
                Some(physical) => format!(
                    "{}. Физических ядер: {}. Логических потоков: {}.",
                    machine.cpu_model, physical, machine.logical_cpus
                ),
                None => machine.cpu_model.clone(),
            },
            "pass",
            Some(machine.cpu_source_label.clone()),
            Some("Автоматически собранный профиль CPU. Набор live-полей зависит от ОС и доступных сенсоров, но источник всегда определяется без хардкода под текущую машину.".to_string()),
            vec![
                metric_row(
                    "Нагрузка",
                    format_optional(machine.cpu_usage_percent, |value| format!("{value:.1}%")),
                    Some("Живая текущая загрузка CPU по всей системе."),
                ),
                metric_row(
                    "Температура",
                    format_optional(machine.cpu_temperature_celsius, format_celsius),
                    Some("Текущая температура CPU по доступному сенсору этой ОС."),
                ),
                metric_row(
                    "Максимум частоты",
                    format_optional(machine.cpu_max_mhz, |value| format!("{value:.0} MHz")),
                    Some("Максимальная частота процессора, которую система смогла определить автоматически."),
                ),
            ],
        ));
        cards.push(card_with_rows(
            "Оперативная память",
            format!("{:.2} GiB", machine.total_memory_gib),
            format!(
                "Тип: {}. Скорость: {}.",
                machine.memory_type, machine.memory_speed_label
            ),
            "pass",
            Some(machine.memory_source_label.clone()),
            Some(
                "Автоматически собранный профиль RAM. Тип и скорость берутся через цепочку OS-specific providers, а live usage идёт из системного runtime.".to_string(),
            ),
            vec![
                metric_row(
                    "Тип",
                    machine.memory_type.clone(),
                    Some("Автоматически определённый тип оперативной памяти."),
                ),
                metric_row(
                    "Скорость",
                    machine.memory_speed_label.clone(),
                    Some("Автоматически определённая скорость оперативной памяти."),
                ),
                metric_row(
                    "Занято",
                    format!("{:.2} GiB", machine.used_memory_gib),
                    Some("Сколько оперативной памяти занято прямо сейчас."),
                ),
                metric_row(
                    "Свободно",
                    format!("{:.2} GiB", machine.available_memory_gib),
                    Some("Сколько оперативной памяти система считает доступной прямо сейчас."),
                ),
                metric_row(
                    "Использование",
                    format_optional(machine.memory_used_percent, |value| format!("{value:.1}%")),
                    Some("Текущая доля занятой оперативной памяти."),
                ),
                metric_row(
                    "Swap",
                    format!(
                        "{:.2} / {:.2} GiB",
                        machine.swap_used_gib, machine.swap_total_gib
                    ),
                    Some("Использование swap прямо сейчас."),
                ),
            ],
        ));
        cards.push(card_with_rows(
            "Основной диск",
            machine.disk_kind.clone(),
            format!(
                "Устройство: {}. Модель: {}.",
                machine.disk_device.as_deref().unwrap_or("ещё нет данных"),
                machine.disk_model
            ),
            "pass",
            Some(machine.disk_source_label.clone()),
            Some("Автоматически собранный профиль основного диска. Где ОС даёт live I/O и термоданные, они показываются здесь; где не даёт, панель честно оставляет поле пустым.".to_string()),
            vec![
                metric_row(
                    "Объём",
                    format!("{:.2} GiB", machine.disk_total_gib),
                    Some("Полный размер основного диска."),
                ),
                metric_row(
                    "Свободно",
                    format!("{:.2} GiB", machine.disk_available_gib),
                    Some("Сколько свободного места осталось на основном диске."),
                ),
                metric_row(
                    "Использование",
                    format_optional(machine.disk_used_percent, |value| format!("{value:.1}%")),
                    Some("Текущая доля занятого пространства на основном диске."),
                ),
                metric_row(
                    "Нагрузка",
                    format_optional(machine.disk_busy_percent, |value| format!("{value:.1}%")),
                    Some("Насколько диск был занят операциями ввода-вывода между двумя последними refresh панели."),
                ),
                metric_row(
                    "Чтение",
                    format_optional(machine.disk_read_mib_per_sec, |value| format!("{value:.2} MiB/s")),
                    Some("Текущая скорость чтения между двумя последними refresh панели."),
                ),
                metric_row(
                    "Запись",
                    format_optional(machine.disk_write_mib_per_sec, |value| format!("{value:.2} MiB/s")),
                    Some("Текущая скорость записи между двумя последними refresh панели."),
                ),
                metric_row(
                    "Температура",
                    format_optional(machine.disk_temperature_celsius, format_celsius),
                    Some("Температура NVMe/SSD по живому датчику."),
                ),
                metric_row(
                    "Firmware",
                    machine.disk_firmware.clone(),
                    Some("Версия прошивки основного диска."),
                ),
            ],
        ));
        cards.extend(build_accelerator_cards(&machine.accelerators));
    } else {
        cards.push(with_status_tooltip(
            card(
                "Машина",
                "ещё нет данных".to_string(),
                "Сводку по железу пока не удалось собрать автоматически.".to_string(),
                "unknown",
            ),
            "Статус пока не может считаться нормальным по следующим причинам:\n- Автоматический сбор machine summary пока не дал результат.\n- Поэтому панель не может показать текущий профиль железа.",
        ));
    }

    if let Some(install_state) = install_state {
        cards.push(with_extra_class(
            card(
                "Установленный клиент",
                client_display_name(&install_state.client_key).to_string(),
                format!(
                    "Профиль: {}. Config: {}.",
                    install_state.stack_profile, install_state.client_config
                ),
                "pass",
            ),
            "machine-compact",
        ));
        cards.push(with_extra_class(
            card(
                "Сборка",
                install_state.package_version.clone(),
                format!(
                    "Ревизия: {}. Установлено: {}.",
                    install_state.repo_revision,
                    human_epoch_seconds(install_state.installed_at_epoch_seconds)
                ),
                "pass",
            ),
            "machine-compact",
        ));
    } else {
        cards.push(with_extra_class(
            with_status_tooltip(
                card(
                    "Установка",
                    "ещё не найдена".to_string(),
                    "state/install_state.json пока не найден, поэтому панель не видит последнюю user-facing установку.".to_string(),
                    "unknown",
                ),
                "Статус пока не может считаться нормальным по следующим причинам:\n- Файл state/install_state.json пока не найден.\n- Без него панель не видит последнюю пользовательскую установку этого клиента.",
            ),
            "machine-compact",
        ));
    }
    cards.push(with_extra_class(
        artifact_cleanup_card(snapshot, machine),
        "machine-compact",
    ));
    cards
}

fn artifact_cleanup_card(snapshot: &Value, machine: Option<&MachineSummary>) -> Value {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return card_with_rows(
            "Локальный мусор и retention",
            "ещё нет данных".to_string(),
            "Policy-driven cleanup для rebuildable хвоста ещё не успел записать последний summary.".to_string(),
            "unknown",
            Some("Источник: state/tooling/artifact_cleanup/latest.json".to_string()),
            Some("Этот блок показывает только rebuildable локальный хвост Amai. Live state и исторические данные сервисов сюда не входят.".to_string()),
            vec![],
        );
    }

    let safe_reclaimable_bytes = cleanup["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let policy_retained_reclaimable_bytes = cleanup["policy_retained_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    let manual_only_reclaimable_bytes = cleanup["manual_only_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    let safe_selected = cleanup["selected"].as_u64().unwrap_or(0);
    let safe_expired = cleanup["expired"].as_u64().unwrap_or(0);
    let aggressive_reclaimable_bytes = cleanup["aggressive_preview_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(safe_reclaimable_bytes);
    let aggressive_selected = cleanup["aggressive_preview_selected"]
        .as_u64()
        .unwrap_or(safe_selected);
    let captured_at_epoch_ms = cleanup["captured_at_epoch_ms"].as_u64();
    let kept_latest = cleanup["kept_latest"].as_u64().unwrap_or(0);
    let protected = cleanup["protected"].as_u64().unwrap_or(0);
    let targets_scanned = cleanup["targets_scanned"].as_u64().unwrap_or(0);
    let repo_inventory = &cleanup["repo_inventory"];
    let repo_total_bytes = repo_inventory["repo_total_bytes"].as_u64().unwrap_or(0);
    let cleanup_scope_bytes = repo_inventory["cleanup_scope_bytes"].as_u64().unwrap_or(0);
    let out_of_policy_bytes = repo_inventory["out_of_policy_bytes"].as_u64().unwrap_or(0);
    let unreadable_paths_count = repo_inventory["unreadable_paths_count"]
        .as_u64()
        .unwrap_or(0);
    let unreadable_paths_sample = repo_inventory["unreadable_paths_sample"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let large_unmanaged_roots = repo_inventory["large_unmanaged_roots"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let manual_only_targets = repo_inventory["manual_only_targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let policy_retained_targets = cleanup["policy_retained_targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let manual_only_reclaimable_targets = cleanup["manual_only_reclaimable_targets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let operator_reclaim_hints = artifact_cleanup_operator_reclaim_hints(cleanup);
    let last_apply = &cleanup["last_apply"];
    let last_reclaim_bytes = last_apply["reclaimed_bytes"].as_u64().unwrap_or(0);
    let last_deleted = last_apply["deleted"].as_u64().unwrap_or(0);
    let last_apply_mode = last_apply["mode"].as_str().unwrap_or("conservative");
    let last_apply_at = last_apply["captured_at_epoch_ms"].as_u64();

    let value = if !large_unmanaged_roots.is_empty() && out_of_policy_bytes > 0 {
        format!("{} вне policy", human_bytes(out_of_policy_bytes as f64))
    } else if safe_reclaimable_bytes > 0 {
        format!("{} safe", human_bytes(safe_reclaimable_bytes as f64))
    } else if manual_only_reclaimable_bytes > 0 {
        format!(
            "{} manual",
            human_bytes(manual_only_reclaimable_bytes as f64)
        )
    } else if policy_retained_reclaimable_bytes > 0 {
        format!(
            "{} ждёт TTL",
            human_bytes(policy_retained_reclaimable_bytes as f64)
        )
    } else if aggressive_reclaimable_bytes > 0 {
        format!(
            "{} preview",
            human_bytes(aggressive_reclaimable_bytes as f64)
        )
    } else {
        "по policy чисто".to_string()
    };
    let mut note = format!(
        "Safe policy чистит только то, что уже aged past TTL и не попадает под keep-latest. Aggressive preview показывает, сколько rebuildable хвоста можно убрать сразу, не трогая live state. Последний sweep: {}.",
        captured_at_epoch_ms
            .map(human_timestamp)
            .unwrap_or_else(|| "ещё нет данных".to_string())
    );
    if let Some(root) = large_unmanaged_roots.first() {
        let root_path = root["path"].as_str().unwrap_or("неизвестный root");
        let root_unmanaged_bytes = root["unmanaged_bytes"].as_u64().unwrap_or(0);
        note.push_str(&format!(
            " Основной локальный вес сейчас лежит вне cleanup policy: {root_path} = {} unmanaged bytes.",
            human_bytes(root_unmanaged_bytes as f64)
        ));
    } else if let Some(sample) = unreadable_paths_sample.first() {
        let sample_path = sample.as_str().unwrap_or("неизвестный path");
        note.push_str(&format!(
            " Inventory читает repo как best-effort lower bound: один из unreadable live-state путей сейчас {sample_path}. Поэтому часть вне-policy веса может жить там и не является broken cleanup contour.",
        ));
    }
    if let Some(target) = manual_only_targets.first() {
        let target_path = target["path"].as_str().unwrap_or("неизвестный target");
        note.push_str(&format!(
            " Для {target_path} уже есть explicit manual-only cleanup contour: используйте `observe cleanup-artifacts --target {target_path} --apply` или `--target {target_path} --aggressive --apply`, auto-retention этот путь не трогает."
        ));
    }
    if let Some(target) = policy_retained_targets.first() {
        let target_path = target["path"].as_str().unwrap_or("неизвестный target");
        let target_bytes = target["aggressive_preview_reclaimable_bytes"]
            .as_u64()
            .unwrap_or(0);
        note.push_str(&format!(
            " Сейчас основной policy-covered hot storage удерживается возрастным запасом и keep-latest: {target_path} = {}. Это не unmanaged drift и не сломанный cleanup, а осознанный retention hold.",
            human_bytes(target_bytes as f64)
        ));
    }
    if let Some(hint) = operator_reclaim_hints.first() {
        let target_path = hint["path"].as_str().unwrap_or("неизвестный target");
        let reclaimable_bytes = hint["reclaimable_bytes"].as_u64().unwrap_or(0);
        let command = hint["recommended_command"]
            .as_str()
            .unwrap_or("observe cleanup-artifacts --help");
        note.push_str(&format!(
            " Если место нужно вернуть раньше, ближайший operator reclaim path уже materialized: {target_path} = {} через `{command}`.",
            human_bytes(reclaimable_bytes as f64)
        ));
    }
    if last_reclaim_bytes > 0 {
        let last_apply_label = last_apply_at
            .map(human_timestamp)
            .unwrap_or_else(|| "неизвестно когда".to_string());
        note.push_str(&format!(
            " Последний apply-run уже вернул {} ({last_deleted} entries, mode={last_apply_mode}) в {last_apply_label}.",
            human_bytes(last_reclaim_bytes as f64)
        ));
    }

    let mut card = card_with_rows(
        "Локальный мусор и retention",
        value,
        note,
        artifact_cleanup_status(snapshot, machine),
        Some("Источник: state/tooling/artifact_cleanup/latest.json".to_string()),
        Some("Это локальный hygiene contour для build/cache хвостов Amai. Он не удаляет state PostgreSQL, Qdrant, MinIO или NATS.".to_string()),
        vec![
            metric_row(
                "Repo footprint",
                human_bytes(repo_total_bytes as f64),
                Some("Сколько места сейчас занимает весь repo-root, включая то, что не входит в cleanup policy."),
            ),
            metric_row(
                "Cleanup scope",
                human_bytes(cleanup_scope_bytes as f64),
                Some("Сколько места сейчас лежит внутри управляемых cleanup-target roots."),
            ),
            metric_row(
                "Вне policy",
                human_bytes(out_of_policy_bytes as f64),
                Some("Сколько места сейчас лежит вне cleanup-target roots и поэтому не удаляется auto-retention path-ом."),
            ),
            metric_row(
                "Safe reclaim now",
                human_bytes(safe_reclaimable_bytes as f64),
                Some("Сколько места можно вернуть прямо сейчас, не нарушая TTL и keep-latest policy."),
            ),
            metric_row(
                "Aggressive preview",
                human_bytes(aggressive_reclaimable_bytes as f64),
                Some("Сколько rebuildable хвоста можно убрать сразу explicit aggressive path-ом, не трогая live state."),
            ),
            metric_row(
                "Policy-retained hot storage",
                human_bytes(policy_retained_reclaimable_bytes as f64),
                Some("Сколько rebuildable веса уже входит в cleanup policy, но пока удерживается TTL/keep-latest и therefore ещё не попадает под safe reclaim."),
            ),
            metric_row(
                "Manual reclaim now",
                human_bytes(manual_only_reclaimable_bytes as f64),
                Some("Сколько веса сейчас доступно только через explicit/manual cleanup contours, а не через auto-retention."),
            ),
            metric_row(
                "Last reclaim",
                if last_reclaim_bytes > 0 {
                    format!(
                        "{} ({last_deleted}, {last_apply_mode})",
                        human_bytes(last_reclaim_bytes as f64)
                    )
                } else {
                    "ещё не было".to_string()
                },
                Some("Сколько места вернул последний apply-run cleanup policy и в каком режиме он был выполнен."),
            ),
            metric_row(
                "Safe кандидаты",
                safe_selected.to_string(),
                Some("Сколько отдельных entries уже попали под текущую conservative policy."),
            ),
            metric_row(
                "Aggressive кандидаты",
                aggressive_selected.to_string(),
                Some("Сколько отдельных entries можно было бы убрать explicit aggressive path-ом прямо сейчас."),
            ),
            metric_row(
                "TTL already expired",
                safe_expired.to_string(),
                Some("Сколько entries уже aged past TTL, даже если limit сейчас не даёт выбрать их все."),
            ),
            metric_row(
                "Heavy unmanaged roots",
                if large_unmanaged_roots.is_empty() {
                    "нет".to_string()
                } else {
                    large_unmanaged_roots
                        .iter()
                        .map(|root| {
                            let path = root["path"].as_str().unwrap_or("неизвестный root");
                            let unmanaged_bytes = root["unmanaged_bytes"].as_u64().unwrap_or(0);
                            format!("{path} ({})", human_bytes(unmanaged_bytes as f64))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Крупные директории вне cleanup policy. Они не попадают под TTL/keep-latest auto-path."),
            ),
            metric_row(
                "Manual-only contours",
                if manual_only_targets.is_empty() {
                    "нет".to_string()
                } else {
                    manual_only_targets
                        .iter()
                        .map(|target| {
                            let path = target["path"].as_str().unwrap_or("неизвестный target");
                            let ttl_hours = target["ttl_hours"].as_u64().unwrap_or(0);
                            let keep_latest = target["keep_latest"].as_u64().unwrap_or(0);
                            let total_bytes = target["total_bytes"].as_u64().unwrap_or(0);
                            format!(
                                "{path} ({}, ttl {ttl_hours}h, keep_latest {keep_latest})",
                                human_bytes(total_bytes as f64)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Пути, которые уже заведены в cleanup policy, но остаются только на explicit/manual path и не удаляются auto-retention-ом."),
            ),
            metric_row(
                "Policy waiting targets",
                if policy_retained_targets.is_empty() {
                    "нет".to_string()
                } else {
                    policy_retained_targets
                        .iter()
                        .map(|target| {
                            let path = target["path"].as_str().unwrap_or("неизвестный target");
                            let ttl_hours = target["ttl_hours"].as_u64().unwrap_or(0);
                            let keep_latest = target["keep_latest"].as_u64().unwrap_or(0);
                            let reclaimable = target["aggressive_preview_reclaimable_bytes"]
                                .as_u64()
                                .unwrap_or(0);
                            format!(
                                "{path} ({}, ttl {ttl_hours}h, keep_latest {keep_latest})",
                                human_bytes(reclaimable as f64)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Cleanup-targets, которые уже policy-covered, но всё ещё intentionally удерживаются возрастным запасом или keep-latest."),
            ),
            metric_row(
                "Manual reclaim targets",
                if manual_only_reclaimable_targets.is_empty() {
                    "нет".to_string()
                } else {
                    manual_only_reclaimable_targets
                        .iter()
                        .map(|target| {
                            let path = target["path"].as_str().unwrap_or("неизвестный target");
                            let reclaimable = target["aggressive_preview_reclaimable_bytes"]
                                .as_u64()
                                .unwrap_or(0);
                            format!("{path} ({})", human_bytes(reclaimable as f64))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Manual-only cleanup contours, где reclaim уже доступен, но auto-retention этот path не трогает."),
            ),
            metric_row(
                "Operator reclaim next",
                if operator_reclaim_hints.is_empty() {
                    "нет".to_string()
                } else {
                    operator_reclaim_hints
                        .iter()
                        .map(artifact_cleanup_reclaim_hint_summary)
                        .collect::<Vec<_>>()
                        .join("; ")
                },
                Some("Точные команды для самых тяжёлых reclaim-кандидатов, если место нужно вернуть раньше TTL/keep-latest."),
            ),
            metric_row(
                "Keep latest / protected",
                format!("{kept_latest} / {protected}"),
                Some("Что policy сейчас удерживает: недавние entries по keep-latest и активные защищённые paths."),
            ),
            metric_row(
                "Targets scanned",
                targets_scanned.to_string(),
                Some("Сколько cleanup-target directories сейчас участвует в policy-driven контуре."),
            ),
            metric_row(
                "Unreadable contents",
                unreadable_paths_count.to_string(),
                Some("Сколько путей inventory не смог прочитать. Repo footprint тогда считается как best-effort lower bound."),
            ),
            metric_row(
                "Unreadable sample",
                if unreadable_paths_sample.is_empty() {
                    "нет".to_string()
                } else {
                    unreadable_paths_sample
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                Some("Примеры путей, которые inventory не смог прочитать и поэтому считает repo footprint только как best-effort lower bound."),
            ),
        ],
    );
    if let Some(tooltip) = status_reason_tooltip(
        artifact_cleanup_status(snapshot, machine),
        artifact_cleanup_warning(snapshot, machine)
            .into_iter()
            .collect(),
        "Cleanup contour видит локальный rebuildable хвост, который уже требует внимания.",
    ) {
        card = with_status_tooltip(card, &tooltip);
    }
    card
}

fn build_accelerator_cards(accelerators: &[AcceleratorSummary]) -> Vec<Value> {
    let mut cards = Vec::new();
    let Some(primary) = accelerators.first() else {
        cards.push(card_with_rows(
            "Графика и ускорители",
            "не обнаружено".to_string(),
            "Автоопределение не нашло доступный GPU, iGPU, eGPU или другой ускоритель в этой среде.".to_string(),
            "unknown",
            Some("Источник: accelerator auto-detect provider chain".to_string()),
            Some("Этот блок показывает все найденные графические и AI-ускорители: встроенную графику, дискретные GPU, внешние GPU и другие accelerator-устройства.".to_string()),
            vec![
                metric_row(
                    "Устройств",
                    "0".to_string(),
                    Some("Сколько графических и accelerator-устройств удалось обнаружить автоматически."),
                ),
                metric_row(
                    "Основное устройство",
                    "не обнаружено".to_string(),
                    Some("Какое устройство система выбрала бы основным для показа, если бы оно было найдено."),
                ),
            ],
        ));
        return cards;
    };

    let additional_count = accelerators.len().saturating_sub(1);
    let primary_note = match &primary.driver_version {
        Some(driver) => format!(
            "{}. Стек: {}. Драйвер: {}.",
            primary.kind_label, primary.backend, driver
        ),
        None => format!("{}. Стек: {}.", primary.kind_label, primary.backend),
    };
    let mut primary_rows = vec![
        metric_row(
            "Устройств",
            accelerators.len().to_string(),
            Some("Сколько графических и accelerator-устройств система обнаружила автоматически."),
        ),
        metric_row(
            "Тип",
            primary.kind_label.clone(),
            Some("Какой тип ускорителя система определила для основного устройства."),
        ),
        metric_row(
            "Стек",
            primary.backend.clone(),
            Some("Какой vendor stack или runtime система смогла определить автоматически."),
        ),
        metric_row(
            "Драйвер",
            primary
                .driver_version
                .clone()
                .unwrap_or_else(|| "данные недоступны".to_string()),
            Some("Версия драйвера или runtime, если provider смог её определить."),
        ),
        metric_row(
            "Память",
            format_optional(primary.total_vram_gib, |value| format!("{value:.2} GiB")),
            Some(
                "Полный объём видеопамяти или локальной памяти ускорителя, если provider дал это поле.",
            ),
        ),
        metric_row(
            "Использовано памяти",
            format_optional(primary.used_vram_gib, |value| format!("{value:.2} GiB")),
            Some("Сколько памяти ускорителя занято прямо сейчас."),
        ),
        metric_row(
            "Нагрузка",
            format_optional(primary.utilization_percent, |value| format!("{value:.1}%")),
            Some("Текущая загрузка основного ускорителя, если live provider умеет её отдавать."),
        ),
        metric_row(
            "Температура",
            format_optional(primary.temperature_celsius, format_celsius),
            Some("Текущая температура основного ускорителя по доступному live provider."),
        ),
        metric_row(
            "Мощность",
            format_optional(primary.power_watts, |value| format!("{value:.2} W")),
            Some(
                "Текущее энергопотребление основного ускорителя, если provider умеет его отдавать.",
            ),
        ),
    ];
    if additional_count > 0 {
        primary_rows.push(metric_row(
            "Другие устройства",
            accelerators[1..]
                .iter()
                .map(|item| format!("{}: {}", item.kind_label, item.model))
                .collect::<Vec<_>>()
                .join("; "),
            Some("Остальные найденные ускорители в этой машине."),
        ));
    }
    cards.push(card_with_rows(
        "Графика и ускорители",
        primary.model.clone(),
        primary_note,
        if primary.detected { "pass" } else { "unknown" },
        Some(primary.source_label.clone()),
        Some("Основным показывается ускоритель с самым богатым live-профилем. Остальные устройства перечислены ниже или отдельными карточками.".to_string()),
        primary_rows,
    ));

    for accelerator in accelerators.iter().skip(1) {
        cards.push(with_extra_class(
            card_with_rows(
                "Доп. ускоритель",
                accelerator.model.clone(),
                match &accelerator.driver_version {
                    Some(driver) => format!(
                        "{}. Стек: {}. Драйвер: {}.",
                        accelerator.kind_label, accelerator.backend, driver
                    ),
                    None => format!("{}. Стек: {}.", accelerator.kind_label, accelerator.backend),
                },
                if accelerator.detected { "pass" } else { "unknown" },
                Some(accelerator.source_label.clone()),
                Some("Дополнительное графическое или accelerator-устройство, найденное в этой машине.".to_string()),
                vec![
                    metric_row("Тип", accelerator.kind_label.clone(), Some("Определённый тип дополнительного ускорителя.")),
                    metric_row(
                        "Память",
                        format_optional(accelerator.total_vram_gib, |value| format!("{value:.2} GiB")),
                        Some("Полный объём памяти дополнительного ускорителя, если provider смог его дать."),
                    ),
                    metric_row(
                        "Нагрузка",
                        format_optional(accelerator.utilization_percent, |value| format!("{value:.1}%")),
                        Some("Текущая загрузка дополнительного ускорителя, если live provider умеет её отдавать."),
                    ),
                    metric_row(
                        "Температура",
                        format_optional(accelerator.temperature_celsius, format_celsius),
                        Some("Текущая температура дополнительного ускорителя, если live provider умеет её отдавать."),
                    ),
                ],
            ),
            "machine-compact",
        ));
    }
    cards
}

fn build_governance_card(snapshot: &Value) -> Value {
    let governance = &snapshot["governance_surface"];
    if !governance.is_object() {
        return card_with_rows(
            "Жизненный цикл памяти",
            "ещё нет данных".to_string(),
            "Пока панель не собрала machine-readable surface по forgetting, quarantine и memory governance."
                .to_string(),
            "unknown",
            Some(
                "Источник: governance_surface из live snapshot. Пока этот слой не surfaced.".to_string(),
            ),
            Some(
                "Показывает, как Amai чистит, архивирует и пересматривает память, не теряя protected truth и explainability."
                    .to_string(),
            ),
            vec![],
        );
    }

    let open_conflicts = governance["wrong_link_rate"]["open_conflict_count"]
        .as_u64()
        .unwrap_or(0);
    let active_quarantine = governance["poisoning_alert_count"]["active_quarantine_items"]
        .as_u64()
        .unwrap_or(0);
    let disputed_items = governance["trust_state_distribution"]["disputed_memory_items"]
        .as_u64()
        .unwrap_or(0);
    let forgetting_total = governance["human_override_audit"]["forgetting_audit_log_entries_total"]
        .as_u64()
        .unwrap_or(0);
    let status = if open_conflicts > 0 || active_quarantine > 0 || disputed_items > 0 {
        "alert"
    } else if forgetting_total > 0 {
        "pass"
    } else {
        "unknown"
    };
    let pruning_job_total = governance["forgetting_job_breakdown"]["pruning_job"]
        .as_u64()
        .unwrap_or(0);
    let cold_archive_job_total = governance["forgetting_job_breakdown"]["cold_archive_job"]
        .as_u64()
        .unwrap_or(0);
    let revalidation_job_total = governance["forgetting_job_breakdown"]["revalidation_job"]
        .as_u64()
        .unwrap_or(0);
    let dedup_job_total = governance["forgetting_job_breakdown"]["de_duplication_job"]
        .as_u64()
        .unwrap_or(0);
    let summarize_job_total = governance["forgetting_job_breakdown"]["summarization_job"]
        .as_u64()
        .unwrap_or(0);
    let stale_rate = governance["stale_memory_error_rate"]["rate"].as_f64();
    let top_quarantine = governance["poisoning_alert_count"]["active_quarantine_breakdown"]
        .as_array()
        .and_then(|items| items.first());
    let top_conflict = governance["open_conflict_breakdown"]
        .as_array()
        .and_then(|items| items.first());
    let headline_value = if status == "alert" {
        let mut parts = Vec::new();
        if active_quarantine > 0 {
            parts.push(format!(
                "{} в quarantine",
                format_u64(Some(active_quarantine))
            ));
        }
        if open_conflicts > 0 {
            parts.push(format!(
                "{} {}",
                format_u64(Some(open_conflicts)),
                format_ru_count_noun(open_conflicts, "конфликт", "конфликта", "конфликтов")
            ));
        }
        if disputed_items > 0 {
            parts.push(format!(
                "{} {}",
                format_u64(Some(disputed_items)),
                format_ru_count_noun(disputed_items, "спорный", "спорных", "спорных")
            ));
        }
        if parts.is_empty() {
            "требует внимания".to_string()
        } else {
            parts.join(" • ")
        }
    } else if forgetting_total > 0 {
        format!(
            "{} forgetting-действий зафиксировано",
            format_u64(Some(forgetting_total))
        )
    } else {
        "ещё нет действий".to_string()
    };
    let alert_note = format_governance_alert_note(top_quarantine, top_conflict);

    card_with_rows(
        "Жизненный цикл памяти",
        headline_value,
        if status == "alert" {
            alert_note.unwrap_or_else(|| {
                "Карточка требует внимания, потому что в live memory governance сейчас есть quarantine или открытые truth-конфликты."
                    .to_string()
            })
        } else {
            "Здесь видно, как Amai реально чистит и пересматривает память: pruning, archive, revalidation и dedup surfaced отдельно, а protected truth не должен исчезать тихо."
                .to_string()
        },
        status,
        Some(
            "Источник: live governance_surface. Карточка показывает не policy-обещание, а фактический audit contour forgetting и trust."
                .to_string(),
        ),
        Some(
            "Stage 9 surface: explainable forgetting, quarantine/trust pressure и реальный объём lifecycle-действий."
                .to_string(),
        ),
        vec![
            metric_row(
                "Pruning",
                format_u64(Some(pruning_job_total)),
                Some("Сколько pruning-действий уже записано через TTL или low-utility cleanup."),
            ),
            metric_row(
                "Archive",
                format_u64(Some(cold_archive_job_total)),
                Some("Сколько stale derivative items уже переведено в cold archive."),
            ),
            metric_row(
                "Revalidation",
                format_u64(Some(revalidation_job_total)),
                Some("Сколько stale current items уже отправлено в pending_review."),
            ),
            metric_row(
                "Dedup / compaction",
                format_u64(Some(dedup_job_total)),
                Some("Сколько duplicate branches уже схлопнуто через de-duplication / compaction contour."),
            ),
            metric_row(
                "Summarization",
                format_u64(Some(summarize_job_total)),
                Some("Пока это explicit no-op contract. Здесь не должно быть тихой псевдо-активности."),
            ),
            metric_row(
                "Stale rate",
                format_ratio_percent(stale_rate),
                Some("Доля archived/pruned items от всей памяти. Это не KPI успеха, а честный pressure indicator cleanup-контура."),
            ),
            metric_row(
                "Quarantine",
                format_u64(Some(active_quarantine)),
                Some("Сколько memory items сейчас ещё удерживаются в quarantine и требуют ручного разбора."),
            ),
            metric_row(
                "Спорные",
                format_u64(Some(disputed_items)),
                Some("Сколько memory items сейчас имеют disputed trust-state."),
            ),
            metric_row(
                "Открытые конфликты",
                format_u64(Some(open_conflicts)),
                Some("Сколько wrong-link / truth конфликтов сейчас ещё не закрыто."),
            ),
        ],
    )
}

fn format_governance_alert_note(
    top_quarantine: Option<&Value>,
    top_conflict: Option<&Value>,
) -> Option<String> {
    let mut reasons = Vec::new();
    if let Some(item) = top_quarantine {
        let count = item["item_count"].as_u64().unwrap_or(0);
        let reason = item["quarantine_reason"].as_str().unwrap_or("unknown");
        let entity_kind = item["entity_kind"].as_str().unwrap_or("unknown");
        let source_kind = item["source_kind"].as_str().unwrap_or("unknown");
        reasons.push(format!(
            "главный quarantine-класс: {} ({}, {}, {})",
            format_u64(Some(count)),
            humanize_identifier(reason),
            humanize_identifier(entity_kind),
            humanize_identifier(source_kind)
        ));
    }
    if let Some(item) = top_conflict {
        let count = item["item_count"].as_u64().unwrap_or(0);
        let summary = compact_dashboard_text(item["summary"].as_str(), 56, "unknown");
        let source_kind = item["source_kind"].as_str().unwrap_or("unknown");
        reasons.push(format!(
            "главный конфликт: {} ({}, {})",
            format_u64(Some(count)),
            summary,
            humanize_identifier(source_kind)
        ));
    }
    if reasons.is_empty() {
        None
    } else {
        Some(format!(
            "Карточка требует внимания: {}.",
            reasons.join("; ")
        ))
    }
}

fn build_warnings(snapshot: &Value, machine: Option<&MachineSummary>) -> Vec<String> {
    let mut warnings = Vec::new();
    for check in snapshot["sla"]["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|check| check["status"].as_str().unwrap_or("unknown") != "pass")
    {
        warnings.push(humanize_check(snapshot, check));
    }
    if let Some(warning) = artifact_cleanup_warning(snapshot, machine) {
        warnings.push(warning);
    }
    warnings
}

fn build_glossary() -> Vec<Value> {
    vec![
        json!({
            "term": "Hot retrieval",
            "meaning": "Повторный запрос по уже прогретому кэшу. Именно здесь Amai показывает самые быстрые цифры."
        }),
        json!({
            "term": "Cold retrieval",
            "meaning": "Первый запрос после старта или без прогрева. Он всегда тяжелее и поэтому медленнее."
        }),
        json!({
            "term": "P50 / P95 / P99 / Max",
            "meaning": "P50 — середина выборки. P95 — почти все запросы, кроме тяжёлого хвоста. P99 — ещё более строгий хвост. Max — самый тяжёлый одиночный выброс."
        }),
        json!({
            "term": "Burst QPS",
            "meaning": "Средняя скорость внутри конкретного benchmark-окна. Это не live поток страницы и не обещание стабильной обычной пропускной способности."
        }),
        json!({
            "term": "Recall",
            "meaning": "Насколько полно система нашла всё нужное. Если recall низкий, часть правильного ответа просто не была найдена."
        }),
        json!({
            "term": "Precision",
            "meaning": "Насколько чисто система попала в нужный контекст. Если precision низкий, система тянет лишнее и шумное."
        }),
        json!({
            "term": "Hit rate",
            "meaning": "Доля запросов, где Amai реально попал в нужную цель: файл, символ, документ или нужный фрагмент контекста."
        }),
        json!({
            "term": "Fallback rate",
            "meaning": "Как часто системе пришлось отходить на запасной путь, потому что основной retrieval или ranking не справился сам."
        }),
        json!({
            "term": "Cross-project leakage",
            "meaning": "Случай, когда контекст одного проекта просочился в другой. Для строгого режима это должно быть только 0."
        }),
        json!({
            "term": "Live probe",
            "meaning": "Короткий живой системный замер, который пересчитывается прямо при refresh панели. Это не исторический snapshot и не benchmark."
        }),
        json!({
            "term": "Cold contour",
            "meaning": "Это проверка первого запроса без прогрева. Она показывает, сколько занимает весь путь ответа целиком, пока у системы ещё нет готового быстрого кэша."
        }),
        json!({
            "term": "Resident memory",
            "meaning": "Объём памяти, который сервис реально держит в RAM прямо сейчас, а не просто зарезервировал теоретически."
        }),
        json!({
            "term": "Semantic search",
            "meaning": "Поиск по смысловой близости, а не по точному совпадению слов. Полезен для recall, но не заменяет lexical/source-of-truth слой."
        }),
        json!({
            "term": "Token savings",
            "meaning": "Сколько токенов Amai сэкономил по сравнению с реалистичным baseline-путём без потери качества."
        }),
        json!({
            "term": "SLA summary",
            "meaning": "Короткая сводка: сколько обязательных checks сейчас проходят, предупреждают или уже горят критически."
        }),
    ]
}

fn build_links(base_url: &str) -> Vec<Value> {
    let mut links = vec![json!({
        "label": "",
        "note": "",
        "items": [
            {
                "label": "Raw dashboard JSON",
                "url": format!("{base_url}/api/dashboard"),
                "note": "Если хотите отдать эти же данные другой программе."
            },
            {
                "label": "Raw snapshot JSON",
                "url": format!("{base_url}/api/snapshot"),
                "note": "Полный live snapshot без human-упаковки."
            },
            {
                "label": "Prometheus metrics",
                "url": format!("{base_url}/metrics"),
                "note": "Инженерный слой для scrape и алертов."
            },
            {
                "label": "Health JSON",
                "url": format!("{base_url}/healthz"),
                "note": "Быстрый health-check с тем же SLA-контуром."
            }
        ]
    })];

    let prometheus_port = env::var("AMI_PROMETHEUS_PORT").unwrap_or_else(|_| "59090".to_string());
    let grafana_port = env::var("AMI_GRAFANA_PORT").unwrap_or_else(|_| "53000".to_string());
    let grafana_admin_user =
        env::var("AMI_GRAFANA_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
    let grafana_default_password = env::var("AMI_GRAFANA_ADMIN_PASSWORD")
        .map(|value| value == "admin_change_me")
        .unwrap_or(false);
    let prometheus_available = tcp_port_is_open("127.0.0.1", &prometheus_port);
    let grafana_available = tcp_port_is_open("127.0.0.1", &grafana_port);
    links.push(json!({
        "label": "",
        "note": "",
        "items": [
            {
                "label": "Prometheus",
                "url": if prometheus_available { Value::from(monitoring_url(base_url, &prometheus_port)) } else { Value::Null },
                "note": if prometheus_available {
                    "Глубокие live-метрики уже доступны."
                } else {
                    "Мониторинг сейчас не поднят. Сначала запустите ./scripts/monitoring_up.sh."
                }
            },
            {
                "label": "Grafana",
                "url": if grafana_available { Value::from(monitoring_url(base_url, &grafana_port)) } else { Value::Null },
                "note": if grafana_available {
                    if grafana_default_password {
                        format!("Готовая инженерная панель уже доступна. Логин: {}. Пароль пока стандартный из .env: admin_change_me. Лучше сменить его в AMI_GRAFANA_ADMIN_PASSWORD.", grafana_admin_user)
                    } else {
                        format!("Готовая инженерная панель уже доступна. Логин: {}. Текущий пароль задан в .env через AMI_GRAFANA_ADMIN_PASSWORD.", grafana_admin_user)
                    }
                } else {
                    "Grafana поднимается вместе с мониторингом. Сначала запустите ./scripts/monitoring_up.sh.".to_string()
                }
            }
        ]
    }));
    links
}

fn live_latency_compare_card(snapshot: &Value) -> Value {
    let hot = rolling_window_live_response_latency_slice(snapshot, "hot");
    let cold = rolling_window_live_response_latency_slice(snapshot, "cold");
    let current_hot = current_series_live_response_latency_slice(snapshot, "hot");
    let current_cold = current_series_live_response_latency_slice(snapshot, "cold");
    let hot_sample_count = hot
        .and_then(|slice| slice["sample_count"].as_u64())
        .unwrap_or_default();
    let cold_sample_count = cold
        .and_then(|slice| slice["sample_count"].as_u64())
        .unwrap_or_default();
    let current_hot_sample_count = current_hot
        .and_then(|slice| slice["sample_count"].as_u64())
        .unwrap_or_default();
    let current_cold_sample_count = current_cold
        .and_then(|slice| slice["sample_count"].as_u64())
        .unwrap_or_default();
    let hot_has_data = hot_sample_count > 0;
    let cold_has_data = cold_sample_count > 0;
    let current_hot_has_data = current_hot_sample_count > 0;
    let current_cold_has_data = current_cold_sample_count > 0;
    let current_series_has_data = current_hot_has_data || current_cold_has_data;
    let current_series_relation_note =
        live_response_latency_current_session_relation_note(snapshot);
    let current_series_exclusions_note =
        live_response_latency_current_session_exclusions_note(snapshot);
    let current_series_minutes = live_response_latency_root(snapshot)
        .and_then(|root| root["current_session_exclusions"]["current_series_minutes"].as_u64())
        .unwrap_or(60);
    let rolling_window_label = latency_window_label(snapshot);
    let rolling_window_label_short = rolling_window_label.trim_end_matches('.');
    let current_live_turn_no_amai_activity =
        token_budget_report_root(snapshot)["current_live_turn"]["status"].as_str()
            == Some("no_amai_activity_in_current_live_turn");
    let hot_targets = live_latency_table_targets(snapshot, "hot");
    let cold_targets = live_latency_table_targets(snapshot, "cold");
    let current_hot_assessment = assess_live_latency_slice(current_hot, &hot_targets);
    let current_cold_assessment = assess_live_latency_slice(current_cold, &cold_targets);
    let hot_assessment = assess_live_latency_slice(hot, &hot_targets);
    let cold_assessment = assess_live_latency_slice(cold, &cold_targets);
    let mut overall_status = if current_series_has_data {
        combine_live_compare_status(&[
            current_hot_assessment.status,
            current_cold_assessment.status,
        ])
    } else {
        combine_live_compare_status(&[hot_assessment.status, cold_assessment.status])
    };
    if overall_status == "unknown" && (hot_has_data || cold_has_data || current_series_has_data) {
        overall_status = "waiting";
    }
    let mut status_tooltip = if current_series_has_data {
        live_latency_compare_status_tooltip(
            overall_status,
            &current_hot_assessment,
            &current_cold_assessment,
        )
    } else {
        live_latency_compare_status_tooltip(overall_status, &hot_assessment, &cold_assessment)
    };
    if current_live_turn_no_amai_activity {
        let inactivity_note = current_series_relation_note.as_deref().unwrap_or(
            "В текущем live-turn нет новых Amai-событий, поэтому живое окно может не расти до нового Amai-запроса.",
        );
        status_tooltip = Some(match status_tooltip {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{existing} {inactivity_note}")
            }
            _ => inactivity_note.to_string(),
        });
    }
    let card_note = {
        let base_note = format!(
            "{} {} {}",
            if current_series_has_data {
                format!(
                    "Главный сигнал теперь строится по текущей серии ответов Amai в этом чате (последние {} минут).",
                    current_series_minutes
                )
            } else {
                format!(
                    "В текущей серии этого чата за последние {} минут ещё мало данных, поэтому главным fallback остаётся накопительное окно 24 часов.",
                    current_series_minutes
                )
            },
            format!(
                "Ниже рядом показаны и текущая серия, и {} по задержке Amai, чтобы сразу видеть и мгновенный сбой, и устойчивый тренд.",
                rolling_window_label_short
            ),
            if current_live_turn_no_amai_activity {
                current_series_relation_note.as_deref().unwrap_or(
                    "В текущем live-turn пока нет новых Amai-событий, поэтому окно обновится после следующего Amai-запроса.",
                )
            } else {
                "Эталоны не меняются; меняются только свежая серия и накопительная выборка."
            }
        );
        if let Some(exclusions_note) = current_series_exclusions_note {
            format!("{base_note} {exclusions_note}")
        } else {
            base_note
        }
    };
    let table_rows = vec![
        json!({
            "label": "Повторный запрос — эталон",
            "tooltip": format!(
                "Фиксированный эталон для этого режима. Строгая проверочная выборка отдельно: > {}.",
                format_u64(Some(hot_targets.benchmark_sample_count))
            ),
            "values": target_values(snapshot, &hot_targets)
        }),
        json!({
            "label": "Повторный запрос — текущая серия",
            "tooltip": "Свежая серия ответов Amai в текущем чате. Именно она определяет мгновенный operator signal.",
            "values": compare_values(snapshot, current_hot, current_hot_sample_count)
        }),
        json!({
            "label": "Повторный запрос — окно 24ч",
            "tooltip": "Накопительное живое окно задержки Amai за последние 24 часа. Оно не сбрасывается на новый чат и нужно для тренда.",
            "values": compare_values(snapshot, hot, hot_sample_count)
        }),
        json!({
            "label": "Новый запрос — эталон",
            "tooltip": format!(
                "Фиксированный эталон для этого режима. Строгая проверочная выборка отдельно: > {}.",
                format_u64(Some(cold_targets.benchmark_sample_count))
            ),
            "values": target_values(snapshot, &cold_targets)
        }),
        json!({
            "label": "Новый запрос — текущая серия",
            "tooltip": "Свежая серия ответов Amai в текущем чате. Именно она определяет мгновенный operator signal.",
            "values": compare_values(snapshot, current_cold, current_cold_sample_count)
        }),
        json!({
            "label": "Новый запрос — окно 24ч",
            "tooltip": "Накопительное живое окно задержки Amai за последние 24 часа. Оно не сбрасывается на новый чат и нужно для тренда.",
            "values": compare_values(snapshot, cold, cold_sample_count)
        }),
    ];
    let mut card = json!({
        "kind": "live_compare",
        "title": "Скорость ответа",
        "title_tooltip": "Показывает два слоя сразу: свежую серию ответов Amai в текущем чате для мгновенного operator signal и накопительное окно 24 часов для тренда. Эталоны для обоих режимов всегда фиксированы в таблице.",
        "status": overall_status,
        "status_label": status_label(overall_status),
        "status_tooltip": status_tooltip,
        "source_label": "Источник: текущая серия и окно 24 часов берутся из live_response_latency по реальным ответам Amai до первого видимого ответа. Retrieval-only срезы и строгие benchmark-прогоны показываются отдельно ниже.",
        "note": card_note,
        "metrics": [
            {
                "label": "Повторный запрос",
                "tooltip": "Сверху показывается P50 по текущей серии ответов этого чата. В note рядом видно, сколько уже накоплено в текущей серии и в окне 24 часов.",
                "value": if current_hot_has_data {
                    format_ms(snapshot, current_hot.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else if hot_has_data {
                    format_ms(snapshot, hot.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else {
                    "ещё нет данных".to_string()
                },
                "note": format!(
                    "Текущая серия: {}. {}: {}. {}",
                    if current_hot_has_data {
                        format_u64(Some(current_hot_sample_count))
                    } else {
                        "ещё нет данных".to_string()
                    },
                    rolling_window_label_short,
                    format_u64(Some(hot_sample_count)),
                    if current_hot_has_data {
                        current_hot_assessment.note.clone()
                    } else if hot_has_data {
                        format!(
                            "Онлайн-серия ещё не накопилась, поэтому временно ориентируемся на {}.",
                            hot_assessment.note
                        )
                    } else {
                        "По этому режиму пока нет ни текущей серии, ни накопленного окна.".to_string()
                    }
                )
            },
            {
                "label": "Новый запрос",
                "tooltip": "Сверху показывается P50 по текущей серии новых запросов этого чата. В note рядом видно, сколько уже накоплено в текущей серии и в окне 24 часов.",
                "value": if current_cold_has_data {
                    format_ms(snapshot, current_cold.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else if cold_has_data {
                    format_ms(snapshot, cold.and_then(|slice| slice["p50_latency_ms"].as_f64()))
                } else {
                    "ещё нет данных".to_string()
                },
                "note": format!(
                    "Текущая серия: {}. {}: {}. {}",
                    if current_cold_has_data {
                        format_u64(Some(current_cold_sample_count))
                    } else {
                        "ещё нет данных".to_string()
                    },
                    rolling_window_label_short,
                    format_u64(Some(cold_sample_count)),
                    if current_cold_has_data {
                        current_cold_assessment.note.clone()
                    } else if cold_has_data {
                        format!(
                            "Онлайн-серия ещё не накопилась, поэтому временно ориентируемся на {}.",
                            cold_assessment.note
                        )
                    } else {
                        "По этому режиму пока нет ни текущей серии, ни накопленного окна.".to_string()
                    }
                )
            }
        ],
        "table": {
            "columns": [
                { "label": "Сценарий", "tooltip": "Какой случай мы сейчас смотрим: повторный запрос или новый запрос." },
                { "label": "P50", "tooltip": "Обычная задержка Amai до первого видимого ответа. Примерно такую скорость пользователь видит чаще всего." },
                { "label": "P95", "tooltip": "Почти вся задержка Amai по ответам должна укладываться в это время." },
                { "label": "P99", "tooltip": "Редкие медленные ответы Amai. Чем меньше, тем лучше." },
                { "label": "Max", "tooltip": "Самая медленная задержка Amai в текущей выборке." },
                { "label": "Запросов", "tooltip": "Сколько ответов уже вошло в расчёт для этой строки." }
            ],
            "rows": table_rows
        }
    });
    if overall_status == "waiting" {
        let label = if current_series_has_data {
            "текущая серия ещё набирается"
        } else {
            "онлайн-серия ещё набирается"
        };
        card = with_status_label(card, label);
    }
    card
}

fn latency_window_label(snapshot: &Value) -> String {
    let root = token_budget_report_root(snapshot);
    match root["profile"]["rolling_window_hours"].as_u64() {
        Some(hours) if hours > 0 => format!("скользящее окно {} ч.", format_u64(Some(hours))),
        _ => "накопительное живое окно".to_string(),
    }
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
    root["live_response_latency"]
        .is_object()
        .then_some(&root["live_response_latency"])
}

fn live_response_latency_slice_in_scope<'a>(
    snapshot: &'a Value,
    scope: &str,
    state: &str,
) -> Option<&'a Value> {
    live_response_latency_root(snapshot)?[scope]["latency_slices"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|slice| slice["state"].as_str() == Some(state))
}

fn current_series_live_response_latency_slice<'a>(
    snapshot: &'a Value,
    state: &str,
) -> Option<&'a Value> {
    live_response_latency_slice_in_scope(snapshot, "current_session", state)
}

fn rolling_window_live_response_latency_slice<'a>(
    snapshot: &'a Value,
    state: &str,
) -> Option<&'a Value> {
    live_response_latency_slice_in_scope(snapshot, "rolling_window", state)
}

fn live_response_latency_current_session_relation<'a>(snapshot: &'a Value) -> Option<&'a Value> {
    let root = live_response_latency_root(snapshot)?;
    root["current_session_relation"]
        .is_object()
        .then_some(&root["current_session_relation"])
}

fn live_response_latency_current_session_relation_note(snapshot: &Value) -> Option<String> {
    live_response_latency_current_session_relation(snapshot)?["note"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn live_response_latency_current_session_exclusions_note(snapshot: &Value) -> Option<String> {
    let root = live_response_latency_root(snapshot)?;
    let exclusions = &root["current_session_exclusions"];
    let total = exclusions["total"].as_u64().unwrap_or_default();
    if total == 0 {
        return None;
    }
    let missing_thread_id = exclusions["missing_thread_id"].as_u64().unwrap_or_default();
    let quality_rejected = exclusions["quality_rejected"].as_u64().unwrap_or_default();
    let invalid_latency = exclusions["invalid_latency"].as_u64().unwrap_or_default();
    let outside_gap = exclusions["outside_current_series_window"]
        .as_u64()
        .unwrap_or_default();
    let current_series_minutes = exclusions["current_series_minutes"].as_u64().unwrap_or(60);
    Some(format!(
        "Из текущей серии ({} мин) исключено: {total} (нет thread_id: {missing_thread_id}, quality_rejected: {quality_rejected}, invalid_latency: {invalid_latency}, вне окна серии: {outside_gap}).",
        current_series_minutes
    ))
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

fn live_latency_compare_status(snapshot: &Value) -> &'static str {
    let hot_targets = live_latency_table_targets(snapshot, "hot");
    let cold_targets = live_latency_table_targets(snapshot, "cold");
    let hot_status = assess_live_latency_slice(
        rolling_window_live_response_latency_slice(snapshot, "hot"),
        &hot_targets,
    )
    .status;
    let cold_status = assess_live_latency_slice(
        rolling_window_live_response_latency_slice(snapshot, "cold"),
        &cold_targets,
    )
    .status;
    combine_live_compare_status(&[hot_status, cold_status])
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

fn savings_status(
    saved_tokens: Option<i64>,
    counted_events: u64,
    events_total: u64,
) -> &'static str {
    if counted_events == 0 {
        if events_total == 0 {
            "unknown"
        } else {
            "waiting"
        }
    } else if saved_tokens.unwrap_or_default() < 0 {
        "alert"
    } else {
        "pass"
    }
}

fn continuity_boundary_pressure(summary: &Value, alignment: &Value) -> Option<(u64, u64)> {
    if alignment["explicit_boundary_surface"]["state"].as_str() != Some("amai_continuity_boundary")
    {
        return None;
    }
    let boundary_tokens = summary["observed_continuity_restore_tokens"]
        .as_u64()
        .unwrap_or(0);
    if boundary_tokens == 0 {
        return None;
    }
    let strict_tokens = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
        .as_u64()
        .unwrap_or(0);
    Some((boundary_tokens, strict_tokens))
}

fn continuity_boundary_pressure_sentence(boundary_tokens: u64, strict_tokens: u64) -> String {
    if strict_tokens > 0 {
        format!(
            "Сейчас живой расход уже уходит в continuity startup: {} токенов continuity-restore против {} токенов strict same-meter slice по клиентскому запросу.",
            format_u64(Some(boundary_tokens)),
            format_u64(Some(strict_tokens))
        )
    } else {
        format!(
            "Сейчас живой расход уже уходит в continuity startup: {} токенов continuity-restore при нулевом strict same-meter slice по клиентскому запросу.",
            format_u64(Some(boundary_tokens))
        )
    }
}

fn continuity_boundary_pressure_is_alert(
    saved_tokens: Option<i64>,
    boundary_tokens: u64,
    strict_tokens: u64,
) -> bool {
    saved_tokens.unwrap_or_default() <= 0
        && boundary_tokens >= strict_tokens.saturating_mul(4).max(256)
}

fn recovery_sentence(median_recovery_tokens: Option<f64>) -> String {
    match median_recovery_tokens {
        Some(value) if value > 0.0 => {
            format!(
                "Медианный штраф на доуточнение: {} токенов.",
                value.round() as i64
            )
        }
        Some(_) => "Доуточнения пока не отъедали токены назад.".to_string(),
        None => "Штраф на доуточнение пока ещё не накоплен.".to_string(),
    }
}

fn current_session_lane_rows(summary: &Value, exact_pair_materialized: bool) -> Vec<Value> {
    let verified_tooltip = if exact_pair_materialized {
        "Здесь считаются только те живые запросы, где польза Amai уже подтвердилась без потери качества. Это внутренний retrieval/recovery KPI Amai: он не тождествен exact model-token pair ниже, где дополнительно учитываются same-meter whole-cycle компоненты."
    } else {
        "Здесь считаются только те живые запросы, где польза Amai уже подтвердилась без потери качества."
    };
    let total_tooltip = if exact_pair_materialized {
        "Здесь показаны все живые запросы подряд, даже если они ещё не вошли в главный итог. Это внутренний retrieval/recovery KPI Amai: он не тождествен exact model-token pair ниже, где дополнительно учитываются same-meter whole-cycle компоненты."
    } else {
        "Здесь показаны все живые запросы подряд, даже если они ещё не вошли в главный итог."
    };
    vec![
        metric_row(
            "Главный итог",
            token_lane_summary(
                summary["verified_baseline_tokens"].as_u64(),
                summary["verified_delivered_tokens"].as_u64(),
                summary["verified_recovery_tokens"].as_u64(),
                summary["verified_effective_saved_tokens"].as_i64(),
            ),
            Some(verified_tooltip),
        ),
        metric_row(
            "Весь живой поток",
            token_lane_summary(
                summary["total_naive_tokens"].as_u64(),
                summary["total_context_tokens"].as_u64(),
                summary["total_recovery_tokens"].as_u64(),
                summary["total_effective_saved_tokens"].as_i64(),
            ),
            Some(total_tooltip),
        ),
        metric_row(
            "Пока вне главного итога",
            format!(
                "{}, разница {}",
                format_count_with_word(
                    summary["excluded_events_count"].as_u64().unwrap_or(0),
                    "событие",
                    "события",
                    "событий"
                ),
                format_signed_count(summary["excluded_effective_saved_tokens"].as_i64())
            ),
            Some(
                "Сколько событий ещё не вошло в главный итог и на какую разницу по токенам они сейчас влияют.",
            ),
        ),
    ]
}

fn raw_savings_sentence(
    baseline_tokens: Option<u64>,
    delivered_tokens: Option<u64>,
    savings_percent: Option<f64>,
) -> String {
    match (baseline_tokens, delivered_tokens) {
        (Some(baseline), Some(delivered)) => format!(
            "По всему живому потоку этой сессии пока видно так: без Amai было бы {} токенов, от Amai пришло {}{}.",
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            savings_percent
                .map(|value| format!(", предварительная разница {}", format_percent(Some(value))))
                .unwrap_or_default()
        ),
        _ => {
            "По всему живому потоку этой сессии пока ещё не накопилась понятная пара «без Amai / с Amai».".to_string()
        }
    }
}

fn client_budget_disclaimer() -> &'static str {
    "Это не процент от лимита этого чата. Здесь считается только размер контекста, который Amai приносит в ответ, а не все токены разговора целиком."
}

fn exact_model_token_pair(
    scope_summary: &Value,
    alignment: &Value,
) -> Option<(u64, u64, i64, f64)> {
    if alignment["same_meter_as_client_limit"].as_bool() != Some(true) {
        return None;
    }
    let without_amai = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
        .as_u64()
        .or_else(|| {
            alignment["baseline_equivalence"]["measured_baseline_tokens_lower_bound"].as_u64()
        })
        .or_else(|| {
            scope_summary["verified_without_amai_measured_tokens"]
                .as_u64()
                .or_else(|| scope_summary["verified_baseline_tokens"].as_u64())
        })
        .unwrap_or(0);
    let with_amai = scope_summary["observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .or_else(|| scope_summary["verified_observed_whole_cycle_with_amai_tokens"].as_u64())
        .or_else(|| scope_summary["with_amai_measured_tokens"].as_u64())
        .or_else(|| scope_summary["verified_with_amai_measured_tokens"].as_u64())
        .unwrap_or(0);
    if without_amai == 0 {
        return None;
    }
    let saved_tokens = without_amai as i64 - with_amai as i64;
    let saved_pct = if without_amai == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / without_amai as f64
    };
    Some((without_amai, with_amai, saved_tokens, saved_pct))
}

fn historical_startup_drag(
    current_exact_pair: Option<(u64, u64, i64, f64)>,
    rolling_exact_pair: Option<(u64, u64, i64, f64)>,
    current_summary: &Value,
    rolling_summary: &Value,
) -> Option<HistoricalStartupDrag> {
    let (current_without, current_with, current_saved, _) = current_exact_pair?;
    let (rolling_without, rolling_with, rolling_saved, _) = rolling_exact_pair?;
    if current_saved <= 0 || rolling_saved >= 0 {
        return None;
    }
    if rolling_without <= current_without || rolling_with <= current_with {
        return None;
    }
    let older_without_amai_tokens = rolling_without.saturating_sub(current_without);
    let older_with_amai_tokens = rolling_with.saturating_sub(current_with);
    if older_without_amai_tokens == 0 && older_with_amai_tokens == 0 {
        return None;
    }
    let older_delta_tokens = older_with_amai_tokens as i64 - older_without_amai_tokens as i64;
    if older_delta_tokens <= 0 {
        return None;
    }
    let current_continuity_tokens = current_summary["observed_continuity_restore_tokens"]
        .as_u64()
        .unwrap_or(0);
    let rolling_continuity_tokens = rolling_summary["observed_continuity_restore_tokens"]
        .as_u64()
        .unwrap_or(0);
    let older_continuity_tokens =
        rolling_continuity_tokens.saturating_sub(current_continuity_tokens);
    Some(HistoricalStartupDrag {
        older_without_amai_tokens,
        older_with_amai_tokens,
        older_delta_tokens,
        current_continuity_tokens,
        older_continuity_tokens,
    })
}

fn historical_startup_drag_note_sentence(drag: Option<HistoricalStartupDrag>) -> Option<String> {
    let drag = drag?;
    Some(format!(
        "Свежая текущая сессия уже profitable, но рабочее окно всё ещё тянет исторический startup-хвост вне текущей сессии: без Amai было {}, с Amai стало {}, это +{} токенов к расходу. Из continuity-restore в текущую сессию приходится {}, а на старший хвост окна — ещё {}.",
        format_u64(Some(drag.older_without_amai_tokens)),
        format_u64(Some(drag.older_with_amai_tokens)),
        format_u64(Some(drag.older_delta_tokens as u64)),
        format_u64(Some(drag.current_continuity_tokens)),
        format_u64(Some(drag.older_continuity_tokens))
    ))
}

fn historical_startup_drag_metric_row(drag: Option<HistoricalStartupDrag>) -> Option<Value> {
    let drag = drag?;
    Some(metric_row(
        "Исторический startup-хвост",
        format!(
            "вне текущей сессии: без Amai {}, с Amai {}, +{} к расходу",
            format_u64(Some(drag.older_without_amai_tokens)),
            format_u64(Some(drag.older_with_amai_tokens)),
            format_u64(Some(drag.older_delta_tokens as u64))
        ),
        Some(
            format!(
                "Этот ряд отделяет свежую текущую сессию от более раннего continuity-startup cohort внутри рабочего окна. Из observed continuity-restore {} токенов приходятся на текущую сессию, а {} остаются в историческом хвосте окна.",
                format_u64(Some(drag.current_continuity_tokens)),
                format_u64(Some(drag.older_continuity_tokens))
            )
            .as_str(),
        ),
    ))
}

fn exact_model_component_deltas(alignment: &Value) -> Vec<(String, u64, u64, i64)> {
    let mut deltas = alignment["baseline_equivalence"]["component_semantics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["whole_cycle_observed_complete"].as_bool() == Some(true))
        .filter_map(|item| {
            let code = item["code"].as_str()?;
            let label = human_client_limit_component(code)?;
            let baseline = item["baseline_measured_tokens"].as_u64()?;
            let observed = item["observed_tokens"].as_u64()?;
            Some((
                label.to_string(),
                baseline,
                observed,
                observed as i64 - baseline as i64,
            ))
        })
        .collect::<Vec<_>>();
    deltas.sort_by(|left, right| {
        right
            .3
            .abs()
            .cmp(&left.3.abs())
            .then_with(|| left.0.cmp(&right.0))
    });
    deltas
}

fn format_exact_model_component_delta_value(
    label: &str,
    baseline: u64,
    observed: u64,
    delta: i64,
) -> String {
    if delta > 0 {
        format!(
            "{label}: {} -> {} (+{} к расходу)",
            format_u64(Some(baseline)),
            format_u64(Some(observed)),
            format_u64(Some(delta as u64))
        )
    } else if delta < 0 {
        format!(
            "{label}: {} -> {} (экономия {})",
            format_u64(Some(baseline)),
            format_u64(Some(observed)),
            format_u64(Some(delta.unsigned_abs()))
        )
    } else {
        format!(
            "{label}: {} -> {} (без разницы)",
            format_u64(Some(baseline)),
            format_u64(Some(observed))
        )
    }
}

fn exact_model_component_delta_metric_row(alignment: &Value) -> Option<Value> {
    let all_components = exact_model_component_deltas(alignment);
    let (label, baseline, observed, delta) = all_components
        .iter()
        .find(|item| item.3 != 0)
        .or_else(|| all_components.first())
        .cloned()?;
    let mut tooltip = String::from(
        "Этот ряд показывает, в каком same-meter компоненте сейчас сидит главная exact-разница между baseline «без Amai» и observed расходом «с Amai». Формат: baseline -> observed.",
    );
    for (component_label, component_baseline, component_observed, component_delta) in all_components
    {
        tooltip.push('\n');
        tooltip.push_str("- ");
        tooltip.push_str(&format_exact_model_component_delta_value(
            &component_label,
            component_baseline,
            component_observed,
            component_delta,
        ));
    }
    Some(metric_row(
        "Главный драйвер exact-пары",
        format_exact_model_component_delta_value(&label, baseline, observed, delta),
        Some(tooltip.as_str()),
    ))
}

fn exact_model_component_delta_note_sentence(alignment: &Value) -> Option<String> {
    let (label, baseline, observed, delta) = exact_model_component_deltas(alignment)
        .into_iter()
        .find(|item| item.3 != 0)?;
    Some(if delta > 0 {
        format!(
            "Главную exact-разницу сейчас даёт {label}: без Amai было {}, с Amai стало {}, это +{} токенов к расходу в том же meter.",
            format_u64(Some(baseline)),
            format_u64(Some(observed)),
            format_u64(Some(delta as u64))
        )
    } else {
        format!(
            "Главную exact-разницу сейчас даёт {label}: без Amai было {}, с Amai стало {}, это уже экономия {} токенов в том же meter.",
            format_u64(Some(baseline)),
            format_u64(Some(observed)),
            format_u64(Some(delta.unsigned_abs()))
        )
    })
}

fn client_live_meter_is_observed(client_live_meter: &Value) -> bool {
    client_live_meter["status"].as_str() == Some("observed")
}

fn exact_status_bar_rate_limits(client_live_meter: &Value) -> Option<&Value> {
    let exact = &client_live_meter["status_bar_rate_limits"];
    (exact["status"].as_str() == Some("observed")).then_some(exact)
}

fn preferred_client_limit_meter_surface(client_live_meter: &Value) -> Option<&Value> {
    exact_status_bar_rate_limits(client_live_meter)
        .or_else(|| client_live_meter_is_observed(client_live_meter).then_some(client_live_meter))
}

fn preferred_client_limit_meter_is_exact(client_live_meter: &Value) -> bool {
    exact_status_bar_rate_limits(client_live_meter).is_some()
}

fn preferred_client_limit_observed_at_epoch_ms(client_live_meter: &Value) -> Option<u64> {
    preferred_client_limit_meter_surface(client_live_meter).and_then(|surface| {
        surface["observed_at_epoch_ms"]
            .as_u64()
            .or_else(|| surface["ended_at_epoch_ms"].as_u64())
    })
}

fn client_limit_remaining_percent(surface: &Value, remaining_key: &str, used_key: &str) -> f64 {
    surface[remaining_key]
        .as_f64()
        .or_else(|| surface[remaining_key].as_u64().map(|value| value as f64))
        .or_else(|| surface[used_key].as_f64().map(|used| 100.0 - used))
        .or_else(|| surface[used_key].as_u64().map(|used| 100.0 - used as f64))
        .unwrap_or(100.0)
}

fn client_live_meter_current_thread_bound(client_live_meter: &Value) -> bool {
    client_live_meter["current_thread_bound"]
        .as_bool()
        .unwrap_or_else(|| {
            client_live_meter["thread_binding_state"]
                .as_str()
                .map(|value| value == "current_thread_bound")
                .unwrap_or(true)
        })
}

fn current_session_client_live_meter_available(client_live_meter: &Value) -> bool {
    client_live_meter_is_observed(client_live_meter)
        && client_live_meter_current_thread_bound(client_live_meter)
}

pub(crate) fn client_budget_root_cause_payload(snapshot: &Value) -> Value {
    let guard = current_session_budget_guard(snapshot);
    client_budget_root_cause_payload_with_guard(snapshot, &guard)
}

pub(crate) fn client_budget_root_cause_payload_with_guard(
    snapshot: &Value,
    guard: &Value,
) -> Value {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    let client_live_meter = &report["client_live_meter"];
    let current_live_turn = &report["current_live_turn"];
    let current_session_statement = &report["statement_previews"]["current_session"];
    let alignment = &current_session_statement["client_limit_meter_alignment"];
    let hourly_burn = &report["client_limit_hourly_burn"];
    let strict_lower_bound_tokens = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
        .as_u64()
        .or_else(|| {
            alignment["baseline_equivalence"]["measured_baseline_tokens_lower_bound"].as_u64()
        });
    let same_meter_exact_pair = exact_model_token_pair(current_session_statement, alignment);
    let continuity_restore_component = alignment["baseline_equivalence"]["component_semantics"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["code"].as_str() == Some("continuity_restore_outside_retrieval"));
    let continuity_restore_baseline_tokens =
        continuity_restore_component.and_then(|item| item["baseline_measured_tokens"].as_u64());
    let continuity_restore_observed_tokens =
        continuity_restore_component.and_then(|item| item["observed_tokens"].as_u64());
    let continuity_restore_delta_tokens = continuity_restore_baseline_tokens
        .zip(continuity_restore_observed_tokens)
        .map(|(baseline_tokens, observed_tokens)| observed_tokens as i64 - baseline_tokens as i64);
    let current_turn_total_tokens = client_live_meter["client_turn_total_tokens"].as_u64();
    let full_turn_overhang_tokens = current_turn_total_tokens
        .zip(strict_lower_bound_tokens)
        .map(|(turn_total_tokens, strict_tokens)| turn_total_tokens.saturating_sub(strict_tokens))
        .filter(|value| *value > 0);
    let full_turn_vs_strict_ratio = current_turn_total_tokens
        .zip(strict_lower_bound_tokens)
        .and_then(|(turn_total_tokens, strict_tokens)| {
            if strict_tokens == 0 {
                None
            } else {
                Some(turn_total_tokens as f64 / strict_tokens as f64)
            }
        });
    let dominant_cost_surface = if current_live_turn["status"].as_str()
        == Some("no_amai_activity_in_current_live_turn")
        && full_turn_overhang_tokens
            .zip(strict_lower_bound_tokens)
            .is_some_and(|(overhang_tokens, strict_tokens)| {
                overhang_tokens >= strict_tokens.saturating_mul(4).max(256)
            }) {
        Some("giant_thread_context_outside_same_meter_slice")
    } else {
        None
    };
    let selected_host_current_thread_control_effect =
        guard["host_current_thread_control_effect"].clone();
    let primary_blocker = alignment["exact_pair_status"]["blockers"]
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(Value::Null);
    let missing_live_events = primary_blocker["missing_live_events"].as_u64().unwrap_or(0);
    let irrecoverable_missing_live_events = primary_blocker["irrecoverable_missing_live_events"]
        .as_u64()
        .unwrap_or(0);
    let recoverable_missing_live_events =
        missing_live_events.saturating_sub(irrecoverable_missing_live_events);
    let live_status = if current_session_client_live_meter_available(client_live_meter)
        || preferred_client_limit_meter_surface(client_live_meter).is_some()
    {
        "observed"
    } else {
        client_live_meter["status"].as_str().unwrap_or("missing")
    };
    let mut current_live_turn_payload = serde_json::Map::new();
    current_live_turn_payload.insert("status".to_string(), current_live_turn["status"].clone());
    current_live_turn_payload.insert(
        "exact_pair_available".to_string(),
        current_live_turn["exact_pair_available"].clone(),
    );
    if current_live_turn["exact_pair_available"].as_bool() == Some(true) {
        let exact_pair = &current_live_turn["exact_pair"];
        let exact_pair_is_zero = exact_pair["without_amai_tokens"].as_u64().unwrap_or(0) == 0
            && exact_pair["with_amai_tokens"].as_u64().unwrap_or(0) == 0
            && exact_pair["saved_tokens"].as_i64().unwrap_or(0) == 0;
        if exact_pair_is_zero
            && current_live_turn["status"].as_str() == Some("no_amai_activity_in_current_live_turn")
        {
            current_live_turn_payload
                .insert("saved_pct".to_string(), exact_pair["saved_pct"].clone());
        } else {
            current_live_turn_payload.insert("exact_pair".to_string(), exact_pair.clone());
        }
    }
    for field in [
        "observed_client_prompt_tokens",
        "observed_assistant_generation_tokens",
        "observed_continuity_restore_tokens",
        "observed_tool_overhead_tokens",
        "observed_whole_cycle_with_amai_tokens",
        "verified_observed_whole_cycle_with_amai_tokens",
    ] {
        if !current_live_turn[field].is_null() {
            current_live_turn_payload.insert(field.to_string(), current_live_turn[field].clone());
        }
    }

    let mut exact_pair_status_payload = serde_json::Map::new();
    if current_live_turn["status"].as_str() == Some("no_amai_activity_in_current_live_turn") {
        exact_pair_status_payload.insert(
            "state".to_string(),
            Value::from("not_applicable_current_live_turn_has_no_amai_activity"),
        );
        exact_pair_status_payload.insert("exact_pair_available".to_string(), Value::from(true));
        exact_pair_status_payload.insert(
            "note".to_string(),
            Value::from(
                "В текущем live-turn у Amai нет активности, поэтому exact-pair blocker surface здесь не про missing measurement, а про нулевой вклад: для этого turn Amai честно даёт 0.00% same-meter savings.",
            ),
        );
    } else {
        exact_pair_status_payload.insert(
            "state".to_string(),
            alignment["exact_pair_status"]["state"].clone(),
        );
        exact_pair_status_payload.insert(
            "exact_pair_available".to_string(),
            alignment["exact_pair_status"]["exact_pair_available"].clone(),
        );
        for (field, value) in [
            (
                "primary_blocking_reason",
                alignment["exact_pair_status"]["primary_blocking_reason"].clone(),
            ),
            ("primary_blocker_code", primary_blocker["code"].clone()),
            (
                "primary_blocker_kind",
                primary_blocker["blocker_kind"].clone(),
            ),
            (
                "blocking_reason",
                primary_blocker["blocking_reason"].clone(),
            ),
            (
                "note",
                exact_pair_primary_blocker_note_sentence(alignment)
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            ),
        ] {
            if !value.is_null() {
                exact_pair_status_payload.insert(field.to_string(), value);
            }
        }
        if missing_live_events > 0 {
            exact_pair_status_payload.insert(
                "missing_live_events".to_string(),
                Value::from(missing_live_events),
            );
        }
        if irrecoverable_missing_live_events > 0 {
            exact_pair_status_payload.insert(
                "irrecoverable_missing_live_events".to_string(),
                Value::from(irrecoverable_missing_live_events),
            );
        }
        if recoverable_missing_live_events > 0 {
            exact_pair_status_payload.insert(
                "recoverable_missing_live_events".to_string(),
                Value::from(recoverable_missing_live_events),
            );
        }
    }

    let mut payload = serde_json::Map::new();
    payload.insert("status".to_string(), json!(live_status));
    payload.insert(
        "reply_prefix".to_string(),
        hourly_burn["reply_prefix"].clone(),
    );
    payload.insert(
        "thread_binding_state".to_string(),
        client_live_meter["thread_binding_state"].clone(),
    );
    payload.insert(
        "current_thread_bound".to_string(),
        client_live_meter["current_thread_bound"].clone(),
    );
    payload.insert(
        "current_live_meter".to_string(),
        json!({
            "ended_at_epoch_ms": preferred_client_limit_observed_at_epoch_ms(client_live_meter)
                .map(Value::from)
                .unwrap_or_else(|| client_live_meter["ended_at_epoch_ms"].clone()),
            "client_turn_total_tokens": client_live_meter["client_turn_total_tokens"].clone(),
            "context_used_percent": client_live_meter["context_used_percent"].clone(),
        }),
    );
    payload.insert(
        "guard".to_string(),
        json!({
            "status_label": guard["status_label"].clone(),
            "should_rotate_chat_now": guard["should_rotate_chat_now"].clone(),
            "should_rotate_chat_soon": guard["should_rotate_chat_soon"].clone(),
            "action_kind": guard["reply_execution_gate"]["action_kind"].clone(),
            "reply_budget_mode": guard["reply_execution_gate"]["reply_budget_mode"].clone(),
            "must_rotate_before_reply": guard["reply_execution_gate"]["must_rotate_before_reply"].clone(),
        }),
    );
    payload.insert(
        "host_context_compaction".to_string(),
        compact_host_context_compaction_payload(&guard["host_context_compaction"]),
    );
    payload.insert(
        "host_current_thread_control_effect".to_string(),
        selected_host_current_thread_control_effect,
    );
    payload.insert(
        "current_live_turn".to_string(),
        Value::Object(current_live_turn_payload),
    );
    payload.insert(
        "exact_pair_status".to_string(),
        Value::Object(exact_pair_status_payload),
    );
    let mut same_meter_economics_payload = serde_json::Map::new();
    if let Some(strict_tokens) = strict_lower_bound_tokens {
        same_meter_economics_payload.insert(
            "strict_lower_bound_tokens".to_string(),
            Value::from(strict_tokens),
        );
    }
    if let Some((without_amai_tokens, with_amai_tokens, saved_tokens, saved_pct)) =
        same_meter_exact_pair
    {
        same_meter_economics_payload.insert(
            "same_meter_without_amai_tokens".to_string(),
            Value::from(without_amai_tokens),
        );
        same_meter_economics_payload.insert(
            "same_meter_with_amai_tokens".to_string(),
            Value::from(with_amai_tokens),
        );
        same_meter_economics_payload.insert(
            "same_meter_saved_tokens".to_string(),
            Value::from(saved_tokens),
        );
        if let Some(saved_pct_value) = serde_json::Number::from_f64(saved_pct) {
            same_meter_economics_payload.insert(
                "same_meter_saved_pct".to_string(),
                Value::Number(saved_pct_value),
            );
        }
    }
    if let Some(baseline_tokens) = continuity_restore_baseline_tokens {
        same_meter_economics_payload.insert(
            "continuity_restore_baseline_tokens".to_string(),
            Value::from(baseline_tokens),
        );
    }
    if let Some(observed_tokens) = continuity_restore_observed_tokens {
        same_meter_economics_payload.insert(
            "continuity_restore_observed_tokens".to_string(),
            Value::from(observed_tokens),
        );
    }
    if let Some(delta_tokens) = continuity_restore_delta_tokens {
        same_meter_economics_payload.insert(
            "continuity_restore_delta_tokens".to_string(),
            Value::from(delta_tokens),
        );
    }
    if let Some(overhang_tokens) = full_turn_overhang_tokens {
        same_meter_economics_payload.insert(
            "full_turn_overhang_tokens".to_string(),
            Value::from(overhang_tokens),
        );
    }
    if let Some(ratio) = full_turn_vs_strict_ratio.and_then(serde_json::Number::from_f64) {
        same_meter_economics_payload.insert(
            "full_turn_vs_strict_ratio".to_string(),
            Value::Number(ratio),
        );
    }
    if let Some(surface) = dominant_cost_surface {
        same_meter_economics_payload
            .insert("dominant_cost_surface".to_string(), Value::from(surface));
    }
    if !same_meter_economics_payload.is_empty() {
        payload.insert(
            "same_meter_economics".to_string(),
            Value::Object(same_meter_economics_payload),
        );
    }
    for field in [
        "measured_components",
        "missing_components",
        "partially_measured_components",
        "blocking_reasons",
    ] {
        if alignment[field]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            payload.insert(field.to_string(), alignment[field].clone());
        }
    }
    Value::Object(payload)
}

fn current_live_turn_exact_pair(current_live_turn: &Value) -> Option<(u64, u64, i64, f64)> {
    if current_live_turn["exact_pair_available"].as_bool() != Some(true) {
        return None;
    }
    let exact_pair = &current_live_turn["exact_pair"];
    Some((
        exact_pair["without_amai_tokens"].as_u64().unwrap_or(0),
        exact_pair["with_amai_tokens"].as_u64().unwrap_or(0),
        exact_pair["saved_tokens"].as_i64().unwrap_or(0),
        exact_pair["saved_pct"].as_f64().unwrap_or(0.0),
    ))
}

fn live_turn_exact_pair(
    current_session: &Value,
    client_live_meter: &Value,
    exact_pair: Option<(u64, u64, i64, f64)>,
) -> Option<(u64, u64, i64, f64)> {
    let exact_pair = exact_pair?;
    if current_session["counted_events"].as_u64().unwrap_or(0) != 1 {
        return None;
    }
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let session_started = current_session["started_at_epoch_ms"].as_i64().unwrap_or(0);
    let session_ended = current_session["ended_at_epoch_ms"].as_i64().unwrap_or(0);
    let live_started = client_live_meter["started_at_epoch_ms"]
        .as_i64()
        .unwrap_or(0);
    let live_ended = client_live_meter["ended_at_epoch_ms"].as_i64().unwrap_or(0);
    if session_started <= 0 || session_ended <= 0 || live_started <= 0 || live_ended <= 0 {
        return None;
    }
    let max_gap_ms = 15_000i64;
    let started_gap = (session_started - live_started).abs();
    let ended_gap = (session_ended - live_ended).abs();
    if started_gap > max_gap_ms || ended_gap > max_gap_ms {
        return None;
    }
    Some(exact_pair)
}

fn full_turn_savings_pct_from_live_meter(
    client_live_meter: &Value,
    exact_pair: Option<(u64, u64, i64, f64)>,
) -> Option<f64> {
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let (_, _, saved_tokens, _) = exact_pair?;
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .unwrap_or(0);
    if turn_total_tokens == 0 {
        return None;
    }
    let without_amai_total_tokens = if saved_tokens >= 0 {
        turn_total_tokens.saturating_add(saved_tokens as u64)
    } else {
        turn_total_tokens.saturating_sub(saved_tokens.unsigned_abs())
    };
    if without_amai_total_tokens == 0 {
        return None;
    }
    Some((saved_tokens as f64 * 100.0) / without_amai_total_tokens as f64)
}

fn client_full_turn_savings_metric_row(
    client_live_meter: &Value,
    exact_pair: Option<(u64, u64, i64, f64)>,
) -> Option<Value> {
    if !client_live_meter_is_observed(client_live_meter) {
        return None;
    }
    if !current_session_client_live_meter_available(client_live_meter) {
        return Some(metric_row_with_key(
            CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY,
            "Amai в полном live-turn",
            "точный процент по шкале VS Code пока не доказан".to_string(),
            Some(
                "Этот ряд должен показывать единственный процент, который напрямую коррелирует с замедлением расхода шкалы VS Code. Сейчас current-thread binding для live meter ещё не materialized, поэтому exact full-turn pair для текущего чата честно не доказывается.",
            ),
        ));
    }
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .unwrap_or(0);
    if turn_total_tokens == 0 {
        return None;
    }
    let Some((_, _, saved_tokens, _)) = exact_pair else {
        return Some(metric_row_with_key(
            CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY,
            "Amai в полном live-turn",
            "точный процент по шкале VS Code пока не доказан".to_string(),
            Some(
                "Этот ряд должен показывать единственный процент, который напрямую коррелирует с замедлением расхода шкалы VS Code. Пока exact full-turn pair для текущего live turn ещё не materialized, поэтому процент здесь честно не показывается.",
            ),
        ));
    };
    let without_amai_total_tokens = if saved_tokens >= 0 {
        turn_total_tokens.saturating_add(saved_tokens as u64)
    } else {
        turn_total_tokens.saturating_sub(saved_tokens.unsigned_abs())
    };
    if without_amai_total_tokens == 0 {
        return None;
    }
    let full_turn_savings_pct =
        full_turn_savings_pct_from_live_meter(client_live_meter, exact_pair)?;
    let tooltip = format!(
        "Этот ряд показывает реальный вклад Amai в полный live-turn клиента, а не только во внутренний Amai-side slice.\n- Без Amai: {}\n- С Amai: {}\n- Delta Amai: {}\n- Процент от полного turn: {}\n- Этот процент должен напрямую коррелировать с замедлением расхода шкалы VS Code.\n- Источник observed full turn: rollout token_count.last_token_usage.total_tokens",
        format_u64(Some(without_amai_total_tokens)),
        format_u64(Some(turn_total_tokens)),
        format_signed_count(Some(saved_tokens)),
        format_percent(Some(full_turn_savings_pct)),
    );
    Some(metric_row_with_key(
        CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY,
        "Amai в полном live-turn",
        format!(
            "{}: без Amai {}, с Amai {}, delta {}",
            format_percent(Some(full_turn_savings_pct)),
            format_u64(Some(without_amai_total_tokens)),
            format_u64(Some(turn_total_tokens)),
            format_signed_count(Some(saved_tokens))
        ),
        Some(tooltip.as_str()),
    ))
}

fn exact_pair_primary_blocker_note_sentence(alignment: &Value) -> Option<String> {
    let blocker = alignment["exact_pair_status"]["blockers"]
        .as_array()?
        .first()?;
    let code = blocker["code"].as_str().unwrap_or_default();
    match code {
        "tool_overhead_outside_retrieval" => {
            let missing_live_events = blocker["missing_live_events"].as_u64().unwrap_or(0);
            let irrecoverable_missing_live_events = blocker["irrecoverable_missing_live_events"]
                .as_u64()
                .unwrap_or(0);
            let recoverable_missing_live_events = missing_live_events
                .saturating_sub(irrecoverable_missing_live_events);
            Some(format!(
                "Exact pair сейчас удерживает tool-overhead outside retrieval: missing {} live events, из них {} irrecoverable и {} ещё recoverable.",
                format_u64(Some(missing_live_events)),
                format_u64(Some(irrecoverable_missing_live_events)),
                format_u64(Some(recoverable_missing_live_events))
            ))
        }
        "assistant_generation" => Some(
            "Exact pair сейчас удерживает assistant-generation baseline semantics: observed output tokens уже видны, но deduplicated same-meter baseline для этого scope ещё не materialized."
                .to_string(),
        ),
        "continuity_restore_outside_retrieval" => Some(
            "Exact pair сейчас удерживает continuity-restore boundary: truthful pre-Amai baseline для этого scope ещё не materialized."
                .to_string(),
        ),
        _ => blocker["blocking_reason"]
            .as_str()
            .map(|reason| format!("Exact pair сейчас удерживает blocker `{reason}`.")),
    }
}

fn exact_pair_card_status_override(
    alignment: &Value,
) -> Option<(&'static str, &'static str, String)> {
    let exact_pair_status = &alignment["exact_pair_status"];
    if exact_pair_status["state"].as_str() != Some("exact_pair_blocked") {
        return None;
    }
    let blocker = exact_pair_status["blockers"].as_array()?.first()?;
    let blocker_code = blocker["code"].as_str().unwrap_or_default();
    let irrecoverable_missing_live_events = blocker["irrecoverable_missing_live_events"]
        .as_u64()
        .unwrap_or(0);
    let missing_live_events = blocker["missing_live_events"].as_u64().unwrap_or(0);
    let recoverable_missing_live_events =
        missing_live_events.saturating_sub(irrecoverable_missing_live_events);
    if blocker_code == "tool_overhead_outside_retrieval" && irrecoverable_missing_live_events > 0 {
        return Some((
            "alert",
            "есть старый долг точности",
            format!(
                "Карточка пока не может считаться полностью точной по следующим причинам:\n- Полное совпадение с реальной шкалой лимита модели ещё не собрано.\n- Главный blocker: tool-overhead outside retrieval.\n- Не хватает строк: {}.\n- Потеряно без восстановления: {}.\n- Ещё можно восстановить: {}.\n- Это уже не временный лаг, а старый исторический хвост, поэтому зелёный точный статус здесь запрещён.",
                format_u64(Some(missing_live_events)),
                format_u64(Some(irrecoverable_missing_live_events)),
                format_u64(Some(recoverable_missing_live_events))
            ),
        ));
    }
    Some((
        "waiting",
        "ждём полного совпадения",
        "Карточка пока не может считаться полностью точной: совпадение с реальной шкалой лимита модели ещё не собрано.".to_string(),
    ))
}

fn exact_pair_status_metric_row(alignment: &Value) -> Option<Value> {
    let exact_pair_status = &alignment["exact_pair_status"];
    if exact_pair_status["exact_pair_available"].as_bool() == Some(true) {
        return Some(metric_row(
            "Совпадение с реальным лимитом",
            "цифра точная: полностью совпадает со шкалой лимита модели".to_string(),
            Some(
                "Этот ряд показывает, совпадает ли процент экономии с той же шкалой токенов, по которой клиент считает лимит. Здесь совпадение полное.",
            ),
        ));
    }
    if exact_pair_status["state"].as_str() != Some("exact_pair_blocked") {
        return None;
    }
    let blocker = exact_pair_status["blockers"].as_array()?.first()?;
    let missing_live_events = blocker["missing_live_events"].as_u64().unwrap_or(0);
    let irrecoverable_missing_live_events = blocker["irrecoverable_missing_live_events"]
        .as_u64()
        .unwrap_or(0);
    let recoverable_missing_live_events =
        missing_live_events.saturating_sub(irrecoverable_missing_live_events);
    if blocker["frozen_gap_candidate"].as_bool() == Some(true) {
        let tooltip = format!(
            "Этот ряд показывает, совпадает ли процент экономии с реальной шкалой лимита модели. Сейчас совпадение неполное не из-за временного лага, а из-за старой исторической потери данных.\n- Не хватает строк: {}\n- Потеряно без восстановления: {}\n- Ещё можно восстановить: {}\n- Пока не принято отдельное решение по старому хвосту, lifetime-корреляция обязана оставаться неточной.",
            format_u64(Some(missing_live_events)),
            format_u64(Some(irrecoverable_missing_live_events)),
            format_u64(Some(recoverable_missing_live_events))
        );
        return Some(metric_row(
            "Совпадение с реальным лимитом",
            format!(
                "цифра пока не полностью точная: в старой истории потеряно {} строк",
                format_u64(Some(irrecoverable_missing_live_events))
            ),
            Some(tooltip.as_str()),
        ));
    }
    let tooltip = format!(
        "Этот ряд показывает, совпадает ли процент экономии с реальной шкалой лимита модели. Полное совпадение пока ещё не собрано.\n- Не хватает строк: {}\n- Потеряно без восстановления: {}\n- Ещё можно восстановить: {}\n- Это пока выглядит как временный и восстановимый хвост.",
        format_u64(Some(missing_live_events)),
        format_u64(Some(irrecoverable_missing_live_events)),
        format_u64(Some(recoverable_missing_live_events))
    );
    Some(metric_row(
        "Совпадение с реальным лимитом",
        format!(
            "цифра пока предварительная: ждём ещё {} строк для полного совпадения",
            format_u64(Some(missing_live_events))
        ),
        Some(tooltip.as_str()),
    ))
}

fn exact_pair_frozen_debt_metric_row(alignment: &Value) -> Option<Value> {
    let frozen_gap_review_surface = &alignment["frozen_gap_review_surface"];
    if frozen_gap_review_surface["state"].as_str() != Some("review_required") {
        return None;
    }
    let blocker_code = frozen_gap_review_surface["blocking_component"]
        .as_str()
        .unwrap_or("unknown_blocker");
    let missing_live_events = frozen_gap_review_surface["missing_live_events"]
        .as_u64()
        .unwrap_or(0);
    let irrecoverable_missing_live_events =
        frozen_gap_review_surface["irrecoverable_missing_live_events"]
            .as_u64()
            .unwrap_or(0);
    let recoverable_missing_live_events =
        frozen_gap_review_surface["recoverable_missing_live_events"]
            .as_u64()
            .unwrap_or_else(|| {
                missing_live_events.saturating_sub(irrecoverable_missing_live_events)
            });
    let resolution_condition = frozen_gap_review_surface["resolution_condition"]
        .as_str()
        .unwrap_or("freeze_irrecoverable_gap_or_keep_exact_pair_unavailable");
    let tooltip = format!(
        "Этот ряд показывает отдельный review-only contour для irrecoverable historical debt, который блокирует raw exact history.\n- Blocker component: {}\n- Missing live events: {}\n- Irrecoverable: {}\n- Recoverable: {}\n- Resolution law: {}\n- Пока frozen-gap решение не принято, `Точность модели` обязана оставаться non-exact и не имеет права притворяться raw exact history.",
        blocker_code,
        format_u64(Some(missing_live_events)),
        format_u64(Some(irrecoverable_missing_live_events)),
        format_u64(Some(recoverable_missing_live_events)),
        resolution_condition
    );
    Some(metric_row(
        "Frozen debt exact-пары",
        format!(
            "{}: {} irrecoverable rows",
            blocker_code,
            format_u64(Some(irrecoverable_missing_live_events))
        ),
        Some(tooltip.as_str()),
    ))
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

fn model_token_savings_metric_row(scope_summary: &Value, alignment: &Value) -> Value {
    let observed_with_amai = scope_summary["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(0);
    let tooltip = model_token_savings_tooltip(scope_summary, alignment);
    let preliminary = scope_summary["preliminary"].as_bool().unwrap_or(false);
    let strict_components =
        human_client_limit_components(&alignment["strict_client_meter_slice"]["components"]);

    let value = if let Some((verified_without, verified_with, verified_saved, _verified_pct)) =
        exact_model_token_pair(scope_summary, alignment)
    {
        let prefix = if preliminary {
            "Предварительный учтённый same-meter срез"
        } else {
            "Учтённый same-meter срез"
        };
        let scope_suffix = strict_components
            .as_deref()
            .map(|components| format!(" ({components})"))
            .unwrap_or_default();
        format!(
            "{prefix}{scope_suffix}: без Amai {}, с Amai {}, экономия {}",
            format_u64(Some(verified_without)),
            format_u64(Some(verified_with)),
            format_signed_count(Some(verified_saved))
        )
    } else if observed_with_amai > 0 {
        format!(
            "Точного процента пока нет; с Amai уже видно {}",
            format_u64(Some(observed_with_amai))
        )
    } else {
        "Точного процента пока нет".to_string()
    };

    metric_row("Экономия токенов модели", value, Some(tooltip.as_str()))
}

fn model_token_savings_note_sentence(scope_summary: &Value, alignment: &Value) -> Option<String> {
    let observed_with_amai = scope_summary["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(0);
    let preliminary = scope_summary["preliminary"].as_bool().unwrap_or(false);
    let counted_events = scope_summary["counted_events"].as_u64().unwrap_or(0);
    let events_total = scope_summary["events_total"].as_u64().unwrap_or(0);

    if let Some((verified_without, verified_with, verified_saved, _verified_pct)) =
        exact_model_token_pair(scope_summary, alignment)
    {
        let mut note = format!(
            "Здесь уже есть полное совпадение с реальной шкалой лимита модели для учтённого среза: без Amai было {}, с Amai стало {}, экономия {}.",
            format_u64(Some(verified_without)),
            format_u64(Some(verified_with)),
            format_signed_count(Some(verified_saved))
        );
        if let Some(components) =
            human_client_limit_components(&alignment["strict_client_meter_slice"]["components"])
        {
            note.push(' ');
            note.push_str(&format!(
                "В exact-пару здесь вошёл только strict same-meter срез: {components}."
            ));
        }
        if preliminary {
            note.push(' ');
            note.push_str(&format!(
                "Это пока предварительная выборка: учтено {} из {} событий, поэтому этот процент нельзя читать как экономию всей сессии целиком.",
                format_u64(Some(counted_events)),
                format_u64(Some(events_total))
            ));
        }
        return Some(note);
    }

    if observed_with_amai > 0 {
        let mut note = format!(
            "Точный процент экономии токенов модели здесь пока не показан: с Amai уже честно видно {}, но полного совпадения с реальной шкалой лимита модели для этого scope ещё нет.",
            format_u64(Some(observed_with_amai))
        );
        if let Some(blocker_sentence) = exact_pair_primary_blocker_note_sentence(alignment) {
            note.push(' ');
            note.push_str(&blocker_sentence);
        }
        return Some(note);
    }

    let mut note = String::from(
        "Точный процент экономии токенов модели здесь пока не показан: полное совпадение с реальной шкалой лимита модели для этого scope ещё не собрано.",
    );
    if let Some(blocker_sentence) = exact_pair_primary_blocker_note_sentence(alignment) {
        note.push(' ');
        note.push_str(&blocker_sentence);
    }
    Some(note)
}

fn model_token_savings_tooltip(statement_preview: &Value, alignment: &Value) -> String {
    let observed_with_amai = statement_preview["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(0);
    let preliminary = statement_preview["preliminary"].as_bool().unwrap_or(false);
    let counted_events = statement_preview["counted_events"].as_u64().unwrap_or(0);
    let events_total = statement_preview["events_total"].as_u64().unwrap_or(0);

    if let Some((verified_without, verified_with, verified_saved, _verified_pct)) =
        exact_model_token_pair(statement_preview, alignment)
    {
        let mut tooltip = format!(
            "Этот ряд показывает exact same-meter pair для учтённого среза, а не для всей сессии.\n- Без Amai: {}\n- С Amai: {}\n- Экономия: {}",
            format_u64(Some(verified_without)),
            format_u64(Some(verified_with)),
            format_signed_count(Some(verified_saved))
        );
        if let Some(components) =
            human_client_limit_components(&alignment["strict_client_meter_slice"]["components"])
        {
            tooltip.push_str(&format!("\n- Что вошло в срез: {components}"));
        }
        if preliminary {
            tooltip.push_str(&format!(
                "\n- Статус выборки: preliminary, учтено {} из {} событий",
                format_u64(Some(counted_events)),
                format_u64(Some(events_total))
            ));
        }
        tooltip.push_str(
            &format!(
                "\n- Slice math здесь exact и идёт по тому же meter, которым клиент считает лимит.\n- Сам процент этого slice intentionally не показывается как user-facing claim: процент на карточке разрешён только для строки «Экономия на реальной шкале».",
            ),
        );
        return tooltip;
    }

    if observed_with_amai > 0 {
        return format!(
            "Этот ряд показывает точную корреляцию между токенами модели без Amai и с Amai только после materialized same-meter pair. С Amai уже видно {} observed токенов, но exact pair для этого scope ещё не materialized, поэтому процент честно не показывается.",
            format_u64(Some(observed_with_amai))
        );
    }

    "Этот ряд показывает точную корреляцию между токенами модели без Amai и с Amai только после materialized same-meter pair. Пока exact pair для этого scope ещё не materialized, поэтому процент честно не показывается.".to_string()
}

fn client_limit_alignment_metric_row(alignment: &Value) -> Option<Value> {
    let state = alignment["alignment_state"].as_str()?;
    let live_events = alignment["live_events_count"].as_u64().unwrap_or(0);
    let non_live_events = alignment["non_live_events_count"].as_u64().unwrap_or(0);
    let value = if alignment["same_meter_as_client_limit"].as_bool() == Some(true) {
        "да".to_string()
    } else {
        match state {
            "no_usage_observed" => "ещё нет usage".to_string(),
            "only_non_live_scope_activity" => format!(
                "нет: только non-live (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "live_usage_unconfirmed_not_meter_equivalent" => format!(
                "нет: live ещё не подтверждено (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "partial_lower_bound_not_meter_equivalent" => format!(
                "нет: lower bound части цикла (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "whole_cycle_partially_observed_not_meter_equivalent" => format!(
                "нет: cycle observed частично (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "whole_cycle_observed_baseline_partial" => format!(
                "нет: cycle observed, baseline ещё partial (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            "whole_cycle_observed_explicit_boundary_not_meter_equivalent" => format!(
                "нет: strict slice есть, continuity boundary explicit (live {} / non-live {})",
                format_u64(Some(live_events)),
                format_u64(Some(non_live_events))
            ),
            other => format!("нет: {other}"),
        }
    };
    Some(metric_row(
        "Связь с лимитом клиента",
        value,
        client_limit_alignment_tooltip(alignment).as_deref(),
    ))
}

fn client_limit_strict_slice_metric_row(alignment: &Value) -> Option<Value> {
    if alignment["strict_client_meter_slice"]["same_meter_equivalent_for_slice"].as_bool()
        != Some(true)
    {
        return None;
    }
    let lower_bound = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
        .as_u64()
        .unwrap_or(0);
    if lower_bound == 0 {
        return None;
    }
    let value = if let Some(components) =
        human_client_limit_components(&alignment["strict_client_meter_slice"]["components"])
    {
        format!("{lower_bound} токенов: {components}")
    } else {
        format!("{lower_bound} токенов")
    };
    Some(metric_row(
        "Строгий same-meter срез",
        value,
        Some(
            "Этот ряд показывает уже materialized strict same-meter lower bound: часть клиентского лимитного метра, где baseline-equivalent semantics уже честно доказаны и не зависят от guessed continuity baseline.",
        ),
    ))
}

fn client_limit_explicit_boundary_metric_row(alignment: &Value) -> Option<Value> {
    if alignment["explicit_boundary_surface"]["blocks_full_same_meter_equivalence"].as_bool()
        != Some(true)
    {
        return None;
    }
    let components =
        human_client_limit_components(&alignment["explicit_boundary_surface"]["components"])?;
    let label = if alignment["explicit_boundary_surface"]["state"].as_str()
        == Some("amai_continuity_boundary")
    {
        "Граница continuity"
    } else {
        "Явная baseline-граница"
    };
    Some(metric_row(
        label,
        components,
        alignment["explicit_boundary_surface"]["note"].as_str(),
    ))
}

fn client_limit_boundary_tokens_metric_row(alignment: &Value) -> Option<Value> {
    let observed_tokens = if alignment["continuity_boundary_rollup"]["state"].as_str()
        == Some("amai_continuity_boundary_observed")
    {
        alignment["continuity_boundary_rollup"]["observed_tokens"]
            .as_u64()
            .unwrap_or(0)
    } else {
        if alignment["explicit_boundary_surface"]["state"].as_str()
            != Some("amai_continuity_boundary")
        {
            return None;
        }
        let boundary_components =
            alignment["explicit_boundary_surface"]["components"].as_array()?;
        alignment["baseline_equivalence"]["component_semantics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| {
                item["code"].as_str().is_some_and(|code| {
                    boundary_components
                        .iter()
                        .any(|component| component.as_str() == Some(code))
                })
            })
            .filter(|item| item["whole_cycle_observed_complete"].as_bool() == Some(true))
            .filter_map(|item| item["observed_tokens"].as_u64())
            .sum::<u64>()
    };
    if observed_tokens == 0 {
        return None;
    }
    Some(metric_row(
        "Токены continuity boundary",
        format!(
            "{} токенов вне strict client-meter slice",
            format_u64(Some(observed_tokens))
        ),
        Some(
            "Этот ряд показывает observed token weight для Amai-specific continuity boundary. Эти токены уже честно видны в agent cycle, но не входят в strict same-meter client slice, пока для них нет truthful pre-Amai baseline-equivalent модели.",
        ),
    ))
}

fn human_client_limit_component(code: &str) -> Option<&'static str> {
    match code {
        "client_prompt" => Some("исходный запрос клиента"),
        "assistant_generation" => Some("генерация ответа моделью"),
        "tool_overhead_outside_retrieval" => Some("tool/orchestration overhead вне retrieval"),
        "continuity_restore_outside_retrieval" => Some("continuity-restore overhead вне retrieval"),
        _ => None,
    }
}

fn human_client_limit_components(node: &Value) -> Option<String> {
    let components = node
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str())
        .filter_map(human_client_limit_component)
        .collect::<Vec<_>>();
    if components.is_empty() {
        None
    } else {
        Some(components.join(", "))
    }
}

fn client_limit_alignment_note_sentence(alignment: &Value) -> Option<String> {
    let state = alignment["alignment_state"].as_str()?;
    if alignment["same_meter_as_client_limit"].as_bool() == Some(true) {
        return Some(
            "Этот срез уже materialized в том же meter, которым клиент считает лимит: цифры карточки и точный процент model-token savings теперь коррелируют напрямую."
                .to_string(),
        );
    }
    Some(match state {
        "no_usage_observed" => {
            "Этот срез ещё не видел usage-событий, поэтому сравнивать его со шкалой лимита клиента пока рано.".to_string()
        }
        "only_non_live_scope_activity" => {
            "Сейчас в этом срезе есть только non-live активность, поэтому его цифра не обязана двигаться вместе со шкалой лимита клиента.".to_string()
        }
        "live_usage_unconfirmed_not_meter_equivalent" => {
            "Здесь уже были live-события, но confirmed lower bound ещё не набрался, поэтому эта цифра пока не эквивалентна шкале лимита клиента.".to_string()
        }
        "partial_lower_bound_not_meter_equivalent" => {
            "Даже здесь это пока lower bound части агентного цикла, а не тот же полный метр, которым клиент считает лимит сессии.".to_string()
        }
        "whole_cycle_partially_observed_not_meter_equivalent" => {
            "Здесь уже начали появляться observed whole-cycle компоненты, но покрытие ещё неполное, поэтому эта цифра всё ещё не эквивалентна шкале лимита клиента.".to_string()
        }
        "whole_cycle_observed_explicit_boundary_not_meter_equivalent" => {
            let measured = human_client_limit_components(
                &alignment["baseline_equivalence"]["measured_baseline_components"],
            );
            let boundary = human_client_limit_components(
                &alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
            );
            match (measured, boundary) {
                (Some(measured), Some(boundary)) => format!(
                    "Здесь whole-cycle компоненты уже fully observed; strict same-meter lower bound уже materialized для {measured}, а для {boundary} boundary сознательно поднят как explicit truth-boundary. Это уже не просто partial baseline, но и не полный client-limit meter."
                ),
                _ => "Здесь whole-cycle observed компоненты уже видны по live событиям, а remaining gap оставлен как explicit truth-boundary, поэтому метрика остаётся честно non-equivalent.".to_string(),
            }
        }
        "whole_cycle_observed_baseline_partial" => {
            if alignment["baseline_equivalence"]["state"].as_str()
                == Some("baseline_semantics_unmaterialized")
            {
                if let Some(fully_observed) = human_client_limit_components(
                    &alignment["baseline_equivalence"]["fully_observed_components"],
                ) {
                    format!(
                        "Здесь applicable whole-cycle компоненты уже полностью observed ({fully_observed}), но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent."
                    )
                } else {
                    "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string()
                }
            } else if alignment["baseline_equivalence"]["state"].as_str()
                == Some("baseline_component_semantics_explicit_boundary")
            {
                let measured = human_client_limit_components(
                    &alignment["baseline_equivalence"]["measured_baseline_components"],
                );
                let boundary = human_client_limit_components(
                    &alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
                );
                match (measured, boundary) {
                    (Some(measured), Some(boundary)) => format!(
                        "Здесь whole-cycle компоненты уже fully observed; baseline-equivalent semantics уже materialized для {measured}, а для {boundary} gap оставлен как explicit truth-boundary без guessed baseline, поэтому метрика остаётся честно non-equivalent."
                    ),
                    _ => "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string(),
                }
            } else if alignment["baseline_equivalence"]["state"].as_str()
                == Some("baseline_component_semantics_partial")
            {
                let measured = human_client_limit_components(
                    &alignment["baseline_equivalence"]["measured_baseline_components"],
                );
                let missing = human_client_limit_components(
                    &alignment["baseline_equivalence"]["missing_baseline_components"],
                );
                match (measured, missing) {
                    (Some(measured), Some(missing)) => format!(
                        "Здесь whole-cycle компоненты уже fully observed; baseline-equivalent semantics уже materialized для {measured}, но ещё не materialized для {missing}, поэтому метрика остаётся честно non-equivalent."
                    ),
                    _ => "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string(),
                }
            } else {
                "Здесь whole-cycle observed компоненты уже видны по live событиям, но baseline всё ещё не эквивалентен полному клиентскому лимиту, поэтому метрика остаётся честно non-equivalent.".to_string()
            }
        }
        other => format!(
            "Этот срез пока не эквивалентен клиентскому лимиту сессии: state={other}."
        ),
    })
}

fn client_limit_alignment_tooltip(alignment: &Value) -> Option<String> {
    let state = alignment["alignment_state"].as_str()?;
    if alignment["same_meter_as_client_limit"].as_bool() == Some(true) {
        return Some(
            "Эта строка показывает, обязана ли карточка двигаться в том же метре, которым клиент считает внешний лимит сессии. Сейчас ответ: да.\n- same-meter alignment уже materialized.\n- Exact model-token pair можно читать как тот же meter, которым клиент считает лимит.\n- Remaining explicit boundary для этого scope нет."
                .to_string(),
        );
    }
    let mut reasons = alignment["blocking_reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reason| reason.as_str())
        .filter_map(human_client_limit_alignment_reason)
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        reasons
            .push("текущий savings-layer всё ещё не совпадает с полным метром клиентского лимита");
    }
    let state_note = match state {
        "no_usage_observed" => "В этом scope ещё нет usage-событий.",
        "only_non_live_scope_activity" => {
            "В этом scope пока есть только non-live события, поэтому карточка не обязана совпадать с внешней шкалой лимита."
        }
        "live_usage_unconfirmed_not_meter_equivalent" => {
            "Live usage уже был, но подтверждённый lower bound ещё не накопился."
        }
        "partial_lower_bound_not_meter_equivalent" => {
            "Даже подтверждённая цифра здесь пока описывает только lower bound части агентного цикла."
        }
        "whole_cycle_partially_observed_not_meter_equivalent" => {
            "Whole-cycle observed компоненты уже начали materialize-иться, но покрытие по live событиям ещё неполное."
        }
        "whole_cycle_observed_explicit_boundary_not_meter_equivalent" => {
            "Whole-cycle observed компоненты уже fully covered, strict same-meter lower bound уже materialized, но remaining gap оставлен как explicit continuity truth-boundary."
        }
        "whole_cycle_observed_baseline_partial" => {
            "Whole-cycle observed компоненты уже видны по live событиям, но baseline-equivalent semantics для клиентского лимита ещё не materialized."
        }
        _ => "Этот scope пока не эквивалентен лимиту клиента.",
    };
    let mut tooltip = String::from(
        "Эта строка показывает, обязана ли карточка двигаться в том же метре, которым клиент считает внешний лимит сессии. Сейчас ответ: нет.",
    );
    tooltip.push('\n');
    tooltip.push_str("- ");
    tooltip.push_str(state_note);
    if alignment["strict_client_meter_slice"]["same_meter_equivalent_for_slice"].as_bool()
        == Some(true)
    {
        let lower_bound = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
            .as_u64()
            .unwrap_or(0);
        let components =
            human_client_limit_components(&alignment["strict_client_meter_slice"]["components"]);
        if lower_bound > 0 {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("strict same-meter lower bound уже materialized: ");
            tooltip.push_str(&lower_bound.to_string());
            tooltip.push_str(" токенов");
            if let Some(components) = components {
                tooltip.push_str(" по компонентам ");
                tooltip.push_str(&components);
            }
        }
    }
    if alignment["baseline_equivalence"]["state"].as_str()
        == Some("baseline_semantics_unmaterialized")
    {
        if let Some(fully_observed) = human_client_limit_components(
            &alignment["baseline_equivalence"]["fully_observed_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("applicable whole-cycle компоненты уже fully observed: ");
            tooltip.push_str(&fully_observed);
        }
    } else if alignment["baseline_equivalence"]["state"].as_str()
        == Some("baseline_component_semantics_explicit_boundary")
    {
        if let Some(measured) = human_client_limit_components(
            &alignment["baseline_equivalence"]["measured_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("baseline-equivalent semantics уже materialized для: ");
            tooltip.push_str(&measured);
        }
        if let Some(boundary) = human_client_limit_components(
            &alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("explicit truth-boundary без guessed baseline оставлен для: ");
            tooltip.push_str(&boundary);
        }
    } else if alignment["baseline_equivalence"]["state"].as_str()
        == Some("baseline_component_semantics_partial")
    {
        if let Some(measured) = human_client_limit_components(
            &alignment["baseline_equivalence"]["measured_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("baseline-equivalent semantics уже materialized для: ");
            tooltip.push_str(&measured);
        }
        if let Some(missing) = human_client_limit_components(
            &alignment["baseline_equivalence"]["missing_baseline_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("baseline-equivalent semantics ещё missing для: ");
            tooltip.push_str(&missing);
        }
    } else if alignment["baseline_equivalence"]["state"].as_str()
        == Some("whole_cycle_components_incomplete")
    {
        if let Some(incomplete) = human_client_limit_components(
            &alignment["baseline_equivalence"]["incomplete_components"],
        ) {
            tooltip.push('\n');
            tooltip.push_str("- ");
            tooltip.push_str("whole-cycle coverage ещё incomplete по: ");
            tooltip.push_str(&incomplete);
        }
    }
    for reason in reasons {
        tooltip.push('\n');
        tooltip.push_str("- ");
        tooltip.push_str(reason);
    }
    Some(tooltip)
}

fn human_client_limit_alignment_reason(reason: &str) -> Option<&'static str> {
    match reason {
        "client_prompt_unmeasured" => {
            Some("в этот слой пока не входят токены исходного запроса клиента")
        }
        "assistant_generation_unmeasured" => {
            Some("в этот слой пока не входят токены генерации ответа моделью")
        }
        "tool_overhead_outside_retrieval_unmeasured" => {
            Some("в этот слой пока не входит tool/orchestration overhead вне retrieval")
        }
        "continuity_restore_outside_retrieval_unmeasured" => {
            Some("в этот слой пока не входит continuity-restore overhead вне retrieval")
        }
        "client_prompt_partially_measured" => {
            Some("токены исходного запроса клиента уже видны только на части live-событий")
        }
        "assistant_generation_partially_measured" => {
            Some("токены генерации ответа уже видны только на части live-событий")
        }
        "tool_overhead_outside_retrieval_partially_measured" => {
            Some("tool/orchestration overhead вне retrieval уже виден только на части live-событий")
        }
        "continuity_restore_outside_retrieval_partially_measured" => {
            Some("continuity-restore overhead вне retrieval уже виден только на части live-событий")
        }
        "same_meter_baseline_unmeasured" => Some(
            "whole-cycle observed слой уже виден, но baseline ещё не эквивалентен клиентскому spend meter",
        ),
        "same_meter_baseline_explicit_boundary" => Some(
            "часть same-meter baseline contour оставлена как явная truth-boundary без guessed pre-Amai baseline",
        ),
        "same_meter_baseline_partially_measured" => Some(
            "часть applicable whole-cycle компонентов уже имеет baseline-equivalent semantics, но не весь contour ещё materialized",
        ),
        "no_usage_observed_in_scope" => Some("в этом scope ещё не было usage-событий"),
        "no_live_usage_in_scope" => Some("в этом scope пока нет live usage"),
        "non_live_events_present_in_scope" => Some(
            "в этом scope уже есть non-live события, которые не совпадают с клиентским spend meter",
        ),
        "no_confirmed_live_usage_in_scope" => {
            Some("live usage уже был, но ещё не дошёл до confirmed lane")
        }
        _ => None,
    }
}

fn token_lane_summary(
    baseline_tokens: Option<u64>,
    delivered_tokens: Option<u64>,
    recovery_tokens: Option<u64>,
    delta_tokens: Option<i64>,
) -> String {
    match (baseline_tokens, delivered_tokens, recovery_tokens) {
        (Some(baseline), Some(delivered), Some(recovery)) => format!(
            "без Amai {}, от Amai {}, уточнения {}, итог {}",
            format_u64(Some(baseline)),
            format_u64(Some(delivered)),
            format_u64(Some(recovery)),
            format_signed_count(delta_tokens)
        ),
        _ => "ещё нет данных".to_string(),
    }
}

fn artifact_cleanup_pressure_state(
    cleanup: &Value,
    machine: Option<&MachineSummary>,
) -> Option<&'static str> {
    if cleanup["policy_retained_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0)
        == 0
    {
        return None;
    }
    let Some(machine) = machine else {
        return Some("waiting");
    };
    let thresholds = &cleanup["disk_pressure_thresholds"];
    let used_percent = machine.disk_used_percent.unwrap_or(0.0);
    let available_gib = machine.disk_available_gib;
    let alert_used_percent = thresholds["alert_used_percent"].as_f64().unwrap_or(85.0);
    let critical_used_percent = thresholds["critical_used_percent"].as_f64().unwrap_or(92.0);
    let alert_available_gib = thresholds["alert_available_gib"].as_f64().unwrap_or(150.0);
    let critical_available_gib = thresholds["critical_available_gib"]
        .as_f64()
        .unwrap_or(60.0);

    if used_percent >= critical_used_percent || available_gib <= critical_available_gib {
        Some("critical")
    } else if used_percent >= alert_used_percent || available_gib <= alert_available_gib {
        Some("alert")
    } else {
        Some("waiting")
    }
}

fn artifact_cleanup_operator_reclaim_hints(cleanup: &Value) -> Vec<Value> {
    if let Some(hints) = cleanup["operator_reclaim_hints"].as_array() {
        if !hints.is_empty() {
            return hints.clone();
        }
    }

    let mut hints = Vec::new();
    if let Some(targets) = cleanup["manual_only_reclaimable_targets"].as_array() {
        for target in targets {
            if let Some(hint) =
                artifact_cleanup_operator_reclaim_hint_from_target(target, "manual_only_cleanup")
            {
                hints.push(hint);
            }
        }
    }
    if let Some(targets) = cleanup["policy_retained_targets"].as_array() {
        for target in targets {
            if let Some(hint) = artifact_cleanup_operator_reclaim_hint_from_target(
                target,
                "policy_retained_hot_storage",
            ) {
                hints.push(hint);
            }
        }
    }
    hints.sort_by_key(|hint| Reverse(hint["reclaimable_bytes"].as_u64().unwrap_or(0)));
    hints.truncate(3);
    hints
}

fn artifact_cleanup_operator_reclaim_hint_from_target(
    target: &Value,
    reason: &str,
) -> Option<Value> {
    let path = target["path"].as_str()?;
    let selected_reclaimable_bytes = target["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let aggressive_preview_reclaimable_bytes = target["aggressive_preview_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    let use_aggressive = selected_reclaimable_bytes == 0;
    let reclaimable_bytes = if use_aggressive {
        aggressive_preview_reclaimable_bytes
    } else {
        selected_reclaimable_bytes
    };
    Some(json!({
        "path": path,
        "reason": reason,
        "reclaimable_bytes": reclaimable_bytes,
        "recommended_command": if use_aggressive {
            format!("observe cleanup-artifacts --target {path} --aggressive --apply")
        } else {
            format!("observe cleanup-artifacts --target {path} --apply")
        }
    }))
}

fn artifact_cleanup_reclaim_hint_summary(hint: &Value) -> String {
    let path = hint["path"].as_str().unwrap_or("неизвестный target");
    let reclaimable_bytes = hint["reclaimable_bytes"].as_u64().unwrap_or(0);
    let command = hint["recommended_command"]
        .as_str()
        .unwrap_or("observe cleanup-artifacts --help");
    format!(
        "{path} -> {command} ({})",
        human_bytes(reclaimable_bytes as f64)
    )
}

fn artifact_cleanup_status(snapshot: &Value, machine: Option<&MachineSummary>) -> &'static str {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return "unknown";
    }
    if cleanup["selected"].as_u64().unwrap_or(0) > 0 {
        "alert"
    } else if cleanup["repo_inventory"]["unmanaged_alert_triggered"].as_bool() == Some(true) {
        "alert"
    } else if cleanup["manual_only_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "alert"
    } else if let Some(status) = artifact_cleanup_pressure_state(cleanup, machine) {
        status
    } else if cleanup["aggressive_preview_selected"].as_u64().unwrap_or(0) > 0 {
        "alert"
    } else {
        "pass"
    }
}

fn artifact_cleanup_warning(snapshot: &Value, machine: Option<&MachineSummary>) -> Option<String> {
    let cleanup = &snapshot["artifact_cleanup"];
    if !cleanup.is_object() || cleanup["status"].as_str().is_some() {
        return None;
    }
    let safe_bytes = cleanup["selected_reclaimable_bytes"].as_u64().unwrap_or(0);
    let aggressive_bytes = cleanup["aggressive_preview_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    if safe_bytes > 0 {
        return Some(format!(
            "Локальный rebuildable хвост уже aged past TTL: safe reclaim сейчас {}. Это не live state и его можно убрать policy-cleanup path-ом.",
            human_bytes(safe_bytes as f64)
        ));
    }
    let repo_inventory = &cleanup["repo_inventory"];
    if repo_inventory["unmanaged_alert_triggered"].as_bool() == Some(true) {
        let out_of_policy_bytes = repo_inventory["out_of_policy_bytes"].as_u64().unwrap_or(0);
        let first_root = repo_inventory["large_unmanaged_roots"]
            .as_array()
            .and_then(|roots| roots.first())
            .cloned()
            .unwrap_or_default();
        let root_path = first_root["path"].as_str().unwrap_or("неизвестный root");
        let root_unmanaged_bytes = first_root["unmanaged_bytes"].as_u64().unwrap_or(0);
        let manual_only_target = repo_inventory["manual_only_targets"]
            .as_array()
            .and_then(|targets| targets.first())
            .cloned()
            .unwrap_or_default();
        let manual_only_path = manual_only_target["path"].as_str();
        let manual_hint = manual_only_path.map(|path| {
            format!(
                " Для {path} уже есть explicit manual cleanup contour: `observe cleanup-artifacts --target {path} --apply` или `--target {path} --aggressive --apply`."
            )
        }).unwrap_or_default();
        return Some(format!(
            "Основной локальный вес сейчас вне cleanup policy: всего {} вне managed targets, крупнейший root {} = {}. Auto-retention это не трогает, пока путь не включён в policy отдельным contour-ом.{}",
            human_bytes(out_of_policy_bytes as f64),
            root_path,
            human_bytes(root_unmanaged_bytes as f64),
            manual_hint
        ));
    }
    let manual_only_bytes = cleanup["manual_only_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    if manual_only_bytes > 0 {
        let operator_hint = artifact_cleanup_operator_reclaim_hints(cleanup)
            .into_iter()
            .next()
            .unwrap_or_default();
        let command = operator_hint["recommended_command"]
            .as_str()
            .unwrap_or("observe cleanup-artifacts --apply");
        return Some(format!(
            "Сейчас уже есть {} reclaimable веса на manual-only cleanup contour. Auto-retention этот путь специально не трогает, поэтому нужен explicit operator run: `{command}`.",
            human_bytes(manual_only_bytes as f64),
        ));
    }
    let policy_retained_bytes = cleanup["policy_retained_reclaimable_bytes"]
        .as_u64()
        .unwrap_or(0);
    if policy_retained_bytes > 0 {
        let pressure_state = artifact_cleanup_pressure_state(cleanup, machine).unwrap_or("waiting");
        let first_hint = artifact_cleanup_operator_reclaim_hints(cleanup)
            .into_iter()
            .next()
            .unwrap_or_default();
        let target_path = first_hint["path"].as_str().unwrap_or("policy target");
        let target_bytes = first_hint["reclaimable_bytes"].as_u64().unwrap_or(0);
        let command = first_hint["recommended_command"]
            .as_str()
            .unwrap_or("observe cleanup-artifacts --aggressive --apply");
        return Some(match pressure_state {
            "critical" | "alert" => {
                let used = machine
                    .and_then(|summary| summary.disk_used_percent)
                    .map(|value| format!("{value:.1}%"))
                    .unwrap_or_else(|| "неизвестно".to_string());
                let available = machine
                    .map(|summary| format!("{:.2} GiB", summary.disk_available_gib))
                    .unwrap_or_else(|| "неизвестно".to_string());
                format!(
                    "На диске уже есть давление: used {used}, свободно {available}. При этом {} policy-covered hot storage всё ещё удерживается TTL/keep-latest. Следующий manual reclaim кандидат: {target_path} = {} через `{command}`.",
                    human_bytes(policy_retained_bytes as f64),
                    human_bytes(target_bytes as f64)
                )
            }
            _ => format!(
                "Сейчас {} rebuildable веса уже policy-covered, но intentionally удерживается TTL/keep-latest. Cleanup не сломан: это hot storage, которое auto-path уберёт позже. Если место нужно раньше, ближайший reclaim path уже готов: `{command}` для {target_path} = {}.",
                human_bytes(policy_retained_bytes as f64),
                human_bytes(target_bytes as f64)
            ),
        });
    }
    if aggressive_bytes > 0 {
        return Some(format!(
            "Локальный rebuildable хвост ещё не дожил до TTL, но aggressive reclaim path уже мог бы вернуть {} без удаления live state. Safe policy сейчас специально ждёт возрастной запас.",
            human_bytes(aggressive_bytes as f64)
        ));
    }
    None
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
    fn model_token_savings_row_surfaces_exact_meter_equivalent_percent() {
        let statement_preview = json!({
            "verified_without_amai_measured_tokens": 320,
            "verified_with_amai_measured_tokens": 240,
            "verified_observed_whole_cycle_with_amai_tokens": 240
        });
        let alignment = json!({
            "same_meter_as_client_limit": true,
            "continuity_boundary_rollup": {
                "observed_tokens": 0
            }
        });

        let row = super::model_token_savings_metric_row(&statement_preview, &alignment);
        assert_eq!(row["label"].as_str(), Some("Экономия токенов модели"));
        assert_eq!(
            row["value"].as_str(),
            Some("Учтённый same-meter срез: без Amai 320, с Amai 240, экономия 80")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сам процент этого slice intentionally не показывается")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("идёт по тому же meter, которым клиент считает лимит")
        );

        let note =
            super::model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("учтённого среза"));
        assert!(!note.contains("25.00%"));
    }

    #[test]
    fn model_token_savings_row_prefers_strict_same_meter_lower_bound() {
        let statement_preview = json!({
            "verified_without_amai_measured_tokens": 605,
            "verified_with_amai_measured_tokens": 0,
            "observed_whole_cycle_with_amai_tokens": 605,
            "verified_observed_whole_cycle_with_amai_tokens": 589
        });
        let alignment = json!({
            "same_meter_as_client_limit": true,
            "strict_client_meter_slice": {
                "lower_bound_tokens": 609
            },
            "baseline_equivalence": {
                "measured_baseline_tokens_lower_bound": 609
            }
        });

        let row = super::model_token_savings_metric_row(&statement_preview, &alignment);
        assert_eq!(
            row["value"].as_str(),
            Some("Учтённый same-meter срез: без Amai 609, с Amai 605, экономия 4")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сам процент этого slice intentionally не показывается")
        );
    }

    #[test]
    fn model_token_savings_row_marks_preliminary_same_meter_slice_and_components() {
        let statement_preview = json!({
            "preliminary": true,
            "counted_events": 1,
            "events_total": 1,
            "observed_whole_cycle_with_amai_tokens": 99,
            "verified_observed_whole_cycle_with_amai_tokens": 99
        });
        let alignment = json!({
            "same_meter_as_client_limit": true,
            "strict_client_meter_slice": {
                "lower_bound_tokens": 640,
                "components": [
                    "client_prompt",
                    "continuity_restore_outside_retrieval"
                ]
            },
            "baseline_equivalence": {
                "measured_baseline_tokens_lower_bound": 640
            }
        });

        let row = super::model_token_savings_metric_row(&statement_preview, &alignment);
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("Предварительный учтённый same-meter срез")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("исходный запрос клиента")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("экономия 541")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("continuity-restore overhead вне retrieval")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Сам процент этого slice intentionally не показывается")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("preliminary, учтено 1 из 1 событий")
        );

        let note =
            super::model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("strict same-meter срез"));
        assert!(note.contains("нельзя читать как экономию всей сессии целиком"));
        assert!(!note.contains("84.53%"));
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
    fn model_token_savings_row_hides_percent_until_exact_pair_materializes() {
        let statement_preview = json!({
            "verified_baseline_tokens": 320,
            "verified_delivered_tokens": 248,
            "verified_recovery_tokens": 8,
            "verified_effective_saved_tokens": 64,
            "verified_effective_savings_pct": 20.0,
            "verified_observed_whole_cycle_with_amai_tokens": 609
        });
        let alignment = json!({
            "same_meter_as_client_limit": false,
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary"
            },
            "continuity_boundary_rollup": {
                "observed_tokens": 609
            }
        });

        let row = super::model_token_savings_metric_row(&statement_preview, &alignment);
        assert_eq!(
            row["value"].as_str(),
            Some("Точного процента пока нет; с Amai уже видно 609")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("exact pair для этого scope ещё не materialized")
        );

        let note =
            super::model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("Точный процент"));
        assert!(note.contains("реальной шкалой лимита модели"));
    }

    #[test]
    fn model_token_note_surfaces_primary_exact_pair_blocker() {
        let statement_preview = json!({
            "verified_observed_whole_cycle_with_amai_tokens": 3524046
        });
        let alignment = json!({
            "same_meter_as_client_limit": false,
            "exact_pair_status": {
                "blockers": [{
                    "code": "tool_overhead_outside_retrieval",
                    "missing_live_events": 36,
                    "irrecoverable_missing_live_events": 13
                }]
            }
        });

        let note =
            super::model_token_savings_note_sentence(&statement_preview, &alignment).expect("note");
        assert!(note.contains("missing 36 live events"));
        assert!(note.contains("13 irrecoverable"));
        assert!(note.contains("23 ещё recoverable"));
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
    fn build_active_agent_budget_session_card_shows_only_active_agent_kpi_and_limit() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "saving",
                    "reply_prefix": "5ч KPI: экономия 40.00%"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "thread_title": "compact dashboard rewrite",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 60.00%",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 43.00%, 7д остаётся 23.00%",
                            "tooltip": "personal limit one"
                        },
                        "client_live_meter": {
                            "status": "observed",
                            "current_thread_bound": true,
                            "thread_binding_state": "current_thread_bound",
                            "ended_at_epoch_ms": 2000,
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
                        }
                    },
                    {
                        "agent_label": "Hunter",
                        "agent_scope": "bug_bounty::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 20.00%",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "label_text": "Личный thread-limit агента:",
                            "value_text": "5ч остаётся 88.00%, 7д остаётся 91.00%",
                            "tooltip": "personal limit two"
                        },
                        "client_live_meter": {
                            "status": "observed",
                            "current_thread_bound": true,
                            "thread_binding_state": "current_thread_bound",
                            "ended_at_epoch_ms": 2000,
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
                        }
                    }
                ]
            }
        });
        let card = super::build_active_agent_budget_session_card(&snapshot).expect("card");
        assert_eq!(card["value"].as_str(), Some("5ч KPI: экономия 40.00%"));
        assert_eq!(
            card["presentation_variant"].as_str(),
            Some("active_agent_budget_grouped_v3")
        );
        assert_eq!(card["status_label"].as_str(), Some(""));
        assert!(card["status_tooltip"].is_null());
        let rows = card["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["label"].as_str(), Some("Агент:"));
        assert_eq!(rows[1]["label"].as_str(), Some("Лимит клиента сейчас:"));
        assert_eq!(rows[2]["label"].as_str(), Some("KPI:"));
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["agent_label"].as_str(), Some("Amai"));
        assert!(
            blocks[0]["agent_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("amai::continuity::default"))
        );
        assert!(
            blocks[0]["limit_value"]
                .as_str()
                .is_some_and(|value| value.contains("5ч остаётся 43.00%"))
        );
        assert!(
            blocks[0]["limit_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("personal limit one"))
        );
        assert_eq!(
            blocks[0]["limit_label"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert_eq!(
            blocks[0]["kpi_value"].as_str(),
            Some("5ч KPI: экономия 60.00%")
        );
        assert!(
            blocks[0]["kpi_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("agent one"))
        );
        assert_eq!(blocks[1]["agent_label"].as_str(), Some("Hunter"));
        assert!(
            blocks[1]["limit_value"]
                .as_str()
                .is_some_and(|value| value.contains("5ч остаётся 88.00%"))
        );
        assert_eq!(
            blocks[1]["limit_label"].as_str(),
            Some("Личный thread-limit агента:")
        );
        assert_eq!(
            blocks[1]["kpi_value"].as_str(),
            Some("5ч KPI: экономия 20.00%")
        );
    }

    #[test]
    fn build_active_agent_budget_session_card_collapses_shared_global_limit() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "overspend",
                    "reply_prefix": "5ч KPI: переплата 33.00%"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: переплата 44.00%",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 12.00%, 7д остаётся 89.00%",
                            "tooltip": "same exact global limit"
                        }
                    },
                    {
                        "agent_label": "Bug Bounty",
                        "agent_scope": "bug_bounty::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: переплата 22.00%",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 12.00%, 7д остаётся 89.00%",
                            "tooltip": "same exact global limit"
                        }
                    }
                ]
            }
        });

        let card = super::build_active_agent_budget_session_card(&snapshot).expect("card");
        assert_eq!(
            card["shared_limit_label"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert_eq!(
            card["shared_limit_value"].as_str(),
            Some("5ч остаётся 12.00%, 7д остаётся 89.00%")
        );
        let rows = card["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0]["label"].as_str(), Some("Лимит клиента сейчас:"));
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].get("limit_label").is_none());
        assert!(blocks[0].get("limit_value").is_none());
        assert!(blocks[1].get("limit_label").is_none());
        assert!(blocks[1].get("limit_value").is_none());
    }

    #[test]
    fn build_active_agent_budget_session_card_surfaces_live_turn_pressure_per_agent() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "overspend",
                    "reply_prefix": "5ч KPI: переплата 120.00%"
                },
                "agents": [
                    {
                        "agent_label": "Bug Bounty",
                        "agent_scope": "bug_bounty::continuity::default",
                        "thread_title": "Авито дальше",
                        "cwd": "/home/art/Bug-Bounty",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: переплата 197.79%",
                            "summary": "Личный 5ч KPI текущего active thread идёт в переплате 197.79%."
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 16.00%, 7д остаётся 90.00%",
                            "tooltip": "same exact global limit"
                        },
                        "client_live_meter": {
                            "status": "observed",
                            "current_thread_bound": true,
                            "thread_binding_state": "current_thread_bound",
                            "ended_at_epoch_ms": 1775155316431u64,
                            "client_turn_total_tokens": 222596,
                            "latest_model_context_window": 258400,
                            "context_used_percent": 86.14,
                            "status_bar_rate_limits": {
                                "status": "observed",
                                "observed_at_epoch_ms": 1775155316431u64
                            }
                        }
                    }
                ]
            }
        });

        let card = super::build_active_agent_budget_session_card(&snapshot).expect("card");
        let rows = card["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3]["label"].as_str(), Some("Последний запрос:"));
        assert_eq!(
            rows[3]["value"].as_str(),
            Some("222596 из 258400 · окно занято 86.14%")
        );
        assert!(
            rows[3]["tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("giant-thread pressure"))
        );
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(
            blocks[0]["pressure_value"].as_str(),
            Some("222596 из 258400 · окно занято 86.14%")
        );
        assert!(
            blocks[0]["pressure_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("Окно занято: 86.14%"))
        );
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
        assert_eq!(
            payload["top_cards"].as_array().map(|cards| cards.len()),
            Some(3)
        );
    }

    #[test]
    fn compact_token_hero_card_leaves_active_agent_budget_minimal_card_unchanged() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "presentation_variant": "active_agent_budget_grouped_v3",
            "status": "pass",
            "status_label": "",
            "rows": [],
            "agent_blocks": [
                {
                    "agent_label": "Amai",
                    "limit_value": "5ч остаётся 43.00%",
                    "kpi_value": "5ч KPI: экономия 60.00%"
                }
            ]
        });

        let compact = super::compact_token_hero_card(card.clone());
        assert_eq!(compact, card);
    }

    #[test]
    fn compact_token_hero_card_keeps_truth_only_rows_for_current_session() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "status": "critical",
            "status_label": "новый чат нужен сейчас",
            "note": "long note",
            "rows": [
                {"label": "Главный итог", "value": "x"},
                {"label": "Amai в полном live-turn", "value": "0.30%: без Amai 1000, с Amai 997, delta 3"},
                {"label": "Экономия токенов модели", "value": "y"},
                {"label": "Главный драйвер exact-пары", "value": "continuity-restore overhead вне retrieval: 636 -> 95 (экономия 541)"},
                {"label": "Совпадение с реальным лимитом", "value": "z"},
                {"label": "Лимит клиента сейчас", "value": "l"},
                {"label": "Следующее действие", "value": "n"},
                {"label": "Строгий same-meter срез", "value": "drop"}
            ]
        });
        let compact = super::compact_token_hero_card(card);
        let labels = compact["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "Экономия на реальной шкале",
                "Экономия на учтённой части",
                "Что именно посчитано",
                "Точность учтённой части",
                "Лимит клиента сейчас",
                "Следующее действие"
            ]
        );
        assert_eq!(
            compact["source_label"].as_str(),
            Some(
                "Источник: живая шкала клиента из rollout token_count и отдельно сведённая учтённая часть Amai по strict same-meter компонентам."
            )
        );
        assert!(
            compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая карточка только с проверяемыми цифрами по текущей сессии")
        );
        assert!(
            compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("strict same-meter срезу")
        );
    }

    #[test]
    fn compact_token_hero_card_keeps_truth_only_rows_for_lifetime() {
        let card = json!({
            "title": "Экономия токенов за всё время записи",
            "status": "alert",
            "status_label": "есть старый долг точности",
            "note": "long note",
            "rows": [
                {"label": "Экономия токенов модели", "value": "a"},
                {"label": "Совпадение с реальным лимитом", "value": "b"},
                {"label": "Review-only export", "value": "c"},
                {"label": "Связь с лимитом клиента", "value": "drop"},
                {"label": "Исторический frozen debt", "value": "d"}
            ]
        });
        let compact = super::compact_token_hero_card(card);
        let labels = compact["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "Экономия на учтённой части",
                "Точность учтённой части",
                "Отчёт для ручной сверки",
                "Исторический долг точности"
            ]
        );
        assert_eq!(
            compact["source_label"].as_str(),
            Some(
                "Источник: подтверждённая учтённая история плюс отдельно отмеченный старый долг точности. Это не полный raw spend всей истории."
            )
        );
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
    fn client_limit_alignment_tooltip_surfaces_explicit_baseline_boundary_components() {
        let alignment = json!({
            "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
            "same_meter_as_client_limit": false,
            "live_events_count": 79,
            "non_live_events_count": 0,
            "strict_client_meter_slice": {
                "same_meter_equivalent_for_slice": true,
                "lower_bound_tokens": 316,
                "components": ["client_prompt"]
            },
            "blocking_reasons": [
                "same_meter_baseline_explicit_boundary"
            ],
            "baseline_equivalence": {
                "state": "baseline_component_semantics_explicit_boundary",
                "measured_baseline_components": [
                    "client_prompt",
                ],
                "explicitly_unmodeled_baseline_components": [
                    "continuity_restore_outside_retrieval"
                ],
                "remaining_gap_reason": "same_meter_baseline_explicit_boundary"
            }
        });

        let tooltip = super::client_limit_alignment_tooltip(&alignment)
            .expect("baseline equivalence tooltip");
        assert!(tooltip.contains("исходный запрос клиента"));
        assert!(tooltip.contains("continuity-restore overhead вне retrieval"));
        assert!(tooltip.contains("explicit truth-boundary"));
        assert!(tooltip.contains("strict same-meter lower bound уже materialized"));
        assert!(tooltip.contains("316 токенов"));

        let note = super::client_limit_alignment_note_sentence(&alignment)
            .expect("baseline equivalence note");
        assert!(note.contains("explicit truth-boundary"));
        assert!(note.contains("не просто partial baseline"));
    }

    #[test]
    fn client_limit_extra_rows_surface_strict_slice_and_continuity_boundary() {
        let alignment = json!({
            "strict_client_meter_slice": {
                "same_meter_equivalent_for_slice": true,
                "lower_bound_tokens": 320,
                "components": ["client_prompt"]
            },
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary",
                "blocks_full_same_meter_equivalence": true,
                "components": ["continuity_restore_outside_retrieval"],
                "note": "Continuity boundary."
            }
        });

        let strict_row =
            super::client_limit_strict_slice_metric_row(&alignment).expect("strict row");
        assert_eq!(
            strict_row["label"].as_str(),
            Some("Строгий same-meter срез")
        );
        assert!(
            strict_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("320")
        );
        assert!(
            strict_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("исходный запрос клиента")
        );

        let boundary_row =
            super::client_limit_explicit_boundary_metric_row(&alignment).expect("boundary row");
        assert_eq!(boundary_row["label"].as_str(), Some("Граница continuity"));
        assert!(
            boundary_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("continuity-restore overhead вне retrieval")
        );

        let boundary_tokens_row = super::client_limit_boundary_tokens_metric_row(&json!({
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary",
                "components": ["continuity_restore_outside_retrieval"]
            },
            "baseline_equivalence": {
                "component_semantics": [
                    {
                        "code": "continuity_restore_outside_retrieval",
                        "whole_cycle_observed_complete": true,
                        "observed_tokens": 50329
                    }
                ]
            }
        }))
        .expect("boundary tokens row");
        assert_eq!(
            boundary_tokens_row["label"].as_str(),
            Some("Токены continuity boundary")
        );
        assert!(
            boundary_tokens_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("50329")
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
