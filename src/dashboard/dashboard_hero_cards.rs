use super::*;

fn active_agent_budget_card_status(surface: &Value) -> (&'static str, &'static str, String) {
    let aggregate = &surface["aggregate"];
    let status = aggregate["status"].as_str().unwrap_or("missing");
    let classification = aggregate["classification"].as_str().unwrap_or("missing");
    match (status, classification) {
        ("observed", "overspend") => (
            "alert",
            "активные агенты жгут лимит",
            "Среднее по активным агентам сейчас в переплате, поэтому карточка требует внимания."
                .to_string(),
        ),
        ("observed", _) => (
            "pass",
            "только активные агенты",
            "Карточка показывает только личный 5ч KPI и текущий лимит клиента для реально активных агентов."
                .to_string(),
        ),
        ("partial", _) => (
            "waiting",
            "не у всех KPI materialized",
            "Не у каждого активного агента уже есть measured личный 5ч KPI, поэтому среднее fail-closed не посчитано."
                .to_string(),
        ),
        _ => (
            "waiting",
            "активных агентов сейчас нет",
            "Сейчас нет active lease, поэтому карточка не показывает персональные KPI."
                .to_string(),
        ),
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
        .unwrap_or("5ч KPI: н/д")
        .to_string();
    let mut agent_blocks = Vec::new();
    for agent in agents {
        let agent_label =
            compact_dashboard_text(agent["agent_label"].as_str(), 72, "Активный агент");
        let kpi_prefix = agent["personal_agent_kpi"]["reply_prefix"]
            .as_str()
            .unwrap_or("5ч KPI: н/д");
        let agent_tooltip = agent["thread_title"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(|thread_title| {
                format!(
                    "{}\n- {}\n- {}",
                    agent["agent_scope"]
                        .as_str()
                        .unwrap_or("scope ещё нет данных"),
                    compact_dashboard_text(Some(thread_title), 88, thread_title),
                    agent["cwd"].as_str().unwrap_or("cwd ещё нет данных"),
                )
            })
            .unwrap_or_else(|| {
                agent["agent_scope"]
                    .as_str()
                    .unwrap_or("scope ещё нет данных")
                    .to_string()
            });
        let limit_label = active_agent_online_limit_label(agent);
        let (limit_value, limit_tooltip) = active_agent_online_limit_field(agent);
        let (pressure_value, pressure_tooltip) = active_agent_live_pressure_field(agent)
            .map(|(value, tooltip)| (Some(value), Some(tooltip)))
            .unwrap_or((None, None));
        let kpi_tooltip = agent["personal_agent_kpi"]["summary"]
            .as_str()
            .map(str::to_string);
        agent_blocks.push(json!({
            "agent_scope": agent["agent_scope"].clone(),
            "agent_label": agent_label,
            "agent_tooltip": agent_tooltip,
            "limit_label": limit_label,
            "limit_value": limit_value,
            "limit_tooltip": limit_tooltip,
            "pressure_label": pressure_value
                .as_ref()
                .map(|_| "Последний запрос:"),
            "pressure_value": pressure_value,
            "pressure_tooltip": pressure_tooltip,
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
            block["agent_tooltip"].as_str(),
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
            "KPI:",
            block["kpi_value"]
                .as_str()
                .unwrap_or("5ч KPI: н/д")
                .to_string(),
            block["kpi_tooltip"].as_str(),
        ));
        if let Some(pressure_value) = block["pressure_value"].as_str() {
            legacy_rows.push(metric_row(
                block["pressure_label"]
                    .as_str()
                    .unwrap_or("Последний запрос:"),
                pressure_value.to_string(),
                block["pressure_tooltip"].as_str(),
            ));
        }
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

fn active_agent_online_limit_field(agent: &Value) -> (String, Option<String>) {
    let value = agent["personal_client_limit"]["value_text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("н/д")
        .to_string();
    let tooltip = agent["personal_client_limit"]["tooltip"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            Some("Личный online limit surface для этого агента ещё не materialized.".to_string())
        });
    (value, tooltip)
}

fn active_agent_online_limit_label(agent: &Value) -> &str {
    agent["personal_client_limit"]["label_text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("Лимит клиента сейчас:")
}

fn active_agent_live_pressure_field(agent: &Value) -> Option<(String, String)> {
    let client_live_meter = &agent["client_live_meter"];
    if !current_session_client_live_meter_available(client_live_meter) {
        return None;
    }
    let turn_total_tokens = client_live_meter["client_turn_total_tokens"]
        .as_u64()
        .filter(|value| *value > 0)?;
    let model_context_window = client_live_meter["latest_model_context_window"]
        .as_u64()
        .filter(|value| *value > 0)?;
    let context_used_percent = client_live_meter["context_used_percent"]
        .as_f64()
        .unwrap_or_else(|| (turn_total_tokens as f64 * 100.0) / model_context_window as f64);
    let observed_at = client_live_meter["ended_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp);
    let observed_at_short = client_live_meter["ended_at_epoch_ms"]
        .as_u64()
        .filter(|value| *value > 0)
        .map(human_timestamp_clock);
    let pressure_note = if context_used_percent >= 70.0 {
        "Это giant-thread pressure: почти всё окно клиента уже занято одним live-turn, поэтому 5ч burn сейчас идёт главным образом размером самого запроса."
    } else if context_used_percent >= 50.0 {
        "Это тяжёлый live-turn: заметная часть burn сейчас приходит от размера текущего клиентского запроса, а не только от Amai-side delta."
    } else {
        "Это текущий observed client turn этого агента. Он помогает отличить реальный burn от UI/агрегационного drift."
    };
    let tooltip = format!(
        "Этот ряд показывает последний observed client turn именно этого active agent из rollout token_count.\n- Последний запрос: {} из {}\n- Окно занято: {}\n- Источник: rollout token_count.last_token_usage.total_tokens / model_context_window{}\n- {}\n- Снято из raw token_count: {}",
        format_u64(Some(turn_total_tokens)),
        format_u64(Some(model_context_window)),
        format_percent(Some(context_used_percent)),
        observed_at_short
            .as_ref()
            .map(|stamp| format!(" ({stamp})"))
            .unwrap_or_default(),
        pressure_note,
        observed_at.unwrap_or_else(|| "ещё нет данных".to_string()),
    );
    Some((
        format!(
            "{} из {} · окно занято {}",
            format_u64(Some(turn_total_tokens)),
            format_u64(Some(model_context_window)),
            format_percent(Some(context_used_percent)),
        ),
        tooltip,
    ))
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

pub(crate) fn compact_token_hero_card(mut card: Value) -> Value {
    if matches!(
        card["presentation_variant"].as_str(),
        Some(
            "active_agent_budget_v1"
                | "active_agent_budget_minimal_v2"
                | "active_agent_budget_grouped_v3"
        )
    ) {
        return card;
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
            if let Some(label) = row["label"].as_str() {
                row["label"] =
                    Value::String(humanize_token_card_row_label(&title, label).to_string());
            }
            if let (Some(label), Some(value)) = (row["label"].as_str(), row["value"].as_str()) {
                row["value"] =
                    Value::String(humanize_token_card_row_value(&title, label, value).to_string());
            }
        }
    }
    if let Some(source_label) = truth_only_token_card_source_label(&card) {
        card["source_label"] = Value::String(source_label);
    }
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
            "Последний запрос клиента",
            "Лимит клиента сейчас",
            "Последний observed лимит клиента",
            "Следующее действие",
        ],
        "Экономия токенов за рабочее окно" => &[
            "Экономия токенов модели",
            "Совпадение с реальным лимитом",
            "Исторический startup-хвост",
            "Следующее действие",
        ],
        "Экономия токенов за всё время записи" => &[
            "Экономия токенов модели",
            "Совпадение с реальным лимитом",
            "Исторический frozen debt",
            "Review-only export",
        ],
        _ => &[],
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
        (_, "Последний запрос клиента") => "Последний запрос в модель",
        (_, "Исторический startup-хвост") => "Хвост от прошлых стартов",
        (_, "Исторический frozen debt") => "Исторический долг точности",
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
        (_, "Исторический долг точности") => {
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

fn truth_only_token_card_title_tooltip(title: &str) -> Option<String> {
    let text = match title {
        "Экономия токенов за текущую сессию" => {
            "Показывает только проверяемые цифры по текущей сессии: реальную долю Amai на полной живой шкале turn, текущий лимит клиента и точность учтённой части."
        }
        "Экономия токенов за рабочее окно" => {
            "Показывает только проверяемые цифры по рабочему окну. Процент здесь относится к подтверждённой учтённой части, а не ко всему полному расходу модели за окно."
        }
        "Экономия токенов за всё время записи" => {
            "Показывает только подтверждённые цифры за всё время записи. Процент здесь относится к подтверждённой учтённой части, а старый исторический хвост вынесен отдельно."
        }
        _ => return None,
    };
    Some(text.to_string())
}

fn truth_only_token_card_source_label(card: &Value) -> Option<String> {
    let title = card["title"].as_str()?;
    let source = match title {
        "Экономия токенов за текущую сессию" => {
            "Источник: живая шкала клиента из rollout token_count и отдельно сведённая учтённая часть Amai по strict same-meter компонентам."
        }
        "Экономия токенов за рабочее окно" => {
            "Источник: подтверждённая учтённая часть окна и подтверждённый хвост прошлых стартов. Это не весь полный расход клиента за окно."
        }
        "Экономия токенов за всё время записи" => {
            "Источник: подтверждённая учтённая история плюс отдельно отмеченный старый долг точности. Это не полный raw spend всей истории."
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
                    "Короткая карточка только с проверяемыми цифрами по текущей сессии: реальная экономия на полной шкале клиента пока не доказана, ниже остаётся только точная учтённая часть. Единственный процент, который должен напрямую совпадать с замедлением шкалы VS Code, живёт в строке «Экономия на реальной шкале» и показывается только после exact full-turn pair. Строка «Экономия на учтённой части» относится только к strict same-meter срезу уже учтённых компонентов; если она помечена как preliminary, это ещё не вся сессия. Статус: {status_label}."
                ),
                _ => format!(
                    "Короткая карточка только с проверяемыми цифрами по текущей сессии: сверху реальная доля Amai на полной шкале текущего turn, ниже точность учтённой части. Единственный процент, который должен напрямую совпадать с замедлением шкалы VS Code, живёт в строке «Экономия на реальной шкале». Строка «Экономия на учтённой части» относится только к strict same-meter срезу уже учтённых компонентов; если она помечена как preliminary, это ещё не вся сессия. Статус: {status_label}."
                ),
            }
        }
        "Экономия токенов за рабочее окно" => {
            format!(
                "Короткая карточка только с проверяемыми цифрами по рабочему окну. Процент здесь относится к подтверждённой учтённой части, а не ко всему полному расходу модели за окно. Статус: {status_label}."
            )
        }
        "Экономия токенов за всё время записи" => {
            format!(
                "Короткая карточка только с подтверждёнными цифрами за всё время записи. Процент здесь относится к подтверждённой учтённой части, а старый долг точности вынесен отдельно. Статус: {status_label}."
            )
        }
        _ => card["note"].as_str().unwrap_or_default().to_string(),
    }
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
                    "reply_prefix": "5ч KPI: экономия 40.00%"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "thread_title": "compact dashboard rewrite",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 60.00%",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 43.00%, 7д остаётся 23.00%",
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
                            "reply_prefix": "5ч KPI: экономия 20.00%",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "label_text": "Личный thread-limit агента:",
                            "value_text": "5ч остаётся 88.00%, 7д остаётся 91.00%",
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
        assert_eq!(card["value"].as_str(), Some("5ч KPI: экономия 40.00%"));
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
        assert_eq!(rows[2]["label"].as_str(), Some("KPI:"));
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["agent_label"].as_str(), Some("Amai"));
        assert!(
            blocks[0]["agent_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("amai::continuity::default"))
        );
        assert!(
            blocks[0]["limit_value"]
                .as_str()
                .is_some_and(|value| value.contains("5ч остаётся 43.00%"))
        );
        assert!(
            blocks[0]["limit_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("personal limit one"))
        );
        assert_eq!(
            blocks[0]["limit_label"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert_eq!(
            blocks[0]["kpi_value"].as_str(),
            Some("5ч KPI: экономия 60.00%")
        );
        assert!(
            blocks[0]["kpi_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("agent one"))
        );
        assert_eq!(blocks[1]["agent_label"].as_str(), Some("Hunter"));
        assert!(
            blocks[1]["limit_value"]
                .as_str()
                .is_some_and(|value| value.contains("5ч остаётся 88.00%"))
        );
        assert_eq!(
            blocks[1]["limit_label"].as_str(),
            Some("Личный thread-limit агента:")
        );
        assert_eq!(
            blocks[1]["kpi_value"].as_str(),
            Some("5ч KPI: экономия 20.00%")
        );
    }

    #[test]
    fn build_active_agent_budget_session_card_collapses_shared_global_limit() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "overspend",
                    "reply_prefix": "5ч KPI: переплата 33.00%"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: переплата 44.00%",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 12.00%, 7д остаётся 89.00%",
                            "tooltip": "same exact global limit"
                        }
                    },
                    {
                        "agent_label": "Bug Bounty",
                        "agent_scope": "bug_bounty::continuity::default",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: переплата 22.00%",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 12.00%, 7д остаётся 89.00%",
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
            Some("5ч остаётся 12.00%, 7д остаётся 89.00%")
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
    fn build_active_agent_budget_session_card_surfaces_live_turn_pressure_per_agent() {
        let snapshot = json!({
            "active_agent_budget": {
                "aggregate": {
                    "status": "observed",
                    "classification": "overspend",
                    "reply_prefix": "5ч KPI: переплата 120.00%"
                },
                "agents": [
                    {
                        "agent_label": "Bug Bounty",
                        "agent_scope": "bug_bounty::continuity::default",
                        "thread_title": "Авито дальше",
                        "cwd": "/home/art/Bug-Bounty",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: переплата 197.79%",
                            "summary": "Личный 5ч KPI текущего active thread идёт в переплате 197.79%."
                        },
                        "personal_client_limit": {
                            "label_text": "Лимит клиента сейчас:",
                            "value_text": "5ч остаётся 16.00%, 7д остаётся 90.00%",
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
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3]["label"].as_str(), Some("Последний запрос:"));
        assert_eq!(
            rows[3]["value"].as_str(),
            Some("222596 из 258400 · окно занято 86.14%")
        );
        assert!(
            rows[3]["tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("giant-thread pressure"))
        );
        let blocks = card["agent_blocks"].as_array().expect("agent blocks");
        assert_eq!(
            blocks[0]["pressure_value"].as_str(),
            Some("222596 из 258400 · окно занято 86.14%")
        );
        assert!(
            blocks[0]["pressure_tooltip"]
                .as_str()
                .is_some_and(|value| value.contains("Окно занято: 86.14%"))
        );
    }

    #[test]
    fn compact_token_hero_card_leaves_active_agent_budget_minimal_card_unchanged() {
        let card = json!({
            "title": "Экономия токенов за текущую сессию",
            "presentation_variant": "active_agent_budget_grouped_v3",
            "status": "pass",
            "status_label": "",
            "rows": [],
            "agent_blocks": [
                {
                    "agent_label": "Amai",
                    "limit_value": "5ч остаётся 43.00%",
                    "kpi_value": "5ч KPI: экономия 60.00%"
                }
            ]
        });
        let compact = compact_token_hero_card(card.clone());
        assert_eq!(compact, card);
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
                "Лимит клиента сейчас",
                "Следующее действие"
            ]
        );
        assert_eq!(
            compact["source_label"].as_str(),
            Some(
                "Источник: живая шкала клиента из rollout token_count и отдельно сведённая учтённая часть Amai по strict same-meter компонентам."
            )
        );
        assert!(
            compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая карточка только с проверяемыми цифрами по текущей сессии")
        );
        assert!(
            compact["note"]
                .as_str()
                .unwrap_or_default()
                .contains("strict same-meter срезу")
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
                "Исторический долг точности"
            ]
        );
        assert_eq!(
            compact["source_label"].as_str(),
            Some(
                "Источник: подтверждённая учтённая история плюс отдельно отмеченный старый долг точности. Это не полный raw spend всей истории."
            )
        );
    }
}
