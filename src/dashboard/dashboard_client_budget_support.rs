use super::dashboard_current_session_budget_guard::current_agent_reply_prefix_fields;
use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ClientTurnPressureGuard {
    pub(super) severity: &'static str,
    pub(super) status_label: &'static str,
    pub(super) client_budget_target_percent: u64,
    pub(super) turn_total_tokens: u64,
    pub(super) model_context_window: u64,
    pub(super) context_used_percent: f64,
    pub(super) primary_remaining_percent: f64,
    pub(super) secondary_remaining_percent: f64,
    pub(super) full_turn_savings_pct: Option<f64>,
    pub(super) hourly_burn_classification: Option<&'static str>,
    pub(super) hourly_burn_kpi_percent: Option<f64>,
    pub(super) no_amai_activity_in_current_live_turn: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct GlobalClientLimitGuard {
    pub(super) severity: &'static str,
    pub(super) status_label: &'static str,
    pub(super) primary_remaining_percent: f64,
    pub(super) secondary_remaining_percent: f64,
}

const HOST_CONTEXT_COMPACTION_PRESERVE_TRIGGER_TOKENS: u64 = 10_000;
const HOST_CONTEXT_COMPACTION_CRITICAL_TRIGGER_TOKENS: u64 = 50_000;
const HOST_CONTEXT_COMPACTION_CRITICAL_REBOUND_RATIO: f64 = 0.25;
const HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_TURN_TOKEN_DELTA: i64 = 50_000;
const HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_CONTEXT_PERCENT_POINTS: f64 = 10.0;
const HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_REGROWTH_TOKENS: i64 = 25_000;
const HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_PRIMARY_LIMIT_OVERRUN_PERCENT_POINTS: f64 = 5.0;
const HOST_CURRENT_THREAD_EFFECT_OVERLAY_TRIAL_TURN_TOKEN_DELTA: i64 = 5_000;
const HOST_CURRENT_THREAD_EFFECT_OVERLAY_TRIAL_CONTEXT_PERCENT_POINTS: f64 = 2.5;
const HOST_CURRENT_THREAD_EFFECT_OVERLAY_TRIAL_REGROWTH_TOKENS: i64 = 5_000;
const HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_ELAPSED_MS: u64 = 30_000;
const HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_TURN_TOKEN_DELTA: u64 = 3_000;
const HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_CONTEXT_PERCENT_POINTS: f64 = 1.0;
const HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_REGROWTH_TOKENS: u64 = 3_000;
const CLIENT_PRIMARY_LIMIT_WINDOW_MS: f64 = 5.0 * 60.0 * 60.0 * 1000.0;

pub(super) fn client_budget_target_percent_from_inputs(
    report: &Value,
    restore_context: &Value,
) -> u64 {
    restore_context["client_budget_target_percent"]
        .as_u64()
        .and_then(working_state::normalize_client_budget_target_percent)
        .or_else(|| {
            report["client_budget_target_percent"]
                .as_u64()
                .and_then(working_state::normalize_client_budget_target_percent)
        })
        .unwrap_or_else(working_state::default_client_budget_target_percent)
}

fn latest_host_current_thread_control_feedback_action_for_command<'a>(
    restore_context: &'a Value,
    command_id: Option<&str>,
) -> Option<&'a Value> {
    let command_id = command_id
        .map(|value| working_state::normalize_host_current_thread_control_command_id(Some(value)));
    restore_context["recent_actions"]
        .as_array()?
        .iter()
        .find(|action| {
            action["source_kind"].as_str()
                == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND)
                && action["host_current_thread_control_feedback"].is_object()
                && command_id.is_none_or(|expected| {
                    action["host_current_thread_control_feedback"]["command_id"]
                        .as_str()
                        .map(|value| {
                            working_state::normalize_host_current_thread_control_command_id(Some(
                                value,
                            ))
                        })
                        == Some(expected)
                })
        })
}

pub(super) fn latest_host_current_thread_control_feedback_kind_for_command<'a>(
    restore_context: &'a Value,
    command_id: Option<&str>,
) -> Option<&'a str> {
    latest_host_current_thread_control_feedback_action_for_command(restore_context, command_id)?[
        "host_current_thread_control_feedback"
    ]["feedback_kind"]
        .as_str()
}

pub(super) fn latest_host_current_thread_control_feedback_summary_for_command(
    restore_context: &Value,
    command_id: Option<&str>,
) -> Option<String> {
    let action = latest_host_current_thread_control_feedback_action_for_command(
        restore_context,
        command_id,
    )?;
    let summary = action["summary"].as_str()?.trim();
    if summary.is_empty() {
        return None;
    }
    let recorded_at = action["recorded_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp_clock);
    Some(match recorded_at {
        Some(stamp) => format!("{summary} ({stamp})"),
        None => summary.to_string(),
    })
}

fn latest_host_current_thread_control_feedback_recorded_at_for_command(
    restore_context: &Value,
    command_id: Option<&str>,
) -> Option<u64> {
    latest_host_current_thread_control_feedback_action_for_command(restore_context, command_id)?
        ["recorded_at_epoch_ms"]
        .as_u64()
}

fn latest_host_current_thread_control_effect_action_for_command<'a>(
    restore_context: &'a Value,
    command_id: Option<&str>,
) -> Option<&'a Value> {
    let command_id = command_id
        .map(|value| working_state::normalize_host_current_thread_control_command_id(Some(value)));
    restore_context["recent_actions"]
        .as_array()?
        .iter()
        .find(|action| {
            action["source_kind"].as_str()
                == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND)
                && matches!(
                    action["host_current_thread_control_feedback"]["feedback_kind"].as_str(),
                    Some(
                        working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED
                            | working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED
                    )
                )
                && action["host_current_thread_control_feedback"]["feedback_snapshot"].is_object()
                && command_id.is_none_or(|expected| {
                    action["host_current_thread_control_feedback"]["command_id"]
                        .as_str()
                        .map(|value| {
                            working_state::normalize_host_current_thread_control_command_id(Some(
                                value,
                            ))
                        })
                        == Some(expected)
                })
        })
}

fn host_current_thread_control_surface_label(command_id: &str) -> &'static str {
    if command_id.trim() == working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID {
        "compact window"
    } else {
        "thread overlay"
    }
}

fn per_minute_rate(value: f64, elapsed_ms: u64) -> Option<f64> {
    (elapsed_ms > 0).then_some(value * 60_000.0 / elapsed_ms as f64)
}

fn ideal_primary_limit_used_percent_point_delta(elapsed_ms: u64) -> f64 {
    if elapsed_ms == 0 {
        0.0
    } else {
        (elapsed_ms as f64 * 100.0 / CLIENT_PRIMARY_LIMIT_WINDOW_MS).clamp(0.0, 100.0)
    }
}

fn host_current_thread_control_effect_measurement_sufficient(
    same_thread: bool,
    elapsed_ms: Option<u64>,
    turn_token_delta: Option<i64>,
    context_used_percent_point_delta: Option<f64>,
    regrowth_since_feedback_tokens: Option<i64>,
) -> bool {
    if !same_thread {
        return false;
    }
    elapsed_ms.is_some_and(|value| value >= HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_ELAPSED_MS)
        || turn_token_delta.is_some_and(|value| {
            value.unsigned_abs() >= HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_TURN_TOKEN_DELTA
        })
        || context_used_percent_point_delta.is_some_and(|value| {
            value.abs() >= HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_CONTEXT_PERCENT_POINTS
        })
        || regrowth_since_feedback_tokens.is_some_and(|value| {
            value.unsigned_abs() >= HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_REGROWTH_TOKENS
        })
}

