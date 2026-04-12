use super::*;

pub(super) const ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS: i64 = 5 * 60 * 1000;
pub(super) const ACTIVE_AGENT_SECONDARY_LIMIT_WINDOW_HOURS: i64 = 24 * 7;

pub(super) fn active_agent_limit_percent_text(percent: f64) -> String {
    format!("{:.2}%", percent.clamp(0.0, 100.0))
}

pub(super) fn active_agent_personal_kpi_window(
    events: &[TokenBudgetEvent],
    selector: &PersonalKpiSelector,
    now_epoch_ms: i64,
) -> (Vec<TokenBudgetEvent>, PersonalKpiSelector, bool) {
    let strict_events = personal_kpi_window_events(events, Some(selector), now_epoch_ms);
    if selector.thread_id.is_none() || !strict_events.is_empty() {
        return (strict_events, selector.clone(), false);
    }

    let fallback_selector = PersonalKpiSelector {
        thread_id: None,
        ..selector.clone()
    };
    let fallback_events =
        personal_kpi_window_events(events, Some(&fallback_selector), now_epoch_ms);
    if fallback_events.is_empty() {
        (strict_events, selector.clone(), false)
    } else {
        (fallback_events, fallback_selector, true)
    }
}
