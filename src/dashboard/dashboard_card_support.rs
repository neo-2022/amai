use super::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub fn monitoring_url(base_url: &str, port: &str) -> String {
    let (scheme, host) = parse_base_url_host(base_url);
    format!("{scheme}://{host}:{port}")
}

pub(super) fn humanize_identifier(value: &str) -> String {
    value
        .split(['_', '-', '/', ':'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let head = first.to_uppercase().collect::<String>();
                    let tail = chars.as_str().to_lowercase();
                    format!("{head}{tail}")
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_base_url_host(base_url: &str) -> (&str, &str) {
    let (scheme, rest) = base_url.split_once("://").unwrap_or(("http", base_url));
    let host = rest
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(rest)
        .trim_end_matches('/');
    (scheme, host)
}

pub(super) fn tcp_port_is_open(host: &str, port: &str) -> bool {
    let Ok(addrs) = format!("{host}:{port}").to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok())
}

pub(super) fn card(title: &str, value: String, note: String, status: &str) -> Value {
    card_with_rows(title, value, note, status, None, None, Vec::new())
}

pub(super) fn compact_dashboard_text(
    value: Option<&str>,
    max_chars: usize,
    fallback: &str,
) -> String {
    let text = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}

pub(super) fn card_with_rows(
    title: &str,
    value: String,
    note: String,
    status: &str,
    source_label: Option<String>,
    title_tooltip: Option<String>,
    rows: Vec<Value>,
) -> Value {
    json!({
        "title": title,
        "value": value,
        "note": note,
        "status": status,
        "status_label": status_label(status),
        "status_tooltip": Value::Null,
        "source_label": source_label,
        "title_tooltip": title_tooltip,
        "rows": rows,
    })
}

pub(super) fn metric_row(label: &str, value: String, tooltip: Option<&str>) -> Value {
    json!({
        "label": label,
        "value": value,
        "tooltip": tooltip,
    })
}

pub(super) fn metric_row_with_key(
    key: &str,
    label: &str,
    value: String,
    tooltip: Option<&str>,
) -> Value {
    let mut row = metric_row(label, value, tooltip);
    if let Some(root) = row.as_object_mut() {
        root.insert("key".to_string(), Value::from(key));
    }
    row
}

pub(super) fn status_reason_tooltip(
    status: &str,
    reasons: Vec<String>,
    fallback: &str,
) -> Option<String> {
    if status == "pass" {
        return None;
    }
    let intro = match status {
        "critical" => "Статус стал критичным по следующим причинам:",
        "alert" => "Статус требует внимания по следующим причинам:",
        "waiting" => "Статус пока не может считаться нормальным по следующим причинам:",
        _ => "Статус пока не может считаться нормальным по следующим причинам:",
    };
    if reasons.is_empty() {
        Some(format!("{intro}\n- {fallback}"))
    } else {
        Some(format!("{intro}\n- {}", reasons.join("\n- ")))
    }
}

pub(super) fn status_label(status: &str) -> &'static str {
    match status {
        "pass" => "в норме",
        "alert" => "внимание",
        "critical" => "критично",
        "waiting" => "ждём подтверждённую выборку",
        _ => "нет данных",
    }
}

pub(super) fn with_extra_class(mut card: Value, extra_class: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert("extra_class".to_string(), Value::from(extra_class));
    }
    card
}

pub(super) fn with_table_orientation(mut card: Value, table_orientation: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert(
            "table_orientation".to_string(),
            Value::from(table_orientation),
        );
    }
    card
}

pub(super) fn with_status_tooltip(mut card: Value, status_tooltip: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert(
            "status_tooltip".to_string(),
            Value::from(status_tooltip.to_string()),
        );
    }
    card
}

pub(super) fn with_status(mut card: Value, status: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert("status".to_string(), Value::from(status.to_string()));
    }
    card
}

pub(super) fn with_status_label(mut card: Value, status_label: &str) -> Value {
    if let Some(object) = card.as_object_mut() {
        object.insert(
            "status_label".to_string(),
            Value::from(status_label.to_string()),
        );
    }
    card
}
