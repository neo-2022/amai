use super::external_truth;
use super::{
    AssistantGenerationScopeObservation, MeasurementConfig, NaiveScope, TokenBudgetContractConfig,
    TokenBudgetEvent, TokenLedgerRepairRequest, apply_reverification_metadata,
    baseline_strategy_breakdown, build_baseline_contract_json, build_billing_policy_json,
    build_continuity_restore_observed_event, build_contractual_evidence_pack,
    build_contractual_statement_summary, build_event_payload, build_margin_contract_json,
    build_margin_scope, build_metering_freshness_contract_json, build_metering_freshness_summary,
    build_product_headline, build_reconciliation_contract_json, build_reconciliation_preview,
    build_settlement_contract_json, build_statement_export_preview, build_statement_preview,
    build_telemetry_surfaces_json, build_usage_event_schema_json, contractual_line_item_json,
    count_cli_context_pack_output_overhead_tokens, count_tool_overhead_tokens,
    current_session_events, dashboard_assistant_scope_source_signature, dashboard_report_signature,
    dashboard_report_signature_components, dashboard_same_meter_sync_signature,
    dashboard_token_events_delta_limit, dashboard_token_events_signature_from_summary,
    default_adjustment_preview_model_version, default_adjustment_registry_version,
    default_adjustment_request_schema_version, default_backfill_policy_version,
    default_baseline_method_version, default_billing_mode, default_billing_policy_version,
    default_contractual_evidence_pack_version, default_correction_policy_version,
    default_coverage_model_version, default_currency_profile, default_dedup_contract_version,
    default_dispute_policy_version, default_event_time_policy_version,
    default_excluded_taxonomy_version, default_freeze_close_policy_version,
    default_infra_cost_profile_version, default_late_arrival_policy_version,
    default_margin_model_version, default_metering_event_schema_version,
    default_metering_freshness_model_version, default_quality_method_version,
    default_rate_card_binding_model_version, default_rate_card_version,
    default_reconciliation_contract_version, default_settlement_lifecycle_model_version,
    default_settlement_statement_version, default_settlement_status,
    default_statement_period_governance_version, default_suitability_model_version,
    default_telemetry_surface_split_version, default_usage_event_schema_version,
    default_usage_lifecycle_model_version, derive_baseline_strategy, derive_quality_verdict,
    derive_query_type, derive_traffic_class, event_to_json, followup_queries_related, hex_sha256,
    include_traffic_class_in_report, latency_slice_breakdown, matches_token_ledger_repair_selector,
    needs_live_reverification, parse_snapshot_event, reconcile_followup_recovery,
    render_context_pack_prompt, repair_legacy_token_event_payload, report_contract_json,
    resolve_session_id, rewrite_token_ledger_source_kind_payload, summarize_events,
    suppress_shadowed_live_events,
};
use crate::postgres::{ObservabilitySnapshotKindSummary, ObservabilitySnapshotRecord};
use crate::token_budget::dashboard_assistant_scope::DashboardAssistantScopeTargetedSummary;
use anyhow::anyhow;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn contract_fixture() -> TokenBudgetContractConfig {
    TokenBudgetContractConfig::default()
}

fn measurement_fixture() -> MeasurementConfig {
    MeasurementConfig {
        tokenizer: "o200k_base".to_string(),
        naive_limit_files: 5,
        naive_max_bytes_per_file: 16384,
        include_verify_events_by_default: false,
        metering_ingest_warning_seconds: 60,
        metering_ingest_slo_seconds: 300,
        late_arrival_grace_minutes: 60,
        preliminary_min_events: 50,
        preliminary_min_baseline_tokens: 100_000,
    }
}

fn profile_fixture() -> super::ResolvedProfile {
    super::ResolvedProfile {
        code: "local_default".to_string(),
        display_name: "Обычная рабочая машина".to_string(),
        description: "test".to_string(),
        session_gap_minutes: 30,
        rolling_window_hours: Some(24),
    }
}

fn unique_temp_path(prefix: &str, extension: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
}

fn adjustment_registry_fixture(contract: &TokenBudgetContractConfig) -> serde_json::Value {
    super::token_adjustments::build_adjustment_registry_json(
        Path::new("/tmp/amai-no-adjustments"),
        contract,
    )
}

fn rate_card_fixture(contract: &TokenBudgetContractConfig) -> serde_json::Value {
    external_truth::build_rate_card_json(Path::new("/tmp/amai-no-rate-card"), contract)
}

fn provider_usage_binding_fixture(rate_card: &serde_json::Value) -> serde_json::Value {
    external_truth::load_provider_usage_binding_from_source(
        &json!({
            "status": "not_configured",
            "binding_status": "not_configured",
        }),
        rate_card,
    )
}

fn provider_invoice_binding_fixture() -> serde_json::Value {
    external_truth::load_provider_invoice_binding_from_source(&json!({
        "status": "not_configured",
        "binding_status": "not_configured",
    }))
}

macro_rules! token_event {
        ($($field:ident : $value:expr,)+) => {
            {
                let mut event = TokenBudgetEvent {
                    snapshot_id: None,
                    created_at_epoch_ms: 0,
                    event_id: "event-default".to_string(),
                    correlation_id: "event-default".to_string(),
                    context_pack_id: None,
                    thread_id: None,
                    turn_id: None,
                    agent_scope: "art::continuity::default".to_string(),
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
                    project: "art".to_string(),
                    namespace: "continuity".to_string(),
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
                    effective_savings_percent: 0.0,
                    quality_ok: true,
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
                };
                $(event.$field = $value;)+
                event
            }
        };
    }

#[test]
fn suppress_shadowed_live_events_drops_stale_live_sibling() {
    let live = token_event! {
        created_at_epoch_ms: 100,
        event_id: "live-event".to_string(),
        correlation_id: "ctx-1".to_string(),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
    };
    let proof = token_event! {
        created_at_epoch_ms: 200,
        event_id: "proof-event".to_string(),
        correlation_id: "ctx-1".to_string(),
        source_kind: "proof_warmup_context_pack".to_string(),
        traffic_class: "proof".to_string(),
        payload_origin: "proof_warmup_context_pack".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
    };

    let filtered = suppress_shadowed_live_events(vec![live, proof.clone()]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].event_id, proof.event_id);
}

#[test]
fn suppress_shadowed_live_events_keeps_unshadowed_live_scope() {
    let live = token_event! {
        created_at_epoch_ms: 100,
        event_id: "live-event".to_string(),
        correlation_id: "ctx-1".to_string(),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
    };
    let proof_other_scope = token_event! {
        created_at_epoch_ms: 200,
        event_id: "proof-event".to_string(),
        correlation_id: "ctx-1".to_string(),
        source_kind: "proof_warmup_context_pack".to_string(),
        traffic_class: "proof".to_string(),
        payload_origin: "proof_warmup_context_pack".to_string(),
        measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
    };

    let filtered = suppress_shadowed_live_events(vec![live.clone(), proof_other_scope]);
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().any(|event| event.event_id == live.event_id));
}

#[test]
fn suppress_shadowed_live_events_drops_stale_live_usage_sibling() {
    let stale = token_event! {
        created_at_epoch_ms: 100,
        ingested_at_epoch_ms: 100,
        event_id: "live-event".to_string(),
        correlation_id: "".to_string(),
        context_pack_id: Some("cp-1".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        tool_overhead_tokens: None,
    };
    let refreshed = token_event! {
        created_at_epoch_ms: 200,
        ingested_at_epoch_ms: 200,
        event_id: "live-event".to_string(),
        correlation_id: "".to_string(),
        context_pack_id: Some("cp-1".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        tool_overhead_tokens: Some(33),
    };

    let filtered = suppress_shadowed_live_events(vec![stale, refreshed.clone()]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(
        filtered[0].created_at_epoch_ms,
        refreshed.created_at_epoch_ms
    );
    assert_eq!(filtered[0].tool_overhead_tokens, Some(33));
}

#[test]
fn suppress_shadowed_live_events_keeps_distinct_repeat_live_events_for_same_context_pack() {
    let first = token_event! {
        created_at_epoch_ms: 100,
        ingested_at_epoch_ms: 100,
        event_id: "live-event-a".to_string(),
        correlation_id: "".to_string(),
        context_pack_id: Some("cp-1".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
    };
    let second = token_event! {
        created_at_epoch_ms: 200,
        ingested_at_epoch_ms: 200,
        event_id: "live-event-b".to_string(),
        correlation_id: "".to_string(),
        context_pack_id: Some("cp-1".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
    };

    let filtered = suppress_shadowed_live_events(vec![first.clone(), second.clone()]);
    assert_eq!(filtered.len(), 2);
    assert!(
        filtered
            .iter()
            .any(|event| event.event_id == first.event_id)
    );
    assert!(
        filtered
            .iter()
            .any(|event| event.event_id == second.event_id)
    );
}

#[test]
fn suppress_shadowed_live_events_falls_back_to_event_id_without_context_pack_identity() {
    let stale = token_event! {
        created_at_epoch_ms: 100,
        ingested_at_epoch_ms: 100,
        event_id: "legacy-live-event".to_string(),
        correlation_id: "legacy-live-event".to_string(),
        context_pack_id: None,
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        tool_overhead_tokens: None,
        tool_overhead_source: Some(json!({
            "state": "missing_context_pack_identity_irrecoverable",
        })),
    };
    let refreshed = token_event! {
        created_at_epoch_ms: 200,
        ingested_at_epoch_ms: 200,
        event_id: "legacy-live-event".to_string(),
        correlation_id: "legacy-live-event".to_string(),
        context_pack_id: None,
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        tool_overhead_tokens: None,
        tool_overhead_source: Some(json!({
            "state": "missing_context_pack_identity_irrecoverable",
        })),
    };

    let filtered = suppress_shadowed_live_events(vec![stale, refreshed.clone()]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(
        filtered[0].created_at_epoch_ms,
        refreshed.created_at_epoch_ms
    );
    assert_eq!(
        filtered[0].tool_overhead_source.as_ref().unwrap()["state"],
        "missing_context_pack_identity_irrecoverable"
    );
}

#[test]
fn filter_dashboard_token_events_drops_live_proof_runtime_contamination() {
    let proof_thread_live = token_event! {
        created_at_epoch_ms: 100,
        event_id: "proof-thread-live".to_string(),
        correlation_id: "ctx-proof-thread".to_string(),
        context_pack_id: Some("ctx-proof-thread".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        thread_id: Some("proof-reuse-123".to_string()),
    };
    let proof_source_live = token_event! {
        created_at_epoch_ms: 110,
        event_id: "proof-source-live".to_string(),
        correlation_id: "ctx-proof-source".to_string(),
        context_pack_id: Some("ctx-proof-source".to_string()),
        source_kind: "live_proof_manual_exact".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
    };
    let real_live = token_event! {
        created_at_epoch_ms: 120,
        event_id: "real-live".to_string(),
        correlation_id: "ctx-real".to_string(),
        context_pack_id: Some("ctx-real".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        thread_id: Some("019d38ab-7c35-7553-b1c0-ae83c5eabf3f".to_string()),
    };

    let filtered = super::filter_dashboard_token_events(
        vec![proof_thread_live, proof_source_live, real_live.clone()],
        false,
    );

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].event_id, real_live.event_id);
}

#[test]
fn filter_dashboard_token_events_drops_live_matrix_and_art_debug_contamination() {
    let matrix_live = token_event! {
        created_at_epoch_ms: 100,
        event_id: "matrix-live".to_string(),
        correlation_id: "ctx-matrix".to_string(),
        context_pack_id: Some("ctx-matrix".to_string()),
        source_kind: "live_matrix_turn_123_semantic".to_string(),
        payload_origin: "context_pack_token_budget_v13".to_string(),
        thread_id: Some("matrix-live-turn-123".to_string()),
        turn_id: Some("turn-semantic".to_string()),
        project: "proof_shape_alpha_123".to_string(),
        cold_warm_state: "cold".to_string(),
        latency_ms: 1534.0,
    };
    let art_debug_live = token_event! {
        created_at_epoch_ms: 110,
        event_id: "art-debug-live".to_string(),
        correlation_id: "ctx-art-debug".to_string(),
        context_pack_id: Some("ctx-art-debug".to_string()),
        source_kind: "live_art_continuity_123".to_string(),
        payload_origin: "context_pack_token_budget_v13".to_string(),
        thread_id: Some("art-live-turn-123".to_string()),
        turn_id: Some("turn-art-continuity".to_string()),
        project: "art".to_string(),
        cold_warm_state: "cold".to_string(),
        latency_ms: 12.0,
    };
    let real_live = token_event! {
        created_at_epoch_ms: 120,
        event_id: "real-live".to_string(),
        correlation_id: "ctx-real".to_string(),
        context_pack_id: Some("ctx-real".to_string()),
        source_kind: "live_context_pack".to_string(),
        payload_origin: "context_pack_token_budget_v13".to_string(),
        thread_id: Some("019d38ab-7c35-7553-b1c0-ae83c5eabf3f".to_string()),
        turn_id: Some("019d59c7-c0de-75d2-80c3-07eefad8ef5f".to_string()),
        project: "amai".to_string(),
        cold_warm_state: "cold".to_string(),
        latency_ms: 4.0,
    };

    let filtered = super::filter_dashboard_token_events(
        vec![matrix_live, art_debug_live, real_live.clone()],
        false,
    );

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].event_id, real_live.event_id);
}

#[test]
fn filter_dashboard_token_events_drops_malformed_v6_live_runtime_without_scope_signature() {
    let malformed = token_event! {
        created_at_epoch_ms: 100,
        event_id: "malformed-live".to_string(),
        payload_origin: "context_pack_token_budget_v6".to_string(),
        retrieval_scope_signature: None,
    };
    let valid = token_event! {
        created_at_epoch_ms: 200,
        event_id: "valid-live".to_string(),
        payload_origin: "context_pack_token_budget_v6".to_string(),
        retrieval_scope_signature: Some("local_fast_cache".to_string()),
    };

    let filtered = super::filter_dashboard_token_events(vec![malformed, valid.clone()], false);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].event_id, valid.event_id);
}

#[test]
fn merge_dashboard_token_events_replaces_stale_live_sibling() {
    let stale = token_event! {
        created_at_epoch_ms: 100,
        ingested_at_epoch_ms: 100,
        event_id: "live-event-stale".to_string(),
        correlation_id: "".to_string(),
        context_pack_id: Some("cp-1".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        tool_overhead_tokens: None,
    };
    let refreshed = token_event! {
        created_at_epoch_ms: 200,
        ingested_at_epoch_ms: 200,
        event_id: "live-event-refreshed".to_string(),
        correlation_id: "".to_string(),
        context_pack_id: Some("cp-1".to_string()),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        tool_overhead_tokens: Some(33),
    };

    let merged = super::merge_dashboard_token_events(vec![stale], vec![refreshed.clone()]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].created_at_epoch_ms, refreshed.created_at_epoch_ms);
    assert_eq!(merged[0].tool_overhead_tokens, Some(33));
}

#[test]
fn traffic_class_comes_from_source_kind_prefix() {
    assert_eq!(derive_traffic_class("live_context_pack"), "live");
    assert_eq!(derive_traffic_class("operator_continuity_startup"), "live");
    assert_eq!(
        derive_traffic_class("operator_continuity_compact_chat"),
        "live"
    );
    assert_eq!(
        derive_traffic_class("operator_client_budget_target"),
        "live"
    );
    assert_eq!(derive_traffic_class("verify_context_pack"), "verify");
    assert_eq!(derive_traffic_class("verify_token_benchmark"), "verify");
    assert_eq!(derive_traffic_class("proof_hostile"), "proof");
    assert_eq!(derive_traffic_class("benchmark_hot_path"), "benchmark");
    assert_eq!(derive_traffic_class("custom_unknown"), "unknown");
}

#[test]
fn normalize_token_event_traffic_class_upgrades_known_operator_source_from_unknown() {
    assert_eq!(
        super::normalize_token_event_traffic_class(Some("unknown"), "operator_continuity_startup"),
        "live"
    );
    assert_eq!(
        super::normalize_token_event_traffic_class(Some("verify"), "verify_context_pack"),
        "verify"
    );
}

#[test]
fn default_product_report_is_live_only() {
    assert!(include_traffic_class_in_report("live", false));
    assert!(!include_traffic_class_in_report("verify", false));
    assert!(!include_traffic_class_in_report("proof", false));
    assert!(!include_traffic_class_in_report("benchmark", false));
    assert!(include_traffic_class_in_report("verify", true));
    assert!(include_traffic_class_in_report("proof", true));
    assert!(include_traffic_class_in_report("benchmark", true));
}

#[test]
fn baseline_strategy_matches_realistic_non_amai_workflows() {
    assert_eq!(
        derive_baseline_strategy("onboarding_query"),
        "legacy_pre_amai"
    );
    assert_eq!(
        derive_baseline_strategy("config_lookup"),
        "ide_search_top_files"
    );
    assert_eq!(
        derive_baseline_strategy("code_lookup"),
        "ide_search_top_files"
    );
    assert_eq!(
        derive_baseline_strategy("symbol_lookup"),
        "ide_search_top_files"
    );
    assert_eq!(derive_baseline_strategy("docs_lookup"), "grep_top_files");
    assert_eq!(
        derive_baseline_strategy("cross_file_trace"),
        "grep_top_files"
    );
    assert_eq!(
        derive_baseline_strategy("architecture_question"),
        "semantic_top_k"
    );
    assert_eq!(derive_baseline_strategy("bugfix_context"), "semantic_top_k");
}

#[test]
fn query_type_is_classified_for_common_human_queries() {
    assert_eq!(
        derive_query_type("Как установить Amai и подключить к VS Code?"),
        "onboarding_query"
    );
    assert_eq!(
        derive_query_type("Почему падает retrieval и как это починить?"),
        "bugfix_context"
    );
    assert_eq!(
        derive_query_type("Где вызывается эта функция и как идёт flow?"),
        "cross_file_trace"
    );
    assert_eq!(
        derive_query_type("Покажи config и .env для Amai"),
        "config_lookup"
    );
    assert_eq!(
        derive_query_type("Где лежит нужный файл для MCP integration?"),
        "code_lookup"
    );
}

#[test]
fn quality_verdict_uses_target_kind_specific_rules() {
    let payload = json!({
        "retrieval": {
            "exact_documents": [],
            "symbol_hits": [{"name": "run"}],
            "lexical_chunks": [],
            "semantic_chunks": []
        },
        "quality": {
            "semantic_guard": {
                "abstained": false
            }
        }
    });
    let verdict = derive_quality_verdict(
        &json!({
            "query": "run symbol",
            "retrieval": payload["retrieval"].clone(),
            "quality": payload["quality"].clone()
        }),
        "symbol_lookup",
        &NaiveScope {
            files: vec![json!({"relative_path": "src/main.rs"})],
            rendered_files: Vec::new(),
        },
    );
    assert_eq!(verdict.target_kind, "symbol");
    assert!(verdict.baseline_hit_target);
    assert!(verdict.amai_hit_target);
    assert!(verdict.quality_ok);
    assert_eq!(verdict.quality_method, "hybrid_answer_proxy");
    assert_eq!(verdict.quality_tier, "answer_proxy");
    assert!(verdict.head_hit_target);
}

#[test]
fn build_event_payload_uses_unique_event_id_but_keeps_context_pack_reference() {
    let measurement = MeasurementConfig {
        tokenizer: "o200k_base".to_string(),
        naive_limit_files: 1,
        naive_max_bytes_per_file: 2048,
        include_verify_events_by_default: false,
        metering_ingest_warning_seconds: 60,
        metering_ingest_slo_seconds: 300,
        late_arrival_grace_minutes: 60,
        preliminary_min_events: 50,
        preliminary_min_baseline_tokens: 100_000,
    };
    let payload = json!({
        "context_pack_id": "ctx-pack-1",
        "project": { "code": "amai" },
        "namespace": { "code": "default" },
        "query": "src/postgres.rs observability snapshot",
        "effective_retrieval_mode": "local_strict",
        "visible_projects": [
            {
                "project_code": "amai",
                "repo_root": "/home/art/agent-memory-index"
            }
        ],
        "retrieval_runtime": { "cache_hit": true },
        "quality": {
            "semantic_guard": {
                "abstained": false
            }
        },
        "retrieval": {
            "exact_documents": [
                {
                    "project_code": "amai",
                    "repo_root": "/home/art/agent-memory-index",
                    "relative_path": "src/postgres.rs",
                    "snippet": "observability snapshot"
                }
            ],
            "symbol_hits": [],
            "lexical_chunks": [],
            "semantic_chunks": []
        }
    });

    let contract = contract_fixture();
    let first = build_event_payload(
        &payload,
        &measurement,
        &contract,
        "live_context_pack",
        "context_pack_token_budget",
    )
    .expect("first payload");
    let second = build_event_payload(
        &payload,
        &measurement,
        &contract,
        "live_context_pack",
        "context_pack_token_budget",
    )
    .expect("second payload");

    assert_eq!(first["token_budget_event"]["context_pack_id"], "ctx-pack-1");
    assert_eq!(
        second["token_budget_event"]["context_pack_id"],
        "ctx-pack-1"
    );
    assert_ne!(
        first["token_budget_event"]["event_id"],
        second["token_budget_event"]["event_id"]
    );
}

#[test]
fn build_event_payload_stamps_contract_metadata_and_correlation_id() {
    let measurement = measurement_fixture();
    let payload = json!({
        "context_pack_id": "ctx-pack-1",
        "timestamp_utc": 12345,
        "query": "token report",
        "query_hash": "hash-1",
        "query_type": "code_lookup",
        "target_kind": "file",
        "baseline_strategy": "naive_top_files",
        "baseline_tokens": 1000,
        "delivered_tokens": 120,
        "saved_tokens": 880,
        "gross_savings_pct": 88.0,
        "recovery_tokens": 20,
        "effective_saved_tokens": 860,
        "effective_savings_pct": 86.0,
        "quality_ok": true,
        "quality_score": 1.0,
        "quality_method": "retrieval_parity",
        "quality_tier": "retrieval",
        "head_hit_target": true,
        "fallback_triggered": false,
        "fallback_count": 0,
        "latency_ms": 4.0,
        "sources_count": 1,
        "chunks_count": 1,
        "file_hits": 1,
        "document_hits": 1,
        "symbol_hits": 0,
        "pack_token_count": 120,
        "deduped_token_count": 120,
        "whole_cycle_observed": {
            "client_prompt_tokens": 30,
            "assistant_generation_tokens": 20,
            "tool_overhead_tokens": 5,
            "continuity_restore_tokens": 4
        },
        "scope_snapshot": {
            "project_code": "art",
            "namespace_code": "continuity"
        }
    });

    let event = build_event_payload(
        &payload,
        &measurement,
        &contract_fixture(),
        "live_context_pack",
        "context_pack_token_budget",
    )
    .expect("event payload");

    let token_event = &event["token_budget_event"];
    assert_eq!(token_event["correlation_id"], "ctx-pack-1");
    assert_eq!(token_event["measurement_scope"], "retrieval_lower_bound");
    assert_eq!(
        token_event["contract"]["usage_event_schema_version"],
        "billing-usage-event-v2"
    );
    assert_eq!(
        token_event["contract"]["metering_event_schema_version"],
        "token-budget-event-v3"
    );
    assert_eq!(
        token_event["contract"]["billing_policy_version"],
        "report-only-v1"
    );
    assert_eq!(
        token_event["contract"]["suitability_model_version"],
        "token-suitability-v1"
    );
    assert_eq!(
        token_event["contract"]["contractual_readiness_model_version"],
        "contractual-readiness-v1"
    );
    assert_eq!(
        token_event["contract"]["customer_contractual_boundary_version"],
        "customer-contractual-boundary-v1"
    );
    assert_eq!(
        token_event["contract"]["client_limit_meter_alignment_version"],
        "client-limit-meter-alignment-v12"
    );
    assert_eq!(
        token_event["contract"]["client_limit_baseline_equivalence_version"],
        "client-limit-baseline-equivalence-v4"
    );
    assert_eq!(
        token_event["contract"]["client_limit_strict_meter_slice_version"],
        "client-limit-strict-meter-slice-v1"
    );
    assert_eq!(
        token_event["whole_cycle_observed"]["client_prompt_tokens"],
        30
    );
    assert_eq!(
        token_event["whole_cycle_observed"]["assistant_generation_tokens"],
        20
    );
    assert_eq!(
        token_event["contract"]["settlement_activation_governance_version"],
        "settlement-activation-governance-v1"
    );
    assert_eq!(
        token_event["contract"]["adjustment_activation_governance_version"],
        "adjustment-activation-governance-v1"
    );
    assert_eq!(token_event["contract"]["billing_mode"], "report_only");
    assert_eq!(
        token_event["contract"]["reconciliation_contract_version"],
        "provider-reconciliation-v10"
    );
    assert_eq!(
        token_event["contract"]["margin_model_version"],
        "margin-view-v9"
    );
    assert_eq!(
        token_event["contract"]["infra_cost_profile_version"],
        "unpriced-infra-v1"
    );
    assert_eq!(
        token_event["contract"]["contractual_evidence_pack_version"],
        "contractual-evidence-pack-v21"
    );
    assert_eq!(
        token_event["contract"]["settlement_lifecycle_model_version"],
        "settlement-lifecycle-v4"
    );
    assert_eq!(
        token_event["contract"]["statement_period_governance_version"],
        "statement-period-governance-v2"
    );
    assert_eq!(
        token_event["contract"]["adjustment_preview_model_version"],
        "adjustment-preview-v1"
    );
    assert_eq!(
        token_event["contract"]["adjustment_request_schema_version"],
        "adjustment-request-v1"
    );
    assert_eq!(
        token_event["contract"]["adjustment_registry_version"],
        "adjustment-registry-v2"
    );
    assert_eq!(
        token_event["contract"]["rate_card_binding_model_version"],
        "rate-card-binding-v3"
    );
    assert_eq!(
        token_event["contract"]["telemetry_surface_split_version"],
        "tokenonomics-surface-split-v1"
    );
    assert_eq!(
        token_event["contract"]["settlement_status"],
        "unsettled_report_only"
    );
}

#[test]
fn event_json_exposes_canonical_token_ledger_aliases() {
    let event = token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-1".to_string(),
        correlation_id: "ctx-pack-1".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "token report".to_string(),
        query_hash: "hash".to_string(),
        query_type: "architecture_question".to_string(),
        target_kind: "evidence_bundle".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "grep_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 3.0,
        saved_tokens: 700,
        naive_tokens: 1000,
        context_tokens: 300,
        recovery_tokens: 20,
        effective_saved_tokens: 680,
        savings_factor: 3.33,
        savings_percent: 70.0,
        effective_savings_percent: 68.0,
        quality_ok: true,
        quality_score: 1.0,
        quality_method: "hybrid_task_success".to_string(),
        quality_tier: "task_success_recovered".to_string(),
        head_hit_target: true,
        needed_followup: false,
        followup_count: 1,
        followup_of_event_id: Some("event-0".to_string()),
        resolved_by_event_id: None,
        fallback_triggered: true,
        fallback_count: 1,
        document_hits: 1,
        symbol_hits_count: 0,
        file_hits: 1,
        sources_count: 2,
        chunks_count: 2,
        pack_token_count: 300,
        deduped_token_count: 300,
    };

    let payload = event_to_json(&event);
    assert_eq!(payload["project_code"], "art");
    assert_eq!(payload["namespace_code"], "continuity");
    assert_eq!(payload["baseline_tokens"], 1000);
    assert_eq!(payload["delivered_tokens"], 300);
    assert_eq!(payload["gross_savings_pct"], 70.0);
    assert_eq!(
        payload["usage_identity"]["dedup_key"],
        "live_context_pack:event-1"
    );
    assert_eq!(
        payload["usage_state"]["lifecycle_status"],
        "verified_included"
    );
    assert_eq!(
        payload["usage_state"]["reporting_layer"],
        "measured_non_billable"
    );
    assert_eq!(payload["usage_state"]["backfill_status"], "live_ingest");
}

#[test]
fn usage_event_schema_contract_is_machine_readable() {
    let schema = build_usage_event_schema_json(&contract_fixture());
    assert_eq!(schema["schema_version"], "billing-usage-event-v2");
    assert_eq!(
        schema["identity"]["dedup_key_format"],
        "source_kind:event_id"
    );
    assert_eq!(schema["dedup"]["policy_version"], "event-id-source-kind-v1");
    assert_eq!(
        schema["backfill"]["policy_version"],
        "report-only-backfill-v1"
    );
    assert_eq!(
        schema["corrections"]["policy_version"],
        "report-only-correction-v1"
    );
}

#[test]
fn metering_freshness_contract_exposes_lag_thresholds() {
    let contract = contract_fixture();
    let measurement = measurement_fixture();
    let freshness_contract = build_metering_freshness_contract_json(&contract, &measurement);

    assert_eq!(freshness_contract["model_version"], "metering-freshness-v1");
    assert_eq!(freshness_contract["ingest_warning_seconds"], 60);
    assert_eq!(freshness_contract["ingest_slo_seconds"], 300);
    assert_eq!(freshness_contract["late_arrival_grace_minutes"], 60);
}

#[test]
fn billing_policy_and_rate_card_are_truthful_report_only() {
    let measurement = measurement_fixture();
    let contract = contract_fixture();
    let billing_policy = build_billing_policy_json(&contract, &measurement);
    let rate_card =
        external_truth::build_rate_card_json(Path::new("/tmp/amai-no-rate-card"), &contract);
    let baseline_contract = build_baseline_contract_json(&contract);

    assert_eq!(billing_policy["mode"], "report_only");
    assert_eq!(
        billing_policy["current_billable_state"],
        "disabled_report_only"
    );
    assert_eq!(billing_policy["required_traffic_class"], "live");
    assert_eq!(billing_policy["preliminary_thresholds"]["min_events"], 50);
    assert_eq!(rate_card["status"], "default_path_missing");
    assert_eq!(rate_card["money_conversion_enabled"], false);
    assert_eq!(baseline_contract["allowed_classes"][0], "naive_top_files");
    assert_eq!(baseline_contract["disallowed_classes"][0], "entire_repo");
}

#[test]
fn rate_card_parser_accepts_machine_readable_profile() {
    let parsed = external_truth::parse_rate_card_file(
        r#"{
                "schema_version":"provider-rate-card-v1",
                "rate_card_version":"demo-priced-v1",
                "currency_profile":"USD",
                "provider":"generic",
                "default_input_cost_per_1k_tokens":0.01,
                "default_output_cost_per_1k_tokens":0.02,
                "effective_from_epoch_ms":1000,
                "effective_to_epoch_ms":2000
            }"#,
    )
    .expect("rate card");
    assert_eq!(parsed.rate_card_version, "demo-priced-v1");
    assert_eq!(parsed.currency_profile, "USD");
    assert_eq!(parsed.effective_from_epoch_ms, Some(1000));
    assert_eq!(parsed.effective_to_epoch_ms, Some(2000));
}

#[test]
fn rate_card_binding_uses_resolved_path_and_sets_priced_status() {
    let contract = contract_fixture();
    let path = unique_temp_path("amai-rate-card", "toml");
    fs::write(
        &path,
        r#"
schema_version = "provider-rate-card-v1"
rate_card_version = "demo-priced-v1"
currency_profile = "USD"
provider = "demo-provider"
default_input_cost_per_1k_tokens = 0.01
default_output_cost_per_1k_tokens = 0.02
effective_from_epoch_ms = 1000
effective_to_epoch_ms = 2000
"#,
    )
    .expect("write rate card");
    let source = json!({
        "status": "configured_existing_path",
        "resolved_path": path.display().to_string(),
        "binding_status": "configured_but_unbound"
    });
    let rate_card = external_truth::bind_rate_card_json_from_source(&source, &contract);
    let _ = fs::remove_file(&path);

    assert_eq!(rate_card["status"], "priced_bound");
    assert_eq!(rate_card["money_conversion_enabled"], true);
    assert_eq!(rate_card["bound_rate_card_version"], "demo-priced-v1");
    assert_eq!(rate_card["provider"], "demo-provider");
    assert_eq!(rate_card["source"]["binding_status"], "priced_bound");
    assert_eq!(rate_card["effective_from_epoch_ms"], 1000);
    assert_eq!(rate_card["effective_to_epoch_ms"], 2000);
    assert_eq!(rate_card["temporal_scope_state"], "source_period_bounded");
    assert!(rate_card["source_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(
        rate_card["source_sha256"]
            .as_str()
            .unwrap_or_default()
            .len()
            > 10
    );
    assert!(rate_card["source_last_modified_epoch_ms"].is_number());
}

#[test]
fn infra_cost_profile_parser_accepts_machine_readable_profile() {
    let parsed = external_truth::parse_infra_cost_profile_file(
        r#"{
                "schema_version":"infra-cost-profile-v1",
                "infra_cost_profile_version":"demo-infra-v1",
                "currency_profile":"USD",
                "provider":"amai-self-hosted",
                "cost_per_1k_internal_billed_tokens":0.002,
                "cost_per_live_event":0.0005,
                "fixed_scope_cost_amount":0.01,
                "effective_from_epoch_ms":1000,
                "effective_to_epoch_ms":2000
            }"#,
    )
    .expect("infra cost profile");
    assert_eq!(parsed.infra_cost_profile_version, "demo-infra-v1");
    assert_eq!(parsed.currency_profile, "USD");
    assert_eq!(parsed.effective_from_epoch_ms, Some(1000));
    assert_eq!(parsed.effective_to_epoch_ms, Some(2000));
}

