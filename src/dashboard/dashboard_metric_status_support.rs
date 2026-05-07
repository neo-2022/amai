use super::*;

pub(super) fn slowest_observe_refresh_stage(snapshot: &Value) -> (Option<String>, Option<u64>) {
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

pub(super) fn sla_metric_reasons(snapshot: &Value, metrics: &[&str]) -> Vec<String> {
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

pub(super) fn failing_metric_reason_strict_less(
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

pub(super) fn failing_metric_reason_strict_more(
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

pub(super) fn failing_metric_reason_at_most_or_equal(
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

pub(super) fn failing_metric_reason_at_least_or_equal(
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

pub(super) fn status_strict_less_than(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current < target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

pub(super) fn status_strict_more_than(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current > target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

pub(super) fn status_at_most_or_equal(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current <= target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

pub(super) fn status_at_least_or_equal(current: Option<f64>, target: Option<f64>) -> &'static str {
    match (current, target) {
        (Some(current), Some(target)) if current >= target => "pass",
        (Some(_), Some(_)) => "critical",
        _ => "unknown",
    }
}

pub(super) fn cold_contour_status(snapshot: &Value) -> &'static str {
    match snapshot["latest_cold_path_benchmark"]["cold_benchmark"]["executive_summary"]["verdict"]
        .as_str()
    {
        Some("TARGET MET") => "pass",
        Some("PARTIALLY MET") => "alert",
        Some("NOT MET") => "critical",
        _ => "unknown",
    }
}

pub(super) fn status_for_metric_prefix(snapshot: &Value, prefix: &str) -> &'static str {
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

pub(super) fn status_for_metric_name(snapshot: &Value, metric_name: &str) -> &'static str {
    snapshot["sla"]["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|check| check["metric"].as_str() == Some(metric_name))
        .and_then(|check| check["status"].as_str())
        .and_then(normalize_status)
        .unwrap_or("unknown")
}

pub(super) fn combine_statuses(statuses: &[&str]) -> &'static str {
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

pub(super) fn worst_status(left: &str, right: &str) -> &'static str {
    if status_rank(left) >= status_rank(right) {
        normalize_status(left).unwrap_or("unknown")
    } else {
        normalize_status(right).unwrap_or("unknown")
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

pub(super) fn humanize_check(snapshot: &Value, check: &Value) -> String {
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
    use super::worst_status;

    #[test]
    fn critical_status_wins() {
        assert_eq!(worst_status("pass", "critical"), "critical");
        assert_eq!(worst_status("alert", "unknown"), "alert");
        assert_eq!(worst_status("unknown", "pass"), "pass");
    }
}
