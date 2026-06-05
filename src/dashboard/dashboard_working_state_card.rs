use super::dashboard_live_response_latency_support::{
    live_response_latency_current_thread_file_hints, token_budget_report_root,
};
use super::*;

fn plain_working_state_scope(restore: &Value) -> String {
    let project = restore["project"]["display_name"]
        .as_str()
        .or_else(|| restore["project"]["code"].as_str())
        .unwrap_or("этот проект");
    let namespace = match restore["namespace"]["display_name"]
        .as_str()
        .or_else(|| restore["namespace"]["code"].as_str())
    {
        Some("default") | None => None,
        Some("continuity") => Some("continuity"),
        Some(value) => Some(value),
    };
    match namespace {
        Some(namespace) => format!("{project} / {namespace}"),
        None => project.to_string(),
    }
}

fn summarize_working_state_command(value: Option<&str>) -> String {
    let raw = value.map(str::trim).filter(|value| !value.is_empty());
    let Some(raw) = raw else {
        return "ещё нет данных".to_string();
    };
    let lowered = raw.to_ascii_lowercase();
    if lowered.contains("dashboard client-budget-host-control-feedback") {
        return "подтверждено действие в окне чата".to_string();
    }
    if lowered.contains("continuity handoff") {
        return "сохранена рабочая сводка".to_string();
    }
    if lowered.contains("context pack") {
        return "собран пакет контекста".to_string();
    }
    if lowered.contains("observe snapshot") {
        return "обновлён снимок состояния".to_string();
    }
    if lowered.contains("proof_") {
        return "запущена проверка".to_string();
    }
    compact_dashboard_text(Some(&humanize_identifier(raw)), 72, "ещё нет данных")
}

fn summarize_working_state_result(value: Option<&str>) -> String {
    let raw = value.map(str::trim).filter(|value| !value.is_empty());
    let Some(raw) = raw else {
        return "ещё нет данных".to_string();
    };
    if raw.contains("Operator confirmed same-thread compact window opened.") {
        return "подтверждён переход в компактный режим".to_string();
    }
    if raw.contains("Operator confirmed same-thread overlay opened.") {
        return "подтверждено открытие панели текущего чата".to_string();
    }
    compact_dashboard_text(Some(raw), 108, "ещё нет данных")
}

fn working_state_task_graph_projection_status(restore: &Value) -> Option<&str> {
    restore["task_graph_projection_validation"]["status"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn working_state_task_graph_projection_is_validated(restore: &Value) -> bool {
    let validation = &restore["task_graph_projection_validation"];
    validation["status"].as_str() == Some("valid")
        && validation["validation_state"].as_str() == Some("valid")
        && validation["projection_source"].as_str() == Some("graph_first_sql_validated")
        && validation["truth_claim"].as_bool() == Some(false)
}

fn russian_count_form<'a>(
    count: u64,
    singular: &'a str,
    paucal: &'a str,
    plural: &'a str,
) -> &'a str {
    let remainder_10 = count % 10;
    let remainder_100 = count % 100;
    if remainder_10 == 1 && remainder_100 != 11 {
        singular
    } else if (2..=4).contains(&remainder_10) && !(12..=14).contains(&remainder_100) {
        paucal
    } else {
        plural
    }
}

fn working_state_task_graph_legacy_debt_summary(validation: &Value) -> Option<String> {
    let deprecated = validation["deprecated_sql_nodes_count"]
        .as_u64()
        .unwrap_or(0);
    let quarantined = validation["quarantined_sql_nodes_count"]
        .as_u64()
        .unwrap_or(0);
    let excluded_total = validation["projection_excluded_sql_nodes_count"]
        .as_u64()
        .unwrap_or(0);
    if excluded_total == 0 && deprecated == 0 && quarantined == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if deprecated > 0 {
        parts.push(format!(
            "{} {}",
            format_u64(Some(deprecated)),
            russian_count_form(deprecated, "устаревший", "устаревших", "устаревших",)
        ));
    }
    if quarantined > 0 {
        parts.push(format!(
            "{} {}",
            format_u64(Some(quarantined)),
            russian_count_form(quarantined, "карантинный", "карантинных", "карантинных",)
        ));
    }
    if parts.is_empty() {
        parts.push(format!("{} legacy", format_u64(Some(excluded_total))));
    }
    Some(format!("отфильтровано из legacy: {}", parts.join(", ")))
}

fn working_state_task_graph_projection_operator_summary(restore: &Value) -> Option<String> {
    let validation = &restore["task_graph_projection_validation"];
    if !validation.is_object() {
        return None;
    }
    let status = working_state_task_graph_projection_status(restore)?;
    if working_state_task_graph_projection_is_validated(restore) {
        if let Some(debt) = working_state_task_graph_legacy_debt_summary(validation) {
            return Some(format!("validated graph-first projection • {debt}"));
        }
        if validation["warnings"]["projection_preview_limited"].as_bool() == Some(true) {
            return Some("validated graph-first projection • compact preview limited".to_string());
        }
        return Some("validated graph-first projection".to_string());
    }
    let reason = validation["reason"]
        .as_str()
        .map(humanize_identifier)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "причина не surfaced".to_string());
    Some(format!("{status} • {reason}"))
}