#[test]
fn infra_cost_profile_binding_uses_resolved_path_and_sets_priced_status() {
    let contract = contract_fixture();
    let path = unique_temp_path("amai-infra-cost", "toml");
    fs::write(
        &path,
        r#"
schema_version = "infra-cost-profile-v1"
infra_cost_profile_version = "demo-infra-v1"
currency_profile = "USD"
provider = "amai-self-hosted"
cost_per_1k_internal_billed_tokens = 0.002
cost_per_live_event = 0.0005
fixed_scope_cost_amount = 0.01
effective_from_epoch_ms = 1000
effective_to_epoch_ms = 2000
"#,
    )
    .expect("write infra cost profile");
    let source = json!({
        "status": "configured_existing_path",
        "resolved_path": path.display().to_string(),
        "binding_status": "configured_but_unbound"
    });
    let binding = external_truth::bind_infra_cost_profile_json_from_source(&source, &contract);
    let _ = fs::remove_file(&path);

    assert_eq!(binding["status"], "priced_bound");
    assert_eq!(binding["money_margin_enabled"], true);
    assert_eq!(binding["bound_profile_version"], "demo-infra-v1");
    assert_eq!(binding["source"]["binding_status"], "priced_bound");
    assert_eq!(binding["effective_from_epoch_ms"], 1000);
    assert_eq!(binding["effective_to_epoch_ms"], 2000);
    assert_eq!(binding["temporal_scope_state"], "source_period_bounded");
    assert!(binding["source_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(binding["source_sha256"].as_str().unwrap_or_default().len() > 10);
    assert!(binding["source_last_modified_epoch_ms"].is_number());
}

#[test]
fn adjustment_registry_binding_uses_resolved_path() {
    let contract = contract_fixture();
    let path = unique_temp_path("amai-adjustment-registry", "json");
    fs::write(
        &path,
        r#"{
  "adjustments": [
    {
      "adjustment_id": "adj-1",
      "scope_code": "current_session",
      "status": "applied_report_only",
      "kind": "adjustment_entry",
      "reason_code": "proof_adjustment",
      "created_at_epoch_ms": 1000,
      "tokens_delta": -12,
      "amount_delta": null,
      "currency_profile": null,
      "related_statement_id": null
    }
  ]
}"#,
    )
    .expect("write adjustment registry");
    let source = json!({
        "status": "configured_existing_path",
        "resolved_path": path.display().to_string(),
        "binding_status": "configured_but_unbound"
    });
    let registry =
        super::token_adjustments::load_adjustment_registry_from_source(&source, &contract);
    let _ = fs::remove_file(&path);

    assert_eq!(registry["status"], "loaded");
    assert_eq!(registry["entries_count"], 1);
    assert_eq!(registry["source"]["binding_status"], "loaded");
    assert!(registry["source_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(registry["source_sha256"].as_str().unwrap_or_default().len() > 10);
    assert!(registry["source_last_modified_epoch_ms"].is_number());
}

#[test]
fn provider_usage_binding_derives_cost_from_priced_rate_card() {
    let path = unique_temp_path("amai-provider-usage", "json");
    fs::write(
        &path,
        r#"{
  "schema_version": "provider-usage-export-v1",
  "provider": "demo-provider",
  "currency_profile": "USD",
  "scopes": [
    {
      "scope_code": "current_session",
      "input_tokens": 1000,
      "output_tokens": 500,
      "period_start_epoch_ms": 1000,
      "period_end_epoch_ms": 2000
    }
  ]
}"#,
    )
    .expect("write provider usage export");
    let source = json!({
        "status": "configured_existing_path",
        "resolved_path": path.display().to_string(),
        "binding_status": "configured_but_unbound"
    });
    let rate_card = json!({
        "money_conversion_enabled": true,
        "default_input_cost_per_1k_tokens": 0.01,
        "default_output_cost_per_1k_tokens": 0.02,
        "bound_currency_profile": "USD",
        "status": "priced_bound"
    });

    let binding = external_truth::load_provider_usage_binding_from_source(&source, &rate_card);
    let _ = fs::remove_file(&path);

    assert_eq!(binding["status"], "usage_and_cost_bound");
    assert_eq!(binding["scope_count"], 1);
    assert_eq!(binding["scopes"]["current_session"]["total_tokens"], 1500);
    assert_eq!(
        binding["scopes"]["current_session"]["provider_cost_amount"],
        json!(0.02)
    );
    assert_eq!(
        binding["scopes"]["current_session"]["temporal_scope_state"],
        "source_period_bounded"
    );
    assert_eq!(binding["source"]["binding_status"], "usage_and_cost_bound");
    assert!(binding["source_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(binding["source_sha256"].as_str().unwrap_or_default().len() > 10);
    assert!(binding["source_last_modified_epoch_ms"].is_number());
}

#[test]
fn provider_invoice_binding_uses_resolved_path() {
    let path = unique_temp_path("amai-provider-invoice", "json");
    fs::write(
        &path,
        r#"{
  "schema_version": "provider-invoice-export-v1",
  "provider": "demo-provider",
  "currency_profile": "USD",
  "scopes": [
    {
      "scope_code": "lifetime",
      "invoice_amount": 12.34,
      "invoice_id": "inv-1",
      "period_start_epoch_ms": 1000,
      "period_end_epoch_ms": 2000
    }
  ]
}"#,
    )
    .expect("write provider invoice export");
    let source = json!({
        "status": "configured_existing_path",
        "resolved_path": path.display().to_string(),
        "binding_status": "configured_but_unbound"
    });

    let binding = external_truth::load_provider_invoice_binding_from_source(&source);
    let _ = fs::remove_file(&path);

    assert_eq!(binding["status"], "invoice_bound");
    assert_eq!(
        binding["scopes"]["lifetime"]["invoice_amount"],
        json!(12.34)
    );
    assert_eq!(
        binding["scopes"]["lifetime"]["temporal_scope_state"],
        "source_period_bounded"
    );
    assert_eq!(binding["source"]["binding_status"], "invoice_bound");
    assert!(binding["source_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(binding["source_sha256"].as_str().unwrap_or_default().len() > 10);
    assert!(binding["source_last_modified_epoch_ms"].is_number());
}

#[test]
fn settlement_contract_and_statement_preview_stay_report_only() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = rate_card_fixture(&contract);
    let sources =
        external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
    let provider_usage_binding = provider_usage_binding_fixture(&rate_card);
    let provider_invoice_binding = provider_invoice_binding_fixture();
    let settlement_contract = build_settlement_contract_json(&contract);
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "verified_effective_saved_tokens": 1234
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1400,
        context_tokens: 166,
        effective_saved_tokens: 1234,
    }];
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );

    assert_eq!(
        settlement_contract["statement_version"],
        "settlement-preview-v6"
    );
    assert_eq!(
        settlement_contract["settlement_lifecycle_model_version"],
        "settlement-lifecycle-v4"
    );
    assert_eq!(
        settlement_contract["current_materialized_boundary"],
        "measured_report_only"
    );
    assert_eq!(
        settlement_contract["freeze_close_status"],
        "provisional_report_only"
    );
    assert_eq!(
        settlement_contract["late_arrival_status"],
        "deadline_from_latest_event_report_only"
    );
    assert_eq!(
        settlement_contract["current_contractual_state"],
        "report_only_preview_open"
    );
    assert_eq!(preview["statement_status"], "report_only_preview");
    assert_eq!(preview["lifecycle_state"], "measured_non_billable_open");
    assert_eq!(preview["settlement_stage"], "measured_open_report_only");
    assert_eq!(preview["settlement_stage_family"], "measured_report_only");
    assert_eq!(
        preview["next_settlement_stage_candidate"],
        "review_ready_blocked"
    );
    assert_eq!(
        preview["next_settlement_stage_blockers"],
        json!(["coverage_not_final", "late_arrival_window_open"])
    );
    assert_eq!(
        preview["future_reserved_settlement_stages"],
        json!([
            "billable_reserved",
            "settled_reserved",
            "invoiced_reserved",
            "credited_reserved",
            "disputed_reserved",
            "closed_reserved"
        ])
    );
    assert_eq!(
        preview["transactional_statuses"]["measured"]["status"],
        "measured_open_report_only"
    );
    assert_eq!(
        preview["transactional_statuses"]["review"]["status"],
        "review_blocked_report_only"
    );
    assert_eq!(
        preview["transactional_statuses"]["billable"]["status"],
        "billable_blocked_reserved"
    );
    assert_eq!(preview["contractual_state"], "report_only_preview_open");
    assert_eq!(
        preview["close_readiness"],
        "provisionally_blocked_report_only"
    );
    assert_eq!(
        preview["provisional_close_state"],
        "report_only_preview_provisional_hold"
    );
    assert_eq!(preview["provisional_close_candidate"], false);
    assert_eq!(preview["freeze_status"], "late_arrival_window_open");
    assert_eq!(preview["close_barriers"][0], "billing_mode_report_only");
    assert_eq!(
        preview["period"]["model_version"],
        "statement-period-governance-v2"
    );
    assert_eq!(preview["period"]["period_start_epoch_ms"], 1_000);
    assert_eq!(preview["period"]["period_end_epoch_ms"], 2_000);
    assert_eq!(
        preview["period"]["late_arrival_deadline_epoch_ms"],
        3_601_000
    );
    assert_eq!(
        preview["period"]["provisional_close_earliest_at_epoch_ms"],
        3_601_000
    );
    assert_eq!(preview["period"]["provisional_close_candidate"], false);
    assert_eq!(
        preview["adjustment_preview"]["status"],
        "default_path_missing"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["model_version"],
        "client-limit-meter-alignment-v12"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["baseline_equivalence"]["model_version"],
        "client-limit-baseline-equivalence-v4"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["strict_client_meter_slice"]["model_version"],
        "client-limit-strict-meter-slice-v1"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["explicit_boundary_surface"]["model_version"],
        "client-limit-explicit-boundary-surface-v2"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["continuity_boundary_rollup"]["model_version"],
        "client-limit-continuity-boundary-rollup-v2"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["pre_amai_baseline_source_status"]["model_version"],
        "client-limit-pre-amai-baseline-source-v2"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["frozen_gap_review_surface"]["model_version"],
        "client-limit-frozen-gap-review-surface-v1"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["surface_kind"],
        "statement_preview"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["alignment_state"],
        "partial_lower_bound_not_meter_equivalent"
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["same_meter_as_client_limit"],
        false
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["live_events_count"],
        1
    );
    assert_eq!(
        preview["client_limit_meter_alignment"]["non_live_events_count"],
        0
    );
    assert_eq!(preview["measured_non_billable_lower_bound_tokens"], 1234);
    assert_eq!(preview["billable_lower_bound_tokens"], json!(null));
}

#[test]
fn statement_preview_marks_scope_as_provisionally_stable_when_window_elapsed() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = rate_card_fixture(&contract);
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources")),
        &provider_usage_binding_fixture(&rate_card),
        &provider_invoice_binding_fixture(),
        &rate_card,
    );
    let summary = json!({
        "coverage": {
            "completeness_state": "confirmed"
        },
        "verified_effective_saved_tokens": 777,
        "delivered_tokens": 200,
        "recovery_tokens": 0
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 977,
        context_tokens: 200,
        effective_saved_tokens: 777,
    }];
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 4_000_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        4_000_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );

    assert_eq!(
        preview["provisional_close_state"],
        "report_only_preview_provisionally_stable"
    );
    assert_eq!(
        preview["settlement_stage"],
        "measured_review_ready_report_only"
    );
    assert_eq!(preview["settlement_stage_family"], "measured_report_only");
    assert_eq!(
        preview["next_settlement_stage_candidate"],
        "billable_blocked"
    );
    assert_eq!(
        preview["next_settlement_stage_blockers"],
        json!([
            "billing_mode_report_only",
            "external_reconciliation_not_bound",
            "rate_card_unpriced"
        ])
    );
    assert_eq!(
        preview["transactional_statuses"]["review"]["status"],
        "review_ready_report_only"
    );
    assert_eq!(
        preview["transactional_statuses"]["billable"]["status"],
        "billable_blocked_reserved"
    );
    assert_eq!(preview["provisional_close_candidate"], true);
    assert_eq!(preview["freeze_status"], "provisionally_frozen_report_only");
    assert_eq!(
        preview["close_readiness"],
        "provisionally_stable_report_only"
    );
    assert_eq!(preview["period"]["provisional_close_candidate"], true);
    assert_eq!(
        preview["period"]["window_state"],
        "provisionally_stable_report_only"
    );
    assert_eq!(
        preview["period"]["close_policy_state"],
        "provisional_close_candidate_report_only"
    );
    assert_eq!(
        preview["period"]["late_arrival_policy_state"],
        "provisional_deadline_elapsed"
    );
}

#[test]
fn statement_preview_uses_observed_whole_cycle_lower_bound_for_internal_meter() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = rate_card_fixture(&contract);
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources")),
        &provider_usage_binding_fixture(&rate_card),
        &provider_invoice_binding_fixture(),
        &rate_card,
    );
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "verified_effective_saved_tokens": 777,
        "delivered_tokens": 200,
        "recovery_tokens": 10,
        "observed_whole_cycle_with_amai_tokens": 260,
        "verified_observed_whole_cycle_with_amai_tokens": 230
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1037,
        context_tokens: 200,
        recovery_tokens: 10,
        effective_saved_tokens: 827,
    }];
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );

    assert_eq!(preview["internal_delivered_tokens"], 200);
    assert_eq!(preview["internal_recovery_tokens"], 10);
    assert_eq!(
        preview["internal_observed_whole_cycle_lower_bound_tokens"],
        260
    );
    assert_eq!(
        preview["verified_internal_observed_whole_cycle_lower_bound_tokens"],
        230
    );
    assert_eq!(preview["internal_provider_billed_tokens"], 260);
}

#[test]
fn statement_preview_adds_turn_attached_assistant_generation_to_internal_meter() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = rate_card_fixture(&contract);
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources")),
        &provider_usage_binding_fixture(&rate_card),
        &provider_invoice_binding_fixture(),
        &rate_card,
    );
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "verified_effective_saved_tokens": 777,
        "delivered_tokens": 200,
        "recovery_tokens": 10,
        "observed_assistant_generation_tokens": 0,
        "observed_whole_cycle_with_amai_tokens": 260,
        "verified_observed_whole_cycle_with_amai_tokens": 230
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1037,
        context_tokens: 200,
        recovery_tokens: 10,
        effective_saved_tokens: 827,
    }];
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        Some(&super::AssistantGenerationScopeObservation {
            target_group_count: 1,
            observed_group_count: 1,
            observed_tokens: 45,
            helper_only_context_pack_ids: std::collections::BTreeSet::new(),
            helper_only_non_model_visible_context_pack_ids: std::collections::BTreeSet::new(),
            target_context_pack_ids: ["cp-1".to_string()].into_iter().collect(),
            matched_context_pack_ids: ["cp-1".to_string()].into_iter().collect(),
            unmatched_context_pack_ids: std::collections::BTreeSet::new(),
            matched_turn_ids: ["thread-1:turn-1".to_string()].into_iter().collect(),
            available_turns: 1,
            available_direct_turns: 1,
            available_rollout_turns: 0,
            matched_direct_turn_ids: ["thread-1:turn-1".to_string()].into_iter().collect(),
            matched_rollout_turn_ids: std::collections::BTreeSet::new(),
        }),
    );

    assert_eq!(
        preview["internal_observed_whole_cycle_lower_bound_tokens"],
        305
    );
    assert_eq!(
        preview["verified_internal_observed_whole_cycle_lower_bound_tokens"],
        275
    );
    assert_eq!(preview["internal_provider_billed_tokens"], 305);
}

#[test]
fn telemetry_surfaces_split_operational_and_contractual_fields() {
    let surfaces = build_telemetry_surfaces_json(&contract_fixture());
    assert_eq!(surfaces["model_version"], "tokenonomics-surface-split-v1");
    assert_eq!(
        surfaces["operational_surface"]["code"],
        "engineering_live_telemetry"
    );
    assert_eq!(
        surfaces["contractual_surface"]["state"],
        "report_only_preview"
    );
    let contractual_fields = surfaces["contractual_surface"]["fields"]
        .as_array()
        .expect("contractual fields");
    assert!(contractual_fields.contains(&json!("contractual_evidence_pack")));
}

#[test]
fn adjustment_request_schema_and_registry_are_truthful_when_missing() {
    let contract = contract_fixture();
    let schema = super::token_adjustments::build_adjustment_request_schema_json(&contract);
    let registry = adjustment_registry_fixture(&contract);

    assert_eq!(schema["schema_version"], "adjustment-request-v1");
    assert_eq!(
        schema["retroactive_rewrite_policy"],
        "forbidden_use_adjustment_entries"
    );
    assert_eq!(registry["schema_version"], "adjustment-registry-v2");
    assert_eq!(registry["status"], "default_path_missing");
    assert_eq!(registry["entries_count"], 0);
}

#[test]
fn reconciliation_contract_is_truthful_without_external_sources() {
    let contract = contract_fixture();
    let sources =
        external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
    let rate_card = rate_card_fixture(&contract);
    let provider_usage_binding = provider_usage_binding_fixture(&rate_card);
    let provider_invoice_binding = provider_invoice_binding_fixture();
    let reconciliation = build_reconciliation_contract_json(
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );

    assert_eq!(
        reconciliation["contract_version"],
        "provider-reconciliation-v10"
    );
    assert_eq!(reconciliation["status"], "awaiting_provider_usage_source");
    assert_eq!(
        reconciliation["usage_truth_completeness_state"],
        "awaiting_provider_usage_source"
    );
    assert_eq!(
        reconciliation["rate_card_truth_completeness_state"],
        "awaiting_rate_card_source"
    );
    assert_eq!(
        reconciliation["provider_cost_truth_completeness_state"],
        "no_external_cost_truth"
    );
    assert_eq!(
        reconciliation["invoice_evidence_completeness_state"],
        "no_invoice_evidence_scope"
    );
    assert_eq!(
        reconciliation["money_truth_completeness_state"],
        "no_external_money_truth"
    );
    assert_eq!(
        reconciliation["reconciliation_readiness_state"],
        "awaiting_provider_usage_source"
    );
    assert_eq!(
        reconciliation["ready_for_external_reconciliation"],
        json!(false)
    );
    assert_eq!(
        reconciliation["external_truth_sources"]["provider_usage_export"]["status"],
        "default_path_missing"
    );
    assert_eq!(
        reconciliation["external_truth_sources"]["provider_rate_card"]["status"],
        "default_path_missing"
    );
    assert_eq!(
        reconciliation["external_truth_sources"]["infra_cost_profile"]["status"],
        "default_path_missing"
    );
    assert_eq!(
        reconciliation["source_requirements"]["required_sources_for_usage_truth"],
        json!(["provider_usage_export"])
    );
    assert_eq!(
        reconciliation["source_requirements"]["required_sources_for_cost_truth"],
        json!(["provider_rate_card", "provider_usage_export"])
    );
    assert_eq!(
        reconciliation["source_requirements"]["optional_sources_for_invoice_evidence"],
        json!(["provider_invoice_export"])
    );
    assert_eq!(
        reconciliation["source_requirements"]["unready_required_sources_for_usage_truth"],
        json!(["provider_usage_export"])
    );
    assert_eq!(
        reconciliation["source_requirements"]["unready_required_sources_for_cost_truth"],
        json!(["provider_rate_card", "provider_usage_export"])
    );
    assert_eq!(
        reconciliation["source_requirements"]["unready_optional_sources_for_invoice_evidence"],
        json!(["provider_invoice_export"])
    );
}

#[test]
fn reconciliation_preview_keeps_external_values_null_until_truth_is_bound() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let sources =
        external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
    let rate_card = rate_card_fixture(&contract);
    let provider_usage_binding = provider_usage_binding_fixture(&rate_card);
    let provider_invoice_binding = provider_invoice_binding_fixture();
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "verified_effective_saved_tokens": 4321
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 2_000,
        naive_tokens: 4500,
        context_tokens: 179,
        effective_saved_tokens: 4321,
    }];
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 3_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        3_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );
    let reconciliation = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &preview,
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );

    assert_eq!(
        reconciliation["reconciliation_state"],
        "awaiting_provider_usage_source"
    );
    assert_eq!(
        reconciliation["usage_truth_completeness_state"],
        "awaiting_provider_usage_source"
    );
    assert_eq!(
        reconciliation["rate_card_truth_completeness_state"],
        "awaiting_rate_card_source"
    );
    assert_eq!(
        reconciliation["provider_cost_truth_completeness_state"],
        "no_external_cost_truth"
    );
    assert_eq!(
        reconciliation["invoice_evidence_completeness_state"],
        "no_invoice_evidence_scope"
    );
    assert_eq!(
        reconciliation["money_truth_completeness_state"],
        "no_external_money_truth"
    );
    assert_eq!(
        reconciliation["reconciliation_readiness_state"],
        "awaiting_provider_usage_source"
    );
    assert_eq!(
        reconciliation["internal_measured_non_billable_lower_bound_tokens"],
        4321
    );
    assert_eq!(
        reconciliation["external_provider_usage_tokens"],
        json!(null)
    );
    assert_eq!(reconciliation["drift_tokens"], json!(null));
    assert_eq!(
        reconciliation["blocking_reasons"],
        json!([
            "provider_usage_source_missing",
            "provider_rate_card_unpriced",
            "billing_policy_report_only",
            "billable_lower_bound_not_materialized"
        ])
    );
}

#[test]
fn margin_contract_stays_unknown_without_rate_card_and_infra_cost_profile() {
    let contract = contract_fixture();
    let sources =
        external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
    let rate_card = rate_card_fixture(&contract);
    let provider_usage_binding = provider_usage_binding_fixture(&rate_card);
    let provider_invoice_binding = provider_invoice_binding_fixture();
    let reconciliation = build_reconciliation_contract_json(
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let infra_cost_source = json!({
        "status": "not_configured"
    });
    let margin = build_margin_contract_json(
        &contract,
        &sources,
        &rate_card,
        &infra_cost_source,
        &reconciliation,
    );

    assert_eq!(margin["model_version"], "margin-view-v9");
    assert_eq!(margin["infra_cost_profile_version"], "unpriced-infra-v1");
    assert_eq!(margin["status"], "awaiting_rate_card");
    assert_eq!(
        margin["rate_card_truth_completeness_state"],
        "awaiting_rate_card_source"
    );
    assert_eq!(
        margin["infra_cost_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        margin["pricing_truth_completeness_state"],
        "awaiting_rate_card_and_infra_cost_profile"
    );
    assert_eq!(
        margin["customer_savings_money_truth_completeness_state"],
        "awaiting_rate_card_source"
    );
    assert_eq!(
        margin["amai_cost_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        margin["margin_truth_completeness_state"],
        "awaiting_rate_card_and_infra_cost_profile"
    );
    assert_eq!(margin["margin_readiness_state"], "awaiting_rate_card");
    assert_eq!(margin["money_margin_enabled"], json!(false));
    assert_eq!(
        margin["source_requirements"]["required_sources_for_margin_truth"],
        json!([
            "infra_cost_profile",
            "provider_rate_card",
            "provider_usage_export"
        ])
    );
    assert_eq!(
        margin["source_requirements"]["unready_required_sources_for_margin_truth"],
        json!([
            "infra_cost_profile",
            "provider_rate_card",
            "provider_usage_export"
        ])
    );
}

#[test]
fn margin_scope_keeps_money_values_null_until_inputs_are_real() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let sources =
        external_truth::build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
    let rate_card = rate_card_fixture(&contract);
    let provider_usage_binding = provider_usage_binding_fixture(&rate_card);
    let provider_invoice_binding = provider_invoice_binding_fixture();
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "verified_effective_saved_tokens": 9876
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 5_000,
        naive_tokens: 10_000,
        context_tokens: 124,
        effective_saved_tokens: 9876,
    }];
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 9_000, &events);
    let preview = build_statement_preview(
        "lifetime",
        "всё время записи",
        9_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );
    let reconciliation = build_reconciliation_preview(
        "lifetime",
        "всё время записи",
        &preview,
        &contract,
        &sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let infra_cost_source = json!({
        "status": "not_configured"
    });
    let margin = build_margin_scope(
        &sources,
        "lifetime",
        "всё время записи",
        &preview,
        &reconciliation,
        &rate_card,
        &infra_cost_source,
    );

    assert_eq!(margin["margin_state"], "awaiting_rate_card");
    assert_eq!(margin["margin_readiness_state"], "awaiting_pricing_truth");
    assert_eq!(
        margin["pricing_truth_completeness_state"],
        "awaiting_rate_card_and_infra_cost_profile"
    );
    assert_eq!(
        margin["customer_savings_money_truth_completeness_state"],
        "awaiting_rate_card_source"
    );
    assert_eq!(
        margin["amai_cost_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        margin["margin_truth_completeness_state"],
        "awaiting_rate_card_and_infra_cost_profile"
    );
    assert_eq!(margin["customer_saved_tokens_lower_bound"], 9876);
    assert_eq!(margin["customer_saved_amount_lower_bound"], json!(null));
    assert_eq!(margin["amai_infra_cost_amount"], json!(null));
    assert_eq!(margin["margin_amount"], json!(null));
    assert_eq!(
        margin["blocking_reasons"],
        json!([
            "rate_card_unpriced",
            "infra_cost_profile_missing",
            "provider_reconciliation_not_complete"
        ])
    );
}

#[test]
fn reconciliation_preview_marks_aligned_external_usage_when_tokens_match() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = json!({
        "money_conversion_enabled": true,
        "default_input_cost_per_1k_tokens": 0.01,
        "default_output_cost_per_1k_tokens": 0.02,
        "bound_currency_profile": "USD",
        "provider": "demo-provider",
        "effective_from_epoch_ms": 1_000,
        "effective_to_epoch_ms": 2_000,
        "status": "priced_bound"
    });
    let provider_usage_binding = json!({
        "status": "usage_and_cost_bound",
        "provider": "demo-provider",
        "scopes": {
            "current_session": {
                "total_tokens": 400,
                "provider_cost_amount": 0.004,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_000,
                "period_end_epoch_ms": 2_000
            }
        }
    });
    let provider_invoice_binding = json!({
        "status": "invoice_bound",
        "provider": "demo-provider",
        "scopes": {
            "current_session": {
                "invoice_amount": 0.004,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_000,
                "period_end_epoch_ms": 2_000
            }
        }
    });
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "delivered_tokens": 400,
        "recovery_tokens": 0,
        "verified_effective_saved_tokens": 800
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1200,
        context_tokens: 400,
        recovery_tokens: 0,
        effective_saved_tokens: 800,
    }];
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );
    let reconciliation = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &preview,
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );

    assert_eq!(
        reconciliation["reconciliation_state"],
        "external_usage_and_invoice_aligned_report_only"
    );
    assert_eq!(
        reconciliation["usage_truth_completeness_state"],
        "provider_usage_bound"
    );
    assert_eq!(
        reconciliation["rate_card_truth_completeness_state"],
        "rate_card_priced_bound"
    );
    assert_eq!(
        reconciliation["money_truth_completeness_state"],
        "provider_cost_and_invoice_bound"
    );
    assert_eq!(
        reconciliation["reconciliation_readiness_state"],
        "usage_cost_and_invoice_truth_ready"
    );
    assert_eq!(
        reconciliation["usage_reconciliation_state"],
        "external_usage_aligned_report_only"
    );
    assert_eq!(
        reconciliation["invoice_reconciliation_state"],
        "invoice_aligned_report_only"
    );
    assert_eq!(
        reconciliation["provider_usage_scope_alignment_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        reconciliation["provider_invoice_scope_alignment_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        reconciliation["rate_card_scope_alignment_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        reconciliation["temporal_truth_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        reconciliation["rate_card_provider_alignment_state"],
        "provider_identity_aligned"
    );
    assert_eq!(
        reconciliation["invoice_provider_alignment_state"],
        "provider_identity_aligned"
    );
    assert_eq!(
        reconciliation["provider_identity_state"],
        "provider_identity_aligned"
    );
    assert_eq!(reconciliation["drift_tokens"], 0);
    assert_eq!(reconciliation["drift_amount"], 0.0);
    assert_eq!(reconciliation["invoice_drift_amount"], 0.0);
}

#[test]
fn margin_scope_computes_money_preview_when_inputs_are_bound() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = json!({
        "money_conversion_enabled": true,
        "default_input_cost_per_1k_tokens": 0.01,
        "default_output_cost_per_1k_tokens": 0.02,
        "bound_currency_profile": "USD",
        "provider": "demo-provider",
        "effective_from_epoch_ms": 1_000,
        "effective_to_epoch_ms": 2_000,
        "status": "priced_bound"
    });
    let provider_usage_binding = json!({
        "status": "usage_and_cost_bound",
        "provider": "demo-provider",
        "scopes": {
            "current_session": {
                "total_tokens": 450,
                "provider_cost_amount": 0.0045,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_000,
                "period_end_epoch_ms": 2_000
            }
        }
    });
    let provider_invoice_binding = json!({
        "status": "invoice_bound",
        "provider": "demo-provider",
        "scopes": {
            "current_session": {
                "invoice_amount": 0.0045,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_000,
                "period_end_epoch_ms": 2_000
            }
        }
    });
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed",
            "included_events": 2
        },
        "delivered_tokens": 400,
        "recovery_tokens": 50,
        "verified_effective_saved_tokens": 900
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1350,
        context_tokens: 400,
        recovery_tokens: 50,
        effective_saved_tokens: 900,
    }];
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );
    let reconciliation = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &preview,
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let infra_cost_profile = json!({
        "status": "priced_bound",
        "bound_currency_profile": "USD",
        "cost_per_1k_internal_billed_tokens": 0.002,
        "cost_per_live_event": 0.0005,
        "fixed_scope_cost_amount": 0.01,
        "effective_from_epoch_ms": 1_000,
        "effective_to_epoch_ms": 2_000
    });
    let margin = build_margin_scope(
        &json!({}),
        "current_session",
        "текущая сессия",
        &preview,
        &reconciliation,
        &rate_card,
        &infra_cost_profile,
    );

    assert_eq!(margin["margin_state"], "priced_preview_report_only");
    assert_eq!(margin["margin_confidence_state"], "aligned_report_only");
    assert_eq!(
        margin["margin_readiness_state"],
        "preview_ready_report_only"
    );
    assert_eq!(
        margin["customer_savings_money_truth_completeness_state"],
        "customer_savings_lower_bound_ready_report_only"
    );
    assert_eq!(
        margin["amai_cost_truth_completeness_state"],
        "amai_cost_preview_ready_report_only"
    );
    assert_eq!(
        margin["margin_truth_completeness_state"],
        "margin_preview_amounts_ready_report_only"
    );
    assert_eq!(
        margin["pricing_truth_completeness_state"],
        "pricing_truth_ready"
    );
    assert_eq!(
        margin["provider_usage_scope_alignment_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        margin["rate_card_scope_alignment_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        margin["infra_cost_scope_alignment_state"],
        "scope_period_aligned"
    );
    assert_eq!(
        margin["provider_identity_state"],
        "provider_identity_aligned"
    );
    assert_eq!(margin["temporal_truth_state"], "scope_period_aligned");
    assert!(
        (margin["customer_saved_amount_lower_bound"]
            .as_f64()
            .expect("saved amount")
            - 0.009)
            .abs()
            < 1e-9
    );
    assert!(
        (margin["amai_infra_cost_amount"]
            .as_f64()
            .expect("infra cost")
            - 0.0119)
            .abs()
            < 1e-9
    );
    assert!((margin["margin_amount"].as_f64().expect("margin amount") + 0.0029).abs() < 1e-9);
    assert!(
        (margin["savings_to_cost_ratio"].as_f64().expect("ratio") - 0.7563025210084033).abs()
            < 1e-9
    );
    assert_eq!(margin["currency_profile"], "USD");
}

#[test]
fn reconciliation_preview_marks_scope_period_mismatch_when_provider_usage_window_does_not_cover_statement()
 {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = json!({
        "money_conversion_enabled": true,
        "default_input_cost_per_1k_tokens": 0.01,
        "default_output_cost_per_1k_tokens": 0.02,
        "bound_currency_profile": "USD",
        "provider": "demo-provider",
        "effective_from_epoch_ms": 1_500,
        "effective_to_epoch_ms": 1_800,
        "status": "priced_bound"
    });
    let provider_usage_binding = json!({
        "status": "usage_and_cost_bound",
        "provider": "demo-provider",
        "scopes": {
            "current_session": {
                "total_tokens": 400,
                "provider_cost_amount": 0.004,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_500,
                "period_end_epoch_ms": 1_800
            }
        }
    });
    let provider_invoice_binding = json!({
        "status": "invoice_bound",
        "provider": "demo-provider",
        "scopes": {
            "current_session": {
                "invoice_amount": 0.004,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_500,
                "period_end_epoch_ms": 1_800
            }
        }
    });
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed"
        },
        "delivered_tokens": 400,
        "recovery_tokens": 0,
        "verified_effective_saved_tokens": 800
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1200,
        context_tokens: 400,
        recovery_tokens: 0,
        effective_saved_tokens: 800,
    }];
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );
    let reconciliation = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &preview,
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );

    assert_eq!(
        reconciliation["provider_usage_scope_alignment_state"],
        "scope_period_mismatch"
    );
    assert_eq!(
        reconciliation["provider_invoice_scope_alignment_state"],
        "scope_period_mismatch"
    );
    assert_eq!(
        reconciliation["rate_card_scope_alignment_state"],
        "scope_period_mismatch"
    );
    assert_eq!(
        reconciliation["temporal_truth_state"],
        "scope_period_mismatch"
    );
    assert!(
        reconciliation["blocking_reasons"]
            .as_array()
            .expect("blocking reasons")
            .iter()
            .any(|reason| reason == "provider_usage_scope_period_mismatch")
    );
}

