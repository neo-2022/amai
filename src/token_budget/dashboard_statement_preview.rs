use super::*;

pub(super) fn build_client_limit_boundary_review_surface(statement_preview: &Value) -> Value {
    let alignment = &statement_preview["client_limit_meter_alignment"];
    let explicit_boundary_surface = alignment["explicit_boundary_surface"].clone();
    let continuity_boundary_rollup = alignment["continuity_boundary_rollup"].clone();
    let pre_amai_baseline_source_status = alignment["pre_amai_baseline_source_status"].clone();
    let exact_pair_status = alignment["exact_pair_status"].clone();
    let frozen_gap_review_surface = alignment["frozen_gap_review_surface"].clone();
    let strict_client_meter_slice = alignment["strict_client_meter_slice"].clone();
    let same_meter_as_client_limit =
        alignment["same_meter_as_client_limit"].as_bool() == Some(true);
    let review_state = if same_meter_as_client_limit {
        "same_meter_equivalent"
    } else {
        match (
            continuity_boundary_rollup["state"]
                .as_str()
                .unwrap_or("unknown"),
            explicit_boundary_surface["state"]
                .as_str()
                .unwrap_or("unknown"),
            strict_client_meter_slice["state"]
                .as_str()
                .unwrap_or("unknown"),
        ) {
            ("amai_continuity_boundary_observed", "amai_continuity_boundary", _) => {
                "strict_slice_plus_observed_amai_continuity_boundary"
            }
            ("amai_continuity_boundary_present_without_tokens", "amai_continuity_boundary", _) => {
                "strict_slice_plus_empty_amai_continuity_boundary"
            }
            (_, "amai_continuity_boundary", _) => "amai_continuity_boundary_present",
            (_, "no_explicit_boundary", "strict_slice_covers_all_applicable_components") => {
                "strict_slice_covers_all_applicable_components"
            }
            (_, "no_explicit_boundary", "strict_slice_partial_lower_bound") => {
                "strict_slice_partial_without_explicit_boundary"
            }
            _ => "client_limit_boundary_review_unknown",
        }
    };
    let note = match review_state {
        "same_meter_equivalent" => {
            "В этом scope full same-meter equivalence уже materialized: карточка и model-token percent теперь можно читать в том же meter, которым клиент считает лимит."
        }
        "strict_slice_plus_observed_amai_continuity_boundary" => {
            "Strict client-meter slice уже measured, а Amai-specific continuity boundary вынесена отдельно как observed token weight вне same-meter slice."
        }
        "strict_slice_plus_empty_amai_continuity_boundary" => {
            "Amai continuity boundary уже объявлена как explicit boundary, но в текущем scope у неё ещё нет observed token weight."
        }
        "amai_continuity_boundary_present" => {
            "В этом scope остаётся explicit Amai continuity boundary, поэтому full same-meter equivalence с клиентским лимитом честно не заявляется."
        }
        "strict_slice_covers_all_applicable_components" => {
            "В этом scope strict client-meter slice покрывает все applicable components без отдельной explicit boundary."
        }
        "strict_slice_partial_without_explicit_boundary" => {
            "Strict client-meter slice уже materialized, но пока покрывает только часть applicable components и не должен выдаваться за полный client-limit meter."
        }
        _ => {
            "Этот surface оставляет boundary semantics отдельным review/export слоем: он показывает measured strict slice и explicit continuity boundary без operational same-meter детализации."
        }
    };
    let note = if frozen_gap_review_surface["state"].as_str() == Some("review_required") {
        format!(
            "{note} Дополнительно exact raw history здесь ещё честно unavailable: irrecoverable historical debt уже требует отдельного frozen-gap review и запрещает claim raw exact history."
        )
    } else {
        note.to_string()
    };
    json!({
        "same_meter_as_client_limit": same_meter_as_client_limit,
        "alignment_state": alignment["alignment_state"].clone(),
        "baseline_equivalence_state": alignment["baseline_equivalence"]["state"].clone(),
        "review_state": review_state,
        "strict_client_meter_slice": strict_client_meter_slice,
        "explicit_boundary_surface": explicit_boundary_surface,
        "continuity_boundary_rollup": continuity_boundary_rollup,
        "pre_amai_baseline_source_status": pre_amai_baseline_source_status,
        "exact_pair_status": exact_pair_status,
        "frozen_gap_review_surface": frozen_gap_review_surface,
        "note": note,
    })
}