pub(super) fn host_current_thread_control_effect_payload_for_command(
    restore_context: &Value,
    client_live_meter: &Value,
    host_context_compaction: &Value,
    command_id: Option<&str>,
) -> Value {
    let Some(action) =
        latest_host_current_thread_control_effect_action_for_command(restore_context, command_id)
    else {
        return Value::Null;
    };
    let feedback = &action["host_current_thread_control_feedback"];
    let snapshot = &feedback["feedback_snapshot"];
    let feedback_kind = feedback["feedback_kind"]
        .as_str()
        .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED);
    let command_id = feedback["command_id"]
        .as_str()
        .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID);
    let surface_label = host_current_thread_control_surface_label(command_id);
    let compact_window_surface =
        command_id.trim() == working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID;
    let recorded_at_epoch_ms = action["recorded_at_epoch_ms"].as_u64();
    let recorded_at_label = recorded_at_epoch_ms.map(human_timestamp_clock);
    let current_observed_at_epoch_ms =
        preferred_client_limit_observed_at_epoch_ms(client_live_meter).or_else(|| {
            client_live_meter["ended_at_epoch_ms"]
                .as_i64()
                .and_then(|value| (value > 0).then_some(value as u64))
        });
    let elapsed_label =
        recorded_at_epoch_ms
            .zip(current_observed_at_epoch_ms)
            .map(|(recorded_at, observed_at)| {
                human_elapsed_ms(observed_at.saturating_sub(recorded_at))
            });
    let elapsed_ms = recorded_at_epoch_ms
        .zip(current_observed_at_epoch_ms)
        .map(|(recorded_at, observed_at)| observed_at.saturating_sub(recorded_at));
    let snapshot_thread_id = snapshot["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value: &&str| !value.is_empty());
    let current_thread_id = client_live_meter["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value: &&str| !value.is_empty());
    let same_thread = snapshot_thread_id.is_some() && snapshot_thread_id == current_thread_id;
    let turn_token_delta = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .zip(snapshot["client_live_meter"]["client_turn_total_tokens"].as_u64())
        .map(|(current, previous)| current as i64 - previous as i64);
    let context_used_percent_point_delta = client_live_meter["context_used_percent"]
        .as_f64()
        .zip(snapshot["client_live_meter"]["context_used_percent"].as_f64())
        .map(|(current, previous)| current - previous);
    let primary_limit_used_percent_point_delta = client_live_meter["primary_limit_used_percent"]
        .as_u64()
        .zip(snapshot["client_live_meter"]["primary_limit_used_percent"].as_u64())
        .map(|(current, previous)| current as i64 - previous as i64);
    let primary_limit_ideal_percent_point_delta =
        elapsed_ms.map(ideal_primary_limit_used_percent_point_delta);
    let primary_limit_used_overrun_percent_points = primary_limit_used_percent_point_delta
        .map(|value| value as f64)
        .zip(primary_limit_ideal_percent_point_delta)
        .map(|(actual, ideal)| actual - ideal);
    let current_compaction_count = host_context_compaction["compaction_count"].as_u64();
    let snapshot_compaction_count =
        snapshot["host_context_compaction"]["compaction_count"].as_u64();
    let current_compacted_at_epoch_ms = host_context_compaction["compacted_at_epoch_ms"].as_u64();
    let snapshot_compacted_at_epoch_ms =
        snapshot["host_context_compaction"]["compacted_at_epoch_ms"].as_u64();
    let compaction_count_delta = current_compaction_count
        .zip(snapshot_compaction_count)
        .map(|(current, previous)| current as i64 - previous as i64);
    let verified_host_compaction_observed_after_feedback = same_thread
        && (compaction_count_delta.is_some_and(|value| value > 0)
            || current_compacted_at_epoch_ms
                .zip(recorded_at_epoch_ms)
                .is_some_and(|(compacted_at, recorded_at)| compacted_at > recorded_at)
            || current_compacted_at_epoch_ms
                .zip(snapshot_compacted_at_epoch_ms)
                .is_some_and(|(current, previous)| current > previous));
    let regrowth_since_feedback_tokens = host_context_compaction["growth_since_compaction_tokens"]
        .as_u64()
        .zip(snapshot["host_context_compaction"]["growth_since_compaction_tokens"].as_u64())
        .map(|(current, previous)| current as i64 - previous as i64);
    let current_stage = host_context_compaction["stage"]
        .as_str()
        .filter(|value: &&str| !value.is_empty());
    let snapshot_stage = snapshot["host_context_compaction"]["stage"]
        .as_str()
        .filter(|value: &&str| !value.is_empty());
    let measurement_sufficient = host_current_thread_control_effect_measurement_sufficient(
        same_thread,
        elapsed_ms,
        turn_token_delta,
        context_used_percent_point_delta,
        regrowth_since_feedback_tokens,
    );
    let measurement_pending = same_thread && !measurement_sufficient;
    let summary = if same_thread {
        let feedback_label =
            if feedback_kind == working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED {
                "opened feedback"
            } else {
                "requested baseline"
            };
        let when = elapsed_label
            .clone()
            .map(|value| format!("{value} назад"))
            .or_else(|| recorded_at_label.clone())
            .unwrap_or_else(|| "недавно".to_string());
        let turn_delta_text = turn_token_delta
            .map(format_signed_token_delta)
            .unwrap_or_else(|| "н/д".to_string());
        let context_delta_text = context_used_percent_point_delta
            .map(format_signed_percent_points)
            .unwrap_or_else(|| "н/д".to_string());
        let primary_delta_text = primary_limit_used_percent_point_delta
            .map(|value| format_signed_percent_points(value as f64))
            .unwrap_or_else(|| "н/д".to_string());
        let mut summary = format!(
            "Последний {feedback_label} по {surface_label} был {when}: с тех пор giant thread изменился на {turn_delta_text} токенов, context {context_delta_text}, 5ч used {primary_delta_text}."
        );
        if let Some(overrun) = primary_limit_used_overrun_percent_points {
            summary.push_str(&format!(
                " Это {} против идеального темпа.",
                format_signed_percent_points(overrun)
            ));
        }
        if let Some(delta) = regrowth_since_feedback_tokens {
            summary.push_str(&format!(
                " Regrowth since compaction: {} токенов.",
                format_signed_token_delta(delta)
            ));
        }
        if verified_host_compaction_observed_after_feedback {
            if let Some(delta) = compaction_count_delta {
                summary.push_str(&format!(
                    " После baseline уже прошло {} реальных host compaction.",
                    format_signed_token_delta(delta)
                ));
            } else {
                summary.push_str(" После baseline уже был реальный host compaction.");
            }
        }
        if current_stage != snapshot_stage
            && let Some(stage) = current_stage
        {
            summary.push_str(&format!(" Текущая host stage: {stage}."));
        }
        summary
    } else {
        let snapshot_thread = snapshot_thread_id.unwrap_or("unknown-thread");
        format!(
            "Последний opened feedback по {surface_label} относится к другому thread ({snapshot_thread}), поэтому effect для текущего giant thread не применим."
        )
    };
    let full_scale_client_burn_worsened = measurement_sufficient
        && primary_limit_used_overrun_percent_points.is_some_and(|value| {
            value >= HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_PRIMARY_LIMIT_OVERRUN_PERCENT_POINTS
        });
    let rotate_fallback_recommended = measurement_sufficient
        && (full_scale_client_burn_worsened
            || turn_token_delta.unwrap_or_default()
                >= HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_TURN_TOKEN_DELTA
            || context_used_percent_point_delta.unwrap_or_default()
                >= HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_CONTEXT_PERCENT_POINTS
            || (current_stage == Some("critical_regrowth")
                && regrowth_since_feedback_tokens.unwrap_or_default()
                    >= HOST_CURRENT_THREAD_EFFECT_ROTATE_FALLBACK_REGROWTH_TOKENS));
    let overlay_trial_recommended = measurement_sufficient
        && compact_window_surface
        && current_stage == Some("critical_regrowth")
        && !rotate_fallback_recommended
        && (turn_token_delta.unwrap_or_default()
            >= HOST_CURRENT_THREAD_EFFECT_OVERLAY_TRIAL_TURN_TOKEN_DELTA
            || context_used_percent_point_delta.unwrap_or_default()
                >= HOST_CURRENT_THREAD_EFFECT_OVERLAY_TRIAL_CONTEXT_PERCENT_POINTS
            || regrowth_since_feedback_tokens.unwrap_or_default()
                >= HOST_CURRENT_THREAD_EFFECT_OVERLAY_TRIAL_REGROWTH_TOKENS);
    let material_compaction_gain_observed = verified_host_compaction_observed_after_feedback
        && (turn_token_delta.is_some_and(|value| {
            value <= -(HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_TURN_TOKEN_DELTA as i64)
        }) || context_used_percent_point_delta.is_some_and(|value| {
            value <= -HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_CONTEXT_PERCENT_POINTS
        }) || regrowth_since_feedback_tokens.is_some_and(|value| {
            value <= -(HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_REGROWTH_TOKENS as i64)
        }));
    let effect_verdict = if !same_thread {
        "other_thread"
    } else if measurement_pending {
        "measurement_pending"
    } else if full_scale_client_burn_worsened {
        "full_scale_client_burn_worsened_rotate_fallback_recommended"
    } else if rotate_fallback_recommended {
        "ineffective_rotate_fallback_recommended"
    } else if overlay_trial_recommended {
        "critical_regrowth_overlay_trial_recommended"
    } else if compact_window_surface
        && feedback_kind == working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED
    {
        "opened_compact_surface_observed"
    } else if !compact_window_surface
        && feedback_kind == working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED
    {
        "opened_overlay_surface_observed"
    } else if compact_window_surface {
        "requested_compact_surface_observed"
    } else {
        "requested_overlay_surface_observed"
    };
    let verdict_summary = if measurement_pending {
        format!(
            "Same-thread {surface_label} уже запущен для этого giant thread; сначала дождись измеримого effect before retry."
        )
    } else if full_scale_client_burn_worsened {
        format!(
            "Same-thread {surface_label} локально меняет thread, но полный 5ч burn всё ещё идёт хуже идеального темпа на {}; rotate fallback should become primary.",
            format_signed_percent_points(
                primary_limit_used_overrun_percent_points.unwrap_or_default()
            )
        )
    } else if rotate_fallback_recommended {
        format!(
            "Последний same-thread {surface_label} не остановил regrowth giant thread; rotate fallback should become primary."
        )
    } else if overlay_trial_recommended {
        "Compact window частично удержал giant thread, но critical regrowth уже вернулся; перед rotate попробуй overlay.".to_string()
    } else if same_thread {
        format!(
            "Same-thread {surface_label} ещё стоит держать primary path, пока effect не доказал обратное."
        )
    } else {
        "Этот effect verdict не переключает primary path giant-thread control.".to_string()
    };
    json!({
        "feedback_kind": feedback_kind,
        "command_id": command_id,
        "surface_label": surface_label,
        "recorded_at_epoch_ms": recorded_at_epoch_ms,
        "recorded_at_label": recorded_at_label,
        "elapsed_label": elapsed_label,
        "elapsed_ms": elapsed_ms,
        "same_thread": same_thread,
        "thread_id": snapshot_thread_id,
        "current_thread_id": current_thread_id,
        "turn_token_delta": turn_token_delta,
        "turn_token_delta_per_minute":
            turn_token_delta.and_then(|value| elapsed_ms.and_then(|elapsed| {
                per_minute_rate(value as f64, elapsed)
            })),
        "context_used_percent_point_delta": context_used_percent_point_delta,
        "context_used_percent_point_delta_per_minute":
            context_used_percent_point_delta.and_then(|value| elapsed_ms.and_then(|elapsed| {
                per_minute_rate(value, elapsed)
            })),
        "primary_limit_used_percent_point_delta": primary_limit_used_percent_point_delta,
        "primary_limit_ideal_percent_point_delta": primary_limit_ideal_percent_point_delta,
        "primary_limit_used_overrun_percent_points": primary_limit_used_overrun_percent_points,
        "current_compaction_count": current_compaction_count,
        "snapshot_compaction_count": snapshot_compaction_count,
        "compaction_count_delta": compaction_count_delta,
        "verified_host_compaction_observed_after_feedback":
            verified_host_compaction_observed_after_feedback,
        "regrowth_since_feedback_tokens": regrowth_since_feedback_tokens,
        "regrowth_since_feedback_tokens_per_minute":
            regrowth_since_feedback_tokens.and_then(|value| elapsed_ms.and_then(|elapsed| {
                per_minute_rate(value as f64, elapsed)
            })),
        "snapshot_stage": snapshot_stage,
        "current_stage": current_stage,
        "measurement_pending": measurement_pending,
        "measurement_sufficient": measurement_sufficient,
        "measurement_min_elapsed_ms": HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_ELAPSED_MS,
        "measurement_min_turn_token_delta": HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_TURN_TOKEN_DELTA,
        "measurement_min_context_percent_points": HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_CONTEXT_PERCENT_POINTS,
        "measurement_min_regrowth_tokens": HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_REGROWTH_TOKENS,
        "material_compaction_gain_observed": material_compaction_gain_observed,
        "retry_allowed": !measurement_pending
            && (!rotate_fallback_recommended || material_compaction_gain_observed),
        "effect_verdict": effect_verdict,
        "full_scale_client_burn_worsened": full_scale_client_burn_worsened,
        "rotate_fallback_recommended": rotate_fallback_recommended,
        "overlay_trial_recommended": overlay_trial_recommended,
        "verdict_summary": verdict_summary,
        "summary": summary,
        "note": if feedback_kind == working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED {
            "Это observational delta после operator-confirmed open. Он не доказывает причинность, но показывает, насколько giant thread продолжил расти после выбранного same-thread surface."
        } else if full_scale_client_burn_worsened {
            "Это observational delta от request-side baseline. Даже если thread локально уменьшился, для giant-thread product path surface считается честно неуспешным, если полный 5ч burn после него идёт заметно хуже идеального темпа."
        } else if measurement_pending {
            "Это ещё слишком свежий same-thread baseline. Не гоняй повторный host-control retry, пока не накопится измеримый effect или хотя бы минимальное окно наблюдения."
        } else {
            "Это observational delta от момента request-side baseline. Он не доказывает, что surface действительно открылся, но уже показывает, как giant thread менялся после запуска same-thread control."
        },
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn host_current_thread_control_effect_payload(
    restore_context: &Value,
    client_live_meter: &Value,
    host_context_compaction: &Value,
) -> Value {
    host_current_thread_control_effect_payload_for_command(
        restore_context,
        client_live_meter,
        host_context_compaction,
        None,
    )
}

fn host_current_thread_control_effect_rotate_fallback_recommended(effect: &Value) -> bool {
    effect["rotate_fallback_recommended"].as_bool() == Some(true)
}

fn host_current_thread_control_effect_material_compaction_gain_observed(effect: &Value) -> bool {
    if effect["verified_host_compaction_observed_after_feedback"].as_bool() != Some(true) {
        return false;
    }
    effect["turn_token_delta"].as_i64().is_some_and(|value| {
        value <= -(HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_TURN_TOKEN_DELTA as i64)
    }) || effect["context_used_percent_point_delta"]
        .as_f64()
        .is_some_and(|value| {
            value <= -HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_CONTEXT_PERCENT_POINTS
        })
        || effect["regrowth_since_feedback_tokens"]
            .as_i64()
            .is_some_and(|value| {
                value <= -(HOST_CURRENT_THREAD_EFFECT_MEASUREMENT_MIN_REGROWTH_TOKENS as i64)
            })
}

fn host_current_thread_control_effect_surface_exhausted_after_verified_failure(
    effect: &Value,
) -> bool {
    effect["verified_host_compaction_observed_after_feedback"].as_bool() == Some(true)
        && host_current_thread_control_effect_rotate_fallback_recommended(effect)
        && !host_current_thread_control_effect_material_compaction_gain_observed(effect)
}

pub(super) fn host_current_thread_control_feedback_pending_from_effect(
    feedback_kind: Option<&str>,
    effect: &Value,
) -> bool {
    feedback_kind == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED)
        && host_current_thread_control_effect_same_thread(effect)
        && effect["verified_host_compaction_observed_after_feedback"].as_bool() != Some(true)
}

fn host_current_thread_control_effect_same_thread(effect: &Value) -> bool {
    effect["same_thread"].as_bool() == Some(true)
}

fn preferred_command_id_by_critical_regrowth_rate(
    compact_effect: &Value,
    overlay_effect: &Value,
) -> Option<&'static str> {
    if !host_current_thread_control_effect_same_thread(compact_effect)
        || !host_current_thread_control_effect_same_thread(overlay_effect)
    {
        return None;
    }
    let compact_turn_rate = compact_effect["turn_token_delta_per_minute"].as_f64()?;
    let overlay_turn_rate = overlay_effect["turn_token_delta_per_minute"].as_f64()?;
    let compact_context_rate = compact_effect["context_used_percent_point_delta_per_minute"]
        .as_f64()
        .unwrap_or(f64::INFINITY);
    let overlay_context_rate = overlay_effect["context_used_percent_point_delta_per_minute"]
        .as_f64()
        .unwrap_or(f64::INFINITY);
    if overlay_turn_rate < compact_turn_rate
        || ((overlay_turn_rate - compact_turn_rate).abs() < f64::EPSILON
            && overlay_context_rate < compact_context_rate)
    {
        Some(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
    } else {
        Some(working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID)
    }
}

pub(super) fn selected_host_current_thread_control_state(
    report: &Value,
    restore_context: &Value,
    client_live_meter: &Value,
    host_context_compaction: &Value,
) -> (Value, Value, bool) {
    let host_context_compaction_stage =
        host_context_compaction_stage_from_payload(host_context_compaction);
    let thread_id = preferred_budget_guard_thread_id(report, restore_context);
    let compact_command_id = working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID;
    let overlay_command_id = working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID;
    let compact_effect = host_current_thread_control_effect_payload_for_command(
        restore_context,
        client_live_meter,
        host_context_compaction,
        Some(compact_command_id),
    );
    let overlay_effect = host_current_thread_control_effect_payload_for_command(
        restore_context,
        client_live_meter,
        host_context_compaction,
        Some(overlay_command_id),
    );
    let compact_failed = host_current_thread_control_effect_surface_exhausted_after_verified_failure(
        &compact_effect,
    )
        || latest_host_current_thread_control_feedback_kind_for_command(
            restore_context,
            Some(compact_command_id),
        ) == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED);
    let compact_feedback_pending = host_current_thread_control_feedback_pending_from_effect(
        latest_host_current_thread_control_feedback_kind_for_command(
            restore_context,
            Some(compact_command_id),
        ),
        &compact_effect,
    ) && !compact_failed;
    let compact_overlay_trial_recommended =
        compact_effect["overlay_trial_recommended"].as_bool() == Some(true);
    let compact_measurement_pending =
        compact_effect["measurement_pending"].as_bool() == Some(true) && !compact_failed;
    let overlay_failed = host_current_thread_control_effect_surface_exhausted_after_verified_failure(
        &overlay_effect,
    )
        || latest_host_current_thread_control_feedback_kind_for_command(
            restore_context,
            Some(overlay_command_id),
        ) == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED);
    let overlay_feedback_pending = host_current_thread_control_feedback_pending_from_effect(
        latest_host_current_thread_control_feedback_kind_for_command(
            restore_context,
            Some(overlay_command_id),
        ),
        &overlay_effect,
    ) && !overlay_failed;
    let overlay_measurement_pending =
        overlay_effect["measurement_pending"].as_bool() == Some(true) && !overlay_failed;
    let rate_selected_command_id = if host_context_compaction_stage.critical_regrowth_active()
        && !compact_measurement_pending
        && !overlay_measurement_pending
        && !compact_failed
        && !overlay_failed
    {
        preferred_command_id_by_critical_regrowth_rate(&compact_effect, &overlay_effect)
    } else {
        None
    };
    let pending_feedback_selected_command_id =
        match (compact_feedback_pending, overlay_feedback_pending) {
            (true, false) => Some(compact_command_id),
            (false, true) => Some(overlay_command_id),
            (true, true) => {
                let compact_recorded_at =
                    latest_host_current_thread_control_feedback_recorded_at_for_command(
                        restore_context,
                        Some(compact_command_id),
                    )
                    .unwrap_or_default();
                let overlay_recorded_at =
                    latest_host_current_thread_control_feedback_recorded_at_for_command(
                        restore_context,
                        Some(overlay_command_id),
                    )
                    .unwrap_or_default();
                if overlay_recorded_at > compact_recorded_at {
                    Some(overlay_command_id)
                } else {
                    Some(compact_command_id)
                }
            }
            (false, false) => None,
        };
    let primary_command_id = if let Some(command_id) = pending_feedback_selected_command_id {
        command_id
    } else if let Some(command_id) = rate_selected_command_id {
        command_id
    } else if compact_measurement_pending {
        compact_command_id
    } else if overlay_measurement_pending {
        overlay_command_id
    } else if host_context_compaction_stage.preserve_active() {
        if (compact_failed || compact_overlay_trial_recommended) && !overlay_failed {
            overlay_command_id
        } else {
            compact_command_id
        }
    } else {
        overlay_command_id
    };
    let surface =
        working_state::build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
            thread_id.as_deref(),
            host_context_compaction_stage,
            Some(primary_command_id),
        );
    let effect = if primary_command_id == compact_command_id {
        compact_effect
    } else {
        overlay_effect
    };
    let same_thread_compaction_preferred =
        surface["available"].as_bool() == Some(true) && !(compact_failed && overlay_failed);
    (surface, effect, same_thread_compaction_preferred)
}