#[test]
fn reconciliation_and_margin_block_on_provider_identity_mismatch() {
    let contract = contract_fixture();
    let profile = profile_fixture();
    let adjustment_registry = adjustment_registry_fixture(&contract);
    let rate_card = json!({
        "money_conversion_enabled": true,
        "default_input_cost_per_1k_tokens": 0.01,
        "default_output_cost_per_1k_tokens": 0.02,
        "bound_currency_profile": "USD",
        "provider": "demo-provider-b",
        "effective_from_epoch_ms": 1_000,
        "effective_to_epoch_ms": 2_000,
        "status": "priced_bound"
    });
    let provider_usage_binding = json!({
        "status": "usage_and_cost_bound",
        "provider": "demo-provider-a",
        "scopes": {
            "current_session": {
                "total_tokens": 450,
                "provider_cost_amount": 0.0045,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_000,
                "period_end_epoch_ms": 2_000
            }
        }
    });
    let provider_invoice_binding = json!({
        "status": "invoice_bound",
        "provider": "demo-provider-a",
        "scopes": {
            "current_session": {
                "invoice_amount": 0.0045,
                "currency_profile": "USD",
                "period_start_epoch_ms": 1_000,
                "period_end_epoch_ms": 2_000
            }
        }
    });
    let summary = json!({
        "coverage": {
            "completeness_state": "partially_confirmed",
            "included_events": 2
        },
        "delivered_tokens": 400,
        "recovery_tokens": 50,
        "verified_effective_saved_tokens": 900
    });
    let events = vec![token_event! {
        occurred_at_epoch_ms: 1_000,
        naive_tokens: 1350,
        context_tokens: 400,
        recovery_tokens: 50,
        effective_saved_tokens: 900,
    }];
    let reconciliation_contract = build_reconciliation_contract_json(
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let freshness =
        build_metering_freshness_summary(&contract, &measurement_fixture(), 2_000, &events);
    let preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        2_000,
        &events,
        &profile,
        &summary,
        &contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &freshness,
        &[],
        None,
    );
    let reconciliation = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &preview,
        &contract,
        &json!({}),
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let infra_cost_profile = json!({
        "status": "priced_bound",
        "bound_currency_profile": "USD",
        "cost_per_1k_internal_billed_tokens": 0.002,
        "cost_per_live_event": 0.0005,
        "fixed_scope_cost_amount": 0.01,
        "effective_from_epoch_ms": 1_000,
        "effective_to_epoch_ms": 2_000
    });
    let margin_contract = build_margin_contract_json(
        &contract,
        &json!({}),
        &rate_card,
        &infra_cost_profile,
        &reconciliation_contract,
    );
    let margin = build_margin_scope(
        &json!({}),
        "current_session",
        "текущая сессия",
        &preview,
        &reconciliation,
        &rate_card,
        &infra_cost_profile,
    );

    assert_eq!(
        reconciliation_contract["provider_identity_state"],
        "provider_identity_mismatch"
    );
    assert!(
        reconciliation_contract["governance_blocking_reasons"]
            .as_array()
            .expect("governance blocking reasons")
            .iter()
            .any(|reason| reason == "provider_identity_mismatch")
    );
    assert_eq!(
        reconciliation["rate_card_provider_alignment_state"],
        "provider_identity_mismatch"
    );
    assert_eq!(
        reconciliation["invoice_provider_alignment_state"],
        "provider_identity_aligned"
    );
    assert_eq!(
        reconciliation["provider_identity_state"],
        "provider_identity_mismatch"
    );
    assert!(
        reconciliation["blocking_reasons"]
            .as_array()
            .expect("blocking reasons")
            .iter()
            .any(|reason| reason == "provider_identity_mismatch")
    );
    assert_eq!(margin_contract["status"], "provider_identity_mismatch");
    assert_eq!(
        margin_contract["provider_identity_state"],
        "provider_identity_mismatch"
    );
    assert_eq!(margin["margin_state"], "provider_identity_mismatch");
    assert_eq!(
        margin["margin_confidence_state"],
        "provider_identity_mismatch"
    );
    assert_eq!(
        margin["margin_readiness_state"],
        "provider_identity_mismatch"
    );
    assert_eq!(
        margin["provider_identity_state"],
        "provider_identity_mismatch"
    );
    assert!(
        margin["blocking_reasons"]
            .as_array()
            .expect("margin blocking reasons")
            .iter()
            .any(|reason| reason == "provider_identity_mismatch")
    );
}

#[test]
fn contractual_statement_summary_compacts_statement_reconciliation_and_margin() {
    let contract = contract_fixture();
    let summary = build_contractual_statement_summary(
        &contract,
        "current_session",
        "текущая сессия",
        &json!({
            "contractual_state": "report_only_preview_open",
            "settlement_stage": "measured_open_report_only",
            "settlement_stage_family": "measured_report_only",
            "next_settlement_stage_candidate": "review_ready_blocked",
            "next_settlement_stage_blockers": ["coverage_not_final"],
            "future_reserved_settlement_stages": [
                "billable_reserved",
                "settled_reserved",
                "invoiced_reserved",
                "credited_reserved",
                "disputed_reserved",
                "closed_reserved"
            ],
            "transactional_statuses": {
                "billable": {
                    "status": "billable_blocked_reserved"
                }
            },
            "provisional_close_state": "report_only_preview_provisional_hold",
            "provisional_close_candidate": false,
            "provisional_close_barriers": ["coverage_not_final"],
            "billing_close_barriers": ["billing_mode_report_only"],
            "coverage": {
                "completeness_state": "partially_confirmed",
                "measured_events": 3,
                "included_events": 1
            },
            "period": {
                "provisional_close_earliest_at_epoch_ms": 4_000,
                "late_arrival_deadline_epoch_ms": 4_000
            },
            "measured_non_billable_lower_bound_tokens": 1234,
            "adjusted_measured_non_billable_lower_bound_tokens": 1234,
            "billable_lower_bound_tokens": json!(null),
            "adjustment_preview": {
                "correction_action_state": "no_registry_configured",
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0
            },
            "client_limit_meter_alignment": {
                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                "baseline_equivalence": {
                    "state": "baseline_component_semantics_explicit_boundary"
                },
                "strict_client_meter_slice": {
                    "state": "strict_slice_partial_lower_bound",
                    "same_meter_equivalent_for_slice": true,
                    "lower_bound_tokens": 320,
                    "components": ["client_prompt"]
                },
                "explicit_boundary_surface": {
                    "state": "amai_continuity_boundary",
                    "components": ["continuity_restore_outside_retrieval"]
                },
                "continuity_boundary_rollup": {
                    "state": "amai_continuity_boundary_observed",
                    "observed_tokens": 50329,
                    "observed_live_events": 80
                }
            },
            "currency_profile": "USD",
            "close_barriers": ["billing_mode_report_only"]
        }),
        &json!({
            "internal_provider_billed_tokens": 456,
            "internal_observed_whole_cycle_lower_bound_tokens": 456,
            "verified_internal_observed_whole_cycle_lower_bound_tokens": 430,
            "external_provider_usage_tokens": 500,
            "external_provider_cost_amount": 0.12,
            "external_invoice_amount": 0.13,
            "drift_tokens": -44,
            "external_truth_bindings": {
                "provider_rate_card": {
                    "status": "priced_bound",
                    "bound_rate_card_version": "demo-priced-v1",
                    "provider": "demo-provider",
                    "bound_currency_profile": "USD"
                },
                "provider_usage_export": {
                    "provider": "demo-provider"
                },
                "provider_invoice_export": {
                    "provider": "demo-provider"
                }
            },
            "usage_truth_completeness_state": "provider_usage_bound",
            "rate_card_truth_completeness_state": "rate_card_priced_bound",
            "provider_cost_truth_completeness_state": "provider_cost_bound",
            "invoice_evidence_completeness_state": "provider_invoice_bound",
            "money_truth_completeness_state": "provider_cost_and_invoice_bound",
            "reconciliation_readiness_state": "usage_cost_and_invoice_truth_ready",
            "governance_blocking_reasons": [],
            "rate_card_provider_alignment_state": "provider_identity_aligned",
            "invoice_provider_alignment_state": "provider_identity_aligned",
            "provider_identity_state": "provider_identity_aligned",
            "reconciliation_state": "external_usage_and_invoice_bound_report_only",
            "blocking_reasons": ["billing_policy_report_only"]
        }),
        &json!({
            "margin_state": "awaiting_infra_cost_profile",
            "margin_confidence_state": "awaiting_infra_cost_profile",
            "margin_readiness_state": "awaiting_pricing_truth",
            "infra_cost_truth_completeness_state": "awaiting_infra_cost_profile",
            "pricing_truth_completeness_state": "awaiting_infra_cost_profile",
            "customer_savings_money_truth_completeness_state": "customer_savings_lower_bound_ready_report_only",
            "amai_cost_truth_completeness_state": "awaiting_infra_cost_profile",
            "margin_truth_completeness_state": "awaiting_infra_cost_profile",
            "provider_identity_state": "provider_identity_aligned",
            "temporal_truth_state": "scope_period_aligned",
            "infra_cost_scope_alignment_state": "scope_period_aligned",
            "blocking_reasons": []
        }),
        &json!({
            "metering_ingest_state": "within_slo",
            "contractual_lag_state": "lag_window_elapsed",
            "contractual_freshness_state": "stable",
            "can_treat_scope_as_stable": true,
            "latest_event_age_ms": 3_000,
            "latest_ingest_lag_ms": 50,
            "p95_ingest_lag_ms": 50.0,
            "blocking_reasons": []
        }),
    );

    assert_eq!(summary["scope_code"], "current_session");
    assert_eq!(summary["settlement_stage"], "measured_open_report_only");
    assert_eq!(summary["settlement_stage_family"], "measured_report_only");
    assert_eq!(
        summary["next_settlement_stage_candidate"],
        "review_ready_blocked"
    );
    assert_eq!(
        summary["next_settlement_stage_blockers"],
        json!(["coverage_not_final"])
    );
    assert_eq!(
        summary["contractual_readiness_model_version"],
        "contractual-readiness-v1"
    );
    assert_eq!(
        summary["transactional_statuses"]["billable"]["status"],
        "billable_blocked_reserved"
    );
    assert_eq!(summary["coverage_state"], "partially_confirmed");
    assert_eq!(
        summary["provisional_close_state"],
        "report_only_preview_provisional_hold"
    );
    assert_eq!(summary["provisional_close_candidate"], false);
    assert_eq!(summary["provisional_close_earliest_at_epoch_ms"], 4_000);
    assert_eq!(summary["late_arrival_deadline_epoch_ms"], 4_000);
    assert_eq!(
        summary["usage_truth_completeness_state"],
        "provider_usage_bound"
    );
    assert_eq!(
        summary["rate_card_truth_completeness_state"],
        "rate_card_priced_bound"
    );
    assert_eq!(
        summary["provider_cost_truth_completeness_state"],
        "provider_cost_bound"
    );
    assert_eq!(
        summary["invoice_evidence_completeness_state"],
        "provider_invoice_bound"
    );
    assert_eq!(
        summary["money_truth_completeness_state"],
        "provider_cost_and_invoice_bound"
    );
    assert_eq!(
        summary["reconciliation_readiness_state"],
        "usage_cost_and_invoice_truth_ready"
    );
    assert_eq!(summary["rate_card_status"], "priced_bound");
    assert_eq!(summary["rate_card_version"], "demo-priced-v1");
    assert_eq!(summary["rate_card_provider"], "demo-provider");
    assert_eq!(summary["rate_card_currency_profile"], "USD");
    assert_eq!(summary["provider_usage_provider"], "demo-provider");
    assert_eq!(summary["provider_invoice_provider"], "demo-provider");
    assert_eq!(
        summary["rate_card_provider_alignment_state"],
        "provider_identity_aligned"
    );
    assert_eq!(
        summary["invoice_provider_alignment_state"],
        "provider_identity_aligned"
    );
    assert_eq!(
        summary["provider_identity_state"],
        "provider_identity_aligned"
    );
    assert_eq!(summary["metering_ingest_state"], "within_slo");
    assert_eq!(summary["contractual_lag_state"], "lag_window_elapsed");
    assert_eq!(summary["contractual_freshness_state"], "stable");
    assert_eq!(summary["internal_provider_billed_tokens"], 456);
    assert_eq!(
        summary["internal_observed_whole_cycle_lower_bound_tokens"],
        456
    );
    assert_eq!(
        summary["verified_internal_observed_whole_cycle_lower_bound_tokens"],
        430
    );
    assert_eq!(summary["external_provider_usage_tokens"], 500);
    assert_eq!(summary["drift_tokens"], -44);
    assert_eq!(
        summary["client_limit_boundary_semantics"]["review_state"],
        "strict_slice_plus_observed_amai_continuity_boundary"
    );
    assert_eq!(
        summary["client_limit_boundary_semantics"]["continuity_boundary_rollup"]["observed_tokens"],
        50329
    );
    assert_eq!(
        summary["reconciliation_state"],
        "external_usage_and_invoice_bound_report_only"
    );
    assert_eq!(summary["margin_state"], "awaiting_infra_cost_profile");
    assert_eq!(summary["margin_readiness_state"], "awaiting_pricing_truth");
    assert_eq!(
        summary["infra_cost_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        summary["pricing_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        summary["customer_savings_money_truth_completeness_state"],
        "customer_savings_lower_bound_ready_report_only"
    );
    assert_eq!(
        summary["amai_cost_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        summary["margin_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        summary["margin_provider_identity_state"],
        "provider_identity_aligned"
    );
    assert_eq!(summary["margin_blocking_reasons"], json!([]));
    assert_eq!(
        summary["internal_money_arithmetic_readiness_state"],
        "awaiting_pricing_truth"
    );
    assert_eq!(
        summary["internal_money_arithmetic_blocking_reasons"],
        json!(["pricing_truth_not_ready"])
    );
    assert_eq!(
        summary["contractual_settlement_readiness_state"],
        "review_not_yet_ready_report_only"
    );
    assert_eq!(
        summary["contractual_settlement_blocking_reasons"],
        json!([
            "billing_mode_report_only",
            "coverage_not_final",
            "money_arithmetic_not_ready"
        ])
    );
    assert_eq!(summary["can_treat_scope_as_stable"], true);
    assert_eq!(summary["customer_review_ready"], true);
    assert_eq!(summary["invoice_ready"], false);
    assert_eq!(
        summary["suitability"]["surfaces"]["product_kpi"]["state"],
        "provisional_lower_bound_with_coverage"
    );
    assert_eq!(
        summary["suitability"]["surfaces"]["billing_amount"]["state"],
        "not_billable_report_only"
    );
    assert_eq!(
        summary["blocking_reasons"],
        json!([
            "billing_mode_report_only",
            "coverage_not_final",
            "billing_policy_report_only"
        ])
    );
}

#[test]
fn metering_freshness_summary_separates_pipeline_lag_and_late_arrival_window() {
    let contract = contract_fixture();
    let measurement = measurement_fixture();
    let summary = build_metering_freshness_summary(
        &contract,
        &measurement,
        120_000,
        &[
            token_event! {
                occurred_at_epoch_ms: 1_000,
                ingested_at_epoch_ms: 1_020,
            },
            token_event! {
                occurred_at_epoch_ms: 60_000,
                ingested_at_epoch_ms: 420_500,
            },
        ],
    );

    assert_eq!(summary["metering_ingest_state"], "lagging");
    assert_eq!(summary["contractual_lag_state"], "awaiting_late_events");
    assert_eq!(summary["contractual_freshness_state"], "lagging_pipeline");
    assert_eq!(summary["can_treat_scope_as_stable"], false);
    assert_eq!(summary["latest_event_age_ms"], 60_000);
    assert_eq!(summary["latest_ingest_lag_ms"], 360_500);
    assert_eq!(
        summary["blocking_reasons"],
        json!(["metering_pipeline_lagging", "late_arrival_window_open"])
    );
}

#[test]
fn statement_export_preview_carries_hashes_and_adjustment_states() {
    let contract = contract_fixture();
    let report = json!({
        "token_budget_report": {
            "statement_previews": {
                "current_session": {
                    "contractual_state": "report_only_preview_open",
                    "adjustment_preview": {
                        "status": "not_configured",
                        "pending_entries_count": 0,
                        "disputed_entries_count": 0,
                    },
                    "client_limit_meter_alignment": {
                        "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                        "baseline_equivalence": {
                            "state": "baseline_component_semantics_explicit_boundary"
                        },
                        "strict_client_meter_slice": {
                            "model_version": "client-limit-strict-meter-slice-v1",
                            "state": "strict_slice_partial_lower_bound",
                            "same_meter_equivalent_for_slice": true,
                            "lower_bound_tokens": 320,
                            "components": ["client_prompt"],
                            "explicit_boundary_components": ["continuity_restore_outside_retrieval"],
                            "missing_components": []
                        },
                        "explicit_boundary_surface": {
                            "model_version": "client-limit-explicit-boundary-surface-v1",
                            "state": "amai_continuity_boundary",
                            "components": ["continuity_restore_outside_retrieval"]
                        },
                        "continuity_boundary_rollup": {
                            "model_version": "client-limit-continuity-boundary-rollup-v1",
                            "state": "amai_continuity_boundary_observed",
                            "components": ["continuity_restore_outside_retrieval"],
                            "observed_tokens": 50329,
                            "observed_live_events": 80
                        }
                    }
                }
            },
            "reconciliation_previews": {
                "current_session": {
                    "reconciliation_state": "awaiting_provider_usage_source"
                }
            },
            "margin_view": {
                "current_session": {
                    "margin_state": "awaiting_rate_card"
                }
            },
            "contractual_statement_summaries": {
                "current_session": {
                    "contractual_state": "report_only_preview_open",
                    "settlement_stage": "measured_open_report_only",
                    "settlement_stage_family": "measured_report_only",
                    "next_settlement_stage_candidate": "review_ready_blocked",
                    "next_settlement_stage_blockers": ["coverage_not_final"],
                    "future_reserved_settlement_stages": [
                        "billable_reserved",
                        "settled_reserved",
                        "invoiced_reserved",
                        "credited_reserved",
                        "disputed_reserved",
                        "closed_reserved"
                    ],
                    "contractual_readiness_model_version": "contractual-readiness-v1",
                    "transactional_statuses": {
                        "review": {
                            "status": "review_blocked_report_only"
                        }
                    },
                    "coverage_state": "partially_confirmed",
                    "required_sources_for_usage_truth": ["provider_usage_export"],
                    "required_sources_for_cost_truth": ["provider_rate_card", "provider_usage_export"],
                    "optional_sources_for_invoice_evidence": ["provider_invoice_export"],
                    "unready_required_sources_for_usage_truth": ["provider_usage_export"],
                    "unready_required_sources_for_cost_truth": ["provider_rate_card", "provider_usage_export"],
                    "unready_optional_sources_for_invoice_evidence": ["provider_invoice_export"],
                    "rate_card_status": "priced_bound",
                    "rate_card_truth_completeness_state": "rate_card_priced_bound",
                    "provider_cost_truth_completeness_state": null,
                    "invoice_evidence_completeness_state": null,
                    "rate_card_version": "demo-priced-v1",
                    "rate_card_provider": "demo-provider",
                    "rate_card_currency_profile": "USD",
                    "provider_usage_provider": "demo-provider",
                    "provider_invoice_provider": "demo-provider",
                    "provider_usage_scope_alignment_state": "scope_period_aligned",
                    "provider_invoice_scope_alignment_state": "scope_period_aligned",
                    "rate_card_scope_alignment_state": "scope_period_aligned",
                    "rate_card_provider_alignment_state": "provider_identity_aligned",
                    "invoice_provider_alignment_state": "provider_identity_aligned",
                    "provider_identity_state": "provider_identity_aligned",
                    "reconciliation_temporal_truth_state": "scope_period_aligned",
                    "contractual_freshness_state": "provisional_open_window",
                    "reconciliation_state": "awaiting_provider_usage_source",
                    "margin_state": "awaiting_rate_card",
                    "margin_confidence_state": "awaiting_rate_card",
                    "margin_readiness_state": "awaiting_pricing_truth",
                    "infra_cost_truth_completeness_state": "awaiting_infra_cost_profile",
                    "pricing_truth_completeness_state": "awaiting_infra_cost_profile",
                    "customer_savings_money_truth_completeness_state": null,
                    "amai_cost_truth_completeness_state": null,
                    "margin_truth_completeness_state": null,
                    "required_sources_for_margin_truth": ["infra_cost_profile", "provider_rate_card", "provider_usage_export"],
                    "optional_sources_for_margin_invoice_evidence": ["provider_invoice_export"],
                    "unready_required_sources_for_margin_truth": ["infra_cost_profile", "provider_rate_card", "provider_usage_export"],
                    "margin_provider_identity_state": "provider_identity_aligned",
                    "margin_temporal_truth_state": "scope_period_aligned",
                    "infra_cost_scope_alignment_state": "infra_cost_profile_not_bound",
                    "margin_blocking_reasons": ["rate_card_unpriced"],
                    "internal_money_arithmetic_readiness_state": "awaiting_pricing_truth",
                    "internal_money_arithmetic_blocking_reasons": ["pricing_truth_not_ready"],
                    "contractual_settlement_readiness_state": "review_not_yet_ready_report_only",
                    "contractual_settlement_blocking_reasons": ["coverage_not_final", "money_arithmetic_not_ready"],
                    "blocking_reasons": ["late_arrival_window_open"],
                    "suitability": {
                        "surfaces": {
                            "product_kpi": {
                                "usable": true,
                                "state": "provisional_lower_bound_with_coverage"
                            },
                            "billing_amount": {
                                "usable": false,
                                "state": "not_billable_report_only"
                            }
                        }
                    }
                }
            },
            "external_truth_manifest": {
                "manifest_hash": "truth-hash"
            }
        }
    });
    let events = vec![
        token_event! {
            event_id: "event-included".to_string(),
            correlation_id: "ctx-a".to_string(),
            query_hash: "hash-a".to_string(),
            naive_tokens: 1000,
            context_tokens: 100,
            effective_saved_tokens: 900,
            quality_ok: true,
        },
        token_event! {
            event_id: "event-excluded".to_string(),
            correlation_id: "ctx-b".to_string(),
            query_hash: "hash-b".to_string(),
            naive_tokens: 500,
            context_tokens: 200,
            effective_saved_tokens: 300,
            quality_ok: false,
        },
    ];

    let preview = build_statement_export_preview(
        &report,
        "current_session",
        "текущая сессия",
        &events,
        &contract,
        false,
    )
    .expect("statement export preview");

    assert_eq!(preview["model_version"], "contractual-statement-export-v21");
    assert_eq!(preview["export_status"], "review_ready_report_only");
    assert_eq!(preview["settlement_stage"], "measured_open_report_only");
    assert_eq!(preview["settlement_stage_family"], "measured_report_only");
    assert_eq!(
        preview["next_settlement_stage_candidate"],
        "review_ready_blocked"
    );
    assert_eq!(
        preview["next_settlement_stage_blockers"],
        json!(["coverage_not_final"])
    );
    assert_eq!(
        preview["provider_identity_state"],
        "provider_identity_aligned"
    );
    assert_eq!(preview["rate_card_version"], "demo-priced-v1");
    assert_eq!(
        preview["rate_card_truth_completeness_state"],
        "rate_card_priced_bound"
    );
    assert_eq!(
        preview["provider_cost_truth_completeness_state"],
        json!(null)
    );
    assert_eq!(preview["invoice_evidence_completeness_state"], json!(null));
    assert_eq!(preview["rate_card_provider"], "demo-provider");
    assert_eq!(preview["rate_card_currency_profile"], "USD");
    assert_eq!(preview["provider_usage_provider"], "demo-provider");
    assert_eq!(preview["provider_invoice_provider"], "demo-provider");
    assert_eq!(
        preview["margin_provider_identity_state"],
        "provider_identity_aligned"
    );
    assert_eq!(preview["margin_readiness_state"], "awaiting_pricing_truth");
    assert_eq!(
        preview["pricing_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        preview["customer_savings_money_truth_completeness_state"],
        json!(null)
    );
    assert_eq!(preview["amai_cost_truth_completeness_state"], json!(null));
    assert_eq!(preview["margin_truth_completeness_state"], json!(null));
    assert_eq!(
        preview["margin_blocking_reasons"],
        json!(["rate_card_unpriced"])
    );
    assert_eq!(
        preview["transactional_statuses"]["review"]["status"],
        "review_blocked_report_only"
    );
    assert_eq!(
        preview["contractual_readiness_model_version"],
        "contractual-readiness-v1"
    );
    assert_eq!(
        preview["customer_contractual_boundary"]["model_version"],
        "customer-contractual-boundary-v1"
    );
    assert_eq!(
        preview["customer_contractual_boundary"]["surface_kind"],
        "customer_review_report_only"
    );
    assert_eq!(
        preview["customer_contractual_boundary"]["review_surface_state"],
        "provisional_report_only"
    );
    assert_eq!(
        preview["customer_contractual_boundary"]["future_settlement_activation_state"],
        "review_not_yet_ready_for_future_settlement"
    );
    assert_eq!(
        preview["customer_contractual_boundary"]["future_settlement_activation_blocking_reasons"],
        json!(["coverage_not_final", "money_arithmetic_not_ready"])
    );
    assert_eq!(
        preview["settlement_activation_governance"]["model_version"],
        "settlement-activation-governance-v1"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["governance_state"],
        "activation_blocked_report_only"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["future_settlement_activation_state"],
        "review_not_yet_ready_for_future_settlement"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["future_settlement_activation_blocking_reasons"],
        json!(["coverage_not_final", "money_arithmetic_not_ready"])
    );
    assert_eq!(
        preview["settlement_activation_governance"]["registry_status"],
        "not_configured"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["adjustment_status"],
        "not_configured"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["credit_action_state"],
        "registry_not_configured"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["dispute_action_state"],
        "no_open_disputes"
    );
    assert_eq!(
        preview["settlement_activation_governance"]["pending_entries_count"],
        0
    );
    assert_eq!(
        preview["settlement_activation_governance"]["applied_entries_count"],
        0
    );
    assert_eq!(
        preview["settlement_activation_governance"]["disputed_entries_count"],
        0
    );
    assert_eq!(
        preview["adjustment_activation_governance"]["model_version"],
        "adjustment-activation-governance-v1"
    );
    assert_eq!(
        preview["adjustment_activation_governance"]["governance_state"],
        "registry_not_configured_report_only"
    );
    assert_eq!(
        preview["adjustment_activation_governance"]["future_adjustment_activation_state"],
        "future_adjustment_registry_not_bound"
    );
    assert_eq!(
        preview["adjustment_activation_governance"]["future_adjustment_activation_blocking_reasons"],
        json!(["adjustment_registry_not_bound"])
    );
    assert_eq!(
        preview["adjustment_activation_governance"]["credit_action_state"],
        "registry_not_configured"
    );
    assert_eq!(
        preview["adjustment_activation_governance"]["dispute_action_state"],
        "no_open_disputes"
    );
    assert_eq!(
        preview["internal_money_arithmetic_readiness_state"],
        "awaiting_pricing_truth"
    );
    assert_eq!(
        preview["internal_money_arithmetic_blocking_reasons"],
        json!(["pricing_truth_not_ready"])
    );
    assert_eq!(
        preview["contractual_settlement_readiness_state"],
        "review_not_yet_ready_report_only"
    );
    assert_eq!(
        preview["contractual_settlement_blocking_reasons"],
        json!(["coverage_not_final", "money_arithmetic_not_ready"])
    );
    assert_eq!(
        preview["export_semantics"]["surface_kind"],
        "customer_review_report_only"
    );
    assert_eq!(
        preview["export_semantics"]["operational_telemetry_included"],
        false
    );
    assert_eq!(
        preview["client_limit_boundary_semantics"]["review_state"],
        "strict_slice_plus_observed_amai_continuity_boundary"
    );
    assert_eq!(
        preview["client_limit_boundary_semantics"]["continuity_boundary_rollup"]["observed_tokens"],
        50329
    );
    assert_eq!(
        preview["reviewed_frozen_debt_export_surface"]["model_version"],
        "client-limit-reviewed-frozen-debt-export-surface-v1"
    );
    assert_eq!(
        preview["reviewed_frozen_debt_export_surface"]["state"],
        "not_applicable"
    );
    assert_eq!(preview["included_events_count"], 1);
    assert_eq!(preview["excluded_events_count"], 1);
    assert_eq!(
        preview["external_truth_manifest"]["manifest_hash"],
        "truth-hash"
    );
    assert_eq!(preview["credit_action_state"], "registry_not_configured");
    assert_eq!(preview["dispute_action_state"], "no_open_disputes");
    assert_eq!(preview["evidence_pack_available"], true);
    assert_eq!(
        preview["settlement_report_preview"]["model_version"],
        "settlement-report-preview-v12"
    );
    assert_eq!(
        preview["settlement_report_preview"]["client_limit_boundary_semantics"]["continuity_boundary_rollup"]
            ["observed_tokens"],
        50329
    );
    assert_eq!(
        preview["settlement_report_preview"]["customer_contractual_boundary"]["surface_kind"],
        "customer_settlement_report_preview_report_only"
    );
    assert_eq!(
        preview["required_sources_for_usage_truth"],
        json!(["provider_usage_export"])
    );
    assert_eq!(
        preview["required_sources_for_margin_truth"],
        json!([
            "infra_cost_profile",
            "provider_rate_card",
            "provider_usage_export"
        ])
    );
    assert_eq!(
        preview["settlement_report_preview"]["scope_code"],
        "current_session"
    );
    assert!(
        preview["settlement_report_preview"]["settlement_report_id"]
            .as_str()
            .unwrap_or("")
            .len()
            > 10
    );
    assert_eq!(
        preview["suitability"]["surfaces"]["product_kpi"]["state"],
        "provisional_lower_bound_with_coverage"
    );
    assert!(preview["statement_preview_id"].as_str().unwrap_or("").len() > 10);
    assert!(preview["included_events_hash"].as_str().unwrap_or("").len() > 10);
    assert!(preview["excluded_events_hash"].as_str().unwrap_or("").len() > 10);
}

#[test]
fn contractual_line_item_redacts_raw_query_but_keeps_token_and_state_proof() {
    let event = token_event! {
        event_id: "event-7".to_string(),
        correlation_id: "ctx-7".to_string(),
        query: "very private raw query".to_string(),
        query_hash: "hash-7".to_string(),
        query_type: "bugfix_context".to_string(),
        baseline_strategy: "semantic_top_k".to_string(),
        naive_tokens: 900,
        context_tokens: 120,
        recovery_tokens: 0,
        effective_saved_tokens: 780,
        quality_ok: true,
        quality_method: "hybrid_answer_success".to_string(),
        quality_tier: "answer_success_recovered".to_string(),
    };

    let line_item = contractual_line_item_json(&event);
    assert_eq!(line_item["query_hash"], "hash-7");
    assert_eq!(line_item["query_type"], "bugfix_context");
    assert_eq!(line_item["baseline_tokens"], 900);
    assert_eq!(line_item["effective_saved_tokens"], 780);
    assert_eq!(
        line_item["usage_state"]["lifecycle_status"],
        "verified_included"
    );
    assert_eq!(line_item.get("query"), None);
}

#[test]
fn contractual_evidence_pack_carries_hashes_and_scope_previews() {
    let contract = contract_fixture();
    let report = json!({
        "token_budget_report": {
            "contract": report_contract_json(&contract),
            "statement_previews": {
                "lifetime": {
                    "coverage": { "completeness_state": "partially_confirmed" },
                    "measured_non_billable_lower_bound_tokens": 780,
                    "billable_lower_bound_tokens": json!(null)
                }
            },
            "reconciliation_previews": {
                "lifetime": {
                    "reconciliation_state": "awaiting_provider_usage_source"
                }
            },
            "margin_view": {
                "lifetime": {
                    "margin_state": "awaiting_rate_card"
                }
            },
            "contractual_statement_summaries": {
                "lifetime": {
                    "settlement_stage": "measured_review_ready_report_only",
                    "settlement_stage_family": "measured_report_only",
                    "next_settlement_stage_candidate": "billable_blocked",
                    "next_settlement_stage_blockers": ["billing_mode_report_only"],
                    "contractual_readiness_model_version": "contractual-readiness-v1",
                    "transactional_statuses": {
                        "billable": {
                            "status": "billable_blocked_reserved"
                        }
                    },
                    "rate_card_truth_completeness_state": "rate_card_priced_bound",
                    "provider_cost_truth_completeness_state": null,
                    "invoice_evidence_completeness_state": null,
                    "required_sources_for_usage_truth": ["provider_usage_export"],
                    "required_sources_for_cost_truth": ["provider_rate_card", "provider_usage_export"],
                    "optional_sources_for_invoice_evidence": ["provider_invoice_export"],
                    "unready_required_sources_for_usage_truth": [],
                    "unready_required_sources_for_cost_truth": ["provider_rate_card"],
                    "unready_optional_sources_for_invoice_evidence": ["provider_invoice_export"],
                    "infra_cost_truth_completeness_state": "awaiting_infra_cost_profile",
                    "pricing_truth_completeness_state": "awaiting_infra_cost_profile",
                    "customer_savings_money_truth_completeness_state": null,
                    "amai_cost_truth_completeness_state": null,
                    "margin_truth_completeness_state": null,
                    "required_sources_for_margin_truth": ["infra_cost_profile", "provider_rate_card", "provider_usage_export"],
                    "optional_sources_for_margin_invoice_evidence": ["provider_invoice_export"],
                    "unready_required_sources_for_margin_truth": ["infra_cost_profile", "provider_rate_card"],
                    "margin_readiness_state": "awaiting_pricing_truth",
                    "internal_money_arithmetic_readiness_state": "awaiting_pricing_truth",
                    "internal_money_arithmetic_blocking_reasons": ["pricing_truth_not_ready"],
                    "contractual_settlement_readiness_state": "customer_review_ready_settlement_activation_blocked_report_only",
                    "contractual_settlement_blocking_reasons": ["billing_mode_report_only", "money_arithmetic_not_ready"],
                    "suitability": {
                        "surfaces": {
                            "contractual_export": {
                                "usable": true,
                                "state": "export_ready_report_only_provisional"
                            }
                        }
                    }
                }
            },
            "statement_export_previews": {
                "lifetime": {
                    "client_limit_boundary_semantics": {
                        "review_state": "strict_slice_plus_observed_amai_continuity_boundary",
                        "continuity_boundary_rollup": {
                            "observed_tokens": 50329
                        }
                    },
                    "settlement_report_preview": {
                        "model_version": "settlement-report-preview-v12",
                        "settlement_report_id": "preview-hash"
                    },
                    "customer_contractual_boundary": {
                        "model_version": "customer-contractual-boundary-v1",
                        "surface_kind": "customer_review_report_only",
                        "review_surface_state": "customer_review_ready_report_only",
                        "review_surface_blocking_reasons": [],
                        "future_settlement_activation_state": "future_settlement_activation_blocked_report_only",
                        "future_settlement_activation_blocking_reasons": ["billing_mode_report_only", "money_arithmetic_not_ready"]
                    },
                    "settlement_activation_governance": {
                        "model_version": "settlement-activation-governance-v1",
                        "governance_state": "activation_blocked_report_only",
                        "future_settlement_activation_state": "future_settlement_activation_blocked_report_only",
                        "future_settlement_activation_blocking_reasons": ["billing_mode_report_only", "money_arithmetic_not_ready"],
                        "next_settlement_stage_candidate": "billable_blocked",
                        "next_settlement_stage_blockers": ["billing_mode_report_only"],
                        "provisional_close_state": "provisional_close_blocked",
                        "provisional_close_candidate": "provisional_close_blocked",
                        "provisional_close_barriers": ["coverage_not_final"],
                        "billing_close_barriers": ["billing_mode_report_only"],
                        "close_barriers": ["coverage_not_final", "billing_mode_report_only"],
                        "registry_status": "not_configured",
                        "adjustment_status": "not_configured",
                        "correction_action_state": "registry_not_configured",
                        "credit_action_state": "registry_not_configured",
                        "dispute_action_state": "no_open_disputes",
                        "pending_entries_count": 0,
                        "applied_entries_count": 0,
                        "disputed_entries_count": 0,
                        "allowed_future_actions": [],
                        "note": "test"
                    },
                    "adjustment_activation_governance": {
                        "model_version": "adjustment-activation-governance-v1",
                        "governance_state": "registry_not_configured_report_only",
                        "future_adjustment_activation_state": "future_adjustment_registry_not_bound",
                        "future_adjustment_activation_blocking_reasons": ["adjustment_registry_not_bound"],
                        "registry_status": "not_configured",
                        "adjustment_status": "not_configured",
                        "request_schema_version": "adjustment-request-v1",
                        "registry_version": "adjustment-registry-v2",
                        "correction_action_state": "registry_not_configured",
                        "credit_action_state": "registry_not_configured",
                        "dispute_action_state": "no_open_disputes",
                        "pending_entries_count": 0,
                        "applied_entries_count": 0,
                        "disputed_entries_count": 0,
                        "allowed_future_actions": [],
                        "note": "test"
                    }
                }
            },
            "external_truth_manifest": {
                "manifest_hash": "truth-hash"
            }
        }
    });
    let profile = super::ResolvedProfile {
        code: "local_default".to_string(),
        display_name: "Обычная рабочая машина".to_string(),
        description: "test".to_string(),
        session_gap_minutes: 30,
        rolling_window_hours: Some(24),
    };
    let events = vec![
        token_event! {
            event_id: "event-included".to_string(),
            correlation_id: "ctx-1".to_string(),
            query_hash: "hash-a".to_string(),
            naive_tokens: 1000,
            context_tokens: 100,
            effective_saved_tokens: 900,
        },
        token_event! {
            event_id: "event-excluded".to_string(),
            correlation_id: "ctx-2".to_string(),
            query_hash: "hash-b".to_string(),
            quality_ok: false,
            needed_followup: true,
            naive_tokens: 800,
            context_tokens: 200,
            effective_saved_tokens: 600,
        },
    ];

    let pack = build_contractual_evidence_pack(
        &report,
        "lifetime",
        "всё время записи",
        &events,
        &contract,
        &profile,
        false,
        777,
    )
    .expect("evidence pack");

    let payload = &pack["contractual_evidence_pack"];
    assert_eq!(payload["pack_version"], "contractual-evidence-pack-v21");
    assert_eq!(
        payload["settlement_stage"],
        "measured_review_ready_report_only"
    );
    assert_eq!(payload["settlement_stage_family"], "measured_report_only");
    assert_eq!(
        payload["next_settlement_stage_candidate"],
        "billable_blocked"
    );
    assert_eq!(
        payload["next_settlement_stage_blockers"],
        json!(["billing_mode_report_only"])
    );
    assert_eq!(
        payload["transactional_statuses"]["billable"]["status"],
        "billable_blocked_reserved"
    );
    assert_eq!(
        payload["contractual_readiness_model_version"],
        "contractual-readiness-v1"
    );
    assert_eq!(
        payload["customer_contractual_boundary"]["model_version"],
        "customer-contractual-boundary-v1"
    );
    assert_eq!(
        payload["customer_contractual_boundary"]["surface_kind"],
        "customer_evidence_pack_report_only"
    );
    assert_eq!(
        payload["customer_contractual_boundary"]["review_surface_state"],
        "customer_review_ready_report_only"
    );
    assert_eq!(
        payload["customer_contractual_boundary"]["future_settlement_activation_state"],
        "future_settlement_activation_blocked_report_only"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["model_version"],
        "settlement-activation-governance-v1"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["governance_state"],
        "activation_blocked_report_only"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["registry_status"],
        "not_configured"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["future_settlement_activation_state"],
        "future_settlement_activation_blocked_report_only"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["next_settlement_stage_candidate"],
        "billable_blocked"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["billing_close_barriers"],
        json!(["billing_mode_report_only"])
    );
    assert_eq!(
        payload["settlement_activation_governance"]["adjustment_status"],
        "not_configured"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["credit_action_state"],
        "registry_not_configured"
    );
    assert_eq!(
        payload["settlement_activation_governance"]["dispute_action_state"],
        "no_open_disputes"
    );
    assert_eq!(
        payload["adjustment_activation_governance"]["model_version"],
        "adjustment-activation-governance-v1"
    );
    assert_eq!(
        payload["adjustment_activation_governance"]["governance_state"],
        "registry_not_configured_report_only"
    );
    assert_eq!(
        payload["adjustment_activation_governance"]["future_adjustment_activation_state"],
        "future_adjustment_registry_not_bound"
    );
    assert_eq!(
        payload["adjustment_activation_governance"]["future_adjustment_activation_blocking_reasons"],
        json!(["adjustment_registry_not_bound"])
    );
    assert_eq!(
        payload["internal_money_arithmetic_readiness_state"],
        "awaiting_pricing_truth"
    );
    assert_eq!(
        payload["internal_money_arithmetic_blocking_reasons"],
        json!(["pricing_truth_not_ready"])
    );
    assert_eq!(
        payload["contractual_settlement_readiness_state"],
        "customer_review_ready_settlement_activation_blocked_report_only"
    );
    assert_eq!(
        payload["contractual_settlement_blocking_reasons"],
        json!(["billing_mode_report_only", "money_arithmetic_not_ready"])
    );
    assert_eq!(
        payload["rate_card_truth_completeness_state"],
        "rate_card_priced_bound"
    );
    assert_eq!(
        payload["provider_cost_truth_completeness_state"],
        json!(null)
    );
    assert_eq!(payload["invoice_evidence_completeness_state"], json!(null));
    assert_eq!(
        payload["infra_cost_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        payload["pricing_truth_completeness_state"],
        "awaiting_infra_cost_profile"
    );
    assert_eq!(
        payload["customer_savings_money_truth_completeness_state"],
        json!(null)
    );
    assert_eq!(payload["amai_cost_truth_completeness_state"], json!(null));
    assert_eq!(payload["margin_truth_completeness_state"], json!(null));
    assert_eq!(payload["margin_readiness_state"], "awaiting_pricing_truth");
    assert_eq!(
        payload["export_semantics"]["surface_kind"],
        "customer_evidence_pack_report_only"
    );
    assert_eq!(
        payload["export_semantics"]["operational_telemetry_included"],
        false
    );
    assert_eq!(payload["included_events_count"], 1);
    assert_eq!(payload["excluded_events_count"], 1);
    assert_eq!(
        payload["external_truth_manifest"]["manifest_hash"],
        "truth-hash"
    );
    assert_eq!(
        payload["settlement_report_preview"]["model_version"],
        "settlement-report-preview-v12"
    );
    assert_eq!(
        payload["client_limit_boundary_semantics"]["review_state"],
        "strict_slice_plus_observed_amai_continuity_boundary"
    );
    assert_eq!(
        payload["client_limit_boundary_semantics"]["continuity_boundary_rollup"]["observed_tokens"],
        50329
    );
    assert_eq!(
        payload["reviewed_frozen_debt_export_surface"]["model_version"],
        "client-limit-reviewed-frozen-debt-export-surface-v1"
    );
    assert_eq!(
        payload["settlement_report_preview"]["customer_contractual_boundary"]["surface_kind"],
        "customer_settlement_report_preview_report_only"
    );
    assert_eq!(
        payload["required_sources_for_usage_truth"],
        json!(["provider_usage_export"])
    );
    assert_eq!(
        payload["settlement_report_preview"]["settlement_report_id"],
        "preview-hash"
    );
    assert!(
        payload["included_events_hash"]
            .as_str()
            .unwrap_or_default()
            .len()
            > 10
    );
    assert!(
        payload["excluded_events_hash"]
            .as_str()
            .unwrap_or_default()
            .len()
            > 10
    );
    assert_eq!(
        payload["truth_guardrail"]["full_session_economics"],
        "not_fully_measured"
    );
    assert_eq!(
        payload["line_items"]["included"][0]["event_id"],
        "event-included"
    );
    assert_eq!(
        payload["line_items"]["excluded"][0]["usage_state"]["excluded_reason_code"],
        "awaiting_followup_reconciliation"
    );
    assert_eq!(
        payload["suitability"]["surfaces"]["contractual_export"]["state"],
        "export_ready_report_only_provisional"
    );
}

#[test]
fn parse_snapshot_event_accepts_canonical_alias_fields() {
    let row = ObservabilitySnapshotRecord {
        snapshot_id: Uuid::new_v4(),
        snapshot_kind: "token_budget_event".to_string(),
        created_at_epoch_ms: 1234,
        payload: json!({
            "token_budget_event": {
                "event_id": "event-1",
                "source_kind": "live_context_pack",
                "traffic_class": "live",
                "project_code": "art",
                "namespace_code": "continuity",
                "query": "token report",
                "query_hash": "hash",
                "query_type": "code_lookup",
                "target_kind": "file",
                "baseline_hit_target": true,
                "amai_hit_target": true,
                "cold_warm_state": "warm",
                "baseline_strategy": "grep_top_files",
                "tokenizer": "o200k_base",
                "latency_ms": 2.0,
                "baseline_tokens": 1500,
                "delivered_tokens": 400,
                "gross_savings_pct": 73.3333333333,
                "recovery": {
                    "recovery_tokens": 40,
                    "fallback_triggered": false,
                    "fallback_count": 0
                },
                "quality": {
                    "quality_ok": true,
                    "quality_score": 1.0,
                    "quality_method": "hybrid_task_success",
                    "quality_tier": "task_success_recovered",
                    "head_hit_target": true
                },
                "followup": {
                    "needed_followup": false,
                    "followup_count": 1,
                    "followup_of_event_id": "event-0",
                    "resolved_by_event_id": null
                },
                "shape": {
                    "document_hits": 1,
                    "symbol_hits": 0,
                    "file_hits": 1,
                    "sources_count": 2,
                    "chunks_count": 2,
                    "pack_token_count": 400,
                    "deduped_token_count": 400
                },
                "whole_cycle_observed": {
                    "client_prompt_tokens": 90,
                    "assistant_generation_tokens": 45,
                    "tool_overhead_tokens": 10,
                    "continuity_restore_tokens": 5
                },
                "savings": {
                    "saved_tokens": 1100,
                    "effective_saved_tokens": 1060,
                    "savings_factor": 3.75,
                    "effective_savings_percent": 70.6666666667
                }
            }
        }),
    };

    let parsed = parse_snapshot_event(&row)
        .expect("parse should succeed")
        .expect("event should exist");
    assert_eq!(parsed.project, "art");
    assert_eq!(parsed.namespace, "continuity");
    assert_eq!(parsed.naive_tokens, 1500);
    assert_eq!(parsed.context_tokens, 400);
    assert_eq!(parsed.client_prompt_tokens, Some(90));
    assert_eq!(parsed.assistant_generation_tokens, Some(45));
    assert_eq!(parsed.savings_percent, 73.3333333333);
}

#[test]
fn product_headline_prefers_verified_metric_when_available() {
    let headline = build_product_headline(
        &json!({
            "events_total": 12,
            "counted_events": 7,
            "preliminary": false,
            "verified_effective_savings_pct": 28.4,
            "effective_savings_pct": 31.2,
            "verified_effective_saved_tokens": 184220,
            "total_effective_saved_tokens": 200000,
            "quality_ok_rate": 96.1,
            "fallback_rate": 3.8,
            "client_limit_meter_alignment": {
                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                "baseline_equivalence": {
                    "state": "baseline_component_semantics_explicit_boundary"
                },
                "strict_client_meter_slice": {
                    "state": "strict_slice_partial_lower_bound",
                    "lower_bound_tokens": 320
                },
                "explicit_boundary_surface": {
                    "state": "amai_continuity_boundary"
                },
                "continuity_boundary_rollup": {
                    "state": "amai_continuity_boundary_observed",
                    "observed_tokens": 50329
                }
            }
        }),
        "окно Codex 5 часов",
        None,
    );
    assert_eq!(headline["metric_code"], "verified_effective_savings_pct");
    assert_eq!(headline["value_percent"], 28.4);
    assert_eq!(headline["saved_tokens"], 184220);
    assert_eq!(headline["status"], "pass");
    assert_eq!(
        headline["client_limit_boundary_semantics"]["review_state"],
        "strict_slice_plus_observed_amai_continuity_boundary"
    );
    assert_eq!(
        headline["client_limit_boundary_semantics"]["continuity_boundary_rollup"]["observed_tokens"],
        50329
    );
    assert!(
        headline["note"]
            .as_str()
            .unwrap_or_default()
            .contains("headline не равен клиентскому лимиту")
    );
    assert_eq!(headline["client_meter_status_bar_correlated"], json!(false));
}

#[test]
fn product_headline_appends_frozen_gap_review_note_when_required() {
    let headline = build_product_headline(
        &json!({
            "events_total": 12,
            "counted_events": 7,
            "preliminary": false,
            "verified_effective_savings_pct": 28.4,
            "effective_savings_pct": 31.2,
            "verified_effective_saved_tokens": 184220,
            "total_effective_saved_tokens": 200000,
            "quality_ok_rate": 96.1,
            "fallback_rate": 3.8,
            "client_limit_meter_alignment": {
                "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
                "baseline_equivalence": {
                    "state": "baseline_component_semantics_explicit_boundary"
                },
                "strict_client_meter_slice": {
                    "state": "strict_slice_partial_lower_bound",
                    "lower_bound_tokens": 320
                },
                "explicit_boundary_surface": {
                    "state": "amai_continuity_boundary"
                },
                "continuity_boundary_rollup": {
                    "state": "amai_continuity_boundary_observed",
                    "observed_tokens": 50329
                },
                "frozen_gap_review_surface": {
                    "state": "review_required"
                }
            }
        }),
        "окно Codex 5 часов",
        None,
    );
    assert!(
        headline["note"]
            .as_str()
            .unwrap_or_default()
            .contains("frozen-gap review")
    );
}

#[test]
fn product_headline_prefers_same_meter_exact_pair_and_marks_below_target() {
    let headline = build_product_headline(
        &json!({
            "events_total": 12,
            "counted_events": 7,
            "preliminary": false,
            "verified_effective_savings_pct": 97.4,
            "effective_savings_pct": 97.8,
            "verified_effective_saved_tokens": 184220,
            "total_effective_saved_tokens": 200000,
            "quality_ok_rate": 96.1,
            "fallback_rate": 3.8,
            "verified_baseline_tokens": 1000,
            "verified_observed_whole_cycle_with_amai_tokens": 250,
            "client_limit_meter_alignment": {
                "same_meter_as_client_limit": true,
                "strict_client_meter_slice": {
                    "lower_bound_tokens": 1000
                },
                "baseline_equivalence": {
                    "measured_baseline_tokens_lower_bound": 1000
                }
            }
        }),
        "окно Codex 5 часов",
        None,
    );
    assert_eq!(headline["metric_code"], "verified_effective_savings_pct");
    assert_eq!(headline["value_percent"], 75.0);
    assert_eq!(headline["saved_tokens"], 750);
    assert_eq!(headline["status"], "alert");
    assert_eq!(headline["client_meter_status_bar_correlated"], json!(true));
    assert_eq!(headline["client_meter_target_percent"], json!(90.0));
    assert_eq!(headline["client_meter_target_met"], json!(false));
    assert!(
        headline["note"]
            .as_str()
            .unwrap_or_default()
            .contains("VS Code status bar")
    );
    assert!(
        headline["note"]
            .as_str()
            .unwrap_or_default()
            .contains("ниже целевой планки 90%")
    );
}

#[test]
fn product_headline_respects_custom_target_percent() {
    let headline = super::build_product_headline_with_target(
        &json!({
            "events_total": 12,
            "counted_events": 7,
            "preliminary": false,
            "verified_effective_savings_pct": 97.4,
            "effective_savings_pct": 97.8,
            "verified_effective_saved_tokens": 184220,
            "total_effective_saved_tokens": 200000,
            "quality_ok_rate": 96.1,
            "fallback_rate": 3.8,
            "verified_baseline_tokens": 1000,
            "verified_observed_whole_cycle_with_amai_tokens": 250,
            "client_limit_meter_alignment": {
                "same_meter_as_client_limit": true,
                "strict_client_meter_slice": {
                    "lower_bound_tokens": 1000
                },
                "baseline_equivalence": {
                    "measured_baseline_tokens_lower_bound": 1000
                }
            }
        }),
        "окно Codex 5 часов",
        None,
        50,
    );
    assert_eq!(headline["status"], "pass");
    assert_eq!(headline["client_meter_target_percent"], json!(50.0));
    assert_eq!(headline["client_meter_target_met"], json!(true));
    assert!(
        !headline["note"]
            .as_str()
            .unwrap_or_default()
            .contains("ниже целевой планки 90%")
    );
}

#[test]
fn product_headline_zero_target_does_not_force_below_target_alert() {
    let headline = super::build_product_headline_with_target(
        &json!({
            "events_total": 12,
            "counted_events": 7,
            "preliminary": false,
            "verified_effective_savings_pct": 97.4,
            "effective_savings_pct": 97.8,
            "verified_effective_saved_tokens": 184220,
            "total_effective_saved_tokens": 200000,
            "quality_ok_rate": 96.1,
            "fallback_rate": 3.8,
            "verified_baseline_tokens": 1000,
            "verified_observed_whole_cycle_with_amai_tokens": 250,
            "client_limit_meter_alignment": {
                "same_meter_as_client_limit": true,
                "strict_client_meter_slice": {
                    "lower_bound_tokens": 1000
                },
                "baseline_equivalence": {
                    "measured_baseline_tokens_lower_bound": 1000
                }
            }
        }),
        "окно Codex 5 часов",
        None,
        0,
    );
    assert_eq!(headline["status"], "pass");
    assert_eq!(headline["client_meter_target_percent"], json!(0.0));
    assert_eq!(headline["client_meter_target_met"], json!(true));
}

#[test]
fn client_limit_boundary_review_surface_appends_frozen_gap_note_when_required() {
    let surface = super::build_client_limit_boundary_review_surface(&json!({
        "client_limit_meter_alignment": {
            "same_meter_as_client_limit": false,
            "alignment_state": "whole_cycle_observed_explicit_boundary_not_meter_equivalent",
            "baseline_equivalence": {
                "state": "baseline_component_semantics_explicit_boundary"
            },
            "strict_client_meter_slice": {
                "state": "strict_slice_partial_lower_bound"
            },
            "explicit_boundary_surface": {
                "state": "amai_continuity_boundary"
            },
            "continuity_boundary_rollup": {
                "state": "amai_continuity_boundary_observed"
            },
            "pre_amai_baseline_source_status": {
                "state": "materialized"
            },
            "exact_pair_status": {
                "state": "exact_pair_blocked"
            },
            "frozen_gap_review_surface": {
                "state": "review_required"
            }
        }
    }));
    assert!(
        surface["note"]
            .as_str()
            .unwrap_or_default()
            .contains("raw exact history")
    );
}

#[test]
fn reviewed_frozen_debt_export_surface_is_report_only_when_review_required() {
    let contract = contract_fixture();
    let surface = super::build_reviewed_frozen_debt_export_surface(
        &contract,
        &json!({
            "same_meter_as_client_limit": false,
            "exact_pair_status": {
                "state": "exact_pair_blocked",
                "exact_pair_available": false
            },
            "frozen_gap_review_surface": {
                "state": "review_required",
                "review_required": true,
                "blocking_component": "tool_overhead_outside_retrieval",
                "missing_live_events": 13,
                "irrecoverable_missing_live_events": 13,
                "recoverable_missing_live_events": 0,
                "resolution_condition": "freeze_irrecoverable_gap_or_keep_exact_pair_unavailable"
            }
        }),
        Some("lifetime"),
    );
    assert_eq!(
        surface["state"],
        "reviewed_frozen_debt_export_ready_report_only"
    );
    assert_eq!(surface["surface_kind"], "reviewed_frozen_debt_report_only");
    assert_eq!(
        surface["forbidden_claims"],
        json!([
            "claim_raw_exact_history",
            "claim_exact_same_meter_pair_materialized"
        ])
    );
    assert_eq!(
        surface["review_bundle_command"],
        "./scripts/amai_exec.sh observe token-statement-export --scope lifetime"
    );
    assert_eq!(
        surface["evidence_pack_command"],
        "./scripts/amai_exec.sh observe token-evidence-pack --scope lifetime"
    );
}

#[test]
fn dashboard_statement_export_preview_surfaces_reviewed_frozen_debt_export() {
    let contract = contract_fixture();
    let preview = super::build_dashboard_statement_export_preview(
        &json!({
            "scope_code": "lifetime",
            "client_limit_meter_alignment": {
                "same_meter_as_client_limit": false,
                "exact_pair_status": {
                    "state": "exact_pair_blocked",
                    "exact_pair_available": false
                },
                "frozen_gap_review_surface": {
                    "state": "review_required",
                    "review_required": true,
                    "blocking_component": "tool_overhead_outside_retrieval",
                    "missing_live_events": 13,
                    "irrecoverable_missing_live_events": 13,
                    "recoverable_missing_live_events": 0,
                    "resolution_condition": "freeze_irrecoverable_gap_or_keep_exact_pair_unavailable"
                }
            }
        }),
        &contract,
    );
    assert_eq!(
        preview["reviewed_frozen_debt_export_surface"]["state"],
        "reviewed_frozen_debt_export_ready_report_only"
    );
    assert_eq!(
        preview["reviewed_frozen_debt_export_surface"]["review_bundle_command"],
        "./scripts/amai_exec.sh observe token-statement-export --scope lifetime"
    );
}

#[test]
fn product_headline_falls_back_to_preliminary_effective_metric() {
    let headline = build_product_headline(
        &json!({
            "events_total": 10,
            "counted_events": 0,
            "legacy_unverified_events": 3,
            "preliminary": true,
            "verified_effective_savings_pct": 0.0,
            "effective_savings_pct": 44.0,
            "verified_effective_saved_tokens": 0,
            "total_effective_saved_tokens": 1200,
            "quality_ok_rate": 0.0,
            "fallback_rate": 0.0
        }),
        "окно Codex 5 часов",
        None,
    );
    assert_eq!(headline["metric_code"], "effective_savings_pct_preliminary");
    assert_eq!(headline["value_percent"], 44.0);
    assert_eq!(headline["saved_tokens"], 1200);
    assert_eq!(headline["status"], "alert");
    assert!(
        headline["note"]
            .as_str()
            .unwrap_or_default()
            .contains("старым форматом")
    );
}

#[test]
fn agent_cycle_economics_exposes_partial_lower_bound_and_timelines() {
    let measurement = MeasurementConfig {
        tokenizer: "o200k_base".to_string(),
        naive_limit_files: 5,
        naive_max_bytes_per_file: 16384,
        include_verify_events_by_default: false,
        metering_ingest_warning_seconds: 60,
        metering_ingest_slo_seconds: 300,
        late_arrival_grace_minutes: 60,
        preliminary_min_events: 50,
        preliminary_min_baseline_tokens: 100_000,
    };
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            correlation_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "first".to_string(),
            query_hash: "hash-1".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 11.0,
            saved_tokens: 60,
            naive_tokens: 100,
            context_tokens: 40,
            recovery_tokens: 0,
            effective_saved_tokens: 60,
            savings_factor: 2.5,
            savings_percent: 60.0,
            effective_savings_percent: 60.0,
            quality_ok: true,
            quality_score: 1.0,
            quality_method: "hybrid_answer_proxy".to_string(),
            quality_tier: "answer_proxy".to_string(),
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
            pack_token_count: 40,
            deduped_token_count: 40,
            client_prompt_tokens: Some(30),
            assistant_generation_tokens: Some(20),
            tool_overhead_tokens: Some(10),
            continuity_restore_tokens: Some(5),
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            correlation_id: "event-2".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            query: "second".to_string(),
            query_hash: "hash-2".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: false,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 5.0,
            saved_tokens: 40,
            naive_tokens: 100,
            context_tokens: 60,
            recovery_tokens: 25,
            effective_saved_tokens: 15,
            savings_factor: 1.67,
            savings_percent: 40.0,
            effective_savings_percent: 15.0,
            quality_ok: false,
            quality_score: 0.4,
            quality_method: "hybrid_partial_retrieval".to_string(),
            quality_tier: "partial".to_string(),
            head_hit_target: false,
            needed_followup: true,
            followup_count: 1,
            followup_of_event_id: Some("event-1".to_string()),
            resolved_by_event_id: None,
            fallback_triggered: true,
            fallback_count: 1,
            document_hits: 0,
            symbol_hits_count: 0,
            file_hits: 1,
            sources_count: 1,
            chunks_count: 1,
            pack_token_count: 60,
            deduped_token_count: 60,
            client_prompt_tokens: Some(15),
            assistant_generation_tokens: Some(12),
            tool_overhead_tokens: Some(4),
            continuity_restore_tokens: Some(3),
        },
    ];

    let economics = super::build_agent_cycle_economics(
        &measurement,
        &contract_fixture(),
        25,
        &events,
        Some(&events),
        &events,
        "Обычная рабочая машина",
        &[],
        &super::AssistantGenerationScopeObservation::default(),
        None,
        None,
    );

    assert_eq!(economics["model_version"], "agent-cycle-lower-bound-v4");
    assert_eq!(economics["status"], "partial_lower_bound");
    assert_eq!(
        economics["current_session"]["without_amai_measured_tokens"],
        200
    );
    assert_eq!(
        economics["current_session"]["with_amai_measured_tokens"],
        125
    );
    assert_eq!(economics["current_session"]["measured_saved_tokens"], 75);
    assert_eq!(
        economics["current_session"]["verified_with_amai_measured_tokens"],
        40
    );
    assert_eq!(
        economics["current_session"]["verified_measured_saved_tokens"],
        60
    );
    assert_eq!(
        economics["current_session"]["all_live_timeline"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        economics["current_session"]["verified_live_timeline"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        economics["contract"]["reporting_layers"]["billable"]["status"],
        "disabled_report_only"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["model_version"],
        "client-limit-meter-alignment-v12"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["baseline_equivalence_model_version"],
        "client-limit-baseline-equivalence-v4"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["strict_meter_slice_model_version"],
        "client-limit-strict-meter-slice-v1"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["explicit_boundary_surface_model_version"],
        "client-limit-explicit-boundary-surface-v2"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["continuity_boundary_rollup_model_version"],
        "client-limit-continuity-boundary-rollup-v2"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["pre_amai_baseline_source_model_version"],
        "client-limit-pre-amai-baseline-source-v2"
    );
    assert_eq!(
        economics["contract"]["client_limit_meter_alignment"]["frozen_gap_review_surface_model_version"],
        "client-limit-frozen-gap-review-surface-v1"
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["strict_client_meter_slice"]["lower_bound_tokens"],
        45
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["strict_client_meter_slice"]["components"],
        json!(["client_prompt", "tool_overhead_outside_retrieval"])
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["surface_kind"],
        "agent_cycle_scope"
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["alignment_state"],
        "whole_cycle_observed_baseline_partial"
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["partially_measured_components"],
        json!([])
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]["state"],
        "baseline_component_semantics_partial"
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]["measured_baseline_components"],
        json!(["client_prompt", "tool_overhead_outside_retrieval"])
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]["missing_baseline_components"],
        json!(["assistant_generation"])
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["explicit_boundary_surface"]["state"],
        "no_explicit_boundary"
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]["measured_baseline_tokens_lower_bound"],
        45
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["live_events_count"],
        2
    );
    assert_eq!(
        economics["current_session"]["client_limit_meter_alignment"]["non_live_events_count"],
        0
    );
    assert_eq!(
        economics["current_session"]["observed_whole_cycle_with_amai_tokens"],
        224
    );
    assert_eq!(
        economics["current_session"]["verified_observed_whole_cycle_with_amai_tokens"],
        105
    );
    assert_eq!(
        economics["current_session"]["coverage"]["completeness_state"],
        "partially_confirmed"
    );
}

