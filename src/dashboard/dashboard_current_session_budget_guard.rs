use super::*;

pub fn current_session_budget_guard(snapshot: &Value) -> Value {
    current_session_budget_guard_with_restore_context(
        snapshot,
        &snapshot["token_budget_report"]["token_budget_report"],
        &snapshot["latest_repo_working_state_restore"]["working_state_restore"],
    )
}

pub(super) fn current_agent_reply_prefix_fields<'a>(
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

pub(super) fn build_client_budget_reply_execution_gate_with_primary_command(
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
