use super::dashboard_current_session_budget_guard::CLIENT_BUDGET_ADVISORY_STATUS_PREFIX;
use super::*;

fn active_agent_budget_card_status(surface: &Value) -> (&'static str, &'static str, String) {
    let aggregate = &surface["aggregate"];
    let status = aggregate["status"].as_str().unwrap_or("missing");
    let classification = aggregate["classification"].as_str().unwrap_or("missing");
    match (status, classification) {
        ("observed", "overspend") => (
            "alert",
            "активным агентам нужна проверка",
            "Суммарная Amai savings по активным агентам сейчас в перерасходе, поэтому карточка требует внимания."
                .to_string(),
        ),
        ("observed", _) => (
            "pass",
            "только активные агенты",
            "Карточка показывает только личную Amai savings и текущий лимит клиента для реально активных агентов."
                .to_string(),
        ),
        ("partial", _) => (
            "waiting",
            "не у всех ещё есть подтверждённая пара",
            "Не у каждого активного агента уже есть полная подтверждённая пара без Amai / с Amai, поэтому суммарная величина пока не посчитана."
                .to_string(),
        ),
        _ => (
            "waiting",
            "активных агентов сейчас нет",
            "Сейчас нет активных агентов, поэтому карточка не показывает персональную экономию."
                .to_string(),
        ),
    }
}

fn reviewed_frozen_debt_export_note_sentence(alignment: &Value) -> Option<&'static str> {
    let surface = &alignment["reviewed_frozen_debt_export_surface"];
    if surface["export_ready_report_only"].as_bool() != Some(true) {
        return None;
    }
    Some(
        "Старый исторический хвост уже вынесен в отдельный отчёт для ручной сверки: его можно смотреть отдельно, но он не считается точной полной историей.",
    )
}

fn historical_frozen_debt_note_sentence(
    current_session_alignment: &Value,
    rolling_window_alignment: &Value,
    lifetime_alignment: &Value,
) -> Option<&'static str> {
    historical_frozen_debt_metric_row(
        current_session_alignment,
        rolling_window_alignment,
        lifetime_alignment,
    )?;
    Some(
        "По текущей сессии и рабочему окну точная пара уже собрана. Старый исторический хвост остался только в накопительной истории и не выглядит как новый расход.",
    )
}

fn historical_frozen_debt_metric_row(
    current_session_alignment: &Value,
    rolling_window_alignment: &Value,
    lifetime_alignment: &Value,
) -> Option<Value> {
    let current_exact = current_session_alignment["exact_pair_status"]["exact_pair_available"]
        .as_bool()
        == Some(true);
    let rolling_exact = rolling_window_alignment["exact_pair_status"]["exact_pair_available"]
        .as_bool()
        == Some(true);
    let frozen_gap_review_surface = &lifetime_alignment["frozen_gap_review_surface"];
    if !(current_exact
        && rolling_exact
        && frozen_gap_review_surface["state"].as_str() == Some("review_required"))
    {
        return None;
    }
    let blocker_code = frozen_gap_review_surface["blocking_component"]
        .as_str()
        .unwrap_or("unknown_blocker");
    let irrecoverable_missing_live_events =
        frozen_gap_review_surface["irrecoverable_missing_live_events"]
            .as_u64()
            .unwrap_or(0);
    let tooltip = format!(
        "Этот ряд показывает, что старый исторический хвост больше не растёт в текущих запросах.\n- Текущая сессия: точная пара уже собрана\n- Рабочее окно: точная пара уже собрана\n- Что ещё мешает накопительной истории: {}\n- Строк без восстановления: {}\n- Значит проблема остаётся только в старом историческом хвосте.",
        blocker_code,
        format_u64(Some(irrecoverable_missing_live_events)),
    );
    Some(metric_row(
        "Исторический frozen debt",
        format!(
            "{}: historical-only, {} rows",
            blocker_code,
            format_u64(Some(irrecoverable_missing_live_events))
        ),
        Some(tooltip.as_str()),
    ))
}

