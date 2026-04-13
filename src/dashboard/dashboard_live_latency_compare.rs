use super::*;

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

pub(super) fn live_latency_compare_card(snapshot: &Value) -> Value {
    let hot = rolling_window_live_response_latency_slice(snapshot, "hot");
    let cold = rolling_window_live_response_latency_slice(snapshot, "cold");
    let current_hot = current_series_live_response_latency_slice(snapshot, "hot");
    let current_cold = current_series_live_response_latency_slice(snapshot, "cold");
    let live_response_latency_surface_materialized =
        token_budget_report_root(snapshot)["live_response_latency"].is_object();
    let has_unclassified_live_signal =
        live_response_latency_scope_has_unclassified_activity(snapshot, "current_session")
            || live_response_latency_scope_has_unclassified_activity(snapshot, "rolling_window");
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
    if overall_status == "unknown"
        && (hot_has_data
            || cold_has_data
            || current_series_has_data
            || has_unclassified_live_signal)
    {
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
            } else if has_unclassified_live_signal && !current_live_turn_no_amai_activity {
                format!(
                    "Последний живой ответ уже появился, но ещё не дал устойчивого разделения на повторный и новый запрос за последние {} минут, поэтому главным fallback остаётся накопительное окно 24 часов.",
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
    let table_rows = if live_response_latency_surface_materialized
        || current_series_has_data
        || current_live_turn_no_amai_activity
    {
        vec![
            live_latency_target_row(snapshot, "Повторный запрос", &hot_targets),
            live_latency_compare_row(
                snapshot,
                "Повторный запрос — текущая серия",
                "Свежая серия ответов Amai в текущем чате. Именно она определяет мгновенный operator signal.",
                current_hot,
                current_hot_sample_count,
            ),
            live_latency_compare_row(
                snapshot,
                "Повторный запрос — окно 24ч",
                "Накопительное живое окно задержки Amai за последние 24 часа. Оно не сбрасывается на новый чат и нужно для тренда.",
                hot,
                hot_sample_count,
            ),
            live_latency_target_row(snapshot, "Новый запрос", &cold_targets),
            live_latency_compare_row(
                snapshot,
                "Новый запрос — текущая серия",
                "Свежая серия ответов Amai в текущем чате. Именно она определяет мгновенный operator signal.",
                current_cold,
                current_cold_sample_count,
            ),
            live_latency_compare_row(
                snapshot,
                "Новый запрос — окно 24ч",
                "Накопительное живое окно задержки Amai за последние 24 часа. Оно не сбрасывается на новый чат и нужно для тренда.",
                cold,
                cold_sample_count,
            ),
        ]
    } else {
        vec![
            live_latency_target_row(snapshot, "Повторный запрос", &hot_targets),
            live_latency_compare_row(
                snapshot,
                "Повторный запрос — окно 24ч",
                "Накопительное живое окно задержки Amai за последние 24 часа. Оно не сбрасывается на новый чат и нужно для тренда.",
                hot,
                hot_sample_count,
            ),
            live_latency_target_row(snapshot, "Новый запрос", &cold_targets),
            live_latency_compare_row(
                snapshot,
                "Новый запрос — окно 24ч",
                "Накопительное живое окно задержки Amai за последние 24 часа. Оно не сбрасывается на новый чат и нужно для тренда.",
                cold,
                cold_sample_count,
            ),
        ]
    };
    let mut card = json!({
        "kind": "live_compare",
        "title": "Скорость ответа",
        "title_tooltip": "Показывает задержку Amai в двух слоях сразу: свежую серию ответов Amai в текущем чате для мгновенного operator signal и накопительное окно 24 часов для тренда. Эталоны для обоих режимов всегда фиксированы в таблице.",
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
        } else if has_unclassified_live_signal {
            "окно ещё набирается"
        } else {
            "онлайн-серия ещё набирается"
        };
        card = with_status_label(card, label);
    }
    card
}

fn live_latency_target_row(
    snapshot: &Value,
    mode_label: &str,
    targets: &LiveLatencyTableTargets,
) -> Value {
    json!({
        "label": format!("{mode_label} — эталон"),
        "tooltip": format!(
            "Фиксированный эталон для этого режима. Строгая проверочная выборка отдельно: > {}.",
            format_u64(Some(targets.benchmark_sample_count))
        ),
        "values": target_values(snapshot, targets)
    })
}

fn live_latency_compare_row(
    snapshot: &Value,
    label: &str,
    tooltip: &str,
    slice: Option<&Value>,
    sample_count: u64,
) -> Value {
    json!({
        "label": label,
        "tooltip": tooltip,
        "values": compare_values(snapshot, slice, sample_count)
    })
}

fn latency_window_label(snapshot: &Value) -> String {
    let root = token_budget_report_root(snapshot);
    match root["profile"]["rolling_window_hours"].as_u64() {
        Some(hours) if hours > 0 => format!("скользящее окно {} ч.", format_u64(Some(hours))),
        _ => "накопительное живое окно".to_string(),
    }
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

fn live_response_latency_scope_has_unclassified_activity(snapshot: &Value, scope: &str) -> bool {
    live_response_latency_root(snapshot)
        .and_then(|root| root[scope]["latency_slices"].as_array())
        .into_iter()
        .flatten()
        .any(|slice| {
            slice["sample_count"].as_u64().unwrap_or_default() > 0
                && !matches!(slice["state"].as_str(), Some("hot" | "cold"))
        })
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

pub(super) fn live_latency_compare_status(snapshot: &Value) -> &'static str {
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