#[test]
fn client_limit_meter_alignment_marks_partial_whole_cycle_observation() {
    let summary = json!({
        "events_total": 2,
        "live_events_count": 2,
        "non_live_events_count": 0,
        "counted_events": 2,
        "observed_client_prompt_tokens": 30,
        "observed_assistant_generation_tokens": 0,
        "observed_tool_overhead_tokens": 0,
        "observed_continuity_restore_tokens": 0,
        "observed_client_prompt_live_events": 1,
        "observed_assistant_generation_live_events": 0,
        "observed_tool_overhead_live_events": 0,
        "observed_continuity_restore_live_events": 0
    });

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        None,
        None,
        None,
    );

    assert_eq!(
        alignment["alignment_state"],
        "whole_cycle_partially_observed_not_meter_equivalent"
    );
    assert_eq!(
        alignment["partially_measured_components"],
        json!(["client_prompt"])
    );
    assert!(
        alignment["blocking_reasons"]
            .as_array()
            .is_some_and(|reasons| {
                reasons
                    .iter()
                    .any(|reason| reason == "client_prompt_partially_measured")
            })
    );
    assert_eq!(
        alignment["baseline_equivalence"]["state"],
        "whole_cycle_components_incomplete"
    );
}

#[test]
fn client_limit_meter_alignment_marks_explicit_baseline_boundary_for_continuity_restore() {
    let summary = json!({
        "events_total": 1,
        "live_events_count": 1,
        "non_live_events_count": 0,
        "counted_events": 1,
        "observed_client_prompt_tokens": 30,
        "observed_assistant_generation_tokens": 0,
        "observed_tool_overhead_tokens": 0,
        "observed_continuity_restore_tokens": 7,
        "observed_client_prompt_live_events": 1,
        "observed_assistant_generation_live_events": 0,
        "observed_tool_overhead_live_events": 0,
        "observed_continuity_restore_live_events": 1
    });
    let events = vec![token_event! {
        event_id: "event-1".to_string(),
        correlation_id: "event-1".to_string(),
        measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
        query_type: "continuity_restore".to_string(),
        target_kind: "continuity_restore".to_string(),
        traffic_class: "live".to_string(),
        client_prompt_tokens: Some(30),
        continuity_restore_tokens: Some(7),
    }];

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        Some(&events),
        None,
        None,
    );

    assert_eq!(
        alignment["baseline_equivalence"]["state"],
        "baseline_component_semantics_explicit_boundary"
    );
    assert_eq!(
        alignment["alignment_state"],
        "whole_cycle_observed_explicit_boundary_not_meter_equivalent"
    );
    assert_eq!(
        alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
        json!(["continuity_restore_outside_retrieval"])
    );
    assert_eq!(
        alignment["baseline_equivalence"]["same_meter_baseline_measured"],
        json!(false)
    );
    assert_eq!(
        alignment["baseline_equivalence"]["missing_baseline_components"],
        json!([])
    );
    assert_eq!(
        alignment["explicit_boundary_surface"]["state"],
        "amai_continuity_boundary"
    );
    assert_eq!(
        alignment["explicit_boundary_surface"]["components"],
        json!(["continuity_restore_outside_retrieval"])
    );
    assert_eq!(
        alignment["explicit_boundary_surface"]["resolution_state"],
        "truthful_pre_amai_baseline_required"
    );
    assert_eq!(
        alignment["explicit_boundary_surface"]["guessed_baseline_prohibited"],
        json!(true)
    );
    assert_eq!(
        alignment["explicit_boundary_surface"]["equivalence_resume_condition"],
        "truthful_pre_amai_baseline_source"
    );
    assert_eq!(
        alignment["continuity_boundary_rollup"]["state"],
        "amai_continuity_boundary_observed"
    );
    assert_eq!(
        alignment["continuity_boundary_rollup"]["observed_tokens"],
        7
    );
    assert_eq!(
        alignment["continuity_boundary_rollup"]["resolution_state"],
        "observed_boundary_only_not_client_meter"
    );
    assert_eq!(
        alignment["continuity_boundary_rollup"]["guessed_baseline_prohibited"],
        json!(true)
    );
    assert_eq!(
        alignment["continuity_boundary_rollup"]["equivalence_resume_condition"],
        "truthful_pre_amai_baseline_source"
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["state"],
        "required_not_materialized"
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["required"],
        json!(true)
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["source_family"],
        "truthful_pre_amai_baseline_source"
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["blocking_reason"],
        "missing_truthful_pre_amai_baseline_source"
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["same_meter_resume_possible"],
        json!(false)
    );
    assert_eq!(
        alignment["exact_pair_status"]["state"],
        "exact_pair_blocked"
    );
    assert_eq!(alignment["exact_pair_status"]["blocking_reason_count"], 1);
    assert_eq!(
        alignment["exact_pair_status"]["primary_blocking_reason"],
        "missing_truthful_pre_amai_baseline_source"
    );
    assert_eq!(
        alignment["exact_pair_status"]["blockers"][0]["code"],
        "continuity_restore_outside_retrieval"
    );
    assert_eq!(
        alignment["exact_pair_status"]["blockers"][0]["blocker_kind"],
        "explicit_truth_boundary"
    );
    assert!(
        alignment["blocking_reasons"]
            .as_array()
            .is_some_and(|reasons| {
                reasons
                    .iter()
                    .any(|reason| reason == "same_meter_baseline_explicit_boundary")
            })
    );
}

#[test]
fn client_limit_meter_alignment_materializes_same_meter_when_continuity_baseline_exists() {
    let summary = json!({
        "events_total": 1,
        "live_events_count": 1,
        "non_live_events_count": 0,
        "counted_events": 1,
        "observed_client_prompt_tokens": 30,
        "observed_assistant_generation_tokens": 0,
        "observed_tool_overhead_tokens": 0,
        "observed_continuity_restore_tokens": 7,
        "observed_client_prompt_live_events": 1,
        "observed_assistant_generation_live_events": 0,
        "observed_tool_overhead_live_events": 0,
        "observed_continuity_restore_live_events": 1
    });
    let events = vec![token_event! {
        event_id: "event-continuity-exact".to_string(),
        correlation_id: "event-continuity-exact".to_string(),
        measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
        query_type: "continuity_restore".to_string(),
        target_kind: "continuity_restore".to_string(),
        traffic_class: "live".to_string(),
        baseline_strategy: super::CONTINUITY_PRE_AMAI_BASELINE_STRATEGY.to_string(),
        naive_tokens: 12,
        client_prompt_tokens: Some(30),
        continuity_restore_tokens: Some(7),
        pre_amai_baseline_source: Some(json!({
            "state": "materialized",
            "source_kind": super::CONTINUITY_PRE_AMAI_BASELINE_STRATEGY,
            "source_family": "truthful_pre_amai_baseline_source",
        })),
    }];

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        Some(&events),
        None,
        None,
    );

    assert_eq!(
        alignment["baseline_equivalence"]["state"],
        "baseline_semantics_materialized"
    );
    assert_eq!(alignment["alignment_state"], "same_meter_equivalent");
    assert_eq!(alignment["same_meter_as_client_limit"], json!(true));
    assert_eq!(
        alignment["baseline_equivalence"]["measured_baseline_tokens_lower_bound"],
        42
    );
    assert_eq!(
        alignment["baseline_equivalence"]["measured_baseline_components"],
        json!(["client_prompt", "continuity_restore_outside_retrieval"])
    );
    assert_eq!(
        alignment["baseline_equivalence"]["explicitly_unmodeled_baseline_components"],
        json!([])
    );
    assert_eq!(
        alignment["explicit_boundary_surface"]["state"],
        "no_explicit_boundary"
    );
    assert_eq!(
        alignment["continuity_boundary_rollup"]["state"],
        "no_amai_continuity_boundary"
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["state"],
        "materialized"
    );
    assert_eq!(
        alignment["pre_amai_baseline_source_status"]["same_meter_resume_possible"],
        json!(true)
    );
    assert_eq!(
        alignment["exact_pair_status"]["state"],
        "exact_pair_materialized"
    );
    assert_eq!(
        alignment["exact_pair_status"]["exact_pair_available"],
        json!(true)
    );
    assert_eq!(alignment["blocking_reasons"], json!([]));
}

