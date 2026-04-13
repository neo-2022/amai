use super::*;

pub(super) fn build_service_cards(snapshot: &Value) -> Vec<Value> {
    let postgres_status = combine_statuses(&[
        status_for_metric_name(snapshot, "postgres.query_probe_p95_ms"),
        status_for_metric_name(snapshot, "postgres.connection_usage_ratio"),
        status_for_metric_name(snapshot, "postgres.replica_lag_seconds"),
        status_for_metric_name(snapshot, "postgres.deadlocks_delta"),
    ]);
    let mut postgres_card = card_with_rows(
        "PostgreSQL",
        format_ms(snapshot, snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
        "Живой probe базы метаданных, policy, проектов и continuity-снимков.".to_string(),
        postgres_status,
        Some("Источник: живой PostgreSQL probe, обновляется на каждом refresh dashboard".to_string()),
        Some("PostgreSQL probe — это короткий живой замер базы метаданных, а не исторический benchmark.".to_string()),
        vec![
            metric_row(
                "Эталон probe P95",
                format_ms(
                    snapshot,
                    snapshot["thresholds"]["postgres"]["query_probe_p95_ms"]["target"].as_f64(),
                ),
                Some("Целевой p95 для короткого живого PostgreSQL probe."),
            ),
            metric_row(
                "Измерено probe P95",
                format_ms(snapshot, snapshot["postgres"]["query_probe_p95_ms"].as_f64()),
                Some("Фактический p95 живого PostgreSQL probe на этом refresh."),
            ),
            metric_row(
                "Эталон usage",
                format_ratio_percent(
                    snapshot["thresholds"]["postgres"]["connection_usage_ratio"]["target"]
                        .as_f64(),
                ),
                Some("Желаемая доля занятых соединений PostgreSQL."),
            ),
            metric_row(
                "Измерено usage",
                format_ratio_percent(snapshot["postgres"]["connection_usage_ratio"].as_f64()),
                Some("Фактическая доля занятых соединений прямо сейчас."),
            ),
            metric_row(
                "Измерено TPS",
                format_optional(snapshot["postgres"]["transactions_per_sec"].as_f64(), |v| {
                    format!("{v:.2}")
                }),
                Some("Сколько транзакций в секунду база делает между snapshot-ами."),
            ),
            metric_row(
                "Измерено WAL throughput",
                format_optional(
                    snapshot["postgres"]["wal_bytes_per_sec"].as_f64(),
                    human_bytes_per_sec,
                ),
                Some("Скорость записи журнала WAL между snapshot-ами."),
            ),
        ],
    );
    if let Some(tooltip) = status_reason_tooltip(
        postgres_status,
        sla_metric_reasons(
            snapshot,
            &[
                "postgres.query_probe_p95_ms",
                "postgres.connection_usage_ratio",
                "postgres.replica_lag_seconds",
                "postgres.deadlocks_delta",
            ],
        ),
        "Живой PostgreSQL probe вышел из своей нормы.",
    ) {
        postgres_card = with_status_tooltip(postgres_card, &tooltip);
    }

    let qdrant_live_status = combine_statuses(&[
        status_for_metric_name(snapshot, "qdrant.index_optimize_queue"),
        status_for_metric_name(snapshot, "qdrant.update_queue_length"),
    ]);
    let mut qdrant_live_card = card_with_rows(
        "Qdrant Amai live",
        format_optional(snapshot["qdrant"]["memory_resident_bytes"].as_f64(), human_bytes),
        "Живые системные показатели векторного слоя. Здесь показаны только действительно живые системные числа, а не исторический search-benchmark.".to_string(),
        qdrant_live_status,
        Some("Источник: live Qdrant /metrics Amai, обновляется на каждом refresh dashboard".to_string()),
        Some("Qdrant — векторный слой. Он помогает recall, но не является source of truth для continuity или кода.".to_string()),
        vec![
            metric_row(
                "Эталон optimize queue",
                format_f64_count(snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64()),
                Some("Целевой максимум очереди оптимизации индекса."),
            ),
            metric_row(
                "Optimize queue",
                format_f64_count(snapshot["qdrant"]["index_optimize_queue"].as_f64()),
                Some("Текущая очередь оптимизации индекса Qdrant."),
            ),
            metric_row(
                "Эталон update queue",
                format_f64_count(snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64()),
                Some("Желаемая длина очереди обновлений Qdrant."),
            ),
            metric_row(
                "Update queue",
                format_f64_count(snapshot["qdrant"]["update_queue_length"].as_f64()),
                Some("Текущая длина очереди обновлений Qdrant."),
            ),
            metric_row(
                "Resident memory",
                format_optional(snapshot["qdrant"]["memory_resident_bytes"].as_f64(), human_bytes),
                Some("Объём памяти, который Qdrant держит в resident state прямо сейчас."),
            ),
            metric_row(
                "Points",
                format_f64_count(snapshot["qdrant"]["points_count"].as_f64()),
                Some("Сколько точек сейчас лежит в активной кодовой коллекции Qdrant."),
            ),
            metric_row(
                "Segments",
                format_f64_count(snapshot["qdrant"]["segments_count"].as_f64()),
                Some("Сколько сегментов сейчас держит Qdrant. Много мелких сегментов может быть признаком будущей оптимизации."),
            ),
        ],
    );
    if let Some(tooltip) = status_reason_tooltip(
        qdrant_live_status,
        sla_metric_reasons(
            snapshot,
            &["qdrant.index_optimize_queue", "qdrant.update_queue_length"],
        ),
        "Живой контур Qdrant вышел из своей нормы.",
    ) {
        qdrant_live_card = with_status_tooltip(qdrant_live_card, &tooltip);
    }

    let mut benchmark_qdrant_card = benchmark_qdrant_live_card(snapshot);
    if let Some(tooltip) = benchmark_qdrant_status_tooltip(snapshot) {
        benchmark_qdrant_card = with_status_tooltip(benchmark_qdrant_card, &tooltip);
    }

    let nats_status = combine_statuses(&[
        status_for_metric_name(snapshot, "nats.publish_probe_p95_ms"),
        status_for_metric_name(snapshot, "nats.consumer_lag_msgs"),
        status_for_metric_name(snapshot, "nats.jetstream_disk_usage_ratio"),
    ]);
    let mut nats_card = card_with_rows(
        "NATS / JetStream",
        format_ms(snapshot, snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
        "Живой probe очереди событий и фонового work plane.".to_string(),
        nats_status,
        Some(
            "Источник: живой NATS/JetStream probe, обновляется на каждом refresh dashboard"
                .to_string(),
        ),
        Some("NATS / JetStream — event и work plane для фоновых событий и очередей.".to_string()),
        vec![
            metric_row(
                "Эталон publish P95",
                format_ms(
                    snapshot,
                    snapshot["thresholds"]["nats"]["publish_probe_p95_ms"]["target"].as_f64(),
                ),
                Some("Целевой p95 для живого publish probe."),
            ),
            metric_row(
                "Измерено publish P95",
                format_ms(snapshot, snapshot["nats"]["publish_probe_p95_ms"].as_f64()),
                Some("Фактический p95 для живого publish probe на этом refresh."),
            ),
            metric_row(
                "Эталон lag",
                format_f64_count(
                    snapshot["thresholds"]["nats"]["consumer_lag_msgs"]["target"].as_f64(),
                ),
                Some("Желаемый максимум непрочитанных сообщений."),
            ),
            metric_row(
                "Измерено lag",
                format_f64_count(snapshot["nats"]["consumer_lag_msgs"].as_f64()),
                Some("Текущая consumer lag в JetStream."),
            ),
            metric_row(
                "Эталон disk usage",
                format_ratio_percent(
                    snapshot["thresholds"]["nats"]["jetstream_disk_usage_ratio"]["target"].as_f64(),
                ),
                Some("Желаемая доля занятого диска JetStream."),
            ),
            metric_row(
                "Измерено disk usage",
                format_ratio_percent(snapshot["nats"]["jetstream_disk_usage_ratio"].as_f64()),
                Some("Текущая доля занятого диска JetStream."),
            ),
        ],
    );
    if let Some(tooltip) = status_reason_tooltip(
        nats_status,
        sla_metric_reasons(
            snapshot,
            &[
                "nats.publish_probe_p95_ms",
                "nats.consumer_lag_msgs",
                "nats.jetstream_disk_usage_ratio",
            ],
        ),
        "Живой контур NATS / JetStream вышел из своей нормы.",
    ) {
        nats_card = with_status_tooltip(nats_card, &tooltip);
    }

    vec![
        postgres_card,
        qdrant_live_card,
        benchmark_qdrant_card,
        nats_card,
        build_governance_card(snapshot),
    ]
}

pub(super) fn benchmark_qdrant_live_card(snapshot: &Value) -> Value {
    let benchmark = &snapshot["benchmark_qdrant"];
    let run_summary = &benchmark["run_summary"];
    let configured = benchmark["configured"].as_bool().unwrap_or(false);
    let available = benchmark["available"].as_bool().unwrap_or(false);
    let active = benchmark["active"].as_bool().unwrap_or(false);
    let from_last_success = benchmark["from_last_success"].as_bool().unwrap_or(false);
    let run_state = run_summary["run_state"].as_str().unwrap_or("not_started");
    let status = if !configured {
        "unknown"
    } else if active && available {
        let live_probe_status = combine_statuses(&[
            status_at_most_or_equal(
                benchmark["index_optimize_queue"].as_f64(),
                snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64(),
            ),
            status_at_most_or_equal(
                benchmark["update_queue_length"].as_f64(),
                snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            ),
        ]);
        if live_probe_status == "pass" {
            "waiting"
        } else {
            live_probe_status
        }
    } else if run_state == "finished_error" || run_state == "finished_benchmark_failed" {
        "alert"
    } else if !active {
        "unknown"
    } else if !available {
        "alert"
    } else {
        "unknown"
    };
    let dataset_size = run_summary["dataset_size"].as_u64();
    let points_count = benchmark["points_count"].as_u64().or_else(|| {
        benchmark["points_count"]
            .as_f64()
            .map(|value| value.max(0.0) as u64)
    });
    let progress_label = match (points_count, dataset_size) {
        (Some(points), Some(total)) if total > 0 => {
            let percent = (points as f64 * 100.0 / total as f64).clamp(0.0, 100.0);
            if points >= total {
                format!(
                    "{} из {} точек",
                    format_u64(Some(points)),
                    format_u64(Some(total))
                )
            } else {
                format!(
                    "{} из {} точек ({percent:.0}%)",
                    format_u64(Some(points)),
                    format_u64(Some(total))
                )
            }
        }
        (Some(points), _) => format!("{} точек", format_u64(Some(points))),
        _ => "ещё нет данных".to_string(),
    };
    let elapsed_label = run_summary["started_at_epoch_s"].as_u64().map(|started| {
        let end_ms = run_summary["finished_at_epoch_s"]
            .as_u64()
            .unwrap_or_else(|| OffsetDateTime::now_utc().unix_timestamp().max(0) as u64)
            * 1000;
        human_elapsed_ms(end_ms.saturating_sub(started.saturating_mul(1000)))
    });
    let aggregate_result = &run_summary["aggregate_result"];
    let last_result = &run_summary["latest_result"];
    let system_evaluation_label = if aggregate_result.is_object() {
        format!(
            "recall {} • p95 {} • p99 {}",
            format_ratio_percent(aggregate_result["recall"].as_f64()),
            format_ms(snapshot, aggregate_result["p95_ms"].as_f64()),
            format_ms(snapshot, aggregate_result["p99_ms"].as_f64())
        )
    } else if last_result.is_object() {
        format!(
            "recall {} • p95 {} • p99 {}",
            format_ratio_percent(last_result["recall"].as_f64()),
            format_ms(snapshot, last_result["p95_ms"].as_f64()),
            format_ms(snapshot, last_result["p99_ms"].as_f64())
        )
    } else {
        "ещё нет данных".to_string()
    };
    let live_progress = &run_summary["live_progress"];
    let current_definition_label = live_progress["definition_label"]
        .as_str()
        .map(humanize_ann_definition_label);
    let live_stage_label = match (
        &current_definition_label,
        live_progress["group_current"].as_u64(),
        live_progress["group_total"].as_u64(),
        live_progress["processed_current"].as_u64(),
        live_progress["processed_total"].as_u64(),
    ) {
        (
            Some(definition),
            Some(group_current),
            Some(group_total),
            Some(processed_current),
            Some(processed_total),
        ) => format!(
            "{definition}; группа {} из {}; {} из {} запросов",
            group_current,
            group_total,
            format_u64(Some(processed_current)),
            format_u64(Some(processed_total))
        ),
        (Some(definition), Some(group_current), Some(group_total), _, _) => {
            format!("{definition}; группа {} из {}", group_current, group_total)
        }
        (Some(definition), _, _, Some(processed_current), Some(processed_total)) => format!(
            "{definition}; {} из {} запросов",
            format_u64(Some(processed_current)),
            format_u64(Some(processed_total))
        ),
        (
            None,
            Some(group_current),
            Some(group_total),
            Some(processed_current),
            Some(processed_total),
        ) => format!(
            "группа {} из {}; {} из {} запросов",
            group_current,
            group_total,
            format_u64(Some(processed_current)),
            format_u64(Some(processed_total))
        ),
        (Some(definition), None, None, None, None) => definition.clone(),
        (None, Some(group_current), Some(group_total), _, _) => {
            format!("группа {} из {}", group_current, group_total)
        }
        (None, _, _, Some(processed_current), Some(processed_total)) => format!(
            "{} из {} запросов",
            format_u64(Some(processed_current)),
            format_u64(Some(processed_total))
        ),
        _ => "ещё нет данных".to_string(),
    };
    let remaining_progress_label = live_progress["group_current"]
        .as_u64()
        .zip(live_progress["group_total"].as_u64())
        .zip(
            live_progress["processed_current"]
                .as_u64()
                .zip(live_progress["processed_total"].as_u64()),
        )
        .map(
            |((group_current, group_total), (processed_current, processed_total))| {
                let remaining_groups = group_total.saturating_sub(group_current);
                let remaining_queries = processed_total.saturating_sub(processed_current);
                if remaining_groups == 0 && remaining_queries == 0 {
                    "текущий шаг завершается".to_string()
                } else if remaining_queries == 0 {
                    format!(
                        "после смены шага останется {} групп",
                        format_u64(Some(remaining_groups))
                    )
                } else if remaining_groups == 0 {
                    format!(
                        "{} запросов до конца текущего шага",
                        format_u64(Some(remaining_queries))
                    )
                } else {
                    format!(
                        "{} групп после текущей; {} запросов до конца шага",
                        format_u64(Some(remaining_groups)),
                        format_u64(Some(remaining_queries))
                    )
                }
            },
        )
        .or_else(|| {
            live_progress["processed_current"]
                .as_u64()
                .zip(live_progress["processed_total"].as_u64())
                .map(|(processed_current, processed_total)| {
                    format!(
                        "{} запросов до конца текущего шага",
                        format_u64(Some(processed_total.saturating_sub(processed_current)))
                    )
                })
        })
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let benchmark_memory_label = benchmark["memory_resident_bytes"].as_f64().map(human_bytes);
    let value = if active {
        "идёт прогон".to_string()
    } else if run_state == "finished_ok" {
        "последний прогон успешен".to_string()
    } else if run_state == "finished_error" || run_state == "finished_benchmark_failed" {
        "последний прогон с ошибкой".to_string()
    } else if available || from_last_success {
        "тест не запущен".to_string()
    } else if configured {
        "ещё нет данных".to_string()
    } else {
        "не настроено".to_string()
    };
    let status_label_override = if !configured {
        None
    } else if active {
        Some("идёт прогон".to_string())
    } else if run_state == "finished_ok" {
        Some("последний прогон успешен".to_string())
    } else if run_state == "finished_error" || run_state == "finished_benchmark_failed" {
        Some("последний прогон с ошибкой".to_string())
    } else {
        Some("тест не запущен".to_string())
    };
    let note = if active && available {
        "Отдельный Qdrant для внешнего бенча. Здесь видно, что сейчас считается, сколько уже загружено и сколько ещё осталось.".to_string()
    } else if run_state == "finished_ok" {
        "Тест сейчас не идёт. Здесь оставлен итог последнего успешного прогона.".to_string()
    } else if run_state == "finished_error" || run_state == "finished_benchmark_failed" {
        "Последний внешний прогон завершился с ошибкой. Здесь виден последний сохранённый срез."
            .to_string()
    } else if from_last_success {
        "Тест сейчас не идёт. Показан последний сохранённый результат и последний известный срез."
            .to_string()
    } else if configured {
        "Отдельный benchmark-Qdrant настроен, но внешний прогон ещё не дал полезного результата."
            .to_string()
    } else {
        "Отдельный benchmark-Qdrant ещё не настроен.".to_string()
    };
    let source_label = if active && available {
        Some(format!(
            "Источник: live Qdrant /metrics + workspace последнего внешнего прогона ({}). Карточка обновляется при refresh dashboard.",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else if run_state == "finished_ok"
        || run_state == "finished_error"
        || run_state == "finished_benchmark_failed"
    {
        Some(format!(
            "Источник: workspace последнего внешнего прогона + последний известный срез benchmark-Qdrant ({}).",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else if from_last_success {
        Some(format!(
            "Источник: последний сохранённый результат и срез benchmark-Qdrant ({}).",
            benchmark["http_url"].as_str().unwrap_or("unknown")
        ))
    } else {
        Some(
            "Источник: отдельный benchmark-Qdrant и workspace внешнего benchmark-прогона. Эта карточка не берёт данные из Amai live."
                .to_string(),
        )
    };
    let result_row_label = if aggregate_result.is_object() || last_result.is_object() {
        "Последний результат"
    } else {
        "Состояние"
    };
    let result_row_value = if aggregate_result.is_object() || last_result.is_object() {
        system_evaluation_label
    } else if active {
        "ещё не сохранён".to_string()
    } else {
        run_state.to_string()
    };
    let result_row_tooltip = if aggregate_result.is_object() || last_result.is_object() {
        "Честная сводная оценка системы по уже завершённым замерам этого прогона. По умолчанию это медиана completed results, а не лучший и не последний шумный файл."
    } else {
        "Текущее machine-readable состояние отдельного внешнего benchmark-прогона, когда итоговая оценка ещё не materialized."
    };
    let rows = vec![
        metric_row(
            "Прогон",
            format!(
                "{} / {}",
                run_summary["benchmark_display_name"]
                    .as_str()
                    .unwrap_or("ещё нет данных"),
                run_summary["dataset_display_name"]
                    .as_str()
                    .unwrap_or("ещё нет данных")
            ),
            Some(
                "Какой внешний benchmark и какой набор данных сейчас выбран для этого отдельного Qdrant.",
            ),
        ),
        metric_row(
            "Данные",
            progress_label,
            Some(
                "Сколько точек уже загружено во внешний benchmark-Qdrant. Если известен полный размер набора данных, показан и процент.",
            ),
        ),
        metric_row(
            "Время",
            elapsed_label.unwrap_or_else(|| "ещё нет данных".to_string()),
            Some(
                "Сколько времени идёт текущий прогон или сколько длился последний завершённый прогон.",
            ),
        ),
        metric_row(
            "Сейчас",
            live_stage_label,
            Some(
                "Какой definition-run сейчас активен и какой query group/сколько запросов уже обработано в текущем замере.",
            ),
        ),
        metric_row(
            "До конца",
            remaining_progress_label,
            Some(
                "Сколько ещё осталось до конца текущей query group и сколько групп останется после неё в текущем definition-run.",
            ),
        ),
        metric_row(result_row_label, result_row_value, Some(result_row_tooltip)),
    ];
    let mut rows = rows;
    if let Some(memory_label) = benchmark_memory_label {
        rows.push(metric_row(
            "Память",
            memory_label,
            Some("Сколько памяти сейчас занимает отдельный Qdrant для внешнего benchmark-прогона."),
        ));
    }
    if let Some(queue) = benchmark["index_optimize_queue"].as_f64() {
        rows.push(metric_row(
            "Очередь optimize",
            format_f64_count(Some(queue)),
            Some("Есть ли сейчас хвост фоновой оптимизации в benchmark-Qdrant."),
        ));
    }
    let mut card = with_extra_class(
        json!({
            "title": "Qdrant внешнего бенча",
            "value": value,
            "note": note,
            "status": status,
            "status_label": status_label_override.unwrap_or_else(|| status_label(status).to_string()),
            "source_label": source_label,
            "title_tooltip": Some("Это отдельный Qdrant для внешних benchmark-прогонов. Карточка показывает состояние самого прогона и его последний результат, а не просто сырую телеметрию памяти.".to_string()),
            "rows": rows,
        }),
        "stack-metric-card",
    );
    if status == "waiting" && active {
        card = with_status_tooltip(
            card,
            "Прогон ещё идёт. Итоговый статус и финальный verdict появятся после завершения.",
        );
    }
    card
}

fn humanize_ann_definition_label(raw: &str) -> String {
    let parts = raw
        .trim_matches(|ch| ch == '[' || ch == ']')
        .split(',')
        .map(|value| value.trim().trim_matches('\''))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 4 {
        return raw
            .trim_matches(|ch| ch == '[' || ch == ']')
            .replace('\'', "");
    }
    let quantization = match parts[1] {
        "none" => "без квантования".to_string(),
        "binary" => "binary".to_string(),
        "scalar" => "scalar".to_string(),
        other => other.to_string(),
    };
    format!("{quantization}, m={}, ef={}", parts[2], parts[3])
}

fn benchmark_qdrant_status_tooltip(snapshot: &Value) -> Option<String> {
    let benchmark = &snapshot["benchmark_qdrant"];
    let configured = benchmark["configured"].as_bool().unwrap_or(false);
    let available = benchmark["available"].as_bool().unwrap_or(false);
    let active = benchmark["active"].as_bool().unwrap_or(false);
    let from_last_success = benchmark["from_last_success"].as_bool().unwrap_or(false);
    let status = if !configured {
        "unknown"
    } else if !active {
        "unknown"
    } else if !available {
        "alert"
    } else {
        combine_statuses(&[
            status_at_most_or_equal(
                benchmark["index_optimize_queue"].as_f64(),
                snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64(),
            ),
            status_at_most_or_equal(
                benchmark["update_queue_length"].as_f64(),
                snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            ),
        ])
    };
    let mut reasons = Vec::new();
    if !configured {
        reasons.push("Отдельный benchmark-Qdrant ещё не настроен.".to_string());
    }
    if configured && !active {
        reasons.push("Внешний benchmark сейчас не запущен, поэтому карточка живёт по последнему срезу, а не по текущему потоку.".to_string());
    }
    if configured && !available && from_last_success {
        reasons.push("Живой benchmark-Qdrant сейчас недоступен, поэтому панель держится на последнем успешном срезе.".to_string());
    } else if configured && !available {
        reasons.push("Живой benchmark-Qdrant сейчас недоступен.".to_string());
    }
    if active && available {
        if let Some(reason) = failing_metric_reason_at_most_or_equal(
            "Optimize queue",
            benchmark["index_optimize_queue"].as_f64(),
            snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64(),
            format_f64_count(benchmark["index_optimize_queue"].as_f64()),
            format_f64_count(snapshot["thresholds"]["qdrant"]["optimize_queue"]["target"].as_f64()),
        ) {
            reasons.push(reason);
        }
        if let Some(reason) = failing_metric_reason_at_most_or_equal(
            "Update queue",
            benchmark["update_queue_length"].as_f64(),
            snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            format_f64_count(benchmark["update_queue_length"].as_f64()),
            format_f64_count(
                snapshot["thresholds"]["qdrant"]["update_queue_length"]["target"].as_f64(),
            ),
        ) {
            reasons.push(reason);
        }
    }
    status_reason_tooltip(
        status,
        reasons,
        "Контур внешнего benchmark-Qdrant сейчас не выглядит устойчивым.",
    )
}
