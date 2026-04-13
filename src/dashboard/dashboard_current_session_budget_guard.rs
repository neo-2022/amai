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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
