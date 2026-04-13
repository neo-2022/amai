use super::*;

pub(super) fn build_benchmark_cards(snapshot: &Value) -> Vec<Value> {
    let hot_load = &snapshot["latest_retrieval_load_hot"]["load_verification"];
    let hot_retrieval = &snapshot["latest_retrieval_hot"]["benchmark"];
    let cold_live_progress = &snapshot["cold_path_benchmark_progress"]["cold_benchmark_progress"];
    let cold_live_running = cold_live_progress["state"].as_str() == Some("running");
    let cold_contour = if cold_live_running {
        cold_live_progress
    } else {
        &snapshot["latest_cold_path_benchmark"]["cold_benchmark"]
    };
    let live_elapsed_seconds = if cold_live_running {
        snapshot["captured_at_epoch_ms"]
            .as_u64()
            .zip(cold_live_progress["started_at_epoch_ms"].as_u64())
            .map(|(captured, started)| captured.saturating_sub(started) as f64 / 1000.0)
    } else {
        None
    };
    let accuracy = &snapshot["latest_retrieval_accuracy"]["accuracy_verification"];
    let thresholds = &snapshot["thresholds"];
    let hot_load_sample_count = hot_load["success_count"]
        .as_u64()
        .zip(hot_load["error_count"].as_u64())
        .map(|(success, errors)| success + errors);
    let hot_load_scope = format!(
        "project={} / namespace={} / query={} / execution_mode={}",
        hot_load["project"].as_str().unwrap_or("ещё нет данных"),
        hot_load["namespace"].as_str().unwrap_or("ещё нет данных"),
        hot_load["query"].as_str().unwrap_or("ещё нет данных"),
        hot_load["execution_mode"]
            .as_str()
            .unwrap_or("ещё нет данных"),
    );
    let hot_retrieval_scope = format!(
        "project={} / namespace={} / query={} / disable_cache={}",
        hot_retrieval["project"]
            .as_str()
            .unwrap_or("ещё нет данных"),
        hot_retrieval["namespace"]
            .as_str()
            .unwrap_or("ещё нет данных"),
        hot_retrieval["query"].as_str().unwrap_or("ещё нет данных"),
        hot_retrieval["disable_cache"]
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "ещё нет данных".to_string()),
    );

    let hot_load_status = hot_load_benchmark_status(hot_load, thresholds);
    let mut hot_load_card = compare_table_card(
            "Нагрузка после прогрева",
            format!(
                "Контур данных: latest_retrieval_load_hot.load_verification. Scope snapshot: {hot_load_scope}. Это отдельный hot-load прогон по прогретому быстрому пути. Он не равен retrieval.hot_p95_ms и не является живой телеметрией текущей сессии. Burst QPS здесь считается как success_count / wall_clock, а не как целый счётчик за полную секунду. В последнем прогоне это {} запросов за {}.",
                format_u64(hot_load_sample_count),
                format_ms(snapshot, hot_load["wall_clock_ms"].as_f64()),
            ),
            hot_load_status,
            Some(source_label(
                &format!(
                    "Источник: benchmark snapshot latest_retrieval_load_hot.load_verification. Scope: {hot_load_scope}. Live-данные страницы сюда не подмешиваются"
                ),
                hot_load["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это отдельный параллельный load-contour. Он нужен для Burst QPS, worker-ов и error-rate под нагрузкой. Его нельзя один к одному сравнивать с retrieval hot benchmark, который питает SLA `retrieval.hot_p95_ms`.".to_string()),
            Some(format_burst_qps_table(hot_load["qps"].as_f64())),
            vec![
                compare_table_row(
                    "Burst QPS",
                    "Средняя скорость внутри короткого benchmark-окна hot-load прогона. Это burst-rate, а не обещание стабильной обычной пропускной способности.",
                    compare_pair(
                        format_burst_qps_threshold(
                            thresholds["load"]["hot_qps"].get("target").and_then(Value::as_f64),
                            ">",
                        ),
                        format_burst_qps_table(hot_load["qps"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "P50",
                    "Медиана hot benchmark. Обычный уровень задержки в отдельном нагрузочном прогоне.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_p50_ms"].as_f64(),
                        hot_load["p50_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P95",
                    "Тяжёлый хвост hot benchmark. Почти все прогретые ответы должны укладываться в эту границу.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_p95_ms"].as_f64(),
                        hot_load["p95_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P99",
                    "Редкие тяжёлые выбросы в отдельном hot-load benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_p99_ms"].as_f64(),
                        hot_load["p99_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Max",
                    "Самый тяжёлый одиночный запрос в последнем hot-load benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["load"]["hot_benchmark_table"]["target_max_ms"].as_f64(),
                        hot_load["max_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Error rate",
                    "Доля ошибок в отдельном hot-load benchmark. Здесь целевой уровень должен быть нулевым.",
                    compare_pair(
                        format_zero_or_at_most_percent(
                            thresholds["load"]["hot_error_rate"].get("target").and_then(Value::as_f64),
                        ),
                        format_percent(hot_load["error_rate"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "Workers",
                    "Сколько параллельных worker-ов участвовало в benchmark-прогоне.",
                    compare_pair(
                        format_threshold_at_least(
                            thresholds["load"]["hot_benchmark_table"]["target_workers"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_load["workers"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Выборка",
                    "Сколько отдельных запросов вошло в benchmark. Это не живая сессия, а размер сохранённого проверочного прогона.",
                    compare_pair(
                        format_threshold_at_least(
                            thresholds["load"]["hot_benchmark_table"]["target_sample_count"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_load_sample_count),
                    ),
                ),
            ],
        );
    if let Some(tooltip) = status_reason_tooltip(
        hot_load_status,
        hot_load_benchmark_reasons(snapshot, hot_load, thresholds),
        "Hot-load benchmark вышел из своей нормы, но детальные причины пока не удалось собрать.",
    ) {
        hot_load_card = with_status_tooltip(hot_load_card, &tooltip);
    }

    let hot_retrieval_status = hot_retrieval_benchmark_status(hot_retrieval, thresholds);
    let mut hot_retrieval_card = compare_table_card(
            "Повторный запрос",
            format!(
                "Контур данных: latest_retrieval_hot.benchmark. Scope snapshot: {hot_retrieval_scope}. Это именно источник SLA-метрики retrieval.hot_p95_ms. Это не hot-load benchmark и не живая телеметрия текущей сессии."
            ),
            hot_retrieval_status,
            Some(source_label(
                &format!(
                    "Источник: benchmark snapshot latest_retrieval_hot.benchmark. Этот snapshot напрямую кормит SLA retrieval.hot_p95_ms. Scope: {hot_retrieval_scope}"
                ),
                hot_retrieval["captured_at_epoch_ms"].as_u64(),
            )),
            Some("Это короткий retrieval-бенчмарк одиночного повторного запроса. Он показывает latency самого retrieval-контура и именно его значения идут в SLA `retrieval.hot_p95_ms`.".to_string()),
            Some(format_ms(snapshot, hot_retrieval["p95_ms"].as_f64())),
            vec![
                compare_table_row(
                    "Burst QPS",
                    "Средняя скорость внутри короткого retrieval benchmark-окна. Это burst-rate этого контура, а не нагрузочный QPS из hot-load и не SLA-порог.",
                    compare_pair(
                        "нет SLA-порога".to_string(),
                        format_burst_qps_table(hot_retrieval["qps"].as_f64()),
                    ),
                ),
                compare_table_row(
                    "P50",
                    "Медиана одиночного повторного retrieval-запроса в benchmark-контуре, который кормит SLA retrieval.hot_p95_ms.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_p50_ms"].as_f64(),
                        hot_retrieval["p50_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P95",
                    "Тяжёлый хвост retrieval hot benchmark. Именно этот показатель используется в SLA retrieval.hot_p95_ms.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_p95_ms"].as_f64(),
                        hot_retrieval["p95_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "P99",
                    "Редкие тяжёлые выбросы в retrieval hot benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_p99_ms"].as_f64(),
                        hot_retrieval["p99_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Max",
                    "Самый тяжёлый одиночный запрос в retrieval hot benchmark.",
                    format_time_compare_pair(
                        snapshot,
                        thresholds["retrieval"]["hot_live_table"]["target_max_ms"].as_f64(),
                        hot_retrieval["max_ms"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Итерации",
                    "Сколько измерений вошло в последний retrieval hot benchmark snapshot.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            thresholds["retrieval"]["hot_benchmark_table"]["target_iterations"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_retrieval["iterations"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Warmup",
                    "Сколько прогревочных запросов было выполнено перед измерением retrieval hot benchmark.",
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            thresholds["retrieval"]["hot_benchmark_table"]["target_warmup"]
                                .as_f64(),
                            "",
                            0,
                        ),
                        format_u64(hot_retrieval["warmup"].as_u64()),
                    ),
                ),
            ],
        );
    if let Some(tooltip) = status_reason_tooltip(
        hot_retrieval_status,
        hot_retrieval_benchmark_reasons(snapshot, hot_retrieval, thresholds),
        "Hot retrieval benchmark вышел из своей нормы, но детальные причины пока не удалось собрать.",
    ) {
        hot_retrieval_card = with_status_tooltip(hot_retrieval_card, &tooltip);
    }

    let cold_status = if cold_live_running {
        "waiting"
    } else {
        cold_contour_status(snapshot)
    };
    let cold_sample_count = cold_contour["machine_readable_summary"]["sample_count"]
        .as_u64()
        .unwrap_or(0);
    let cold_has_samples = cold_sample_count > 0;
    let cold_headline_value = if cold_has_samples {
        Some(format_ms(
            snapshot,
            cold_contour["machine_readable_summary"]["p95"].as_f64(),
        ))
    } else if cold_live_running {
        Some("ещё нет данных".to_string())
    } else {
        Some(format_ms(
            snapshot,
            cold_contour["machine_readable_summary"]["p95"].as_f64(),
        ))
    };
    let mut cold_rows = Vec::new();
    if cold_live_running {
        cold_rows.push(compare_table_row(
            "Прогресс",
            "Сколько cold-case уже завершено в текущем живом прогоне.",
            compare_pair(
                "идёт прогон".to_string(),
                format!(
                    "{} из {}",
                    format_u64(cold_live_progress["progress"]["completed_case_count"].as_u64()),
                    format_u64(cold_live_progress["progress"]["target_case_count"].as_u64()),
                ),
            ),
        ));
        cold_rows.push(compare_table_row(
            "Прошло",
            "Сколько уже длится текущий живой прогон по wall-clock времени.",
            compare_pair(
                "живой прогон".to_string(),
                format_seconds(snapshot, live_elapsed_seconds),
            ),
        ));
        if let Some(current_repo_code) = cold_live_progress["current_repo_code"].as_str() {
            let current_repo_name = cold_live_progress["current_repo_display_name"]
                .as_str()
                .unwrap_or(current_repo_code);
            cold_rows.push(compare_table_row(
                "Индексирование",
                "Сколько файлов текущего репозитория уже реально записано в индекс для этого cold-прогона.",
                compare_pair(
                    current_repo_name.to_string(),
                    format!(
                        "{} из {}",
                        format_u64(
                            cold_live_progress["progress"]["current_repo_indexed_files"].as_u64()
                        ),
                        format_u64(
                            cold_live_progress["progress"]["current_repo_target_files"].as_u64()
                        ),
                    ),
                ),
            ));
        }
    }
    cold_rows.extend([
                compare_table_row(
                    "Cold P50",
                    if cold_live_running {
                        "Текущий обычный уровень задержки по уже завершённой части живого cold-прогона."
                    } else {
                        "Цель и факт по обычному уровню задержки в полном cold end-to-end пути."
                    },
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_p50_ms"].as_f64(),
                        cold_has_samples.then(|| cold_contour["machine_readable_summary"]["p50"].as_f64()).flatten(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Cold P95",
                    if cold_live_running {
                        "Текущий тяжёлый хвост по уже завершённой части живого cold-прогона."
                    } else {
                        "Цель и факт по p95 в полном cold end-to-end пути."
                    },
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_p95_ms"].as_f64(),
                        cold_has_samples.then(|| cold_contour["machine_readable_summary"]["p95"].as_f64()).flatten(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Cold P99",
                    if cold_live_running {
                        "Текущий редкий хвост по уже завершённой части живого cold-прогона."
                    } else {
                        "Цель и факт по p99 в полном cold end-to-end пути."
                    },
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_p99_ms"].as_f64(),
                        cold_has_samples.then(|| cold_contour["machine_readable_summary"]["p99"].as_f64()).flatten(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Cold Max",
                    if cold_live_running {
                        "Самый тяжёлый уже завершённый запрос в текущем живом cold-прогоне."
                    } else {
                        "Цель и факт по самому тяжёлому выбросу в cold benchmark."
                    },
                    format_time_compare_pair(
                        snapshot,
                        cold_contour["profile"]["target_max_ms"].as_f64(),
                        cold_has_samples.then(|| cold_contour["machine_readable_summary"]["max"].as_f64()).flatten(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Precision",
                    if cold_live_running {
                        "Текущая чистота найденного контекста по уже завершённым cold-case."
                    } else {
                        "Точность: насколько чисто найденный контекст оказался релевантным."
                    },
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["min_precision"]
                                .as_f64()
                                .map(|value| value * 100.0),
                            ">=",
                            "%",
                            2,
                        ),
                        format_ratio_percent(cold_has_samples.then(|| cold_contour["machine_readable_summary"]["precision"].as_f64()).flatten()),
                    ),
                ),
                compare_table_row(
                    "Recall",
                    if cold_live_running {
                        "Текущая полнота найденного контекста по уже завершённым cold-case."
                    } else {
                        "Полнота: насколько полно система нашла нужные целевые данные."
                    },
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["min_recall"]
                                .as_f64()
                                .map(|value| value * 100.0),
                            ">=",
                            "%",
                            2,
                        ),
                        format_ratio_percent(cold_has_samples.then(|| cold_contour["machine_readable_summary"]["recall"].as_f64()).flatten()),
                    ),
                ),
                compare_table_row(
                    "Hit rate",
                    if cold_live_running {
                        "Доля уже завершённых cold-case, где система попала в нужную цель."
                    } else {
                        "Доля запросов, где система действительно попала в нужную цель."
                    },
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["min_target_hit_rate"]
                                .as_f64()
                                .map(|value| value * 100.0),
                            ">=",
                            "%",
                            2,
                        ),
                        format_ratio_percent(cold_has_samples.then(|| cold_contour["machine_readable_summary"]["hit_rate"].as_f64()).flatten()),
                    ),
                ),
                compare_table_row(
                    "Выборка",
                    if cold_live_running {
                        "Сколько cold-case уже вошло в текущий живой прогон."
                    } else {
                        "Сколько cold-запросов вошло в итоговый benchmark."
                    },
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            cold_contour["profile"]["min_sample_count"].as_f64(),
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["sample_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Repo count",
                    if cold_live_running {
                        "Сколько разных репозиториев уже покрыто в текущем живом прогоне."
                    } else {
                        "Сколько разных репозиториев вошло в последний cold benchmark."
                    },
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            cold_contour["profile"]["min_repo_count"].as_f64(),
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["repo_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Query slices",
                    if cold_live_running {
                        "Сколько разных query-slice уже покрыто в текущем живом прогоне."
                    } else {
                        "Сколько разных типов запросов покрывает последний cold benchmark."
                    },
                    compare_pair(
                        format_threshold_at_least_or_equal(
                            cold_contour["profile"]["min_query_slice_count"].as_f64(),
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["query_slice_count"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Duration",
                    if cold_live_running {
                        "Сколько чистого benchmark-времени уже накоплено по завершённым cold-case. Это та же метрика, которая станет финальной `Duration` после завершения прогона."
                    } else {
                        "Сколько длился полный последний cold benchmark."
                    },
                    format_seconds_compare_pair(
                        snapshot,
                        cold_contour["profile"]["max_duration_seconds"].as_f64(),
                        cold_contour["machine_readable_summary"]["duration"].as_f64(),
                        "<",
                    ),
                ),
                compare_table_row(
                    "Leakage",
                    if cold_live_running {
                        "Сколько cross-project утечек уже поймано в текущем живом прогоне."
                    } else {
                        "Сколько cross-project утечек поймал cold benchmark. Для строгой изоляции здесь должно оставаться ровно 0."
                    },
                    compare_pair(
                        format_threshold_value(
                            cold_contour["profile"]["max_leakage"].as_f64(),
                            "=",
                            "",
                            0,
                        ),
                        format_u64(cold_contour["machine_readable_summary"]["leakage"].as_u64()),
                    ),
                ),
                compare_table_row(
                    "Error rate",
                    if cold_live_running {
                        "Доля ошибок по уже завершённой части текущего живого прогона."
                    } else {
                        "Доля ошибок в последнем полном cold benchmark."
                    },
                    compare_pair(
                        format_zero_or_at_most_percent(
                            cold_contour["profile"]["max_error_rate"]
                                .as_f64()
                                .map(|value| value * 100.0),
                        ),
                        format_percent(cold_contour["machine_readable_summary"]["error_rate"].as_f64()),
                    ),
                ),
    ]);
    let mut cold_card = compare_table_card(
        "Новый запрос без прогрева",
        if cold_live_running {
            "Контур данных: cold_path_benchmark_progress.cold_benchmark_progress. Сейчас реально идёт живой cold benchmark: цифры ниже частичные, обновляются по мере прогона и не подменяют финальный сохранённый snapshot.".to_string()
        } else {
            "Контур данных: latest_cold_path_benchmark.cold_benchmark. Это последний честный полноразмерный end-to-end cold benchmark по реальным репозиториям и query slices; proof/smoke прогоны эту витрину не перетирают.".to_string()
        },
        cold_status,
        Some(source_label(
            if cold_live_running {
                "Источник: live progress cold_path_benchmark_progress.cold_benchmark_progress. Финальный snapshot latest_cold_path_benchmark обновится после завершения этого прогона"
            } else {
                "Источник: coverage-qualified benchmark snapshot latest_cold_path_benchmark.cold_benchmark. Live-данные страницы сюда не подмешиваются"
            },
            if cold_live_running {
                snapshot["captured_at_epoch_ms"].as_u64()
            } else {
                cold_contour["captured_at_epoch_ms"]
                    .as_u64()
                    .or_else(|| cold_live_progress["captured_at_epoch_ms"].as_u64())
            },
        )),
        Some(if cold_live_running {
            "Это тот же cold contour, но в живом режиме: карточка показывает честный частичный прогресс текущего прогона и обновляется по мере новых завершённых case. Финальный verdict появится только после завершения полного benchmark.".to_string()
        } else {
            "Это проверка первого запроса без прогрева. Она меряет весь путь ответа целиком: от выбора нужного маршрута до сборки готового контекста для ответа.".to_string()
        }),
        cold_headline_value,
        cold_rows,
    );
    if cold_live_running {
        cold_card["status_label"] = Value::String("идёт прогон".to_string());
        cold_card["table"]["columns"][2]["label"] = Value::String("Онлайн\nсейчас".to_string());
    }
    if let Some(tooltip) = status_reason_tooltip(
        cold_status,
        if cold_live_running {
            cold_benchmark_progress_reasons(snapshot, cold_contour, cold_live_progress)
        } else {
            cold_benchmark_reasons(snapshot, cold_contour)
        },
        "Cold end-to-end benchmark вышел из своей нормы, но детальные причины пока не удалось собрать.",
    ) {
        cold_card = with_status_tooltip(cold_card, &tooltip);
    }

    let accuracy_status = worst_status(
        status_for_metric_prefix(snapshot, "accuracy.cross_project_leakage"),
        worst_status(
            status_for_metric_prefix(snapshot, "accuracy.symbol_precision"),
            status_for_metric_prefix(snapshot, "accuracy.semantic_precision"),
        ),
    );
    let mut accuracy_card = compare_table_card(
                    "Точность и изоляция",
                    "Контур данных: latest_retrieval_accuracy.accuracy_verification. Этот блок не потоковый: он показывает последний сохранённый accuracy/isolation verification contour. Карточка развернута по ширине, чтобы accuracy и isolation читались рядом и не сжимали остальные benchmark-блоки."
                        .to_string(),
                    accuracy_status,
                    Some(source_label(
                        "Источник: benchmark snapshot latest_retrieval_accuracy.accuracy_verification. Live-данные страницы сюда не подмешиваются",
                        accuracy["captured_at_epoch_ms"].as_u64(),
                    )),
                    Some("Проверка точности и изоляции показывает, не течёт ли один проект в другой и насколько точно Amai попадает в нужные символы и семантику.".to_string()),
                    Some(format!(
                        "утечки {} • symbol {} • semantic {}",
                        format_f64_count(accuracy["cross_project_leakage"].as_f64()),
                        format_ratio_percent(accuracy["symbol_precision"].as_f64()),
                        format_ratio_percent(accuracy["semantic_precision"].as_f64())
                    )),
                    vec![
                        compare_table_row(
                            "Leakage",
                            "Для строгой проектной изоляции утечки между проектами должны быть равны нулю.",
                            compare_pair(
                                "0".to_string(),
                                format_f64_count(accuracy["cross_project_leakage"].as_f64()),
                            ),
                        ),
                        compare_table_row(
                            "Symbol precision",
                            "Насколько точно retrieval попадает в нужные символы, функции и сущности.",
                            compare_pair(
                                format_ratio_percent(
                                    thresholds["accuracy"]["symbol_precision"]["target"].as_f64(),
                                ),
                                format_ratio_percent(accuracy["symbol_precision"].as_f64()),
                            ),
                        ),
                        compare_table_row(
                            "Semantic precision",
                            "Насколько точно семантический слой попадает в правильный контекст.",
                            compare_pair(
                                format_ratio_percent(
                                    thresholds["accuracy"]["semantic_precision"]["target"].as_f64(),
                                ),
                                format_ratio_percent(accuracy["semantic_precision"].as_f64()),
                            ),
                        ),
                    ],
                );
    if let Some(tooltip) = status_reason_tooltip(
        accuracy_status,
        accuracy_benchmark_reasons(accuracy, thresholds),
        "Accuracy / isolation contour вышел из своей нормы, но детальные причины пока не удалось собрать.",
    ) {
        accuracy_card = with_status_tooltip(accuracy_card, &tooltip);
    }

    let memory_benchmark = &snapshot["latest_memory_benchmark_score"]["memory_benchmark_score"];
    let memory_total_cases = memory_benchmark["summary"]["total"].as_u64().unwrap_or(0);
    let memory_missing_predictions = memory_benchmark["summary"]["missing_prediction"]
        .as_u64()
        .unwrap_or(0);
    let memory_overall_accuracy =
        memory_benchmark["capability_breakdown"]["longmemeval_overall_accuracy"].as_f64();
    let memory_abstention_accuracy =
        memory_benchmark["capability_breakdown"]["longmemeval_abstention_accuracy"].as_f64();
    let memory_false_answer_rate =
        memory_benchmark["capability_breakdown"]["longmemeval_false_answer_rate_on_abstention"]
            .as_f64();
    let memory_status = if memory_total_cases == 0 {
        "waiting"
    } else if memory_missing_predictions == 0
        && memory_overall_accuracy.unwrap_or(0.0) >= 0.95
        && memory_abstention_accuracy.unwrap_or(0.0) >= 0.95
        && memory_false_answer_rate.unwrap_or(1.0) <= 0.05
    {
        "pass"
    } else {
        "critical"
    };
    let mut memory_card = compare_table_card(
        "Память и изоляция",
        format!(
            "Контур данных: latest_memory_benchmark_score.memory_benchmark_score. Это отдельный benchmark score-lane для долговременной памяти и честного abstention-поведения. Online текущей сессии сюда не подмешивается. LongMemEval не имеет права исчезать из benchmark-раздела даже если результат плохой. {}",
            memory_benchmark["note"]
                .as_str()
                .unwrap_or("Подробный scorer note пока не materialized.")
        ),
        memory_status,
        Some(source_label(
            "Источник: benchmark snapshot latest_memory_benchmark_score.memory_benchmark_score. Live-данные страницы сюда не подмешиваются",
            snapshot["latest_memory_benchmark_score"]["_observability"]["captured_at_epoch_ms"]
                .as_u64(),
        )),
        Some("Показывает честный внешний benchmark памяти. Эта карточка должна surface-ить провал memory contour, а не скрывать его из benchmark plane.".to_string()),
        Some(if memory_total_cases == 0 {
            "ещё нет данных".to_string()
        } else {
            format!(
                "{} кейсов • overall {} • abstention {}",
                format_u64(Some(memory_total_cases)),
                format_ratio_percent(memory_overall_accuracy),
                format_ratio_percent(memory_abstention_accuracy),
            )
        }),
        vec![
            compare_table_row(
                "Bench",
                "Какой именно benchmark memory contour дал этот score snapshot.",
                compare_pair(
                    "LongMemEval".to_string(),
                    memory_benchmark["bench"]
                        .as_str()
                        .unwrap_or("ещё нет данных")
                        .to_string(),
                ),
            ),
            compare_table_row(
                "Dataset",
                "На каком benchmark dataset посчитан этот memory score.",
                compare_pair(
                    "memory benchmark dataset".to_string(),
                    memory_benchmark["dataset"]
                        .as_str()
                        .unwrap_or("ещё нет данных")
                        .to_string(),
                ),
            ),
            compare_table_row(
                "Кейсов",
                "Сколько benchmark-case реально должно было попасть в score. Именно это число обычно и видно как размер memory benchmark прогона.",
                compare_pair("полный набор".to_string(), format_u64(Some(memory_total_cases))),
            ),
            compare_table_row(
                "Overall accuracy",
                "Общая точность memory benchmark. Для сильной памяти это не может оставаться около нуля.",
                compare_pair("чем выше, тем лучше".to_string(), format_ratio_percent(memory_overall_accuracy)),
            ),
            compare_table_row(
                "Abstention accuracy",
                "Насколько честно система воздерживается там, где должна сказать \"не знаю\" вместо выдумки.",
                compare_pair("100.00%".to_string(), format_ratio_percent(memory_abstention_accuracy)),
            ),
            compare_table_row(
                "False answer on abstention",
                "Как часто вместо честного abstain система всё равно даёт ложный ответ.",
                compare_pair("0.00%".to_string(), format_ratio_percent(memory_false_answer_rate)),
            ),
            compare_table_row(
                "Missing predictions",
                "Сколько кейсов вообще не получили валидного предсказания в последнем memory benchmark прогоне.",
                compare_pair("= 0".to_string(), format_u64(memory_benchmark["summary"]["missing_prediction"].as_u64())),
            ),
            compare_table_row(
                "Expected abstentions",
                "Сколько кейсов в этом наборе специально проверяют честное воздержание.",
                compare_pair(
                    "контрольный набор".to_string(),
                    format_u64(memory_benchmark["summary"]["abstention_expected"].as_u64()),
                ),
            ),
        ],
    );
    if let Some(tooltip) = status_reason_tooltip(
        memory_status,
        vec![
            if memory_missing_predictions > 0 {
                Some(format!(
                    "Missing predictions слишком велики: {} из {} кейсов остались без валидного ответа.",
                    format_u64(Some(memory_missing_predictions)),
                    format_u64(Some(memory_total_cases))
                ))
            } else {
                None
            },
            memory_overall_accuracy.map(|value| {
                format!(
                    "Overall accuracy memory benchmark слишком низкая: сейчас {}.",
                    format_ratio_percent(Some(value))
                )
            }),
            memory_abstention_accuracy.map(|value| {
                format!(
                    "Abstention accuracy провалена: сейчас {}.",
                    format_ratio_percent(Some(value))
                )
            }),
            memory_false_answer_rate.map(|value| {
                format!(
                    "False answer rate на abstention-case слишком высок: сейчас {}.",
                    format_ratio_percent(Some(value))
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect(),
        "Memory benchmark вышел из своей нормы, но детальные причины пока не удалось собрать.",
    ) {
        memory_card = with_status_tooltip(memory_card, &tooltip);
    }

    let procedural = &snapshot["latest_procedural_benchmark"]["procedural_benchmark"];
    let procedural_total = procedural["summary"]["total_metrics"].as_u64().unwrap_or(0);
    let procedural_passed = procedural["summary"]["passed_metrics"]
        .as_u64()
        .unwrap_or(0);
    let procedural_percent = procedural["summary"]["pass_percent"].as_f64();
    let procedural_without_available = procedural["summary"]["without_amai_series_available"]
        .as_bool()
        .unwrap_or(false);
    let procedural_run_state = procedural["benchmark_run_state"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let procedural_run_state_ru = procedural["benchmark_run_state_ru"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let procedural_metric_kind = procedural["benchmark_metric_kind"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let procedural_runtime_contract =
        procedural["benchmark_run_passport"]["multi_platform_runtime_contract"]
            .as_str()
            .unwrap_or("ещё нет данных");
    let procedural_history = &snapshot["procedural_benchmark_history"];
    let procedural_history_count = procedural_history["history_count"].as_u64().unwrap_or(0);
    let procedural_with_history_count = procedural_history["with_amai_history_count"]
        .as_u64()
        .unwrap_or(0);
    let procedural_without_history_count = procedural_history["without_amai_history_count"]
        .as_u64()
        .unwrap_or(0);
    let procedural_with_series_count = procedural["benchmark_with_amai_series"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or(0);
    let procedural_without_series_count = procedural["benchmark_without_amai_series"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or(0);
    let procedural_with_summary = &procedural["benchmark_line_summaries"]["with_amai"];
    let procedural_without_summary =
        &procedural["benchmark_line_summaries"]["without_amai_but_measuring"];
    let procedural_status = if procedural_total == 0 {
        "waiting"
    } else if !procedural_without_available {
        "waiting"
    } else if procedural_passed == procedural_total {
        "pass"
    } else {
        "critical"
    };
    let mut procedural_rows = vec![
        compare_table_row(
            "Metric kind",
            "Какой именно тип benchmark-метрик показывает карточка. Для procedural contour здесь не может быть generic score.",
            compare_pair(
                "procedural_skill_metrics".to_string(),
                procedural_metric_kind.to_string(),
            ),
        ),
        compare_table_row(
            "Run state",
            "Какой именно benchmark-state сейчас materialized. Если вторая линия ещё не готова, карточка обязана показывать partial compare-state, а не притворяться completed compare.",
            compare_pair(
                "honest compare state".to_string(),
                format!("{procedural_run_state} ({procedural_run_state_ru})"),
            ),
        ),
        compare_table_row(
            "Линия с Amai",
            "Сколько точек уже materialized в benchmark_with_amai_series. Это benchmark lane, а не online series текущего чата.",
            compare_pair(
                ">= 1".to_string(),
                format_u64(Some(procedural_with_series_count as u64)),
            ),
        ),
        compare_table_row(
            "Статус линии с Amai",
            "Сверяет benchmark_line_summaries.with_amai со series count. Это fail-closed слой для честного compare payload.",
            compare_pair(
                "materialized".to_string(),
                procedural_with_summary["state"]
                    .as_str()
                    .unwrap_or("ещё нет данных")
                    .to_string(),
            ),
        ),
        compare_table_row(
            "Линия без Amai",
            "Если честная линия без Amai ещё не materialized, карточка обязана сказать это прямо и не рисовать guessed compare.",
            compare_pair(
                if procedural_without_available {
                    ">= 1".to_string()
                } else {
                    "ещё не materialized".to_string()
                },
                if procedural_without_available {
                    format_u64(Some(procedural_without_series_count as u64))
                } else {
                    "не рисуется честно".to_string()
                },
            ),
        ),
        compare_table_row(
            "Статус линии без Amai",
            "Сверяет benchmark_line_summaries.without_amai_but_measuring с наличием второй линии. Пока bypass contour не materialized, здесь должен быть not_materialized.",
            compare_pair(
                if procedural_without_available {
                    "materialized".to_string()
                } else {
                    "not_materialized".to_string()
                },
                procedural_without_summary["state"]
                    .as_str()
                    .unwrap_or("ещё нет данных")
                    .to_string(),
            ),
        ),
        compare_table_row(
            "Runtime contract",
            "Показывает, что benchmark payload сохраняет platform-neutral runtime contract и не завязан смыслом на одну host-платформу.",
            compare_pair(
                "platform-neutral benchmark snapshot".to_string(),
                procedural_runtime_contract.to_string(),
            ),
        ),
        compare_table_row(
            "История benchmark",
            "Сколько immutable benchmark snapshots уже накоплено для procedural compare-plane. Это persisted history, а не live online lane.",
            compare_pair(
                ">= 1".to_string(),
                format_u64(Some(procedural_history_count)),
            ),
        ),
        compare_table_row(
            "История с Amai",
            "Сколько history-points уже есть в persisted time-series для линии с Amai.",
            compare_pair(
                ">= 1".to_string(),
                format_u64(Some(procedural_with_history_count)),
            ),
        ),
        compare_table_row(
            "История без Amai",
            "Сколько history-points уже есть в persisted time-series для линии without_amai_but_measuring.",
            compare_pair(
                if procedural_without_available {
                    ">= 1".to_string()
                } else {
                    "0".to_string()
                },
                format_u64(Some(procedural_without_history_count)),
            ),
        ),
    ];
    procedural_rows.extend(
        procedural["procedural_metrics"]
        .as_array()
        .map(|items| {
            items.iter()
                .map(|item| {
                    compare_table_row(
                        item["label_ru"].as_str().unwrap_or("ещё нет данных"),
                        item["tooltip_ru"].as_str().unwrap_or(
                            "Какая именно procedural skill-метрика проверялась в benchmark contour.",
                        ),
                        compare_pair(
                            "должно пройти".to_string(),
                            if item["passed"].as_bool() == Some(true) {
                                "pass".to_string()
                            } else {
                                "fail".to_string()
                            },
                        ),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default(),
    );
    let mut procedural_card = compare_table_card(
        "Навыки и память действий",
        format!(
            "Контур данных: latest_procedural_benchmark.procedural_benchmark. Это отдельный benchmark quality-lane для procedural memory. Online текущей сессии сюда не подмешивается, а generic memory score запрещён: карточка показывает именно skill-метрики reuse/suppression/uplift/evaluator. {}",
            if procedural_without_available {
                "Линия без Amai materialized отдельно и не смешивается с online lane."
            } else {
                "Линия без Amai ещё не materialized, поэтому карточка честно не рисует guessed second line."
            }
        ),
        procedural_status,
        Some(source_label(
            "Источник: benchmark snapshot latest_procedural_benchmark.procedural_benchmark. Live-данные страницы сюда не подмешиваются",
            snapshot["latest_procedural_benchmark"]["captured_at_epoch_ms"].as_u64(),
        )),
        Some(
            "Показывает procedural benchmark как набор отдельных skill-метрик. Эта карточка не имеет права схлопывать reuse/suppression/uplift в безликий общий memory score.".to_string(),
        ),
        Some(if procedural_total == 0 {
            "ещё нет данных".to_string()
        } else if !procedural_without_available {
            format!(
                "{} из {} метрик подтверждены с Amai ({}); линия без Amai ещё не materialized",
                format_u64(Some(procedural_passed)),
                format_u64(Some(procedural_total)),
                format_percent(procedural_percent)
            )
        } else {
            format!(
                "{} из {} skill-метрик подтверждены с Amai ({}); линия без Amai materialized отдельно",
                format_u64(Some(procedural_passed)),
                format_u64(Some(procedural_total)),
                format_percent(procedural_percent)
            )
        }),
        procedural_rows,
    );
    if procedural_total == 0 {
        procedural_card = with_status_label(procedural_card, "ждём procedural benchmark");
    }
    if let Some(object) = procedural_card.as_object_mut() {
        object.insert(
            "benchmark_metric_kind".to_string(),
            Value::from(procedural_metric_kind),
        );
        object.insert(
            "benchmark_run_state".to_string(),
            Value::from(procedural_run_state),
        );
        object.insert(
            "benchmark_run_state_ru".to_string(),
            Value::from(procedural_run_state_ru),
        );
        object.insert(
            "benchmark_with_amai_series".to_string(),
            procedural["benchmark_with_amai_series"].clone(),
        );
        object.insert(
            "benchmark_without_amai_series".to_string(),
            procedural["benchmark_without_amai_series"].clone(),
        );
        object.insert(
            "without_amai_series_available".to_string(),
            Value::Bool(procedural_without_available),
        );
        object.insert(
            "benchmark_line_summaries".to_string(),
            procedural["benchmark_line_summaries"].clone(),
        );
        object.insert(
            "multi_platform_runtime_contract".to_string(),
            Value::from(procedural_runtime_contract),
        );
        object.insert(
            "procedural_benchmark_history".to_string(),
            procedural_history.clone(),
        );
        object.insert(
            "benchmark_with_amai_history_series".to_string(),
            procedural_history["with_amai_pass_percent_series"].clone(),
        );
        object.insert(
            "benchmark_without_amai_history_series".to_string(),
            procedural_history["without_amai_pass_percent_series"].clone(),
        );
    }

    vec![
        hot_load_card,
        hot_retrieval_card,
        cold_card,
        with_table_orientation(
            with_extra_class(accuracy_card, "benchmark-span-full"),
            "transposed",
        ),
        memory_card,
        procedural_card,
    ]
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

fn compare_table_card(
    title: &str,
    note: String,
    status: &str,
    source_label: Option<String>,
    title_tooltip: Option<String>,
    headline_value: Option<String>,
    rows: Vec<Value>,
) -> Value {
    json!({
        "kind": "compare_table",
        "title": title,
        "note": note,
        "status": status,
        "status_label": status_label(status),
        "status_tooltip": Value::Null,
        "source_label": source_label,
        "title_tooltip": title_tooltip,
        "headline_value": headline_value,
        "metrics": [],
        "table": {
            "columns": [
                { "label": "Метрика", "tooltip": "Что именно измерялось в этом проверочном прогоне." },
                { "label": "Эталон", "tooltip": "Фиксированная целевая планка. Она не зависит от текущей сессии и не меняется от запроса к запросу." },
                { "label": "Тестовые\nданные", "tooltip": "Фактический результат последнего сохранённого benchmark-прогона." }
            ],
            "rows": rows,
        }
    })
}

fn compare_table_row(label: &str, tooltip: &str, values: Vec<String>) -> Value {
    json!({
        "label": label,
        "tooltip": tooltip,
        "values": values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