fn reviewed_frozen_debt_export_metric_row(alignment: &Value) -> Option<Value> {
    let surface = &alignment["reviewed_frozen_debt_export_surface"];
    if surface["export_ready_report_only"].as_bool() != Some(true) {
        return None;
    }
    let surface_kind = surface["surface_kind"]
        .as_str()
        .unwrap_or("reviewed_frozen_debt_report_only");
    let blocker_code = surface["blocking_component"]
        .as_str()
        .unwrap_or("unknown_blocker");
    let irrecoverable_missing_live_events = surface["irrecoverable_missing_live_events"]
        .as_u64()
        .unwrap_or(0);
    let allowed_claims = surface["allowed_claims"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let forbidden_claims = surface["forbidden_claims"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let propagated_surfaces = surface["propagated_surfaces"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let review_bundle_command = surface["review_bundle_command"]
        .as_str()
        .unwrap_or_default();
    let evidence_pack_command = surface["evidence_pack_command"]
        .as_str()
        .unwrap_or_default();
    let tooltip = format!(
        "Этот ряд показывает отдельный отчёт для ручной сверки старого исторического хвоста.\n- Вид отчёта: {}\n- Что мешает восстановлению: {}\n- Строк без восстановления: {}\n- Что можно подтверждать этим отчётом: {}\n- Что этим отчётом подтверждать нельзя: {}\n- Куда он передаётся дальше: {}\n- Команда для сверки: {}\n- Команда для пакета доказательств: {}\n- Этот отчёт не превращает старый хвост в точную полную историю.",
        surface_kind,
        blocker_code,
        format_u64(Some(irrecoverable_missing_live_events)),
        allowed_claims,
        forbidden_claims,
        propagated_surfaces,
        review_bundle_command,
        evidence_pack_command
    );
    Some(metric_row(
        "Review-only export",
        format!(
            "{}: {} irrecoverable rows",
            surface_kind,
            format_u64(Some(irrecoverable_missing_live_events))
        ),
        Some(tooltip.as_str()),
    ))
}

struct RollingWindowCopy {
    noun_phrase: String,
    locative_phrase: String,
    inside_phrase: String,
}

fn rolling_window_copy(report: &Value) -> RollingWindowCopy {
    match report["profile"]["rolling_window_hours"].as_u64() {
        Some(hours) if hours > 0 => RollingWindowCopy {
            noun_phrase: format!("окно на {} ч.", format_u64(Some(hours))),
            locative_phrase: format!("окне на {} ч.", format_u64(Some(hours))),
            inside_phrase: format!("внутри окна на {} ч.", format_u64(Some(hours))),
        },
        _ => RollingWindowCopy {
            noun_phrase: "рабочее окно".to_string(),
            locative_phrase: "рабочем окне".to_string(),
            inside_phrase: "внутри рабочего окна".to_string(),
        },
    }
}

pub(crate) fn build_active_agent_budget_session_card_from_surface(
    surface: &Value,
) -> Option<Value> {
    let agents = surface["agents"].as_array()?;
    let (status, _status_label, _status_tooltip) = active_agent_budget_card_status(surface);
    let aggregate_value = surface["aggregate"]["reply_prefix"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("Amai savings: н/д")
        .to_string();
    let mut agent_blocks = Vec::new();
    for agent in agents {
        let agent_label =
            compact_dashboard_text(agent["agent_label"].as_str(), 72, "Активный агент");
        let kpi_prefix = agent["personal_agent_kpi"]["reply_prefix"]
            .as_str()
            .unwrap_or("Amai savings: н/д");
        let limit_label = active_agent_online_limit_label(agent);
        let (limit_value, limit_tooltip) = active_agent_online_limit_field(agent);
        let kpi_tooltip = public_active_agent_kpi_tooltip_text(Some(kpi_prefix));
        agent_blocks.push(json!({
            "agent_label": agent_label,
            "agent_tooltip": Value::Null,
            "limit_label": limit_label,
            "limit_value": limit_value,
            "limit_tooltip": limit_tooltip,
            "kpi_value": kpi_prefix,
            "kpi_tooltip": kpi_tooltip,
        }));
    }
    let shared_limit = shared_active_agent_limit(&agent_blocks);
    if shared_limit.is_some() {
        for block in agent_blocks.iter_mut() {
            if let Some(root) = block.as_object_mut() {
                root.remove("limit_label");
                root.remove("limit_value");
                root.remove("limit_tooltip");
            }
        }
    }
    let mut legacy_rows = Vec::new();
    if let Some((label, value, tooltip)) = shared_limit.as_ref() {
        legacy_rows.push(metric_row(label, value.to_string(), tooltip.as_deref()));
    }
    for block in &agent_blocks {
        legacy_rows.push(metric_row(
            "Агент:",
            block["agent_label"]
                .as_str()
                .unwrap_or("Активный агент")
                .to_string(),
            None,
        ));
        if let Some(limit_value) = block["limit_value"].as_str() {
            legacy_rows.push(metric_row(
                block["limit_label"]
                    .as_str()
                    .unwrap_or("Лимит клиента сейчас:"),
                limit_value.to_string(),
                block["limit_tooltip"].as_str(),
            ));
        }
        legacy_rows.push(metric_row(
            "Экономия:",
            block["kpi_value"]
                .as_str()
                .unwrap_or("Amai savings: н/д")
                .to_string(),
            block["kpi_tooltip"].as_str(),
        ));
    }
    let mut card = card_with_rows(
        "Экономия токенов за текущую сессию",
        aggregate_value,
        String::new(),
        status,
        None,
        None,
        legacy_rows,
    );
    if let Some(root) = card.as_object_mut() {
        root.insert(
            "presentation_variant".to_string(),
            Value::from("active_agent_budget_grouped_v3"),
        );
        root.insert("status_label".to_string(), Value::from(String::new()));
        root.insert("status_tooltip".to_string(), Value::Null);
        root.insert("agent_blocks".to_string(), Value::from(agent_blocks));
        if let Some((label, value, tooltip)) = shared_limit {
            root.insert("shared_limit_label".to_string(), Value::from(label));
            root.insert("shared_limit_value".to_string(), Value::from(value));
            root.insert(
                "shared_limit_tooltip".to_string(),
                tooltip.map(Value::from).unwrap_or(Value::Null),
            );
        }
    }
    Some(card)
}

pub(crate) fn build_active_agent_budget_session_card(snapshot: &Value) -> Option<Value> {
    build_active_agent_budget_session_card_from_surface(&snapshot["active_agent_budget"])
}

fn public_active_agent_limit_tooltip(label: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "н/д" {
        return Some(
            "Для этой работы точный лимит по основному и расширенному окну пока ещё не подтверждён."
                .to_string(),
        );
    }
    Some(match label {
        "Лимит клиента сейчас:" => {
            "Этот ряд показывает текущий лимит клиента по двум окнам: основному и расширенному."
                .to_string()
        }
        "Лимит этой работы сейчас:" => {
            "Этот ряд показывает текущий лимит именно этой работы по двум окнам: основному и расширенному."
                .to_string()
        }
        _ => "Этот ряд показывает текущий лимит по основному и расширенному окну."
            .to_string(),
    })
}

fn active_agent_online_limit_field(agent: &Value) -> (String, Option<String>) {
    let label = active_agent_online_limit_label(agent);
    let value = agent["personal_client_limit"]["value_text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("н/д")
        .to_string();
    let tooltip = public_active_agent_limit_tooltip(label, &value);
    (value, tooltip)
}

fn active_agent_online_limit_label(agent: &Value) -> &str {
    match agent["personal_client_limit"]["label_text"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        Some("Личный thread-limit агента:") => "Лимит этой работы сейчас:",
        Some(label) => label,
        None => "Лимит клиента сейчас:",
    }
}

fn shared_active_agent_limit(agent_blocks: &[Value]) -> Option<(String, String, Option<String>)> {
    let first = agent_blocks.first()?;
    let label = first["limit_label"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let value = first["limit_value"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if label != "Лимит клиента сейчас:" {
        return None;
    }
    let tooltip = first["limit_tooltip"].as_str().map(str::to_string);
    let same_across_blocks = agent_blocks.iter().all(|block| {
        block["limit_label"].as_str().map(str::trim) == Some(label)
            && block["limit_value"].as_str().map(str::trim) == Some(value)
            && block["limit_tooltip"].as_str().map(str::to_string) == tooltip
    });
    if !same_across_blocks {
        return None;
    }
    Some((label.to_string(), value.to_string(), tooltip))
}

fn active_agent_budget_public_row_allowed(label: &str) -> bool {
    matches!(
        label,
        "Агент:"
            | "Лимит клиента сейчас:"
            | "Лимит этой работы сейчас:"
            | "Личный thread-limit агента:"
            | "Экономия:"
    )
}

fn public_active_agent_kpi_tooltip_text(kpi_value: Option<&str>) -> Option<String> {
    let value = kpi_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Amai savings: н/д");
    Some(match value {
        "Amai savings: н/д" => {
            "Для этого активного агента пока нет полной подтверждённой пары без Amai / с Amai."
                .to_string()
        }
        _ => {
            "Для этого активного агента показана та же честная пара без Amai / с Amai.".to_string()
        }
    })
}

fn strip_public_row_operator_controls(row: &mut Value) {
    if let Some(root) = row.as_object_mut() {
        root.remove("target_selector");
    }
}

fn sanitize_public_active_agent_limit_row(row: &mut Value, public_label: &str) {
    let value = row["value"].as_str().unwrap_or_default().to_string();
    row["label"] = Value::from(public_label);
    row["tooltip"] = public_active_agent_limit_tooltip(public_label, &value)
        .map(Value::from)
        .unwrap_or(Value::Null);
}

fn compact_active_agent_budget_card(mut card: Value) -> Value {
    if let Some(rows) = card["rows"].as_array_mut() {
        rows.retain(|row| {
            row["label"]
                .as_str()
                .is_some_and(active_agent_budget_public_row_allowed)
        });
        for row in rows.iter_mut() {
            strip_public_row_operator_controls(row);
            match row["label"].as_str() {
                Some("Агент:") => {
                    row["tooltip"] = Value::Null;
                }
                Some("Лимит клиента сейчас:") => {
                    sanitize_public_active_agent_limit_row(row, "Лимит клиента сейчас:");
                }
                Some("Лимит этой работы сейчас:") => {
                    sanitize_public_active_agent_limit_row(row, "Лимит этой работы сейчас:");
                }
                Some("Личный thread-limit агента:") => {
                    sanitize_public_active_agent_limit_row(row, "Лимит этой работы сейчас:");
                }
                Some("Экономия:") => {
                    row["tooltip"] = public_active_agent_kpi_tooltip_text(row["value"].as_str())
                        .map(Value::from)
                        .unwrap_or(Value::Null);
                }
                _ => {}
            }
        }
    }
    if let Some(blocks) = card["agent_blocks"].as_array_mut() {
        for block in blocks.iter_mut() {
            if let Some(root) = block.as_object_mut() {
                root.remove("agent_scope");
                root.insert("agent_tooltip".to_string(), Value::Null);
                root.insert("kpi_label".to_string(), Value::from("Экономия:"));
                root.insert(
                    "kpi_tooltip".to_string(),
                    public_active_agent_kpi_tooltip_text(
                        root.get("kpi_value").and_then(Value::as_str),
                    )
                    .map(Value::from)
                    .unwrap_or(Value::Null),
                );
                root.remove("pressure_label");
                root.remove("pressure_value");
                root.remove("pressure_tooltip");
            }
        }
    }
    card["status_label"] = Value::from(String::new());
    card["status_tooltip"] = Value::Null;
    card["note"] = Value::from(String::new());
    if let Some(title_tooltip) =
        truth_only_token_card_title_tooltip(card["title"].as_str().unwrap_or_default())
    {
        card["title_tooltip"] = Value::String(title_tooltip);
    }
    if let Some(source_label) = truth_only_token_card_source_label(&card) {
        card["source_label"] = Value::String(source_label);
    }
    card
}

pub(crate) fn compact_token_hero_card(mut card: Value) -> Value {
    if matches!(
        card["presentation_variant"].as_str(),
        Some(
            "active_agent_budget_v1"
                | "active_agent_budget_minimal_v2"
                | "active_agent_budget_grouped_v3"
        )
    ) {
        return compact_active_agent_budget_card(card);
    }
    let title = card["title"].as_str().unwrap_or_default().to_string();
    if let Some(rows) = card["rows"].as_array_mut() {
        let allowed = truth_only_token_card_labels(&title);
        rows.retain(|row| {
            row["label"]
                .as_str()
                .is_some_and(|label| allowed.iter().any(|allowed_label| label == *allowed_label))
        });
        for row in rows {
            strip_public_row_operator_controls(row);
            if let Some(label) = row["label"].as_str() {
                row["label"] =
                    Value::String(humanize_token_card_row_label(&title, label).to_string());
            }
            if let (Some(label), Some(value)) = (row["label"].as_str(), row["value"].as_str()) {
                let label = label.to_string();
                let value = value.to_string();
                row["value"] = Value::String(
                    humanize_token_card_row_value(&title, &label, &value).to_string(),
                );
                row["tooltip"] = truth_only_token_card_row_tooltip(&title, &label, &value)
                    .map(Value::from)
                    .unwrap_or(Value::Null);
            }
        }
    }
    if let Some(source_label) = truth_only_token_card_source_label(&card) {
        card["source_label"] = Value::String(source_label);
    }
    apply_truth_only_token_card_status(&mut card);
    ensure_truth_only_status_tooltip(&mut card);
    card["note"] = Value::String(truth_only_token_card_note(&card));
    if let Some(title_tooltip) = truth_only_token_card_title_tooltip(&title) {
        card["title_tooltip"] = Value::String(title_tooltip);
    }
    card
}

fn truth_only_token_card_labels(title: &str) -> &'static [&'static str] {
    match title {
        "Экономия токенов за текущую сессию" => &[
            "Amai в полном live-turn",
            "Экономия токенов модели",
            "Главный драйвер exact-пары",
            "Совпадение с реальным лимитом",
            "Токены continuity boundary",
            "Последний запрос клиента",
            "Лимит клиента сейчас",
            "Последний observed лимит клиента",
        ],
        "Экономия токенов за рабочее окно" => &[
            "Экономия токенов модели",
            "Совпадение с реальным лимитом",
            "Токены continuity boundary",
            "Исторический startup-хвост",
        ],
        "Экономия токенов за всё время записи" => &[
            "Экономия токенов модели",
            "Совпадение с реальным лимитом",
            "Токены continuity boundary",
            "Исторический frozen debt",
            "Review-only export",
        ],
        _ => &[],
    }
}

fn compact_public_card_value_is_undetermined(card: &Value) -> bool {
    card["value"].as_str().map(str::trim) == Some("не доказано")
}

fn compact_public_card_value_is_negative(card: &Value) -> bool {
    card["value"]
        .as_str()
        .map(str::trim)
        .is_some_and(|value| value.starts_with('-'))
}

fn compact_public_card_status_label(card: &Value) -> Option<&str> {
    card["status_label"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn compact_public_card_operator_only_status(label: &str) -> bool {
    matches!(
        label,
        "новый чат нужен сейчас"
            | "новый чат рекомендован"
            | "сожми текущий чат"
            | "сожми текущий чат сейчас"
    ) || (label.starts_with("цель ") && label.ends_with("% не достигнута"))
}

fn apply_truth_only_token_card_status(card: &mut Value) {
    let title = card["title"].as_str().unwrap_or_default();
    let Some(status_label) = compact_public_card_status_label(card).map(str::to_string) else {
        return;
    };
    match title {
        "Экономия токенов за текущую сессию" => {
            if compact_public_card_operator_only_status(&status_label) {
                if compact_public_card_value_is_undetermined(card) {
                    card["status"] = Value::from("waiting");
                    card["status_label"] = Value::from("реальная экономия ещё не доказана");
                    card["status_tooltip"] = Value::from(
                        "Публичная карточка показывает только честную пару «обычный путь / путь через Amai» и уже подтверждённую часть расхода. Пока полная пара для текущего запроса ещё не собрана, точную экономию на полной шкале показать нельзя.",
                    );
                } else if compact_public_card_value_is_negative(card) {
                    card["status"] = Value::from("alert");
                    card["status_label"] = Value::from("на полной шкале сейчас перерасход");
                    card["status_tooltip"] = Value::from(
                        "На полной шкале текущего запроса Amai пока уже в перерасходе.",
                    );
                } else {
                    card["status"] = Value::from("pass");
                    card["status_label"] = Value::from("экономия на полной шкале подтверждена");
                    card["status_tooltip"] = Value::from(
                        "На полной шкале текущего запроса уже есть подтверждённая положительная экономия Amai.",
                    );
                }
            } else if status_label == "burn в continuity startup" {
                card["status"] = Value::from("alert");
                card["status_label"] = Value::from("расход ушёл в подготовку контекста");
                card["status_tooltip"] = Value::from(
                    "В текущей сессии заметная часть расхода ушла на подготовку контекста, а не на полезный результат.",
                );
            } else if status_label == "реальная экономия не доказана" {
                card["status_tooltip"] = Value::from(
                    "Публичная карточка пока не может честно показать реальную экономию на полной шкале клиента, потому что полная пара ещё не подтверждена.",
                );
            }
        }
        "Экономия токенов за рабочее окно" => {
            if status_label == "burn в continuity startup" {
                card["status"] = Value::from("alert");
                card["status_label"] = Value::from("окно уходит в подготовку контекста");
                card["status_tooltip"] = Value::from(
                    "В этом рабочем окне заметная часть расхода ушла на подготовку контекста. Поэтому итог окна пока слабый или отрицательный.",
                );
            } else if status_label == "исторический startup drag" {
                card["status"] = Value::from("alert");
                card["status_label"] = Value::from("тянется хвост прошлых стартов");
                card["status_tooltip"] = Value::from(
                    "Негативный итог окна сейчас в основном тянется от хвоста прошлых стартов, а не от текущих запросов.",
                );
            }
        }
        _ => {}
    }
}

fn humanize_token_card_row_label<'a>(title: &str, label: &'a str) -> &'a str {
    match (title, label) {
        ("Экономия токенов за текущую сессию", "Amai в полном live-turn") => {
            "Экономия на реальной шкале"
        }
        ("Экономия токенов за текущую сессию", "Экономия токенов модели")
        | ("Экономия токенов за рабочее окно", "Экономия токенов модели")
        | ("Экономия токенов за всё время записи", "Экономия токенов модели") => {
            "Экономия на учтённой части"
        }
        ("Экономия токенов за текущую сессию", "Главный драйвер exact-пары") => {
            "Что именно посчитано"
        }
        (_, "Совпадение с реальным лимитом") => "Точность учтённой части",
        (_, "Токены continuity boundary") => "Подготовка контекста",
        (_, "Последний запрос клиента") => "Последний запрос в модель",
        (_, "Исторический startup-хвост") => "Хвост от прошлых стартов",
        (_, "Исторический frozen debt") => "Старый исторический хвост",
        (_, "Review-only export") => "Отчёт для ручной сверки",
        _ => label,
    }
}

fn humanize_token_card_row_value(title: &str, label: &str, value: &str) -> String {
    match (title, label) {
        ("Экономия токенов за текущую сессию", "Экономия на реальной шкале") => {
            humanize_full_turn_savings_value(value)
        }
        ("Экономия токенов за текущую сессию", "Экономия на учтённой части")
        | ("Экономия токенов за рабочее окно", "Экономия на учтённой части")
        | ("Экономия токенов за всё время записи", "Экономия на учтённой части") => {
            humanize_tracked_slice_savings_value(value)
        }
        (_, "Точность учтённой части") => {
            humanize_tracked_slice_exactness_value(value)
        }
        (_, "Подготовка контекста") => humanize_service_startup_row_value(value),
        (_, "Старый исторический хвост") => {
            if let Some((_, rows)) = value.rsplit_once(", ") {
                return format!(
                    "старый исторический хвост: {}",
                    humanize_history_row_count(rows)
                );
            }
            if let Some((_, rows)) = value.rsplit_once(": ") {
                return format!(
                    "старый исторический хвост: {}",
                    humanize_history_row_count(rows)
                );
            }
            "старый исторический хвост".to_string()
        }
        (_, "Отчёт для ручной сверки") => {
            if let Some((_, rows)) = value.rsplit_once(": ") {
                return format!(
                    "есть отдельный отчёт для ручной сверки: {}",
                    humanize_review_row_count(rows)
                );
            }
            "есть отдельный отчёт для ручной сверки".to_string()
        }
        _ => value.to_string(),
    }
}

fn humanize_full_turn_savings_value(value: &str) -> String {
    let normalized = value.replace("delta ", "экономия ");
    if let Some((pct, rest)) = normalized.split_once(": ") {
        if pct.trim_start().starts_with('-') {
            return format!(
                "На полной шкале Amai пока добавил расход {}: {}",
                pct,
                rest.replace("экономия -", "перерасход ")
            );
        }
        return format!("На полной шкале Amai сэкономил {}: {rest}", pct);
    }
    normalized
}

pub(super) fn humanize_tracked_slice_savings_value(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("Предварительный учтённый same-meter срез: ")
    {
        return format!("На учтённой части пока предварительно: {rest}");
    }
    if let Some(rest) = value.strip_prefix("Учтённый same-meter срез: ") {
        return format!("На учтённой части: {rest}");
    }
    if let Some(rest) = value.strip_prefix("Точного процента пока нет; ") {
        return format!("По полной шкале точного процента пока нет; на учтённой части {rest}");
    }
    value.to_string()
}

pub(super) fn humanize_tracked_slice_exactness_value(value: &str) -> String {
    if value == "цифра точная: полностью совпадает со шкалой лимита модели"
    {
        return "учтённая часть посчитана точно по той же шкале клиента".to_string();
    }
    if let Some(rest) = value.strip_prefix("цифра пока не полностью точная: ")
    {
        return format!("учтённая часть пока не сведена полностью: {rest}");
    }
    if let Some(rest) = value.strip_prefix("цифра пока предварительная: ") {
        return format!("учтённая часть пока предварительная: {rest}");
    }
    value.to_string()
}

fn humanize_history_row_count(value: &str) -> String {
    value.replace(" rows", " строк")
}

fn humanize_review_row_count(value: &str) -> String {
    value
        .replace(" irrecoverable rows", " строк без восстановления")
        .replace(" rows", " строк")
}

fn truth_only_token_card_row_tooltip(title: &str, label: &str, _value: &str) -> Option<String> {
    let text = match (title, label) {
        ("Экономия токенов за текущую сессию", "Экономия на реальной шкале") => {
            "Полная подтверждённая пара «обычный путь / путь через Amai» для текущего запроса."
        }
        (_, "Экономия на учтённой части") => {
            "Уже подтверждённая часть расхода, которую можно честно сравнить на одной шкале."
        }
        (_, "Что именно посчитано") => {
            "Показывает, какая часть расхода уже вошла в честный расчёт."
        }
        (_, "Точность учтённой части") => {
            "Показывает, насколько подтверждённая часть уже сведена с реальной шкалой клиента."
        }
        (_, "Подготовка контекста") => {
            "Показывает, сколько токенов ушло на подготовку контекста и восстановление рабочего состояния."
        }
        (_, "Последний запрос в модель") => {
            "Последний полностью увиденный запрос в модель на той же шкале клиента."
        }
        (_, "Лимит клиента сейчас") | (_, "Последний observed лимит клиента") => {
            "Текущий лимит клиента по основной и расширенной шкале."
        }
        (_, "Хвост от прошлых стартов") => {
            "Показывает, какая часть окна тянется от прошлых запусков, а не от текущих запросов."
        }
        (_, "Старый исторический хвост") => {
            "Старый исторический хвост, который пока нельзя восстановить до точной полной истории."
        }
        (_, "Отчёт для ручной сверки") => {
            "Отдельный отчёт для ручной сверки старого исторического хвоста."
        }
        _ => return None,
    };
    Some(text.to_string())
}

fn humanize_service_startup_row_value(value: &str) -> String {
    if let Some((_, tokens)) = value.split_once(':') {
        return format!("на подготовку контекста: {}", tokens.trim());
    }
    value
        .replace("continuity-restore", "подготовка контекста")
        .replace("continuity startup", "подготовка контекста")
}

fn public_status_tooltip_contains_internal_terms(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    [
        "continuity boundary",
        "retrieval/workflow",
        "live-turn",
        "same-meter",
        "materialized",
        "verified",
        "startup drag",
        "frozen debt",
        "burn-guard",
        "operator",
        "target ",
        "rotate",
        "thread-limit",
        "active thread",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn truth_only_status_tooltip(card: &Value) -> Option<String> {
    let title = card["title"].as_str().unwrap_or_default();
    let status = card["status"].as_str().unwrap_or_default();
    let status_label = card["status_label"].as_str().unwrap_or_default();
    let value = card["value"].as_str().unwrap_or_default().trim();
    let text = match title {
        "Экономия токенов за текущую сессию" => {
            if status_label == "расход ушёл в подготовку контекста" {
                "В текущей сессии заметная часть расхода ушла на подготовку контекста, а не на полезный результат."
            } else if status_label == "реальная экономия ещё не доказана"
                || compact_public_card_value_is_undetermined(card)
            {
                "Для текущей сессии пока нет полной подтверждённой пары «обычный путь / путь через Amai», поэтому точную экономию на полной шкале клиента ещё нельзя честно показать."
            } else if status_label == "на полной шкале сейчас перерасход" || value.starts_with('-')
            {
                "На полной шкале текущего запроса Amai пока добавляет расход, а не экономит его."
            } else if status == "pass" || status_label == "экономия на полной шкале подтверждена"
            {
                "На полной шкале текущего запроса уже есть подтверждённая экономия без дополнительных подсказок."
            } else {
                "Карточка показывает только проверяемые цифры по текущей сессии без дополнительных подсказок."
            }
        }
        "Экономия токенов за рабочее окно" => {
            if status_label == "окно уходит в подготовку контекста" {
                "В этом рабочем окне заметная часть расхода ушла на подготовку контекста. Поэтому итог окна пока слабый или отрицательный."
            } else if status_label == "тянется хвост прошлых стартов" {
                "Негативный итог окна сейчас в основном тянется от хвоста прошлых стартов, а не от текущих запросов."
            } else if status == "alert" && value.starts_with('-') {
                "По подтверждённой части рабочего окна Amai пока выходит в перерасход."
            } else if status == "waiting" {
                "По рабочему окну пока не набралась подтверждённая выборка для честного итогового процента."
            } else if status == "pass" {
                "По подтверждённой части рабочего окна Amai сейчас экономит токены."
            } else {
                "Карточка показывает только проверяемые цифры по рабочему окну."
            }
        }
        "Экономия токенов за всё время записи" => {
            if status == "alert" && value.starts_with('-') {
                "По подтверждённой части всей истории Amai пока выходит в перерасход."
            } else if status == "waiting" {
                "По всей истории пока не набралась подтверждённая выборка для честного накопительного итога."
            } else if status == "pass" {
                "По подтверждённой части всей истории Amai сейчас экономит токены."
            } else {
                "Карточка показывает только подтверждённые цифры за всё время записи."
            }
        }
        _ => return None,
    };
    Some(text.to_string())
}

fn ensure_truth_only_status_tooltip(card: &mut Value) {
    let should_replace = card["status_tooltip"]
        .as_str()
        .map(|value| {
            value.trim().is_empty() || public_status_tooltip_contains_internal_terms(value)
        })
        .unwrap_or(true);
    if should_replace {
        card["status_tooltip"] = truth_only_status_tooltip(card)
            .map(Value::from)
            .unwrap_or(Value::Null);
    }
}

fn truth_only_token_card_title_tooltip(title: &str) -> Option<String> {
    let text = match title {
        "Экономия токенов за текущую сессию" => {
            "Показывает только проверяемые цифры по текущей сессии: итоговую экономию, текущий лимит клиента и уже подтверждённую часть расхода."
        }
        "Экономия токенов за рабочее окно" => {
            "Показывает только проверяемые цифры по рабочему окну. Процент здесь относится к уже подтверждённой части, а не ко всему расходу клиента за окно."
        }
        "Экономия токенов за всё время записи" => {
            "Показывает только подтверждённые цифры за всё время записи. Процент здесь относится к уже подтверждённой части, а старый исторический хвост вынесен отдельно."
        }
        _ => return None,
    };
    Some(text.to_string())
}

fn truth_only_token_card_source_label(card: &Value) -> Option<String> {
    let title = card["title"].as_str()?;
    let source = match title {
        "Экономия токенов за текущую сессию" => {
            "Источник: текущая шкала клиента и отдельно подтверждённая часть уже проверенных запросов."
        }
        "Экономия токенов за рабочее окно" => {
            "Источник: подтверждённая часть рабочего окна и отдельно отмеченный хвост прошлых стартов. Это не весь расход клиента за окно."
        }
        "Экономия токенов за всё время записи" => {
            "Источник: подтверждённая история плюс отдельно отмеченный старый исторический хвост. Это не весь расход за всё время."
        }
        _ => return None,
    };
    Some(source.to_string())
}

fn truth_only_token_card_note(card: &Value) -> String {
    let title = card["title"].as_str().unwrap_or_default();
    let status_label = card["status_label"]
        .as_str()
        .unwrap_or(card["status"].as_str().unwrap_or("неизвестно"));
    match title {
        "Экономия токенов за текущую сессию" => {
            match card["value"].as_str() {
                Some("не доказано") => format!(
                    "Короткая карточка показывает только проверяемые цифры по текущей сессии: общая экономия появится после полного подтверждения, а строка «Экономия на учтённой части» до этого показывает только уже проверенную часть. Поэтому до полного подтверждения эти цифры могут различаться. Подсказки управления вынесены отдельно. Статус: {status_label}."
                ),
                _ => format!(
                    "Короткая карточка показывает только проверяемые цифры по текущей сессии: сверху общий подтверждённый итог, ниже уже проверенная часть. Если нижняя строка ещё предварительная, это значит, что полное подтверждение сессии ещё не закончено. Подсказки управления вынесены отдельно. Статус: {status_label}."
                ),
            }
        }
        "Экономия токенов за рабочее окно" => {
            format!(
                "Короткая карточка только с проверяемыми цифрами по рабочему окну. Процент здесь относится к подтверждённой части, а не ко всему расходу клиента за окно. Подсказки управления вынесены отдельно. Статус: {status_label}."
            )
        }
        "Экономия токенов за всё время записи" => {
            format!(
                "Короткая карточка только с подтверждёнными цифрами за всё время записи. Процент здесь относится к подтверждённой части, а старый исторический хвост вынесен отдельно. Подсказки управления вынесены отдельно. Статус: {status_label}."
            )
        }
        _ => card["note"].as_str().unwrap_or_default().to_string(),
    }
}

fn build_current_session_hero_card(snapshot: &Value) -> Value {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    let current_session = &report["current_session"];
    let current_session_statement = &report["statement_previews"]["current_session"];
    let client_live_meter = &report["client_live_meter"];
    let current_session_alignment = &current_session_statement["client_limit_meter_alignment"];
    let current_session_exact_pair =
        exact_model_token_pair(current_session_statement, current_session_alignment);
    let session_events_total = current_session["events_total"].as_u64().unwrap_or(0);
    let session_events = current_session["counted_events"].as_u64().unwrap_or(0);
    let session_saved = current_session_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| current_session["verified_effective_saved_tokens"].as_i64());
    let session_percent = current_session_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| current_session["verified_effective_savings_pct"].as_f64());
    let session_started = current_session["started_at_epoch_ms"].as_u64();
    let session_ended = current_session["ended_at_epoch_ms"].as_u64();
    let session_raw_baseline = current_session["total_naive_tokens"]
        .as_u64()
        .or_else(|| current_session["baseline_tokens"].as_u64());
    let session_raw_delivered = current_session["total_context_tokens"]
        .as_u64()
        .or_else(|| current_session["delivered_tokens"].as_u64());
    let session_raw_percent = current_session["effective_savings_pct"].as_f64();
    let session_recovery = current_session["median_recovery_tokens"].as_f64();
    let session_answer_rate = current_session["answer_like_rate"].as_f64();
    let session_answer_count = current_session["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let session_answer_percent = current_session["verified_answer_like_savings_pct"].as_f64();

    let mut session_note = if session_events > 0 {
        format!(
            "Текущая сессия — это непрерывная работа без паузы дольше 30 минут. Длительность: {}. В главный итог уже вошли {} из {} живых запросов. Проверенная экономия по ним: {}. {}",
            elapsed_since_epoch_label(session_started, session_ended),
            format_u64(Some(session_events)),
            format_u64(Some(session_events_total)),
            format_percent(session_percent),
            recovery_sentence(session_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от всей выборки, экономия по ним: {}.",
            format_count_with_word(session_answer_count, "случай", "случая", "случаев"),
            format_percent(session_answer_rate),
            format_percent(session_answer_percent)
        ) + if current_session_exact_pair.is_some() {
            " Нижние строки ниже разделяют внутренний retrieval-KPI Amai и exact model-meter breakdown."
        } else {
            " Подробные цифры по главному итогу, всему живому потоку и тому, что пока вне главного итога, вынесены в нижние строки."
        }
    } else if session_events_total > 0 {
        format!(
            "В этой сессии уже есть Amai-запросы: {}. Но пока ни один случай ещё не подтвердился как полезный без потери качества. Поэтому главный итог по сессии ещё не накоплен.",
            format_u64(Some(session_events_total)),
        ) + &format!(
            " {} {}",
            raw_savings_sentence(
                session_raw_baseline,
                session_raw_delivered,
                session_raw_percent
            ),
            client_budget_disclaimer()
        )
    } else {
        "В текущей непрерывной сессии Amai ещё не накопил ни одного учтённого запроса, поэтому реальную экономию пока рано показывать.".to_string()
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(current_session_alignment) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(current_session_statement, current_session_alignment)
    {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(current_session_alignment) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_live_turn_exact_pair = live_turn_exact_pair(
        current_session,
        client_live_meter,
        current_session_exact_pair,
    );
    let session_live_turn_exact_pair =
        current_live_turn_exact_pair(&report["current_live_turn"]).or(session_live_turn_exact_pair);
    let restore_context = &snapshot["latest_repo_working_state_restore"]["working_state_restore"];
    let client_budget_target_percent =
        client_budget_target_percent_from_inputs(report, restore_context);
    let client_budget_target_active = client_budget_target_active(client_budget_target_percent);
    let client_budget_target_percent_f64 =
        client_budget_target_percent_f64(client_budget_target_percent);
    let host_context_compaction = latest_host_context_compaction_payload(report, restore_context);
    let host_context_compaction_stage =
        host_context_compaction_stage_from_payload(&host_context_compaction);
    let (
        host_current_thread_control,
        _host_current_thread_control_effect,
        same_thread_compaction_preferred,
    ) = selected_host_current_thread_control_state(
        report,
        restore_context,
        client_live_meter,
        &host_context_compaction,
    );
    let session_rotate_bundle = restore_context.is_object().then(|| {
        working_state::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
            restore_context["project"]["code"].as_str(),
            restore_context["namespace"]["code"].as_str(),
            restore_context["project"]["repo_root"].as_str(),
            restore_context["execctl_resume_state"]
                .as_str()
                .is_some_and(|value| value != "clear"),
            restore_context["current_goal"].as_str(),
            restore_context["next_step"].as_str(),
            host_context_compaction_stage,
            same_thread_compaction_preferred,
            host_current_thread_control["thread_id"].as_str(),
            host_current_thread_control["command_id"].as_str(),
        )
    });
    if let Some(sentence) =
        client_live_meter_note_sentence(client_live_meter, session_live_turn_exact_pair)
    {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_full_turn_savings_pct =
        full_turn_savings_pct_from_live_meter(client_live_meter, session_live_turn_exact_pair);
    let session_client_turn_pressure = client_turn_pressure_guard_with_target(
        client_live_meter,
        session_live_turn_exact_pair,
        &report["client_limit_hourly_burn"],
        &report["current_live_turn"],
        client_budget_target_percent,
    );
    if let Some(sentence) = client_turn_pressure_note_sentence_for_preference(
        session_client_turn_pressure,
        same_thread_compaction_preferred,
    ) {
        session_note.push(' ');
        session_note.push_str(&sentence);
    }
    let session_boundary_pressure =
        continuity_boundary_pressure(current_session, current_session_alignment);
    if let Some((boundary_tokens, strict_tokens)) = session_boundary_pressure {
        session_note.push(' ');
        session_note.push_str(&continuity_boundary_pressure_sentence(
            boundary_tokens,
            strict_tokens,
        ));
    }
    let mut session_rows =
        current_session_lane_rows(current_session, current_session_exact_pair.is_some());
    if let Some(row) =
        client_full_turn_savings_metric_row(client_live_meter, session_live_turn_exact_pair)
    {
        session_rows.push(row);
    }
    session_rows.push(model_token_savings_metric_row(
        current_session_statement,
        current_session_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_live_context_metric_row(client_live_meter) {
        session_rows.push(row);
    }
    if let Some(row) = client_live_limit_metric_row(client_live_meter) {
        session_rows.push(row);
    }
    if let Some(row) = client_turn_pressure_metric_row(
        session_client_turn_pressure,
        session_rotate_bundle.as_ref(),
        same_thread_compaction_preferred,
    ) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(current_session_alignment) {
        session_rows.push(row);
    }
    let session_status = if let Some(guard) = session_client_turn_pressure {
        guard.severity
    } else if session_boundary_pressure.is_some_and(|(boundary_tokens, strict_tokens)| {
        continuity_boundary_pressure_is_alert(session_saved, boundary_tokens, strict_tokens)
    }) {
        "alert"
    } else if client_budget_target_active
        && session_full_turn_savings_pct
            .is_some_and(|value| value < client_budget_target_percent_f64)
    {
        "alert"
    } else {
        savings_status(session_saved, session_events, session_events_total)
    };
    let mut session_card = card_with_rows(
        "Экономия токенов за текущую сессию",
        session_full_turn_savings_pct
            .map(|value| format_percent(Some(value)))
            .unwrap_or_else(|| "не доказано".to_string()),
        session_note,
        session_status,
        None,
        Some("Эта карточка показывает, сколько токенов Amai сэкономил в текущем непрерывном заходе работы. Новый заход начинается после паузы дольше 30 минут. В главный итог попадают только те живые запросы, которые уже подтвердились как полезные без потери качества. Нижние строки нужны, чтобы показать разницу между главным итогом и всем живым потоком.".to_string()),
        session_rows,
    );
    if let Some(guard) = session_client_turn_pressure {
        session_card = with_status_label(
            session_card,
            client_turn_pressure_display_status_label(
                guard.status_label,
                same_thread_compaction_preferred,
            ),
        );
        session_card = with_status_tooltip(
            session_card,
            "Публичная карточка показывает только проверяемую пару и уже подтверждённую часть расхода. Отдельные подсказки по размеру текущего запроса вынесены за пределы этой карточки.",
        );
    } else if session_full_turn_savings_pct.is_none()
        && current_session_client_live_meter_available(client_live_meter)
    {
        session_card = with_status(session_card, "alert");
        session_card = with_status_label(session_card, "реальная экономия не доказана");
        session_card = with_status_tooltip(
            session_card,
            "Для текущего запроса пока нет полной подтверждённой пары «обычный путь / путь через Amai». Поэтому реальную экономию на полной шкале клиента пока нельзя честно показать числом.",
        );
    } else if let Some(full_turn_savings_pct) = session_full_turn_savings_pct
        .filter(|value| client_budget_target_active && *value < client_budget_target_percent_f64)
    {
        session_card = with_status_label(
            session_card,
            &client_budget_target_alert_label(client_budget_target_percent),
        );
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "Реальная экономия на полной шкале клиента сейчас всего {}.\n- {}\n- Это значит, что на полной шкале клиента экономия пока остаётся ограниченной.",
                format_percent(Some(full_turn_savings_pct)),
                client_budget_target_sentence(client_budget_target_percent)
            ),
        );
    } else if let Some((boundary_tokens, strict_tokens)) =
        session_boundary_pressure.filter(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(session_saved, *boundary_tokens, *strict_tokens)
        })
    {
        session_card = with_status_label(session_card, "расход ушёл в подготовку контекста");
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "В этой сессии подтверждённая экономия пока не вышла в плюс.\n- На подготовку контекста уже ушло {} токенов.\n- Подтверждённая полезная часть клиентского запроса пока даёт только {} токенов.\n- Значит заметная часть расхода сейчас уходит в подготовку контекста, а не в полезный результат.",
                format_u64(Some(boundary_tokens)),
                format_u64(Some(strict_tokens))
            ),
        );
    } else if session_events_total > 0 && session_events == 0 {
        session_card = with_status_tooltip(
            session_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В этой сессии уже были живые запросы.\n- Но пока ни один из них ещё не подтвердился как полезный без потери качества.\n- Как только появится первый такой случай, главный итог этой карточки начнёт считаться.",
        );
    } else if session_events > 0 && session_saved.unwrap_or_default() < 0 {
        session_card = with_status_tooltip(
            session_card,
            &format!(
                "{CLIENT_BUDGET_ADVISORY_STATUS_PREFIX}\n- В подтверждённой части текущей сессии экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai вышел тяжелее обычного пути без Amai.\n- Нижние строки со всем живым потоком показаны отдельно и не отменяют этот итог.",
                format_signed_count(session_saved)
            ),
        );
    }
    if session_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) =
            exact_pair_card_status_override(current_session_alignment)
        {
            session_card = with_status(session_card, status);
            session_card = with_status_label(session_card, label);
            session_card = with_status_tooltip(session_card, &tooltip);
        }
    }
    session_card
}

pub(super) fn build_hero_cards(snapshot: &Value) -> Vec<Value> {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    let lifetime = &report["lifetime"];
    let rolling_window = &report["rolling_window"];
    let rolling_window_statement = &report["statement_previews"]["rolling_window"];
    let lifetime_statement = &report["statement_previews"]["lifetime"];
    let lifetime_statement_export = &report["statement_export_previews"]["lifetime"];
    let rolling_window_alignment = &rolling_window_statement["client_limit_meter_alignment"];
    let lifetime_alignment = &lifetime_statement["client_limit_meter_alignment"];
    let current_session_statement = &report["statement_previews"]["current_session"];
    let current_session_alignment = &current_session_statement["client_limit_meter_alignment"];
    let current_session = &report["current_session"];
    let current_session_exact_pair =
        exact_model_token_pair(current_session_statement, current_session_alignment);
    let rolling_window_exact_pair =
        exact_model_token_pair(rolling_window_statement, rolling_window_alignment);
    let lifetime_exact_pair = exact_model_token_pair(lifetime_statement, lifetime_alignment);
    let lifetime_events_total = lifetime["events_total"].as_u64().unwrap_or(0);
    let lifetime_events = lifetime["counted_events"].as_u64().unwrap_or(0);
    let lifetime_saved = lifetime_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| lifetime["verified_effective_saved_tokens"].as_i64());
    let lifetime_percent = lifetime_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| lifetime["verified_effective_savings_pct"].as_f64());
    let lifetime_started = lifetime["started_at_epoch_ms"].as_u64();
    let lifetime_ended = lifetime["ended_at_epoch_ms"].as_u64();
    let rolling_events_total = rolling_window["events_total"].as_u64().unwrap_or(0);
    let rolling_events = rolling_window["counted_events"].as_u64().unwrap_or(0);
    let rolling_saved = rolling_window_exact_pair
        .as_ref()
        .map(|(_, _, saved, _)| *saved)
        .or_else(|| rolling_window["verified_effective_saved_tokens"].as_i64());
    let rolling_percent = rolling_window_exact_pair
        .as_ref()
        .map(|(_, _, _, pct)| *pct)
        .or_else(|| rolling_window["verified_effective_savings_pct"].as_f64());
    let rolling_started = rolling_window["started_at_epoch_ms"].as_u64();
    let rolling_ended = rolling_window["ended_at_epoch_ms"].as_u64();
    let rolling_window_label = report["profile"]["display_name"]
        .as_str()
        .unwrap_or("рабочее окно");
    let rolling_window_copy = rolling_window_copy(report);
    let rolling_recovery = rolling_window["median_recovery_tokens"].as_f64();
    let lifetime_recovery = lifetime["median_recovery_tokens"].as_f64();
    let rolling_answer_rate = rolling_window["answer_like_rate"].as_f64();
    let rolling_answer_count = rolling_window["answer_like_counted_events"]
        .as_u64()
        .unwrap_or(0);
    let rolling_answer_percent = rolling_window["verified_answer_like_savings_pct"].as_f64();
    let lifetime_answer_rate = lifetime["answer_like_rate"].as_f64();
    let lifetime_answer_count = lifetime["answer_like_counted_events"].as_u64().unwrap_or(0);
    let lifetime_answer_percent = lifetime["verified_answer_like_savings_pct"].as_f64();

    let mut session_card = build_current_session_hero_card(snapshot);

    let mut rolling_note = if rolling_events > 0 {
        format!(
            "{} — это скользящее {} за {}. В главный итог уже вошли {} из {} живых запросов. Проверенная экономия по ним: {}. {}",
            rolling_window_label,
            rolling_window_copy.noun_phrase,
            elapsed_since_epoch_label(rolling_started, rolling_ended),
            format_u64(Some(rolling_events)),
            format_u64(Some(rolling_events_total)),
            format_percent(rolling_percent),
            recovery_sentence(rolling_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от проверенной части окна, экономия по ним: {}.",
            format_count_with_word(rolling_answer_count, "случай", "случая", "случаев"),
            format_percent(rolling_answer_rate),
            format_percent(rolling_answer_percent)
        ) + if current_session_exact_pair.is_some() {
            " Нижние строки показывают, как verified window соотносится с exact model-meter slice и historical startup drag внутри окна."
        } else {
            " Нижние строки ниже разделяют verified window итог, exact model-meter slice и historical startup drag внутри окна."
        }
    } else if rolling_events_total > 0 {
        format!(
            "В {} уже было {} Amai-запросов, но пока ни один случай ещё не подтвердился как полезный без потери качества. Поэтому verified итог окна ещё не накоплен.",
            rolling_window_copy.locative_phrase,
            format_u64(Some(rolling_events_total)),
        )
    } else {
        format!(
            "В {} пока нет ни одного учтённого Amai-запроса, поэтому verified экономия по рабочему окну ещё не считается.",
            rolling_window_copy.locative_phrase
        )
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(rolling_window_alignment) {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(rolling_window_statement, rolling_window_alignment)
    {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(rolling_window_alignment) {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    let rolling_historical_startup_drag = historical_startup_drag(
        current_session_exact_pair,
        rolling_window_exact_pair,
        current_session,
        rolling_window,
    );
    if let Some(sentence) = historical_startup_drag_note_sentence(rolling_historical_startup_drag) {
        rolling_note.push(' ');
        rolling_note.push_str(&sentence);
    }
    let rolling_boundary_pressure =
        continuity_boundary_pressure(rolling_window, rolling_window_alignment);
    if let Some((boundary_tokens, strict_tokens)) = rolling_boundary_pressure {
        rolling_note.push(' ');
        rolling_note.push_str(&continuity_boundary_pressure_sentence(
            boundary_tokens,
            strict_tokens,
        ));
    }
    let mut rolling_rows = Vec::new();
    rolling_rows.push(model_token_savings_metric_row(
        rolling_window_statement,
        rolling_window_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = historical_startup_drag_metric_row(rolling_historical_startup_drag) {
        rolling_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(rolling_window_alignment) {
        rolling_rows.push(row);
    }
    let mut rolling_card = card_with_rows(
        "Экономия токенов за рабочее окно",
        format_signed_count(rolling_saved),
        rolling_note,
        savings_status(rolling_saved, rolling_events, rolling_events_total),
        None,
        Some(format!(
            "Эта карточка показывает verified-экономию по рабочему окну {}. Это не лимит клиента и не накопление за все времена, а только подтверждённые живые запросы {}.",
            rolling_window_label, rolling_window_copy.inside_phrase
        )),
        rolling_rows,
    );
    if rolling_events_total > 0 && rolling_events == 0 {
        rolling_card = with_status_tooltip(
            rolling_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В рабочем окне уже были живые запросы.\n- Но пока ни один из них ещё не подтвердился как полезный без потери качества.\n- Поэтому verified итог окна ещё не накоплен.",
        );
    } else if let Some((boundary_tokens, strict_tokens)) =
        rolling_boundary_pressure.filter(|(boundary_tokens, strict_tokens)| {
            continuity_boundary_pressure_is_alert(rolling_saved, *boundary_tokens, *strict_tokens)
        })
    {
        rolling_card = with_status(rolling_card, "alert");
        rolling_card = with_status_label(rolling_card, "окно уходит в подготовку контекста");
        rolling_card = with_status_tooltip(
            rolling_card,
            &format!(
                "В рабочем окне экономия сейчас не положительная: {}.\n- На подготовку контекста уже ушло {} токенов.\n- Подтверждённая полезная часть пока даёт только {} токенов.\n- Значит заметная часть расхода окна сейчас уходит в подготовку контекста, а не в полезный результат.",
                format_signed_count(rolling_saved),
                format_u64(Some(boundary_tokens)),
                format_u64(Some(strict_tokens))
            ),
        );
    } else if rolling_events > 0
        && rolling_saved.unwrap_or_default() < 0
        && rolling_historical_startup_drag.is_some()
    {
        rolling_card = with_status_label(rolling_card, "исторический startup drag");
        rolling_card = with_status_tooltip(
            rolling_card,
            &format!(
                "{CLIENT_BUDGET_ADVISORY_STATUS_PREFIX}\n- В verified рабочем окне экономия сейчас отрицательная: {}.\n- При этом текущая сессия уже в плюсе, значит отрицательный итог окна в основном идёт от старого startup-хвоста {}.\n- Этот хвост вынесен в отдельную строку ниже, чтобы не смешивать его с текущим live-turn effect.",
                format_signed_count(rolling_saved),
                rolling_window_copy.inside_phrase
            ),
        );
    } else if rolling_events > 0 && rolling_saved.unwrap_or_default() < 0 {
        rolling_card = with_status_tooltip(
            rolling_card,
            &format!(
                "{CLIENT_BUDGET_ADVISORY_STATUS_PREFIX}\n- В verified рабочем окне экономия сейчас отрицательная: {}.\n- Это значит, что в уже подтверждённых случаях Amai пока выходил тяжелее обычного пути без Amai.",
                format_signed_count(rolling_saved)
            ),
        );
    }
    if rolling_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) =
            exact_pair_card_status_override(rolling_window_alignment)
        {
            rolling_card = with_status(rolling_card, status);
            rolling_card = with_status_label(rolling_card, label);
            rolling_card = with_status_tooltip(rolling_card, &tooltip);
        }
    }

    let mut lifetime_note = if lifetime_events > 0 {
        format!(
            "Это накопительный verified итог за всю историю текущей установки Amai. Длительность: {}. В главный итог уже вошли {} из {} живых запросов. Проверенная экономия по ним: {}. {}",
            elapsed_since_epoch_label(lifetime_started, lifetime_ended),
            format_u64(Some(lifetime_events)),
            format_u64(Some(lifetime_events_total)),
            format_percent(lifetime_percent),
            recovery_sentence(lifetime_recovery)
        ) + &format!(
            " Уже есть {}, где Amai дошёл до более полного ответа без лишнего уточнения. Это {} от всей verified истории, экономия по ним: {}.",
            format_count_with_word(lifetime_answer_count, "случай", "случая", "случаев"),
            format_percent(lifetime_answer_rate),
            format_percent(lifetime_answer_percent)
        )
    } else if lifetime_events_total > 0 {
        format!(
            "В истории этой установки уже есть {} живых запросов, но пока ещё нет ни одного подтверждённого случая без потери качества. Поэтому verified накопительный итог пока не считается.",
            format_u64(Some(lifetime_events_total)),
        )
    } else {
        "В истории текущей установки пока нет ни одного учтённого Amai-запроса, поэтому накопительная экономия ещё не считается.".to_string()
    };
    if let Some(sentence) = client_limit_alignment_note_sentence(lifetime_alignment) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) =
        model_token_savings_note_sentence(lifetime_statement, lifetime_alignment)
    {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) = exact_model_component_delta_note_sentence(lifetime_alignment) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) = reviewed_frozen_debt_export_note_sentence(lifetime_statement_export) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    if let Some(sentence) = historical_frozen_debt_note_sentence(
        current_session_alignment,
        rolling_window_alignment,
        lifetime_alignment,
    ) {
        lifetime_note.push(' ');
        lifetime_note.push_str(&sentence);
    }
    let mut lifetime_rows = Vec::new();
    lifetime_rows.push(model_token_savings_metric_row(
        lifetime_statement,
        lifetime_alignment,
    ));
    if let Some(row) = exact_pair_status_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = exact_pair_frozen_debt_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = historical_frozen_debt_metric_row(
        current_session_alignment,
        rolling_window_alignment,
        lifetime_alignment,
    ) {
        lifetime_rows.push(row);
    }
    if let Some(row) = reviewed_frozen_debt_export_metric_row(lifetime_statement_export) {
        lifetime_rows.push(row);
    }
    if let Some(row) = exact_model_component_delta_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_alignment_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_strict_slice_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_explicit_boundary_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    if let Some(row) = client_limit_boundary_tokens_metric_row(lifetime_alignment) {
        lifetime_rows.push(row);
    }
    let mut lifetime_card = card_with_rows(
        "Экономия токенов за всё время записи",
        format_signed_count(lifetime_saved),
        lifetime_note,
        savings_status(lifetime_saved, lifetime_events, lifetime_events_total),
        None,
        Some("Эта карточка показывает накопительный итог с первого записанного запроса Amai в текущей установке. Это не процент от лимита чата и не вся история всех внешних клиентов навсегда. В главный итог попадают только те живые запросы, которые уже подтвердились как полезные без потери качества; проверочные прогоны и другой инженерный трафик сюда не подмешиваются.".to_string()),
        lifetime_rows,
    );
    if lifetime_events_total > 0 && lifetime_events == 0 {
        lifetime_card = with_status_tooltip(
            lifetime_card,
            "Статус пока не может считаться нормальным по следующим причинам:\n- В истории уже есть живые запросы.\n- Но пока ещё нет ни одного подтверждённого случая без потери качества.\n- Поэтому накопительный итог ещё не может считаться надёжным.",
        );
    } else if lifetime_events > 0 && lifetime_saved.unwrap_or_default() < 0 {
        lifetime_card = with_status_tooltip(
            lifetime_card,
            &format!(
                "{CLIENT_BUDGET_ADVISORY_STATUS_PREFIX}\n- В подтверждённой части всей истории экономия сейчас отрицательная: {}.\n- Это значит, что в уже проверенных случаях контекст от Amai пока выходит тяжелее обычного пути без Amai.",
                format_signed_count(lifetime_saved)
            ),
        );
    }
    if lifetime_card["status"].as_str() == Some("pass") {
        if let Some((status, label, tooltip)) = exact_pair_card_status_override(lifetime_alignment)
        {
            lifetime_card = with_status(lifetime_card, status);
            lifetime_card = with_status_label(lifetime_card, label);
            lifetime_card = with_status_tooltip(lifetime_card, &tooltip);
        }
    }

    if let Some(active_agent_budget_card) = build_active_agent_budget_session_card(snapshot) {
        session_card = active_agent_budget_card;
    }

    vec![
        compact_token_hero_card(session_card),
        compact_token_hero_card(rolling_card),
        compact_token_hero_card(lifetime_card),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_active_agent_budget_session_card_shows_only_active_agent_kpi_and_limit() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "saving",
                    "reply_prefix": "Amai savings: без Amai 2000, с Amai 1200, экономия 800 (40.00%)"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "thread_title": "compact dashboard rewrite",
                        "personal_agent_kpi": {
                            "reply_prefix": "Amai savings: без Amai 1000, с Amai 400, экономия 600 (60.00%)",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "основное окно остаётся 43.00%, расширенное окно остаётся 23.00%",
                            "tooltip": "personal limit one"
                        },
                        "client_live_meter": {
                            "status": "observed",
                            "current_thread_bound": true,
                            "thread_binding_state": "current_thread_bound",
                            "ended_at_epoch_ms": 2000,
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
                        }
                    },
                    {
                        "agent_label": "Hunter",
                        "agent_scope": "bug_bounty::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "Amai savings: без Amai 1000, с Amai 800, экономия 200 (20.00%)",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "label_text": "Личный thread-limit агента:",
                            "value_text": "основное окно остаётся 88.00%, расширенное окно остаётся 91.00%",
                            "tooltip": "personal limit two"
                        },
                        "client_live_meter": {
                            "status": "observed",
                            "current_thread_bound": true,
                            "thread_binding_state": "current_thread_bound",
                            "ended_at_epoch_ms": 2000,
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
                        }
                    }
                ]
            }
        });
        let card = build_active_agent_budget_session_card(&snapshot).expect("card");
        assert_eq!(
            card["value"].as_str(),
            Some("Amai savings: без Amai 2000, с Amai 1200, экономия 800 (40.00%)")
        );
        assert_eq!(
            card["presentation_variant"].as_str(),
            Some("active_agent_budget_grouped_v3")
        );
        assert_eq!(card["status_label"].as_str(), Some(""));
        assert!(card["status_tooltip"].is_null());
        let rows = card["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["label"].as_str(), Some("Агент:"));
        assert_eq!(rows[1]["label"].as_str(), Some("Лимит клиента сейчас:"));
        assert_eq!(rows[2]["label"].as_str(), Some("Экономия:"));
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["agent_label"].as_str(), Some("Amai"));
        assert!(blocks[0]["agent_tooltip"].is_null());
        assert!(blocks[0].get("agent_scope").is_none());
        assert!(
            blocks[0]["limit_value"]
                .as_str()
                .is_some_and(|value| value.contains("основное окно остаётся 43.00%"))
        );
        assert!(
            blocks[0]["limit_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("текущий лимит клиента"))
        );
        assert_eq!(
            blocks[0]["limit_label"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert_eq!(
            blocks[0]["kpi_value"].as_str(),
            Some("Amai savings: без Amai 1000, с Amai 400, экономия 600 (60.00%)")
        );
        assert!(
            blocks[0]["kpi_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("честная пара без Amai / с Amai"))
        );
        assert_eq!(blocks[1]["agent_label"].as_str(), Some("Hunter"));
        assert!(blocks[1]["agent_tooltip"].is_null());
        assert!(blocks[1].get("agent_scope").is_none());
        assert!(
            blocks[1]["limit_value"]
                .as_str()
                .is_some_and(|value| value.contains("основное окно остаётся 88.00%"))
        );
        assert_eq!(
            blocks[1]["limit_label"].as_str(),
            Some("Лимит этой работы сейчас:")
        );
        assert!(
            blocks[1]["limit_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("текущий лимит именно этой работы"))
        );
        assert_eq!(
            blocks[1]["kpi_value"].as_str(),
            Some("Amai savings: без Amai 1000, с Amai 800, экономия 200 (20.00%)")
        );
    }

    #[test]
    fn build_active_agent_budget_session_card_collapses_shared_global_limit() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "overspend",
                    "reply_prefix": "Amai savings: без Amai 2000, с Amai 2660, перерасход 660 (33.00%)"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "Amai savings: без Amai 1000, с Amai 1440, перерасход 440 (44.00%)",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "основное окно остаётся 12.00%, расширенное окно остаётся 89.00%",
                            "tooltip": "same exact global limit"
                        }
                    },
                    {
                        "agent_label": "Bug Bounty",
                        "agent_scope": "bug_bounty::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "Amai savings: без Amai 1000, с Amai 1220, перерасход 220 (22.00%)",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "основное окно остаётся 12.00%, расширенное окно остаётся 89.00%",
                            "tooltip": "same exact global limit"
                        }
                    }
                ]
            }
        });
        let card = build_active_agent_budget_session_card(&snapshot).expect("card");
        assert_eq!(
            card["shared_limit_label"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert_eq!(
            card["shared_limit_value"].as_str(),
            Some("основное окно остаётся 12.00%, расширенное окно остаётся 89.00%")
        );
        assert!(
            card["shared_limit_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("текущий лимит клиента"))
        );
        let rows = card["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0]["label"].as_str(), Some("Лимит клиента сейчас:"));
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].get("limit_label").is_none());
        assert!(blocks[0].get("limit_value").is_none());
        assert!(blocks[1].get("limit_label").is_none());
        assert!(blocks[1].get("limit_value").is_none());
    }

    #[test]
    fn build_active_agent_budget_session_card_keeps_live_turn_pressure_out_of_public_card() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "overspend",
                    "reply_prefix": "Amai savings: без Amai 1000, с Amai 2200, перерасход 1200 (120.00%)"
                },
                "agents": [
                    {
                        "agent_label": "Bug Bounty",
                        "agent_scope": "bug_bounty::continuity::default",
                        "thread_title": "Авито дальше",
                        "cwd": "/home/art/Bug-Bounty",
                        "personal_agent_kpi": {
                            "reply_prefix": "Amai savings: без Amai 1000, с Amai 2978, перерасход 1978 (197.80%)",
                            "summary": "Личная Amai savings текущего active thread идёт в перерасходе 197.80%."
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "основное окно остаётся 16.00%, расширенное окно остаётся 90.00%",
                            "tooltip": "same exact global limit"
                        },
                        "client_live_meter": {
                            "status": "observed",
                            "current_thread_bound": true,
                            "thread_binding_state": "current_thread_bound",
                            "ended_at_epoch_ms": 1775155316431u64,
                            "client_turn_total_tokens": 222596,
                            "latest_model_context_window": 258400,
                            "context_used_percent": 86.14,
                            "status_bar_rate_limits": {
                                "status": "observed",
                                "observed_at_epoch_ms": 1775155316431u64
                            }
                        }
                    }
                ]
            }
        });

        let card = build_active_agent_budget_session_card(&snapshot).expect("card");
        let rows = card["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 3);
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert!(blocks[0]["agent_tooltip"].is_null());
        assert!(blocks[0].get("agent_scope").is_none());
        assert!(blocks[0].get("pressure_value").is_none());
        assert!(blocks[0].get("pressure_tooltip").is_none());
    }

    #[test]
    fn build_active_agent_budget_session_card_uses_public_tooltip_for_missing_limit_value() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "partial",
                    "classification": "saving",
                    "reply_prefix": "Amai savings: н/д"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "Amai savings: н/д"
                        },
                        "personal_client_limit": {
                            "label_text": "Личный thread-limit агента:",
                            "value_text": ""
                        }
                    }
                ]
            }
        });

        let card = build_active_agent_budget_session_card(&snapshot).expect("card");
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0]["limit_label"].as_str(),
            Some("Лимит этой работы сейчас:")
        );
        assert_eq!(blocks[0]["limit_value"].as_str(), Some("н/д"));
        assert!(
            blocks[0]["limit_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("точный лимит"))
        );
    }

    #[test]
    fn current_session_card_explains_raw_savings_vs_client_budget() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 2,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "total_naive_tokens": 920432,
                        "total_context_tokens": 94,
                        "effective_savings_pct": 99.98978740417543
                    },
                    "rolling_window": {},
                    "lifetime": {},
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        let note = cards[0]["note"].as_str().unwrap_or_default();
        assert!(
            note.contains(
                "Короткая карточка показывает только проверяемые цифры по текущей сессии"
            )
        );
        assert!(note.contains("общая экономия появится после полного подтверждения"));
        let rows = cards[0]["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]["label"].as_str(),
            Some("Экономия на учтённой части")
        );
    }

    #[test]
    fn build_hero_cards_drops_target_selector_from_real_client_burn_path() {
        let snapshot = json!({
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "project": {
                        "code": "amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity"
                    },
                    "execctl_resume_state": "clear",
                    "current_goal": "audit public tokenonomics hero cards",
                    "next_step": "keep operator controls out of public cards"
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 1500,
                        "verified_effective_savings_pct": 10.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 100.0,
                        "answer_like_counted_events": 1,
                        "verified_answer_like_savings_pct": 10.0
                    },
                    "rolling_window": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "lifetime": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "current_live_turn": {
                        "status": "exact_pair_materialized",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 15000,
                            "with_amai_tokens": 13500,
                            "saved_tokens": 1500,
                            "saved_pct": 10.0
                        }
                    },
                    "client_live_meter": {
                        "status": "observed",
                        "client_turn_total_tokens": 13500,
                        "latest_model_context_window": 258400,
                        "context_used_percent": 5.22,
                        "primary_limit_remaining_percent": 93.0,
                        "secondary_limit_remaining_percent": 99.0
                    },
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "classification": "saving",
                        "reply_prefix": "Burn guard: экономия 50.00%",
                        "projected_primary_used_per_hour_percent": 10.0,
                        "kpi_percent": 50.0,
                        "remaining_window_minutes": 30.0,
                        "actual_remaining_percent": 75.0,
                        "ideal_remaining_percent": 50.0,
                        "latest_observed_at_epoch_ms": 2000,
                        "projected_reset_delta_minutes": 30.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_without_amai_measured_tokens": 15000,
                            "verified_with_amai_measured_tokens": 13500,
                            "verified_measured_saved_tokens": 1500,
                            "verified_measured_saved_pct": 10.0,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": {
                                    "exact_pair_available": true,
                                    "exact_pair": {
                                        "without_amai_tokens": 15000,
                                        "with_amai_tokens": 13500,
                                        "saved_tokens": 1500,
                                        "saved_pct": 10.0
                                    }
                                }
                            }
                        },
                        "rolling_window": {
                            "client_limit_meter_alignment": {}
                        },
                        "lifetime": {
                            "client_limit_meter_alignment": {}
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        let session_card = &cards[0];
        assert!(
            session_card["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .all(|row| row.get("target_selector").is_none())
        );
        assert!(
            session_card["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .all(|row| row["label"].as_str() != Some("Операционный burn-guard"))
        );
        assert!(
            !session_card["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Burn guard")
        );
    }

    #[test]
    fn hero_cards_explain_scope_and_strict_verified_fraction() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 120,
                        "verified_effective_savings_pct": 25.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 25.0,
                        "answer_like_counted_events": 1,
                        "verified_answer_like_savings_pct": 25.0,
                        "verified_baseline_tokens": 200,
                        "verified_delivered_tokens": 80,
                        "verified_recovery_tokens": 0,
                        "excluded_events_count": 3,
                        "excluded_effective_saved_tokens": 50,
                        "excluded_baseline_tokens": 400,
                        "excluded_delivered_tokens": 350,
                        "excluded_recovery_tokens": 0,
                        "total_naive_tokens": 600,
                        "total_context_tokens": 430,
                        "effective_savings_pct": 28.33,
                        "total_effective_saved_tokens": 170,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 12,
                        "counted_events": 6,
                        "verified_effective_saved_tokens": 38622,
                        "verified_effective_savings_pct": 83.29,
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 20,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 33.33,
                        "answer_like_counted_events": 6,
                        "verified_answer_like_savings_pct": 83.29
                    },
                    "lifetime": {
                        "events_total": 56,
                        "counted_events": 22,
                        "verified_effective_saved_tokens": 4824306,
                        "verified_effective_savings_pct": 99.14,
                        "started_at_epoch_ms": 100,
                        "ended_at_epoch_ms": 200,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 39.29,
                        "answer_like_counted_events": 22,
                        "verified_answer_like_savings_pct": 99.14
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("pass"));
        assert_eq!(
            cards[0]["title_tooltip"].as_str(),
            Some(
                "Показывает только проверяемые цифры по текущей сессии: итоговую экономию, текущий лимит клиента и уже подтверждённую часть расхода."
            )
        );
        assert!(cards[1]["title_tooltip"].as_str().is_some_and(|value| {
            value.contains("только проверяемые цифры по рабочему окну")
        }));
        assert!(cards[2]["title_tooltip"].as_str().is_some_and(|value| {
            value.contains("только подтверждённые цифры за всё время")
        }));
        assert!(
            cards[1]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("отдельно отмеченный хвост прошлых стартов")
        );
        assert!(
            cards[2]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("старый исторический хвост")
        );
    }

    #[test]
    fn hero_session_card_uses_waiting_status_before_verified_sample_exists() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 1,
                        "excluded_effective_saved_tokens": 243216,
                        "excluded_baseline_tokens": 243300,
                        "excluded_delivered_tokens": 84,
                        "excluded_recovery_tokens": 0,
                        "total_naive_tokens": 243300,
                        "total_context_tokens": 84,
                        "effective_savings_pct": 99.97,
                        "total_effective_saved_tokens": 243216,
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
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("waiting"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("ждём подтверждённую выборку")
        );
        assert!(
            cards[0]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("ни один из них ещё не подтвердился")
        );
        assert!(
            cards[0]["note"].as_str().unwrap_or_default().contains(
                "Короткая карточка показывает только проверяемые цифры по текущей сессии"
            )
        );
    }

    #[test]
    fn hero_cards_surface_client_limit_alignment_when_preview_is_present() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 4,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 1200,
                        "total_context_tokens": 900,
                        "effective_savings_pct": 25.0,
                        "total_effective_saved_tokens": 300,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 7,
                        "counted_events": 0,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0
                    },
                    "lifetime": {
                        "events_total": 12,
                        "counted_events": 3,
                        "verified_effective_saved_tokens": 900,
                        "verified_effective_savings_pct": 75.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "only_non_live_scope_activity",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 0,
                                "non_live_events_count": 4,
                                "blocking_reasons": [
                                    "client_prompt_unmeasured",
                                    "no_live_usage_in_scope",
                                    "non_live_events_present_in_scope"
                                ]
                            }
                        },
                        "rolling_window": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "live_usage_unconfirmed_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 2,
                                "non_live_events_count": 5,
                                "blocking_reasons": [
                                    "client_prompt_unmeasured",
                                    "no_confirmed_live_usage_in_scope"
                                ]
                            }
                        },
                        "lifetime": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "partial_lower_bound_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 12,
                                "non_live_events_count": 0,
                                "blocking_reasons": [
                                    "client_prompt_unmeasured",
                                    "assistant_generation_unmeasured"
                                ]
                            }
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        for card in &cards {
            assert!(
                card["rows"]
                    .as_array()
                    .expect("rows")
                    .iter()
                    .all(|row| row["label"].as_str() != Some("Связь с лимитом клиента"))
            );
        }
        assert!(
            cards[0]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("текущая шкала клиента")
        );
    }

    #[test]
    fn hero_cards_alert_when_continuity_startup_burn_dominates_live_window() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 0,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 0,
                        "total_context_tokens": 0,
                        "effective_savings_pct": 0.0,
                        "total_effective_saved_tokens": 0,
                        "total_recovery_tokens": 0,
                        "observed_continuity_restore_tokens": 817
                    },
                    "rolling_window": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "observed_continuity_restore_tokens": 817
                    },
                    "lifetime": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_without_amai_measured_tokens": 4,
                            "verified_with_amai_measured_tokens": 4,
                            "verified_measured_saved_tokens": 0,
                            "verified_measured_saved_pct": 0.0,
                            "client_limit_meter_alignment": {
                                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 1,
                                "non_live_events_count": 0,
                                "blocking_reasons": ["same_meter_baseline_explicit_boundary"],
                                "strict_client_meter_slice": {
                                    "same_meter_equivalent_for_slice": true,
                                    "lower_bound_tokens": 4,
                                    "components": ["client_prompt"]
                                },
                                "explicit_boundary_surface": {
                                    "state": "amai_continuity_boundary",
                                    "components": ["continuity_restore_outside_retrieval"],
                                    "note": "Continuity boundary."
                                },
                                "continuity_boundary_rollup": {
                                    "state": "amai_continuity_boundary_observed",
                                    "observed_tokens": 817
                                },
                                "baseline_equivalence": {
                                    "state": "baseline_component_semantics_explicit_boundary",
                                    "measured_baseline_components": ["client_prompt"],
                                    "explicitly_unmodeled_baseline_components": ["continuity_restore_outside_retrieval"]
                                }
                            }
                        },
                        "rolling_window": {
                            "verified_without_amai_measured_tokens": 4,
                            "verified_with_amai_measured_tokens": 4,
                            "verified_measured_saved_tokens": 0,
                            "verified_measured_saved_pct": 0.0,
                            "client_limit_meter_alignment": {
                                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 1,
                                "non_live_events_count": 0,
                                "blocking_reasons": ["same_meter_baseline_explicit_boundary"],
                                "strict_client_meter_slice": {
                                    "same_meter_equivalent_for_slice": true,
                                    "lower_bound_tokens": 4,
                                    "components": ["client_prompt"]
                                },
                                "explicit_boundary_surface": {
                                    "state": "amai_continuity_boundary",
                                    "components": ["continuity_restore_outside_retrieval"],
                                    "note": "Continuity boundary."
                                },
                                "continuity_boundary_rollup": {
                                    "state": "amai_continuity_boundary_observed",
                                    "observed_tokens": 817
                                },
                                "baseline_equivalence": {
                                    "state": "baseline_component_semantics_explicit_boundary",
                                    "measured_baseline_components": ["client_prompt"],
                                    "explicitly_unmodeled_baseline_components": ["continuity_restore_outside_retrieval"]
                                }
                            }
                        },
                        "lifetime": {
                            "verified_without_amai_measured_tokens": 8,
                            "verified_with_amai_measured_tokens": 8,
                            "verified_measured_saved_tokens": 0,
                            "verified_measured_saved_pct": 0.0,
                            "client_limit_meter_alignment": {
                                "alignment_state": "partial_lower_bound_not_meter_equivalent",
                                "same_meter_as_client_limit": false,
                                "live_events_count": 1,
                                "non_live_events_count": 0,
                                "blocking_reasons": ["client_prompt_unmeasured"]
                            }
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("alert"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("расход ушёл в подготовку контекста")
        );
        assert!(
            cards[0]["note"].as_str().unwrap_or_default().contains(
                "Короткая карточка показывает только проверяемые цифры по текущей сессии"
            )
        );
        let model_row = cards[0]["rows"]
            .as_array()
            .expect("session rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Экономия на учтённой части"))
            .expect("model-token row");
        assert!(
            model_row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("Точного процента пока нет")
        );
    }

    #[test]
    fn exact_model_component_delta_row_surfaces_top_same_meter_driver() {
        let alignment = json!({
            "baseline_equivalence": {
                "component_semantics": [
                    {
                        "code": "client_prompt",
                        "baseline_measured_tokens": 48,
                        "observed_tokens": 48,
                        "whole_cycle_observed_complete": true
                    },
                    {
                        "code": "continuity_restore_outside_retrieval",
                        "baseline_measured_tokens": 8228,
                        "observed_tokens": 8456,
                        "whole_cycle_observed_complete": true
                    }
                ]
            }
        });

        let row = exact_model_component_delta_metric_row(&alignment).expect("row");
        assert_eq!(row["label"].as_str(), Some("Главный драйвер exact-пары"));
        assert_eq!(
            row["value"].as_str(),
            Some("continuity-restore overhead вне retrieval: 8228 -> 8456 (+228 к расходу)")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("исходный запрос клиента: 48 -> 48 (без разницы)")
        );
    }

    #[test]
    fn rolling_window_card_surfaces_historical_startup_drag_when_current_session_is_profitable() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 265,
                        "verified_effective_savings_pct": 60.91954022988506,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "excluded_events_count": 0,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 0,
                        "total_context_tokens": 0,
                        "effective_savings_pct": 0.0,
                        "total_effective_saved_tokens": 0,
                        "total_recovery_tokens": 0,
                        "observed_continuity_restore_tokens": 170
                    },
                    "rolling_window": {
                        "events_total": 15,
                        "counted_events": 15,
                        "verified_effective_saved_tokens": -181,
                        "verified_effective_savings_pct": -1.8986677855869087,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0,
                        "observed_continuity_restore_tokens": 9714
                    },
                    "lifetime": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 0,
                        "verified_effective_savings_pct": 0.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_without_amai_measured_tokens": 435,
                            "verified_with_amai_measured_tokens": 0,
                            "verified_observed_whole_cycle_with_amai_tokens": 174,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "continuity_boundary_rollup": {
                                    "observed_tokens": 0
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 439
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 439,
                                    "component_semantics": [
                                        {
                                            "code": "client_prompt",
                                            "baseline_measured_tokens": 4,
                                            "observed_tokens": 4,
                                            "whole_cycle_observed_complete": true
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 435,
                                            "observed_tokens": 170,
                                            "whole_cycle_observed_complete": true
                                        }
                                    ]
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        },
                        "rolling_window": {
                            "verified_without_amai_measured_tokens": 9533,
                            "verified_with_amai_measured_tokens": 0,
                            "verified_observed_whole_cycle_with_amai_tokens": 9774,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "continuity_boundary_rollup": {
                                    "observed_tokens": 0
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 9593
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 9593,
                                    "component_semantics": [
                                        {
                                            "code": "client_prompt",
                                            "baseline_measured_tokens": 60,
                                            "observed_tokens": 60,
                                            "whole_cycle_observed_complete": true
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 9533,
                                            "observed_tokens": 9714,
                                            "whole_cycle_observed_complete": true
                                        }
                                    ]
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        },
                        "lifetime": {
                            "verified_without_amai_measured_tokens": 8,
                            "verified_with_amai_measured_tokens": 8,
                            "verified_observed_whole_cycle_with_amai_tokens": 8,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "continuity_boundary_rollup": {
                                    "observed_tokens": 0
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 8
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 8
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        }
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина",
                        "rolling_window_hours": 24
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[1]["status"].as_str(), Some("alert"));
        assert_eq!(
            cards[1]["status_label"].as_str(),
            Some("тянется хвост прошлых стартов")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая карточка только с проверяемыми цифрами по рабочему окну.")
        );
        let row = cards[1]["rows"]
            .as_array()
            .expect("rolling rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Хвост от прошлых стартов"))
            .expect("historical startup drag row");
        assert_eq!(
            row["value"].as_str(),
            Some("вне текущей сессии: без Amai 9154, с Amai 9600, +446 к расходу")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("тянется от прошлых запусков")
        );
    }

    #[test]
    fn rolling_window_card_uses_dynamic_window_copy() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 1,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 10,
                        "verified_effective_savings_pct": 10.0,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "rolling_window": {
                        "events_total": 2,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 20,
                        "verified_effective_savings_pct": 20.0,
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 20,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "lifetime": {
                        "events_total": 2,
                        "counted_events": 1,
                        "verified_effective_saved_tokens": 20,
                        "verified_effective_savings_pct": 20.0,
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 20,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 0.0,
                        "answer_like_counted_events": 0,
                        "verified_answer_like_savings_pct": 0.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {}
                        },
                        "rolling_window": {
                            "client_limit_meter_alignment": {}
                        },
                        "lifetime": {
                            "client_limit_meter_alignment": {}
                        }
                    },
                    "statement_export_previews": {
                        "lifetime": {}
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина",
                        "rolling_window_hours": 24
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert!(
            !cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("duration-shaped profile")
        );
        assert!(
            cards[1]["title_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("только проверяемые цифры по рабочему окну")
        );
        assert!(
            cards[1]["source_label"]
                .as_str()
                .unwrap_or_default()
                .contains("окна")
        );
    }

    #[test]
    fn compact_token_hero_card_sanitizes_active_agent_budget_grouped_card() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "presentation_variant": "active_agent_budget_grouped_v3",
            "status": "pass",
            "status_label": "",
            "rows": [
                {"label": "Агент:", "value": "Amai", "tooltip": "amai::continuity::default\n- Dashboard\n- /home/art/agent-memory-index"},
                {"label": "Экономия:", "value": "Amai savings: без Amai 1000, с Amai 400, экономия 600 (60.00%)", "tooltip": "Личный burn-guard active thread"},
                {"label": "Последний запрос:", "value": "222596 из 258400 · окно занято 86.14%", "tooltip": "giant-thread pressure"}
            ],
            "agent_blocks": [
                {
                    "agent_label": "Amai",
                    "agent_tooltip": "amai::continuity::default\n- Dashboard\n- /home/art/agent-memory-index",
                    "limit_value": "основное окно остаётся 43.00%, расширенное окно остаётся 72.00%",
                    "kpi_value": "Amai savings: без Amai 1000, с Amai 400, экономия 600 (60.00%)",
                    "kpi_tooltip": "Личный burn-guard текущего active thread идёт в экономии 60.00%.",
                    "pressure_value": "222596 из 258400 · окно занято 86.14%",
                    "pressure_tooltip": "giant-thread pressure"
                }
            ]
        });
        let compact = compact_token_hero_card(card);
        let rows = compact["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["label"].as_str(), Some("Агент:"));
        assert!(rows[0]["tooltip"].is_null());
        assert_eq!(rows[1]["label"].as_str(), Some("Экономия:"));
        assert!(
            rows[1]["tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("честная пара без Amai / с Amai"))
        );
        let blocks = compact["agent_blocks"].as_array().expect("agent blocks");
        assert!(blocks[0]["agent_tooltip"].is_null());
        assert!(blocks[0].get("agent_scope").is_none());
        assert_eq!(blocks[0]["kpi_label"].as_str(), Some("Экономия:"));
        assert!(
            blocks[0]["kpi_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("честная пара без Amai / с Amai"))
        );
        assert!(blocks[0].get("pressure_value").is_none());
        assert!(blocks[0].get("pressure_tooltip").is_none());
        assert_eq!(compact["note"].as_str(), Some(""));
        assert!(
            compact["source_label"]
                .as_str()
                .is_some_and(|value| value.contains("текущая шкала клиента"))
        );
    }

    #[test]
    fn compact_token_hero_card_rewrites_legacy_thread_limit_row_and_drops_target_selector() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "presentation_variant": "active_agent_budget_grouped_v3",
            "status": "pass",
            "status_label": "",
            "rows": [
                {
                    "label": "Личный thread-limit агента:",
                    "value": "н/д",
                    "tooltip": "internal thread-limit detail",
                    "target_selector": {
                        "kpi_value_text": "Burn guard 75.00%"
                    }
                }
            ],
            "agent_blocks": []
        });
        let compact = compact_token_hero_card(card);
        let rows = compact["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["label"].as_str(), Some("Лимит этой работы сейчас:"));
        assert!(
            rows[0]["tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("точный лимит"))
        );
        assert!(rows[0].get("target_selector").is_none());
    }

    #[test]
    fn compact_token_hero_card_keeps_truth_only_rows_for_current_session() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "status": "critical",
            "status_label": "новый чат нужен сейчас",
            "note": "long note",
            "rows": [
                {"label": "Главный итог", "value": "x"},
                {"label": "Amai в полном live-turn", "value": "0.30%: без Amai 1000, с Amai 997, delta 3"},
                {"label": "Экономия токенов модели", "value": "y"},
                {"label": "Главный драйвер exact-пары", "value": "continuity-restore overhead вне retrieval: 636 -> 95 (экономия 541)"},
                {"label": "Совпадение с реальным лимитом", "value": "z"},
                {"label": "Токены continuity boundary", "value": "continuity-restore: 541 токен"},
                {"label": "Лимит клиента сейчас", "value": "l"},
                {"label": "Следующее действие", "value": "n"},
                {"label": "Строгий same-meter срез", "value": "drop"}
            ]
        });
        let compact = compact_token_hero_card(card);
        let labels = compact["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "Экономия на реальной шкале",
                "Экономия на учтённой части",
                "Что именно посчитано",
                "Точность учтённой части",
                "Подготовка контекста",
                "Лимит клиента сейчас",
            ]
        );
        assert_eq!(compact["status"].as_str(), Some("pass"));
        assert_eq!(
            compact["status_label"].as_str(),
            Some("экономия на полной шкале подтверждена")
        );
        assert_eq!(
            compact["source_label"].as_str(),
            Some(
                "Источник: текущая шкала клиента и отдельно подтверждённая часть уже проверенных запросов."
            )
        );
        assert!(
            compact["note"].as_str().unwrap_or_default().contains(
                "Короткая карточка показывает только проверяемые цифры по текущей сессии"
            )
        );
        assert!(
            compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("ниже уже проверенная часть")
        );
        assert!(
            compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Подсказки управления вынесены отдельно")
        );
        assert!(
            !compact["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Operator burn-guard")
        );
        assert!(
            !compact["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("target")
        );
        assert!(
            compact["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .all(|row| row.get("target_selector").is_none())
        );
    }

    #[test]
    fn compact_token_hero_card_keeps_truth_only_rows_for_lifetime() {
        let card = json!({
            "title": "Экономия токенов за всё время записи",
            "status": "alert",
            "status_label": "есть старый долг точности",
            "note": "long note",
            "rows": [
                {"label": "Экономия токенов модели", "value": "a"},
                {"label": "Совпадение с реальным лимитом", "value": "b"},
                {"label": "Review-only export", "value": "c"},
                {"label": "Связь с лимитом клиента", "value": "drop"},
                {"label": "Исторический frozen debt", "value": "d"}
            ]
        });
        let compact = compact_token_hero_card(card);
        let labels = compact["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "Экономия на учтённой части",
                "Точность учтённой части",
                "Отчёт для ручной сверки",
                "Старый исторический хвост"
            ]
        );
        assert_eq!(
            compact["source_label"].as_str(),
            Some(
                "Источник: подтверждённая история плюс отдельно отмеченный старый исторический хвост. Это не весь расход за всё время."
            )
        );
    }

    #[test]
    fn compact_token_hero_card_rewrites_target_shortfall_to_public_truth_status() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "value": "75.00%",
            "status": "alert",
            "status_label": "цель 90% не достигнута",
            "status_tooltip": "operator-only target alert",
            "rows": [
                {"label": "Amai в полном live-turn", "value": "75.00%: без Amai 1000, с Amai 250, delta 750"},
                {"label": "Экономия токенов модели", "value": "Учтённый same-meter срез: 75.00%"},
                {"label": "Совпадение с реальным лимитом", "value": "цифра точная: полностью совпадает со шкалой лимита модели"}
            ]
        });
        let compact = compact_token_hero_card(card);
        assert_eq!(compact["status"].as_str(), Some("pass"));
        assert_eq!(
            compact["status_label"].as_str(),
            Some("экономия на полной шкале подтверждена")
        );
        assert!(
            !compact["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("цель 90%")
        );
    }

    #[test]
    fn compact_token_hero_card_rewrites_same_thread_rotate_label_to_public_waiting_status() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "value": "не доказано",
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "status_tooltip": "operator rotate guidance",
            "rows": [
                {"label": "Экономия токенов модели", "value": "Предварительный учтённый same-meter срез: 12.00%"},
                {"label": "Совпадение с реальным лимитом", "value": "цифра пока предварительная: current same-meter pair ещё не собрана"}
            ]
        });
        let compact = compact_token_hero_card(card);
        assert_eq!(compact["status"].as_str(), Some("waiting"));
        assert_eq!(
            compact["status_label"].as_str(),
            Some("реальная экономия ещё не доказана")
        );
        assert!(
            !compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("сожми текущий чат")
        );
    }

    #[test]
    fn compact_token_hero_card_keeps_continuity_boundary_row_for_public_surface() {
        let card = json!({
            "title": "Экономия токенов за рабочее окно",
            "value": "-12.00%",
            "status": "alert",
            "status_label": "burn в continuity startup",
            "rows": [
                {"label": "Экономия токенов модели", "value": "a"},
                {"label": "Токены continuity boundary", "value": "continuity-restore: 817 токенов"},
                {"label": "Следующее действие", "value": "rotate"},
                {"label": "Исторический startup-хвост", "value": "b"}
            ]
        });
        let compact = compact_token_hero_card(card);
        let labels = compact["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|row| row["label"].as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"Подготовка контекста"));
        assert!(!labels.contains(&"Следующее действие"));
        assert_eq!(
            compact["status_label"].as_str(),
            Some("окно уходит в подготовку контекста")
        );
    }

    #[test]
    fn historical_frozen_debt_metric_row_surfaces_historical_only_tail() {
        let current_session_alignment = json!({
            "exact_pair_status": {
                "exact_pair_available": true
            }
        });
        let rolling_window_alignment = json!({
            "exact_pair_status": {
                "exact_pair_available": true
            }
        });
        let lifetime_alignment = json!({
            "frozen_gap_review_surface": {
                "state": "review_required",
                "blocking_component": "tool_overhead_outside_retrieval",
                "irrecoverable_missing_live_events": 13
            }
        });

        let row = historical_frozen_debt_metric_row(
            &current_session_alignment,
            &rolling_window_alignment,
            &lifetime_alignment,
        )
        .expect("historical tail row");
        assert_eq!(row["label"], "Исторический frozen debt");
        assert_eq!(
            row["value"].as_str(),
            Some("tool_overhead_outside_retrieval: historical-only, 13 rows")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Текущая сессия: точная пара уже собрана")
        );
    }

    #[test]
    fn reviewed_frozen_debt_export_metric_row_surfaces_report_only_path() {
        let alignment = json!({
            "reviewed_frozen_debt_export_surface": {
                "export_ready_report_only": true,
                "surface_kind": "reviewed_frozen_debt_report_only",
                "blocking_component": "tool_overhead_outside_retrieval",
                "irrecoverable_missing_live_events": 13,
                "allowed_claims": [
                    "reviewed_frozen_debt_report_only",
                    "historical_source_loss_disclosed_non_exact"
                ],
                "forbidden_claims": [
                    "claim_raw_exact_history",
                    "claim_exact_same_meter_pair_materialized"
                ],
                "propagated_surfaces": [
                    "statement_export_preview",
                    "settlement_report_preview",
                    "contractual_evidence_pack"
                ],
                "review_bundle_command": "./scripts/amai_exec.sh observe token-statement-export --scope lifetime",
                "evidence_pack_command": "./scripts/amai_exec.sh observe token-evidence-pack --scope lifetime"
            }
        });

        let row = reviewed_frozen_debt_export_metric_row(&alignment).expect("export row");
        assert_eq!(row["label"], "Review-only export");
        assert_eq!(
            row["value"].as_str(),
            Some("reviewed_frozen_debt_report_only: 13 irrecoverable rows")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Что этим отчётом подтверждать нельзя")
        );
        assert!(
            row["tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("observe token-statement-export --scope lifetime")
        );
    }
}
