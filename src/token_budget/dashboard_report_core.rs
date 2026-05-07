use super::*;

static DASHBOARD_REPORT_CACHE: OnceLock<Mutex<Option<DashboardReportCache>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub(super) struct DashboardReportCache {
    pub(super) repo_root: PathBuf,
    pub(super) signature: String,
    pub(super) components: DashboardReportSignatureComponents,
    pub(super) report: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DashboardReportSignatureComponents {
    pub(super) current_session_events: String,
    pub(super) rolling_window_events: String,
    pub(super) lifetime_events: String,
    pub(super) personal_agent_scope: String,
    pub(super) personal_agent_5h_events: String,
    pub(super) current_session_assistant_scope: String,
    pub(super) rolling_window_assistant_scope: String,
    pub(super) lifetime_assistant_scope: String,
    pub(super) client_live_meter: String,
    pub(super) exact_client_limits: String,
    pub(super) live_response_latency: String,
    pub(super) client_budget_target_percent: u64,
}

pub(super) fn dashboard_report_signature_components(
    current_session_events: &[TokenBudgetEvent],
    rolling_window_events: &[TokenBudgetEvent],
    lifetime_events: &[TokenBudgetEvent],
    personal_agent_scope: Option<&PersonalKpiSelector>,
    personal_agent_5h_events: &[TokenBudgetEvent],
    current_session_assistant_scope: &AssistantGenerationScopeObservation,
    rolling_window_assistant_scope: Option<&AssistantGenerationScopeObservation>,
    lifetime_assistant_scope: &AssistantGenerationScopeObservation,
    client_live_meter_observation: Option<&codex_threads::RolloutClientMeterObservation>,
    client_live_meter_binding_hint: Option<&str>,
    exact_client_limits_observation: Option<&CodexAppServerRateLimitsObservation>,
    live_response_latency: &Value,
    client_budget_target_percent: u64,
) -> DashboardReportSignatureComponents {
    DashboardReportSignatureComponents {
        current_session_events: dashboard_report_events_signature(current_session_events),
        rolling_window_events: dashboard_report_events_signature(rolling_window_events),
        lifetime_events: dashboard_report_events_signature(lifetime_events),
        personal_agent_scope: personal_agent_scope
            .map(PersonalKpiSelector::signature_key)
            .unwrap_or_else(|| "unbound".to_string()),
        personal_agent_5h_events: dashboard_report_events_signature(personal_agent_5h_events),
        current_session_assistant_scope: dashboard_report_assistant_scope_signature(
            current_session_assistant_scope,
        ),
        rolling_window_assistant_scope: rolling_window_assistant_scope
            .map(dashboard_report_assistant_scope_signature)
            .unwrap_or_else(|| hex_sha256(b"dashboard_rolling_window_scope:null")),
        lifetime_assistant_scope: dashboard_report_assistant_scope_signature(
            lifetime_assistant_scope,
        ),
        client_live_meter: dashboard_client_live_meter_signature(
            client_live_meter_observation,
            client_live_meter_binding_hint,
        ),
        exact_client_limits: dashboard_exact_client_limits_signature(
            exact_client_limits_observation,
        ),
        live_response_latency: live_response_latency_surface_signature(live_response_latency),
        client_budget_target_percent,
    }
}

pub(super) fn dashboard_report_signature(
    components: &DashboardReportSignatureComponents,
) -> String {
    let payload = json!({
        "current_session_events": components.current_session_events,
        "rolling_window_events": components.rolling_window_events,
        "lifetime_events": components.lifetime_events,
        "personal_agent_scope": components.personal_agent_scope,
        "personal_agent_5h_events": components.personal_agent_5h_events,
        "current_session_assistant_scope": components.current_session_assistant_scope,
        "rolling_window_assistant_scope": components.rolling_window_assistant_scope,
        "lifetime_assistant_scope": components.lifetime_assistant_scope,
        "client_live_meter": components.client_live_meter,
        "exact_client_limits": components.exact_client_limits,
        "live_response_latency": components.live_response_latency,
        "client_budget_target_percent": components.client_budget_target_percent,
    });
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(super) fn refresh_dashboard_report_live_ages(
    mut report: Value,
    now_epoch_ms: i64,
    current_session_events: &[TokenBudgetEvent],
    rolling_window_events: &[TokenBudgetEvent],
    lifetime_events: &[TokenBudgetEvent],
    pre_cache_timings: &DashboardReportPreCacheTimings,
    assistant_scope_debug: &DashboardAssistantScopeDebug,
) -> Value {
    if let Some(node) = report["token_budget_report"]["cache_debug"].as_object_mut() {
        node.insert("status".to_string(), Value::from("hit"));
        node.insert(
            "pre_cache_total_ms".to_string(),
            Value::from(pre_cache_timings.total_ms),
        );
        node.insert(
            "pre_cache_stage_ms".to_string(),
            dashboard_precache_stage_ms_value(pre_cache_timings),
        );
        node.insert(
            "assistant_scope_debug".to_string(),
            dashboard_assistant_scope_debug_value(assistant_scope_debug),
        );
    }
    refresh_dashboard_report_scope_age(
        &mut report,
        "current_session",
        now_epoch_ms,
        current_session_events,
    );
    refresh_dashboard_report_scope_age(
        &mut report,
        "rolling_window",
        now_epoch_ms,
        rolling_window_events,
    );
    refresh_dashboard_report_scope_age(&mut report, "lifetime", now_epoch_ms, lifetime_events);
    report
}

fn refresh_dashboard_report_scope_age(
    report: &mut Value,
    scope_key: &str,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
) {
    let Some(scope) = report["token_budget_report"][scope_key].as_object_mut() else {
        return;
    };
    if events.is_empty() {
        scope.insert("age_ms_since_latest".to_string(), Value::Null);
        return;
    }
    let latest_epoch_ms = events
        .last()
        .map(|event| event.created_at_epoch_ms)
        .unwrap_or_default();
    scope.insert(
        "age_ms_since_latest".to_string(),
        Value::from(now_epoch_ms.saturating_sub(latest_epoch_ms)),
    );
}

pub(super) fn cached_dashboard_report_entry(repo_root: &Path) -> Option<DashboardReportCache> {
    let cache = DASHBOARD_REPORT_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let entry = guard.as_ref()?;
    if canonical_repo_root(repo_root) == entry.repo_root {
        Some(entry.clone())
    } else {
        None
    }
}

pub(super) fn store_dashboard_report(
    repo_root: &Path,
    signature: &str,
    components: &DashboardReportSignatureComponents,
    report: &Value,
) {
    let cache = DASHBOARD_REPORT_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    *guard = Some(DashboardReportCache {
        repo_root: canonical_repo_root(repo_root),
        signature: signature.to_string(),
        components: components.clone(),
        report: report.clone(),
    });
}