#[test]
fn client_limit_meter_alignment_materializes_same_meter_without_quality_gate_when_strict_slice_is_complete()
 {
    let summary = json!({
        "events_total": 1,
        "live_events_count": 1,
        "non_live_events_count": 0,
        "counted_events": 0,
        "observed_client_prompt_tokens": 13,
        "observed_assistant_generation_tokens": 0,
        "observed_tool_overhead_tokens": 222,
        "observed_continuity_restore_tokens": 0,
        "observed_client_prompt_live_events": 1,
        "observed_assistant_generation_live_events": 0,
        "observed_tool_overhead_live_events": 1,
        "observed_continuity_restore_live_events": 0
    });
    let events = vec![token_event! {
        event_id: "event-live-meter-slice".to_string(),
        correlation_id: "event-live-meter-slice".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        traffic_class: "live".to_string(),
        quality_ok: false,
        client_prompt_tokens: Some(13),
        tool_overhead_tokens: Some(222),
    }];
    let assistant_scope = super::AssistantGenerationScopeObservation::default();

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        Some(&events),
        None,
        Some(&assistant_scope),
    );

    assert_eq!(
        alignment["baseline_equivalence"]["state"],
        "baseline_semantics_materialized_without_quality_gate"
    );
    assert_eq!(
        alignment["baseline_equivalence"]["retrieval_quality_gate_confirmed"],
        json!(false)
    );
    assert_eq!(
        alignment["strict_client_meter_slice"]["state"],
        "strict_slice_covers_all_applicable_components"
    );
    assert_eq!(alignment["blocking_reasons"], json!([]));
    assert_eq!(alignment["same_meter_as_client_limit"], json!(true));
    assert_eq!(alignment["alignment_state"], "same_meter_equivalent");
    assert_eq!(
        alignment["exact_pair_status"]["state"],
        "exact_pair_materialized"
    );
}

#[test]
fn client_limit_meter_alignment_keeps_tool_overhead_target_scoped() {
    let summary = json!({
        "events_total": 2,
        "live_events_count": 2,
        "non_live_events_count": 0,
        "counted_events": 2,
        "observed_client_prompt_tokens": 30,
        "observed_assistant_generation_tokens": 0,
        "observed_tool_overhead_tokens": 55,
        "observed_continuity_restore_tokens": 7,
        "observed_client_prompt_live_events": 2,
        "observed_assistant_generation_live_events": 0,
        "observed_tool_overhead_live_events": 2,
        "observed_continuity_restore_live_events": 1
    });
    let events = vec![
        token_event! {
            event_id: "event-retrieval-tool-target".to_string(),
            context_pack_id: Some("cp-tool-target".to_string()),
            correlation_id: "cp-tool-target".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            traffic_class: "live".to_string(),
            client_prompt_tokens: Some(10),
            tool_overhead_tokens: Some(33),
        },
        token_event! {
            event_id: "event-continuity-extra-tool".to_string(),
            correlation_id: "event-continuity-extra-tool".to_string(),
            measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
            query_type: "continuity_restore".to_string(),
            target_kind: "continuity_restore".to_string(),
            traffic_class: "live".to_string(),
            baseline_strategy: super::CONTINUITY_PRE_AMAI_BASELINE_STRATEGY.to_string(),
            naive_tokens: 12,
            client_prompt_tokens: Some(20),
            continuity_restore_tokens: Some(7),
            tool_overhead_tokens: Some(22),
            pre_amai_baseline_source: Some(json!({
                "state": "materialized",
                "source_kind": super::CONTINUITY_PRE_AMAI_BASELINE_STRATEGY,
                "source_family": "truthful_pre_amai_baseline_source",
            })),
        },
    ];
    let assistant_scope = super::AssistantGenerationScopeObservation::default();

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        Some(&events),
        None,
        Some(&assistant_scope),
    );

    let tool_coverage = alignment["component_event_coverage"]
        .as_array()
        .expect("component coverage")
        .iter()
        .find(|item| item["code"].as_str() == Some("tool_overhead_outside_retrieval"))
        .expect("tool overhead coverage");
    assert_eq!(tool_coverage["observed_live_events"], json!(1));
    assert_eq!(tool_coverage["target_live_events_count"], json!(1));
    assert_eq!(tool_coverage["observed_tokens"], json!(33));

    let tool_semantics = alignment["baseline_equivalence"]["component_semantics"]
        .as_array()
        .expect("component semantics")
        .iter()
        .find(|item| item["code"].as_str() == Some("tool_overhead_outside_retrieval"))
        .expect("tool overhead semantics");
    assert_eq!(tool_semantics["whole_cycle_observed_complete"], json!(true));
    assert_eq!(tool_semantics["observed_live_events"], json!(1));
    assert_eq!(tool_semantics["observed_tokens"], json!(33));

    assert_eq!(alignment["blocking_reasons"], json!([]));
    assert_eq!(alignment["same_meter_as_client_limit"], json!(true));
    assert_eq!(
        alignment["exact_pair_status"]["state"],
        "exact_pair_materialized"
    );
}

#[test]
fn client_limit_exact_pair_status_keeps_only_tool_gap_after_assistant_passthrough() {
    let summary = json!({
        "events_total": 2,
        "live_events_count": 2,
        "non_live_events_count": 0,
        "counted_events": 2,
        "observed_client_prompt_tokens": 30,
        "observed_assistant_generation_tokens": 11,
        "observed_tool_overhead_tokens": 7,
        "observed_continuity_restore_tokens": 0,
        "observed_client_prompt_live_events": 2,
        "observed_assistant_generation_live_events": 1,
        "observed_tool_overhead_live_events": 1,
        "observed_continuity_restore_live_events": 0
    });
    let events = vec![
        token_event! {
            event_id: "event-retrieval-1".to_string(),
            context_pack_id: Some("cp-1".to_string()),
            correlation_id: "cp-1".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            traffic_class: "live".to_string(),
            client_prompt_tokens: Some(10),
            tool_overhead_tokens: Some(7),
        },
        token_event! {
            event_id: "event-retrieval-2".to_string(),
            context_pack_id: Some("cp-2".to_string()),
            correlation_id: "cp-2".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            traffic_class: "live".to_string(),
            client_prompt_tokens: Some(20),
            tool_overhead_tokens: None,
            tool_overhead_source: Some(json!({
                "state": "context_pack_payload_missing_irrecoverable",
                "source_kind": "ami.context_packs",
            })),
        },
    ];
    let assistant_scope = super::AssistantGenerationScopeObservation {
        target_group_count: 1,
        observed_group_count: 1,
        observed_tokens: 11,
        helper_only_context_pack_ids: std::collections::BTreeSet::new(),
        helper_only_non_model_visible_context_pack_ids: std::collections::BTreeSet::new(),
        target_context_pack_ids: ["cp-1".to_string()].into_iter().collect(),
        matched_context_pack_ids: ["cp-1".to_string()].into_iter().collect(),
        unmatched_context_pack_ids: std::collections::BTreeSet::new(),
        matched_turn_ids: ["thread-1:turn-1".to_string()].into_iter().collect(),
        available_turns: 1,
        available_direct_turns: 1,
        available_rollout_turns: 0,
        matched_direct_turn_ids: ["thread-1:turn-1".to_string()].into_iter().collect(),
        matched_rollout_turn_ids: std::collections::BTreeSet::new(),
    };

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        Some(&events),
        None,
        Some(&assistant_scope),
    );

    assert_eq!(alignment["same_meter_as_client_limit"], json!(false));
    assert_eq!(
        alignment["baseline_equivalence"]["component_semantics"][1]["code"],
        "assistant_generation"
    );
    assert_eq!(
        alignment["baseline_equivalence"]["component_semantics"][1]["baseline_semantics_state"],
        "observed_tokens_passthrough"
    );
    assert_eq!(
        alignment["baseline_equivalence"]["component_semantics"][1]["baseline_measured_tokens"],
        11
    );
    assert_eq!(
        alignment["exact_pair_status"]["state"],
        "exact_pair_blocked"
    );
    let blockers = alignment["exact_pair_status"]["blockers"]
        .as_array()
        .expect("exact pair blockers");
    assert!(
        !blockers
            .iter()
            .any(|blocker| blocker["code"].as_str() == Some("assistant_generation"))
    );
    let tool_blocker = blockers
        .iter()
        .find(|blocker| blocker["code"].as_str() == Some("tool_overhead_outside_retrieval"))
        .expect("tool overhead blocker");
    assert_eq!(
        alignment["exact_pair_status"]["primary_blocking_reason"],
        "tool_overhead_outside_retrieval_irrecoverable_debt"
    );
    assert_eq!(tool_blocker["code"], "tool_overhead_outside_retrieval");
    assert_eq!(tool_blocker["blocker_kind"], "whole_cycle_observation_gap");
    assert_eq!(
        tool_blocker["recoverability_state"],
        "source_loss_irrecoverable"
    );
    assert_eq!(tool_blocker["frozen_gap_candidate"], true);
    assert_eq!(
        tool_blocker["resolution_condition"],
        "freeze_irrecoverable_gap_or_keep_exact_pair_unavailable"
    );
    assert_eq!(
        alignment["frozen_gap_review_surface"]["state"],
        "review_required"
    );
    assert_eq!(
        alignment["frozen_gap_review_surface"]["review_required"],
        json!(true)
    );
    assert_eq!(
        alignment["frozen_gap_review_surface"]["blocking_component"],
        "tool_overhead_outside_retrieval"
    );
    assert_eq!(
        alignment["frozen_gap_review_surface"]["forbidden_paths"],
        json!(["claim_raw_exact_history"])
    );
}

#[test]
fn assistant_generation_baseline_stays_unmaterialized_without_turn_scope() {
    let summary = json!({
        "events_total": 1,
        "live_events_count": 1,
        "non_live_events_count": 0,
        "counted_events": 1,
        "observed_client_prompt_tokens": 10,
        "observed_assistant_generation_tokens": 17,
        "observed_tool_overhead_tokens": 0,
        "observed_continuity_restore_tokens": 0,
        "observed_client_prompt_live_events": 1,
        "observed_assistant_generation_live_events": 1,
        "observed_tool_overhead_live_events": 1,
        "observed_continuity_restore_live_events": 0
    });
    let events = vec![token_event! {
        event_id: "event-retrieval-ungrouped".to_string(),
        context_pack_id: Some("cp-ungrouped".to_string()),
        correlation_id: "cp-ungrouped".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        traffic_class: "live".to_string(),
        client_prompt_tokens: Some(10),
        assistant_generation_tokens: Some(17),
        tool_overhead_tokens: Some(0),
    }];

    let alignment = super::build_client_limit_meter_alignment(
        &contract_fixture(),
        "statement_preview",
        &summary,
        Some(&events),
        None,
        None,
    );

    assert_eq!(
        alignment["baseline_equivalence"]["component_semantics"][1]["code"],
        "assistant_generation"
    );
    assert_eq!(
        alignment["baseline_equivalence"]["component_semantics"][1]["baseline_semantics_state"],
        "baseline_semantics_unmaterialized"
    );
    assert_eq!(
        alignment["baseline_equivalence"]["component_semantics"][1]["baseline_measured_tokens"],
        json!(null)
    );
    assert_eq!(
        alignment["exact_pair_status"]["blockers"][0]["code"],
        "assistant_generation"
    );
    assert_eq!(
        alignment["exact_pair_status"]["blockers"][0]["blocking_reason"],
        "assistant_generation_baseline_semantics_unmaterialized"
    );
}

#[test]
fn assistant_generation_source_status_reports_direct_turn_attach_scope() {
    let status = super::assistant_generation_observation_source_status(
        None,
        None,
        Some(&super::AssistantGenerationScopeObservation {
            target_group_count: 2,
            observed_group_count: 1,
            observed_tokens: 123,
            helper_only_context_pack_ids: std::collections::BTreeSet::new(),
            helper_only_non_model_visible_context_pack_ids: std::collections::BTreeSet::new(),
            target_context_pack_ids: ["cp-1".to_string(), "cp-2".to_string()]
                .into_iter()
                .collect(),
            matched_context_pack_ids: ["cp-1".to_string()].into_iter().collect(),
            unmatched_context_pack_ids: ["cp-2".to_string()].into_iter().collect(),
            matched_turn_ids: ["thread-1:turn-1".to_string()].into_iter().collect(),
            available_turns: 1,
            available_direct_turns: 1,
            available_rollout_turns: 0,
            matched_direct_turn_ids: ["thread-1:turn-1".to_string()].into_iter().collect(),
            matched_rollout_turn_ids: std::collections::BTreeSet::new(),
        }),
    );

    assert_eq!(status["source_kind"], "direct_turn_attach_v1");
    assert_eq!(
        status["state"],
        "assistant_generation_source_partial_scope_overlap"
    );
    assert_eq!(status["usable_direct_turns"], 1);
    assert_eq!(status["usable_rollout_turns"], 0);
}

#[test]
fn current_live_turn_assistant_scope_from_client_meter_matches_current_turn_window() {
    let events = vec![token_event! {
        event_id: "event-live-turn".to_string(),
        context_pack_id: Some("cp-live".to_string()),
        correlation_id: "cp-live".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        traffic_class: "live".to_string(),
        context_tokens: 40,
        client_prompt_tokens: Some(12),
        assistant_generation_tokens: None,
        tool_overhead_tokens: Some(9),
    }];
    let matched_context_pack_ids = ["cp-live".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let observation = crate::codex_threads::RolloutClientMeterObservation {
        thread_id: "thread-1".to_string(),
        rollout_path: "/tmp/current-live-turn.jsonl".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_epoch_ms: 100,
        ended_at_epoch_ms: 200,
        first_assistant_response_at_epoch_ms: None,
        client_turn_total_tokens: 94,
        client_turn_input_tokens: 12,
        client_turn_cached_input_tokens: 0,
        client_turn_output_tokens: 33,
        client_turn_reasoning_output_tokens: 0,
        latest_cumulative_total_tokens: 94,
        latest_model_context_window: 258400,
        latest_primary_limit_used_percent: 10,
        latest_secondary_limit_used_percent: 20,
        latest_primary_window_duration_mins: None,
        latest_primary_resets_at_epoch_seconds: None,
        latest_secondary_window_duration_mins: None,
        latest_secondary_resets_at_epoch_seconds: None,
        rollout_jsonl_tolerance_summary:
            crate::codex_threads::RolloutJsonlToleranceSummary::default(),
        observation_source: "codex_rollout_client_meter_v2".to_string(),
    };

    let scope = super::current_live_turn_assistant_scope_from_client_meter(
        &events,
        &matched_context_pack_ids,
        &observation,
    )
    .expect("scope");

    assert_eq!(scope.target_group_count, 1);
    assert_eq!(scope.observed_group_count, 1);
    assert_eq!(scope.observed_tokens, 33);
    assert_eq!(
        scope.target_context_pack_ids,
        ["cp-live".to_string()].into_iter().collect()
    );
    assert_eq!(
        scope.matched_context_pack_ids,
        ["cp-live".to_string()].into_iter().collect()
    );
    assert_eq!(
        scope.matched_rollout_turn_ids,
        ["thread-1:turn-1".to_string()].into_iter().collect()
    );
    assert_eq!(scope.available_rollout_turns, 1);
}

#[test]
fn current_live_turn_context_pack_match_bounds_stay_turn_scoped() {
    let measurement = measurement_fixture();
    let started_at_epoch_ms = 1_000_000i64;
    let ended_at_epoch_ms = 1_010_000i64;

    let (lower_bound, upper_bound) = super::current_live_turn_context_pack_match_bounds(
        started_at_epoch_ms,
        ended_at_epoch_ms,
        super::current_live_turn_context_pack_match_grace_ms(),
    )
    .expect("bounds");

    assert_eq!(
        super::current_live_turn_context_pack_match_grace_ms(),
        super::ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS
    );
    assert_eq!(lower_bound, 940_000);
    assert_eq!(upper_bound, 1_070_000);
    assert!(
        super::current_live_turn_context_pack_match_grace_ms()
            < (measurement
                .late_arrival_grace_minutes
                .saturating_mul(60_000)) as i64
    );
}

#[test]
fn live_turn_token_budget_events_match_thread_turn_without_context_pack_ids() {
    let events = vec![token_event! {
        event_id: "event-live-turn".to_string(),
        thread_id: Some("thread-1".to_string()),
        turn_id: Some("turn-1".to_string()),
        traffic_class: "live".to_string(),
        occurred_at_epoch_ms: 150,
        created_at_epoch_ms: 150,
    }];

    let matched = super::live_turn_token_budget_events(
        &events,
        "thread-1",
        "turn-1",
        &std::collections::BTreeSet::new(),
        100,
        200,
        super::current_live_turn_context_pack_match_grace_ms(),
    );

    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].event_id, "event-live-turn");
}

#[test]
fn working_state_retrieval_context_pack_live_filter_requires_live_marker() {
    assert!(super::working_state_retrieval_context_pack_is_live(
        Some("live"),
        None,
    ));
    assert!(super::working_state_retrieval_context_pack_is_live(
        None,
        Some("live_continuity_restore"),
    ));
    assert!(!super::working_state_retrieval_context_pack_is_live(
        Some("proof"),
        Some("proof_context_recovery"),
    ));
    assert!(!super::working_state_retrieval_context_pack_is_live(
        None, None
    ));
}

#[test]
fn current_live_turn_meter_summary_enables_same_meter_pair_without_quality_gate() {
    let measurement = measurement_fixture();
    let contract = contract_fixture();
    let events = vec![token_event! {
        event_id: "event-live-turn".to_string(),
        context_pack_id: Some("cp-live".to_string()),
        correlation_id: "cp-live".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        traffic_class: "live".to_string(),
        quality_ok: false,
        context_tokens: 40,
        naive_tokens: 200,
        effective_saved_tokens: 160,
        client_prompt_tokens: Some(12),
        assistant_generation_tokens: None,
        tool_overhead_tokens: Some(9),
    }];
    let matched_context_pack_ids = ["cp-live".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let observation = crate::codex_threads::RolloutClientMeterObservation {
        thread_id: "thread-1".to_string(),
        rollout_path: "/tmp/current-live-turn.jsonl".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_epoch_ms: 100,
        ended_at_epoch_ms: 200,
        first_assistant_response_at_epoch_ms: None,
        client_turn_total_tokens: 94,
        client_turn_input_tokens: 12,
        client_turn_cached_input_tokens: 0,
        client_turn_output_tokens: 33,
        client_turn_reasoning_output_tokens: 0,
        latest_cumulative_total_tokens: 94,
        latest_model_context_window: 258400,
        latest_primary_limit_used_percent: 10,
        latest_secondary_limit_used_percent: 20,
        latest_primary_window_duration_mins: None,
        latest_primary_resets_at_epoch_seconds: None,
        latest_secondary_window_duration_mins: None,
        latest_secondary_resets_at_epoch_seconds: None,
        rollout_jsonl_tolerance_summary:
            crate::codex_threads::RolloutJsonlToleranceSummary::default(),
        observation_source: "codex_rollout_client_meter_v2".to_string(),
    };
    let assistant_scope = super::current_live_turn_assistant_scope_from_client_meter(
        &events,
        &matched_context_pack_ids,
        &observation,
    )
    .expect("scope");
    let summary = super::summarize_events(&events, 250, &measurement, &contract);
    let raw_preview = super::build_dashboard_statement_preview(
        "current_live_turn",
        "текущий live-turn",
        &summary,
        &events,
        &contract,
        &[],
        Some(&assistant_scope),
    );
    assert!(super::scope_same_meter_exact_pair(&raw_preview).is_none());

    let adjusted_summary = super::current_live_turn_meter_summary(&summary);
    let preview = super::build_dashboard_statement_preview(
        "current_live_turn",
        "текущий live-turn",
        &adjusted_summary,
        &events,
        &contract,
        &[],
        Some(&assistant_scope),
    );
    let (without_amai, with_amai, saved_tokens, saved_pct) =
        super::scope_same_meter_exact_pair(&preview).expect("exact pair");

    assert_eq!(
        preview["client_limit_meter_alignment"]["same_meter_as_client_limit"],
        json!(true)
    );
    assert_eq!(without_amai, 45);
    assert_eq!(with_amai, 94);
    assert_eq!(saved_tokens, -49);
    assert!((saved_pct - (-108.888_888_888_888_89)).abs() < 1e-9);
}

#[test]
fn current_live_turn_full_turn_exact_pair_uses_retrieval_baseline_and_tool_overhead() {
    let measurement = measurement_fixture();
    let contract = contract_fixture();
    let events = vec![token_event! {
        event_id: "event-live-turn".to_string(),
        context_pack_id: Some("cp-live".to_string()),
        correlation_id: "cp-live".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        traffic_class: "live".to_string(),
        quality_ok: false,
        context_tokens: 40,
        naive_tokens: 200,
        effective_saved_tokens: 160,
        client_prompt_tokens: Some(12),
        assistant_generation_tokens: None,
        tool_overhead_tokens: Some(9),
    }];
    let matched_context_pack_ids = ["cp-live".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let observation = crate::codex_threads::RolloutClientMeterObservation {
        thread_id: "thread-1".to_string(),
        rollout_path: "/tmp/current-live-turn.jsonl".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_epoch_ms: 100,
        ended_at_epoch_ms: 200,
        first_assistant_response_at_epoch_ms: None,
        client_turn_total_tokens: 94,
        client_turn_input_tokens: 12,
        client_turn_cached_input_tokens: 0,
        client_turn_output_tokens: 33,
        client_turn_reasoning_output_tokens: 0,
        latest_cumulative_total_tokens: 94,
        latest_model_context_window: 258400,
        latest_primary_limit_used_percent: 10,
        latest_secondary_limit_used_percent: 20,
        latest_primary_window_duration_mins: None,
        latest_primary_resets_at_epoch_seconds: None,
        latest_secondary_window_duration_mins: None,
        latest_secondary_resets_at_epoch_seconds: None,
        rollout_jsonl_tolerance_summary:
            crate::codex_threads::RolloutJsonlToleranceSummary::default(),
        observation_source: "codex_rollout_client_meter_v2".to_string(),
    };
    let assistant_scope = super::current_live_turn_assistant_scope_from_client_meter(
        &events,
        &matched_context_pack_ids,
        &observation,
    )
    .expect("scope");
    let summary = super::current_live_turn_meter_summary(&super::summarize_events(
        &events,
        250,
        &measurement,
        &contract,
    ));
    let preview = super::build_dashboard_statement_preview(
        "current_live_turn",
        "текущий live-turn",
        &summary,
        &events,
        &contract,
        &[],
        Some(&assistant_scope),
    );

    let (without_amai, with_amai, saved_tokens, saved_pct) =
        super::current_live_turn_full_turn_exact_pair(&preview, &observation)
            .expect("full turn exact pair");

    assert_eq!(without_amai, 245);
    assert_eq!(with_amai, 94);
    assert_eq!(saved_tokens, 151);
    assert!((saved_pct - 61.632_653_061_224_49).abs() < 1e-9);
}

#[test]
fn helper_only_context_packs_do_not_expand_assistant_generation_target_scope() {
    let target_context_pack_ids = ["cp-real".to_string(), "cp-helper".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let metadata = [
        (
            "cp-real".to_string(),
            super::WorkingStateContextPackMeta {
                thread_id: "thread-1".to_string(),
                captured_at_epoch_ms: 100,
                turn_id: String::new(),
            },
        ),
        (
            "cp-helper".to_string(),
            super::WorkingStateContextPackMeta {
                thread_id: "thread-1".to_string(),
                captured_at_epoch_ms: 101,
                turn_id: String::new(),
            },
        ),
    ]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();

    let scope = super::derive_rollout_assistant_generation_scope_from_sources(
        &target_context_pack_ids,
        &[],
        &metadata,
        &std::collections::BTreeMap::new(),
        &["cp-helper".to_string()].into_iter().collect(),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(scope.target_group_count, 1);
    assert_eq!(
        scope.target_context_pack_ids,
        ["cp-real".to_string()].into_iter().collect()
    );
    assert_eq!(
        scope.helper_only_context_pack_ids,
        ["cp-helper".to_string()].into_iter().collect()
    );
    assert_eq!(
        scope.unmatched_context_pack_ids,
        ["cp-real".to_string()].into_iter().collect()
    );
}

#[test]
fn merged_context_pack_thread_ids_falls_back_to_token_budget_thread_id() {
    let metadata = BTreeMap::new();
    let context_pack_ids = ["cp-live".to_string()].into_iter().collect::<BTreeSet<_>>();
    let rows = BTreeMap::from([(
        "cp-live".to_string(),
        ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "token_budget_event".to_string(),
            created_at_epoch_ms: 100,
            payload: json!({
                "token_budget_event": {
                    "context_pack_id": "cp-live",
                    "thread_id": "thread-live"
                }
            }),
        },
    )]);

    let merged = super::merged_context_pack_thread_ids(&metadata, &rows, &context_pack_ids);

    assert_eq!(
        merged,
        BTreeMap::from([("cp-live".to_string(), "thread-live".to_string())])
    );
}

#[test]
fn merged_context_pack_thread_ids_prefers_working_state_metadata() {
    let metadata = BTreeMap::from([(
        "cp-live".to_string(),
        super::WorkingStateContextPackMeta {
            thread_id: "thread-working-state".to_string(),
            captured_at_epoch_ms: 100,
            turn_id: String::new(),
        },
    )]);
    let context_pack_ids = ["cp-live".to_string()].into_iter().collect::<BTreeSet<_>>();
    let rows = BTreeMap::from([(
        "cp-live".to_string(),
        ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "token_budget_event".to_string(),
            created_at_epoch_ms: 100,
            payload: json!({
                "token_budget_event": {
                    "context_pack_id": "cp-live",
                    "thread_id": "thread-token-budget"
                }
            }),
        },
    )]);

    let merged = super::merged_context_pack_thread_ids(&metadata, &rows, &context_pack_ids);

    assert_eq!(
        merged,
        BTreeMap::from([("cp-live".to_string(), "thread-working-state".to_string())])
    );
}

#[test]
fn merged_context_pack_thread_ids_with_repo_fallback_uses_repo_hint_when_missing() {
    let metadata = BTreeMap::new();
    let rows = BTreeMap::new();
    let context_pack_ids = ["cp-live".to_string()].into_iter().collect::<BTreeSet<_>>();
    let repo_thread_ids = BTreeMap::from([("cp-live".to_string(), "thread-repo".to_string())]);

    let merged = super::merged_context_pack_thread_ids_with_repo_fallback(
        &metadata,
        &rows,
        &repo_thread_ids,
        &context_pack_ids,
    );

    assert_eq!(
        merged,
        BTreeMap::from([("cp-live".to_string(), "thread-repo".to_string())])
    );
}

#[test]
fn merged_context_pack_thread_ids_with_repo_fallback_preserves_existing_thread_binding() {
    let metadata = BTreeMap::from([(
        "cp-live".to_string(),
        super::WorkingStateContextPackMeta {
            thread_id: "thread-working-state".to_string(),
            captured_at_epoch_ms: 100,
            turn_id: String::new(),
        },
    )]);
    let rows = BTreeMap::new();
    let context_pack_ids = ["cp-live".to_string()].into_iter().collect::<BTreeSet<_>>();
    let repo_thread_ids = BTreeMap::from([("cp-live".to_string(), "thread-repo".to_string())]);

    let merged = super::merged_context_pack_thread_ids_with_repo_fallback(
        &metadata,
        &rows,
        &repo_thread_ids,
        &context_pack_ids,
    );

    assert_eq!(
        merged,
        BTreeMap::from([("cp-live".to_string(), "thread-working-state".to_string())])
    );
}

#[test]
fn assistant_generation_scope_skips_non_model_visible_retrieval_events() {
    let helper_like_event = token_event! {
        context_pack_id: Some("cp-helper".to_string()),
        correlation_id: "cp-helper".to_string(),
        context_tokens: 0,
        recovery_tokens: 0,
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };
    let model_visible_event = token_event! {
        context_pack_id: Some("cp-real".to_string()),
        correlation_id: "cp-real".to_string(),
        context_tokens: 10,
        recovery_tokens: 0,
        client_prompt_tokens: Some(5),
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };

    let target_ids = super::assistant_generation_missing_scope_context_pack_ids(Some(&[
        helper_like_event,
        model_visible_event,
    ]));

    assert_eq!(target_ids, ["cp-real".to_string()].into_iter().collect());
}

#[test]
fn assistant_generation_scope_skips_legacy_continuity_bootstrap_events() {
    let continuity_event = token_event! {
        context_pack_id: Some("cp-cont".to_string()),
        correlation_id: "cp-cont".to_string(),
        namespace: "continuity".to_string(),
        source_kind: "live_context_pack".to_string(),
        query: super::CONTINUITY_SNAPSHOT_QUERY_LABEL.to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        context_tokens: 675,
        recovery_tokens: 0,
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: Some(2077),
        continuity_restore_tokens: None,
    };

    let target_ids =
        super::assistant_generation_missing_scope_context_pack_ids(Some(&[continuity_event]));

    assert!(target_ids.is_empty());
}

#[test]
fn assistant_generation_scope_skips_context_packs_without_turn_lineage() {
    let target_context_pack_ids = ["cp-real".to_string(), "cp-detached".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let metadata = [(
        "cp-real".to_string(),
        super::WorkingStateContextPackMeta {
            thread_id: "thread-1".to_string(),
            captured_at_epoch_ms: 100,
            turn_id: String::new(),
        },
    )]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();

    let scope = super::derive_rollout_assistant_generation_scope_from_sources(
        &target_context_pack_ids,
        &[],
        &metadata,
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeSet::new(),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(scope.target_group_count, 1);
    assert_eq!(
        scope.target_context_pack_ids,
        ["cp-real".to_string()].into_iter().collect()
    );
    assert_eq!(
        scope.unmatched_context_pack_ids,
        ["cp-real".to_string()].into_iter().collect()
    );
}

#[test]
fn assistant_generation_scope_skips_metadata_without_capture_time() {
    let target_context_pack_ids = ["cp-real".to_string(), "cp-stale".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let metadata = [
        (
            "cp-real".to_string(),
            super::WorkingStateContextPackMeta {
                thread_id: "thread-1".to_string(),
                captured_at_epoch_ms: 100,
                turn_id: String::new(),
            },
        ),
        (
            "cp-stale".to_string(),
            super::WorkingStateContextPackMeta {
                thread_id: "thread-1".to_string(),
                captured_at_epoch_ms: 0,
                turn_id: String::new(),
            },
        ),
    ]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();

    let scope = super::derive_rollout_assistant_generation_scope_from_sources(
        &target_context_pack_ids,
        &[],
        &metadata,
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeSet::new(),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(scope.target_group_count, 1);
    assert_eq!(
        scope.target_context_pack_ids,
        ["cp-real".to_string()].into_iter().collect()
    );
}

#[test]
fn assistant_generation_scope_matches_rollout_by_turn_id_without_capture_time() {
    let target_context_pack_ids = ["cp-stale".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let metadata = [(
        "cp-stale".to_string(),
        super::WorkingStateContextPackMeta {
            thread_id: "thread-1".to_string(),
            captured_at_epoch_ms: 0,
            turn_id: "turn-1".to_string(),
        },
    )]
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();
    let turns_by_thread = std::collections::BTreeMap::from([(
        "thread-1".to_string(),
        vec![
            crate::codex_threads::RolloutAssistantGenerationTurnObservation {
                thread_id: "thread-1".to_string(),
                rollout_path: "/tmp/thread-1.jsonl".to_string(),
                turn_id: "turn-1".to_string(),
                started_at_epoch_ms: 100,
                ended_at_epoch_ms: 200,
                assistant_generation_tokens: 33,
                token_count_events: 1,
                approved_context_pack_calls: 1,
                observation_source: "codex_rollout_turn_timeline_v1".to_string(),
            },
        ],
    )]);

    let scope = super::derive_rollout_assistant_generation_scope_from_sources(
        &target_context_pack_ids,
        &[],
        &metadata,
        &turns_by_thread,
        &std::collections::BTreeSet::new(),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(scope.target_group_count, 1);
    assert_eq!(scope.observed_group_count, 1);
    assert_eq!(scope.observed_tokens, 33);
    assert_eq!(
        scope.matched_context_pack_ids,
        ["cp-stale".to_string()].into_iter().collect()
    );
    assert_eq!(
        scope.matched_rollout_turn_ids,
        ["thread-1:turn-1".to_string()].into_iter().collect()
    );
}

#[test]
fn tool_overhead_target_count_skips_helper_only_non_model_visible_events() {
    let helper_event = token_event! {
        context_pack_id: Some("cp-helper".to_string()),
        correlation_id: "cp-helper".to_string(),
        context_tokens: 0,
        recovery_tokens: 0,
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };
    let real_event = token_event! {
        context_pack_id: Some("cp-real".to_string()),
        correlation_id: "cp-real".to_string(),
        tool_overhead_tokens: Some(33),
    };
    let scope = super::AssistantGenerationScopeObservation {
        helper_only_context_pack_ids: ["cp-helper".to_string()].into_iter().collect(),
        helper_only_non_model_visible_context_pack_ids: ["cp-helper".to_string()]
            .into_iter()
            .collect(),
        ..super::AssistantGenerationScopeObservation::default()
    };

    let count = super::client_limit_component_target_live_events(
        "tool_overhead_outside_retrieval",
        Some(&[helper_event, real_event]),
        0,
        Some(&scope),
    );

    assert_eq!(count, 1);
}

#[test]
fn tool_overhead_target_count_skips_legacy_continuity_bootstrap_events() {
    let continuity_event = token_event! {
        context_pack_id: Some("cp-cont".to_string()),
        correlation_id: "cp-cont".to_string(),
        namespace: "continuity".to_string(),
        source_kind: "live_context_pack".to_string(),
        query: super::CONTINUITY_SNAPSHOT_QUERY_LABEL.to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        tool_overhead_tokens: Some(2077),
    };
    let real_event = token_event! {
        context_pack_id: Some("cp-real".to_string()),
        correlation_id: "cp-real".to_string(),
        tool_overhead_tokens: Some(33),
    };

    let count = super::client_limit_component_target_live_events(
        "tool_overhead_outside_retrieval",
        Some(&[continuity_event, real_event]),
        0,
        Some(&super::AssistantGenerationScopeObservation::default()),
    );

    assert_eq!(count, 1);
}

#[test]
fn tool_overhead_observation_source_groups_missing_events() {
    let missing_event = token_event! {
        context_pack_id: Some("cp-missing".to_string()),
        correlation_id: "cp-missing".to_string(),
        namespace: "art".to_string(),
        source_kind: "live_context_pack".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        tool_overhead_tokens: None,
    };
    let observed_event = token_event! {
        context_pack_id: Some("cp-observed".to_string()),
        correlation_id: "cp-observed".to_string(),
        namespace: "art".to_string(),
        source_kind: "live_context_pack".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        tool_overhead_tokens: Some(33),
    };

    let status = super::tool_overhead_observation_source_status(
        Some(&[missing_event, observed_event]),
        Some(&super::AssistantGenerationScopeObservation::default()),
    );

    assert_eq!(
        status["state"],
        "tool_overhead_source_partial_scope_overlap"
    );
    assert_eq!(status["target_live_events"], 2);
    assert_eq!(status["observed_live_events"], 1);
    assert_eq!(status["missing_live_events"], 1);
    assert_eq!(
        status["missing_class_sample"][0]["source_kind"],
        "live_context_pack"
    );
}

#[test]
fn tool_overhead_observation_source_reports_irrecoverable_source_loss() {
    let missing_event = token_event! {
        context_pack_id: Some("cp-missing".to_string()),
        correlation_id: "cp-missing".to_string(),
        namespace: "art".to_string(),
        source_kind: "live_context_pack".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        tool_overhead_tokens: None,
        tool_overhead_source: Some(json!({
            "state": "context_pack_payload_missing_irrecoverable",
            "source_kind": "ami.context_packs",
        })),
    };

    let status = super::tool_overhead_observation_source_status(
        Some(&[missing_event]),
        Some(&super::AssistantGenerationScopeObservation::default()),
    );

    assert_eq!(status["state"], "tool_overhead_irrecoverable_debt_only");
    assert_eq!(status["recoverability_state"], "source_loss_irrecoverable");
    assert_eq!(
        status["gap_semantics"],
        "irrecoverable_historical_debt_only"
    );
    assert_eq!(status["irrecoverable_missing_live_events"], 1);
    assert_eq!(
        status["missing_source_state_sample"][0]["state"],
        "context_pack_payload_missing_irrecoverable"
    );
}

#[test]
fn tool_overhead_observation_source_treats_missing_identity_as_irrecoverable() {
    let missing_event = token_event! {
        context_pack_id: None,
        correlation_id: "event-without-context-pack".to_string(),
        event_id: "event-without-context-pack".to_string(),
        namespace: "art".to_string(),
        source_kind: "live_context_pack".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        tool_overhead_tokens: None,
        tool_overhead_source: Some(json!({
            "state": "missing_context_pack_identity_irrecoverable",
            "source_kind": "token_budget_event_identity_v1",
        })),
    };

    let status = super::tool_overhead_observation_source_status(
        Some(&[missing_event]),
        Some(&super::AssistantGenerationScopeObservation::default()),
    );

    assert_eq!(status["state"], "tool_overhead_irrecoverable_debt_only");
    assert_eq!(status["recoverability_state"], "source_loss_irrecoverable");
    assert_eq!(
        status["gap_semantics"],
        "irrecoverable_historical_debt_only"
    );
    assert_eq!(status["irrecoverable_missing_live_events"], 1);
    assert_eq!(
        status["missing_source_state_sample"][0]["state"],
        "missing_context_pack_identity_irrecoverable"
    );
}

#[test]
fn tool_overhead_atomic_attach_keeps_tokens_when_source_status_updates() {
    let mut payload = json!({
        "token_budget_event": {
            "event_id": "event-tool-overhead-attach",
            "correlation_id": "event-tool-overhead-attach",
            "whole_cycle_observed": {
                "tool_overhead_tokens": null
            },
            "whole_cycle_observed_source": {}
        }
    });
    let node = payload["token_budget_event"]
        .as_object_mut()
        .expect("token budget event");
    let source_status = json!({
        "state": "secondary_context_pack_match_materialized",
        "source_kind": "context_pack_query_time_match_v1",
        "resolution_condition": "already_materialized"
    });

    let (updated_fields, retained_fields, whole_cycle_observed, tool_overhead_source, attached) =
        super::apply_tool_overhead_observed_and_source_status(node, 2077, &source_status, None)
            .expect("apply tool overhead attach");

    assert!(attached);
    assert_eq!(updated_fields, vec!["tool_overhead_tokens".to_string()]);
    assert!(retained_fields.is_empty());
    assert_eq!(whole_cycle_observed["tool_overhead_tokens"], 2077);
    assert_eq!(
        tool_overhead_source["state"],
        "secondary_context_pack_match_materialized"
    );
    assert_eq!(
        payload["token_budget_event"]["whole_cycle_observed"]["tool_overhead_tokens"],
        2077
    );
    assert_eq!(
        payload["token_budget_event"]["whole_cycle_observed_source"]["tool_overhead"]["state"],
        "secondary_context_pack_match_materialized"
    );
}

#[test]
fn tool_overhead_atomic_attach_replaces_exact_legacy_cli_raw_value_only() {
    let mut payload = json!({
        "token_budget_event": {
            "event_id": "event-tool-overhead-replace",
            "correlation_id": "event-tool-overhead-replace",
            "whole_cycle_observed": {
                "tool_overhead_tokens": 3175
            },
            "whole_cycle_observed_source": {}
        }
    });
    let node = payload["token_budget_event"]
        .as_object_mut()
        .expect("token budget event");
    let source_status = json!({
        "state": "context_pack_cli_model_visible_output_materialized",
        "source_kind": "context_pack_cli_output_v2",
        "tool_overhead_contract_version": super::CLI_CONTEXT_PACK_TOOL_OVERHEAD_CONTRACT_VERSION,
    });

    let (updated_fields, retained_fields, whole_cycle_observed, tool_overhead_source, attached) =
        super::apply_tool_overhead_observed_and_source_status(
            node,
            1605,
            &source_status,
            Some(3175),
        )
        .expect("replace legacy cli tool overhead");

    assert!(attached);
    assert_eq!(updated_fields, vec!["tool_overhead_tokens".to_string()]);
    assert!(retained_fields.is_empty());
    assert_eq!(whole_cycle_observed["tool_overhead_tokens"], 1605);
    assert_eq!(
        tool_overhead_source["tool_overhead_contract_version"],
        super::CLI_CONTEXT_PACK_TOOL_OVERHEAD_CONTRACT_VERSION
    );
    assert_eq!(
        payload["token_budget_event"]["whole_cycle_observed"]["tool_overhead_tokens"],
        1605
    );
}

#[test]
fn tool_overhead_atomic_attach_replaces_legacy_payload_value_for_mcp_surface() {
    let mut payload = json!({
        "token_budget_event": {
            "event_id": "event-tool-overhead-mcp-replace",
            "correlation_id": "event-tool-overhead-mcp-replace",
            "whole_cycle_observed": {
                "tool_overhead_tokens": 987
            },
            "whole_cycle_observed_source": {
                "tool_overhead": {
                    "state": "context_pack_payload_materialized",
                    "source_kind": "ami.context_packs"
                }
            }
        }
    });
    let node = payload["token_budget_event"]
        .as_object_mut()
        .expect("token budget event");
    let source_status = super::mcp_context_pack_tool_overhead_source_status();

    let (updated_fields, retained_fields, whole_cycle_observed, tool_overhead_source, attached) =
        super::apply_tool_overhead_observed_and_source_status(node, 395, &source_status, Some(987))
            .expect("replace legacy payload tool overhead for mcp");

    assert!(attached);
    assert_eq!(updated_fields, vec!["tool_overhead_tokens".to_string()]);
    assert!(retained_fields.is_empty());
    assert_eq!(whole_cycle_observed["tool_overhead_tokens"], 395);
    assert_eq!(
        tool_overhead_source["source_kind"],
        "context_pack_mcp_structured_content_v2"
    );
    assert_eq!(
        payload["token_budget_event"]["whole_cycle_observed"]["tool_overhead_tokens"],
        395
    );
    assert_eq!(
        payload["token_budget_event"]["whole_cycle_observed_source"]["tool_overhead"]["state"],
        "context_pack_mcp_structured_content_materialized"
    );
}

#[test]
fn explicit_legacy_cli_tool_overhead_source_allows_direct_replacement() {
    let source_status = json!({
        "state": "context_pack_payload_materialized",
        "source_kind": "ami.context_packs",
        "tool_overhead_contract_version": super::CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION,
    });

    assert!(super::is_replaceable_legacy_cli_tool_overhead_source(
        &source_status
    ));
    assert!(super::legacy_cli_tool_overhead_source_allows_direct_replacement(&source_status));
}

#[test]
fn heuristic_legacy_payload_source_still_requires_raw_match() {
    let source_status = json!({
        "state": "context_pack_payload_materialized",
        "source_kind": "ami.context_packs",
    });

    assert!(super::is_replaceable_legacy_cli_tool_overhead_source(
        &source_status
    ));
    assert!(!super::legacy_cli_tool_overhead_source_allows_direct_replacement(&source_status));
}

#[test]
fn secondary_context_pack_selector_accepts_unique_nearest_match() {
    let selected = super::select_secondary_context_pack_payload_match(vec![
        super::SecondaryContextPackPayloadMatch {
            context_pack_id: "ctx-nearest".to_string(),
            payload_json: "{}".to_string(),
            delta_ms: 420,
        },
        super::SecondaryContextPackPayloadMatch {
            context_pack_id: "ctx-far".to_string(),
            payload_json: "{}".to_string(),
            delta_ms: 1700,
        },
    ]);

    match selected {
        super::SecondaryContextPackPayloadLookup::Resolved(candidate) => {
            assert_eq!(candidate.context_pack_id, "ctx-nearest");
            assert_eq!(candidate.delta_ms, 420);
        }
        other => panic!("unexpected selector result: {other:?}"),
    }
}

#[test]
fn secondary_context_pack_selector_rejects_tied_nearest_matches() {
    let selected = super::select_secondary_context_pack_payload_match(vec![
        super::SecondaryContextPackPayloadMatch {
            context_pack_id: "ctx-a".to_string(),
            payload_json: "{}".to_string(),
            delta_ms: 250,
        },
        super::SecondaryContextPackPayloadMatch {
            context_pack_id: "ctx-b".to_string(),
            payload_json: "{}".to_string(),
            delta_ms: 250,
        },
    ]);

    match selected {
        super::SecondaryContextPackPayloadLookup::AmbiguousNearest {
            delta_ms,
            candidate_count,
        } => {
            assert_eq!(delta_ms, 250);
            assert_eq!(candidate_count, 2);
        }
        other => panic!("unexpected selector result: {other:?}"),
    }
}

#[test]
fn secondary_context_pack_selector_rejects_far_nearest_match() {
    let selected = super::select_secondary_context_pack_payload_match(vec![
        super::SecondaryContextPackPayloadMatch {
            context_pack_id: "ctx-far".to_string(),
            payload_json: "{}".to_string(),
            delta_ms: super::TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS + 1,
        },
    ]);

    match selected {
        super::SecondaryContextPackPayloadLookup::NearestTooFar { delta_ms } => {
            assert_eq!(
                delta_ms,
                super::TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS + 1
            );
        }
        other => panic!("unexpected selector result: {other:?}"),
    }
}

#[test]
fn summarize_events_derives_client_prompt_tokens_from_query_when_field_missing() {
    let measurement = measurement_fixture();
    let contract = contract_fixture();
    let events = vec![token_event! {
        event_id: "event-derived-client-prompt".to_string(),
        query: "token report".to_string(),
        tokenizer: "o200k_base".to_string(),
        naive_tokens: 100,
        context_tokens: 40,
        recovery_tokens: 0,
        effective_saved_tokens: 60,
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    }];

    let summary = summarize_events(&events, 1_000, &measurement, &contract);
    assert!(
        summary["observed_client_prompt_tokens"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    assert_eq!(summary["observed_client_prompt_live_events"], 1);

    let alignment = super::build_client_limit_meter_alignment(
        &contract,
        "statement_preview",
        &summary,
        Some(&events),
        None,
        None,
    );
    assert_eq!(
        alignment["alignment_state"],
        "whole_cycle_partially_observed_not_meter_equivalent"
    );
}

#[test]
fn legacy_token_event_repair_adds_missing_fields() {
    let repaired = repair_legacy_token_event_payload(&json!({
        "token_budget_event": {
            "query": "Как установить Amai и подключить к VS Code?",
            "source_kind": "live_context_pack",
            "naive_scope": { "tokens": 1000 },
            "context_pack_render": { "tokens": 200 },
            "savings": {
                "saved_tokens": 800,
                "savings_percent": 80.0,
                "savings_factor": 5.0
            }
        }
    }))
    .expect("repair should produce patched payload");

    let event = &repaired["token_budget_event"];
    assert_eq!(event["traffic_class"], "live");
    assert_eq!(event["query_type"], "onboarding_query");
    assert_eq!(event["quality"]["quality_method"], "legacy_unverified");
    assert_eq!(event["savings"]["effective_saved_tokens"], 800);
    assert_eq!(event["savings"]["effective_savings_percent"], 80.0);
}

#[test]
fn current_session_events_prefers_latest_session_id_over_time_gap_chain() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            timestamp_utc: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            session_id: "session-1".to_string(),
            timestamp_utc: 20,
        },
        token_event! {
            created_at_epoch_ms: 30,
            event_id: "event-3".to_string(),
            session_id: "session-2".to_string(),
            timestamp_utc: 30,
        },
        token_event! {
            created_at_epoch_ms: 40,
            event_id: "event-4".to_string(),
            session_id: "session-2".to_string(),
            timestamp_utc: 40,
        },
    ];

    let session = current_session_events(&events, 60_000);
    let ids = session
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["event-3", "event-4"]);
}

#[test]
fn current_session_events_falls_back_to_gap_for_legacy_empty_session_id() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 20,
        },
        token_event! {
            created_at_epoch_ms: 90_100,
            event_id: "event-3".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 90_100,
        },
    ];

    let session = current_session_events(&events, 60_000);
    let ids = session
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["event-3"]);
}

#[test]
fn recent_current_session_slice_complete_when_session_boundary_is_present() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            timestamp_utc: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            session_id: "session-1".to_string(),
            timestamp_utc: 20,
        },
        token_event! {
            created_at_epoch_ms: 30,
            event_id: "event-3".to_string(),
            session_id: "session-2".to_string(),
            timestamp_utc: 30,
        },
        token_event! {
            created_at_epoch_ms: 40,
            event_id: "event-4".to_string(),
            session_id: "session-2".to_string(),
            timestamp_utc: 40,
        },
    ];

    assert!(super::recent_current_session_slice_complete(
        &events, 4, 4, 60_000
    ));
}