pub(super) fn decorate_host_current_thread_control_surface(
    surface: &Value,
    effect: &Value,
    feedback_pending: bool,
    feedback_summary: Option<&str>,
) -> Value {
    let mut surface = surface.clone();
    let measurement_pending = effect["measurement_pending"].as_bool() == Some(true);
    let surface_exhausted_after_verified_failure =
        host_current_thread_control_effect_surface_exhausted_after_verified_failure(effect);
    let retry_allowed = effect["retry_allowed"].as_bool().unwrap_or(true)
        && !feedback_pending
        && !surface_exhausted_after_verified_failure;
    let retry_blocked_reason = if retry_allowed {
        None
    } else if let Some(summary) = feedback_summary.filter(|value| !value.trim().is_empty()) {
        Some(summary.to_string())
    } else if surface_exhausted_after_verified_failure {
        effect["verdict_summary"].as_str().map(str::to_string)
    } else if measurement_pending {
        effect["verdict_summary"].as_str().map(str::to_string)
    } else {
        Some("Same-thread control уже запрошен для этого giant thread. Сначала дождись измеримого effect.".to_string())
    };
    if let Some(object) = surface.as_object_mut() {
        object.insert("feedback_pending".to_string(), json!(feedback_pending));
        object.insert(
            "measurement_pending".to_string(),
            json!(measurement_pending),
        );
        object.insert("retry_allowed".to_string(), json!(retry_allowed));
        object.insert(
            "retry_blocked_reason".to_string(),
            retry_blocked_reason.map_or(Value::Null, Value::String),
        );
        object.insert(
            "effect_verdict".to_string(),
            effect["effect_verdict"].clone(),
        );
        object.insert("effect_summary".to_string(), effect["summary"].clone());
        object.insert(
            "surface_exhausted_after_verified_failure".to_string(),
            json!(surface_exhausted_after_verified_failure),
        );
        object.insert(
            "availability_state".to_string(),
            if surface_exhausted_after_verified_failure {
                json!("exhausted_after_verified_failure")
            } else {
                json!("available")
            },
        );
        if surface_exhausted_after_verified_failure {
            object.insert("available".to_string(), json!(false));
            object.insert("automation_ready".to_string(), json!(false));
        }
    }
    surface
}

fn plausible_codex_thread_id(value: &str) -> bool {
    value.len() == 36 && value.chars().filter(|ch| *ch == '-').count() == 4
}

fn preferred_budget_guard_thread_id(report: &Value, restore_context: &Value) -> Option<String> {
    [
        report["client_live_meter"]["thread_id"].as_str(),
        restore_context["thread_id"].as_str(),
        codex_threads::current_thread_id().as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|value| !value.is_empty() && plausible_codex_thread_id(value))
    .map(str::to_string)
}

pub(super) fn latest_host_context_compaction_payload(
    report: &Value,
    restore_context: &Value,
) -> Value {
    let Some(thread_id) = preferred_budget_guard_thread_id(report, restore_context) else {
        return Value::Null;
    };
    let Some(observation) =
        codex_threads::latest_rollout_context_compaction_observation_for_thread(&thread_id)
            .ok()
            .flatten()
    else {
        return Value::Null;
    };
    let current_turn_total_tokens = report["client_live_meter"]["client_turn_total_tokens"]
        .as_u64()
        .unwrap_or_default();
    let current_thread_bound = report["client_live_meter"]["current_thread_bound"]
        .as_bool()
        .unwrap_or(false);
    let growth_tokens =
        current_turn_total_tokens.saturating_sub(observation.post_compaction_turn_total_tokens);
    let recovered_surface_tokens = observation
        .pre_compaction_turn_total_tokens
        .saturating_sub(observation.post_compaction_turn_total_tokens);
    let rebound_ratio = if recovered_surface_tokens > 0 {
        growth_tokens as f64 / recovered_surface_tokens as f64
    } else {
        0.0
    };
    let preserve_active = current_thread_bound
        && current_turn_total_tokens > 0
        && observation.post_compaction_turn_total_tokens > 0
        && growth_tokens >= HOST_CONTEXT_COMPACTION_PRESERVE_TRIGGER_TOKENS;
    let critical_regrowth_active = preserve_active
        && (growth_tokens >= HOST_CONTEXT_COMPACTION_CRITICAL_TRIGGER_TOKENS
            || rebound_ratio >= HOST_CONTEXT_COMPACTION_CRITICAL_REBOUND_RATIO);
    let stage = if critical_regrowth_active {
        "critical_regrowth"
    } else if preserve_active {
        "preserve"
    } else {
        "inactive"
    };
    json!({
        "thread_id": observation.thread_id,
        "rollout_path": observation.rollout_path,
        "compacted_at_epoch_ms": observation.compacted_at_epoch_ms,
        "pre_compaction_turn_total_tokens": observation.pre_compaction_turn_total_tokens,
        "post_compaction_turn_total_tokens": observation.post_compaction_turn_total_tokens,
        "post_compaction_turn_id": observation.post_compaction_turn_id,
        "current_turn_total_tokens": current_turn_total_tokens,
        "recovered_surface_tokens": recovered_surface_tokens,
        "growth_since_compaction_tokens": growth_tokens,
        "regrowth_of_recovered_surface_ratio": rebound_ratio,
        "preserve_trigger_tokens": HOST_CONTEXT_COMPACTION_PRESERVE_TRIGGER_TOKENS,
        "critical_trigger_tokens": HOST_CONTEXT_COMPACTION_CRITICAL_TRIGGER_TOKENS,
        "critical_rebound_ratio": HOST_CONTEXT_COMPACTION_CRITICAL_REBOUND_RATIO,
        "stage": stage,
        "preserve_active": preserve_active,
        "critical_regrowth_active": critical_regrowth_active,
        "current_thread_bound": current_thread_bound,
        "compaction_count": observation.compaction_count,
        "observation_source": observation.observation_source,
    })
}

pub(super) fn host_context_compaction_stage_from_payload(
    host_context_compaction: &Value,
) -> working_state::HostContextCompactionStage {
    match host_context_compaction["stage"]
        .as_str()
        .unwrap_or("inactive")
    {
        "critical_regrowth" => working_state::HostContextCompactionStage::CriticalRegrowth,
        "preserve" => working_state::HostContextCompactionStage::Preserve,
        _ => working_state::HostContextCompactionStage::Inactive,
    }
}

pub(super) fn compact_host_context_compaction_payload(value: &Value) -> Value {
    let Some(node) = value.as_object() else {
        return Value::Null;
    };
    let mut compact = serde_json::Map::new();
    for field in [
        "stage",
        "preserve_active",
        "critical_regrowth_active",
        "current_thread_bound",
        "post_compaction_turn_total_tokens",
        "current_turn_total_tokens",
        "growth_since_compaction_tokens",
        "recovered_surface_tokens",
        "regrowth_of_recovered_surface_ratio",
    ] {
        if node.get(field).is_some_and(|item| !item.is_null()) {
            compact.insert(field.to_string(), value[field].clone());
        }
    }
    Value::Object(compact)
}

pub(super) fn client_budget_target_active(target_percent: u64) -> bool {
    target_percent > 0
}

pub(super) fn client_budget_target_percent_f64(target_percent: u64) -> f64 {
    target_percent as f64
}

pub(super) fn client_budget_target_alert_label(target_percent: u64) -> String {
    format!("цель {target_percent}% не достигнута")
}

pub(super) fn client_budget_target_sentence(target_percent: u64) -> String {
    format!("Целевая реальная экономия для Amai сейчас задана как не ниже {target_percent}%.")
}

pub(super) fn client_budget_target_shortfall_sentence(
    target_percent: u64,
    kpi_percent: Option<f64>,
) -> String {
    format!(
        "точный 5ч KPI пока даёт только экономию {} вместо целевых {}%",
        format_percent(kpi_percent),
        target_percent
    )
}

pub(super) fn allowed_client_budget_target_values() -> Vec<u64> {
    (0..=working_state::MAX_CLIENT_BUDGET_TARGET_PERCENT)
        .step_by(working_state::CLIENT_BUDGET_TARGET_STEP_PERCENT as usize)
        .collect()
}

pub(super) fn client_budget_target_chat_command(target_percent: u64) -> String {
    format!("экономия_{target_percent}%")
}

pub(super) fn client_budget_compact_chat_command() -> &'static str {
    continuity::CLIENT_BUDGET_COMPACT_CHAT_COMMAND
}

