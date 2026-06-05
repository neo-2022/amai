use super::*;
use crate::dashboard_format::format_u64;

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

fn personal_agent_savings_reply_prefix_from_pair(
    without_amai_tokens: u64,
    with_amai_tokens: u64,
) -> String {
    if without_amai_tokens == 0 {
        return "Amai savings: н/д".to_string();
    }

    let signed_saved_tokens = without_amai_tokens as i64 - with_amai_tokens as i64;
    let signed_saved_percent = signed_saved_tokens as f64 * 100.0 / without_amai_tokens as f64;

    match signed_kpi_classification(signed_saved_percent) {
        "saving" => format!(
            "Amai savings: без Amai {}, с Amai {}, экономия {} ({:.2}%)",
            format_u64(Some(without_amai_tokens)),
            format_u64(Some(with_amai_tokens)),
            format_u64(Some(signed_saved_tokens as u64)),
            signed_saved_percent
        ),
        "overspend" => format!(
            "Amai savings: без Amai {}, с Amai {}, перерасход {} ({:.2}%)",
            format_u64(Some(without_amai_tokens)),
            format_u64(Some(with_amai_tokens)),
            format_u64(Some(signed_saved_tokens.unsigned_abs())),
            signed_saved_percent.abs()
        ),
        _ => format!(
            "Amai savings: без Amai {}, с Amai {}, 1:1",
            format_u64(Some(without_amai_tokens)),
            format_u64(Some(with_amai_tokens)),
        ),
    }
}

