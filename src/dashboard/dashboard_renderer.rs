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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_html_refresh_contract_is_live_on_focus_and_visibility() {
        let html = render_html(1000, None);
        assert!(html.contains("const TOOLTIP_HIDE_GRACE_MS = 220;"));
        assert!(html.contains("const DASHBOARD_BOOTSTRAP_PAYLOAD = null;"));
        assert!(html.contains("async function fetchWithTimeout(path, timeoutMs, init = {}) {"));
        assert!(html.contains(
            "renderDashboardPayload(chooseInitialDashboardPayload(DASHBOARD_BOOTSTRAP_PAYLOAD));"
        ));
        assert!(html.contains(
            "function chooseInitialDashboardPayload(bootstrapPayload) {\n      if (bootstrapPayload) {\n        return bootstrapPayload;\n      }\n      return null;\n    }"
        ));
        assert!(html.contains(
            "const DASHBOARD_PAYLOAD_CACHE_KEY = \"amai-human-dashboard-last-payload-v1\";"
        ));
        assert!(html.contains(
            "function scheduleHideTooltip(target = null, delayMs = TOOLTIP_HIDE_GRACE_MS) {"
        ));
        assert!(html.contains(
            "function isDocumentVisibleForRefresh() {\n      return document.visibilityState === \"visible\";\n    }"
        ));
        assert!(html.contains(
            "function scheduleForcedDashboardRefresh(reason = \"forced_refresh\", delayMs = 0) {"
        ));
        assert!(html.contains("document.addEventListener(\"visibilitychange\""));
        assert!(html.contains(
            "window.addEventListener(\"focus\", () => scheduleForcedDashboardRefresh(\"window_focus\"));"
        ));
        assert!(html.contains(
            "window.addEventListener(\"pageshow\", () => scheduleForcedDashboardRefresh(\"window_pageshow\"));"
        ));
        assert!(html.contains("const dashboardThreadId = new URLSearchParams(window.location.search).get(\"thread_id\");"));
        assert!(html.contains(
            "fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/client-budget-live\")"
        ));
        assert!(html.contains("scheduleForcedDashboardRefresh(\"initial_boot\");"));
        assert!(html.contains(
            "fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/dashboard-live-summary\")"
        ));
        assert!(html.contains("fetch(apiPathWithThreadHint(\"/api/client-budget-target\")"));
        assert!(html.contains("/api/client-budget-host-control-launch"));
        assert!(html.contains("/api/client-budget-host-control-feedback"));
        assert!(
            html.contains("fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/dashboard\")")
        );
        assert!(html.contains("id=\"dashboard-toast\""));
        assert!(html.contains("tooltipLayer.addEventListener(\"mouseenter\", () => {"));
        assert!(!html.contains(
            "setInterval(() => syncDashboardLiveSummary(false), DASHBOARD_LIVE_SUMMARY_REFRESH_MS);"
        ));
        assert!(html.contains(
            "setInterval(() => syncClientBudgetLiveRows(false), CLIENT_BUDGET_LIVE_REFRESH_MS);"
        ));
        assert!(!html.contains("setInterval(() => loadDashboard(false), REFRESH_MS);"));
        assert!(!html.contains("syncActiveAgentBudgetLiveCard(false)"));
        assert!(!html.contains("fetchActiveAgentBudgetLivePayload(force = false)"));
        assert!(!html.contains(
            "async function fetchClientBudgetLivePayload(force = false) {\n      if (!force && isRefreshPaused()) {"
        ));
        assert!(!html.contains("INTERACTION_HOLD_SELECTOR"));
    }

    #[test]
    fn dashboard_html_contains_agent_rename_endpoint_and_inline_tooltip_trigger() {
        let html = render_html(1000, None);
        assert!(html.contains("/api/agent-display-name"));
        assert!(html.contains("content.className = \"tooltip-inline-trigger has-tooltip\";"));
    }
}