#[test]
fn recent_current_session_slice_complete_rejects_truncated_single_session_tail() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 30,
            event_id: "event-3".to_string(),
            session_id: "session-2".to_string(),
            timestamp_utc: 30,
        },
        token_event! {
            created_at_epoch_ms: 40,
            event_id: "event-4".to_string(),
            session_id: "session-2".to_string(),
            timestamp_utc: 40,
        },
    ];

    assert!(!super::recent_current_session_slice_complete(
        &events, 2, 2, 60_000
    ));
}

#[test]
fn recent_current_session_slice_complete_accepts_legacy_gap_boundary() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 20,
        },
        token_event! {
            created_at_epoch_ms: 90_100,
            event_id: "event-3".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 90_100,
        },
    ];

    assert!(super::recent_current_session_slice_complete(
        &events, 3, 3, 60_000
    ));
}

#[test]
fn recent_current_session_slice_complete_rejects_legacy_slice_without_gap_boundary() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 20,
        },
        token_event! {
            created_at_epoch_ms: 30,
            event_id: "event-3".to_string(),
            session_id: "".to_string(),
            timestamp_utc: 30,
        },
    ];

    assert!(!super::recent_current_session_slice_complete(
        &events, 3, 3, 60_000
    ));
}

#[test]
fn continuity_startup_source_kind_starts_new_session_id() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-1".to_string(),
        session_id: "session-1".to_string(),
        timestamp_utc: 10,
        source_kind: "live_context_pack".to_string(),
    }];

    let session_id = resolve_session_id(
        &events,
        20,
        60_000,
        "live_continuity_startup",
        "art",
        "continuity",
        "art::continuity::default",
    );
    assert_ne!(session_id, "session-1");
    assert!(!session_id.is_empty());
}

#[test]
fn non_startup_source_kind_reuses_recent_live_session_id() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-1".to_string(),
        session_id: "session-1".to_string(),
        timestamp_utc: 10,
        source_kind: "live_context_pack".to_string(),
    }];

    let session_id = resolve_session_id(
        &events,
        20,
        60_000,
        "live_context_pack",
        "art",
        "continuity",
        "art::continuity::default",
    );
    assert_eq!(session_id, "session-1");
}

#[test]
fn resolve_session_id_reuses_only_same_agent_scope() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-exact".to_string(),
            session_id: "session-exact".to_string(),
            timestamp_utc: 10,
            agent_scope: "art::continuity::default".to_string(),
        },
        token_event! {
            created_at_epoch_ms: 19,
            event_id: "event-foreign".to_string(),
            session_id: "session-foreign".to_string(),
            timestamp_utc: 19,
            agent_scope: "art::continuity::secondary".to_string(),
        },
    ];

    let session_id = resolve_session_id(
        &events,
        20,
        60_000,
        "live_context_pack",
        "art",
        "continuity",
        "art::continuity::default",
    );
    assert_eq!(session_id, "session-exact");
}

#[test]
fn only_legacy_live_events_need_reverification() {
    assert!(needs_live_reverification(&json!({
        "token_budget_event": {
            "source_kind": "live_context_pack",
            "traffic_class": "live",
            "quality": {
                "quality_ok": false,
                "quality_method": "legacy_unverified"
            }
        }
    })));
    assert!(needs_live_reverification(&json!({
        "token_budget_event": {
            "source_kind": "live_context_pack",
            "traffic_class": "live",
            "target_kind": "unknown",
            "quality": {
                "quality_ok": true,
                "quality_method": "reverified_retrieval_parity"
            },
            "shape": {}
        }
    })));
    assert!(!needs_live_reverification(&json!({
        "token_budget_event": {
            "source_kind": "verify_token_benchmark",
            "traffic_class": "verify",
            "quality": {
                "quality_ok": true,
                "quality_method": "benchmark_assumption"
            }
        }
    })));
    assert!(!needs_live_reverification(&json!({
        "token_budget_event": {
            "source_kind": "live_context_pack",
            "traffic_class": "live",
            "target_kind": "file",
            "latency_ms": 12.0,
            "quality": {
                "quality_ok": true,
                "quality_method": "retrieval_parity",
                "quality_tier": "retrieval",
                "head_hit_target": true
            },
            "followup": {
                "needed_followup": false,
                "followup_count": 0
            },
            "shape": {
                "file_hits": 1,
                "pack_token_count": 100,
                "deduped_token_count": 100
            }
        }
    })));
}

#[test]
fn token_ledger_repair_selector_matches_project_prefix_namespace_and_source_kind() {
    let event = TokenBudgetEvent {
        snapshot_id: None,
        created_at_epoch_ms: 0,
        event_id: "event-1".to_string(),
        correlation_id: "corr-1".to_string(),
        context_pack_id: None,
        thread_id: None,
        turn_id: None,
        agent_scope: "amai::continuity::default".to_string(),
        payload_origin: "context_pack_token_budget".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 1,
        occurred_at_epoch_ms: 1,
        ingested_at_epoch_ms: 1,
        snapshot_kind: "token_budget_event".to_string(),
        source_kind: "live_context_pack".to_string(),
        traffic_class: "live".to_string(),
        measurement_scope: "retrieval_lower_bound".to_string(),
        usage_event_schema_version: default_usage_event_schema_version(),
        settlement_statement_version: default_settlement_statement_version(),
        metering_event_schema_version: default_metering_event_schema_version(),
        usage_lifecycle_model_version: default_usage_lifecycle_model_version(),
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
        project: "memory_eval_archival_read".to_string(),
        namespace: "continuity".to_string(),
        query: "shared_runtime_marker".to_string(),
        query_hash: "hash".to_string(),
        query_type: "architecture_question".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "semantic_top_k".to_string(),
        retrieval_mode: Some("local_plus_related".to_string()),
        retrieval_scope_signature: None,
        tokenizer: "o200k_base".to_string(),
        latency_ms: 12.0,
        saved_tokens: 50,
        naive_tokens: 100,
        context_tokens: 50,
        recovery_tokens: 0,
        effective_saved_tokens: 50,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
        quality_score: 1.0,
        quality_method: "hybrid_answer_proxy".to_string(),
        quality_tier: "task_success_recovered".to_string(),
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
        pack_token_count: 50,
        deduped_token_count: 50,
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
        tool_overhead_source: None,
        pre_amai_baseline_source: None,
    };

    let matches = matches_token_ledger_repair_selector(
        &event,
        &TokenLedgerRepairRequest {
            limit: None,
            project: None,
            project_prefix: Some("memory_eval".to_string()),
            namespace: Some("continuity".to_string()),
            source_kind: Some("live_context_pack".to_string()),
            correlation_id: Some("corr-1".to_string()),
            rewrite_source_kind: Some("verify_memory_matrix_context_pack".to_string()),
            repair_reason: None,
        },
    );
    assert!(matches);
    let mismatched = matches_token_ledger_repair_selector(
        &event,
        &TokenLedgerRepairRequest {
            limit: None,
            project: None,
            project_prefix: Some("project_alpha".to_string()),
            namespace: Some("continuity".to_string()),
            source_kind: Some("live_context_pack".to_string()),
            correlation_id: Some("corr-1".to_string()),
            rewrite_source_kind: Some("verify_memory_matrix_context_pack".to_string()),
            repair_reason: None,
        },
    );
    assert!(!mismatched);

    let mismatched_correlation = matches_token_ledger_repair_selector(
        &event,
        &TokenLedgerRepairRequest {
            limit: None,
            project: None,
            project_prefix: Some("memory_eval".to_string()),
            namespace: Some("continuity".to_string()),
            source_kind: Some("live_context_pack".to_string()),
            correlation_id: Some("corr-2".to_string()),
            rewrite_source_kind: Some("verify_memory_matrix_context_pack".to_string()),
            repair_reason: None,
        },
    );
    assert!(!mismatched_correlation);
}

#[test]
fn source_kind_rewrite_updates_usage_state_and_observability_source() {
    let row = ObservabilitySnapshotRecord {
        snapshot_id: Uuid::nil(),
        snapshot_kind: "token_budget_event".to_string(),
        created_at_epoch_ms: 123,
        payload: json!({
            "_observability": {
                "source_kind": "live_context_pack",
                "scope_project_code": "project_alpha",
                "scope_namespace_code": "review",
                "captured_at_epoch_ms": 123
            },
            "token_budget_event": {
                "event_id": "event-1",
                "correlation_id": "corr-1",
                "source_kind": "live_context_pack",
                "traffic_class": "live",
                "measurement_scope": "retrieval_lower_bound",
                "payload_origin": "context_pack_token_budget",
                "project": "project_alpha",
                "project_code": "project_alpha",
                "namespace": "review",
                "namespace_code": "review",
                "query": "shared_runtime_marker",
                "query_hash": "hash",
                "query_type": "architecture_question",
                "target_kind": "file",
                "baseline_hit_target": true,
                "amai_hit_target": true,
                "cold_warm_state": "warm",
                "baseline_strategy": "semantic_top_k",
                "retrieval_mode": "local_plus_related",
                "tokenizer": "o200k_base",
                "latency_ms": 12.0,
                "created_at_epoch_ms": 123,
                "timestamp_utc": 123,
                "occurred_at_epoch_ms": 123,
                "ingested_at_epoch_ms": 123,
                "session_id": "session-1",
                "rolling_window_profile": "codex_5h",
                "contract": {
                    "usage_event_schema_version": "billing-usage-event-v2",
                    "settlement_statement_version": "settlement-preview-v0",
                    "metering_event_schema_version": "token-budget-event-v3",
                    "usage_lifecycle_model_version": "usage-lifecycle-v0",
                    "baseline_method_version": "retrieval-baseline-v0",
                    "quality_method_version": "quality-gate-v0",
                    "coverage_model_version": "token-coverage-v0",
                    "metering_freshness_model_version": "metering-freshness-v0",
                    "excluded_taxonomy_version": "token-excluded-usage-v0",
                    "dedup_contract_version": "event-id-source-kind-v0",
                    "backfill_policy_version": "report-only-backfill-v0",
                    "correction_policy_version": "report-only-correction-v0",
                    "freeze_close_policy_version": "freeze-close-v0",
                    "late_arrival_policy_version": "late-arrival-v0",
                    "dispute_policy_version": "report-only-dispute-v0",
                    "settlement_lifecycle_model_version": "settlement-lifecycle-v0",
                    "statement_period_governance_version": "statement-period-governance-v0",
                    "adjustment_preview_model_version": "adjustment-preview-v0",
                    "adjustment_request_schema_version": "adjustment-request-v0",
                    "adjustment_registry_version": "adjustment-registry-v0",
                    "rate_card_binding_model_version": "rate-card-binding-v0",
                    "telemetry_surface_split_version": "tokenonomics-surface-split-v0",
                    "event_time_policy_version": "client-visible-ingest-v0",
                    "billing_policy_version": "report-only-v0",
                    "suitability_model_version": "token-suitability-v0",
                    "billing_mode": "report_only",
                    "reconciliation_contract_version": "provider-reconciliation-v0",
                    "margin_model_version": "margin-view-v0",
                    "infra_cost_profile_version": "unpriced-infra-v0",
                    "contractual_evidence_pack_version": "contractual-evidence-pack-v0",
                    "rate_card_version": "unpriced-v0",
                    "currency_profile": "unpriced",
                    "settlement_status": "unsettled_report_only"
                },
                "naive_scope": {
                    "tokens": 100
                },
                "context_pack_render": {
                    "tokens": 40
                },
                "recovery": {
                    "recovery_tokens": 0,
                    "fallback_triggered": false,
                    "fallback_count": 0
                },
                "savings": {
                    "saved_tokens": 60,
                    "effective_saved_tokens": 60,
                    "savings_factor": 2.5,
                    "savings_percent": 60.0,
                    "effective_savings_percent": 60.0
                },
                "quality": {
                    "quality_ok": true,
                    "quality_score": 1.0,
                    "quality_method": "hybrid_answer_proxy",
                    "quality_tier": "task_success_recovered",
                    "head_hit_target": true
                },
                "followup": {
                    "needed_followup": false,
                    "followup_count": 0
                },
                "shape": {
                    "document_hits": 1,
                    "symbol_hits": 0,
                    "file_hits": 1,
                    "sources_count": 1,
                    "chunks_count": 1,
                    "pack_token_count": 40,
                    "deduped_token_count": 40
                }
            }
        }),
    };

    let rewritten =
        rewrite_token_ledger_source_kind_payload(&row, "proof_mcp_context_pack", "test_reclassify")
            .expect("rewrite should succeed")
            .expect("rewrite should change payload");
    let event = &rewritten["token_budget_event"];
    assert_eq!(event["source_kind"], "proof_mcp_context_pack");
    assert_eq!(event["traffic_class"], "proof");
    assert_eq!(
        rewritten["_observability"]["source_kind"],
        "proof_mcp_context_pack"
    );
    assert_eq!(
        event["usage_identity"]["dedup_key"],
        "proof_mcp_context_pack:event-1"
    );
    assert_eq!(
        event["usage_state"]["excluded_reason_code"],
        "non_live_other"
    );
    assert_eq!(event["usage_state"]["reporting_layer"], "excluded");
    assert_eq!(event["usage_state"]["backfill_status"], "synthetic_ingest");
    assert_eq!(
        event["repair"]["operator_source_kind_rewrite"]["previous_source_kind"],
        "live_context_pack"
    );
    assert_eq!(
        event["repair"]["operator_source_kind_rewrite"]["rewritten_source_kind"],
        "proof_mcp_context_pack"
    );
}

#[test]
fn reverification_keeps_identity_and_marks_method() {
    let mut rebuilt = json!({
        "retrieval": {
            "exact_documents": [{}],
            "symbol_hits": [],
            "lexical_chunks": [],
            "semantic_chunks": []
        },
        "token_budget_event": {
            "target_kind": "file",
            "quality": {
                "quality_ok": true,
                "quality_score": 1.0,
                "quality_method": "retrieval_parity",
                "quality_tier": "retrieval",
                "head_hit_target": true
            }
        }
    });
    apply_reverification_metadata(
        &mut rebuilt,
        &json!({
            "event_id": "existing-event",
            "timestamp_utc": 12345,
            "source_kind": "live_context_pack",
            "quality": {
                "quality_ok": false,
                "quality_method": "legacy_unverified"
            }
        }),
        99999,
    )
    .expect("reverification metadata should apply");

    let event = &rebuilt["token_budget_event"];
    assert_eq!(event["event_id"], "existing-event");
    assert_eq!(event["timestamp_utc"], 12345);
    assert_eq!(event["traffic_class"], "live");
    assert_eq!(
        event["quality"]["quality_method"],
        "reverified_answer_proxy"
    );
    assert_eq!(event["quality"]["quality_tier"], "answer_proxy");
    assert_eq!(
        event["reverification"]["previous_quality_method"],
        "legacy_unverified"
    );
}

#[test]
fn preliminary_turns_off_when_token_volume_is_high_enough() {
    let measurement = MeasurementConfig {
        tokenizer: "o200k_base".to_string(),
        naive_limit_files: 5,
        naive_max_bytes_per_file: 16384,
        include_verify_events_by_default: false,
        metering_ingest_warning_seconds: 60,
        metering_ingest_slo_seconds: 300,
        late_arrival_grace_minutes: 60,
        preliminary_min_events: 50,
        preliminary_min_baseline_tokens: 100_000,
    };
    let summary = summarize_events(
        &[token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            correlation_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "explain token savings".to_string(),
            query_hash: "hash".to_string(),
            query_type: "architecture_question".to_string(),
            target_kind: "evidence_bundle".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 12.0,
            saved_tokens: 150_000,
            naive_tokens: 160_000,
            context_tokens: 10_000,
            recovery_tokens: 0,
            effective_saved_tokens: 150_000,
            savings_factor: 16.0,
            savings_percent: 93.75,
            effective_savings_percent: 93.75,
            quality_ok: true,
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
            symbol_hits_count: 2,
            file_hits: 3,
            sources_count: 5,
            chunks_count: 4,
            pack_token_count: 10_000,
            deduped_token_count: 10_000,
        }],
        20,
        &measurement,
        &contract_fixture(),
    );

    assert_eq!(summary["preliminary"], false);
    assert_eq!(summary["counted_events"], 1);
    assert_eq!(summary["answer_like_counted_events"], 1);
    assert_eq!(summary["verified_effective_savings_pct"], 93.75);
    assert_eq!(summary["verified_answer_like_savings_pct"], 93.75);
}

#[test]
fn summarize_events_exposes_extended_latency_distribution() {
    let measurement = MeasurementConfig {
        tokenizer: "o200k_base".to_string(),
        naive_limit_files: 5,
        naive_max_bytes_per_file: 16384,
        include_verify_events_by_default: false,
        metering_ingest_warning_seconds: 60,
        metering_ingest_slo_seconds: 300,
        late_arrival_grace_minutes: 60,
        preliminary_min_events: 50,
        preliminary_min_baseline_tokens: 100_000,
    };
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            correlation_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "first".to_string(),
            query_hash: "hash-1".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 11.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            correlation_id: "event-2".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            query: "second".to_string(),
            query_hash: "hash-2".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 5.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];

    let summary = summarize_events(&events, 20, &measurement, &contract_fixture());
    assert_eq!(summary["sample_count"], 2);
    assert_eq!(summary["current_latency_ms"], 5.0);
    assert_eq!(summary["p50_latency_ms"], 11.0);
    assert_eq!(summary["p95_latency_ms"], 11.0);
    assert_eq!(summary["p99_latency_ms"], 11.0);
    assert_eq!(summary["max_latency_ms"], 11.0);
}

#[test]
fn summarize_events_exposes_coverage_and_excluded_taxonomy() {
    let measurement = measurement_fixture();
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            correlation_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "verified".to_string(),
            query_hash: "hash-1".to_string(),
            query_type: "code_lookup".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            naive_tokens: 1000,
            context_tokens: 100,
            saved_tokens: 900,
            effective_saved_tokens: 900,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
            quality_method: "retrieval_parity".to_string(),
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            correlation_id: "event-2".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            query: "excluded".to_string(),
            query_hash: "hash-2".to_string(),
            query_type: "architecture_question".to_string(),
            baseline_strategy: "semantic_top_k".to_string(),
            naive_tokens: 500,
            context_tokens: 200,
            saved_tokens: 300,
            effective_saved_tokens: 300,
            savings_percent: 60.0,
            effective_savings_percent: 60.0,
            quality_ok: false,
            quality_method: "retrieval_parity".to_string(),
        },
    ];

    let summary = summarize_events(&events, 20, &measurement, &contract_fixture());
    assert_eq!(summary["coverage"]["model_version"], "token-coverage-v1");
    assert_eq!(summary["coverage"]["measured_events"], 2);
    assert_eq!(summary["coverage"]["included_events"], 1);
    assert_eq!(summary["coverage"]["excluded_events"], 1);
    assert_eq!(
        summary["coverage"]["completeness_state"],
        "partially_confirmed"
    );
    assert_eq!(summary["coverage"]["event_coverage_pct"], 50.0);
    let baseline_token_coverage_pct = summary["coverage"]["baseline_token_coverage_pct"]
        .as_f64()
        .expect("baseline token coverage pct");
    assert!((baseline_token_coverage_pct - (1000.0 / 1500.0 * 100.0)).abs() < 1e-9);

    let excluded_items = summary["excluded_breakdown"]["items"]
        .as_array()
        .expect("excluded items");
    assert_eq!(
        summary["excluded_breakdown"]["model_version"],
        "token-excluded-usage-v1"
    );
    assert_eq!(excluded_items.len(), 1);
    assert_eq!(excluded_items[0]["code"], "quality_gate_failed");
    assert_eq!(excluded_items[0]["events_count"], 1);
    assert_eq!(excluded_items[0]["baseline_tokens"], 500);
    assert_eq!(excluded_items[0]["delivered_tokens"], 200);
}