pub(super) fn global_client_limit_source(client_live_meter: &Value) -> Option<Value> {
    if preferred_client_limit_meter_is_exact(client_live_meter)
        && !current_session_client_live_meter_available(client_live_meter)
    {
        return Some(json!({
            "source_kind": "codex_app_server_account_rate_limits_read_v1",
            "derived_from_latest_observed_client_limits": false,
            "truly_global_source_materialized": true,
            "status_bar_correlated": true,
            "authoritative_for": [
                "live_client_limits_now",
                "global_client_limit_hint",
                "wait_for_global_client_budget_recovery_when_critical"
            ],
            "not_authoritative_for": [
                "thread_local_rotate_pressure",
                "live_turn_rows"
            ],
            "observed_at_epoch_ms": preferred_client_limit_observed_at_epoch_ms(client_live_meter),
            "summary": "При отсутствии current-thread binding Amai читает exact global rate limits напрямую из codex app-server account/rateLimits/read. Этот source совпадает с VS Code status bar для 5ч/7д окна и годится для честного live client limit surface и hard wait при глобальном исчерпании, но не для thread-local rotate pressure.",
        }));
    }
    if !client_live_meter_is_observed(client_live_meter)
        || client_live_meter_current_thread_bound(client_live_meter)
    {
        return None;
    }
    Some(working_state::build_global_client_limit_source_contract())
}

pub(super) fn global_client_limit_guard(
    client_live_meter: &Value,
) -> Option<GlobalClientLimitGuard> {
    let limit_surface = if preferred_client_limit_meter_is_exact(client_live_meter)
        && !current_session_client_live_meter_available(client_live_meter)
    {
        preferred_client_limit_meter_surface(client_live_meter)
    } else if client_live_meter_is_observed(client_live_meter)
        && !client_live_meter_current_thread_bound(client_live_meter)
    {
        Some(client_live_meter)
    } else {
        None
    }?;
    let primary_remaining_percent = client_limit_remaining_percent(
        limit_surface,
        "primary_limit_remaining_percent",
        "primary_limit_used_percent",
    );
    let secondary_remaining_percent = client_limit_remaining_percent(
        limit_surface,
        "secondary_limit_remaining_percent",
        "secondary_limit_used_percent",
    );
    if primary_remaining_percent <= 5.0 || secondary_remaining_percent <= 5.0 {
        Some(GlobalClientLimitGuard {
            severity: "critical",
            status_label: "глобальный лимит клиента почти исчерпан",
            primary_remaining_percent,
            secondary_remaining_percent,
        })
    } else if primary_remaining_percent <= 10.0 || secondary_remaining_percent <= 10.0 {
        Some(GlobalClientLimitGuard {
            severity: "alert",
            status_label: "глобальный лимит клиента на исходе",
            primary_remaining_percent,
            secondary_remaining_percent,
        })
    } else {
        None
    }
}

pub(super) fn global_client_limit_guard_note(
    guard: GlobalClientLimitGuard,
    client_limits_value: Option<&str>,
    exact_live_source: bool,
) -> String {
    let rendered_limits =
        client_limits_value.unwrap_or("последнее observed значение лимита клиента");
    if exact_live_source && guard.severity == "critical" {
        format!(
            "Current thread binding ещё не materialized, но Amai уже читает live global rate-limit source клиента напрямую из codex app-server: 5ч {}, 7д {}. Этого уже достаточно для fail-closed wait path: новая чистая рабочая поверхность не поможет, нужно дождаться восстановления внешнего клиентского лимита. Текущее live значение: {rendered_limits}.",
            format_percent(Some(guard.primary_remaining_percent)),
            format_percent(Some(guard.secondary_remaining_percent)),
        )
    } else if exact_live_source {
        format!(
            "Current thread binding ещё не materialized, но Amai уже читает live global rate-limit source клиента напрямую из codex app-server: 5ч {}, 7д {}. Это пока только global warning hint: rotate gate не включается, но следующий substantive reply стоит делать только после повторной проверки budget. Текущее live значение: {rendered_limits}.",
            format_percent(Some(guard.primary_remaining_percent)),
            format_percent(Some(guard.secondary_remaining_percent)),
        )
    } else if guard.severity == "critical" {
        format!(
            "Current thread binding ещё не materialized, поэтому Amai видит только последнее observed значение client limits: 5ч {}, 7д {}. Этого уже достаточно для fail-closed wait path: новая чистая рабочая поверхность не поможет, нужно дождаться восстановления внешнего клиентского лимита. Текущее observed значение: {rendered_limits}.",
            format_percent(Some(guard.primary_remaining_percent)),
            format_percent(Some(guard.secondary_remaining_percent)),
        )
    } else {
        format!(
            "Current thread binding ещё не materialized, поэтому Amai видит только последнее observed значение client limits: 5ч {}, 7д {}. Это пока только global warning hint: rotate gate не включается, но следующий substantive reply стоит делать только после повторной проверки budget. Текущее observed значение: {rendered_limits}.",
            format_percent(Some(guard.primary_remaining_percent)),
            format_percent(Some(guard.secondary_remaining_percent)),
        )
    }
}
#[cfg(test)]
pub(super) fn client_turn_pressure_guard(
    client_live_meter: &Value,
    exact_pair: Option<(u64, u64, i64, f64)>,
    client_limit_hourly_burn: &Value,
    current_live_turn: &Value,
) -> Option<ClientTurnPressureGuard> {
    client_turn_pressure_guard_with_target(
        client_live_meter,
        exact_pair,
        client_limit_hourly_burn,
        current_live_turn,
        working_state::default_client_budget_target_percent(),
    )
}

pub(super) fn client_turn_pressure_guard_with_target(
    client_live_meter: &Value,
    exact_pair: Option<(u64, u64, i64, f64)>,
    client_limit_hourly_burn: &Value,
    current_live_turn: &Value,
    client_budget_target_percent: u64,
) -> Option<ClientTurnPressureGuard> {
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let client_budget_target_active = client_budget_target_active(client_budget_target_percent);
    let client_budget_target_percent_f64 =
        client_budget_target_percent_f64(client_budget_target_percent);
    let limit_surface =
        preferred_client_limit_meter_surface(client_live_meter).unwrap_or(client_live_meter);
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .unwrap_or(0);
    let model_context_window = client_live_meter["latest_model_context_window"]
        .as_u64()
        .unwrap_or(0);
    if turn_total_tokens == 0 || model_context_window == 0 {
        return None;
    }
    let context_used_percent = client_live_meter["context_used_percent"]
        .as_f64()
        .unwrap_or_else(|| (turn_total_tokens as f64 * 100.0) / model_context_window as f64);
    let primary_remaining_percent = client_limit_remaining_percent(
        limit_surface,
        "primary_limit_remaining_percent",
        "primary_limit_used_percent",
    );
    let secondary_remaining_percent = client_limit_remaining_percent(
        limit_surface,
        "secondary_limit_remaining_percent",
        "secondary_limit_used_percent",
    );
    let exact_pair_missing = exact_pair.is_none();
    let hourly_burn_classification =
        observed_client_limit_hourly_burn_classification(client_limit_hourly_burn);
    let hourly_burn_kpi_percent = if client_limit_hourly_burn["status"].as_str() == Some("observed")
    {
        client_limit_hourly_burn["kpi_percent"].as_f64()
    } else {
        None
    };
    let hourly_burn_overspend = hourly_burn_classification == Some("overspend");
    let hourly_burn_target_saving = client_budget_target_active
        && hourly_burn_classification == Some("saving")
        && hourly_burn_kpi_percent.is_some_and(|value| value >= client_budget_target_percent_f64);
    let hourly_burn_below_target = client_budget_target_active
        && client_limit_hourly_burn["status"].as_str() == Some("observed")
        && !hourly_burn_target_saving;
    let no_amai_activity_in_current_live_turn =
        current_live_turn["status"].as_str() == Some("no_amai_activity_in_current_live_turn");
    let full_turn_savings_pct = exact_pair.and_then(|(_, _, saved_tokens, _)| {
        let without_amai_total_tokens = if saved_tokens >= 0 {
            turn_total_tokens.saturating_add(saved_tokens as u64)
        } else {
            turn_total_tokens.saturating_sub(saved_tokens.unsigned_abs())
        };
        if without_amai_total_tokens == 0 {
            None
        } else {
            Some((saved_tokens as f64 * 100.0) / without_amai_total_tokens as f64)
        }
    });
    let weak_amai_share = full_turn_savings_pct
        .map(|value| value <= 3.0)
        .unwrap_or(true);
    let tiny_amai_share = full_turn_savings_pct
        .map(|value| value <= 1.0)
        .unwrap_or(true);
    let negligible_amai_share = full_turn_savings_pct
        .map(|value| value <= 0.5)
        .unwrap_or(false);
    let below_target_full_turn_savings = client_budget_target_active
        && full_turn_savings_pct
            .map(|value| value < client_budget_target_percent_f64)
            .unwrap_or(false);
    let very_early_context_pressure = context_used_percent >= 18.0;
    let early_context_pressure = context_used_percent >= 25.0;
    let moderate_context_pressure = context_used_percent >= 30.0;
    let extreme_context_pressure = context_used_percent >= 70.0;
    let high_context_pressure = context_used_percent >= 50.0;
    let small_kpi_thread = turn_total_tokens >= 10_000 || context_used_percent >= 4.0;
    let moderate_kpi_thread = turn_total_tokens >= 18_000 || context_used_percent >= 7.0;
    let early_live_thread = turn_total_tokens >= 45_000 || very_early_context_pressure;
    let early_large_live_thread = turn_total_tokens >= 60_000 || early_context_pressure;
    let inflation_locking_in_burn = turn_total_tokens >= 75_000 || moderate_context_pressure;
    let large_live_thread = turn_total_tokens >= 90_000 || context_used_percent >= 40.0;
    let huge_live_thread = turn_total_tokens >= 120_000 || context_used_percent >= 50.0;
    let emergency_primary_limit = primary_remaining_percent <= 10.0;
    let critical_primary_limit = primary_remaining_percent <= 20.0;
    let low_primary_limit = primary_remaining_percent <= 35.0;
    let stressed_primary_limit = primary_remaining_percent <= 50.0;
    let softened_primary_limit = primary_remaining_percent <= 75.0;
    let early_primary_limit = primary_remaining_percent <= 90.0;
    let generous_primary_limit = primary_remaining_percent <= 95.0;

    if hourly_burn_target_saving && !critical_primary_limit {
        return None;
    }

    let (severity, status_label) = if (no_amai_activity_in_current_live_turn && huge_live_thread)
        || (no_amai_activity_in_current_live_turn && hourly_burn_overspend && moderate_kpi_thread)
        || (no_amai_activity_in_current_live_turn && hourly_burn_below_target && early_live_thread)
        || (hourly_burn_below_target && inflation_locking_in_burn && weak_amai_share)
        || (hourly_burn_overspend
            && early_large_live_thread
            && weak_amai_share
            && softened_primary_limit)
        || (hourly_burn_overspend && huge_live_thread && tiny_amai_share)
        || (hourly_burn_overspend && large_live_thread && tiny_amai_share && generous_primary_limit)
        || (hourly_burn_below_target
            && large_live_thread
            && high_context_pressure
            && below_target_full_turn_savings)
        || (exact_pair_missing && inflation_locking_in_burn && softened_primary_limit)
        || (exact_pair_missing && large_live_thread)
        || (exact_pair_missing && emergency_primary_limit && early_live_thread)
        || (early_large_live_thread && negligible_amai_share && stressed_primary_limit)
        || (inflation_locking_in_burn && tiny_amai_share && early_primary_limit)
        || (((extreme_context_pressure && critical_primary_limit) || context_used_percent >= 90.0)
            && tiny_amai_share)
        || (emergency_primary_limit && early_large_live_thread)
        || (emergency_primary_limit && huge_live_thread && weak_amai_share)
    {
        ("critical", "новый чат нужен сейчас")
    } else if (no_amai_activity_in_current_live_turn && inflation_locking_in_burn)
        || (no_amai_activity_in_current_live_turn && hourly_burn_below_target && small_kpi_thread)
        || (no_amai_activity_in_current_live_turn
            && hourly_burn_below_target
            && moderate_kpi_thread)
        || (hourly_burn_below_target && small_kpi_thread && below_target_full_turn_savings)
        || (hourly_burn_below_target && moderate_kpi_thread && below_target_full_turn_savings)
        || (hourly_burn_below_target && early_live_thread && below_target_full_turn_savings)
        || (exact_pair_missing && early_live_thread)
        || (early_live_thread && negligible_amai_share && generous_primary_limit)
        || (((high_context_pressure && low_primary_limit) || extreme_context_pressure)
            && weak_amai_share)
        || (critical_primary_limit && large_live_thread && weak_amai_share)
    {
        ("alert", "новый чат рекомендован")
    } else {
        return None;
    };

    Some(ClientTurnPressureGuard {
        severity,
        status_label,
        client_budget_target_percent,
        turn_total_tokens,
        model_context_window,
        context_used_percent,
        primary_remaining_percent,
        secondary_remaining_percent,
        full_turn_savings_pct,
        hourly_burn_classification,
        hourly_burn_kpi_percent,
        no_amai_activity_in_current_live_turn,
    })
}

