use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HistoricalStartupDrag {
    pub(super) older_without_amai_tokens: u64,
    pub(super) older_with_amai_tokens: u64,
    pub(super) older_delta_tokens: i64,
    pub(super) current_continuity_tokens: u64,
    pub(super) older_continuity_tokens: u64,
}

pub(super) fn savings_status(
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

pub(super) fn continuity_boundary_pressure(
    summary: &Value,
    alignment: &Value,
) -> Option<(u64, u64)> {
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

pub(super) fn continuity_boundary_pressure_sentence(
    boundary_tokens: u64,
    strict_tokens: u64,
) -> String {
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

pub(super) fn continuity_boundary_pressure_is_alert(
    saved_tokens: Option<i64>,
    boundary_tokens: u64,
    strict_tokens: u64,
) -> bool {
    saved_tokens.unwrap_or_default() <= 0
        && boundary_tokens >= strict_tokens.saturating_mul(4).max(256)
}

pub(super) fn recovery_sentence(median_recovery_tokens: Option<f64>) -> String {
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

pub(super) fn current_session_lane_rows(
    summary: &Value,
    exact_pair_materialized: bool,
) -> Vec<Value> {
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

pub(super) fn raw_savings_sentence(
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

pub(super) fn client_budget_disclaimer() -> &'static str {
    "Это не процент от лимита этого чата. Здесь считается только размер контекста, который Amai приносит в ответ, а не все токены разговора целиком."
}

pub(super) fn exact_model_token_pair(
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

pub(super) fn historical_startup_drag(
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

pub(super) fn historical_startup_drag_note_sentence(
    drag: Option<HistoricalStartupDrag>,
) -> Option<String> {
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

pub(super) fn historical_startup_drag_metric_row(
    drag: Option<HistoricalStartupDrag>,
) -> Option<Value> {
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

pub(super) fn exact_model_component_delta_metric_row(alignment: &Value) -> Option<Value> {
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

pub(super) fn exact_model_component_delta_note_sentence(alignment: &Value) -> Option<String> {
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

pub(super) fn client_live_meter_is_observed(client_live_meter: &Value) -> bool {
    client_live_meter["status"].as_str() == Some("observed")
}

fn exact_status_bar_rate_limits(client_live_meter: &Value) -> Option<&Value> {
    let exact = &client_live_meter["status_bar_rate_limits"];
    (exact["status"].as_str() == Some("observed")).then_some(exact)
}

pub(super) fn preferred_client_limit_meter_surface(client_live_meter: &Value) -> Option<&Value> {
    exact_status_bar_rate_limits(client_live_meter)
        .or_else(|| client_live_meter_is_observed(client_live_meter).then_some(client_live_meter))
}

pub(super) fn preferred_client_limit_meter_is_exact(client_live_meter: &Value) -> bool {
    exact_status_bar_rate_limits(client_live_meter).is_some()
}

pub(super) fn preferred_client_limit_observed_at_epoch_ms(
    client_live_meter: &Value,
) -> Option<u64> {
    preferred_client_limit_meter_surface(client_live_meter).and_then(|surface| {
        surface["observed_at_epoch_ms"]
            .as_u64()
            .or_else(|| surface["ended_at_epoch_ms"].as_u64())
    })
}

pub(super) fn client_limit_remaining_percent(
    surface: &Value,
    remaining_key: &str,
    used_key: &str,
) -> f64 {
    surface[remaining_key]
        .as_f64()
        .or_else(|| surface[remaining_key].as_u64().map(|value| value as f64))
        .or_else(|| surface[used_key].as_f64().map(|used| 100.0 - used))
        .or_else(|| surface[used_key].as_u64().map(|used| 100.0 - used as f64))
        .unwrap_or(100.0)
}

pub(super) fn client_live_meter_current_thread_bound(client_live_meter: &Value) -> bool {
    client_live_meter["current_thread_bound"]
        .as_bool()
        .unwrap_or_else(|| {
            client_live_meter["thread_binding_state"]
                .as_str()
                .map(|value| value == "current_thread_bound")
                .unwrap_or(true)
        })
}

pub(super) fn current_session_client_live_meter_available(client_live_meter: &Value) -> bool {
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

pub(super) fn current_live_turn_exact_pair(
    current_live_turn: &Value,
) -> Option<(u64, u64, i64, f64)> {
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

pub(super) fn live_turn_exact_pair(
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

pub(super) fn full_turn_savings_pct_from_live_meter(
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

pub(super) fn client_full_turn_savings_metric_row(
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

pub(super) fn exact_pair_primary_blocker_note_sentence(alignment: &Value) -> Option<String> {
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

pub(super) fn exact_pair_card_status_override(
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

pub(super) fn exact_pair_status_metric_row(alignment: &Value) -> Option<Value> {
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

pub(super) fn exact_pair_frozen_debt_metric_row(alignment: &Value) -> Option<Value> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            exact_pair_card_status_override(&alignment).expect("status override");
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
            exact_pair_card_status_override(&alignment).expect("status override");
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

        let row = exact_pair_status_metric_row(&alignment).expect("exact pair row");
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

        let row = exact_pair_status_metric_row(&alignment).expect("exact pair row");
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

        let row = exact_pair_frozen_debt_metric_row(&alignment).expect("frozen debt row");
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
    fn client_full_turn_savings_metric_row_surfaces_full_turn_share() {
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 35534
        });
        let row = client_full_turn_savings_metric_row(&meter, Some((550, 127, 423, 76.91)))
            .expect("full turn row");
        assert_eq!(
            row["key"].as_str(),
            Some(CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
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
        let row = client_full_turn_savings_metric_row(&meter, None).expect("full turn row");
        assert_eq!(
            row["key"].as_str(),
            Some(CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
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
        let row = client_full_turn_savings_metric_row(&meter, None).expect("full turn row");
        assert_eq!(
            row["key"].as_str(),
            Some(CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY)
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
            current_live_turn_exact_pair(&current_live_turn),
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
        let row = client_full_turn_savings_metric_row(&meter, Some((0, 0, 0, 0.0)))
            .expect("full turn row");
        assert!(row["value"].as_str().unwrap_or_default().contains("0.00%"));
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("delta 0")
        );
    }
}
