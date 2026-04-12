use super::*;

pub(super) fn default_agent_scope_label(project_code: &str, namespace_code: &str) -> String {
    let project_code = project_code.trim();
    let namespace_code = namespace_code.trim();
    if project_code.is_empty() || namespace_code.is_empty() {
        "shared".to_string()
    } else {
        format!("{project_code}::{namespace_code}::default")
    }
}

pub(super) fn normalize_token_event_agent_scope(
    raw_agent_scope: Option<&str>,
    project_code: &str,
    namespace_code: &str,
) -> String {
    raw_agent_scope
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_agent_scope_label(project_code, namespace_code))
}

fn event_matches_personal_kpi_selector(
    event: &TokenBudgetEvent,
    selector: &PersonalKpiSelector,
) -> bool {
    if event.project != selector.project_code || event.namespace != selector.namespace_code {
        return false;
    }
    if let Some(thread_id) = selector.thread_id.as_deref() {
        return event.thread_id.as_deref() == Some(thread_id);
    }
    event.agent_scope == selector.agent_scope
}

pub(super) fn filter_events_for_personal_kpi_selector(
    events: &[TokenBudgetEvent],
    selector: &PersonalKpiSelector,
) -> Vec<TokenBudgetEvent> {
    events
        .iter()
        .filter(|event| event_matches_personal_kpi_selector(event, selector))
        .cloned()
        .collect()
}

pub(super) fn personal_kpi_window_events(
    events: &[TokenBudgetEvent],
    selector: Option<&PersonalKpiSelector>,
    now_epoch_ms: i64,
) -> Vec<TokenBudgetEvent> {
    selector
        .map(|selector| {
            let scoped = filter_events_for_personal_kpi_selector(events, selector);
            rolling_window_events_for_duration(
                &scoped,
                now_epoch_ms,
                PERSONAL_AGENT_KPI_WINDOW_HOURS,
            )
        })
        .unwrap_or_default()
}

pub(super) fn rolling_window_events_for_duration(
    events: &[TokenBudgetEvent],
    now_epoch_ms: i64,
    hours: i64,
) -> Vec<TokenBudgetEvent> {
    let lower_bound = now_epoch_ms.saturating_sub(hours.saturating_mul(3_600_000));
    events
        .iter()
        .filter(|event| event.created_at_epoch_ms >= lower_bound)
        .cloned()
        .collect()
}
