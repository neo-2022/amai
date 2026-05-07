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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn selector(thread_id: Option<&str>) -> PersonalKpiSelector {
        PersonalKpiSelector {
            project_code: "amai".to_string(),
            namespace_code: "continuity".to_string(),
            agent_scope: "amai::continuity::default".to_string(),
            thread_id: thread_id.map(str::to_string),
        }
    }

    #[test]
    fn personal_agent_kpi_from_summary_uses_verified_scope_savings() {
        let value = personal_agent_kpi_from_summary(
            &json!({
                "events_total": 3,
                "counted_events": 2,
                "verified_effective_savings_pct": 61.25,
                "effective_savings_pct": 44.0
            }),
            Some(&selector(None)),
        );
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(value["confidence"].as_str(), Some("verified"));
        assert_eq!(value["classification"].as_str(), Some("saving"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 61.25%")
        );
    }

    #[test]
    fn personal_agent_kpi_from_summary_prefers_thread_scope_label_when_bound() {
        let value = personal_agent_kpi_from_summary(
            &json!({
                "events_total": 0,
                "counted_events": 0
            }),
            Some(&selector(Some("thread-123"))),
        );
        assert_eq!(value["scope_kind"].as_str(), Some("personal_thread_scope"));
        assert_eq!(value["scope_label"].as_str(), Some("thread-123"));
        assert_eq!(value["reply_prefix"].as_str(), Some("5ч KPI: н/д"));
    }

    #[test]
    fn preferred_personal_agent_kpi_prefers_online_limit_contour_over_measured_summary() {
        let value = preferred_personal_agent_kpi(
            &json!({
                "events_total": 3,
                "counted_events": 2,
                "verified_effective_savings_pct": 61.25,
                "effective_savings_pct": 44.0
            }),
            Some(&selector(Some("thread-amai"))),
            Some(&json!({
                "status": "observed",
                "current_thread_bound": true,
                "ended_at_epoch_ms": 1775056740000u64,
                "primary_limit_used_percent": 14.0,
                "primary_window_duration_mins": 300,
                "primary_resets_at_epoch_seconds": 1775063220u64,
                "status_bar_rate_limits": {
                    "status": "observed",
                    "observed_at_epoch_ms": 1775056740000u64,
                    "primary_limit_used_percent": 14.0,
                    "primary_window_duration_mins": 300,
                    "primary_resets_at_epoch_seconds": 1775063220u64
                }
            })),
        );
        assert_eq!(value["confidence"].as_str(), Some("online_limit_contour"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 78.12%")
        );
    }

    #[test]
    fn preferred_personal_agent_kpi_fails_closed_without_online_limit_contour() {
        let value = preferred_personal_agent_kpi(
            &json!({
                "events_total": 3,
                "counted_events": 2,
                "verified_effective_savings_pct": 61.25,
                "effective_savings_pct": 44.0
            }),
            Some(&selector(Some("thread-amai"))),
            Some(&json!({
                "ended_at_epoch_ms": 1775056740000u64,
                "status_bar_rate_limits": {
                    "status": "missing"
                }
            })),
        );
        assert_eq!(value["status"].as_str(), Some("missing"));
        assert_eq!(value["reply_prefix"].as_str(), Some("5ч KPI: н/д"));
        assert!(
            value["summary"]
                .as_str()
                .is_some_and(|summary| summary.contains("запрещён"))
        );
    }

    fn test_token_event(
        created_at_epoch_ms: i64,
        project: &str,
        namespace: &str,
        agent_scope: &str,
        event_id: &str,
        correlation_id: &str,
        effective_savings_percent: f64,
        quality_ok: bool,
    ) -> TokenBudgetEvent {
        TokenBudgetEvent {
            snapshot_id: None,
            created_at_epoch_ms,
            event_id: event_id.to_string(),
            correlation_id: correlation_id.to_string(),
            context_pack_id: None,
            thread_id: None,
            turn_id: None,
            agent_scope: agent_scope.to_string(),
            payload_origin: "context_pack_token_budget".to_string(),
            session_id: "session-default".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 0,
            occurred_at_epoch_ms: 0,
            ingested_at_epoch_ms: 0,
            snapshot_kind: "token_budget_event".to_string(),
            source_kind: "live_context_pack".to_string(),
            traffic_class: "live".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            usage_event_schema_version: "billing-usage-event-v2".to_string(),
            settlement_statement_version: default_settlement_statement_version(),
            metering_event_schema_version: "token-budget-event-v3".to_string(),
            usage_lifecycle_model_version: "usage-lifecycle-v1".to_string(),
            baseline_method_version: default_baseline_method_version(),
            quality_method_version: default_quality_method_version(),
            coverage_model_version: default_coverage_model_version(),
            metering_freshness_model_version: default_metering_freshness_model_version(),
            excluded_taxonomy_version: default_excluded_taxonomy_version(),
            dedup_contract_version: default_dedup_contract_version(),
            backfill_policy_version: default_backfill_policy_version(),
            correction_policy_version: default_correction_policy_version(),
            freeze_close_policy_version: default_freeze_close_policy_version(),
            late_arrival_policy_version: default_late_arrival_policy_version(),
            dispute_policy_version: default_dispute_policy_version(),
            settlement_lifecycle_model_version: default_settlement_lifecycle_model_version(),
            statement_period_governance_version: default_statement_period_governance_version(),
            adjustment_preview_model_version: default_adjustment_preview_model_version(),
            adjustment_request_schema_version: default_adjustment_request_schema_version(),
            adjustment_registry_version: default_adjustment_registry_version(),
            rate_card_binding_model_version: default_rate_card_binding_model_version(),
            telemetry_surface_split_version: default_telemetry_surface_split_version(),
            event_time_policy_version: default_event_time_policy_version(),
            billing_policy_version: default_billing_policy_version(),
            suitability_model_version: default_suitability_model_version(),
            billing_mode: default_billing_mode(),
            reconciliation_contract_version: default_reconciliation_contract_version(),
            margin_model_version: default_margin_model_version(),
            infra_cost_profile_version: default_infra_cost_profile_version(),
            contractual_evidence_pack_version: default_contractual_evidence_pack_version(),
            rate_card_version: default_rate_card_version(),
            currency_profile: default_currency_profile(),
            settlement_status: default_settlement_status(),
            project: project.to_string(),
            namespace: namespace.to_string(),
            query: "token report".to_string(),
            query_hash: "hash".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            retrieval_scope_signature: Some("local_fast_cache".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 0.0,
            saved_tokens: 0,
            naive_tokens: 0,
            context_tokens: 0,
            recovery_tokens: 0,
            effective_saved_tokens: 0,
            savings_factor: 0.0,
            savings_percent: 0.0,
            effective_savings_percent,
            quality_ok,
            quality_score: 1.0,
            quality_method: "retrieval_parity".to_string(),
            quality_tier: "retrieval".to_string(),
            head_hit_target: true,
            needed_followup: false,
            followup_count: 0,
            followup_of_event_id: None,
            resolved_by_event_id: None,
            fallback_triggered: false,
            fallback_count: 0,
            document_hits: 1,
            symbol_hits_count: 0,
            file_hits: 1,
            sources_count: 1,
            chunks_count: 1,
            pack_token_count: 0,
            deduped_token_count: 0,
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
            tool_overhead_source: None,
            pre_amai_baseline_source: None,
        }
    }

    #[test]
    fn active_agent_personal_kpi_window_falls_back_to_agent_scope_when_thread_slice_missing() {
        let selector = PersonalKpiSelector {
            project_code: "bug_bounty".to_string(),
            namespace_code: "continuity".to_string(),
            agent_scope: "bug_bounty::continuity::default".to_string(),
            thread_id: Some("thread-live".to_string()),
        };
        let events = vec![
            test_token_event(
                1_000,
                "bug_bounty",
                "continuity",
                "bug_bounty::continuity::default",
                "bug-scope",
                "bug-scope",
                72.0,
                true,
            ),
            test_token_event(
                1_000,
                "bug_bounty",
                "continuity",
                "bug_bounty::continuity::other",
                "foreign-scope",
                "foreign-scope",
                10.0,
                true,
            ),
        ];

        let (window, resolved_selector, used_fallback) =
            active_agent_personal_kpi_window(&events, &selector, 2_000);

        assert!(used_fallback);
        assert_eq!(resolved_selector.thread_id, None);
        assert_eq!(
            resolved_selector.agent_scope,
            "bug_bounty::continuity::default"
        );
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].event_id, "bug-scope");
    }
}