#[derive(Debug, Clone, Default)]
struct WorkingStateTaskGraphSurface {
    validated: bool,
    requires_attention: bool,
    operator_summary: Option<String>,
    note_sentence: Option<String>,
    tooltip_sentence: Option<String>,
    status_label: Option<String>,
    tree_summary: Option<String>,
    ledger_summary: Option<String>,
}

fn build_working_state_task_graph_surface(restore: &Value) -> WorkingStateTaskGraphSurface {
    let mut surface = WorkingStateTaskGraphSurface::default();
    let Some(operator_summary) = working_state_task_graph_projection_operator_summary(restore)
    else {
        return surface;
    };
    let validation = &restore["task_graph_projection_validation"];
    let validated = working_state_task_graph_projection_is_validated(restore);
    surface.validated = validated;
    surface.requires_attention = !validated;
    surface.operator_summary = Some(operator_summary);
    if validated {
        surface.tree_summary = restore["project_task_tree_summary"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        surface.ledger_summary = restore["project_task_ledger_summary"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let Some(debt) = working_state_task_graph_legacy_debt_summary(validation) {
            surface.note_sentence = Some(format!(
                "В compact-проекции задач legacy-долг уже отфильтрован: {debt}."
            ));
            surface.tooltip_sentence = Some(format!(
                "Validated graph-first path уже отфильтровал legacy-долг: {debt}."
            ));
        } else if validation["warnings"]["projection_preview_limited"].as_bool() == Some(true) {
            surface.note_sentence = Some(
                "Полный validated SQL-набор уже дочитан, а compact preview просто ужат до короткой операторской формы."
                    .to_string(),
            );
            surface.tooltip_sentence = Some(
                "compact preview limited не означает fallback: validated graph-first projection уже materialized."
                    .to_string(),
            );
        }
        return surface;
    }

    let reason = validation["reason"]
        .as_str()
        .map(humanize_identifier)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "причина не surfaced".to_string());
    let status = working_state_task_graph_projection_status(restore).unwrap_or("unknown");
    surface.status_label = Some("граф через fallback".to_string());
    surface.note_sentence = Some(format!(
        "Граф задач сейчас идёт не по validated SQL-проекции, а через {status}: {reason}."
    ));
    surface.tooltip_sentence = Some(format!(
        "Task-graph projection сейчас degraded: validated graph-first projection недоступна и surfaced как {status}. Причина: {reason}."
    ));
    surface
}

fn worsen_working_state_status<'a>(current: &'a str, candidate: &'a str) -> &'a str {
    match candidate {
        "alert" => "alert",
        "waiting" if current == "pass" || current == "unknown" => "waiting",
        _ => current,
    }
}

pub(super) fn summarize_working_state_goal(
    value: Option<&str>,
    last_command: Option<&str>,
) -> String {
    let raw = value.map(str::trim).filter(|value| !value.is_empty());
    if let Some(raw) = raw {
        if raw.contains("continue the same simplification pass on other dashboard cards") {
            return "упрощение следующих карточек панели".to_string();
        }
        if raw.contains("refine operator-facing copy")
            || raw.contains("other live cards")
            || raw.contains("same reconciliation pattern")
            || raw.contains("enrich current-work card")
            || raw.contains("live-thread active files")
        {
            return "доработка live-карточек панели".to_string();
        }
        if raw.is_ascii() {
            let lowered = raw.to_ascii_lowercase();
            if lowered.contains("card")
                || lowered.contains("dashboard")
                || lowered.contains("panel")
            {
                return "обновление панели".to_string();
            }
            if lowered.contains("dashboard") {
                return "обновление панели".to_string();
            }
            if lowered.contains("benchmark") {
                return "прогон benchmark".to_string();
            }
            if lowered.contains("proof") {
                return "запуск проверки".to_string();
            }
        }
        return compact_dashboard_text(Some(raw), 72, "ещё нет данных");
    }
    summarize_working_state_command(last_command)
}

pub(super) fn summarize_working_state_next_step(value: Option<&str>) -> String {
    let raw = value.map(str::trim).filter(|value| !value.is_empty());
    let Some(raw) = raw else {
        return "ещё нет данных".to_string();
    };
    if raw.contains("continue the same simplification pass on other dashboard cards") {
        return "упростить ещё несколько карточек панели".to_string();
    }
    if raw.contains("keep the same release-rebuild-restart loop") {
        return "продолжить цикл: правка, сборка, перезапуск панели".to_string();
    }
    if raw.contains("If user continues, refine operator-facing copy") {
        return "уточнить операторский текст в live-карточках".to_string();
    }
    if raw.contains("expand the same reconciliation pattern to other live cards") {
        return "распространить ту же логику согласования на остальные live-карточки".to_string();
    }
    if raw.contains("If user continues, enrich current-work card") {
        return "добавить в карточку текущей работы живые подсказки по активным файлам".to_string();
    }
    if raw.contains("Optionally continue by filling last-command placeholder") {
        return "заполнить в карточке текущей работы последнюю команду из живого Amai-turn"
            .to_string();
    }
    if raw.contains("humanizing the remaining English next-step fallback") {
        return "дочистить английский fallback в карточке текущей работы".to_string();
    }
    compact_dashboard_text(Some(raw), 108, "ещё нет данных")
}

fn working_state_live_turn_activity_surface(snapshot: &Value) -> Option<(Value, String)> {
    let current_live_turn = &token_budget_report_root(snapshot)["current_live_turn"];
    let status = current_live_turn["status"].as_str()?;
    let current_thread_bound = current_live_turn["current_thread_bound"].as_bool() == Some(true);
    let retrieval_context_pack_count = current_live_turn["retrieval_context_pack_count"]
        .as_u64()
        .unwrap_or(0);
    let matched_context_pack_ids_count = current_live_turn["matched_context_pack_ids_count"]
        .as_u64()
        .unwrap_or(0);
    let observed_context_pack_count =
        retrieval_context_pack_count.max(matched_context_pack_ids_count);
    let current_live_turn_note = current_live_turn["note"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Свежая активность текущего thread/turn Amai уже наблюдается.");

    let (value, note_sentence) = match status {
        "exact_pair_materialized" => {
            let saved_pct = current_live_turn["exact_pair"]["saved_pct"].as_f64();
            let value = if observed_context_pack_count > 0 {
                format!(
                    "{} context-pack • {} exact-pair",
                    format_u64(Some(observed_context_pack_count)),
                    format_percent(saved_pct)
                )
            } else {
                format!("exact-pair готов • {}", format_percent(saved_pct))
            };
            (
                value,
                "В текущем thread уже есть свежий живой ответ Amai, и same-turn exact-pair уже materialized."
                    .to_string(),
            )
        }
        "thread_activity_observed_turn_open" => {
            let value = if observed_context_pack_count > 0 {
                format!(
                    "{} context-pack • turn ещё открыт",
                    format_u64(Some(observed_context_pack_count))
                )
            } else {
                "turn ещё открыт".to_string()
            };
            (
                value,
                "В текущем thread уже есть свежая активность Amai, но текущий turn ещё не закрыт."
                    .to_string(),
            )
        }
        "activity_observed_exact_pair_unavailable" => {
            let value = if observed_context_pack_count > 0 {
                format!(
                    "{} context-pack • exact-pair ещё materialize-ится",
                    format_u64(Some(observed_context_pack_count))
                )
            } else {
                "exact-pair ещё materialize-ится".to_string()
            };
            (
                value,
                "В текущем thread уже observed активность Amai, но same-turn exact-pair ещё не готов."
                    .to_string(),
            )
        }
        "no_amai_activity_in_current_live_turn" if current_thread_bound => (
            "turn открыт • ответов Amai ещё нет".to_string(),
            "Новый live-turn этого чата уже открыт, но Amai в нём пока ещё не ответила."
                .to_string(),
        ),
        _ => return None,
    };

    Some((
        metric_row("Живой turn Amai", value, Some(current_live_turn_note)),
        note_sentence,
    ))
}

fn working_state_live_turn_last_command_fallback(snapshot: &Value) -> Option<String> {
    let current_live_turn = &token_budget_report_root(snapshot)["current_live_turn"];
    let status = current_live_turn["status"].as_str()?;
    let observed_context_pack_count = current_live_turn["retrieval_context_pack_count"]
        .as_u64()
        .unwrap_or(0)
        .max(
            current_live_turn["matched_context_pack_ids_count"]
                .as_u64()
                .unwrap_or(0),
        );
    match status {
        "exact_pair_materialized"
        | "thread_activity_observed_turn_open"
        | "activity_observed_exact_pair_unavailable"
            if observed_context_pack_count > 0 =>
        {
            Some("Amai context pack".to_string())
        }
        _ => {
            let live_file_hints = live_response_latency_current_thread_file_hints(snapshot);
            if !live_file_hints.is_empty() {
                Some("Amai context pack".to_string())
            } else {
                None
            }
        }
    }
}

pub(super) fn should_override_last_command_with_live_turn(
    summarized_last_command: &str,
    restore_confidence: &str,
    recent_queries: u64,
) -> bool {
    if summarized_last_command == "ещё нет данных" {
        return true;
    }
    restore_confidence == "preliminary"
        && recent_queries == 0
        && summarized_last_command == "сохранена рабочая сводка"
}

fn working_state_live_turn_last_result_fallback(snapshot: &Value) -> Option<String> {
    let current_live_turn = &token_budget_report_root(snapshot)["current_live_turn"];
    let status = current_live_turn["status"].as_str()?;
    match status {
        "exact_pair_materialized"
        | "thread_activity_observed_turn_open"
        | "activity_observed_exact_pair_unavailable" => {}
        _ => return None,
    }
    current_live_turn["note"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| compact_dashboard_text(Some(value), 108, "ещё нет данных"))
}

pub(super) fn working_state_live_card(snapshot: &Value) -> Value {
    let live_turn_activity = working_state_live_turn_activity_surface(snapshot);
    let restore_root = &snapshot["latest_repo_working_state_restore"]["working_state_restore"];
    if !restore_root.is_object() {
        let mut rows = Vec::new();
        let mut note =
            "Для этого репозитория пока нет свежего локального снимка работы.".to_string();
        let status = if let Some((row, note_sentence)) = live_turn_activity.as_ref() {
            rows.push(row.clone());
            note = format!(
                "Локальный рабочий снимок ещё не materialized, но текущий chat turn уже видит свежую активность Amai. {}",
                note_sentence
            );
            "waiting"
        } else {
            "unknown"
        };
        let source_label = if status == "waiting" {
            "Источник: current_live_turn same-thread; latest_repo_working_state_restore.working_state_restore ещё не materialized.".to_string()
        } else {
            "Источник: latest_repo_working_state_restore.working_state_restore".to_string()
        };
        let mut card = card_with_rows(
            "Текущая работа",
            "ещё нет данных".to_string(),
            note,
            status,
            Some(source_label),
            Some("Показывает простую сводку по текущей работе в этом репозитории: что сейчас делаем, что дальше и когда это обновлялось.".to_string()),
            rows,
        );
        if status == "waiting" {
            card = with_status_label(card, "живой turn уже виден");
            card = with_status_tooltip(
                card,
                "Статус пока не может считаться полностью нормальным по следующим причинам:\n- Локальный working-state snapshot для этого репозитория ещё не materialized.\n- Но текущий thread уже observed свежую активность Amai, поэтому панель показывает live-turn отдельно.",
            );
            return card;
        }
        return with_status_tooltip(
            card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- Для текущего репозитория ещё нет локального рабочего снимка.\n- Панель специально не подмешивает сюда более свежую рабочую линию другого проекта.",
        );
    }
    let restore = restore_root;
    let current_goal = summarize_working_state_goal(
        restore["current_goal"].as_str(),
        restore["last_command"].as_str(),
    );
    let next_step = summarize_working_state_next_step(restore["next_step"].as_str());
    let scope = plain_working_state_scope(restore);
    let events_count = restore["events_count"].as_u64();
    let snapshot_age = elapsed_since_epoch_label(
        restore["captured_at_epoch_ms"].as_u64(),
        snapshot["captured_at_epoch_ms"].as_u64(),
    );
    let restore_confidence = restore["restore_confidence"]
        .as_str()
        .unwrap_or("preliminary");
    let task_graph_surface = build_working_state_task_graph_surface(restore);
    let recent_queries = restore["recent_queries"]
        .as_array()
        .map(|items| items.len() as u64)
        .unwrap_or(0);
    let last_command = summarize_working_state_command(restore["last_command"].as_str());
    let last_command = if should_override_last_command_with_live_turn(
        &last_command,
        restore_confidence,
        recent_queries,
    ) {
        working_state_live_turn_last_command_fallback(snapshot).unwrap_or(last_command)
    } else {
        last_command
    };
    let last_results = summarize_working_state_result(restore["last_results_summary"].as_str());
    let last_results = if last_results == "ещё нет данных" {
        working_state_live_turn_last_result_fallback(snapshot).unwrap_or(last_results)
    } else {
        last_results
    };
    let active_files = restore["active_files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let active_files_count = active_files.len() as u64;
    let live_file_hints = live_response_latency_current_thread_file_hints(snapshot);
    let active_files_preview = active_files
        .iter()
        .filter_map(Value::as_str)
        .map(|path| {
            Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path)
                .to_string()
        })
        .take(3)
        .collect::<Vec<_>>()
        .join(", ");
    let live_file_hints_preview = live_file_hints
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let restore_confidence_human = match restore_confidence {
        "high" => "высокая",
        "medium" => "средняя",
        "preliminary" => "предварительная",
        "low" => "низкая",
        other => other,
    };
    let mut status = match restore_confidence {
        "high" | "medium" => "pass",
        "low" => "alert",
        _ if events_count.unwrap_or(0) > 0 => "waiting",
        _ => "unknown",
    };
    if task_graph_surface.requires_attention {
        status = worsen_working_state_status(status, "alert");
    }
    if live_turn_activity.is_some() && status == "unknown" {
        status = "waiting";
    }
    let mut rows = vec![
        metric_row(
            "Где",
            scope,
            Some("В каком проекте и разделе сейчас ведётся эта работа."),
        ),
        metric_row(
            "Обновлено",
            snapshot_age.clone(),
            Some("Когда эта сводка обновлялась в последний раз."),
        ),
        metric_row(
            "Что дальше",
            next_step.clone(),
            Some("Какой следующий шаг сейчас считается основным."),
        ),
        metric_row(
            "Состояние проекции",
            compact_dashboard_text(
                task_graph_surface.operator_summary.as_deref(),
                108,
                "ещё нет данных",
            ),
            Some(
                "Показывает, validated ли текущая task-graph truth-проекция и есть ли surfaced legacy-долг или fallback.",
            ),
        ),
        metric_row(
            "Последний результат",
            last_results,
            Some("Коротко: что уже получилось на последнем шаге."),
        ),
        metric_row(
            "Последняя команда",
            last_command,
            Some("Какое последнее действие было перед этой сводкой."),
        ),
        metric_row(
            "Активные файлы",
            if active_files_preview.is_empty() {
                if !live_file_hints_preview.is_empty() {
                    format!(
                        "{} • {}",
                        format_u64(Some(live_file_hints.len() as u64)),
                        live_file_hints_preview
                    )
                } else {
                    format_u64(Some(active_files_count))
                }
            } else {
                format!(
                    "{} • {}",
                    format_u64(Some(active_files_count)),
                    active_files_preview
                )
            },
            Some(if !active_files_preview.is_empty() {
                "Какие файлы сейчас чаще всего фигурируют в этой работе."
            } else if !live_file_hints_preview.is_empty() {
                "Ранние живые подсказки по файлам из последних same-thread запросов Amai до полного working-state snapshot."
            } else {
                "Какие файлы сейчас чаще всего фигурируют в этой работе."
            }),
        ),
        metric_row(
            "Следов в истории",
            format_count_with_word(events_count.unwrap_or(0), "событие", "события", "событий"),
            Some("Сколько подтверждённых событий уже есть у этой рабочей линии."),
        ),
    ];
    if task_graph_surface.validated {
        if let Some(tree_summary) = task_graph_surface.tree_summary.as_deref() {
            rows.push(metric_row(
                "Сводка графа",
                compact_dashboard_text(Some(tree_summary), 108, "ещё нет данных"),
                Some("Короткая operator-facing сводка по текущей validated task-graph проекции."),
            ));
        }
        if let Some(ledger_summary) = task_graph_surface.ledger_summary.as_deref() {
            rows.push(metric_row(
                "Сводка ленты",
                compact_dashboard_text(Some(ledger_summary), 108, "ещё нет данных"),
                Some(
                    "Короткая operator-facing сводка по active/historical state этой рабочей линии.",
                ),
            ));
        }
    }
    if recent_queries > 0 {
        rows.push(metric_row(
            "Недавние запросы",
            format_u64(Some(recent_queries)),
            Some("Сколько недавних запросов попало в эту рабочую линию."),
        ));
    }
    let live_turn_note_sentence = live_turn_activity.as_ref().map(|(_, note)| note.clone());
    if let Some((row, _)) = live_turn_activity {
        rows.push(row);
    }

    let live_turn_note_present = live_turn_note_sentence.is_some();
    let projection_note_sentence = task_graph_surface.note_sentence.clone();
    let card_note = if let Some(ref note_sentence) = live_turn_note_sentence {
        format!(
            "Короткая сводка по текущей работе. Следующий шаг: {}. {}{}",
            next_step,
            note_sentence,
            projection_note_sentence
                .as_deref()
                .map(|value| format!(" {value}"))
                .unwrap_or_default()
        )
    } else if let Some(note_sentence) = projection_note_sentence.as_deref() {
        format!(
            "Короткая сводка по текущей работе. Следующий шаг: {}. {}",
            next_step, note_sentence
        )
    } else {
        format!(
            "Короткая сводка по текущей работе. Следующий шаг: {}.",
            next_step
        )
    };
    let mut card = card_with_rows(
        "Текущая работа",
        current_goal,
        card_note,
        status,
        Some(source_label(
            "Источник: последний рабочий снимок именно этого репозитория.",
            restore["captured_at_epoch_ms"].as_u64(),
        )),
        Some("Показывает простую сводку по текущей работе в этом репозитории: что делаем, что дальше и на чём сейчас сфокусированы.".to_string()),
        rows,
    );
    if let Some(status_label) = task_graph_surface.status_label.as_deref() {
        card = with_status_label(card, status_label);
    }
    if status == "waiting" {
        let waiting_label = if live_turn_note_sentence
            .as_deref()
            .is_some_and(|note| note.contains("пока ещё не ответила"))
        {
            "ждём ответ Amai"
        } else if live_turn_note_present {
            "живой turn уже виден"
        } else {
            "ждём устойчивый снимок"
        };
        card = with_status_label(card, waiting_label);
    }
    if status != "pass" {
        let projection_tooltip_suffix = task_graph_surface
            .tooltip_sentence
            .as_deref()
            .map(|value| format!("\n- {value}"))
            .unwrap_or_default();
        let tooltip = if status == "alert" {
            format!(
                "Статус требует внимания по следующим причинам:\n- Уверенность в этом рабочем снимке пока {}.\n- Последний локальный снимок сделан {}.\n- Рабочая линия уже содержит {}, но снимок ещё недостаточно устойчив.\n- Следующий обязательный шаг сейчас: {}.",
                restore_confidence_human,
                snapshot_age,
                format_count_with_word(events_count.unwrap_or(0), "событие", "события", "событий"),
                next_step
            ) + &projection_tooltip_suffix
        } else if status == "waiting" {
            if live_turn_note_sentence
                .as_deref()
                .is_some_and(|note| note.contains("пока ещё не ответила"))
            {
                format!(
                    "Статус пока не может считаться нормальным по следующим причинам:\n- Новый live-turn уже открыт, но Amai в нём ещё не ответила.\n- Последний локальный снимок сделан {}.\n- Рабочая линия уже содержит {}, но для устойчивого локального снимка нужно больше подтверждённых событий.\n- Следующий обязательный шаг сейчас: {}.",
                    snapshot_age,
                    format_count_with_word(
                        events_count.unwrap_or(0),
                        "событие",
                        "события",
                        "событий"
                    ),
                    next_step
                ) + &projection_tooltip_suffix
            } else {
                format!(
                    "Статус пока не может считаться нормальным по следующим причинам:\n- Уверенность в этом рабочем снимке пока {}.\n- Последний локальный снимок сделан {}.\n- Рабочая линия уже содержит {}, но для устойчивого локального снимка нужно больше подтверждённых событий.\n- Следующий обязательный шаг сейчас: {}.",
                    restore_confidence_human,
                    snapshot_age,
                    format_count_with_word(
                        events_count.unwrap_or(0),
                        "событие",
                        "события",
                        "событий"
                    ),
                    next_step
                ) + &projection_tooltip_suffix
            }
        } else {
            "Статус пока не может считаться нормальным по следующим причинам:\n- Рабочая линия ещё не накопила достаточно надёжный рабочий снимок.\n- Пока панель видит только предварительный или почти пустой след текущей работы.".to_string()
                + &projection_tooltip_suffix
        };
        card = with_status_tooltip(card, &tooltip);
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn working_state_card_hides_empty_decision_trace_rows_and_requires_repo_scoped_snapshot() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "default" },
                    "agent_scope": "amai::default::default",
                    "session_age_ms": 7u64,
                    "events_count": 1u64,
                    "current_goal": "Рабочий запрос: structural graph proof",
                    "next_step": "Уточните запрос или задайте follow-up.",
                    "last_command": "context pack",
                    "last_results_summary": "Найдено: документов 0, символов 0.",
                    "latest_decision_trace": null,
                    "active_files": [],
                    "recent_queries": ["structural graph proof"],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(
            card["status_label"].as_str(),
            Some("ждём устойчивый снимок")
        );
        let rows = card["rows"].as_array().expect("rows");
        assert!(
            rows.iter()
                .all(|row| row["label"].as_str() != Some("Почему включено"))
        );
        assert!(
            rows.iter()
                .all(|row| row["label"].as_str() != Some("Почему не вошло"))
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая сводка по текущей работе")
        );

        let unknown_card = working_state_live_card(&json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "latest_repo_working_state_restore": null
        }));
        assert_eq!(unknown_card["status"], json!("unknown"));
        assert!(
            unknown_card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("нет свежего локального снимка")
        );
        assert_eq!(
            unknown_card["source_label"].as_str(),
            Some("Источник: latest_repo_working_state_restore.working_state_restore")
        );
    }

    #[test]
    fn working_state_card_surfaces_current_live_turn_activity_when_exact_pair_is_ready() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "exact_pair_materialized",
                        "retrieval_context_pack_count": 1,
                        "matched_context_pack_ids_count": 1,
                        "note": "Exact full-turn pair materialized from the actual VS Code meter.",
                        "exact_pair": {
                            "saved_pct": 76.52
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Repair dashboard live-turn behavior",
                    "next_step": "Surface live turn in current work card.",
                    "last_command": "context pack",
                    "last_results_summary": "current_live_turn exact pair materialized",
                    "active_files": [
                        "/home/art/agent-memory-index/src/dashboard.rs"
                    ],
                    "recent_queries": [],
                    "restore_confidence": "medium"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        assert!(
            card["note"]
                .as_str()
                .is_some_and(|note| { note.contains("свежий живой ответ Amai") })
        );
        let rows = card["rows"].as_array().expect("rows");
        let live_turn_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Живой turn Amai"))
            .expect("live turn row");
        assert!(
            live_turn_row["value"].as_str().is_some_and(|value| {
                value.contains("1 context-pack") && value.contains("76.52%")
            })
        );
    }

    #[test]
    fn working_state_card_uses_waiting_status_when_only_live_turn_activity_is_fresh() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "exact_pair_materialized",
                        "retrieval_context_pack_count": 1,
                        "matched_context_pack_ids_count": 1,
                        "note": "Exact full-turn pair materialized from the actual VS Code meter.",
                        "exact_pair": {
                            "saved_pct": 69.64
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 0u64,
                    "current_goal": "Current live-turn now surfaces same-thread Amai activity after fresh context-pack",
                    "next_step": "Tighten current-work card so fresh exact-pair / thread activity is surfaced there too.",
                    "last_command": "continuity handoff",
                    "last_results_summary": null,
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("живой turn уже виден"));
        let rows = card["rows"].as_array().expect("rows");
        let last_result_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Последний результат"))
            .expect("last result row");
        assert!(
            last_result_row["value"]
                .as_str()
                .is_some_and(|value| { value.contains("Exact full-turn pair materialized") })
        );
        let last_command_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Последняя команда"))
            .expect("last command row");
        assert_eq!(
            last_command_row["value"].as_str(),
            Some("Amai context pack")
        );
    }

    #[test]
    fn preliminary_handoff_command_is_overridden_by_fresh_live_turn_command() {
        assert!(should_override_last_command_with_live_turn(
            "сохранена рабочая сводка",
            "preliminary",
            0,
        ));
        assert!(!should_override_last_command_with_live_turn(
            "сохранена рабочая сводка",
            "high",
            0,
        ));
        assert!(!should_override_last_command_with_live_turn(
            "сохранена рабочая сводка",
            "preliminary",
            2,
        ));
    }

    #[test]
    fn live_file_hints_restore_last_command_when_new_turn_is_still_empty() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "current_thread_bound": true,
                        "retrieval_context_pack_count": 0,
                        "matched_context_pack_ids_count": 0
                    },
                    "live_response_latency": {
                        "current_session_relation": {
                            "status": "recent_same_chat_series_previous_turn"
                        },
                        "current_thread_live_file_hints": {
                            "hints": [
                                {"label": "dashboard.rs", "query": "./src/dashboard.rs"}
                            ]
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 0u64,
                    "current_goal": "Current live-turn now surfaces same-thread Amai activity after fresh context-pack",
                    "next_step": "Tighten current-work card so fresh exact-pair / thread activity is surfaced there too.",
                    "last_command": null,
                    "last_results_summary": null,
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        let rows = card["rows"].as_array().expect("rows");
        let last_command_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Последняя команда"))
            .expect("last command row");
        assert_eq!(
            last_command_row["value"].as_str(),
            Some("Amai context pack")
        );
    }

    #[test]
    fn working_state_card_falls_back_to_live_turn_when_working_state_is_missing() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "thread_activity_observed_turn_open",
                        "retrieval_context_pack_count": 2,
                        "matched_context_pack_ids_count": 1,
                        "note": "Observed new retrieval_context_pack after the last completed turn."
                    }
                }
            },
            "latest_repo_working_state_restore": null
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("живой turn уже виден"));
        assert!(card["note"].as_str().is_some_and(|note| {
            note.contains("текущий chat turn уже видит свежую активность Amai")
        }));
        let rows = card["rows"].as_array().expect("rows");
        let live_turn_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Живой turn Amai"))
            .expect("live turn row");
        assert_eq!(
            live_turn_row["value"].as_str(),
            Some("2 context-pack • turn ещё открыт")
        );
        assert_eq!(
            card["source_label"].as_str(),
            Some(
                "Источник: current_live_turn same-thread; latest_repo_working_state_restore.working_state_restore ещё не materialized."
            )
        );
    }

    #[test]
    fn working_state_card_does_not_mix_stale_global_restore_when_repo_restore_is_missing() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "thread_activity_observed_turn_open",
                        "retrieval_context_pack_count": 1,
                        "matched_context_pack_ids_count": 1,
                        "note": "Observed new retrieval_context_pack after the last completed turn."
                    }
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "current_goal": "stale foreign global goal",
                    "next_step": "stale foreign global next step"
                }
            },
            "latest_repo_working_state_restore": null
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["value"].as_str(), Some("ещё нет данных"));
        assert!(card["note"].as_str().is_some_and(|note| {
            note.contains("Локальный рабочий снимок ещё не materialized")
                && !note.contains("stale foreign global")
        }));
        assert_eq!(
            card["source_label"].as_str(),
            Some(
                "Источник: current_live_turn same-thread; latest_repo_working_state_restore.working_state_restore ещё не materialized."
            )
        );
    }

    #[test]
    fn working_state_card_surfaces_open_turn_without_amai_answer_yet() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775420265000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "current_thread_bound": true,
                        "thread_id": "thread-live",
                        "note": "В текущем live-turn не наблюдалось ни одного retrieval_context_pack от Amai."
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "next_step": "Wait for the next real Amai answer in this chat.",
                    "current_goal": "Observe the next online answer",
                    "events_count": 0u64,
                    "restore_confidence": "preliminary"
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("waiting"));
        assert_eq!(card["status_label"].as_str(), Some("ждём ответ Amai"));
        let rows = card["rows"].as_array().expect("rows");
        let live_turn_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Живой turn Amai"))
            .expect("live turn row");
        assert_eq!(
            live_turn_row["value"].as_str(),
            Some("turn открыт • ответов Amai ещё нет")
        );
        assert!(
            card["status_tooltip"]
                .as_str()
                .is_some_and(|tooltip| tooltip.contains("Amai в нём ещё не ответила"))
        );
    }

    #[test]
    fn working_state_card_uses_live_file_hints_when_active_files_are_empty() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775420265000u64,
            "token_budget_report": {
                "token_budget_report": {
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "current_thread_bound": true,
                        "thread_id": "thread-live"
                    },
                    "live_response_latency": {
                        "current_thread_live_file_hints": {
                            "hints": [
                                { "label": "dashboard.rs", "query": "./src/dashboard.rs" },
                                { "label": "token_budget.rs", "query": "./src/token_budget.rs" }
                            ]
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "next_step": "Add live file hints.",
                    "current_goal": "Observe the next online answer",
                    "events_count": 0u64,
                    "restore_confidence": "preliminary",
                    "active_files": []
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        let rows = card["rows"].as_array().expect("rows");
        let active_files_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Активные файлы"))
            .expect("active files row");
        assert_eq!(
            active_files_row["value"].as_str(),
            Some("2 • dashboard.rs, token_budget.rs")
        );
    }

    #[test]
    fn summarize_working_state_next_step_humanizes_live_card_reconciliation_text() {
        assert_eq!(
            summarize_working_state_next_step(Some(
                "If user continues, refine operator-facing copy or expand the same reconciliation pattern to other live cards."
            )),
            "уточнить операторский текст в live-карточках"
        );
        assert_eq!(
            summarize_working_state_goal(
                Some(
                    "If user continues, refine operator-facing copy or expand the same reconciliation pattern to other live cards."
                ),
                None
            ),
            "доработка live-карточек панели"
        );
        assert_eq!(
            summarize_working_state_next_step(Some(
                "If user continues, enrich current-work card with live-thread active files or replace generic next-step text."
            )),
            "добавить в карточку текущей работы живые подсказки по активным файлам"
        );
        assert_eq!(
            summarize_working_state_next_step(Some(
                "Optionally continue by filling last-command placeholder from the same live-turn source so the card is fully operator-readable before working-state catches up."
            )),
            "заполнить в карточке текущей работы последнюю команду из живого Amai-turn"
        );
    }

    #[test]
    fn working_state_card_surfaces_validated_task_graph_debt() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "events_count": 4u64,
                    "current_goal": "Keep startup and dashboard truth aligned",
                    "next_step": "Surface excluded legacy debt in the current-work card.",
                    "last_command": "continuity handoff",
                    "last_results_summary": "graph-first runtime artifact already carries excluded legacy debt",
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "high",
                    "project_task_tree_summary": "active: Real Amai work first; open(14); excluded_legacy(2396 deprecated)",
                    "project_task_ledger_summary": "active: Real Amai work first; open(14); pending_return(0); excluded_legacy(2396 deprecated)",
                    "task_graph_projection_validation": {
                        "status": "valid",
                        "validation_state": "valid",
                        "projection_source": "graph_first_sql_validated",
                        "truth_claim": false,
                        "projection_excluded_sql_nodes_count": 2396,
                        "deprecated_sql_nodes_count": 2396,
                        "quarantined_sql_nodes_count": 0
                    }
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        let rows = card["rows"].as_array().expect("rows");
        let projection_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Состояние проекции"))
            .expect("projection row");
        assert!(projection_row["value"].as_str().is_some_and(|value| {
            value.contains("validated graph-first projection")
                && value.contains("2396")
                && value.contains("устаревших")
        }));
        let tree_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Сводка графа"))
            .expect("tree row");
        assert!(
            tree_row["value"]
                .as_str()
                .is_some_and(|value| value.contains("excluded_legacy(2396 deprecated)"))
        );
        let ledger_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Сводка ленты"))
            .expect("ledger row");
        assert!(
            ledger_row["value"]
                .as_str()
                .is_some_and(|value| value.contains("excluded_legacy(2396 deprecated)"))
        );
        assert!(card["note"].as_str().is_some_and(|value| {
            value.contains("legacy-долг уже отфильтрован")
                || value.contains("legacy долг уже отфильтрован")
        }));
    }

    #[test]
    fn working_state_card_marks_task_graph_fallback_as_alert_without_trusting_stale_summaries() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "events_count": 4u64,
                    "current_goal": "Keep startup and dashboard truth aligned",
                    "next_step": "Surface fallback explicitly in the current-work card.",
                    "last_command": "continuity handoff",
                    "last_results_summary": "task graph projection fell back to execctl ledger",
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "high",
                    "project_task_tree_summary": "active: stale compact summary that should not be trusted",
                    "project_task_ledger_summary": "active: stale compact ledger summary that should not be trusted",
                    "task_graph_projection_validation": {
                        "status": "fallback_to_execctl_ledger",
                        "validation_state": "fallback_to_execctl_ledger",
                        "projection_source": "execctl_ledger_fallback",
                        "reason": "control_invariant_mismatch"
                    }
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("alert"));
        assert_eq!(card["status_label"].as_str(), Some("граф через fallback"));
        let rows = card["rows"].as_array().expect("rows");
        let projection_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Состояние проекции"))
            .expect("projection row");
        assert!(projection_row["value"].as_str().is_some_and(|value| {
            value.contains("fallback_to_execctl_ledger")
                && value.contains("Control Invariant Mismatch")
        }));
        assert!(
            rows.iter()
                .all(|row| row["label"].as_str() != Some("Сводка графа"))
        );
        assert!(
            rows.iter()
                .all(|row| row["label"].as_str() != Some("Сводка ленты"))
        );
    }

    #[test]
    fn working_state_card_keeps_preview_limited_projection_as_validated() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1775412360000u64,
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1775412359000u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "events_count": 4u64,
                    "current_goal": "Keep startup and dashboard truth aligned",
                    "next_step": "Explain preview-limited without surfacing fallback.",
                    "last_command": "observe snapshot",
                    "last_results_summary": "validated graph-first projection still has compact preview pressure",
                    "active_files": [],
                    "recent_queries": [],
                    "restore_confidence": "high",
                    "project_task_tree_summary": "active: Real Amai work first; open(14)",
                    "project_task_ledger_summary": "active: Real Amai work first; open(14); pending_return(0)",
                    "task_graph_projection_validation": {
                        "status": "valid",
                        "validation_state": "valid",
                        "projection_source": "graph_first_sql_validated",
                        "truth_claim": false,
                        "warnings": {
                            "projection_preview_limited": true
                        }
                    }
                }
            }
        });

        let card = working_state_live_card(&snapshot);
        assert_eq!(card["status"].as_str(), Some("pass"));
        let rows = card["rows"].as_array().expect("rows");
        let projection_row = rows
            .iter()
            .find(|row| row["label"].as_str() == Some("Состояние проекции"))
            .expect("projection row");
        assert!(projection_row["value"].as_str().is_some_and(|value| {
            value.contains("validated graph-first projection")
                && value.contains("compact preview limited")
        }));
        assert_ne!(card["status_label"].as_str(), Some("граф через fallback"));
    }
}
