use super::*;

enum ServiceSourceKind<'a> {
    LiveProbe {
        system: &'a str,
    },
    LiveMetrics {
        system: &'a str,
    },
    LiveSnapshot {
        field: &'a str,
    },
    DurableArtifacts {
        detail: &'a str,
    },
    ExternalBenchmarkWorkspace {
        mode: ExternalBenchmarkWorkspaceMode<'a>,
    },
}

enum ExternalBenchmarkWorkspaceMode<'a> {
    LiveWithWorkspace { endpoint: &'a str },
    LastRunWorkspace { endpoint: &'a str },
    SavedResultAndSlice { endpoint: &'a str },
    ExternalOnly,
}

fn service_source_sentence(source: ServiceSourceKind<'_>) -> String {
    match source {
        ServiceSourceKind::LiveProbe { system } => {
            format!("Источник: живой {system} probe, обновляется на каждом refresh dashboard")
        }
        ServiceSourceKind::LiveMetrics { system } => {
            format!("Источник: live {system}, обновляется на каждом refresh dashboard")
        }
        ServiceSourceKind::LiveSnapshot { field } => {
            format!("Источник: {field} из live snapshot.")
        }
        ServiceSourceKind::DurableArtifacts { detail } => {
            format!("Источник: durable {detail}.")
        }
        ServiceSourceKind::ExternalBenchmarkWorkspace { mode } => match mode {
            ExternalBenchmarkWorkspaceMode::LiveWithWorkspace { endpoint } => format!(
                "Источник: live Qdrant /metrics + workspace последнего внешнего прогона ({endpoint}). Карточка обновляется при refresh dashboard."
            ),
            ExternalBenchmarkWorkspaceMode::LastRunWorkspace { endpoint } => format!(
                "Источник: workspace последнего внешнего прогона + последний известный срез benchmark-Qdrant ({endpoint})."
            ),
            ExternalBenchmarkWorkspaceMode::SavedResultAndSlice { endpoint } => format!(
                "Источник: последний сохранённый результат и срез benchmark-Qdrant ({endpoint})."
            ),
            ExternalBenchmarkWorkspaceMode::ExternalOnly => {
                "Источник: отдельный benchmark-Qdrant и workspace внешнего benchmark-прогона. Эта карточка не берёт данные из Amai live.".to_string()
            }
        },
    }
}

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
        Some(service_source_sentence(ServiceSourceKind::LiveProbe {
            system: "PostgreSQL",
        })),
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
        Some(service_source_sentence(ServiceSourceKind::LiveMetrics {
            system: "Qdrant /metrics Amai",
        })),
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
        Some(service_source_sentence(ServiceSourceKind::LiveProbe {
            system: "NATS/JetStream",
        })),
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

    let remediation_card = build_remediation_inbox_card();

    vec![
        postgres_card,
        qdrant_live_card,
        benchmark_qdrant_card,
        nats_card,
        remediation_card,
        build_capacity_forecast_card(snapshot),
        build_governance_card(snapshot),
        build_regression_explain_card(snapshot),
    ]
}

fn build_remediation_inbox_card() -> Value {
    let repo_root = config::discover_repo_root(None)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let bundle_dir = crate::indexer::qdrant_postgres_remediation_bundle_dir(&repo_root);
    let summary = collect_remediation_inbox_summary(&bundle_dir);
    let status = if summary.invalid_items > 0 {
        "critical"
    } else if summary.total_items > 0 {
        "alert"
    } else {
        "pass"
    };
    let value = if summary.invalid_items > 0 {
        format!(
            "{} incident bundle(s), {} invalid artifact(s)",
            summary.total_items, summary.invalid_items
        )
    } else if summary.total_items > 0 {
        format!(
            "{} incident bundle(s) требуют просмотра",
            summary.total_items
        )
    } else {
        "открытых remediation bundles нет".to_string()
    };
    let note = if summary.total_items > 0 {
        "Read-only operator inbox для cross-store manual recovery: только inspect path, без retry и reconcile semantics.".to_string()
    } else {
        "Read-only operator inbox пуст: новых qdrant/postgres manual recovery bundles сейчас не видно.".to_string()
    };
    let latest_created_label = summary
        .latest_created_at_epoch_ms
        .map(human_timestamp)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let mut details = vec![
        format!(
            "Inspect API: /api/remediation-bundles?limit={}",
            summary.suggested_limit
        ),
        format!("Bundle dir: {}", bundle_dir.display()),
    ];
    if summary.total_items > 0 {
        details.push(
            "Surface остаётся read-only: решение и reconcile operator делает отдельно, не из dashboard."
                .to_string(),
        );
    }
    let mut card = card_with_rows(
        "Remediation inbox",
        value,
        note,
        status,
        Some(
            service_source_sentence(ServiceSourceKind::DurableArtifacts {
                detail: "remediation bundle artifacts из state/incidents/qdrant-postgres-remediation",
            }),
        ),
        Some(
            "Этот surface показывает только operator-facing incident inbox и не делает qdrant/postgres recovery автоматически."
                .to_string(),
        ),
        vec![
            metric_row(
                "Открытых bundles",
                summary.total_items.to_string(),
                Some("Сколько remediation bundle artifacts сейчас видно в durable inbox."),
            ),
            metric_row(
                "Invalid artifacts",
                summary.invalid_items.to_string(),
                Some("Сколько artifacts не читаются или не проходят минимальную schema/semantic validation."),
            ),
            metric_row(
                "Последний bundle",
                latest_created_label,
                Some("Время newest remediation artifact по created_at_epoch_ms."),
            ),
        ],
    );
    if let Some(root) = card.as_object_mut() {
        root.insert("details".to_string(), json!(details));
    }
    card
}

#[derive(Debug, Default)]
struct RemediationInboxSummary {
    total_items: usize,
    invalid_items: usize,
    latest_created_at_epoch_ms: Option<u64>,
    suggested_limit: usize,
}

fn collect_remediation_inbox_summary(bundle_dir: &Path) -> RemediationInboxSummary {
    let mut summary = RemediationInboxSummary {
        suggested_limit: 20,
        ..RemediationInboxSummary::default()
    };
    let Ok(entries) = std::fs::read_dir(bundle_dir) else {
        return summary;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        summary.total_items += 1;
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        {
            Some(payload)
                if payload["artifact_version"]
                    == json!(
                        crate::indexer::QDRANT_POSTGRES_REMEDIATION_BUNDLE_ARTIFACT_VERSION
                    )
                    && remediation_payload_has_required_dashboard_fields(&payload) =>
            {
                let created_at = payload["created_at_epoch_ms"].as_u64();
                summary.latest_created_at_epoch_ms =
                    std::cmp::max(summary.latest_created_at_epoch_ms, created_at);
            }
            _ => {
                summary.invalid_items += 1;
            }
        }
    }
    summary
}

fn remediation_payload_has_required_dashboard_fields(payload: &Value) -> bool {
    [
        "bundle_id",
        "relative_path",
        "document_id",
        "failure_mode",
        "failure_phase",
        "consistency_state",
        "required_action",
        "operator_summary",
        "observability_stage",
    ]
    .into_iter()
    .all(|field| payload[field].as_str().is_some())
        && payload["created_at_epoch_ms"].as_u64().is_some()
        && payload["had_existing_document"].as_bool().is_some()
        && payload["compensation_attempted"].as_bool().is_some()
        && payload["operator_checklist"].is_array()
}

pub(crate) fn build_capacity_forecast_card(snapshot: &Value) -> Value {
    let forecast = &snapshot["capacity_forecast"];
    if !forecast.is_object() {
        return card_with_rows(
            "Capacity forecast",
            "ещё нет данных".to_string(),
            "Queue 5 capacity/arrival contour ещё не surfaced в live snapshot.".to_string(),
            "unknown",
            Some(service_source_sentence(ServiceSourceKind::LiveSnapshot {
                field: "capacity_forecast",
            })),
            Some(
                "Forecast-only surface для arrival pressure и пропускной способности без runtime authority."
                    .to_string(),
            ),
            vec![],
        );
    }
    let family = forecast["families"]
        .as_array()
        .and_then(|families| families.first());
    let Some(family) = family else {
        return card_with_rows(
            "Capacity forecast",
            "ещё нет данных".to_string(),
            "В live snapshot пока нет ни одного supported family для Queue 5.".to_string(),
            "unknown",
            Some(service_source_sentence(ServiceSourceKind::LiveSnapshot {
                field: "capacity_forecast",
            })),
            Some(
                "Forecast-only surface для arrival pressure и пропускной способности без runtime authority."
                    .to_string(),
            ),
            vec![],
        );
    };
    let windows = family["windows"].as_array().cloned().unwrap_or_default();
    let measured_window = windows
        .iter()
        .find(|item| item["window_key"].as_str() == Some("5m"))
        .or_else(|| {
            windows
                .iter()
                .find(|item| item["status"].as_str() == Some("measured"))
        });
    let headline = measured_window
        .map(|window| {
            format!(
                "5м λ {} • запас {}",
                format_optional(window["lambda"].as_f64(), |value| format!("{value:.2}/s")),
                format_optional(window["capacity_margin"].as_f64(), |value| format!(
                    "{value:.2}/s"
                )),
            )
        })
        .unwrap_or_else(|| {
            let insufficient = forecast["summary"]["insufficient_families"]
                .as_u64()
                .unwrap_or(0);
            format!(
                "0 measured • {} insufficient",
                format_u64(Some(insufficient))
            )
        });
    let status = measured_window
        .and_then(|window| window["capacity_margin"].as_f64())
        .map(|margin| if margin >= 0.0 { "pass" } else { "warning" })
        .unwrap_or("unknown");
    let mut rows = Vec::new();
    for window_key in ["1m", "5m"] {
        if let Some(window) = windows
            .iter()
            .find(|item| item["window_key"].as_str() == Some(window_key))
        {
            let value = if window["status"].as_str() == Some("measured") {
                format!(
                    "λ {} • запас {} • n={}",
                    format_optional(window["lambda"].as_f64(), |value| format!("{value:.2}/s")),
                    format_optional(window["capacity_margin"].as_f64(), |value| format!(
                        "{value:.2}/s"
                    )),
                    format_u64(window["sample_count"].as_u64())
                )
            } else {
                format!(
                    "insufficient • span {} • n={}",
                    format_optional(window["observed_span_seconds"].as_f64(), |value| format!(
                        "{value:.0}s"
                    )),
                    format_u64(window["sample_count"].as_u64())
                )
            };
            rows.push(metric_row(
                &format!("Окно {window_key}"),
                value,
                Some("Forecast-only оценка arrivals и service rate по history system_snapshot."),
            ));
        }
    }
    rows.push(metric_row(
        "History scope",
        humanize_identifier(
            forecast["history_scope"]["mode"]
                .as_str()
                .unwrap_or("unknown"),
        ),
        Some("Откуда взята history для расчёта Queue 5."),
    ));
    card_with_rows(
        "Capacity forecast",
        headline,
        "Forecast-only contour для arrival pressure в NATS event plane без runtime enforcement."
            .to_string(),
        status,
        Some(service_source_sentence(ServiceSourceKind::LiveSnapshot {
            field: "capacity_forecast",
        })),
        Some(
            "Read-only capacity surface. Не authority для throttling, routing или truth claims."
                .to_string(),
        ),
        rows,
    )
}

pub(crate) fn build_regression_explain_card(snapshot: &Value) -> Value {
    let explain = &snapshot["regression_explain"];
    if !explain.is_object() {
        return card_with_rows(
            "Regression explain",
            "ещё нет данных".to_string(),
            "Queue 4 explainability contour ещё не surfaced в live snapshot.".to_string(),
            "unknown",
            Some(service_source_sentence(ServiceSourceKind::LiveSnapshot {
                field: "regression_explain",
            })),
            Some(
                "Read-only explain surface для helpful/stale/benchmark outcomes без routing или truth authority."
                    .to_string(),
            ),
            vec![],
        );
    }
    let outcomes = explain["outcomes"].as_array().cloned().unwrap_or_default();
    let measured = explain["summary"]["measured_outcomes"]
        .as_u64()
        .unwrap_or(0);
    let insufficient = explain["summary"]["insufficient_sample_outcomes"]
        .as_u64()
        .unwrap_or(0);
    let status = if measured > 0 { "pass" } else { "unknown" };
    let headline = format!(
        "{} measured • {} insufficient",
        format_u64(Some(measured)),
        format_u64(Some(insufficient))
    );
    let mut rows = Vec::new();
    for key in ["benchmark_pass", "stale_error", "retrieval_helpful"] {
        let maybe_outcome = outcomes
            .iter()
            .find(|item| item["outcome_key"].as_str() == Some(key));
        if let Some(outcome) = maybe_outcome {
            let label = outcome["title"].as_str().unwrap_or(key);
            let value = match outcome["status"].as_str().unwrap_or("unknown") {
                "measured" => format!(
                    "AUC {} • n={}",
                    format_optional(outcome["auc"].as_f64(), |v| format!("{v:.3}")),
                    format_u64(outcome["sample_size"].as_u64())
                ),
                "insufficient_sample" => format!(
                    "insufficient • n={} • +={}",
                    format_u64(outcome["sample_size"].as_u64()),
                    format_u64(outcome["positive_count"].as_u64())
                ),
                _ => "not materialized".to_string(),
            };
            rows.push(metric_row(
                label,
                value,
                Some(
                    "Queue 4 read-only explain contour. Метрика не имеет routing/truth authority.",
                ),
            ));
        }
    }
    card_with_rows(
        "Regression explain",
        headline,
        "Карточка показывает, какие explanatory модели уже честно materialized поверх live snapshot surfaces. Если outcome пустой или одноклассный, surface обязан явно показать insufficient sample."
            .to_string(),
        status,
        Some(service_source_sentence(ServiceSourceKind::LiveSnapshot {
            field: "regression_explain",
        })),
        Some(
            "Queue 4: logistic-regression explain surface с quality metrics, coefficient table и fail-closed insufficient-sample semantics."
                .to_string(),
        ),
        rows,
    )
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
        Some(service_source_sentence(
            ServiceSourceKind::ExternalBenchmarkWorkspace {
                mode: ExternalBenchmarkWorkspaceMode::LiveWithWorkspace {
                    endpoint: benchmark["http_url"].as_str().unwrap_or("unknown"),
                },
            },
        ))
    } else if run_state == "finished_ok"
        || run_state == "finished_error"
        || run_state == "finished_benchmark_failed"
    {
        Some(service_source_sentence(
            ServiceSourceKind::ExternalBenchmarkWorkspace {
                mode: ExternalBenchmarkWorkspaceMode::LastRunWorkspace {
                    endpoint: benchmark["http_url"].as_str().unwrap_or("unknown"),
                },
            },
        ))
    } else if from_last_success {
        Some(service_source_sentence(
            ServiceSourceKind::ExternalBenchmarkWorkspace {
                mode: ExternalBenchmarkWorkspaceMode::SavedResultAndSlice {
                    endpoint: benchmark["http_url"].as_str().unwrap_or("unknown"),
                },
            },
        ))
    } else {
        Some(service_source_sentence(
            ServiceSourceKind::ExternalBenchmarkWorkspace {
                mode: ExternalBenchmarkWorkspaceMode::ExternalOnly,
            },
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn service_source_sentence_renders_live_snapshot_and_durable_artifacts() {
        let live_snapshot = service_source_sentence(ServiceSourceKind::LiveSnapshot {
            field: "capacity_forecast",
        });
        let live_probe = service_source_sentence(ServiceSourceKind::LiveProbe {
            system: "PostgreSQL",
        });
        let live_metrics = service_source_sentence(ServiceSourceKind::LiveMetrics {
            system: "Qdrant /metrics Amai",
        });
        let durable = service_source_sentence(ServiceSourceKind::DurableArtifacts {
            detail: "remediation bundle artifacts из state/incidents/qdrant-postgres-remediation",
        });

        assert_eq!(
            live_snapshot,
            "Источник: capacity_forecast из live snapshot."
        );
        assert_eq!(
            live_probe,
            "Источник: живой PostgreSQL probe, обновляется на каждом refresh dashboard"
        );
        assert_eq!(
            live_metrics,
            "Источник: live Qdrant /metrics Amai, обновляется на каждом refresh dashboard"
        );
        assert!(durable.contains("Источник: durable remediation bundle artifacts"));
    }

    #[test]
    fn service_source_sentence_renders_external_benchmark_workspace_variants() {
        let live = service_source_sentence(ServiceSourceKind::ExternalBenchmarkWorkspace {
            mode: ExternalBenchmarkWorkspaceMode::LiveWithWorkspace {
                endpoint: "http://127.0.0.1:7633",
            },
        });
        let external_only =
            service_source_sentence(ServiceSourceKind::ExternalBenchmarkWorkspace {
                mode: ExternalBenchmarkWorkspaceMode::ExternalOnly,
            });

        assert!(live.contains("live Qdrant /metrics + workspace последнего внешнего прогона"));
        assert!(live.contains("http://127.0.0.1:7633"));
        assert!(external_only.contains("Эта карточка не берёт данные из Amai live"));
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
        assert!(card["source_label"].as_str().unwrap_or_default().contains(
            "workspace последнего внешнего прогона + последний известный срез benchmark-Qdrant"
        ));
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
        assert!(titles.contains(&"Remediation inbox"));
        assert!(titles.contains(&"Жизненный цикл памяти"));
        assert!(!titles.contains(&"Поведение при сбоях"));
        assert!(!titles.contains(&"Правильное продолжение"));
        let postgres = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("PostgreSQL"))
            .expect("postgres card");
        assert_eq!(
            postgres["source_label"].as_str(),
            Some("Источник: живой PostgreSQL probe, обновляется на каждом refresh dashboard")
        );
        let forecast = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Capacity forecast"))
            .expect("capacity forecast card");
        assert_eq!(
            forecast["source_label"].as_str(),
            Some("Источник: capacity_forecast из live snapshot.")
        );
    }

    #[test]
    fn remediation_inbox_card_surfaces_open_and_invalid_bundles() {
        let bundle_dir =
            std::env::temp_dir().join(format!("amai-dashboard-remediation-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&bundle_dir).expect("create remediation dir");
        std::fs::write(
            bundle_dir.join("200_open.json"),
            serde_json::to_string_pretty(&json!({
                "artifact_version": crate::indexer::QDRANT_POSTGRES_REMEDIATION_BUNDLE_ARTIFACT_VERSION,
                "bundle_id": "bundle-open",
                "created_at_epoch_ms": 200,
                "relative_path": "src/open.rs",
                "document_id": "00000000-0000-0000-0000-000000000010",
                "had_existing_document": true,
                "failure_mode": "existing_document_inconsistent_state",
                "failure_phase": "before_commit",
                "consistency_state": "cross_store_inconsistent_after_compensation_failure",
                "required_action": "manual_cross_store_investigation_required",
                "operator_summary": "manual recovery required",
                "operator_checklist": ["inspect"],
                "compensation_attempted": true,
                "observability_stage": "index_project.qdrant_postgres_failure_verdict"
            }))
            .expect("serialize remediation bundle"),
        )
        .expect("write remediation bundle");
        std::fs::write(bundle_dir.join("300_invalid.json"), "{")
            .expect("write invalid remediation bundle");
        let dir_value = bundle_dir.display().to_string();
        unsafe {
            std::env::set_var("AMAI_QDRANT_POSTGRES_REMEDIATION_DIR", &dir_value);
        }

        let card = build_remediation_inbox_card();
        assert_eq!(card["status"].as_str(), Some("critical"));
        assert!(
            card["value"]
                .as_str()
                .is_some_and(|value| value.contains("invalid artifact"))
        );
        assert_eq!(card["rows"][0]["value"].as_str(), Some("2"));
        assert_eq!(card["rows"][1]["value"].as_str(), Some("1"));
        assert!(
            card["details"]
                .as_array()
                .is_some_and(|details| details.iter().any(|item| {
                    item.as_str()
                        .is_some_and(|text| text.contains("/api/remediation-bundles"))
                }))
        );

        unsafe {
            std::env::remove_var("AMAI_QDRANT_POSTGRES_REMEDIATION_DIR");
        }
        std::fs::remove_dir_all(&bundle_dir).expect("cleanup remediation dir");
    }

    #[test]
    fn capacity_forecast_card_surfaces_missing_family_as_no_data() {
        let snapshot = json!({
            "capacity_forecast": {
                "summary": {
                    "status": "unknown",
                    "measured_families": 0,
                    "insufficient_families": 0
                },
                "families": []
            }
        });

        let card = build_capacity_forecast_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(card["value"].as_str(), Some("ещё нет данных"));
        assert_eq!(
            card["source_label"].as_str(),
            Some("Источник: capacity_forecast из live snapshot.")
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("нет ни одного supported family")
        );
    }

    #[test]
    fn capacity_forecast_card_prefers_measured_five_minute_window_for_headline() {
        let snapshot = json!({
            "capacity_forecast": {
                "summary": {
                    "status": "pass",
                    "measured_families": 1,
                    "insufficient_families": 0
                },
                "history_scope": {
                    "mode": "project_scoped_observe_history"
                },
                "families": [{
                    "family_key": "nats_events",
                    "status": "measured",
                    "windows": [
                        {
                            "window_key": "1m",
                            "status": "insufficient_sample",
                            "sample_count": 1,
                            "observed_span_seconds": 30.0
                        },
                        {
                            "window_key": "5m",
                            "status": "measured",
                            "sample_count": 6,
                            "lambda": 2.5,
                            "capacity_margin": 0.75
                        }
                    ]
                }]
            }
        });

        let card = build_capacity_forecast_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        assert_eq!(card["value"].as_str(), Some("5м λ 2.50/s • запас 0.75/s"));
        let empty_rows = Vec::new();
        let rows = card["rows"].as_array().unwrap_or(&empty_rows);
        let five_minute_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Окно 5m"))
            .expect("5m row");
        assert!(
            five_minute_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("λ 2.50/s • запас 0.75/s • n=6")
        );
        let scope_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("History scope"))
            .expect("history scope row");
        assert_eq!(
            scope_row["value"].as_str(),
            Some("Project Scoped Observe History")
        );
    }

    #[test]
    fn capacity_forecast_card_handles_family_without_windows_fail_closed() {
        let snapshot = json!({
            "capacity_forecast": {
                "summary": {
                    "status": "unknown",
                    "measured_families": 0,
                    "insufficient_families": 1
                },
                "history_scope": {
                    "mode": "project_scoped_observe_history"
                },
                "families": [{
                    "family_key": "nats_events",
                    "status": "insufficient_sample",
                    "windows": []
                }]
            }
        });

        let card = build_capacity_forecast_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(card["value"].as_str(), Some("0 measured • 1 insufficient"));
        let empty_rows = Vec::new();
        let rows = card["rows"].as_array().unwrap_or(&empty_rows);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["label"].as_str(), Some("History scope"));
        assert_eq!(
            rows[0]["value"].as_str(),
            Some("Project Scoped Observe History")
        );
    }

    #[test]
    fn regression_explain_card_surfaces_insufficient_sample_outcomes() {
        let snapshot = json!({
            "regression_explain": {
                "summary": {
                    "status": "unknown",
                    "measured_outcomes": 0,
                    "insufficient_sample_outcomes": 3
                },
                "outcomes": [
                    {
                        "outcome_key": "benchmark_pass",
                        "title": "Benchmark pass",
                        "status": "insufficient_sample",
                        "sample_size": 17,
                        "positive_count": 17
                    },
                    {
                        "outcome_key": "stale_error",
                        "title": "Stale error",
                        "status": "insufficient_sample",
                        "sample_size": 31,
                        "positive_count": 0
                    },
                    {
                        "outcome_key": "retrieval_helpful",
                        "title": "Retrieval helpful",
                        "status": "insufficient_sample",
                        "sample_size": 31,
                        "positive_count": 31
                    }
                ]
            }
        });

        let card = build_regression_explain_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(card["value"].as_str(), Some("0 measured • 3 insufficient"));
        assert_eq!(
            card["source_label"].as_str(),
            Some("Источник: regression_explain из live snapshot.")
        );
        let empty_rows = Vec::new();
        let rows = card["rows"].as_array().unwrap_or(&empty_rows);
        assert_eq!(rows.len(), 3);
        for (label, expected) in [
            ("Benchmark pass", "insufficient • n=17 • +=17"),
            ("Stale error", "insufficient • n=31 • +=0"),
            ("Retrieval helpful", "insufficient • n=31 • +=31"),
        ] {
            let row = rows
                .iter()
                .find(|row| row["label"].as_str() == Some(label))
                .unwrap_or_else(|| panic!("missing row for {label}"));
            assert_eq!(row["value"].as_str(), Some(expected));
        }
    }

    #[test]
    fn regression_explain_card_omits_unrelated_outcomes_fail_closed() {
        let snapshot = json!({
            "regression_explain": {
                "summary": {
                    "status": "unknown",
                    "measured_outcomes": 0,
                    "insufficient_sample_outcomes": 1
                },
                "outcomes": [
                    {
                        "outcome_key": "some_future_metric",
                        "title": "Future metric",
                        "status": "measured",
                        "sample_size": 9,
                        "auc": 0.91
                    }
                ]
            }
        });

        let card = build_regression_explain_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("unknown"));
        assert_eq!(card["value"].as_str(), Some("0 measured • 1 insufficient"));
        let empty_rows = Vec::new();
        let rows = card["rows"].as_array().unwrap_or(&empty_rows);
        assert!(rows.is_empty());
    }
}
