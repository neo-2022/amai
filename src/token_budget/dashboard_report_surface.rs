use super::*;

pub(super) struct DashboardReadOnlyStatementSurfaces {
    pub(super) current_session_statement_preview: Value,
    pub(super) rolling_window_statement_preview: Value,
    pub(super) lifetime_statement_preview: Value,
    pub(super) current_session_statement_export_preview: Value,
    pub(super) rolling_window_statement_export_preview: Value,
    pub(super) lifetime_statement_export_preview: Value,
    pub(super) headline_summary: Value,
}

pub(super) fn build_dashboard_current_session_statement_preview(
    current_session_summary: &Value,
    session_events: &[TokenBudgetEvent],
    contract: &TokenBudgetContractConfig,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    current_session_assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    build_dashboard_statement_preview(
        "current_session",
        "текущая сессия",
        current_session_summary,
        session_events,
        contract,
        rollout_observations,
        current_session_assistant_scope,
    )
}

pub(super) fn build_dashboard_read_only_statement_surfaces(
    profile: &ResolvedProfile,
    contract: &TokenBudgetContractConfig,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    current_session_summary: &Value,
    session_events: &[TokenBudgetEvent],
    current_session_assistant_scope: Option<&AssistantGenerationScopeObservation>,
    rolling_window_summary: &Value,
    rolling_window_events: &[TokenBudgetEvent],
    rolling_window_assistant_scope: Option<&AssistantGenerationScopeObservation>,
    lifetime_summary: &Value,
    lifetime_events: &[TokenBudgetEvent],
    lifetime_assistant_scope: Option<&AssistantGenerationScopeObservation>,
    client_budget_target_percent: u64,
) -> DashboardReadOnlyStatementSurfaces {
    let current_session_statement_preview = build_dashboard_current_session_statement_preview(
        current_session_summary,
        session_events,
        contract,
        rollout_observations,
        current_session_assistant_scope,
    );
    let rolling_window_statement_preview = if profile.rolling_window_hours.is_some() {
        build_dashboard_statement_preview(
            "rolling_window",
            &format!("окно {}", profile.display_name),
            rolling_window_summary,
            rolling_window_events,
            contract,
            rollout_observations,
            rolling_window_assistant_scope,
        )
    } else {
        Value::Null
    };
    let lifetime_statement_preview = build_dashboard_statement_preview(
        "lifetime",
        "всё время записи",
        lifetime_summary,
        lifetime_events,
        contract,
        rollout_observations,
        lifetime_assistant_scope,
    );
    let current_session_statement_export_preview =
        build_dashboard_statement_export_preview(&current_session_statement_preview, contract);
    let rolling_window_statement_export_preview = if profile.rolling_window_hours.is_some() {
        build_dashboard_statement_export_preview(&rolling_window_statement_preview, contract)
    } else {
        Value::Null
    };
    let lifetime_statement_export_preview =
        build_dashboard_statement_export_preview(&lifetime_statement_preview, contract);
    let headline_boundary = if profile.rolling_window_hours.is_some() {
        build_client_limit_boundary_review_surface(&rolling_window_statement_preview)
    } else {
        build_client_limit_boundary_review_surface(&lifetime_statement_preview)
    };
    let headline_summary = if profile.rolling_window_hours.is_some() {
        build_product_headline_with_target(
            rolling_window_summary,
            &format!("окно {}", profile.display_name),
            Some(&headline_boundary),
            client_budget_target_percent,
        )
    } else {
        build_product_headline_with_target(
            lifetime_summary,
            "всё время записи",
            Some(&headline_boundary),
            client_budget_target_percent,
        )
    };
    DashboardReadOnlyStatementSurfaces {
        current_session_statement_preview,
        rolling_window_statement_preview,
        lifetime_statement_preview,
        current_session_statement_export_preview,
        rolling_window_statement_export_preview,
        lifetime_statement_export_preview,
        headline_summary,
    }
}