pub(super) fn build_dashboard_statement_preview(
    scope_code: &str,
    scope_label: &str,
    summary: &Value,
    events: &[TokenBudgetEvent],
    contract: &TokenBudgetContractConfig,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let with_amai_measured_tokens = summary["total_context_tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(summary["total_recovery_tokens"].as_u64().unwrap_or(0));
    let verified_with_amai_measured_tokens = summary["verified_delivered_tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(summary["verified_recovery_tokens"].as_u64().unwrap_or(0));
    let observed_whole_cycle_with_amai_tokens =
        observed_whole_cycle_with_assistant_scope_tokens(summary, assistant_scope)
            .unwrap_or(with_amai_measured_tokens);
    let verified_observed_whole_cycle_with_amai_tokens =
        verified_observed_whole_cycle_with_assistant_scope_tokens(summary, assistant_scope)
            .unwrap_or(verified_with_amai_measured_tokens);
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "statement_status": "dashboard_read_only_preview",
        "preliminary": summary["preliminary"].clone(),
        "counted_events": summary["counted_events"].clone(),
        "events_total": summary["events_total"].clone(),
        "coverage": summary["coverage"].clone(),
        "client_limit_meter_alignment": build_client_limit_meter_alignment(
            contract,
            "dashboard_statement_preview",
            summary,
            Some(events),
            Some(rollout_observations),
            assistant_scope,
        ),
        "observed_client_prompt_tokens": summary["observed_client_prompt_tokens"].clone(),
        "observed_assistant_generation_tokens": Value::from(
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or_else(|| summary["observed_assistant_generation_tokens"].as_u64().unwrap_or(0))
        ),
        "observed_tool_overhead_tokens": summary["observed_tool_overhead_tokens"].clone(),
        "observed_continuity_restore_tokens": summary["observed_continuity_restore_tokens"].clone(),
        "without_amai_measured_tokens": summary["total_naive_tokens"].as_u64().unwrap_or(0),
        "with_amai_measured_tokens": with_amai_measured_tokens,
        "observed_whole_cycle_with_amai_tokens": observed_whole_cycle_with_amai_tokens,
        "measured_saved_tokens": summary["total_effective_saved_tokens"].as_i64().unwrap_or(0),
        "measured_saved_pct": summary["effective_savings_pct"].as_f64().unwrap_or(0.0),
        "verified_without_amai_measured_tokens": summary["verified_baseline_tokens"].as_u64().unwrap_or(0),
        "verified_with_amai_measured_tokens": verified_with_amai_measured_tokens,
        "verified_observed_whole_cycle_with_amai_tokens": verified_observed_whole_cycle_with_amai_tokens,
        "verified_measured_saved_tokens": summary["verified_effective_saved_tokens"].as_i64().unwrap_or(0),
        "verified_measured_saved_pct": summary["verified_effective_savings_pct"].as_f64().unwrap_or(0.0),
        "note": "Dashboard preview intentionally stays read-only and lightweight: it is for fast operator cards, not for contractual export, settlement, or billing semantics."
    })
}

pub(super) fn build_dashboard_statement_export_preview(
    statement_preview: &Value,
    contract: &TokenBudgetContractConfig,
) -> Value {
    if statement_preview.is_null() {
        return Value::Null;
    }
    let client_limit_boundary_semantics =
        build_client_limit_boundary_review_surface(statement_preview);
    json!({
        "surface": "dashboard_export_compact",
        "reviewed_frozen_debt_export_surface": build_reviewed_frozen_debt_export_surface(
            contract,
            &client_limit_boundary_semantics,
            statement_preview["scope_code"].as_str(),
        ),
    })
}

pub(super) fn observed_whole_cycle_with_assistant_scope_tokens(
    summary: &Value,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Option<u64> {
    let observed_whole_cycle_with_amai_tokens =
        summary["observed_whole_cycle_with_amai_tokens"].as_u64()?;
    let observed_assistant_generation_tokens = summary["observed_assistant_generation_tokens"]
        .as_u64()
        .unwrap_or(0);
    Some(
        observed_whole_cycle_with_amai_tokens.saturating_add(
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or(observed_assistant_generation_tokens)
                .saturating_sub(observed_assistant_generation_tokens),
        ),
    )
}

pub(super) fn verified_observed_whole_cycle_with_assistant_scope_tokens(
    summary: &Value,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Option<u64> {
    let verified_observed_whole_cycle_with_amai_tokens =
        summary["verified_observed_whole_cycle_with_amai_tokens"].as_u64()?;
    let verified_observed_assistant_generation_tokens =
        summary["verified_observed_assistant_generation_tokens"]
            .as_u64()
            .unwrap_or(0);
    Some(
        verified_observed_whole_cycle_with_amai_tokens.saturating_add(
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or(verified_observed_assistant_generation_tokens)
                .saturating_sub(verified_observed_assistant_generation_tokens),
        ),
    )
}
