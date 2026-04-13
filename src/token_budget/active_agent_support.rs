use super::*;

pub(super) const ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS: i64 = 5 * 60 * 1000;
pub(super) const ACTIVE_AGENT_SECONDARY_LIMIT_WINDOW_HOURS: i64 = 24 * 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PersonalKpiSelector {
    pub(super) project_code: String,
    pub(super) namespace_code: String,
    pub(super) agent_scope: String,
    pub(super) thread_id: Option<String>,
}

impl PersonalKpiSelector {
    pub(super) fn signature_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.project_code,
            self.namespace_code,
            self.agent_scope,
            self.thread_id.as_deref().unwrap_or("no-thread")
        )
    }

    pub(super) fn scope_kind(&self) -> &'static str {
        if self.thread_id.is_some() {
            "personal_thread_scope"
        } else {
            "personal_agent_scope"
        }
    }

    pub(super) fn scope_label(&self) -> &str {
        self.thread_id
            .as_deref()
            .unwrap_or(self.agent_scope.as_str())
    }
}

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

pub(super) async fn current_workspace_personal_kpi_selector(
    db: &Client,
    repo_root: &Path,
    explicit_thread_id_hint: Option<&str>,
) -> Result<Option<PersonalKpiSelector>> {
    let repo_root_display = repo_root.display().to_string();
    let Ok(project) = postgres::get_project_by_repo_root(db, &repo_root_display).await else {
        return Ok(None);
    };
    let snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    let namespace_code = snapshot
        .as_ref()
        .and_then(|value| value["working_state_restore"]["namespace"]["code"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("continuity")
        .to_string();
    let agent_scope = working_state::current_agent_scope_for(&project.code, &namespace_code);
    let thread_id = preferred_dashboard_thread_binding_hint_with_override(
        db,
        repo_root,
        explicit_thread_id_hint,
    )
    .await?;
    Ok(Some(PersonalKpiSelector {
        project_code: project.code,
        namespace_code,
        agent_scope,
        thread_id,
    }))
}

#[cfg(test)]
pub(super) fn personal_agent_kpi_from_summary(
    summary: &Value,
    selector: Option<&PersonalKpiSelector>,
) -> Value {
    let (scope_kind, scope_label) = selector
        .map(|value| (value.scope_kind(), value.scope_label()))
        .unwrap_or(("unbound", "unbound"));
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    let (status, confidence, kpi_percent) = if counted_events > 0 {
        (
            "observed",
            "verified",
            summary["verified_effective_savings_pct"]
                .as_f64()
                .unwrap_or(0.0),
        )
    } else if events_total > 0 {
        (
            "observed",
            "preliminary",
            summary["effective_savings_pct"].as_f64().unwrap_or(0.0),
        )
    } else {
        ("missing", "missing", 0.0)
    };
    if status == "missing" {
        return json!({
            "status": "missing",
            "confidence": confidence,
            "scope_kind": scope_kind,
            "scope_label": scope_label,
            "window_hours": PERSONAL_AGENT_KPI_WINDOW_HOURS,
            "events_total": events_total,
            "counted_events": counted_events,
            "reply_prefix": "5ч KPI: н/д",
            "summary": "Для личного 5ч KPI этого agent_scope пока нет measured событий.",
        });
    }
    let classification = signed_kpi_classification(kpi_percent);
    let reply_prefix = reply_prefix_for_signed_kpi_percent(kpi_percent);
    json!({
        "status": "observed",
        "confidence": confidence,
        "scope_kind": scope_kind,
        "scope_label": scope_label,
        "window_hours": PERSONAL_AGENT_KPI_WINDOW_HOURS,
        "events_total": events_total,
        "counted_events": counted_events,
        "classification": classification,
        "kpi_percent": kpi_percent.abs(),
        "signed_kpi_percent": kpi_percent,
        "reply_prefix": reply_prefix,
        "summary": match classification {
            "saving" => format!(
                "Личный 5ч KPI текущего agent_scope идёт в экономии {:.2}% по measured token budget.",
                kpi_percent
            ),
            "overspend" => format!(
                "Личный 5ч KPI текущего agent_scope идёт в переплате {:.2}% по measured token budget.",
                kpi_percent.abs()
            ),
            _ => "Личный 5ч KPI текущего agent_scope идёт примерно 1:1 по measured token budget."
                .to_string(),
        },
    })
}

pub(super) fn preferred_personal_agent_kpi(
    summary: &Value,
    selector: Option<&PersonalKpiSelector>,
    client_live_meter: Option<&Value>,
) -> Value {
    let (scope_kind, scope_label) = selector
        .map(|value| (value.scope_kind(), value.scope_label()))
        .unwrap_or(("unbound", "unbound"));
    if let Some(online) = client_live_meter.and_then(|meter| {
        personal_agent_online_kpi_from_client_live_meter(meter, scope_kind, scope_label)
    }) {
        return online;
    }
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    json!({
        "status": "missing",
        "confidence": "missing",
        "scope_kind": scope_kind,
        "scope_label": scope_label,
        "window_hours": PERSONAL_AGENT_KPI_WINDOW_HOURS,
        "events_total": events_total,
        "counted_events": counted_events,
        "reply_prefix": "5ч KPI: н/д",
        "summary": "Для личного 5ч KPI нет exact VS Code status-bar rate-limit contour. Measured token-budget fallback для этого KPI запрещён.",
    })
}
