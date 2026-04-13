use super::*;

pub(super) fn token_budget_report_root<'a>(snapshot: &'a Value) -> &'a Value {
    if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    }
}

pub(super) fn live_response_latency_root<'a>(snapshot: &'a Value) -> Option<&'a Value> {
    let root = token_budget_report_root(snapshot);
    if root["live_response_latency"].is_object() {
        Some(&root["live_response_latency"])
    } else if root["current_session"].is_object()
        || root["rolling_window"].is_object()
        || root["current_session_relation"].is_object()
        || root["current_session_exclusions"].is_object()
        || root["current_thread_live_file_hints"].is_object()
    {
        Some(root)
    } else {
        None
    }
}

pub(super) fn live_response_latency_current_thread_file_hints(snapshot: &Value) -> Vec<String> {
    live_response_latency_root(snapshot)
        .and_then(|root| root["current_thread_live_file_hints"]["hints"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item["label"].as_str().map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}
