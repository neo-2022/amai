use super::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub fn monitoring_url(base_url: &str, port: &str) -> String {
    let (scheme, host) = parse_base_url_host(base_url);
    format!("{scheme}://{host}:{port}")
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