pub(super) fn active_agent_personal_kpi_window(
    events: &[TokenBudgetEvent],
    selector: &PersonalKpiSelector,
    now_epoch_ms: i64,
    public_savings_window: &PublicSavingsWindowContract,
) -> (Vec<TokenBudgetEvent>, PersonalKpiSelector, bool) {
    let strict_events = personal_kpi_window_events_for_hours(
        events,
        Some(selector),
        now_epoch_ms,
        public_savings_window.hours,
    );
    if selector.thread_id.is_none() || !strict_events.is_empty() {
        return (strict_events, selector.clone(), false);
    }

    let fallback_selector = PersonalKpiSelector {
        thread_id: None,
        ..selector.clone()
    };
    let fallback_events = personal_kpi_window_events_for_hours(
        events,
        Some(&fallback_selector),
        now_epoch_ms,
        public_savings_window.hours,
    );
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
    let snapshot =
        postgres::latest_working_state_restore_snapshot_for_project(db, &project.code).await?;
    let namespace_code = snapshot
        .as_ref()
        .and_then(|value| value["working_state_restore"]["namespace"]["code"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("continuity")
        .to_string();
    let agent_scope =
        working_state::current_agent_scope_for_result(&project.code, &namespace_code)?;
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

pub(super) fn personal_agent_kpi_from_summary(
    summary: &Value,
    selector: Option<&PersonalKpiSelector>,
    public_savings_window: &PublicSavingsWindowContract,
) -> Value {
    let (scope_kind, scope_label) = selector
        .map(|value| (value.scope_kind(), value.scope_label()))
        .unwrap_or(("unbound", "unbound"));
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    let confidence = if counted_events > 0 {
        "verified"
    } else if events_total > 0 {
        "preliminary"
    } else {
        "missing"
    };
    let pair = summary["verified_baseline_tokens"]
        .as_u64()
        .or_else(|| summary["baseline_tokens"].as_u64())
        .and_then(|without_amai_tokens| {
            let with_amai_tokens = summary["verified_observed_whole_cycle_with_amai_tokens"]
                .as_u64()
                .or_else(|| summary["verified_with_amai_measured_tokens"].as_u64())
                .or_else(|| summary["observed_whole_cycle_with_amai_tokens"].as_u64())
                .or_else(|| summary["with_amai_measured_tokens"].as_u64())
                .or_else(|| summary["verified_delivered_tokens"].as_u64())
                .or_else(|| summary["delivered_tokens"].as_u64())?;
            (without_amai_tokens > 0).then_some((without_amai_tokens, with_amai_tokens))
        });
    let Some((without_amai_tokens, with_amai_tokens)) = pair else {
        return json!({
            "status": "missing",
            "confidence": confidence,
            "scope_kind": scope_kind,
            "scope_label": scope_label,
            "window_hours": public_savings_window.hours,
            "window_source": public_savings_window.source,
            "events_total": events_total,
            "counted_events": counted_events,
            "without_amai_tokens": Value::Null,
            "with_amai_tokens": Value::Null,
            "saved_tokens": Value::Null,
            "saved_pct": Value::Null,
            "reply_prefix": "Amai savings: н/д",
            "summary": "Для Amai savings этой линии пока не materialized честная token-pair пара.",
        });
    };
    let saved_tokens = without_amai_tokens as i64 - with_amai_tokens as i64;
    let signed_kpi_percent = if without_amai_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / without_amai_tokens as f64
    };
    let classification = signed_kpi_classification(signed_kpi_percent);
    let reply_prefix =
        personal_agent_savings_reply_prefix_from_pair(without_amai_tokens, with_amai_tokens);
    json!({
        "status": "observed",
        "confidence": confidence,
        "scope_kind": scope_kind,
        "scope_label": scope_label,
        "window_hours": public_savings_window.hours,
        "window_source": public_savings_window.source,
        "events_total": events_total,
        "counted_events": counted_events,
        "classification": classification,
        "without_amai_tokens": without_amai_tokens,
        "with_amai_tokens": with_amai_tokens,
        "saved_tokens": saved_tokens,
        "saved_pct": signed_kpi_percent.abs(),
        "kpi_percent": signed_kpi_percent.abs(),
        "signed_kpi_percent": signed_kpi_percent,
        "reply_prefix": reply_prefix,
        "summary": match classification {
            "saving" => format!(
                "Личная Amai savings текущего agent_scope: без Amai {}, с Amai {}, экономия {} ({:.2}%).",
                format_u64(Some(without_amai_tokens)),
                format_u64(Some(with_amai_tokens)),
                format_u64(Some(saved_tokens as u64)),
                signed_kpi_percent.abs()
            ),
            "overspend" => format!(
                "Личная Amai savings текущего agent_scope: без Amai {}, с Amai {}, перерасход {} ({:.2}%).",
                format_u64(Some(without_amai_tokens)),
                format_u64(Some(with_amai_tokens)),
                format_u64(Some(saved_tokens.unsigned_abs())),
                signed_kpi_percent.abs()
            ),
            _ => format!(
                "Личная Amai savings текущего agent_scope идёт примерно 1:1: без Amai {}, с Amai {}.",
                format_u64(Some(without_amai_tokens)),
                format_u64(Some(with_amai_tokens)),
            ),
        },
    })
}

pub(super) fn preferred_personal_agent_kpi(
    summary: &Value,
    selector: Option<&PersonalKpiSelector>,
    client_live_meter: Option<&Value>,
    public_savings_window: &PublicSavingsWindowContract,
) -> Value {
    let _ = client_live_meter;
    personal_agent_kpi_from_summary(summary, selector, public_savings_window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn public_savings_window() -> PublicSavingsWindowContract {
        PublicSavingsWindowContract {
            hours: 24,
            source: "repo_config",
        }
    }

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
                "effective_savings_pct": 44.0,
                "verified_baseline_tokens": 240,
                "verified_observed_whole_cycle_with_amai_tokens": 93
            }),
            Some(&selector(None)),
            &public_savings_window(),
        );
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(value["confidence"].as_str(), Some("verified"));
        assert_eq!(value["classification"].as_str(), Some("saving"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("Amai savings: без Amai 240, с Amai 93, экономия 147 (61.25%)")
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
            &public_savings_window(),
        );
        assert_eq!(value["scope_kind"].as_str(), Some("personal_thread_scope"));
        assert_eq!(value["scope_label"].as_str(), Some("thread-123"));
        assert_eq!(value["reply_prefix"].as_str(), Some("Amai savings: н/д"));
    }

    #[test]
    fn preferred_personal_agent_kpi_uses_summary_even_when_live_meter_present() {
        let value = preferred_personal_agent_kpi(
            &json!({
                "events_total": 3,
                "counted_events": 2,
                "verified_effective_savings_pct": 61.25,
                "effective_savings_pct": 44.0,
                "verified_baseline_tokens": 240,
                "verified_observed_whole_cycle_with_amai_tokens": 93
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
            &public_savings_window(),
        );
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("Amai savings: без Amai 240, с Amai 93, экономия 147 (61.25%)")
        );
    }

    #[test]
    fn preferred_personal_agent_kpi_uses_summary_without_live_meter() {
        let value = preferred_personal_agent_kpi(
            &json!({
                "events_total": 3,
                "counted_events": 2,
                "verified_effective_savings_pct": 61.25,
                "effective_savings_pct": 44.0,
                "verified_baseline_tokens": 240,
                "verified_observed_whole_cycle_with_amai_tokens": 93
            }),
            Some(&selector(Some("thread-amai"))),
            Some(&json!({
                "ended_at_epoch_ms": 1775056740000u64
            })),
            &public_savings_window(),
        );
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("Amai savings: без Amai 240, с Amai 93, экономия 147 (61.25%)")
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
            rolling_window_profile: "client_primary_budget".to_string(),
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
            active_agent_personal_kpi_window(&events, &selector, 2_000, &public_savings_window());

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
