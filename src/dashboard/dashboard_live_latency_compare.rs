use super::dashboard_live_response_latency_support::{
    live_response_latency_root, token_budget_report_root,
};
use super::*;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