#[test]
fn baseline_strategy_breakdown_exposes_allowed_class_and_coverage() {
    let measurement = measurement_fixture();
    let events = vec![
        token_event! {
            event_id: "event-1".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            naive_tokens: 1000,
            context_tokens: 100,
            saved_tokens: 900,
            effective_saved_tokens: 900,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
        },
        token_event! {
            event_id: "event-2".to_string(),
            baseline_strategy: "semantic_top_k".to_string(),
            naive_tokens: 500,
            context_tokens: 200,
            saved_tokens: 300,
            effective_saved_tokens: 300,
            savings_percent: 60.0,
            effective_savings_percent: 60.0,
            quality_ok: false,
        },
    ];

    let breakdown = baseline_strategy_breakdown(&events, &measurement, &contract_fixture());
    let items = breakdown.as_array().expect("baseline breakdown");
    let naive = items
        .iter()
        .find(|item| item["baseline_strategy"] == "naive_top_files")
        .expect("naive strategy");
    let semantic = items
        .iter()
        .find(|item| item["baseline_strategy"] == "semantic_top_k")
        .expect("semantic strategy");

    assert_eq!(naive["allowed_class"], true);
    assert_eq!(naive["coverage"]["included_events"], 1);
    assert_eq!(semantic["allowed_class"], true);
    assert_eq!(semantic["coverage"]["excluded_events"], 1);
}

#[test]
fn latency_slice_breakdown_normalizes_hot_cold_and_mixed() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            correlation_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "cold".to_string(),
            query_hash: "hash-1".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 12.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-2".to_string(),
            correlation_id: "event-2".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            query: "hot".to_string(),
            query_hash: "hash-2".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 4.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];
    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let mixed = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("mixed"))
        .expect("mixed");
    let hot = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("hot"))
        .expect("hot");
    let cold = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("cold"))
        .expect("cold");

    assert_eq!(mixed["sample_count"], 2);
    assert_eq!(mixed["current_latency_ms"], 4.0);
    assert_eq!(hot["sample_count"], 1);
    assert_eq!(hot["display_name"], "hot");
    assert_eq!(cold["sample_count"], 1);
    assert_eq!(cold["display_name"], "cold");
}

#[test]
fn latency_slice_breakdown_ignores_continuity_restore_observed_events() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "retrieval-event".to_string(),
            correlation_id: "retrieval-event".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "lookup".to_string(),
            query_hash: "hash-1".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 12.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            event_id: "continuity-restore".to_string(),
            correlation_id: "continuity-restore".to_string(),
            session_id: "session-1".to_string(),
            source_kind: "live_continuity_startup".to_string(),
            measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
            query: "CHAT_START_RESTORE".to_string(),
            query_hash: "hash-2".to_string(),
            query_type: "continuity_restore".to_string(),
            target_kind: "continuity_restore".to_string(),
            baseline_hit_target: false,
            cold_warm_state: "observed_only".to_string(),
            baseline_strategy: "observed_only".to_string(),
            retrieval_mode: None,
            quality_method: "continuity_restore_observed".to_string(),
            quality_tier: "observed_only".to_string(),
            document_hits: 0,
            file_hits: 0,
            sources_count: 0,
            chunks_count: 0,
            continuity_restore_tokens: Some(817),
        },
    ];

    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let mixed = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("mixed"))
        .expect("mixed");

    assert_eq!(mixed["sample_count"], 1);
    assert_eq!(mixed["current_latency_ms"], 12.0);
    assert_eq!(mixed["p50_latency_ms"], 12.0);
}

#[test]
fn latency_slice_breakdown_ignores_unconfirmed_live_events() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "excluded-live".to_string(),
            correlation_id: "excluded-live".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            payload_origin: "context_pack_token_budget_v13".to_string(),
            query: "excluded".to_string(),
            query_hash: "hash-excluded".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 87.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: false,
            quality_score: 0.0,
            quality_method: "retrieval_parity".to_string(),
            quality_tier: "retrieval".to_string(),
            head_hit_target: true,
            needed_followup: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "confirmed-live".to_string(),
            correlation_id: "confirmed-live".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            payload_origin: "context_pack_token_budget_v13".to_string(),
            query: "confirmed".to_string(),
            query_hash: "hash-confirmed".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 5.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];

    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let cold = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("cold"))
        .expect("cold");

    assert_eq!(cold["sample_count"], 1);
    assert_eq!(cold["p50_latency_ms"], 5.0);
    assert_eq!(cold["max_latency_ms"], 5.0);
    assert_eq!(cold["current_latency_ms"], 5.0);
}

#[test]
fn latency_slice_breakdown_prefers_v2_live_measurement_when_present() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "legacy-v1".to_string(),
            correlation_id: "legacy-v1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            payload_origin: "context_pack_token_budget".to_string(),
            query: "legacy".to_string(),
            query_hash: "hash-legacy".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 55.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "live-v2".to_string(),
            correlation_id: "live-v2".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            payload_origin: "context_pack_token_budget_v2".to_string(),
            query: "fresh".to_string(),
            query_hash: "hash-fresh".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 7.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];

    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let cold = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("cold"))
        .expect("cold");

    assert_eq!(cold["sample_count"], 1);
    assert_eq!(cold["p50_latency_ms"], 7.0);
    assert_eq!(cold["max_latency_ms"], 7.0);
}

#[test]
fn latency_slice_breakdown_prefers_v3_live_measurement_when_present() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "legacy-v2".to_string(),
            correlation_id: "legacy-v2".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            payload_origin: "context_pack_token_budget_v2".to_string(),
            query: "legacy".to_string(),
            query_hash: "hash-legacy".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 55.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "live-v3".to_string(),
            correlation_id: "live-v3".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            payload_origin: "context_pack_token_budget_v3".to_string(),
            query: "fresh".to_string(),
            query_hash: "hash-fresh".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 7.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];

    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let cold = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("cold"))
        .expect("cold");

    assert_eq!(cold["sample_count"], 1);
    assert_eq!(cold["p50_latency_ms"], 7.0);
    assert_eq!(cold["max_latency_ms"], 7.0);
}

#[test]
fn latency_slice_breakdown_prefers_v4_live_measurement_when_present() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "legacy-v3".to_string(),
            correlation_id: "legacy-v3".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            payload_origin: "context_pack_token_budget_v3".to_string(),
            query: "legacy".to_string(),
            query_hash: "hash-legacy".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 55.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "live-v4".to_string(),
            correlation_id: "live-v4".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            payload_origin: "context_pack_token_budget_v4".to_string(),
            query: "fresh".to_string(),
            query_hash: "hash-fresh".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 7.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];

    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let cold = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("cold"))
        .expect("cold");

    assert_eq!(cold["sample_count"], 1);
    assert_eq!(cold["p50_latency_ms"], 7.0);
    assert_eq!(cold["max_latency_ms"], 7.0);
}

#[test]
fn latency_slice_breakdown_prefers_v9_live_measurement_when_present() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "legacy-v4".to_string(),
            correlation_id: "legacy-v4".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            payload_origin: "context_pack_token_budget_v4".to_string(),
            query: "legacy".to_string(),
            query_hash: "hash-legacy".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 55.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "live-v5".to_string(),
            correlation_id: "live-v5".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            payload_origin: "context_pack_token_budget_v5".to_string(),
            query: "fresh".to_string(),
            query_hash: "hash-fresh".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 7.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 30,
            event_id: "live-v6".to_string(),
            correlation_id: "live-v6".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 30,
            occurred_at_epoch_ms: 30,
            ingested_at_epoch_ms: 30,
            payload_origin: "context_pack_token_budget_v6".to_string(),
            query: "fresher".to_string(),
            query_hash: "hash-fresher".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 3.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 40,
            event_id: "live-v7".to_string(),
            correlation_id: "live-v7".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 40,
            occurred_at_epoch_ms: 40,
            ingested_at_epoch_ms: 40,
            payload_origin: "context_pack_token_budget_v7".to_string(),
            query: "freshest".to_string(),
            query_hash: "hash-freshest".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 2.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 60,
            event_id: "live-v10".to_string(),
            correlation_id: "live-v10".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 60,
            occurred_at_epoch_ms: 60,
            ingested_at_epoch_ms: 60,
            payload_origin: "context_pack_token_budget_v10".to_string(),
            query: "latest-postfix".to_string(),
            query_hash: "hash-latest-postfix".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 0.8,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
        token_event! {
            created_at_epoch_ms: 50,
            event_id: "live-v8".to_string(),
            correlation_id: "live-v8".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 50,
            occurred_at_epoch_ms: 50,
            ingested_at_epoch_ms: 50,
            payload_origin: "context_pack_token_budget_v8".to_string(),
            query: "latest".to_string(),
            query_hash: "hash-latest".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 1.0,
            saved_tokens: 90,
            naive_tokens: 100,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 90,
            savings_factor: 10.0,
            savings_percent: 90.0,
            effective_savings_percent: 90.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
        },
    ];

    let breakdown = latency_slice_breakdown(&events);
    let slices = breakdown.as_array().expect("array");
    let cold = slices
        .iter()
        .find(|slice| slice["state"].as_str() == Some("cold"))
        .expect("cold");

    assert_eq!(cold["sample_count"], 1);
    assert_eq!(cold["p50_latency_ms"], 0.8);
    assert_eq!(cold["max_latency_ms"], 0.8);
}

#[test]
fn dashboard_same_meter_sync_signature_tracks_missing_whole_cycle_targets() {
    let events = vec![
        token_event! {
            created_at_epoch_ms: 10,
            event_id: "event-a".to_string(),
            correlation_id: "ctx-a".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            occurred_at_epoch_ms: 10,
            ingested_at_epoch_ms: 10,
            query: "q-a".to_string(),
            query_hash: "hash-a".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 1.0,
            saved_tokens: 10,
            naive_tokens: 20,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 10,
            savings_factor: 2.0,
            savings_percent: 50.0,
            effective_savings_percent: 50.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
            tool_overhead_tokens: None,
            assistant_generation_tokens: None,
        },
        token_event! {
            created_at_epoch_ms: 20,
            event_id: "event-b".to_string(),
            correlation_id: "ctx-b".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 20,
            occurred_at_epoch_ms: 20,
            ingested_at_epoch_ms: 20,
            query: "q-b".to_string(),
            query_hash: "hash-b".to_string(),
            query_type: "code_lookup".to_string(),
            target_kind: "file".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "cold".to_string(),
            baseline_strategy: "naive_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 2.0,
            saved_tokens: 10,
            naive_tokens: 20,
            context_tokens: 10,
            recovery_tokens: 0,
            effective_saved_tokens: 10,
            savings_factor: 2.0,
            savings_percent: 50.0,
            effective_savings_percent: 50.0,
            quality_ok: true,
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
            pack_token_count: 10,
            deduped_token_count: 10,
            tool_overhead_tokens: Some(4),
            assistant_generation_tokens: None,
        },
    ];

    let signature = dashboard_same_meter_sync_signature(&events, "rollout-a");
    let reversed = vec![events[1].clone(), events[0].clone()];
    let reversed_signature = dashboard_same_meter_sync_signature(&reversed, "rollout-a");
    assert_eq!(signature, reversed_signature);

    let mut resolved = events.clone();
    resolved[0].assistant_generation_tokens = Some(5);
    let resolved_signature = dashboard_same_meter_sync_signature(&resolved, "rollout-a");
    assert_ne!(signature, resolved_signature);
    let changed_rollout_signature = dashboard_same_meter_sync_signature(&events, "rollout-b");
    assert_ne!(signature, changed_rollout_signature);
}

#[test]
fn shared_dashboard_same_meter_sync_signature_roundtrips() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-dashboard-same-meter-sync-cache-{}",
        Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_root).expect("temp root");

    super::write_shared_dashboard_same_meter_sync_signature(&temp_root, "sig-a")
        .expect("write shared cache");
    assert_eq!(
        super::load_shared_dashboard_same_meter_sync_signature(&temp_root).as_deref(),
        Some("sig-a")
    );

    let cache_path = super::dashboard_same_meter_sync_shared_cache_path(&temp_root);
    let mut cache: serde_json::Value =
        serde_json::from_slice(&fs::read(&cache_path).expect("read cache")).expect("json");
    cache["cache_version"] = serde_json::Value::from("stale-version");
    fs::write(
        &cache_path,
        serde_json::to_vec(&cache).expect("serialize stale cache"),
    )
    .expect("write stale cache");
    assert!(
        super::load_shared_dashboard_same_meter_sync_signature(&temp_root).is_none(),
        "stale cache version must fail closed"
    );

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn event_context_pack_id_ignores_synthetic_event_id_fallback() {
    let synthetic = token_event! {
        event_id: "legacy-event-id".to_string(),
        correlation_id: "legacy-event-id".to_string(),
        context_pack_id: None,
    };
    assert_eq!(super::event_context_pack_id(&synthetic), None);

    let real_correlation = token_event! {
        event_id: "legacy-event-id".to_string(),
        correlation_id: "ctx-real".to_string(),
        context_pack_id: None,
    };
    assert_eq!(
        super::event_context_pack_id(&real_correlation).as_deref(),
        Some("ctx-real")
    );

    let explicit_context_pack = token_event! {
        event_id: "legacy-event-id".to_string(),
        correlation_id: "legacy-event-id".to_string(),
        context_pack_id: Some("ctx-explicit".to_string()),
    };
    assert_eq!(
        super::event_context_pack_id(&explicit_context_pack).as_deref(),
        Some("ctx-explicit")
    );
}

#[test]
fn dashboard_report_signature_ignores_wall_clock_when_inputs_match() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-a".to_string(),
        correlation_id: "ctx-a".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "q-a".to_string(),
        query_hash: "hash-a".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "naive_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 1.0,
        saved_tokens: 10,
        naive_tokens: 20,
        context_tokens: 10,
        recovery_tokens: 0,
        effective_saved_tokens: 10,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
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
        pack_token_count: 10,
        deduped_token_count: 10,
    }];
    let scope = AssistantGenerationScopeObservation::default();
    let left = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        None,
        &json!({}),
        90,
    ));
    let right = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        None,
        &json!({}),
        90,
    ));

    assert_eq!(left, right);
}

#[test]
fn dashboard_report_signature_changes_when_assistant_scope_changes() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-a".to_string(),
        correlation_id: "ctx-a".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "q-a".to_string(),
        query_hash: "hash-a".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "naive_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 1.0,
        saved_tokens: 10,
        naive_tokens: 20,
        context_tokens: 10,
        recovery_tokens: 0,
        effective_saved_tokens: 10,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
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
        pack_token_count: 10,
        deduped_token_count: 10,
    }];
    let base_scope = AssistantGenerationScopeObservation::default();
    let changed_scope = AssistantGenerationScopeObservation {
        observed_tokens: 42,
        matched_context_pack_ids: ["ctx-a".to_string()].into_iter().collect(),
        ..AssistantGenerationScopeObservation::default()
    };
    let left = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &base_scope,
        Some(&base_scope),
        &base_scope,
        None,
        None,
        None,
        &json!({}),
        90,
    ));
    let right = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &changed_scope,
        Some(&base_scope),
        &base_scope,
        None,
        None,
        None,
        &json!({}),
        90,
    ));

    assert_ne!(left, right);
}

#[test]
fn dashboard_report_signature_changes_when_exact_client_limits_change() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-a".to_string(),
        correlation_id: "ctx-a".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "q-a".to_string(),
        query_hash: "hash-a".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "naive_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 1.0,
        saved_tokens: 10,
        naive_tokens: 20,
        context_tokens: 10,
        recovery_tokens: 0,
        effective_saved_tokens: 10,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
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
        pack_token_count: 10,
        deduped_token_count: 10,
    }];
    let scope = AssistantGenerationScopeObservation::default();
    let baseline_limits = super::CodexAppServerRateLimitsObservation {
        observed_at_epoch_ms: 100,
        rate_limits: super::CodexAppServerRateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            plan_type: None,
            primary: Some(super::CodexAppServerRateLimitWindow {
                used_percent: 22.0,
                window_duration_mins: Some(300),
                resets_at: Some(1_774_625_102),
            }),
            secondary: Some(super::CodexAppServerRateLimitWindow {
                used_percent: 37.0,
                window_duration_mins: Some(10_080),
                resets_at: Some(1_774_700_000),
            }),
            credits: None,
        },
    };
    let changed_limits = super::CodexAppServerRateLimitsObservation {
        observed_at_epoch_ms: 101,
        rate_limits: super::CodexAppServerRateLimitSnapshot {
            primary: Some(super::CodexAppServerRateLimitWindow {
                used_percent: 23.0,
                window_duration_mins: Some(300),
                resets_at: Some(1_774_625_202),
            }),
            ..baseline_limits.rate_limits.clone()
        },
    };
    let left = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        Some(&baseline_limits),
        &json!({}),
        90,
    ));
    let right = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        Some(&changed_limits),
        &json!({}),
        90,
    ));

    assert_ne!(left, right);
}

#[test]
fn dashboard_report_signature_changes_when_live_response_latency_changes() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-a".to_string(),
        correlation_id: "ctx-a".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "q-a".to_string(),
        query_hash: "hash-a".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "naive_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 1.0,
        saved_tokens: 10,
        naive_tokens: 20,
        context_tokens: 10,
        recovery_tokens: 0,
        effective_saved_tokens: 10,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
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
        pack_token_count: 10,
        deduped_token_count: 10,
    }];
    let scope = AssistantGenerationScopeObservation::default();
    let left = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        None,
        &json!({"rolling_window": {"latency_slices": []}}),
        90,
    ));
    let right = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        None,
        &json!({"rolling_window": {"latency_slices": [{"state": "cold", "sample_count": 1, "p50_latency_ms": 5.0}]}}),
        90,
    ));

    assert_ne!(left, right);
}

#[test]
fn current_session_live_response_turns_splits_thread_on_gap() {
    let turns = vec![
        super::LiveResponseTurnObservation {
            thread_id: "thread-live".to_string(),
            turn_id: "turn-1".to_string(),
            state: "cold".to_string(),
            started_at_epoch_ms: 1_000,
            ended_at_epoch_ms: 2_000,
            latency_ms: 1.0,
        },
        super::LiveResponseTurnObservation {
            thread_id: "thread-live".to_string(),
            turn_id: "turn-2".to_string(),
            state: "hot".to_string(),
            started_at_epoch_ms: 3_000,
            ended_at_epoch_ms: 4_000,
            latency_ms: 0.8,
        },
        super::LiveResponseTurnObservation {
            thread_id: "thread-live".to_string(),
            turn_id: "turn-3".to_string(),
            state: "cold".to_string(),
            started_at_epoch_ms: 800_000,
            ended_at_epoch_ms: 801_000,
            latency_ms: 2.2,
        },
    ];

    let current = super::current_session_live_response_turns(&turns, Some("thread-live"), 5);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].turn_id, "turn-3");
}

#[test]
fn annotate_live_response_latency_surface_marks_previous_turn_series_when_new_turn_is_empty() {
    let mut surface = json!({
        "current_thread_id": "thread-live",
        "current_session": {
            "sample_count": 3,
            "latest_turn": {
                "ended_at_epoch_ms": 2_000
            }
        }
    });
    let current_live_turn = json!({
        "status": "no_amai_activity_in_current_live_turn",
        "thread_id": "thread-live",
        "started_at_epoch_ms": 3_000
    });

    super::annotate_live_response_latency_surface(&mut surface, &current_live_turn);

    assert_eq!(
        surface["current_session_relation"]["status"],
        json!("recent_same_chat_series_previous_turn")
    );
    assert!(
        surface["current_session_relation"]["note"]
            .as_str()
            .is_some_and(|note| note.contains("из предыдущего turn"))
    );
}

#[test]
fn current_thread_live_file_hints_use_latest_same_thread_file_events() {
    let events = vec![
        token_event! {
            thread_id: Some("thread-live".to_string()),
            traffic_class: "live".to_string(),
            target_kind: "file".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            quality_ok: true,
            query: "./src/dashboard.rs".to_string(),
            occurred_at_epoch_ms: 1_000,
        },
        token_event! {
            thread_id: Some("thread-live".to_string()),
            traffic_class: "live".to_string(),
            target_kind: "file".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            quality_ok: true,
            query: "./src/token_budget.rs".to_string(),
            occurred_at_epoch_ms: 2_000,
        },
        token_event! {
            thread_id: Some("thread-other".to_string()),
            traffic_class: "live".to_string(),
            target_kind: "file".to_string(),
            measurement_scope: "retrieval_lower_bound".to_string(),
            quality_ok: true,
            query: "./src/ignore.rs".to_string(),
            occurred_at_epoch_ms: 3_000,
        },
    ];

    let hints = super::build_current_thread_live_file_hints(
        &events,
        Some("art"),
        Some("continuity"),
        Some("thread-live"),
        5,
    );
    assert_eq!(hints["sample_count"], json!(2));
    assert_eq!(
        hints["hints"],
        json!([
            { "label": "dashboard.rs", "query": "./src/dashboard.rs" },
            { "label": "token_budget.rs", "query": "./src/token_budget.rs" }
        ])
    );
}

#[test]
fn live_response_latency_rolling_window_is_project_scoped_not_thread_scoped() {
    let repo_root = Path::new("/home/art/agent-memory-index");
    let events = vec![
        token_event! {
            project: "amai".to_string(),
            namespace: "continuity".to_string(),
            thread_id: Some("thread-current".to_string()),
            turn_id: Some("turn-1".to_string()),
            cold_warm_state: "warm".to_string(),
            latency_ms: 1.0,
            occurred_at_epoch_ms: 1_000,
            created_at_epoch_ms: 1_000,
        },
        token_event! {
            project: "amai".to_string(),
            namespace: "continuity".to_string(),
            thread_id: Some("thread-other".to_string()),
            turn_id: Some("turn-2".to_string()),
            cold_warm_state: "cold".to_string(),
            latency_ms: 4.0,
            occurred_at_epoch_ms: 2_000,
            created_at_epoch_ms: 2_000,
        },
        token_event! {
            project: "other".to_string(),
            namespace: "continuity".to_string(),
            thread_id: Some("thread-foreign".to_string()),
            turn_id: Some("turn-3".to_string()),
            cold_warm_state: "warm".to_string(),
            latency_ms: 9.0,
            occurred_at_epoch_ms: 3_000,
            created_at_epoch_ms: 3_000,
        },
    ];

    let surface = super::build_live_response_latency_surface(
        repo_root,
        Some("amai"),
        Some("continuity"),
        &events,
        Some(24),
        5,
        60,
        60_000,
    )
    .expect("surface");

    assert_eq!(surface["rolling_window"]["sample_count"], json!(2));
    assert_eq!(
        surface["rolling_window"]["latency_slices"][0]["sample_count"],
        json!(2)
    );
}

#[test]
fn dashboard_token_events_signature_changes_with_snapshot_summary() {
    let base = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(100),
    }];
    let changed_count = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 11,
        latest_created_at_epoch_ms: Some(100),
    }];
    let changed_latest = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(101),
    }];

    let base_sig = dashboard_token_events_signature_from_summary(&["token_budget_event"], &base);
    assert_ne!(
        base_sig,
        dashboard_token_events_signature_from_summary(&["token_budget_event"], &changed_count)
    );
    assert_ne!(
        base_sig,
        dashboard_token_events_signature_from_summary(&["token_budget_event"], &changed_latest)
    );
    assert_ne!(
        dashboard_token_events_signature_from_summary(&["token_budget_event"], &base),
        dashboard_token_events_signature_from_summary(&["token_budget_event"], &changed_count)
    );
}

#[test]
fn dashboard_token_events_signature_changes_with_snapshot_kinds() {
    let summary = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(100),
    }];

    assert_ne!(
        dashboard_token_events_signature_from_summary(&["token_budget_event"], &summary),
        dashboard_token_events_signature_from_summary(
            &["token_budget_event", "token_benchmark"],
            &summary,
        )
    );
}

#[test]
fn dashboard_token_events_signature_changes_with_invalidation_epoch() {
    let summary = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(100),
    }];

    let left = super::dashboard_token_events_signature(&["token_budget_event"], &summary, 0);
    let right = super::dashboard_token_events_signature(&["token_budget_event"], &summary, 1);

    assert_ne!(left, right);
}

#[test]
fn dashboard_event_snapshot_kinds_drop_verify_rows_for_default_live_path() {
    assert_eq!(
        super::dashboard_event_snapshot_kinds(false),
        &["token_budget_event"]
    );
    assert_eq!(
        super::dashboard_event_snapshot_kinds(true),
        &["token_budget_event", "token_benchmark"]
    );
}

#[test]
fn dashboard_token_events_delta_limit_detects_small_append_only_tail() {
    let previous = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(100),
    }];
    let current = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 12,
        latest_created_at_epoch_ms: Some(120),
    }];

    assert_eq!(
        dashboard_token_events_delta_limit(&previous, &current, 32),
        Some(2)
    );
}

#[test]
fn dashboard_token_events_delta_limit_rejects_non_append_changes() {
    let previous = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(100),
    }];
    let decreased = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 9,
        latest_created_at_epoch_ms: Some(100),
    }];
    let regressed_latest = vec![ObservabilitySnapshotKindSummary {
        snapshot_kind: "token_budget_event".to_string(),
        snapshots_count: 10,
        latest_created_at_epoch_ms: Some(99),
    }];

    assert_eq!(
        dashboard_token_events_delta_limit(&previous, &decreased, 32),
        None
    );
    assert_eq!(
        dashboard_token_events_delta_limit(&previous, &regressed_latest, 32),
        None
    );
}

#[test]
fn dashboard_assistant_scope_source_signature_changes_with_rollout_threads() {
    let target_context_pack_ids = BTreeSet::from(["ctx-a".to_string(), "ctx-b".to_string()]);
    let working_state_summary = DashboardAssistantScopeTargetedSummary {
        snapshot_kind: "working_state_event".to_string(),
        matched_rows: 10,
        latest_created_at_epoch_ms: Some(100),
    };
    let direct_turn_summary = DashboardAssistantScopeTargetedSummary {
        snapshot_kind: "assistant_generation_turn_observed".to_string(),
        matched_rows: 3,
        latest_created_at_epoch_ms: Some(90),
    };
    let token_budget_summary = DashboardAssistantScopeTargetedSummary {
        snapshot_kind: "token_budget_event".to_string(),
        matched_rows: 12,
        latest_created_at_epoch_ms: Some(110),
    };
    let left = dashboard_assistant_scope_source_signature(
        &target_context_pack_ids,
        &working_state_summary,
        &direct_turn_summary,
        &token_budget_summary,
        &BTreeMap::from([("thread-a".to_string(), "sig-a".to_string())]),
    );
    let right = dashboard_assistant_scope_source_signature(
        &target_context_pack_ids,
        &working_state_summary,
        &direct_turn_summary,
        &token_budget_summary,
        &BTreeMap::from([("thread-a".to_string(), "sig-b".to_string())]),
    );

    assert_ne!(left, right);
}

#[test]
fn dashboard_assistant_scope_source_signature_ignores_unrelated_global_snapshot_noise() {
    let target_context_pack_ids = BTreeSet::from(["ctx-a".to_string()]);
    let left = dashboard_assistant_scope_source_signature(
        &target_context_pack_ids,
        &DashboardAssistantScopeTargetedSummary {
            snapshot_kind: "working_state_event".to_string(),
            matched_rows: 4,
            latest_created_at_epoch_ms: Some(100),
        },
        &DashboardAssistantScopeTargetedSummary {
            snapshot_kind: "assistant_generation_turn_observed".to_string(),
            matched_rows: 1,
            latest_created_at_epoch_ms: Some(80),
        },
        &DashboardAssistantScopeTargetedSummary {
            snapshot_kind: "token_budget_event".to_string(),
            matched_rows: 5,
            latest_created_at_epoch_ms: Some(110),
        },
        &BTreeMap::from([("thread-a".to_string(), "sig-a".to_string())]),
    );
    let right = dashboard_assistant_scope_source_signature(
        &target_context_pack_ids,
        &DashboardAssistantScopeTargetedSummary {
            snapshot_kind: "working_state_event".to_string(),
            matched_rows: 4,
            latest_created_at_epoch_ms: Some(100),
        },
        &DashboardAssistantScopeTargetedSummary {
            snapshot_kind: "assistant_generation_turn_observed".to_string(),
            matched_rows: 1,
            latest_created_at_epoch_ms: Some(80),
        },
        &DashboardAssistantScopeTargetedSummary {
            snapshot_kind: "token_budget_event".to_string(),
            matched_rows: 5,
            latest_created_at_epoch_ms: Some(110),
        },
        &BTreeMap::from([("thread-a".to_string(), "sig-a".to_string())]),
    );

    assert_eq!(left, right);
}

#[test]
fn followup_recovery_is_attributed_to_successful_followup_event() {
    let reconciled = reconcile_followup_recovery(
        &[
            token_event! {
                created_at_epoch_ms: 1000,
                event_id: "event-1".to_string(),
                correlation_id: "event-1".to_string(),
                session_id: "session-1".to_string(),
                rolling_window_profile: "codex_5h".to_string(),
                timestamp_utc: 1000,
                occurred_at_epoch_ms: 1000,
                ingested_at_epoch_ms: 1000,
                query: "find dashboard token bug".to_string(),
                query_hash: "hash-1".to_string(),
                query_type: "code_lookup".to_string(),
                target_kind: "file".to_string(),
                baseline_hit_target: true,
                amai_hit_target: false,
                cold_warm_state: "cold".to_string(),
                baseline_strategy: "naive_top_files".to_string(),
                retrieval_mode: Some("local_strict".to_string()),
                tokenizer: "o200k_base".to_string(),
                latency_ms: 10.0,
                saved_tokens: 900,
                naive_tokens: 1000,
                context_tokens: 100,
                recovery_tokens: 0,
                effective_saved_tokens: 900,
                savings_factor: 10.0,
                savings_percent: 90.0,
                effective_savings_percent: 90.0,
                quality_ok: false,
                quality_score: 0.0,
                quality_method: "hybrid_retrieval_parity".to_string(),
                quality_tier: "retrieval".to_string(),
                head_hit_target: false,
                needed_followup: true,
                followup_count: 0,
                followup_of_event_id: None,
                resolved_by_event_id: None,
                fallback_triggered: false,
                fallback_count: 0,
                document_hits: 0,
                symbol_hits_count: 0,
                file_hits: 0,
                sources_count: 0,
                chunks_count: 0,
                pack_token_count: 100,
                deduped_token_count: 100,
            },
            token_event! {
                created_at_epoch_ms: 2000,
                event_id: "event-2".to_string(),
                correlation_id: "event-2".to_string(),
                session_id: "session-1".to_string(),
                rolling_window_profile: "codex_5h".to_string(),
                timestamp_utc: 2000,
                occurred_at_epoch_ms: 2000,
                ingested_at_epoch_ms: 2000,
                query: "dashboard token bug file".to_string(),
                query_hash: "hash-2".to_string(),
                query_type: "code_lookup".to_string(),
                target_kind: "file".to_string(),
                baseline_hit_target: true,
                amai_hit_target: true,
                cold_warm_state: "warm".to_string(),
                baseline_strategy: "naive_top_files".to_string(),
                retrieval_mode: Some("local_strict".to_string()),
                tokenizer: "o200k_base".to_string(),
                latency_ms: 4.0,
                saved_tokens: 800,
                naive_tokens: 1000,
                context_tokens: 120,
                recovery_tokens: 0,
                effective_saved_tokens: 800,
                savings_factor: 8.0,
                savings_percent: 80.0,
                effective_savings_percent: 80.0,
                quality_ok: true,
                quality_score: 1.0,
                quality_method: "hybrid_retrieval_parity".to_string(),
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
                pack_token_count: 120,
                deduped_token_count: 120,
            },
        ],
        30 * 60_000,
    );

    assert_eq!(reconciled[0].recovery_tokens, 0);
    assert_eq!(
        reconciled[0].resolved_by_event_id.as_deref(),
        Some("event-2")
    );
    assert_eq!(reconciled[1].recovery_tokens, 100);
    assert_eq!(reconciled[1].followup_count, 1);
    assert_eq!(
        reconciled[1].followup_of_event_id.as_deref(),
        Some("event-1")
    );
    assert_eq!(reconciled[1].effective_saved_tokens, 780);
    assert_eq!(reconciled[1].effective_savings_percent, 78.0);
}

#[test]
fn followup_query_matching_requires_same_shape_and_meaningful_overlap() {
    assert!(followup_queries_related(
        super::FollowupEventKey {
            query: "find dashboard token bug",
            query_hash: "",
            query_type: "code_lookup",
            target_kind: "file",
        },
        super::FollowupEventKey {
            query: "dashboard token bug file",
            query_hash: "",
            query_type: "code_lookup",
            target_kind: "file",
        },
    ));
    assert!(!followup_queries_related(
        super::FollowupEventKey {
            query: "find dashboard token bug",
            query_hash: "",
            query_type: "code_lookup",
            target_kind: "file",
        },
        super::FollowupEventKey {
            query: "dashboard config",
            query_hash: "",
            query_type: "config_lookup",
            target_kind: "file",
        },
    ));
    assert!(!followup_queries_related(
        super::FollowupEventKey {
            query: "find dashboard token bug",
            query_hash: "",
            query_type: "code_lookup",
            target_kind: "file",
        },
        super::FollowupEventKey {
            query: "dashboard token",
            query_hash: "",
            query_type: "code_lookup",
            target_kind: "symbol",
        },
    ));
}