fn observed_client_limit_hourly_burn_classification(
    client_limit_hourly_burn: &Value,
) -> Option<&'static str> {
    if client_limit_hourly_burn["status"].as_str() != Some("observed") {
        return None;
    }
    match client_limit_hourly_burn["classification"].as_str() {
        Some("overspend") => Some("overspend"),
        Some("saving") => Some("saving"),
        Some("one_to_one") => Some("one_to_one"),
        _ => None,
    }
}

pub(crate) fn client_turn_pressure_display_status_label<'a>(
    status_label: &'a str,
    same_thread_compaction_preferred: bool,
) -> &'a str {
    if same_thread_compaction_preferred {
        match status_label {
            "новый чат нужен сейчас" => "сожми текущий чат сейчас",
            "новый чат рекомендован" => "сожми текущий чат",
            _ => status_label,
        }
    } else {
        status_label
    }
}

pub(crate) const CLIENT_TURN_PRESSURE_ROTATE_STATUS_LABELS: [&str; 2] =
    ["сожми текущий чат", "сожми текущий чат сейчас"];

pub(super) fn delivery_surface_status_label(status_label: &str) -> &str {
    match status_label {
        "новый чат нужен сейчас" => {
            "новая чистая рабочая поверхность нужна сейчас"
        }
        "новый чат рекомендован" => {
            "новая чистая рабочая поверхность рекомендована"
        }
        _ => status_label,
    }
}

pub(super) fn client_turn_pressure_note_sentence_for_preference(
    guard: Option<ClientTurnPressureGuard>,
    same_thread_compaction_preferred: bool,
) -> Option<String> {
    let guard = guard?;
    let full_turn_sentence = guard
        .full_turn_savings_pct
        .map(|value| {
            format!(
                "Amai в полном live-turn даёт только {}",
                format_percent(Some(value))
            )
        })
        .unwrap_or_else(|| {
            "Amai в полном live-turn пока ещё нельзя честно измерить exact same-meter парой, а текущий turn уже раздувается быстрее, чем это можно доказать"
                .to_string()
        });
    let kpi_sentence = guard
        .hourly_burn_classification
        .map(|classification| match classification {
            "overspend" => format!(
                "точный 5ч KPI уже показывает переплату {}",
                format_percent(guard.hourly_burn_kpi_percent)
            ),
            "saving" if client_budget_target_active(guard.client_budget_target_percent) => {
                client_budget_target_shortfall_sentence(
                    guard.client_budget_target_percent,
                    guard.hourly_burn_kpi_percent,
                )
            }
            "saving" => format!(
                "точный 5ч KPI показывает экономию {}, но текущий live-turn всё равно раздувается быстрее, чем Amai materialized устойчивый exact pair",
                format_percent(guard.hourly_burn_kpi_percent)
            ),
            _ if client_budget_target_active(guard.client_budget_target_percent) => format!(
                "точный 5ч KPI идёт только 1:1 к сбросу, а не в режим целевой экономии {}%",
                guard.client_budget_target_percent
            ),
            _ => "точный 5ч KPI идёт только 1:1 к сбросу, а не в safe-saving режим".to_string(),
        })
        .unwrap_or_else(|| "точный 5ч KPI пока ещё не materialized".to_string());
    let no_amai_sentence = if guard.no_amai_activity_in_current_live_turn {
        " При этом в текущем live-turn вообще не видно Amai-активности, поэтому burn создаёт сам размер thread/context, а не retrieval-помощь."
    } else {
        ""
    };
    Some(format!(
        "{} последний observed запрос уже занимает {} из {} окна, по 5ч лимиту остаётся {}, по 7д — {}, {}, а {}.{}",
        if same_thread_compaction_preferred {
            "Сейчас выгоднее сначала сжать текущий giant thread через same-thread compact window:"
        } else {
            "Сейчас giant thread уже требует fallback-внимания: не раздувай его дальше; новая рабочая поверхность допустима только после подтверждённого провала same-thread control:"
        },
        format_u64(Some(guard.turn_total_tokens)),
        format_u64(Some(guard.model_context_window)),
        format_percent(Some(guard.primary_remaining_percent)),
        format_percent(Some(guard.secondary_remaining_percent)),
        kpi_sentence,
        full_turn_sentence,
        no_amai_sentence,
    ))
}

pub(super) fn client_turn_pressure_metric_row(
    guard: Option<ClientTurnPressureGuard>,
    action_bundle: Option<&Value>,
    same_thread_compaction_preferred: bool,
) -> Option<Value> {
    let guard = guard?;
    Some(metric_row(
        "Следующее действие",
        if same_thread_compaction_preferred && guard.severity == "critical" {
            "открой compact window текущего giant thread и проверь effect".to_string()
        } else if same_thread_compaction_preferred {
            "защити текущий giant thread через same-thread control".to_string()
        } else if guard.severity == "critical" {
            "не раздувай giant thread; fallback через handoff/startup только если same-thread control реально не помог".to_string()
        } else {
            "не раздувай giant thread; handoff/новая рабочая поверхность допустимы только как fallback после same-thread failure".to_string()
        },
        Some(
            client_turn_pressure_tooltip(guard, action_bundle, same_thread_compaction_preferred)
                .as_str(),
        ),
    ))
}

pub(super) fn client_turn_pressure_tooltip(
    guard: ClientTurnPressureGuard,
    action_bundle: Option<&Value>,
    same_thread_compaction_preferred: bool,
) -> String {
    let mut tooltip = format!(
        "Этот guard показывает, что внешний лимит клиента уже горит быстрее, чем Amai успевает экономить в полном live-turn.\n- Последний observed запрос клиента: {} из {} ({})\n- По лимиту 5ч остаётся {}\n- По лимиту 7д остаётся {}",
        format_u64(Some(guard.turn_total_tokens)),
        format_u64(Some(guard.model_context_window)),
        format_percent(Some(guard.context_used_percent)),
        format_percent(Some(guard.primary_remaining_percent)),
        format_percent(Some(guard.secondary_remaining_percent)),
    );
    if let Some(full_turn_savings_pct) = guard.full_turn_savings_pct {
        tooltip.push_str(&format!(
            "\n- Amai в полном live-turn сейчас даёт только {}",
            format_percent(Some(full_turn_savings_pct))
        ));
    } else {
        tooltip.push_str(
            "\n- Exact same-meter share Amai в полном live-turn пока ещё не materialized, а текущий turn уже слишком раздут, чтобы откладывать same-thread compaction и ждать точный pair на длинном контексте",
        );
    }
    if let Some(classification) = guard.hourly_burn_classification {
        let line = match classification {
            "overspend" => format!(
                "\n- Exact 5ч KPI из VS Code toolbar уже показывает переплату {}",
                format_percent(guard.hourly_burn_kpi_percent)
            ),
            "saving" if client_budget_target_active(guard.client_budget_target_percent) => format!(
                "\n- {}",
                client_budget_target_shortfall_sentence(
                    guard.client_budget_target_percent,
                    guard.hourly_burn_kpi_percent,
                )
            ),
            "saving" => format!(
                "\n- Exact 5ч KPI уже показывает экономию {}, но текущий live-turn всё равно раздувается быстрее, чем Amai materialized устойчивый exact pair",
                format_percent(guard.hourly_burn_kpi_percent)
            ),
            _ if client_budget_target_active(guard.client_budget_target_percent) => format!(
                "\n- Exact 5ч KPI пока идёт лишь 1:1 к reset, а не в режим целевой экономии {}%",
                guard.client_budget_target_percent
            ),
            _ => {
                "\n- Exact 5ч KPI пока идёт лишь 1:1 к reset, а не в safe-saving режиме".to_string()
            }
        };
        tooltip.push_str(&line);
    }
    if guard.no_amai_activity_in_current_live_turn {
        tooltip.push_str(
            "\n- В текущем live-turn нет retrieval_context_pack от Amai: расход сейчас создаёт сам раздутый thread/context, поэтому лучший способ спасти 5ч окно — не раздувать дальше этот thread и удержать same-thread compact surface",
        );
    }
    if same_thread_compaction_preferred {
        tooltip.push_str(
            "\n- При таком соотношении продолжение в том же thread жжёт внешний клиентский лимит в основном размером самого thread/context, а не Amai-delta\n- Для этого giant thread Amai уже поднял same-thread compact window как primary action. Новая чистая рабочая поверхность остаётся fallback, если compact surface не уменьшит regrowth/burn.",
        );
    } else {
        tooltip.push_str(
            "\n- При таком соотношении продолжение в том же thread жжёт внешний клиентский лимит в основном размером самого thread/context, а не Amai-delta\n- Если same-thread control недоступен или уже подтверждённо не помог, handoff и новая чистая рабочая поверхность остаются только fallback.",
        );
    }
    if let Some(bundle) = action_bundle {
        if let Some(summary) = bundle["host_current_thread_control"]["summary"].as_str() {
            tooltip.push_str(&format!(
                "\n- Ближайший same-thread host surface: {summary}"
            ));
        }
        if let Some(summary) =
            bundle["host_current_thread_control"]["external_uri_launch"]["summary"]
                .as_str()
                .filter(|_| {
                    bundle["host_current_thread_control"]["external_uri_launch"]["available"]
                        .as_bool()
                        == Some(true)
                })
        {
            tooltip.push_str(&format!("\n- Best-effort external launch: {summary}"));
        }
        if let Some(command_id) = bundle["host_current_thread_control"]["command_id"].as_str() {
            tooltip.push_str(&format!("\n- Host internal command id: {command_id}"));
        }
        if let Some(uri) =
            bundle["host_current_thread_control"]["external_uri_launch"]["uri"].as_str()
        {
            tooltip.push_str(&format!("\n- VS Code URI launch: {uri}"));
        }
        if let Some(command) =
            bundle["host_current_thread_control"]["external_uri_launch"]["platform_launch_command"]
                .as_str()
        {
            tooltip.push_str(&format!("\n- Shell launch: {command}"));
        }
        if let Some(note) = bundle["host_current_thread_control"]["note"].as_str() {
            tooltip.push_str(&format!("\n- Ограничение: {note}"));
        }
        if let Some(command) = bundle["operator_flow"]["rotate_helper_command"].as_str() {
            tooltip.push_str(&format!("\n- Fallback rotate helper: {command}"));
        }
        if let Some(command) = bundle["operator_flow"]["handoff_command"].as_str() {
            tooltip.push_str(&format!("\n- Готовая команда handoff: {command}"));
        }
        if let Some(command) = bundle["operator_flow"]["startup_command"].as_str() {
            tooltip.push_str(&format!(
                "\n- Если fallback всё-таки понадобится, после новой чистой рабочей поверхности запусти startup: {command}"
            ));
        }
    }
    tooltip
}

pub(super) fn client_live_meter_note_sentence(
    client_live_meter: &Value,
    exact_pair: Option<(u64, u64, i64, f64)>,
) -> Option<String> {
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .unwrap_or(0);
    let model_context_window = client_live_meter["latest_model_context_window"]
        .as_u64()
        .unwrap_or(0);
    let context_remaining_percent = client_live_meter["context_used_percent"]
        .as_f64()
        .map(|value| 100.0 - value)
        .unwrap_or_default();
    let Some((_, _, saved_tokens, _)) = exact_pair else {
        return Some(format!(
            "Текущий client live meter уже виден напрямую из rollout: последний observed запрос клиента занимает {} из {}, остаётся {}. Live pressure по 5ч и 7д лимитам вынесен в отдельную строку. Exact full-turn delta по Amai здесь ещё нельзя честно посчитать, пока same-meter pair не materialized.",
            format_u64(Some(turn_total_tokens)),
            format_u64(Some(model_context_window)),
            format_percent(Some(context_remaining_percent)),
        ));
    };
    let without_amai_total_tokens = if saved_tokens >= 0 {
        turn_total_tokens.saturating_add(saved_tokens as u64)
    } else {
        turn_total_tokens.saturating_sub(saved_tokens.unsigned_abs())
    };
    let full_turn_savings_pct = if without_amai_total_tokens == 0 {
        0.0
    } else {
        (saved_tokens as f64 * 100.0) / without_amai_total_tokens as f64
    };
    Some(format!(
        "В полном live-turn клиента Amai сейчас даёт {}: без Amai было {}, с Amai стало {}. Последний observed запрос клиента сейчас занимает {} из {} окна, остаётся {}. Live pressure по 5ч и 7д лимитам вынесен в отдельную строку. Значит внешний burn сейчас определяется не только Amai-delta, а и общим размером текущего запроса внутри клиентского окна.",
        format_percent(Some(full_turn_savings_pct)),
        format_u64(Some(without_amai_total_tokens)),
        format_u64(Some(turn_total_tokens)),
        format_u64(Some(turn_total_tokens)),
        format_u64(Some(model_context_window)),
        format_percent(Some(context_remaining_percent)),
    ))
}

