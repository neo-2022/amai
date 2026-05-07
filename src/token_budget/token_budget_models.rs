use crate::codex_threads;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TokenBudgetConfigFile {
    pub(crate) default_profile: String,
    pub(crate) measurement: MeasurementConfig,
    #[serde(default)]
    pub(crate) contract: TokenBudgetContractConfig,
    #[serde(default)]
    pub(crate) profiles: BTreeMap<String, TokenBudgetProfile>,
    #[serde(default)]
    pub(crate) client_budget_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedTokenBudgetConfig {
    pub(crate) size_bytes: u64,
    pub(crate) modified_epoch_ms: u64,
    pub(crate) config: TokenBudgetConfigFile,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MeasurementConfig {
    pub(crate) tokenizer: String,
    pub(crate) naive_limit_files: usize,
    pub(crate) naive_max_bytes_per_file: usize,
    #[serde(default)]
    pub(crate) include_verify_events_by_default: bool,
    #[serde(default = "default_metering_ingest_warning_seconds")]
    pub(crate) metering_ingest_warning_seconds: u64,
    #[serde(default = "default_metering_ingest_slo_seconds")]
    pub(crate) metering_ingest_slo_seconds: u64,
    #[serde(default = "default_late_arrival_grace_minutes")]
    pub(crate) late_arrival_grace_minutes: u64,
    #[serde(default = "default_preliminary_min_events")]
    pub(crate) preliminary_min_events: u64,
    #[serde(default = "default_preliminary_min_baseline_tokens")]
    pub(crate) preliminary_min_baseline_tokens: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TokenBudgetContractConfig {
    #[serde(default = "default_usage_event_schema_version")]
    pub(crate) usage_event_schema_version: String,
    #[serde(default = "default_settlement_statement_version")]
    pub(crate) settlement_statement_version: String,
    #[serde(default = "default_metering_event_schema_version")]
    pub(crate) metering_event_schema_version: String,
    #[serde(default = "default_usage_lifecycle_model_version")]
    pub(crate) usage_lifecycle_model_version: String,
    #[serde(default = "default_baseline_method_version")]
    pub(crate) baseline_method_version: String,
    #[serde(default = "default_quality_method_version")]
    pub(crate) quality_method_version: String,
    #[serde(default = "default_coverage_model_version")]
    pub(crate) coverage_model_version: String,
    #[serde(default = "default_metering_freshness_model_version")]
    pub(crate) metering_freshness_model_version: String,
    #[serde(default = "default_agent_cycle_model_version")]
    pub(crate) agent_cycle_model_version: String,
    #[serde(default = "default_client_limit_meter_alignment_version")]
    pub(crate) client_limit_meter_alignment_version: String,
    #[serde(default = "default_client_limit_baseline_equivalence_version")]
    pub(crate) client_limit_baseline_equivalence_version: String,
    #[serde(default = "default_client_limit_strict_meter_slice_version")]
    pub(crate) client_limit_strict_meter_slice_version: String,
    #[serde(default = "default_client_limit_explicit_boundary_surface_version")]
    pub(crate) client_limit_explicit_boundary_surface_version: String,
    #[serde(default = "default_client_limit_continuity_boundary_rollup_version")]
    pub(crate) client_limit_continuity_boundary_rollup_version: String,
    #[serde(default = "default_client_limit_pre_amai_baseline_source_version")]
    pub(crate) client_limit_pre_amai_baseline_source_version: String,
    #[serde(default = "default_client_limit_frozen_gap_review_surface_version")]
    pub(crate) client_limit_frozen_gap_review_surface_version: String,
    #[serde(default = "default_client_limit_reviewed_frozen_debt_export_surface_version")]
    pub(crate) client_limit_reviewed_frozen_debt_export_surface_version: String,
    #[serde(default = "default_excluded_taxonomy_version")]
    pub(crate) excluded_taxonomy_version: String,
    #[serde(default = "default_dedup_contract_version")]
    pub(crate) dedup_contract_version: String,
    #[serde(default = "default_backfill_policy_version")]
    pub(crate) backfill_policy_version: String,
    #[serde(default = "default_correction_policy_version")]
    pub(crate) correction_policy_version: String,
    #[serde(default = "default_freeze_close_policy_version")]
    pub(crate) freeze_close_policy_version: String,
    #[serde(default = "default_late_arrival_policy_version")]
    pub(crate) late_arrival_policy_version: String,
    #[serde(default = "default_dispute_policy_version")]
    pub(crate) dispute_policy_version: String,
    #[serde(default = "default_settlement_lifecycle_model_version")]
    pub(crate) settlement_lifecycle_model_version: String,
    #[serde(default = "default_statement_period_governance_version")]
    pub(crate) statement_period_governance_version: String,
    #[serde(default = "default_adjustment_preview_model_version")]
    pub(crate) adjustment_preview_model_version: String,
    #[serde(default = "default_adjustment_request_schema_version")]
    pub(crate) adjustment_request_schema_version: String,
    #[serde(default = "default_adjustment_registry_version")]
    pub(crate) adjustment_registry_version: String,
    #[serde(default = "default_rate_card_binding_model_version")]
    pub(crate) rate_card_binding_model_version: String,
    #[serde(default = "default_infra_cost_binding_model_version")]
    pub(crate) infra_cost_binding_model_version: String,
    #[serde(default = "default_telemetry_surface_split_version")]
    pub(crate) telemetry_surface_split_version: String,
    #[serde(default = "default_event_time_policy_version")]
    pub(crate) event_time_policy_version: String,
    #[serde(default = "default_billing_policy_version")]
    pub(crate) billing_policy_version: String,
    #[serde(default = "default_suitability_model_version")]
    pub(crate) suitability_model_version: String,
    #[serde(default = "default_contractual_readiness_model_version")]
    pub(crate) contractual_readiness_model_version: String,
    #[serde(default = "default_customer_contractual_boundary_version")]
    pub(crate) customer_contractual_boundary_version: String,
    #[serde(default = "default_settlement_activation_governance_version")]
    pub(crate) settlement_activation_governance_version: String,
    #[serde(default = "default_adjustment_activation_governance_version")]
    pub(crate) adjustment_activation_governance_version: String,
    #[serde(default = "default_billing_mode")]
    pub(crate) billing_mode: String,
    #[serde(default = "default_reconciliation_contract_version")]
    pub(crate) reconciliation_contract_version: String,
    #[serde(default = "default_margin_model_version")]
    pub(crate) margin_model_version: String,
    #[serde(default = "default_infra_cost_profile_version")]
    pub(crate) infra_cost_profile_version: String,
    #[serde(default = "default_contractual_evidence_pack_version")]
    pub(crate) contractual_evidence_pack_version: String,
    #[serde(default = "default_contractual_statement_export_version")]
    pub(crate) contractual_statement_export_version: String,
    #[serde(default = "default_settlement_report_preview_version")]
    pub(crate) settlement_report_preview_version: String,
    #[serde(default = "default_rate_card_version")]
    pub(crate) rate_card_version: String,
    #[serde(default = "default_currency_profile")]
    pub(crate) currency_profile: String,
    #[serde(default = "default_settlement_status")]
    pub(crate) settlement_status: String,
}

impl Default for TokenBudgetContractConfig {
    fn default() -> Self {
        Self {
            usage_event_schema_version: default_usage_event_schema_version(),
            settlement_statement_version: default_settlement_statement_version(),
            metering_event_schema_version: default_metering_event_schema_version(),
            usage_lifecycle_model_version: default_usage_lifecycle_model_version(),
            baseline_method_version: default_baseline_method_version(),
            quality_method_version: default_quality_method_version(),
            coverage_model_version: default_coverage_model_version(),
            metering_freshness_model_version: default_metering_freshness_model_version(),
            agent_cycle_model_version: default_agent_cycle_model_version(),
            client_limit_meter_alignment_version: default_client_limit_meter_alignment_version(),
            client_limit_baseline_equivalence_version:
                default_client_limit_baseline_equivalence_version(),
            client_limit_strict_meter_slice_version:
                default_client_limit_strict_meter_slice_version(),
            client_limit_explicit_boundary_surface_version:
                default_client_limit_explicit_boundary_surface_version(),
            client_limit_continuity_boundary_rollup_version:
                default_client_limit_continuity_boundary_rollup_version(),
            client_limit_pre_amai_baseline_source_version:
                default_client_limit_pre_amai_baseline_source_version(),
            client_limit_frozen_gap_review_surface_version:
                default_client_limit_frozen_gap_review_surface_version(),
            client_limit_reviewed_frozen_debt_export_surface_version:
                default_client_limit_reviewed_frozen_debt_export_surface_version(),
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
            infra_cost_binding_model_version: default_infra_cost_binding_model_version(),
            telemetry_surface_split_version: default_telemetry_surface_split_version(),
            event_time_policy_version: default_event_time_policy_version(),
            billing_policy_version: default_billing_policy_version(),
            suitability_model_version: default_suitability_model_version(),
            contractual_readiness_model_version: default_contractual_readiness_model_version(),
            customer_contractual_boundary_version: default_customer_contractual_boundary_version(),
            settlement_activation_governance_version:
                default_settlement_activation_governance_version(),
            adjustment_activation_governance_version:
                default_adjustment_activation_governance_version(),
            billing_mode: default_billing_mode(),
            reconciliation_contract_version: default_reconciliation_contract_version(),
            margin_model_version: default_margin_model_version(),
            infra_cost_profile_version: default_infra_cost_profile_version(),
            contractual_evidence_pack_version: default_contractual_evidence_pack_version(),
            contractual_statement_export_version: default_contractual_statement_export_version(),
            settlement_report_preview_version: default_settlement_report_preview_version(),
            rate_card_version: default_rate_card_version(),
            currency_profile: default_currency_profile(),
            settlement_status: default_settlement_status(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TokenBudgetProfile {
    pub(crate) display_name: String,
    pub(crate) description: String,
    pub(crate) session_gap_minutes: u64,
    pub(crate) rolling_window_hours: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedProfile {
    pub(crate) code: String,
    pub(crate) display_name: String,
    pub(crate) description: String,
    pub(crate) session_gap_minutes: u64,
    pub(crate) rolling_window_hours: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TokenBudgetEvent {
    pub(crate) snapshot_id: Option<Uuid>,
    pub(crate) created_at_epoch_ms: i64,
    pub(crate) event_id: String,
    pub(crate) correlation_id: String,
    pub(crate) context_pack_id: Option<String>,
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) agent_scope: String,
    pub(crate) payload_origin: String,
    pub(crate) session_id: String,
    pub(crate) rolling_window_profile: String,
    pub(crate) timestamp_utc: i64,
    pub(crate) occurred_at_epoch_ms: i64,
    pub(crate) ingested_at_epoch_ms: i64,
    pub(crate) snapshot_kind: String,
    pub(crate) source_kind: String,
    pub(crate) traffic_class: String,
    pub(crate) measurement_scope: String,
    pub(crate) usage_event_schema_version: String,
    pub(crate) settlement_statement_version: String,
    pub(crate) metering_event_schema_version: String,
    pub(crate) usage_lifecycle_model_version: String,
    pub(crate) baseline_method_version: String,
    pub(crate) quality_method_version: String,
    pub(crate) coverage_model_version: String,
    pub(crate) metering_freshness_model_version: String,
    pub(crate) excluded_taxonomy_version: String,
    pub(crate) dedup_contract_version: String,
    pub(crate) backfill_policy_version: String,
    pub(crate) correction_policy_version: String,
    pub(crate) freeze_close_policy_version: String,
    pub(crate) late_arrival_policy_version: String,
    pub(crate) dispute_policy_version: String,
    pub(crate) settlement_lifecycle_model_version: String,
    pub(crate) statement_period_governance_version: String,
    pub(crate) adjustment_preview_model_version: String,
    pub(crate) adjustment_request_schema_version: String,
    pub(crate) adjustment_registry_version: String,
    pub(crate) rate_card_binding_model_version: String,
    pub(crate) telemetry_surface_split_version: String,
    pub(crate) event_time_policy_version: String,
    pub(crate) billing_policy_version: String,
    pub(crate) suitability_model_version: String,
    pub(crate) billing_mode: String,
    pub(crate) reconciliation_contract_version: String,
    pub(crate) margin_model_version: String,
    pub(crate) infra_cost_profile_version: String,
    pub(crate) contractual_evidence_pack_version: String,
    pub(crate) rate_card_version: String,
    pub(crate) currency_profile: String,
    pub(crate) settlement_status: String,
    pub(crate) project: String,
    pub(crate) namespace: String,
    pub(crate) query: String,
    pub(crate) query_hash: String,
    pub(crate) query_type: String,
    pub(crate) target_kind: String,
    pub(crate) baseline_hit_target: bool,
    pub(crate) amai_hit_target: bool,
    pub(crate) cold_warm_state: String,
    pub(crate) baseline_strategy: String,
    pub(crate) retrieval_mode: Option<String>,
    pub(crate) retrieval_scope_signature: Option<String>,
    pub(crate) tokenizer: String,
    pub(crate) latency_ms: f64,
    pub(crate) saved_tokens: u64,
    pub(crate) naive_tokens: u64,
    pub(crate) context_tokens: u64,
    pub(crate) recovery_tokens: u64,
    pub(crate) effective_saved_tokens: i64,
    pub(crate) savings_factor: f64,
    pub(crate) savings_percent: f64,
    pub(crate) effective_savings_percent: f64,
    pub(crate) quality_ok: bool,
    pub(crate) quality_score: f64,
    pub(crate) quality_method: String,
    pub(crate) quality_tier: String,
    pub(crate) head_hit_target: bool,
    pub(crate) needed_followup: bool,
    pub(crate) followup_count: u64,
    pub(crate) followup_of_event_id: Option<String>,
    pub(crate) resolved_by_event_id: Option<String>,
    pub(crate) fallback_triggered: bool,
    pub(crate) fallback_count: u64,
    pub(crate) document_hits: u64,
    pub(crate) symbol_hits_count: u64,
    pub(crate) file_hits: u64,
    pub(crate) sources_count: u64,
    pub(crate) chunks_count: u64,
    pub(crate) pack_token_count: u64,
    pub(crate) deduped_token_count: u64,
    pub(crate) client_prompt_tokens: Option<u64>,
    pub(crate) assistant_generation_tokens: Option<u64>,
    pub(crate) tool_overhead_tokens: Option<u64>,
    pub(crate) continuity_restore_tokens: Option<u64>,
    pub(crate) tool_overhead_source: Option<Value>,
    pub(crate) pre_amai_baseline_source: Option<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuityPreAmaiBaselineMaterialization {
    pub(crate) baseline_tokens: u64,
    pub(crate) baseline_bytes: usize,
    pub(crate) source_entries: Vec<String>,
    pub(crate) source_ref: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct SecondaryContextPackPayloadMatch {
    pub(crate) context_pack_id: String,
    pub(crate) payload_json: String,
    pub(crate) delta_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) enum SecondaryContextPackPayloadLookup {
    Resolved(SecondaryContextPackPayloadMatch),
    NoCandidates,
    AmbiguousNearest {
        delta_ms: i64,
        candidate_count: usize,
    },
    NearestTooFar {
        delta_ms: i64,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AssistantGenerationScopeObservation {
    pub(crate) target_group_count: u64,
    pub(crate) observed_group_count: u64,
    pub(crate) observed_tokens: u64,
    pub(crate) helper_only_context_pack_ids: BTreeSet<String>,
    pub(crate) helper_only_non_model_visible_context_pack_ids: BTreeSet<String>,
    pub(crate) target_context_pack_ids: BTreeSet<String>,
    pub(crate) matched_context_pack_ids: BTreeSet<String>,
    pub(crate) unmatched_context_pack_ids: BTreeSet<String>,
    pub(crate) matched_turn_ids: BTreeSet<String>,
    pub(crate) available_turns: u64,
    pub(crate) available_direct_turns: u64,
    pub(crate) available_rollout_turns: u64,
    pub(crate) matched_direct_turn_ids: BTreeSet<String>,
    pub(crate) matched_rollout_turn_ids: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DashboardRolloutObservationCache {
    pub(crate) repo_root: PathBuf,
    pub(crate) signature: String,
    pub(crate) observations: Vec<codex_threads::RolloutAssistantGenerationObservation>,
}

#[derive(Debug, Clone)]
pub(crate) struct DashboardSameMeterSyncCache {
    pub(crate) repo_root: PathBuf,
    pub(crate) signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedDashboardSameMeterSyncCache {
    pub(crate) cache_version: String,
    pub(crate) repo_root: String,
    pub(crate) signature: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DashboardWorkingStateMetadataCache {
    pub(crate) signature: String,
    pub(crate) metadata: BTreeMap<String, WorkingStateContextPackMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkingStateContextPackMeta {
    pub(crate) thread_id: String,
    pub(crate) captured_at_epoch_ms: i64,
    pub(crate) turn_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AssistantGenerationTurnObservedSnapshot {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) assistant_generation_tokens: u64,
    pub(crate) context_pack_ids: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveResponseTurnObservation {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) state: String,
    pub(crate) started_at_epoch_ms: i64,
    pub(crate) ended_at_epoch_ms: i64,
    pub(crate) latency_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DashboardReportPreCacheTimings {
    pub(crate) stage_ms: BTreeMap<String, u64>,
    pub(crate) total_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct QualityVerdict {
    pub(crate) target_kind: &'static str,
    pub(crate) baseline_hit_target: bool,
    pub(crate) amai_hit_target: bool,
    pub(crate) quality_ok: bool,
    pub(crate) quality_score: f64,
    pub(crate) quality_method: &'static str,
    pub(crate) quality_tier: &'static str,
    pub(crate) head_hit_target: bool,
    pub(crate) needed_followup: bool,
    pub(crate) followup_count: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FollowupEventKey<'a> {
    pub(crate) query: &'a str,
    pub(crate) query_hash: &'a str,
    pub(crate) query_type: &'a str,
    pub(crate) target_kind: &'a str,
}

#[derive(Debug)]
pub(crate) struct NaiveScopeFile {
    pub(crate) project_code: String,
    pub(crate) relative_path: String,
    pub(crate) original_bytes: usize,
    pub(crate) bytes_used: usize,
    pub(crate) truncated: bool,
    pub(crate) content: String,
}

#[derive(Debug)]
pub(crate) struct NaiveScope {
    pub(crate) files: Vec<Value>,
    pub(crate) rendered_files: Vec<NaiveScopeFile>,
}

pub(crate) fn default_preliminary_min_events() -> u64 {
    50
}

pub(crate) fn default_preliminary_min_baseline_tokens() -> u64 {
    100_000
}

pub(crate) fn default_metering_ingest_warning_seconds() -> u64 {
    60
}

pub(crate) fn default_metering_ingest_slo_seconds() -> u64 {
    300
}

pub(crate) fn default_late_arrival_grace_minutes() -> u64 {
    60
}

pub(crate) fn default_usage_event_schema_version() -> String {
    "billing-usage-event-v2".to_string()
}

pub(crate) fn default_settlement_statement_version() -> String {
    "settlement-preview-v6".to_string()
}

pub(crate) fn default_metering_event_schema_version() -> String {
    "token-budget-event-v3".to_string()
}

pub(crate) fn default_usage_lifecycle_model_version() -> String {
    "usage-lifecycle-v1".to_string()
}

pub(crate) fn default_baseline_method_version() -> String {
    "retrieval-baseline-v1".to_string()
}

pub(crate) fn default_quality_method_version() -> String {
    "quality-gate-v1".to_string()
}

pub(crate) fn default_coverage_model_version() -> String {
    "token-coverage-v1".to_string()
}

pub(crate) fn default_metering_freshness_model_version() -> String {
    "metering-freshness-v1".to_string()
}

pub(crate) fn default_agent_cycle_model_version() -> String {
    "agent-cycle-lower-bound-v4".to_string()
}

pub(crate) fn default_client_limit_meter_alignment_version() -> String {
    "client-limit-meter-alignment-v12".to_string()
}

pub(crate) fn default_client_limit_baseline_equivalence_version() -> String {
    "client-limit-baseline-equivalence-v4".to_string()
}

pub(crate) fn default_client_limit_strict_meter_slice_version() -> String {
    "client-limit-strict-meter-slice-v1".to_string()
}

pub(crate) fn default_client_limit_explicit_boundary_surface_version() -> String {
    "client-limit-explicit-boundary-surface-v2".to_string()
}

pub(crate) fn default_client_limit_continuity_boundary_rollup_version() -> String {
    "client-limit-continuity-boundary-rollup-v2".to_string()
}

pub(crate) fn default_client_limit_pre_amai_baseline_source_version() -> String {
    "client-limit-pre-amai-baseline-source-v2".to_string()
}

pub(crate) fn default_client_limit_frozen_gap_review_surface_version() -> String {
    "client-limit-frozen-gap-review-surface-v1".to_string()
}

pub(crate) fn default_client_limit_reviewed_frozen_debt_export_surface_version() -> String {
    "client-limit-reviewed-frozen-debt-export-surface-v1".to_string()
}

pub(crate) fn default_excluded_taxonomy_version() -> String {
    "token-excluded-usage-v1".to_string()
}

pub(crate) fn default_dedup_contract_version() -> String {
    "event-id-source-kind-v1".to_string()
}

pub(crate) fn default_backfill_policy_version() -> String {
    "report-only-backfill-v1".to_string()
}

pub(crate) fn default_correction_policy_version() -> String {
    "report-only-correction-v1".to_string()
}

pub(crate) fn default_freeze_close_policy_version() -> String {
    "freeze-close-v2".to_string()
}

pub(crate) fn default_late_arrival_policy_version() -> String {
    "late-arrival-v2".to_string()
}

pub(crate) fn default_dispute_policy_version() -> String {
    "report-only-dispute-v1".to_string()
}

pub(crate) fn default_settlement_lifecycle_model_version() -> String {
    "settlement-lifecycle-v4".to_string()
}

pub(crate) fn default_statement_period_governance_version() -> String {
    "statement-period-governance-v2".to_string()
}

pub(crate) fn default_adjustment_preview_model_version() -> String {
    "adjustment-preview-v1".to_string()
}

pub(crate) fn default_adjustment_request_schema_version() -> String {
    "adjustment-request-v1".to_string()
}

pub(crate) fn default_adjustment_registry_version() -> String {
    "adjustment-registry-v2".to_string()
}

pub(crate) fn default_rate_card_binding_model_version() -> String {
    "rate-card-binding-v3".to_string()
}

pub(crate) fn default_infra_cost_binding_model_version() -> String {
    "infra-cost-binding-v3".to_string()
}

pub(crate) fn default_telemetry_surface_split_version() -> String {
    "tokenonomics-surface-split-v1".to_string()
}

pub(crate) fn default_event_time_policy_version() -> String {
    "client-visible-ingest-v1".to_string()
}

pub(crate) fn default_billing_policy_version() -> String {
    "report-only-v1".to_string()
}

pub(crate) fn default_suitability_model_version() -> String {
    "token-suitability-v1".to_string()
}

pub(crate) fn default_contractual_readiness_model_version() -> String {
    "contractual-readiness-v1".to_string()
}

pub(crate) fn default_customer_contractual_boundary_version() -> String {
    "customer-contractual-boundary-v1".to_string()
}

pub(crate) fn default_settlement_activation_governance_version() -> String {
    "settlement-activation-governance-v1".to_string()
}

pub(crate) fn default_adjustment_activation_governance_version() -> String {
    "adjustment-activation-governance-v1".to_string()
}

pub(crate) fn default_billing_mode() -> String {
    "report_only".to_string()
}

pub(crate) fn default_reconciliation_contract_version() -> String {
    "provider-reconciliation-v10".to_string()
}

pub(crate) fn default_margin_model_version() -> String {
    "margin-view-v9".to_string()
}

pub(crate) fn default_infra_cost_profile_version() -> String {
    "unpriced-infra-v1".to_string()
}

pub(crate) fn default_contractual_evidence_pack_version() -> String {
    "contractual-evidence-pack-v21".to_string()
}

pub(crate) fn default_contractual_statement_export_version() -> String {
    "contractual-statement-export-v21".to_string()
}

pub(crate) fn default_settlement_report_preview_version() -> String {
    "settlement-report-preview-v12".to_string()
}

pub(crate) fn default_rate_card_version() -> String {
    "unpriced-v1".to_string()
}

pub(crate) fn default_currency_profile() -> String {
    "unpriced".to_string()
}

pub(crate) fn default_settlement_status() -> String {
    "unsettled_report_only".to_string()
}
