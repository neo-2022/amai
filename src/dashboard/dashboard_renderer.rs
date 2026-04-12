use super::*;

const TEMPLATE: &str = include_str!("dashboard_template.html");

fn inline_bootstrap_json(payload: Option<&Value>) -> String {
    payload
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string())
        .replace("</", "<\\/")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub fn render_html(refresh_ms: u64, bootstrap_payload: Option<&Value>) -> String {
    TEMPLATE
        .replace("__REFRESH_MS__", &refresh_ms.to_string())
        .replace(
            "__BOOTSTRAP_PAYLOAD__",
            &inline_bootstrap_json(bootstrap_payload),
        )
        .replace("__ASSET_VERSION__", env!("CARGO_PKG_VERSION"))
}