pub(super) fn client_live_context_metric_row(client_live_meter: &Value) -> Option<Value> {
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .unwrap_or(0);
    let model_context_window = client_live_meter["latest_model_context_window"]
        .as_u64()
        .unwrap_or(0);
    let context_remaining_percent = client_live_meter["context_used_percent"]
        .as_f64()
        .map(|value| 100.0 - value)
        .unwrap_or_default();
    let observed_at = client_live_meter["ended_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp);
    let observed_at_short = client_live_meter["ended_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp_clock);
    let tooltip = format!(
        "Этот ряд показывает последний observed запрос клиента из rollout token_count.\n- Последний запрос: {} из {}\n- Остаётся в окне: {}\n- Источник: rollout token_count.last_token_usage.total_tokens / model_context_window{}\n- Снято из raw token_count: {}",
        format_u64(Some(turn_total_tokens)),
        format_u64(Some(model_context_window)),
        format_percent(Some(context_remaining_percent)),
        observed_at_short
            .as_ref()
            .map(|stamp| format!(" ({stamp})"))
            .unwrap_or_default(),
        observed_at
            .clone()
            .unwrap_or_else(|| "ещё нет данных".to_string()),
    );
    Some(metric_row_with_key(
        CLIENT_LIVE_CONTEXT_ROW_KEY,
        "Последний запрос клиента",
        format!(
            "{} из {}, остаётся {}{}",
            format_u64(Some(turn_total_tokens)),
            format_u64(Some(model_context_window)),
            format_percent(Some(context_remaining_percent)),
            observed_at_short
                .as_ref()
                .map(|stamp| format!(" · raw {stamp}"))
                .unwrap_or_default()
        ),
        Some(tooltip.as_str()),
    ))
}

pub(super) fn client_live_limit_metric_row(client_live_meter: &Value) -> Option<Value> {
    let limit_surface = preferred_client_limit_meter_surface(client_live_meter)?;
    let exact_source = preferred_client_limit_meter_is_exact(client_live_meter);
    let rollout_observed = client_live_meter_is_observed(client_live_meter);
    let current_thread_bound =
        rollout_observed && client_live_meter_current_thread_bound(client_live_meter);
    let primary_remaining_percent = client_limit_remaining_percent(
        limit_surface,
        "primary_limit_remaining_percent",
        "primary_limit_used_percent",
    );
    let secondary_remaining_percent = client_limit_remaining_percent(
        limit_surface,
        "secondary_limit_remaining_percent",
        "secondary_limit_used_percent",
    );
    let observed_at = preferred_client_limit_observed_at_epoch_ms(client_live_meter)
        .filter(|value| *value > 0)
        .map(human_timestamp);
    let observed_at_short = preferred_client_limit_observed_at_epoch_ms(client_live_meter)
        .filter(|value| *value > 0)
        .map(human_timestamp_clock);
    let (label, tooltip, value) = if exact_source {
        let thread_context_note = if current_thread_bound {
            "Параллельно есть current-thread-bound rollout meter для строки `Последний запрос клиента`, поэтому 5ч/7д лимиты и размер текущего запроса читаются из двух независимых truth-source."
        } else if rollout_observed {
            "Current thread binding для rollout meter ещё не materialized, поэтому этот ряд остаётся live global client-limit source, а не pressure текущего thread."
        } else {
            "Rollout meter для текущего thread пока не materialized, поэтому этот ряд остаётся единственным честным live source для 5ч/7д лимита клиента."
        };
        (
            "Лимит клиента сейчас",
            format!(
                "Этот ряд показывает live rate-limit contour клиента из codex app-server account/rateLimits/read, тем же upstream path, что использует VS Code status bar.\n- Лимит 5ч: остаётся {} (использовано {})\n- Лимит 7д: остаётся {} (использовано {})\n- Снято из upstream: {}\n- {}",
                format_percent(Some(primary_remaining_percent)),
                format_percent(
                    limit_surface["primary_limit_used_percent"]
                        .as_f64()
                        .or_else(|| limit_surface["primary_limit_used_percent"]
                            .as_u64()
                            .map(|value| value as f64))
                ),
                format_percent(Some(secondary_remaining_percent)),
                format_percent(
                    limit_surface["secondary_limit_used_percent"]
                        .as_f64()
                        .or_else(|| limit_surface["secondary_limit_used_percent"]
                            .as_u64()
                            .map(|value| value as f64))
                ),
                observed_at
                    .clone()
                    .unwrap_or_else(|| "ещё нет данных".to_string()),
                thread_context_note,
            ),
            format!(
                "5ч остаётся {}, 7д остаётся {}{}",
                format_percent(Some(primary_remaining_percent)),
                format_percent(Some(secondary_remaining_percent)),
                observed_at_short
                    .as_ref()
                    .map(|stamp| format!(" · live {stamp}"))
                    .unwrap_or_default()
            ),
        )
    } else if current_thread_bound {
        (
            "Лимит клиента сейчас",
            format!(
                "Этот ряд показывает live rate-limit contour клиента из rollout token_count/rate_limits.\n- Лимит 5ч: остаётся {} (использовано {})\n- Лимит 7д: остаётся {} (использовано {})\n- Снято из raw token_count: {}",
                format_percent(Some(primary_remaining_percent)),
                format_percent(client_live_meter["primary_limit_used_percent"].as_f64()),
                format_percent(Some(secondary_remaining_percent)),
                format_percent(client_live_meter["secondary_limit_used_percent"].as_f64()),
                observed_at.unwrap_or_else(|| "ещё нет данных".to_string()),
            ),
            format!(
                "5ч остаётся {}, 7д остаётся {}{}",
                format_percent(Some(primary_remaining_percent)),
                format_percent(Some(secondary_remaining_percent)),
                observed_at_short
                    .as_ref()
                    .map(|stamp| format!(" · raw {stamp}"))
                    .unwrap_or_default()
            ),
        )
    } else {
        (
            "Последний observed лимит клиента",
            format!(
                "Этот ряд показывает последнее observed значение клиентского rate-limit contour из rollout token_count/rate_limits.\n- Current thread binding ещё не materialized, поэтому это global client limit hint, а не pressure текущего thread.\n- Лимит 5ч: остаётся {} (использовано {})\n- Лимит 7д: остаётся {} (использовано {})\n- Последнее observed значение снято из raw token_count: {}",
                format_percent(Some(primary_remaining_percent)),
                format_percent(client_live_meter["primary_limit_used_percent"].as_f64()),
                format_percent(Some(secondary_remaining_percent)),
                format_percent(client_live_meter["secondary_limit_used_percent"].as_f64()),
                observed_at.unwrap_or_else(|| "ещё нет данных".to_string()),
            ),
            format!(
                "последнее observed: 5ч остаётся {}, 7д остаётся {}{}",
                format_percent(Some(primary_remaining_percent)),
                format_percent(Some(secondary_remaining_percent)),
                observed_at_short
                    .as_ref()
                    .map(|stamp| format!(" · latest observed {stamp}"))
                    .unwrap_or_default()
            ),
        )
    };
    Some(metric_row_with_key(
        CLIENT_LIVE_LIMIT_ROW_KEY,
        label,
        value,
        Some(tooltip.as_str()),
    ))
}

fn compact_chat_selector_repo_root(restore_context: &Value) -> Option<PathBuf> {
    restore_context["project"]["repo_root"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| config::discover_repo_root(None).ok())
}

pub(super) fn compact_chat_selector_client_surface(restore_context: &Value) -> Value {
    let repo_root = compact_chat_selector_repo_root(restore_context);
    if let Some(repo_root) = repo_root {
        onboarding::describe_client_surface(repo_root.as_path(), None).unwrap_or_else(|_| {
            json!({
                "client_key": "unknown",
                "display_name": "Unknown client",
                "startup_instruction_path": Value::Null,
                "startup_instruction_mode": Value::Null,
                "reconnect_shell_command": Value::Null,
                "reconnect_bootstrap_command": Value::Null,
                "fresh_chat_assist_summary": Value::Null,
                "delivery_surface_assist_summary": Value::Null,
            })
        })
    } else {
        json!({
            "client_key": "unknown",
            "display_name": "Unknown client",
            "startup_instruction_path": Value::Null,
            "startup_instruction_mode": Value::Null,
            "reconnect_shell_command": Value::Null,
            "reconnect_bootstrap_command": Value::Null,
            "fresh_chat_assist_summary": Value::Null,
            "delivery_surface_assist_summary": Value::Null,
        })
    }
}

fn compact_chat_selector_prompt_path(restore_context: &Value) -> Option<PathBuf> {
    let repo_root = compact_chat_selector_repo_root(restore_context)?;
    let prompt_path = repo_root.join(".amai/continuity/compact-chat-prompt.txt");
    if prompt_path.is_file() {
        Some(prompt_path)
    } else {
        None
    }
}

fn compact_chat_selector_prompt_file(restore_context: &Value) -> Option<String> {
    compact_chat_selector_prompt_path(restore_context).map(|path| path.display().to_string())
}

fn compact_chat_selector_clean_launch_surface(
    restore_context: &Value,
    client_surface: &Value,
) -> Value {
    let Some(repo_root) = compact_chat_selector_repo_root(restore_context) else {
        return json!({
            "status": "bridge_unavailable",
            "supported_auto_launch": false,
            "command_kind": Value::Null,
            "unavailable_reason": "repo_root_unavailable",
            "ux_verdict": "not_seamless_until_live_client_proof",
        });
    };
    crate::continuity::compact_chat_clean_launch_surface(
        client_surface,
        repo_root.as_path(),
        compact_chat_selector_prompt_path(restore_context).as_deref(),
    )
}

fn compact_chat_selector_manual_note(client_surface: &Value) -> Option<String> {
    let mut note = "Если automatic clean-surface launch недоступен, открой новую чистую рабочую поверхность и вставь prompt_text вручную.".to_string();
    if let Some(display_name) = client_surface["display_name"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        note.push_str(&format!(" Клиент: {display_name}."));
    }
    if let Some(summary) = client_surface["delivery_surface_assist_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            client_surface["fresh_chat_assist_summary"]
                .as_str()
                .filter(|value| !value.is_empty())
        })
    {
        note.push(' ');
        note.push_str(summary);
    }
    if let Some(path) = client_surface["startup_instruction_path"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        let mode = client_surface["startup_instruction_mode"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown");
        note.push_str(&format!(" Startup surface: {path} ({mode})."));
    }
    Some(note)
}

pub(super) fn client_limit_hourly_burn_metric_row(
    hourly_burn: &Value,
    client_budget_target_percent: u64,
    restore_context: &Value,
    client_live_meter: &Value,
    host_context_compaction: &Value,
    host_current_thread_control: &Value,
) -> Option<Value> {
    let status = hourly_burn["status"].as_str().unwrap_or("missing");
    let label = "KPI 5ч лимита";
    match status {
        "observed" => {
            let projected = hourly_burn["projected_primary_used_per_hour_percent"].as_f64();
            let kpi_percent = hourly_burn["kpi_percent"].as_f64();
            let classification = hourly_burn["classification"].as_str().unwrap_or("unknown");
            let reply_prefix = hourly_burn["reply_prefix"]
                .as_str()
                .unwrap_or("5ч KPI: н/д");
            let kpi_value_text = if classification == "aligned" {
                reply_prefix
                    .strip_prefix("5ч KPI: ")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("1:1")
                    .to_string()
            } else {
                format_percent(kpi_percent)
            };
            let selector_value_parts = match classification {
                "saving" => Some((
                    "5ч KPI: экономия ".to_string(),
                    format!(" · tempo {}", format_percent(projected)),
                )),
                "overspend" => Some((
                    "5ч KPI: переплата ".to_string(),
                    format!(" · tempo {}", format_percent(projected)),
                )),
                "aligned" => Some((
                    "5ч KPI: ".to_string(),
                    format!(" · tempo {}", format_percent(projected)),
                )),
                _ => None,
            };
            let remaining_window_minutes = hourly_burn["remaining_window_minutes"]
                .as_f64()
                .unwrap_or(0.0);
            let observed_at = hourly_burn["latest_observed_at_epoch_ms"]
                .as_u64()
                .filter(|value| *value > 0)
                .map(human_timestamp_clock)
                .unwrap_or_else(|| "ещё нет данных".to_string());
            let selected_host_current_thread_control_command_id =
                host_current_thread_control["command_id"].as_str();
            let last_host_feedback_kind =
                latest_host_current_thread_control_feedback_kind_for_command(
                    restore_context,
                    selected_host_current_thread_control_command_id,
                );
            let last_host_feedback_summary =
                latest_host_current_thread_control_feedback_summary_for_command(
                    restore_context,
                    selected_host_current_thread_control_command_id,
                );
            let host_current_thread_control_effect =
                host_current_thread_control_effect_payload_for_command(
                    restore_context,
                    client_live_meter,
                    host_context_compaction,
                    selected_host_current_thread_control_command_id,
                );
            let host_feedback_pending = host_current_thread_control_feedback_pending_from_effect(
                last_host_feedback_kind,
                &host_current_thread_control_effect,
            );
            let host_current_thread_control = decorate_host_current_thread_control_surface(
                host_current_thread_control,
                &host_current_thread_control_effect,
                host_feedback_pending,
                last_host_feedback_summary.as_deref(),
            );
            let host_current_thread_control_button_label =
                host_current_thread_control["button_label"]
                    .as_str()
                    .unwrap_or("Same-thread control");
            let host_current_thread_control_intro = host_current_thread_control["intro_message"]
                .as_str()
                .unwrap_or("Открыть ближайший same-thread control surface текущего giant thread.");
            let host_current_thread_control_notice_message =
                host_current_thread_control["requested_message_text"]
                    .as_str()
                    .unwrap_or(
                        "Запрошен ближайший same-thread host control текущего giant thread.",
                    );
            let host_current_thread_control_ack_intro =
                host_current_thread_control["feedback_ack_intro"]
                    .as_str()
                    .unwrap_or("После попытки запуска отметь исход same-thread host control.");
            let compact_chat_client_surface = compact_chat_selector_client_surface(restore_context);
            let compact_chat_prompt_file = compact_chat_selector_prompt_file(restore_context);
            let compact_chat_prompt_file_value = compact_chat_prompt_file
                .as_deref()
                .map(Value::from)
                .unwrap_or(Value::Null);
            let actual_remaining_percent = hourly_burn["actual_remaining_percent"].as_f64();
            let ideal_remaining_percent = hourly_burn["ideal_remaining_percent"].as_f64();
            let projected_reset_delta_minutes =
                hourly_burn["projected_reset_delta_minutes"].as_f64();
            let verdict = match classification {
                "overspend" => format!(
                    "Переплата к идеальному окну: {}.",
                    format_percent(kpi_percent)
                ),
                "saving" => format!(
                    "Экономия к идеальному окну: {}.",
                    format_percent(kpi_percent)
                ),
                _ => "Идёт почти 1:1 к идеальному окну 5ч.".to_string(),
            };
            let reset_delta = projected_reset_delta_minutes
                .map(|value| {
                    if value < -0.5 {
                        format!(
                            "При таком темпе лимит закончится примерно на {:.2} мин раньше сброса.",
                            value.abs()
                        )
                    } else if value > 0.5 {
                        format!(
                            "При таком темпе к сбросу останется запас примерно {:.2} мин окна.",
                            value
                        )
                    } else {
                        "При таком темпе лимит идёт почти ровно к моменту сброса.".to_string()
                    }
                })
                .unwrap_or_else(|| {
                    "Точное смещение к моменту сброса пока не вычислено.".to_string()
                });
            let tooltip = format!(
                "Этот ряд считает KPI 5ч лимита по тому же upstream source, что и VS Code status bar.\n- Снято из upstream: {}\n- До сброса остаётся {:.2} мин окна\n- Реально остаётся лимита: {}\n- Идеально к этому моменту должно оставаться: {}\n- Текущий темп burn: {}\n- {}\n- {}\n- Это и есть источник для короткого reply-prefix `{}'`.",
                observed_at,
                remaining_window_minutes,
                format_percent(actual_remaining_percent),
                format_percent(ideal_remaining_percent),
                format_percent(projected),
                verdict,
                reset_delta,
                reply_prefix,
            );
            let mut row = metric_row_with_key(
                CLIENT_LIMIT_HOURLY_BURN_ROW_KEY,
                label,
                format!("{} · tempo {}", reply_prefix, format_percent(projected)),
                Some(tooltip.as_str()),
            );
            if let Some(root) = row.as_object_mut() {
                if let Some((value_prefix, value_suffix)) = selector_value_parts {
                    let compact_chat_clean_launch = compact_chat_selector_clean_launch_surface(
                        restore_context,
                        &compact_chat_client_surface,
                    );
                    root.insert(
                        "target_selector".to_string(),
                        json!({
                            "current_target_percent": client_budget_target_percent,
                            "allowed_target_percents": allowed_client_budget_target_values(),
                            "selected_chat_command": client_budget_target_chat_command(client_budget_target_percent),
                            "chat_command_prefix": "экономия_",
                            "reply_prefix": reply_prefix,
                            "kpi_value_text": kpi_value_text,
                            "value_prefix": value_prefix,
                            "value_suffix": value_suffix,
                            "tooltip_intro": format!(
                                "Целевой режим клиентской экономии сейчас = {}%. Можно переключить его прямо отсюда.",
                                client_budget_target_percent
                            ),
                            "compact_chat_command": client_budget_compact_chat_command(),
                            "compact_chat_button_label": "Compact chat",
                            "compact_chat_intro": "Подготовить startup restore пакет для переноса рабочей линии на новую clean work surface.",
                            "compact_chat_required_host_action":
                                "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                            "compact_chat_prompt_file": compact_chat_prompt_file_value,
                            "compact_chat_note":
                                compact_chat_selector_manual_note(&compact_chat_client_surface),
                            "compact_chat_client_surface": compact_chat_client_surface.clone(),
                            "compact_chat_client_display_name":
                                compact_chat_client_surface["display_name"].clone(),
                            "compact_chat_assist_summary":
                                compact_chat_client_surface["fresh_chat_assist_summary"].clone(),
                            "compact_chat_delivery_surface_assist_summary":
                                compact_chat_client_surface["delivery_surface_assist_summary"].clone(),
                            "compact_chat_manual_fallback_steps":
                                crate::continuity::compact_chat_manual_fallback_steps(
                                    &compact_chat_client_surface,
                                ),
                            "compact_chat_launch_status":
                                compact_chat_clean_launch["status"].clone(),
                            "compact_chat_launch_supported_auto":
                                compact_chat_clean_launch["supported_auto_launch"].clone(),
                            "compact_chat_launch_command_kind":
                                compact_chat_clean_launch["command_kind"].clone(),
                            "compact_chat_launch_command":
                                compact_chat_clean_launch["launch_clean_chat_command"].clone(),
                            "compact_chat_launch_fallback_command":
                                compact_chat_clean_launch["launch_clean_chat_fallback_command"].clone(),
                            "compact_chat_launch_unavailable_reason":
                                compact_chat_clean_launch["unavailable_reason"].clone(),
                            "compact_chat_launch_ux_verdict":
                                compact_chat_clean_launch["ux_verdict"].clone(),
                            "compact_chat_startup_instruction_path":
                                compact_chat_client_surface["startup_instruction_path"].clone(),
                            "compact_chat_startup_instruction_mode":
                                compact_chat_client_surface["startup_instruction_mode"].clone(),
                            "compact_chat_reconnect_shell_command":
                                compact_chat_client_surface["reconnect_shell_command"].clone(),
                            "compact_chat_reconnect_bootstrap_command":
                                compact_chat_client_surface["reconnect_bootstrap_command"].clone(),
                            "host_current_thread_control": host_current_thread_control.clone(),
                            "host_current_thread_control_button_label":
                                host_current_thread_control_button_label,
                            "host_current_thread_control_intro": host_current_thread_control_intro,
                            "host_current_thread_control_notice_message":
                                host_current_thread_control_notice_message,
                            "host_current_thread_control_feedback_pending": host_feedback_pending,
                            "host_current_thread_control_measurement_pending":
                                host_current_thread_control["measurement_pending"].clone(),
                            "host_current_thread_control_retry_allowed":
                                host_current_thread_control["retry_allowed"].clone(),
                            "host_current_thread_control_retry_blocked_reason":
                                host_current_thread_control["retry_blocked_reason"].clone(),
                            "host_current_thread_control_last_feedback_kind": last_host_feedback_kind,
                            "host_current_thread_control_last_feedback_summary": last_host_feedback_summary,
                            "host_current_thread_control_effect": host_current_thread_control_effect.clone(),
                            "host_current_thread_control_effect_summary":
                                host_current_thread_control_effect["summary"].clone(),
                            "host_current_thread_control_effect_note":
                                host_current_thread_control_effect["note"].clone(),
                            "host_current_thread_control_ack_intro":
                                host_current_thread_control_ack_intro,
                        }),
                    );
                }
            }
            Some(row)
        }
        "stale" => Some(metric_row_with_key(
            CLIENT_LIMIT_HOURLY_BURN_ROW_KEY,
            label,
            "exact KPI устарел".to_string(),
            Some("Последний exact sample 5ч лимита устарел, поэтому KPI fail-closed не считается."),
        )),
        "missing_reset" => Some(metric_row_with_key(
            CLIENT_LIMIT_HOURLY_BURN_ROW_KEY,
            label,
            "нет reset time".to_string(),
            Some(
                "Exact source не дал reset time для 5ч окна, поэтому KPI fail-closed не считается.",
            ),
        )),
        _ => Some(metric_row_with_key(
            CLIENT_LIMIT_HOURLY_BURN_ROW_KEY,
            label,
            "exact KPI ещё нет".to_string(),
            Some("Exact source 5ч лимита ещё не materialized, поэтому KPI пока не посчитан."),
        )),
    }
}

pub(crate) fn client_budget_live_payload(snapshot: &Value) -> Value {
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let restore_context = &snapshot["latest_repo_working_state_restore"]["working_state_restore"];
    let client_budget_target_percent =
        client_budget_target_percent_from_inputs(report, restore_context);
    let nested_client_live_meter = &report["client_live_meter"];
    let client_live_meter = if nested_client_live_meter.is_object() {
        nested_client_live_meter
    } else {
        &snapshot["token_budget_report"]["client_live_meter"]
    };
    let nested_current_live_turn = &report["current_live_turn"];
    let current_live_turn = if nested_current_live_turn.is_object() {
        nested_current_live_turn
    } else {
        &snapshot["token_budget_report"]["current_live_turn"]
    };
    let nested_hourly_burn = &report["client_limit_hourly_burn"];
    let client_limit_hourly_burn = if nested_hourly_burn.is_object() {
        nested_hourly_burn
    } else {
        &snapshot["token_budget_report"]["client_limit_hourly_burn"]
    };
    let host_context_compaction = latest_host_context_compaction_payload(report, restore_context);
    let (host_current_thread_control, host_current_thread_control_effect, _) =
        selected_host_current_thread_control_state(
            report,
            restore_context,
            client_live_meter,
            &host_context_compaction,
        );
    let selected_host_current_thread_control_command_id =
        host_current_thread_control["command_id"].as_str();
    let host_feedback_kind = latest_host_current_thread_control_feedback_kind_for_command(
        restore_context,
        selected_host_current_thread_control_command_id,
    );
    let host_feedback_summary = latest_host_current_thread_control_feedback_summary_for_command(
        restore_context,
        selected_host_current_thread_control_command_id,
    );
    let host_feedback_pending = host_current_thread_control_feedback_pending_from_effect(
        host_feedback_kind,
        &host_current_thread_control_effect,
    );
    let host_current_thread_control = decorate_host_current_thread_control_surface(
        &host_current_thread_control,
        &host_current_thread_control_effect,
        host_feedback_pending,
        host_feedback_summary.as_deref(),
    );
    let mut rows = Vec::new();
    if let Some(row) = client_full_turn_savings_metric_row(
        client_live_meter,
        current_live_turn_exact_pair(current_live_turn),
    ) {
        rows.push(row);
    }
    if let Some(row) = client_live_context_metric_row(client_live_meter) {
        rows.push(row);
    }
    if let Some(row) = client_live_limit_metric_row(client_live_meter) {
        rows.push(row);
    }
    if let Some(row) = client_limit_hourly_burn_metric_row(
        client_limit_hourly_burn,
        client_budget_target_percent,
        restore_context,
        client_live_meter,
        &host_context_compaction,
        &host_current_thread_control,
    ) {
        rows.push(row);
    }
    let live_status = if current_session_client_live_meter_available(client_live_meter)
        || preferred_client_limit_meter_surface(client_live_meter).is_some()
    {
        "observed"
    } else {
        client_live_meter["status"].as_str().unwrap_or("missing")
    };
    let (reply_prefix, global_reply_prefix, reply_prefix_source) =
        current_agent_reply_prefix_fields(snapshot, report);
    json!({
        "status": live_status,
        "client_budget_target_percent": client_budget_target_percent,
        "thread_binding_state": client_live_meter["thread_binding_state"].clone(),
        "current_thread_bound": client_live_meter["current_thread_bound"].clone(),
        "ended_at_epoch_ms": preferred_client_limit_observed_at_epoch_ms(client_live_meter)
            .map(Value::from)
            .unwrap_or_else(|| client_live_meter["ended_at_epoch_ms"].clone()),
        "reply_prefix": reply_prefix,
        "global_reply_prefix": global_reply_prefix,
        "reply_prefix_source": Value::from(reply_prefix_source),
        "rows": rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::working_state;
    use serde_json::{Value, json};

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
        let row = client_live_limit_metric_row(&meter).expect("limit row");
        assert_eq!(row["key"].as_str(), Some(CLIENT_LIVE_LIMIT_ROW_KEY));
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
        let row = client_live_limit_metric_row(&meter).expect("limit row");
        assert_eq!(row["key"].as_str(), Some(CLIENT_LIVE_LIMIT_ROW_KEY));
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
        let row = client_live_context_metric_row(&meter).expect("context row");
        assert_eq!(row["key"].as_str(), Some(CLIENT_LIVE_CONTEXT_ROW_KEY));
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
            client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn).is_none()
        );
        assert!(client_live_context_metric_row(&meter).is_none());
        let row = client_live_limit_metric_row(&meter).expect("limit row");
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
        let payload = client_budget_live_payload(&snapshot);
        let rows = payload["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0]["key"].as_str(),
            Some(CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
        );
        assert_eq!(rows[1]["key"].as_str(), Some(CLIENT_LIVE_LIMIT_ROW_KEY));
        assert_eq!(
            rows[2]["key"].as_str(),
            Some(CLIENT_LIMIT_HOURLY_BURN_ROW_KEY)
        );
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
        let payload = client_budget_live_payload(&snapshot);
        let rows = payload["rows"].as_array().expect("rows array");
        assert_eq!(payload["status"], json!("observed"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["key"].as_str(), Some(CLIENT_LIVE_LIMIT_ROW_KEY));
        assert_eq!(rows[0]["label"].as_str(), Some("Лимит клиента сейчас"));
        assert!(
            rows[0]["value"]
                .as_str()
                .unwrap_or_default()
                .contains("5ч остаётся 61.00%, 7д остаётся 58.00%")
        );
        assert_eq!(
            rows[1]["key"].as_str(),
            Some(CLIENT_LIMIT_HOURLY_BURN_ROW_KEY)
        );
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
        let payload = client_budget_live_payload(&snapshot);
        let rows = payload["rows"].as_array().expect("rows array");
        assert_eq!(
            rows[0]["key"].as_str(),
            Some(CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
        );
        assert!(
            rows[0]["value"]
                .as_str()
                .unwrap_or_default()
                .contains("0.00%")
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

        let payload = client_budget_live_payload(&snapshot);
        assert_eq!(
            payload["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 50.00%")
        );
        let rows = payload["rows"].as_array().expect("rows");
        assert!(
            rows.iter()
                .any(|row| { row["key"].as_str() == Some(CLIENT_LIMIT_HOURLY_BURN_ROW_KEY) })
        );
        let hourly_row = rows
            .iter()
            .find(|row| row["key"].as_str() == Some(CLIENT_LIMIT_HOURLY_BURN_ROW_KEY))
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

        let payload = client_budget_live_payload(&snapshot);
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
        assert_eq!(
            super::delivery_surface_status_label(guard.status_label),
            "новая чистая рабочая поверхность нужна сейчас"
        );
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
        assert!(tooltip.contains("новая чистая рабочая поверхность"));
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
            Some((90106, 90000, 106, 0.12)),
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
            "client_turn_total_tokens": 74000,
            "latest_model_context_window": 258400,
            "context_used_percent": 28.64,
            "primary_limit_remaining_percent": 82.0,
            "secondary_limit_remaining_percent": 95.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((74072, 74000, 72, 0.1)),
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
            "client_turn_total_tokens": 118116,
            "latest_model_context_window": 258400,
            "context_used_percent": 45.71,
            "primary_limit_remaining_percent": 1.0,
            "secondary_limit_remaining_percent": 88.0
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
            "kpi_percent": 2100.0,
            "reply_prefix": "5ч KPI: переплата 2100.00%"
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 172361,
            "latest_model_context_window": 258400,
            "context_used_percent": 66.7,
            "primary_limit_remaining_percent": 86.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((172361, 172361, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_when_5h_kpi_overspends_with_weak_live_gain() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 180.0,
            "reply_prefix": "5ч KPI: переплата 180.00%"
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 175432,
                "with_amai_tokens": 172361,
                "saved_tokens": 3071,
                "saved_pct": 1.750
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 172361,
            "latest_model_context_window": 258400,
            "context_used_percent": 66.7,
            "primary_limit_remaining_percent": 86.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((175432, 172361, 3071, 1.75)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_overspend_large_thread_even_with_fresh_budget() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 110.0,
            "reply_prefix": "5ч KPI: переплата 110.00%"
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 206720,
                "with_amai_tokens": 206274,
                "saved_tokens": 446,
                "saved_pct": 0.22
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 206274,
            "latest_model_context_window": 258400,
            "context_used_percent": 79.83,
            "primary_limit_remaining_percent": 92.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((206720, 206274, 446, 0.22)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_huge_overspend_thread_before_primary_limit_softens()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 205.0,
            "reply_prefix": "5ч KPI: переплата 205.00%"
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 206720,
                "with_amai_tokens": 206274,
                "saved_tokens": 446,
                "saved_pct": 0.22
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 245000,
            "latest_model_context_window": 258400,
            "context_used_percent": 94.8,
            "primary_limit_remaining_percent": 92.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((245630, 245000, 630, 0.25)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_huge_no_amai_thread_without_hourly_burn_surface()
    {
        let hourly_burn = json!({
            "status": "missing"
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 245000,
            "latest_model_context_window": 258400,
            "context_used_percent": 94.8,
            "primary_limit_remaining_percent": 92.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((245000, 245000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_large_no_amai_thread_without_hourly_burn_surface()
     {
        let hourly_burn = json!({
            "status": "missing"
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 161000,
            "latest_model_context_window": 258400,
            "context_used_percent": 62.3,
            "primary_limit_remaining_percent": 91.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((161000, 161000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_huge_no_amai_thread_when_exact_5h_kpi_is_below_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 40.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 240000,
            "latest_model_context_window": 258400,
            "context_used_percent": 92.88,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((240000, 240000, 0, 0.0)),
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
            "kpi_percent": 40.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 206720,
                "with_amai_tokens": 206274,
                "saved_tokens": 446,
                "saved_pct": 0.22
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 206274,
            "latest_model_context_window": 258400,
            "context_used_percent": 79.83,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((206720, 206274, 446, 0.22)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_high_context_thread_when_exact_pair_is_far_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 95.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 206720,
                "with_amai_tokens": 206274,
                "saved_tokens": 446,
                "saved_pct": 0.22
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 206274,
            "latest_model_context_window": 258400,
            "context_used_percent": 79.83,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard(
                &meter,
                Some((206720, 206274, 446, 0.22)),
                &hourly_burn,
                &current_live_turn,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_high_context_thread_when_exact_pair_is_below_90_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 95.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 206720,
                "with_amai_tokens": 206274,
                "saved_tokens": 446,
                "saved_pct": 0.22
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 206274,
            "latest_model_context_window": 258400,
            "context_used_percent": 79.83,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((206720, 206274, 446, 0.22)),
                &hourly_burn,
                &current_live_turn,
                90,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_early_when_exact_pair_is_below_90_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 95.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 15000,
                "with_amai_tokens": 13500,
                "saved_tokens": 1500,
                "saved_pct": 10.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 13500,
            "latest_model_context_window": 258400,
            "context_used_percent": 5.22,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((15000, 13500, 1500, 10.0)),
                &hourly_burn,
                &current_live_turn,
                90,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_on_small_thread_when_exact_pair_and_5h_kpi_are_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 70.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 15000,
                "with_amai_tokens": 13500,
                "saved_tokens": 1500,
                "saved_pct": 10.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 13500,
            "latest_model_context_window": 258400,
            "context_used_percent": 5.22,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard_with_target(
            &meter,
            Some((15000, 13500, 1500, 10.0)),
            &hourly_burn,
            &current_live_turn,
            90,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_on_moderate_thread_when_exact_pair_and_5h_kpi_are_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 70.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 60000,
                "with_amai_tokens": 54000,
                "saved_tokens": 6000,
                "saved_pct": 10.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 54000,
            "latest_model_context_window": 258400,
            "context_used_percent": 20.9,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard_with_target(
            &meter,
            Some((60000, 54000, 6000, 10.0)),
            &hourly_burn,
            &current_live_turn,
            90,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
    }

    #[test]
    fn client_turn_pressure_guard_respects_custom_50_percent_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 60.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 60000,
                "with_amai_tokens": 54000,
                "saved_tokens": 6000,
                "saved_pct": 10.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 54000,
            "latest_model_context_window": 258400,
            "context_used_percent": 20.9,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((60000, 54000, 6000, 10.0)),
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
            "kpi_percent": 1.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 60000,
                "with_amai_tokens": 54000,
                "saved_tokens": 6000,
                "saved_pct": 10.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 54000,
            "latest_model_context_window": 258400,
            "context_used_percent": 20.9,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((60000, 54000, 6000, 10.0)),
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
            "kpi_percent": 40.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 46000,
            "latest_model_context_window": 258400,
            "context_used_percent": 17.8,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((46000, 46000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_stays_off_for_huge_no_amai_thread_when_exact_5h_kpi_is_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 80.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 245000,
            "latest_model_context_window": 258400,
            "context_used_percent": 94.8,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((245000, 245000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_moderate_no_amai_thread_when_5h_kpi_is_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 40.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 126000,
            "latest_model_context_window": 258400,
            "context_used_percent": 48.76,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((126000, 126000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_early_no_amai_thread_when_5h_kpi_is_below_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 40.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 68000,
            "latest_model_context_window": 258400,
            "context_used_percent": 26.32,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((68000, 68000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_huge_no_amai_thread_when_exact_5h_kpi_is_one_to_one()
    {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "one_to_one",
            "kpi_percent": 0.6
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn"
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 245000,
            "latest_model_context_window": 258400,
            "context_used_percent": 94.8,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((245000, 245000, 0, 0.0)),
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
            "kpi_percent": 95.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 120000,
                "with_amai_tokens": 108000,
                "saved_tokens": 12000,
                "saved_pct": 10.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 108000,
            "latest_model_context_window": 258400,
            "context_used_percent": 41.81,
            "primary_limit_remaining_percent": 2.0,
            "secondary_limit_remaining_percent": 95.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((120000, 108000, 12000, 10.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
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
        assert_eq!(
            row["target_selector"]["compact_chat_delivery_surface_assist_summary"],
            row["target_selector"]["compact_chat_assist_summary"]
        );
        assert!(
            row["target_selector"]["compact_chat_manual_fallback_steps"]
                .as_array()
                .and_then(|steps| steps.first())
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("новую чистую рабочую поверхность")
        );
        assert!(
            row["target_selector"]["compact_chat_launch_status"]
                .as_str()
                .is_some_and(|value| {
                    [
                        "manual_only",
                        "bridge_unavailable",
                        "launch_command_available",
                    ]
                    .contains(&value)
                })
        );
        assert_eq!(
            row["target_selector"]["compact_chat_launch_ux_verdict"],
            json!("not_seamless_until_live_client_proof")
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