#[test]
fn continuity_restore_observed_event_carries_prompt_meter() {
    let measurement = measurement_fixture();
    let contract = contract_fixture();
    let prompt_text = "CHAT_START_RESTORE\nProject: Art\n";
    let payload = build_continuity_restore_observed_event(
        "art",
        "continuity",
        "proof_continuity_startup",
        &measurement,
        &contract,
        prompt_text,
        37,
    )
    .expect("payload");
    let event = &payload["token_budget_event"];

    assert_eq!(
        event["measurement_scope"],
        "whole_cycle_observed_lower_bound"
    );
    assert_eq!(event["source_kind"], "proof_continuity_startup");
    assert_eq!(event["query_type"], "continuity_restore");
    assert_eq!(
        event["whole_cycle_observed"]["continuity_restore_tokens"],
        37
    );
    assert_eq!(event["baseline_tokens"], 0);
    assert_eq!(event["delivered_tokens"], 0);
    assert_eq!(event["quality"]["quality_ok"], true);
    assert_eq!(event["query_hash"], hex_sha256(prompt_text.as_bytes()));
}

#[test]
fn tool_overhead_token_counter_is_positive_for_mcp_payload() {
    let measurement = measurement_fixture();
    let tokens = count_tool_overhead_tokens(
        &measurement,
        "context pack built for art:continuity",
        &json!({
            "context_pack_summary": {
                "included_reasons_summary": "exact",
                "excluded_reasons_summary": "none"
            },
            "stats": {
                "context_pack_id": "ctx-pack-1",
                "exact_documents": 1
            }
        }),
    )
    .expect("token count");
    assert!(tokens > 0);
}

#[test]
fn cli_context_pack_output_overhead_counter_uses_total_output_minus_delivered_tokens() {
    let measurement = measurement_fixture();
    let output_json = r#"{"context_pack_id":"ctx-pack-1","retrieval":{"lexical_chunks":[{"text":"hello world"}]}}"#;
    let tokens = count_cli_context_pack_output_overhead_tokens(&measurement, output_json, 3)
        .expect("token count");
    assert!(tokens > 0);
}

#[test]
fn render_context_pack_prompt_compacts_same_thread_cache_reuse_payload() {
    let prompt = render_context_pack_prompt(&json!({
        "query": "Continuity snapshot",
        "effective_retrieval_mode": "local_strict",
        "visible_projects": [{
            "project_code": "art",
            "repo_root": "/home/art/Art"
        }],
        "retrieval": {
            "exact_documents": [],
            "symbol_hits": [],
            "lexical_chunks": [],
            "semantic_chunks": []
        },
        "cache_reuse_reference": {
            "state": "same_thread_context_pack_replay",
            "source_context_pack_id": "ctx-pack-1",
            "active_files": ["docs/continuity.md", "src/lib.rs"],
            "retrieval_counts": {
                "exact_documents": 1,
                "symbol_hits": 1,
                "lexical_chunks": 2,
                "semantic_chunks": 0
            }
        }
    }));

    assert!(prompt.contains("ctx=ctx-pack-1"));
    assert!(prompt.contains("file=docs/continuity.md"));
    assert!(prompt.contains("reuse=prior_full_payload_same_thread"));
    assert!(!prompt.contains("lexical excerpt"));
}

fn unique_test_repo_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("amai-token-budget-{name}-{nanos}"))
}

#[test]
fn provider_usage_source_uses_repo_local_default_path_when_env_missing() {
    unsafe {
        std::env::remove_var("AMAI_PROVIDER_USAGE_EXPORT_PATH");
    }
    let repo_root = unique_test_repo_root("provider-usage-default-missing");
    fs::create_dir_all(&repo_root).expect("create repo root");
    let source = external_truth::configured_provider_usage_source(&repo_root);
    assert_eq!(source["status"], "default_path_missing");
    assert_eq!(source["binding_status"], "not_configured");
    assert_eq!(
        source["resolved_path"],
        external_truth::provider_usage_default_path(&repo_root)
            .display()
            .to_string()
    );
    fs::remove_dir_all(&repo_root).expect("cleanup repo root");
}

#[test]
fn provider_rate_card_source_promotes_existing_repo_local_file() {
    unsafe {
        std::env::remove_var("AMAI_PROVIDER_RATE_CARD_PATH");
    }
    let repo_root = unique_test_repo_root("provider-rate-card-default-existing");
    let default_path = external_truth::provider_rate_card_default_path(&repo_root);
    fs::create_dir_all(default_path.parent().expect("parent")).expect("create state dir");
    fs::write(
        &default_path,
        r#"{
  "schema_version": "provider-rate-card-v1",
  "rate_card_version": "test-rate-card-v1",
  "currency_profile": "USD",
  "provider": "openai",
  "default_input_cost_per_1k_tokens": 2.0,
  "default_output_cost_per_1k_tokens": 1.0
}"#,
    )
    .expect("write rate card");
    let source = external_truth::configured_provider_rate_card_source(&repo_root);
    assert_eq!(source["status"], "default_existing_path");
    assert_eq!(source["binding_status"], "default_but_unbound");
    assert_eq!(source["resolved_path"], default_path.display().to_string());
    fs::remove_dir_all(&repo_root).expect("cleanup repo root");
}

#[test]
fn client_live_meter_thread_binding_state_marks_matching_current_thread() {
    let (state, bound) =
        super::client_live_meter_thread_binding_state(Some("thread-current"), "thread-current");
    assert_eq!(state, "current_thread_bound");
    assert!(bound);
}

#[test]
fn client_live_meter_thread_binding_state_marks_missing_current_thread_as_unbound() {
    let (state, bound) = super::client_live_meter_thread_binding_state(None, "thread-previous");
    assert_eq!(state, "no_current_thread_binding");
    assert!(!bound);
}

#[test]
fn client_live_meter_thread_binding_state_marks_mismatched_thread_as_unbound() {
    let (state, bound) =
        super::client_live_meter_thread_binding_state(Some("thread-current"), "thread-previous");
    assert_eq!(state, "current_thread_mismatch");
    assert!(!bound);
}

#[test]
fn build_client_live_meter_json_surfaces_rollout_jsonl_tolerance_summary() {
    let observation = crate::codex_threads::RolloutClientMeterObservation {
        thread_id: "thread-current".to_string(),
        rollout_path: "/tmp/rollout.jsonl".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_epoch_ms: 100,
        ended_at_epoch_ms: 200,
        first_assistant_response_at_epoch_ms: None,
        client_turn_total_tokens: 500,
        client_turn_input_tokens: 400,
        client_turn_cached_input_tokens: 100,
        client_turn_output_tokens: 50,
        client_turn_reasoning_output_tokens: 25,
        latest_cumulative_total_tokens: 1500,
        latest_model_context_window: 258400,
        latest_primary_limit_used_percent: 33,
        latest_secondary_limit_used_percent: 44,
        latest_primary_window_duration_mins: None,
        latest_primary_resets_at_epoch_seconds: None,
        latest_secondary_window_duration_mins: None,
        latest_secondary_resets_at_epoch_seconds: None,
        rollout_jsonl_tolerance_summary: crate::codex_threads::RolloutJsonlToleranceSummary {
            skipped_empty_or_nul_lines: 12,
            skipped_non_json_prefix_lines: 3,
            sanitized_nul_json_lines: 1,
        },
        observation_source: "codex_rollout_client_meter_v2".to_string(),
    };

    let live_meter =
        super::build_client_live_meter_json(Some(&observation), Some("thread-current"), None);

    assert_eq!(
        live_meter["rollout_jsonl_tolerance_summary"]["skipped_empty_or_nul_lines"],
        json!(12)
    );
    assert_eq!(
        live_meter["rollout_jsonl_tolerance_summary"]["skipped_non_json_prefix_lines"],
        json!(3)
    );
    assert_eq!(
        live_meter["rollout_jsonl_tolerance_summary"]["sanitized_nul_json_lines"],
        json!(1)
    );
    assert_eq!(
        live_meter["rollout_jsonl_tolerated_skips_present"],
        json!(true)
    );
    assert_eq!(
        live_meter["rollout_jsonl_malformed_objects_fail_closed"],
        json!(true)
    );
}

#[test]
fn dashboard_thread_binding_hint_accepts_fresh_matching_restore_and_lease() {
    let restore = json!({
        "restore_freshness_state": "fresh",
        "thread_id": "thread-live",
        "session_id": "session-live",
        "execctl_active_lease": {
            "lease_state": "active",
            "owner_thread_id": "thread-live",
            "owner_session_id": "session-live"
        }
    });
    assert_eq!(
        super::dashboard_thread_binding_hint_from_working_state_restore(&restore),
        Some("thread-live".to_string())
    );
}

#[test]
fn dashboard_thread_binding_hint_rejects_mismatched_active_lease_owner() {
    let restore = json!({
        "restore_freshness_state": "fresh",
        "thread_id": "thread-live",
        "session_id": "session-live",
        "execctl_active_lease": {
            "lease_state": "active",
            "owner_thread_id": "thread-other",
            "owner_session_id": "session-live"
        }
    });
    assert_eq!(
        super::dashboard_thread_binding_hint_from_working_state_restore(&restore),
        None
    );
}

#[test]
fn dashboard_thread_binding_hint_rejects_stale_restore() {
    let restore = json!({
        "restore_freshness_state": "stale",
        "thread_id": "thread-live",
        "session_id": "session-live",
        "execctl_active_lease": {
            "lease_state": "active",
            "owner_thread_id": "thread-live",
            "owner_session_id": "session-live"
        }
    });
    assert_eq!(
        super::dashboard_thread_binding_hint_from_working_state_restore(&restore),
        None
    );
}

#[test]
fn dashboard_report_signature_changes_when_client_live_meter_binding_hint_changes() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-a".to_string(),
        correlation_id: "ctx-a".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "q-a".to_string(),
        query_hash: "hash-a".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "naive_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 1.0,
        saved_tokens: 10,
        naive_tokens: 20,
        context_tokens: 10,
        recovery_tokens: 0,
        effective_saved_tokens: 10,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
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
        pack_token_count: 10,
        deduped_token_count: 10,
    }];
    let scope = AssistantGenerationScopeObservation::default();
    let meter = crate::codex_threads::RolloutClientMeterObservation {
        thread_id: "thread-live".to_string(),
        rollout_path: "/tmp/thread-live.jsonl".to_string(),
        turn_id: "turn-1".to_string(),
        started_at_epoch_ms: 10,
        ended_at_epoch_ms: 11,
        first_assistant_response_at_epoch_ms: None,
        client_turn_total_tokens: 100,
        client_turn_input_tokens: 80,
        client_turn_cached_input_tokens: 0,
        client_turn_output_tokens: 20,
        client_turn_reasoning_output_tokens: 0,
        latest_cumulative_total_tokens: 1000,
        latest_model_context_window: 258400,
        latest_primary_limit_used_percent: 10,
        latest_secondary_limit_used_percent: 20,
        latest_primary_window_duration_mins: None,
        latest_primary_resets_at_epoch_seconds: None,
        latest_secondary_window_duration_mins: None,
        latest_secondary_resets_at_epoch_seconds: None,
        rollout_jsonl_tolerance_summary:
            crate::codex_threads::RolloutJsonlToleranceSummary::default(),
        observation_source: "codex_rollout_client_meter_v2".to_string(),
    };
    let left = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        Some(&meter),
        Some("thread-live"),
        None,
        &json!({}),
        90,
    ));
    let right = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        Some(&meter),
        None,
        None,
        &json!({}),
        90,
    ));
    assert_ne!(left, right);
}

#[test]
fn dashboard_report_signature_changes_when_client_budget_target_changes() {
    let events = vec![token_event! {
        created_at_epoch_ms: 10,
        event_id: "event-a".to_string(),
        correlation_id: "ctx-a".to_string(),
        session_id: "session-1".to_string(),
        rolling_window_profile: "codex_5h".to_string(),
        timestamp_utc: 10,
        occurred_at_epoch_ms: 10,
        ingested_at_epoch_ms: 10,
        query: "q-a".to_string(),
        query_hash: "hash-a".to_string(),
        query_type: "code_lookup".to_string(),
        target_kind: "file".to_string(),
        baseline_hit_target: true,
        amai_hit_target: true,
        cold_warm_state: "warm".to_string(),
        baseline_strategy: "naive_top_files".to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        tokenizer: "o200k_base".to_string(),
        latency_ms: 1.0,
        saved_tokens: 10,
        naive_tokens: 20,
        context_tokens: 10,
        recovery_tokens: 0,
        effective_saved_tokens: 10,
        savings_factor: 2.0,
        savings_percent: 50.0,
        effective_savings_percent: 50.0,
        quality_ok: true,
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
        pack_token_count: 10,
        deduped_token_count: 10,
    }];
    let scope = AssistantGenerationScopeObservation::default();
    let left = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        None,
        &json!({}),
        90,
    ));
    let right = dashboard_report_signature(&dashboard_report_signature_components(
        &events,
        &events,
        &events,
        None,
        &events,
        &scope,
        Some(&scope),
        &scope,
        None,
        None,
        None,
        &json!({}),
        50,
    ));
    assert_ne!(left, right);
}

#[test]
fn scope_same_meter_exact_pair_surfaces_exact_tuple() {
    let scope_summary = json!({
        "client_limit_meter_alignment": {
            "same_meter_as_client_limit": true,
            "strict_client_meter_slice": {
                "lower_bound_tokens": 640
            }
        },
        "observed_whole_cycle_with_amai_tokens": 99
    });
    assert_eq!(
        super::scope_same_meter_exact_pair(&scope_summary),
        Some((640, 99, 541, 84.53125))
    );
}

#[test]
fn live_turn_token_budget_events_matches_thread_turn_and_context_pack_without_duplicates() {
    let events = vec![
        token_event! {
            event_id: "event-thread-turn".to_string(),
            thread_id: Some("thread-live".to_string()),
            turn_id: Some("turn-live".to_string()),
            context_pack_id: Some("ctx-1".to_string()),
            traffic_class: "live".to_string(),
        },
        token_event! {
            event_id: "event-context-pack".to_string(),
            thread_id: None,
            turn_id: None,
            context_pack_id: Some("ctx-2".to_string()),
            traffic_class: "live".to_string(),
        },
        token_event! {
            event_id: "event-non-live".to_string(),
            thread_id: Some("thread-live".to_string()),
            turn_id: Some("turn-live".to_string()),
            context_pack_id: Some("ctx-3".to_string()),
            traffic_class: "proof".to_string(),
        },
    ];
    let context_pack_ids = BTreeSet::from(["ctx-1".to_string(), "ctx-2".to_string()]);
    let matched = super::live_turn_token_budget_events(
        &events,
        "thread-live",
        "turn-live",
        &context_pack_ids,
        1_000,
        2_000,
        100,
    );
    let matched_ids = matched
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(matched_ids, vec!["event-thread-turn", "event-context-pack"]);
}

#[test]
fn live_turn_token_budget_events_matches_live_continuity_restore_by_thread_and_window() {
    let events = vec![
        token_event! {
            event_id: "event-continuity-window".to_string(),
            thread_id: Some("thread-live".to_string()),
            turn_id: None,
            traffic_class: "live".to_string(),
            measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
            query_type: "continuity_restore".to_string(),
            target_kind: "continuity_restore".to_string(),
            occurred_at_epoch_ms: 1_500,
            continuity_restore_tokens: Some(42),
        },
        token_event! {
            event_id: "event-continuity-outside-window".to_string(),
            thread_id: Some("thread-live".to_string()),
            turn_id: None,
            traffic_class: "live".to_string(),
            measurement_scope: "whole_cycle_observed_lower_bound".to_string(),
            query_type: "continuity_restore".to_string(),
            target_kind: "continuity_restore".to_string(),
            occurred_at_epoch_ms: 5_000,
            continuity_restore_tokens: Some(99),
        },
    ];
    let matched = super::live_turn_token_budget_events(
        &events,
        "thread-live",
        "turn-live",
        &BTreeSet::new(),
        1_000,
        2_000,
        100,
    );
    let matched_ids = matched
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(matched_ids, vec!["event-continuity-window"]);
}

#[test]
fn open_turn_pending_activity_surface_overrides_no_activity_state() {
    let mut surface = json!({
        "status": "no_amai_activity_in_current_live_turn",
        "matched_context_pack_ids_count": 0,
        "retrieval_context_pack_count": 0
    });

    assert!(super::apply_open_turn_pending_activity_surface(
        &mut surface,
        2,
        3
    ));
    assert_eq!(
        surface["status"],
        json!("thread_activity_observed_turn_open")
    );
    assert_eq!(surface["matched_context_pack_ids_count"], json!(2));
    assert_eq!(surface["retrieval_context_pack_count"], json!(3));
    assert!(
        surface["note"]
            .as_str()
            .is_some_and(|note| note.contains("после последнего завершённого client-meter turn"))
    );
}

#[test]
fn exact_client_limit_hourly_burn_value_classifies_saving() {
    let samples = vec![super::ExactClientLimitSample {
        observed_at_epoch_ms: 1_800_000,
        primary_used_percent: 25.0,
        primary_window_duration_mins: Some(60),
        primary_resets_at_epoch_seconds: Some(3_600),
        source: "codex_app_server_account_rate_limits_read_v1".to_string(),
    }];
    let value = super::exact_client_limit_hourly_burn_value(&samples, 1_800_000, 60, 10, 0);
    assert_eq!(value["status"].as_str(), Some("observed"));
    assert_eq!(value["classification"].as_str(), Some("saving"));
    assert_eq!(
        value["reply_prefix"].as_str(),
        Some("5ч KPI: экономия 50.00%")
    );
}

#[test]
fn exact_client_limit_hourly_burn_value_classifies_one_to_one() {
    let samples = vec![super::ExactClientLimitSample {
        observed_at_epoch_ms: 1_800_000,
        primary_used_percent: 50.0,
        primary_window_duration_mins: Some(60),
        primary_resets_at_epoch_seconds: Some(3_600),
        source: "codex_app_server_account_rate_limits_read_v1".to_string(),
    }];
    let value = super::exact_client_limit_hourly_burn_value(&samples, 1_800_000, 60, 10, 0);
    assert_eq!(value["status"].as_str(), Some("observed"));
    assert_eq!(value["classification"].as_str(), Some("one_to_one"));
    assert_eq!(value["reply_prefix"].as_str(), Some("5ч KPI: 1:1"));
}

#[test]
fn exact_client_limit_hourly_burn_value_classifies_overspend() {
    let samples = vec![super::ExactClientLimitSample {
        observed_at_epoch_ms: 1_800_000,
        primary_used_percent: 75.0,
        primary_window_duration_mins: Some(60),
        primary_resets_at_epoch_seconds: Some(3_600),
        source: "codex_app_server_account_rate_limits_read_v1".to_string(),
    }];
    let value = super::exact_client_limit_hourly_burn_value(&samples, 1_800_000, 60, 10, 0);
    assert_eq!(value["status"].as_str(), Some("observed"));
    assert_eq!(value["classification"].as_str(), Some("overspend"));
    assert_eq!(
        value["reply_prefix"].as_str(),
        Some("5ч KPI: переплата 50.00%")
    );
}

#[test]
fn exact_client_limit_hourly_burn_value_damps_before_min_history_span() {
    let samples = vec![super::ExactClientLimitSample {
        observed_at_epoch_ms: 300_000,
        primary_used_percent: 11.0,
        primary_window_duration_mins: Some(300),
        primary_resets_at_epoch_seconds: Some(18_000),
        source: "codex_app_server_account_rate_limits_read_v1".to_string(),
    }];
    let value = super::exact_client_limit_hourly_burn_value(&samples, 300_000, 60, 10, 55);
    assert_eq!(value["status"].as_str(), Some("observed"));
    assert_eq!(value["window_progress_state"].as_str(), Some("preliminary"));
    assert_eq!(
        value["reply_prefix"].as_str(),
        Some("5ч KPI: переплата 50.91%")
    );
    assert_eq!(value["minimum_elapsed_window_minutes"].as_u64(), Some(55));
    assert!(
        value["raw_kpi_percent"]
            .as_f64()
            .is_some_and(|value| (value - 560.0).abs() < 0.001)
    );
}

#[test]
fn codex_app_server_rate_limits_response_accepts_numeric_strings() {
    let payload = json!({
        "rateLimits": {
            "limitId": "codex",
            "limitName": null,
            "primary": {
                "usedPercent": "3",
                "windowDurationMins": "300",
                "resetsAt": "1774805186"
            },
            "secondary": {
                "usedPercent": 1,
                "windowDurationMins": 10080,
                "resetsAt": 1775391986
            },
            "credits": {
                "hasCredits": false,
                "unlimited": false,
                "balance": "0"
            },
            "planType": "plus"
        }
    });

    let parsed: super::CodexAppServerRateLimitsResponse =
        serde_json::from_value(payload).expect("parse codex app-server rate limits response");
    let primary = parsed
        .rate_limits
        .primary
        .as_ref()
        .expect("primary rate-limit window");
    let credits = parsed
        .rate_limits
        .credits
        .as_ref()
        .expect("credits snapshot");

    assert_eq!(primary.used_percent, 3.0);
    assert_eq!(primary.window_duration_mins, Some(300));
    assert_eq!(primary.resets_at, Some(1_774_805_186));
    assert_eq!(credits.balance, Some(0.0));
}

#[test]
fn client_limit_trend_direction_marks_overspend_growth_as_away_from_saving() {
    assert_eq!(
        super::client_limit_trend_direction(Some("overspend"), 35.0, Some("overspend"), 42.0),
        "away_from_saving"
    );
}

#[test]
fn client_limit_trend_direction_marks_overspend_drop_as_toward_saving() {
    assert_eq!(
        super::client_limit_trend_direction(Some("overspend"), 42.0, Some("overspend"), 35.0),
        "toward_saving"
    );
}

#[test]
fn client_limit_trend_direction_marks_stronger_saving_as_toward_saving() {
    assert_eq!(
        super::client_limit_trend_direction(Some("saving"), 12.0, Some("saving"), 28.0),
        "toward_saving"
    );
}

#[test]
fn best_effort_exact_client_limit_observation_from_result_prefers_live_observation() {
    let cached = super::CodexAppServerRateLimitsObservation {
        observed_at_epoch_ms: 100,
        rate_limits: sample_rate_limits_snapshot(10.0),
    };
    let live = super::CodexAppServerRateLimitsObservation {
        observed_at_epoch_ms: 200,
        rate_limits: sample_rate_limits_snapshot(20.0),
    };
    let selected = super::best_effort_exact_client_limit_observation_from_result(
        Ok(Some(live.clone())),
        Some(cached),
    );
    let selected = selected.expect("selected observation");
    assert_eq!(selected.observed_at_epoch_ms, live.observed_at_epoch_ms);
    assert_eq!(
        selected
            .rate_limits
            .primary
            .as_ref()
            .map(|window| window.used_percent),
        live.rate_limits
            .primary
            .as_ref()
            .map(|window| window.used_percent)
    );
}

#[test]
fn best_effort_exact_client_limit_observation_from_result_falls_back_to_cached_on_error() {
    let cached = super::CodexAppServerRateLimitsObservation {
        observed_at_epoch_ms: 100,
        rate_limits: sample_rate_limits_snapshot(10.0),
    };
    let selected = super::best_effort_exact_client_limit_observation_from_result(
        Err(anyhow!("timeout")),
        Some(cached.clone()),
    );
    let selected = selected.expect("cached observation");
    assert_eq!(selected.observed_at_epoch_ms, cached.observed_at_epoch_ms);
    assert_eq!(
        selected
            .rate_limits
            .primary
            .as_ref()
            .map(|window| window.used_percent),
        cached
            .rate_limits
            .primary
            .as_ref()
            .map(|window| window.used_percent)
    );
}

#[test]
fn shared_dashboard_exact_client_limits_cache_reuses_fresh_observation() {
    let temp_root =
        std::env::temp_dir().join(format!("amai-exact-client-limits-cache-{}", Uuid::new_v4()));
    let cache_path = temp_root.join("state/token_budget/exact_client_limits_cache.json");
    let entry = super::DashboardExactClientLimitsCache {
        fetched_at_epoch_ms: 5_000,
        observation: Some(super::CodexAppServerRateLimitsObservation {
            observed_at_epoch_ms: 4_900,
            rate_limits: sample_rate_limits_snapshot(33.0),
        }),
    };
    super::write_shared_dashboard_exact_client_limits_cache(&cache_path, &entry)
        .expect("write shared cache");
    let loaded = super::load_shared_dashboard_exact_client_limits_cache(&cache_path, 7_500)
        .expect("fresh shared cache");
    assert_eq!(loaded.fetched_at_epoch_ms, entry.fetched_at_epoch_ms);
    assert_eq!(
        loaded.observation.as_ref().and_then(|observation| {
            observation
                .rate_limits
                .primary
                .as_ref()
                .map(|window| window.used_percent)
        }),
        Some(33.0)
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_exact_client_limits_cache_rejects_fresh_missing_entry() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-exact-client-limits-cache-missing-{}",
        Uuid::new_v4()
    ));
    let cache_path = temp_root.join("state/token_budget/exact_client_limits_cache.json");
    let entry = super::DashboardExactClientLimitsCache {
        fetched_at_epoch_ms: 5_000,
        observation: None,
    };
    super::write_shared_dashboard_exact_client_limits_cache(&cache_path, &entry)
        .expect("write shared missing cache");
    assert!(
        super::load_shared_dashboard_exact_client_limits_cache(&cache_path, 7_500).is_none()
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn fresh_dashboard_exact_client_limits_cache_entry_rejects_missing_observation() {
    crate::token_budget::dashboard_exact_client_limits::set_dashboard_exact_client_limits_cache(
        super::DashboardExactClientLimitsCache {
        fetched_at_epoch_ms: 5_000,
        observation: None,
    });
    assert!(
        crate::token_budget::dashboard_exact_client_limits::fresh_dashboard_exact_client_limits_cache_entry(7_500)
            .is_none()
    );
}

#[test]
fn shared_dashboard_exact_client_limits_cache_fail_closed_when_stale() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-exact-client-limits-cache-stale-{}",
        Uuid::new_v4()
    ));
    let cache_path = temp_root.join("state/token_budget/exact_client_limits_cache.json");
    let entry = super::DashboardExactClientLimitsCache {
        fetched_at_epoch_ms: 5_000,
        observation: Some(super::CodexAppServerRateLimitsObservation {
            observed_at_epoch_ms: 4_900,
            rate_limits: sample_rate_limits_snapshot(33.0),
        }),
    };
    super::write_shared_dashboard_exact_client_limits_cache(&cache_path, &entry)
        .expect("write shared cache");
    assert!(
        super::load_shared_dashboard_exact_client_limits_cache(
            &cache_path,
            5_000 + super::DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS + 1
        )
        .is_none()
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_current_session_events_cache_roundtrips() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-current-session-events-cache-{}",
        Uuid::new_v4()
    ));
    let entry = super::DashboardCurrentSessionEventsCache {
        repo_root: temp_root.clone(),
        signature: "sig-a".to_string(),
        session_gap_ms: 60_000,
        events: vec![token_event! {
            event_id: "event-a".to_string(),
            session_id: "session-a".to_string(),
            created_at_epoch_ms: 100,
            occurred_at_epoch_ms: 100,
            ingested_at_epoch_ms: 100,
        }],
    };
    super::write_shared_dashboard_current_session_events(&entry)
        .expect("write current-session cache");
    let loaded = super::load_shared_dashboard_current_session_events(&temp_root, "sig-a", 60_000)
        .expect("load current-session cache");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].event_id, "event-a");
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_current_session_events_cache_rejects_gap_mismatch() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-current-session-events-cache-gap-{}",
        Uuid::new_v4()
    ));
    let entry = super::DashboardCurrentSessionEventsCache {
        repo_root: temp_root.clone(),
        signature: "sig-a".to_string(),
        session_gap_ms: 60_000,
        events: vec![token_event! {
            event_id: "event-a".to_string(),
            session_id: "session-a".to_string(),
            created_at_epoch_ms: 100,
            occurred_at_epoch_ms: 100,
            ingested_at_epoch_ms: 100,
        }],
    };
    super::write_shared_dashboard_current_session_events(&entry)
        .expect("write current-session cache");
    assert!(
        super::load_shared_dashboard_current_session_events(&temp_root, "sig-a", 30_000).is_none()
    );
    assert!(
        super::load_shared_dashboard_current_session_events(&temp_root, "sig-b", 60_000).is_none()
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_live_turn_retrieval_cache_roundtrips() {
    let temp_root =
        std::env::temp_dir().join(format!("amai-live-turn-retrieval-cache-{}", Uuid::new_v4()));
    let context_pack_ids = BTreeSet::from(["ctx-a".to_string(), "ctx-b".to_string()]);
    super::write_shared_dashboard_live_turn_retrieval_invalidation(&temp_root, 321)
        .expect("write invalidation");
    super::store_dashboard_live_turn_retrieval(
        &temp_root,
        "thread-a",
        100,
        200,
        321,
        &context_pack_ids,
        2,
    );
    let loaded = super::cached_dashboard_live_turn_retrieval(&temp_root, "thread-a", 100, 200, 321)
        .expect("load live-turn retrieval cache");
    assert_eq!(loaded.0, context_pack_ids);
    assert_eq!(loaded.1, 2);
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_live_turn_retrieval_cache_rejects_invalidation_mismatch() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-live-turn-retrieval-cache-stale-{}",
        Uuid::new_v4()
    ));
    let context_pack_ids = BTreeSet::from(["ctx-a".to_string()]);
    super::write_shared_dashboard_live_turn_retrieval_invalidation(&temp_root, 321)
        .expect("write invalidation");
    super::store_dashboard_live_turn_retrieval(
        &temp_root,
        "thread-a",
        100,
        200,
        321,
        &context_pack_ids,
        1,
    );
    super::write_shared_dashboard_live_turn_retrieval_invalidation(&temp_root, 654)
        .expect("advance invalidation");
    assert!(
        super::cached_dashboard_live_turn_retrieval(&temp_root, "thread-a", 100, 200, 654)
            .is_none()
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_live_turn_retrieval_cache_reuses_expanded_window_without_invalidation() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-live-turn-retrieval-cache-expand-{}",
        Uuid::new_v4()
    ));
    let context_pack_ids = BTreeSet::from(["ctx-a".to_string(), "ctx-b".to_string()]);
    super::write_shared_dashboard_live_turn_retrieval_invalidation(&temp_root, 321)
        .expect("write invalidation");
    super::store_dashboard_live_turn_retrieval(
        &temp_root,
        "thread-a",
        100,
        200,
        321,
        &context_pack_ids,
        2,
    );
    let loaded = super::cached_dashboard_live_turn_retrieval(&temp_root, "thread-a", 100, 260, 321)
        .expect("load expanded live-turn retrieval cache");
    assert_eq!(loaded.0, context_pack_ids);
    assert_eq!(loaded.1, 2);
    let persisted = super::load_shared_dashboard_live_turn_retrieval_cache(&temp_root)
        .expect("persisted live-turn retrieval cache");
    assert_eq!(persisted.upper_bound, 260);
    assert_eq!(persisted.retrieval_count, 2);
    assert_eq!(persisted.context_pack_ids, context_pack_ids);
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_dashboard_live_turn_retrieval_invalidation_keeps_latest_epoch() {
    let temp_root = std::env::temp_dir().join(format!(
        "amai-live-turn-retrieval-invalidation-{}",
        Uuid::new_v4()
    ));
    super::bump_dashboard_live_turn_retrieval_invalidation(&temp_root, 321).expect("first bump");
    super::bump_dashboard_live_turn_retrieval_invalidation(&temp_root, 123).expect("older bump");
    assert_eq!(
        super::current_dashboard_live_turn_retrieval_invalidation_epoch_ms(&temp_root),
        321
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_active_thread_hint_roundtrips_when_fresh() {
    let temp_root =
        std::env::temp_dir().join(format!("amai-active-thread-hint-{}", Uuid::new_v4()));
    let path = super::active_thread_hint_shared_cache_path(&temp_root);
    fs::create_dir_all(path.parent().expect("active-thread-hint parent"))
        .expect("create active-thread-hint parent");
    let payload = super::PersistedActiveThreadHintCache {
        cache_version: super::ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION.to_string(),
        updated_at_epoch_ms: 5_000,
        thread_id: "thread-current".to_string(),
    };
    fs::write(
        &path,
        serde_json::to_vec(&payload).expect("serialize active-thread-hint"),
    )
    .expect("write active-thread-hint");
    assert_eq!(
        super::load_shared_active_thread_hint(&temp_root, 5_001).as_deref(),
        Some("thread-current")
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn shared_active_thread_hint_fails_closed_when_stale() {
    let temp_root =
        std::env::temp_dir().join(format!("amai-active-thread-hint-stale-{}", Uuid::new_v4()));
    let path = super::active_thread_hint_shared_cache_path(&temp_root);
    fs::create_dir_all(path.parent().expect("active-thread-hint parent"))
        .expect("create active-thread-hint parent");
    let payload = super::PersistedActiveThreadHintCache {
        cache_version: super::ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION.to_string(),
        updated_at_epoch_ms: 5_000,
        thread_id: "thread-current".to_string(),
    };
    fs::write(
        &path,
        serde_json::to_vec(&payload).expect("serialize active-thread-hint"),
    )
    .expect("write active-thread-hint");
    assert!(
        super::load_shared_active_thread_hint(
            &temp_root,
            5_000 + super::ACTIVE_THREAD_HINT_MAX_AGE_MS + 1,
        )
        .is_none()
    );
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn continuity_restore_observed_dedupe_roundtrips_while_fresh() {
    let temp_root =
        std::env::temp_dir().join(format!("amai-continuity-restore-dedupe-{}", Uuid::new_v4()));
    super::write_continuity_restore_observed_dedupe_cache(
        &temp_root,
        "amai",
        "continuity",
        "restore_observed",
        "prompt-hash-1",
        5_000,
    )
    .expect("write continuity restore dedupe cache");
    assert!(super::continuity_restore_observed_event_recently_recorded(
        &temp_root,
        "amai",
        "continuity",
        "restore_observed",
        "prompt-hash-1",
        5_100,
    ));
    let _ = fs::remove_dir_all(&temp_root);
}

fn sample_rate_limits_snapshot(
    primary_used_percent: f64,
) -> super::CodexAppServerRateLimitSnapshot {
    super::CodexAppServerRateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: Some("Codex".to_string()),
        plan_type: Some("plus".to_string()),
        credits: None,
        primary: Some(super::CodexAppServerRateLimitWindow {
            window_duration_mins: Some(300),
            resets_at: Some(1_000),
            used_percent: primary_used_percent,
        }),
        secondary: None,
    }
}
