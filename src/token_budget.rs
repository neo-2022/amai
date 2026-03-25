use crate::cli::{
    ContextPackArgs, ObserveTokenAdjustmentAddArgs, ObserveTokenAdjustmentRegistryArgs,
    ObserveTokenContractualSourcesArgs, ObserveTokenEvidencePackArgs, ObserveTokenReportArgs,
    ObserveTokenRolloutAssistantGenerationArgs, ObserveTokenStatementExportArgs,
    ObserveTokenWholeCycleAttachArgs,
};
use crate::codex_threads;
use crate::config::{self, AppConfig};
use crate::language;
use crate::postgres::{self, ObservabilitySnapshotRecord};
use crate::retrieval;
use anyhow::{Context, Result, anyhow, bail};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tiktoken_rs::{CoreBPE, cl100k_base, o200k_base};
use tokio_postgres::Client;
use uuid::Uuid;

const CONFIG_RELATIVE_PATH: &str = "config/token_budget_profiles.toml";
const AGENT_CYCLE_TIMELINE_MAX_POINTS: usize = 256;
const ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS: i64 = 60_000;

#[derive(Debug, Clone, Deserialize)]
struct TokenBudgetConfigFile {
    default_profile: String,
    measurement: MeasurementConfig,
    #[serde(default)]
    contract: TokenBudgetContractConfig,
    #[serde(default)]
    profiles: BTreeMap<String, TokenBudgetProfile>,
    #[serde(default)]
    client_budget_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MeasurementConfig {
    tokenizer: String,
    naive_limit_files: usize,
    naive_max_bytes_per_file: usize,
    #[serde(default)]
    include_verify_events_by_default: bool,
    #[serde(default = "default_metering_ingest_warning_seconds")]
    metering_ingest_warning_seconds: u64,
    #[serde(default = "default_metering_ingest_slo_seconds")]
    metering_ingest_slo_seconds: u64,
    #[serde(default = "default_late_arrival_grace_minutes")]
    late_arrival_grace_minutes: u64,
    #[serde(default = "default_preliminary_min_events")]
    preliminary_min_events: u64,
    #[serde(default = "default_preliminary_min_baseline_tokens")]
    preliminary_min_baseline_tokens: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TokenBudgetContractConfig {
    #[serde(default = "default_usage_event_schema_version")]
    usage_event_schema_version: String,
    #[serde(default = "default_settlement_statement_version")]
    settlement_statement_version: String,
    #[serde(default = "default_metering_event_schema_version")]
    metering_event_schema_version: String,
    #[serde(default = "default_usage_lifecycle_model_version")]
    usage_lifecycle_model_version: String,
    #[serde(default = "default_baseline_method_version")]
    baseline_method_version: String,
    #[serde(default = "default_quality_method_version")]
    quality_method_version: String,
    #[serde(default = "default_coverage_model_version")]
    coverage_model_version: String,
    #[serde(default = "default_metering_freshness_model_version")]
    metering_freshness_model_version: String,
    #[serde(default = "default_agent_cycle_model_version")]
    agent_cycle_model_version: String,
    #[serde(default = "default_client_limit_meter_alignment_version")]
    client_limit_meter_alignment_version: String,
    #[serde(default = "default_excluded_taxonomy_version")]
    excluded_taxonomy_version: String,
    #[serde(default = "default_dedup_contract_version")]
    dedup_contract_version: String,
    #[serde(default = "default_backfill_policy_version")]
    backfill_policy_version: String,
    #[serde(default = "default_correction_policy_version")]
    correction_policy_version: String,
    #[serde(default = "default_freeze_close_policy_version")]
    freeze_close_policy_version: String,
    #[serde(default = "default_late_arrival_policy_version")]
    late_arrival_policy_version: String,
    #[serde(default = "default_dispute_policy_version")]
    dispute_policy_version: String,
    #[serde(default = "default_settlement_lifecycle_model_version")]
    settlement_lifecycle_model_version: String,
    #[serde(default = "default_statement_period_governance_version")]
    statement_period_governance_version: String,
    #[serde(default = "default_adjustment_preview_model_version")]
    adjustment_preview_model_version: String,
    #[serde(default = "default_adjustment_request_schema_version")]
    adjustment_request_schema_version: String,
    #[serde(default = "default_adjustment_registry_version")]
    adjustment_registry_version: String,
    #[serde(default = "default_rate_card_binding_model_version")]
    rate_card_binding_model_version: String,
    #[serde(default = "default_infra_cost_binding_model_version")]
    infra_cost_binding_model_version: String,
    #[serde(default = "default_telemetry_surface_split_version")]
    telemetry_surface_split_version: String,
    #[serde(default = "default_event_time_policy_version")]
    event_time_policy_version: String,
    #[serde(default = "default_billing_policy_version")]
    billing_policy_version: String,
    #[serde(default = "default_suitability_model_version")]
    suitability_model_version: String,
    #[serde(default = "default_contractual_readiness_model_version")]
    contractual_readiness_model_version: String,
    #[serde(default = "default_customer_contractual_boundary_version")]
    customer_contractual_boundary_version: String,
    #[serde(default = "default_settlement_activation_governance_version")]
    settlement_activation_governance_version: String,
    #[serde(default = "default_adjustment_activation_governance_version")]
    adjustment_activation_governance_version: String,
    #[serde(default = "default_billing_mode")]
    billing_mode: String,
    #[serde(default = "default_reconciliation_contract_version")]
    reconciliation_contract_version: String,
    #[serde(default = "default_margin_model_version")]
    margin_model_version: String,
    #[serde(default = "default_infra_cost_profile_version")]
    infra_cost_profile_version: String,
    #[serde(default = "default_contractual_evidence_pack_version")]
    contractual_evidence_pack_version: String,
    #[serde(default = "default_contractual_statement_export_version")]
    contractual_statement_export_version: String,
    #[serde(default = "default_settlement_report_preview_version")]
    settlement_report_preview_version: String,
    #[serde(default = "default_rate_card_version")]
    rate_card_version: String,
    #[serde(default = "default_currency_profile")]
    currency_profile: String,
    #[serde(default = "default_settlement_status")]
    settlement_status: String,
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
struct TokenBudgetProfile {
    display_name: String,
    description: String,
    session_gap_minutes: u64,
    rolling_window_hours: Option<u64>,
}

#[derive(Debug, Clone)]
struct ResolvedProfile {
    code: String,
    display_name: String,
    description: String,
    session_gap_minutes: u64,
    rolling_window_hours: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct AdjustmentRegistryFile {
    #[serde(default)]
    adjustments: Vec<AdjustmentRegistryEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdjustmentRegistryEntry {
    adjustment_id: String,
    scope_code: String,
    kind: String,
    status: String,
    reason_code: String,
    created_at_epoch_ms: i64,
    #[serde(default)]
    tokens_delta: Option<i64>,
    #[serde(default)]
    amount_delta: Option<f64>,
    #[serde(default)]
    currency_profile: Option<String>,
    #[serde(default)]
    related_statement_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RateCardFile {
    schema_version: String,
    rate_card_version: String,
    currency_profile: String,
    provider: String,
    default_input_cost_per_1k_tokens: f64,
    default_output_cost_per_1k_tokens: f64,
    #[serde(default)]
    effective_from_epoch_ms: Option<i64>,
    #[serde(default)]
    effective_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ProviderUsageExportFile {
    schema_version: String,
    provider: String,
    #[serde(default)]
    currency_profile: Option<String>,
    #[serde(default)]
    scopes: Vec<ProviderUsageScopeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderUsageScopeEntry {
    scope_code: String,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    provider_cost_amount: Option<f64>,
    #[serde(default)]
    currency_profile: Option<String>,
    #[serde(default)]
    period_start_epoch_ms: Option<i64>,
    #[serde(default)]
    period_end_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ProviderInvoiceExportFile {
    schema_version: String,
    provider: String,
    #[serde(default)]
    currency_profile: Option<String>,
    #[serde(default)]
    scopes: Vec<ProviderInvoiceScopeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderInvoiceScopeEntry {
    scope_code: String,
    invoice_amount: f64,
    #[serde(default)]
    currency_profile: Option<String>,
    #[serde(default)]
    invoice_id: Option<String>,
    #[serde(default)]
    period_start_epoch_ms: Option<i64>,
    #[serde(default)]
    period_end_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct InfraCostProfileFile {
    schema_version: String,
    infra_cost_profile_version: String,
    currency_profile: String,
    #[serde(default)]
    provider: Option<String>,
    cost_per_1k_internal_billed_tokens: f64,
    #[serde(default)]
    cost_per_live_event: f64,
    #[serde(default)]
    fixed_scope_cost_amount: f64,
    #[serde(default)]
    effective_from_epoch_ms: Option<i64>,
    #[serde(default)]
    effective_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct TokenBudgetEvent {
    created_at_epoch_ms: i64,
    event_id: String,
    correlation_id: String,
    payload_origin: String,
    session_id: String,
    rolling_window_profile: String,
    timestamp_utc: i64,
    occurred_at_epoch_ms: i64,
    ingested_at_epoch_ms: i64,
    snapshot_kind: String,
    source_kind: String,
    traffic_class: String,
    measurement_scope: String,
    usage_event_schema_version: String,
    settlement_statement_version: String,
    metering_event_schema_version: String,
    usage_lifecycle_model_version: String,
    baseline_method_version: String,
    quality_method_version: String,
    coverage_model_version: String,
    metering_freshness_model_version: String,
    excluded_taxonomy_version: String,
    dedup_contract_version: String,
    backfill_policy_version: String,
    correction_policy_version: String,
    freeze_close_policy_version: String,
    late_arrival_policy_version: String,
    dispute_policy_version: String,
    settlement_lifecycle_model_version: String,
    statement_period_governance_version: String,
    adjustment_preview_model_version: String,
    adjustment_request_schema_version: String,
    adjustment_registry_version: String,
    rate_card_binding_model_version: String,
    telemetry_surface_split_version: String,
    event_time_policy_version: String,
    billing_policy_version: String,
    suitability_model_version: String,
    billing_mode: String,
    reconciliation_contract_version: String,
    margin_model_version: String,
    infra_cost_profile_version: String,
    contractual_evidence_pack_version: String,
    rate_card_version: String,
    currency_profile: String,
    settlement_status: String,
    project: String,
    namespace: String,
    query: String,
    query_hash: String,
    query_type: String,
    target_kind: String,
    baseline_hit_target: bool,
    amai_hit_target: bool,
    cold_warm_state: String,
    baseline_strategy: String,
    retrieval_mode: Option<String>,
    tokenizer: String,
    latency_ms: f64,
    saved_tokens: u64,
    naive_tokens: u64,
    context_tokens: u64,
    recovery_tokens: u64,
    effective_saved_tokens: i64,
    savings_factor: f64,
    savings_percent: f64,
    effective_savings_percent: f64,
    quality_ok: bool,
    quality_score: f64,
    quality_method: String,
    quality_tier: String,
    head_hit_target: bool,
    needed_followup: bool,
    followup_count: u64,
    followup_of_event_id: Option<String>,
    resolved_by_event_id: Option<String>,
    fallback_triggered: bool,
    fallback_count: u64,
    document_hits: u64,
    symbol_hits_count: u64,
    file_hits: u64,
    sources_count: u64,
    chunks_count: u64,
    pack_token_count: u64,
    deduped_token_count: u64,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct AssistantGenerationScopeObservation {
    target_group_count: u64,
    observed_group_count: u64,
    observed_tokens: u64,
    target_context_pack_ids: BTreeSet<String>,
    matched_context_pack_ids: BTreeSet<String>,
    unmatched_context_pack_ids: BTreeSet<String>,
    matched_turn_ids: BTreeSet<String>,
    available_turns: u64,
}

#[derive(Debug, Clone)]
struct WorkingStateContextPackMeta {
    thread_id: String,
    captured_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
struct QualityVerdict {
    target_kind: &'static str,
    baseline_hit_target: bool,
    amai_hit_target: bool,
    quality_ok: bool,
    quality_score: f64,
    quality_method: &'static str,
    quality_tier: &'static str,
    head_hit_target: bool,
    needed_followup: bool,
    followup_count: u64,
}

#[derive(Debug, Clone, Copy)]
struct FollowupEventKey<'a> {
    query: &'a str,
    query_hash: &'a str,
    query_type: &'a str,
    target_kind: &'a str,
}

#[derive(Debug)]
struct NaiveScopeFile {
    project_code: String,
    relative_path: String,
    original_bytes: usize,
    bytes_used: usize,
    truncated: bool,
    content: String,
}

#[derive(Debug)]
struct NaiveScope {
    files: Vec<Value>,
    rendered_files: Vec<NaiveScopeFile>,
}

fn default_preliminary_min_events() -> u64 {
    50
}

fn default_preliminary_min_baseline_tokens() -> u64 {
    100_000
}

fn default_metering_ingest_warning_seconds() -> u64 {
    60
}

fn default_metering_ingest_slo_seconds() -> u64 {
    300
}

fn default_late_arrival_grace_minutes() -> u64 {
    60
}

fn default_usage_event_schema_version() -> String {
    "billing-usage-event-v2".to_string()
}

fn default_settlement_statement_version() -> String {
    "settlement-preview-v5".to_string()
}

fn default_metering_event_schema_version() -> String {
    "token-budget-event-v3".to_string()
}

fn default_usage_lifecycle_model_version() -> String {
    "usage-lifecycle-v1".to_string()
}

fn default_baseline_method_version() -> String {
    "retrieval-baseline-v1".to_string()
}

fn default_quality_method_version() -> String {
    "quality-gate-v1".to_string()
}

fn default_coverage_model_version() -> String {
    "token-coverage-v1".to_string()
}

fn default_metering_freshness_model_version() -> String {
    "metering-freshness-v1".to_string()
}

fn default_agent_cycle_model_version() -> String {
    "agent-cycle-lower-bound-v3".to_string()
}

fn default_client_limit_meter_alignment_version() -> String {
    "client-limit-meter-alignment-v5".to_string()
}

fn default_excluded_taxonomy_version() -> String {
    "token-excluded-usage-v1".to_string()
}

fn default_dedup_contract_version() -> String {
    "event-id-source-kind-v1".to_string()
}

fn default_backfill_policy_version() -> String {
    "report-only-backfill-v1".to_string()
}

fn default_correction_policy_version() -> String {
    "report-only-correction-v1".to_string()
}

fn default_freeze_close_policy_version() -> String {
    "freeze-close-v2".to_string()
}

fn default_late_arrival_policy_version() -> String {
    "late-arrival-v2".to_string()
}

fn default_dispute_policy_version() -> String {
    "report-only-dispute-v1".to_string()
}

fn default_settlement_lifecycle_model_version() -> String {
    "settlement-lifecycle-v4".to_string()
}

fn default_statement_period_governance_version() -> String {
    "statement-period-governance-v2".to_string()
}

fn default_adjustment_preview_model_version() -> String {
    "adjustment-preview-v1".to_string()
}

fn default_adjustment_request_schema_version() -> String {
    "adjustment-request-v1".to_string()
}

fn default_adjustment_registry_version() -> String {
    "adjustment-registry-v2".to_string()
}

fn default_rate_card_binding_model_version() -> String {
    "rate-card-binding-v3".to_string()
}

fn default_infra_cost_binding_model_version() -> String {
    "infra-cost-binding-v3".to_string()
}

fn default_telemetry_surface_split_version() -> String {
    "tokenonomics-surface-split-v1".to_string()
}

fn default_event_time_policy_version() -> String {
    "client-visible-ingest-v1".to_string()
}

fn default_billing_policy_version() -> String {
    "report-only-v1".to_string()
}

fn default_suitability_model_version() -> String {
    "token-suitability-v1".to_string()
}

fn default_contractual_readiness_model_version() -> String {
    "contractual-readiness-v1".to_string()
}

fn default_customer_contractual_boundary_version() -> String {
    "customer-contractual-boundary-v1".to_string()
}

fn default_settlement_activation_governance_version() -> String {
    "settlement-activation-governance-v1".to_string()
}

fn default_adjustment_activation_governance_version() -> String {
    "adjustment-activation-governance-v1".to_string()
}

fn default_billing_mode() -> String {
    "report_only".to_string()
}

fn default_reconciliation_contract_version() -> String {
    "provider-reconciliation-v10".to_string()
}

fn default_margin_model_version() -> String {
    "margin-view-v9".to_string()
}

fn default_infra_cost_profile_version() -> String {
    "unpriced-infra-v1".to_string()
}

fn default_contractual_evidence_pack_version() -> String {
    "contractual-evidence-pack-v18".to_string()
}

fn default_contractual_statement_export_version() -> String {
    "contractual-statement-export-v18".to_string()
}

fn default_settlement_report_preview_version() -> String {
    "settlement-report-preview-v9".to_string()
}

fn default_rate_card_version() -> String {
    "unpriced-v1".to_string()
}

fn default_currency_profile() -> String {
    "unpriced".to_string()
}

fn default_settlement_status() -> String {
    "unsettled_report_only".to_string()
}

fn report_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "usage_event_schema_version": contract.usage_event_schema_version.clone(),
        "settlement_statement_version": contract.settlement_statement_version.clone(),
        "metering_event_schema_version": contract.metering_event_schema_version.clone(),
        "usage_lifecycle_model_version": contract.usage_lifecycle_model_version.clone(),
        "baseline_method_version": contract.baseline_method_version.clone(),
        "quality_method_version": contract.quality_method_version.clone(),
        "coverage_model_version": contract.coverage_model_version.clone(),
        "metering_freshness_model_version": contract.metering_freshness_model_version.clone(),
        "agent_cycle_model_version": contract.agent_cycle_model_version.clone(),
        "client_limit_meter_alignment_version": contract.client_limit_meter_alignment_version.clone(),
        "excluded_taxonomy_version": contract.excluded_taxonomy_version.clone(),
        "dedup_contract_version": contract.dedup_contract_version.clone(),
        "backfill_policy_version": contract.backfill_policy_version.clone(),
        "correction_policy_version": contract.correction_policy_version.clone(),
        "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
        "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
        "dispute_policy_version": contract.dispute_policy_version.clone(),
        "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
        "statement_period_governance_version": contract.statement_period_governance_version.clone(),
        "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
        "adjustment_request_schema_version": contract.adjustment_request_schema_version.clone(),
        "adjustment_registry_version": contract.adjustment_registry_version.clone(),
        "rate_card_binding_model_version": contract.rate_card_binding_model_version.clone(),
        "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "telemetry_surface_split_version": contract.telemetry_surface_split_version.clone(),
        "event_time_policy_version": contract.event_time_policy_version.clone(),
        "billing_policy_version": contract.billing_policy_version.clone(),
        "suitability_model_version": contract.suitability_model_version.clone(),
        "contractual_readiness_model_version": contract.contractual_readiness_model_version.clone(),
        "customer_contractual_boundary_version": contract.customer_contractual_boundary_version.clone(),
        "settlement_activation_governance_version": contract
            .settlement_activation_governance_version
            .clone(),
        "adjustment_activation_governance_version": contract
            .adjustment_activation_governance_version
            .clone(),
        "billing_mode": contract.billing_mode.clone(),
        "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
        "margin_model_version": contract.margin_model_version.clone(),
        "infra_cost_profile_version": contract.infra_cost_profile_version.clone(),
        "contractual_evidence_pack_version": contract.contractual_evidence_pack_version.clone(),
        "contractual_statement_export_version": contract.contractual_statement_export_version.clone(),
        "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
        "rate_card_version": contract.rate_card_version.clone(),
        "currency_profile": contract.currency_profile.clone(),
        "settlement_status": contract.settlement_status.clone(),
        "note": "Сейчас tokenonomics работает в report-only режиме: metering и lower-bound semantics уже materialized, но money-facing billable settlement ещё не включён."
    })
}

fn token_contract_metadata_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "usage_event_schema_version": contract.usage_event_schema_version.clone(),
        "settlement_statement_version": contract.settlement_statement_version.clone(),
        "metering_event_schema_version": contract.metering_event_schema_version.clone(),
        "usage_lifecycle_model_version": contract.usage_lifecycle_model_version.clone(),
        "baseline_method_version": contract.baseline_method_version.clone(),
        "quality_method_version": contract.quality_method_version.clone(),
        "coverage_model_version": contract.coverage_model_version.clone(),
        "metering_freshness_model_version": contract.metering_freshness_model_version.clone(),
        "agent_cycle_model_version": contract.agent_cycle_model_version.clone(),
        "client_limit_meter_alignment_version": contract.client_limit_meter_alignment_version.clone(),
        "excluded_taxonomy_version": contract.excluded_taxonomy_version.clone(),
        "dedup_contract_version": contract.dedup_contract_version.clone(),
        "backfill_policy_version": contract.backfill_policy_version.clone(),
        "correction_policy_version": contract.correction_policy_version.clone(),
        "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
        "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
        "dispute_policy_version": contract.dispute_policy_version.clone(),
        "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
        "statement_period_governance_version": contract.statement_period_governance_version.clone(),
        "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
        "adjustment_request_schema_version": contract.adjustment_request_schema_version.clone(),
        "adjustment_registry_version": contract.adjustment_registry_version.clone(),
        "rate_card_binding_model_version": contract.rate_card_binding_model_version.clone(),
        "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "telemetry_surface_split_version": contract.telemetry_surface_split_version.clone(),
        "event_time_policy_version": contract.event_time_policy_version.clone(),
        "billing_policy_version": contract.billing_policy_version.clone(),
        "suitability_model_version": contract.suitability_model_version.clone(),
        "contractual_readiness_model_version": contract.contractual_readiness_model_version.clone(),
        "customer_contractual_boundary_version": contract.customer_contractual_boundary_version.clone(),
        "settlement_activation_governance_version": contract
            .settlement_activation_governance_version
            .clone(),
        "adjustment_activation_governance_version": contract
            .adjustment_activation_governance_version
            .clone(),
        "billing_mode": contract.billing_mode.clone(),
        "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
        "margin_model_version": contract.margin_model_version.clone(),
        "infra_cost_profile_version": contract.infra_cost_profile_version.clone(),
        "contractual_evidence_pack_version": contract.contractual_evidence_pack_version.clone(),
        "contractual_statement_export_version": contract.contractual_statement_export_version.clone(),
        "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
        "rate_card_version": contract.rate_card_version.clone(),
        "currency_profile": contract.currency_profile.clone(),
        "settlement_status": contract.settlement_status.clone(),
    })
}

fn build_usage_event_schema_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "schema_version": contract.usage_event_schema_version.clone(),
        "identity": {
            "required_fields": [
                "event_id",
                "correlation_id",
                "source_kind",
                "traffic_class",
                "project_code",
                "namespace_code",
                "measurement_scope",
                "occurred_at_epoch_ms",
                "ingested_at_epoch_ms"
            ],
            "dedup_key_format": "source_kind:event_id",
            "event_identity_note": "Исторические события сохраняют записанные contract versions; новые report semantics не переписывают прошлую truth-схему."
        },
        "lifecycle": {
            "model_version": contract.usage_lifecycle_model_version.clone(),
            "statuses": [
                "verified_included",
                "excluded_quality_gate_failed",
                "excluded_awaiting_followup_reconciliation",
                "excluded_legacy_unverified",
                "excluded_non_live"
            ],
            "reporting_layers": [
                "measured_non_billable",
                "excluded"
            ]
        },
        "dedup": {
            "policy_version": contract.dedup_contract_version.clone(),
            "idempotency_scope": "source_kind + event_id",
            "retry_behavior": "same dedup key must resolve to the same usage event identity"
        },
        "time_policy": {
            "policy_version": contract.event_time_policy_version.clone(),
            "canonical_window_field": "occurred_at_epoch_ms",
            "ingest_field": "ingested_at_epoch_ms",
            "ordering_note": "Rollup-окна считаются по occurred_at_epoch_ms; ingest time хранится отдельно и не подменяет event time."
        },
        "backfill": {
            "policy_version": contract.backfill_policy_version.clone(),
            "status": "report_only_manual_repair_or_reverify",
            "note": "Backfill пока разрешён только через явные repair/reverify paths и не должен тихо переписывать старую event truth."
        },
        "corrections": {
            "policy_version": contract.correction_policy_version.clone(),
            "status": "mutable_snapshot_report_only",
            "note": "До settlement layer corrections остаются report-only snapshot updates, а не invoice-grade credit workflow."
        },
        "whole_cycle_observed": {
            "status": "optional_progressive_measurement",
            "component_fields": [
                "client_prompt_tokens",
                "assistant_generation_tokens",
                "tool_overhead_tokens",
                "continuity_restore_tokens"
            ],
            "note": "Observed whole-cycle fields можно materialize-ить постепенно: их наличие расширяет видимость клиентского spend meter, но не даёт права объявлять same-meter savings без baseline-equivalent semantics."
        }
    })
}

fn build_metering_freshness_contract_json(
    contract: &TokenBudgetContractConfig,
    measurement: &MeasurementConfig,
) -> Value {
    json!({
        "model_version": contract.metering_freshness_model_version.clone(),
        "ingest_warning_seconds": measurement.metering_ingest_warning_seconds,
        "ingest_slo_seconds": measurement.metering_ingest_slo_seconds,
        "late_arrival_grace_minutes": measurement.late_arrival_grace_minutes,
        "ingest_states": [
            "empty",
            "within_slo",
            "soft_lag",
            "lagging"
        ],
        "contractual_lag_states": [
            "empty",
            "awaiting_late_events",
            "lag_window_elapsed"
        ],
        "contractual_freshness_states": [
            "empty",
            "provisional_open_window",
            "stable",
            "lagging_pipeline"
        ],
        "note": "Freshness и lag semantics разделены: ingest state показывает здоровье metering pipeline, а contractual lag state — можно ли уже считать окно стабилизированным без поздних событий."
    })
}

fn combine_reason_arrays(values: &[&Value]) -> Value {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for value in values {
        let Some(array) = value.as_array() else {
            continue;
        };
        for item in array {
            let Some(reason) = item.as_str() else {
                continue;
            };
            if seen.insert(reason.to_string()) {
                items.push(Value::String(reason.to_string()));
            }
        }
    }
    Value::Array(items)
}

fn event_ingest_lag_ms(event: &TokenBudgetEvent) -> u64 {
    event
        .ingested_at_epoch_ms
        .saturating_sub(event.occurred_at_epoch_ms)
        .max(0) as u64
}

fn build_metering_freshness_summary(
    contract: &TokenBudgetContractConfig,
    measurement: &MeasurementConfig,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
) -> Value {
    if events.is_empty() {
        return json!({
            "model_version": contract.metering_freshness_model_version.clone(),
            "events_count": 0,
            "metering_ingest_state": "empty",
            "contractual_lag_state": "empty",
            "contractual_freshness_state": "empty",
            "can_treat_scope_as_stable": false,
            "late_arrival_grace_ms": measurement.late_arrival_grace_minutes.saturating_mul(60_000),
            "latest_event_occurred_at_epoch_ms": Value::Null,
            "latest_event_ingested_at_epoch_ms": Value::Null,
            "latest_event_age_ms": Value::Null,
            "latest_ingest_lag_ms": Value::Null,
            "p50_ingest_lag_ms": 0.0,
            "p95_ingest_lag_ms": 0.0,
            "max_ingest_lag_ms": 0.0,
            "negative_ingest_skew_events": 0,
            "blocking_reasons": ["no_measured_usage_events"],
        });
    }

    let latest_event = events
        .iter()
        .max_by_key(|event| (event.occurred_at_epoch_ms, event.ingested_at_epoch_ms))
        .expect("events is not empty");
    let late_arrival_grace_ms = measurement
        .late_arrival_grace_minutes
        .saturating_mul(60_000);
    let latest_event_age_ms = now_epoch_ms.saturating_sub(latest_event.occurred_at_epoch_ms);
    let latest_ingest_lag_ms = event_ingest_lag_ms(latest_event);
    let negative_ingest_skew_events = events
        .iter()
        .filter(|event| event.ingested_at_epoch_ms < event.occurred_at_epoch_ms)
        .count() as u64;
    let mut lag_values = events
        .iter()
        .map(|event| event_ingest_lag_ms(event) as f64)
        .collect::<Vec<_>>();
    lag_values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let max_ingest_lag_ms = lag_values.last().copied().unwrap_or_default();
    let warning_lag_ms = measurement
        .metering_ingest_warning_seconds
        .saturating_mul(1000) as f64;
    let slo_lag_ms = measurement.metering_ingest_slo_seconds.saturating_mul(1000) as f64;
    let metering_ingest_state = if max_ingest_lag_ms == 0.0 {
        "within_slo"
    } else if max_ingest_lag_ms <= warning_lag_ms {
        "within_slo"
    } else if max_ingest_lag_ms <= slo_lag_ms {
        "soft_lag"
    } else {
        "lagging"
    };
    let contractual_lag_state = if latest_event_age_ms < late_arrival_grace_ms as i64 {
        "awaiting_late_events"
    } else {
        "lag_window_elapsed"
    };
    let contractual_freshness_state = if metering_ingest_state == "lagging" {
        "lagging_pipeline"
    } else if contractual_lag_state == "awaiting_late_events" {
        "provisional_open_window"
    } else {
        "stable"
    };
    let mut blocking_reasons = Vec::new();
    if metering_ingest_state == "lagging" {
        blocking_reasons.push("metering_pipeline_lagging");
    }
    if contractual_lag_state == "awaiting_late_events" {
        blocking_reasons.push("late_arrival_window_open");
    }
    if negative_ingest_skew_events > 0 {
        blocking_reasons.push("negative_ingest_clock_skew_detected");
    }

    json!({
        "model_version": contract.metering_freshness_model_version.clone(),
        "events_count": events.len(),
        "metering_ingest_state": metering_ingest_state,
        "contractual_lag_state": contractual_lag_state,
        "contractual_freshness_state": contractual_freshness_state,
        "can_treat_scope_as_stable": contractual_freshness_state == "stable",
        "late_arrival_grace_ms": late_arrival_grace_ms,
        "latest_event_occurred_at_epoch_ms": latest_event.occurred_at_epoch_ms,
        "latest_event_ingested_at_epoch_ms": latest_event.ingested_at_epoch_ms,
        "latest_event_age_ms": latest_event_age_ms,
        "latest_ingest_lag_ms": latest_ingest_lag_ms,
        "p50_ingest_lag_ms": percentile_from_sorted(&lag_values, 0.50),
        "p95_ingest_lag_ms": percentile_from_sorted(&lag_values, 0.95),
        "max_ingest_lag_ms": max_ingest_lag_ms,
        "negative_ingest_skew_events": negative_ingest_skew_events,
        "blocking_reasons": blocking_reasons,
    })
}

fn allowed_baseline_classes() -> [&'static str; 5] {
    [
        "naive_top_files",
        "grep_top_files",
        "ide_search_top_files",
        "semantic_top_k",
        "legacy_pre_amai",
    ]
}

fn disallowed_baseline_classes() -> [&'static str; 2] {
    ["entire_repo", "all_docs"]
}

fn build_baseline_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "baseline_method_version": contract.baseline_method_version.clone(),
        "allowed_classes": allowed_baseline_classes(),
        "disallowed_classes": disallowed_baseline_classes(),
        "fairness_note": "Savings разрешено считать только против реалистичного baseline scope; раздутый entire_repo/all_docs baseline запрещён."
    })
}

fn build_billing_policy_json(
    contract: &TokenBudgetContractConfig,
    measurement: &MeasurementConfig,
) -> Value {
    json!({
        "policy_version": contract.billing_policy_version.clone(),
        "mode": contract.billing_mode.clone(),
        "status": "report_only",
        "settlement_status": contract.settlement_status.clone(),
        "current_billable_state": "disabled_report_only",
        "savings_floor_term": "savings floor",
        "confirmed_lower_bound_term": "confirmed lower bound",
        "retrieval_savings_floor_term": "retrieval savings floor",
        "whole_cycle_term": "partial whole-agent-cycle lower bound",
        "quality_gate_required": true,
        "required_traffic_class": "live",
        "preliminary_thresholds": {
            "min_events": measurement.preliminary_min_events,
            "min_baseline_tokens": measurement.preliminary_min_baseline_tokens
        },
        "included_reporting_layers": [
            "measured_non_billable",
            "unmeasured"
        ],
        "excluded_from_future_billing": [
            "synthetic traffic",
            "unverified live events",
            "quality_gate_failed",
            "awaiting_followup_reconciliation"
        ],
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "note": "Billing semantics пока не активны: lower bound уже измеряется, но current policy остаётся report-only и не превращает savings в денежное начисление. confirmed lower bound пригоден для truthful KPI только вместе с coverage и completeness state."
    })
}

fn build_suitability_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "model_version": contract.suitability_model_version.clone(),
        "surfaces": [
            {
                "code": "operational_live",
                "meaning": "Инженерный live-contour для наблюдения за текущим потоком. Может показывать и положительный, и отрицательный результат без денежного смысла."
            },
            {
                "code": "product_kpi",
                "meaning": "Truthful product KPI по confirmed lower bound. Требует confirmed usage и обязан показываться вместе с coverage и completeness state."
            },
            {
                "code": "customer_review",
                "meaning": "Customer-facing review/report-only слой. Может быть пригоден даже в provisional состоянии, если это прямо показано."
            },
            {
                "code": "contractual_export",
                "meaning": "Export/evidence surface для review и audit. Это не invoice и не settlement."
            },
            {
                "code": "billing_amount",
                "meaning": "Будущий money-facing слой. До честного billable close и reconciliation обязан оставаться непригодным."
            },
            {
                "code": "compensation_pricing",
                "meaning": "Самый строгий слой для success-fee или pay-from-savings. Требует billable lower bound, money truth и final settlement semantics."
            }
        ],
        "required_companions": [
            "coverage",
            "completeness_state",
            "truth_guardrail"
        ],
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "note": "Suitability отвечает не на вопрос, хорошая ли цифра, а на вопрос, где её можно использовать без обмана. Отрицательная экономия тоже может быть truthful KPI, если scope и coverage показаны честно."
    })
}

fn parse_rate_card_file(raw: &str) -> Result<RateCardFile> {
    serde_json::from_str::<RateCardFile>(raw)
        .or_else(|_| toml::from_str::<RateCardFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse rate-card file as JSON or TOML")
}

fn parse_provider_usage_export_file(raw: &str) -> Result<ProviderUsageExportFile> {
    serde_json::from_str::<ProviderUsageExportFile>(raw)
        .or_else(|_| toml::from_str::<ProviderUsageExportFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse provider usage export as JSON or TOML")
}

fn parse_provider_invoice_export_file(raw: &str) -> Result<ProviderInvoiceExportFile> {
    serde_json::from_str::<ProviderInvoiceExportFile>(raw)
        .or_else(|_| toml::from_str::<ProviderInvoiceExportFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse provider invoice export as JSON or TOML")
}

fn file_last_modified_epoch_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as i64)
}

fn attach_source_file_evidence(base: &mut Value, path: &Path, raw: &str) {
    base["source_bytes"] = json!(raw.len() as u64);
    base["source_sha256"] = Value::String(hex_sha256(raw.as_bytes()));
    base["source_last_modified_epoch_ms"] = match file_last_modified_epoch_ms(path) {
        Some(value) => json!(value),
        None => Value::Null,
    };
}

fn parse_infra_cost_profile_file(raw: &str) -> Result<InfraCostProfileFile> {
    serde_json::from_str::<InfraCostProfileFile>(raw)
        .or_else(|_| toml::from_str::<InfraCostProfileFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse infra cost profile as JSON or TOML")
}

fn bind_rate_card_json_from_source(source: &Value, contract: &TokenBudgetContractConfig) -> Value {
    let mut base = json!({
        "binding_model_version": contract.rate_card_binding_model_version.clone(),
        "configured_contract_version": contract.rate_card_version.clone(),
        "configured_currency_profile": contract.currency_profile.clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "money_conversion_enabled": false,
        "status": source["status"].clone(),
        "bound_rate_card_version": Value::Null,
        "bound_currency_profile": Value::Null,
        "provider": Value::Null,
        "default_input_cost_per_1k_tokens": Value::Null,
        "default_output_cost_per_1k_tokens": Value::Null,
        "effective_from_epoch_ms": Value::Null,
        "effective_to_epoch_ms": Value::Null,
        "temporal_scope_state": "source_period_unspecified",
        "note": "Денежная конверсия включается только после честного bind на versioned rate-card file."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let rate_card = match parse_rate_card_file(&raw) {
        Ok(rate_card) => rate_card,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let money_conversion_enabled = rate_card.default_input_cost_per_1k_tokens > 0.0
        && rate_card.default_output_cost_per_1k_tokens > 0.0;
    base["status"] = Value::String(if money_conversion_enabled {
        "priced_bound".to_string()
    } else {
        "bound_but_unpriced".to_string()
    });
    base["source"]["binding_status"] = base["status"].clone();
    base["money_conversion_enabled"] = Value::Bool(money_conversion_enabled);
    base["schema_version"] = Value::String(rate_card.schema_version);
    base["bound_rate_card_version"] = Value::String(rate_card.rate_card_version);
    base["bound_currency_profile"] = Value::String(rate_card.currency_profile);
    base["provider"] = Value::String(rate_card.provider);
    base["default_input_cost_per_1k_tokens"] =
        Value::from(rate_card.default_input_cost_per_1k_tokens);
    base["default_output_cost_per_1k_tokens"] =
        Value::from(rate_card.default_output_cost_per_1k_tokens);
    base["effective_from_epoch_ms"] = match rate_card.effective_from_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["effective_to_epoch_ms"] = match rate_card.effective_to_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["temporal_scope_state"] = Value::String(
        source_temporal_scope_state(
            rate_card.effective_from_epoch_ms,
            rate_card.effective_to_epoch_ms,
        )
        .to_string(),
    );
    base
}

fn build_rate_card_json(repo_root: &Path, contract: &TokenBudgetContractConfig) -> Value {
    let source = configured_provider_rate_card_source(repo_root);
    bind_rate_card_json_from_source(&source, contract)
}

fn bind_infra_cost_profile_json_from_source(
    source: &Value,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut base = json!({
        "binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "configured_contract_version": contract.infra_cost_profile_version.clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "status": source["status"].clone(),
        "schema_version": Value::Null,
        "bound_profile_version": Value::Null,
        "bound_currency_profile": Value::Null,
        "provider": Value::Null,
        "cost_per_1k_internal_billed_tokens": Value::Null,
        "cost_per_live_event": Value::Null,
        "fixed_scope_cost_amount": Value::Null,
        "effective_from_epoch_ms": Value::Null,
        "effective_to_epoch_ms": Value::Null,
        "temporal_scope_state": "source_period_unspecified",
        "money_margin_enabled": false,
        "note": "Infra cost profile начинает влиять на margin preview только после честного bind на versioned machine-readable profile."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let profile = match parse_infra_cost_profile_file(&raw) {
        Ok(profile) => profile,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let money_margin_enabled = profile.cost_per_1k_internal_billed_tokens > 0.0
        || profile.cost_per_live_event > 0.0
        || profile.fixed_scope_cost_amount > 0.0;
    base["status"] = Value::String(if money_margin_enabled {
        "priced_bound".to_string()
    } else {
        "bound_but_unpriced".to_string()
    });
    base["source"]["binding_status"] = base["status"].clone();
    base["schema_version"] = Value::String(profile.schema_version);
    base["bound_profile_version"] = Value::String(profile.infra_cost_profile_version);
    base["bound_currency_profile"] = Value::String(profile.currency_profile);
    base["provider"] = match profile.provider {
        Some(provider) => Value::String(provider),
        None => Value::Null,
    };
    base["cost_per_1k_internal_billed_tokens"] =
        Value::from(profile.cost_per_1k_internal_billed_tokens);
    base["cost_per_live_event"] = Value::from(profile.cost_per_live_event);
    base["fixed_scope_cost_amount"] = Value::from(profile.fixed_scope_cost_amount);
    base["effective_from_epoch_ms"] = match profile.effective_from_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["effective_to_epoch_ms"] = match profile.effective_to_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["temporal_scope_state"] = Value::String(
        source_temporal_scope_state(
            profile.effective_from_epoch_ms,
            profile.effective_to_epoch_ms,
        )
        .to_string(),
    );
    base["money_margin_enabled"] = Value::Bool(money_margin_enabled);
    base
}

fn build_infra_cost_profile_json(repo_root: &Path, contract: &TokenBudgetContractConfig) -> Value {
    let source = configured_infra_cost_profile_source(repo_root);
    bind_infra_cost_profile_json_from_source(&source, contract)
}

fn build_settlement_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "statement_version": contract.settlement_statement_version.clone(),
        "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
        "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
        "correction_policy_version": contract.correction_policy_version.clone(),
        "dispute_policy_version": contract.dispute_policy_version.clone(),
        "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
        "statement_period_governance_version": contract.statement_period_governance_version.clone(),
        "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
        "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
        "telemetry_surface_split_version": contract.telemetry_surface_split_version.clone(),
        "settlement_status": contract.settlement_status.clone(),
        "current_materialized_boundary": "measured_report_only",
        "statement_lifecycle": [
            {
                "code": "live_measurement_open",
                "surface": "operational",
                "meaning": "Live token rollup ещё открыт и меняется по мере новых событий."
            },
            {
                "code": "report_only_preview_open",
                "surface": "contractual",
                "meaning": "Есть contractual preview, но billing и закрытие периода ещё не включены."
            },
            {
                "code": "report_only_preview_provisionally_stable",
                "surface": "contractual",
                "meaning": "Scope уже перестал плыть по late-arrival и ingest lag, но остаётся только report-only preview, а не закрытым statement."
            },
            {
                "code": "report_only_preview_provisional_hold",
                "surface": "contractual",
                "meaning": "Scope ещё нельзя даже provisionally считать устойчивым: есть lag, late-arrival окно, coverage gap или adjustment/dispute hold."
            },
            {
                "code": "close_blocked_report_only",
                "surface": "contractual",
                "meaning": "Период нельзя честно закрыть: settlement остаётся report-only."
            },
            {
                "code": "closed_with_adjustments_reserved",
                "surface": "future_reserved",
                "meaning": "Будущий invoice-grade слой должен использовать отдельные adjustment/credit semantics, а не тихую перезапись."
            }
        ],
        "materialized_settlement_stages": [
            {
                "code": "empty_report_only",
                "family": "empty",
                "surface": "contractual",
                "meaning": "Пока нет измеренных usage-событий даже для report-only statement preview."
            },
            {
                "code": "measured_open_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Измеренный report-only scope уже есть, но он ещё не дотянулся даже до review-ready состояния."
            },
            {
                "code": "measured_review_ready_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Measured lower bound уже provisionally stable и пригоден для review/export, но всё ещё не является billable amount."
            },
            {
                "code": "measured_adjusted_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Measured lower bound уже содержит applied report-only adjustment entries."
            },
            {
                "code": "measured_pending_adjustment_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Есть measured scope, но adjustment review ещё не завершён."
            },
            {
                "code": "measured_disputed_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Measured scope существует, но по нему открыт dispute hold."
            }
        ],
        "future_reserved_settlement_stages": future_reserved_settlement_stages(),
        "transition_contract": {
            "current_materialized_boundary": "measured_report_only",
            "future_reserved_boundary": "billable_and_beyond_reserved",
            "note": "Текущий runtime materialize-ит только measured/report-only lifecycle. Billable, settled, invoiced, credited, disputed и closed остаются зарезервированными стадиями, а не активной денежной логикой."
        },
        "current_operational_state": "live_measurement_open",
        "current_contractual_state": "report_only_preview_open",
        "freeze_close_status": "provisional_report_only",
        "late_arrival_status": "deadline_from_latest_event_report_only",
        "note": "Settlement layer остаётся report-only preview: scope уже можно честно маркировать как provisionally stable или provisional hold, но это всё ещё не денежный close workflow и не invoice."
    })
}

fn build_statement_period_json(
    scope_code: &str,
    scope_label: &str,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
    profile: &ResolvedProfile,
    contract: &TokenBudgetContractConfig,
    metering_freshness: &Value,
    provisional_close_candidate: bool,
    provisional_close_barriers: &[String],
) -> Value {
    let start_epoch_ms = match scope_code {
        "current_session" | "lifetime" => {
            events.iter().map(|event| event.occurred_at_epoch_ms).min()
        }
        "rolling_window" => profile
            .rolling_window_hours
            .map(|hours| now_epoch_ms - (hours as i64 * 60 * 60 * 1000)),
        _ => None,
    };
    let window_anchor = match scope_code {
        "current_session" => json!({
            "kind": "session_gap_minutes",
            "value": profile.session_gap_minutes,
        }),
        "rolling_window" => json!({
            "kind": "rolling_window_hours",
            "value": profile.rolling_window_hours,
        }),
        "lifetime" => json!({
            "kind": "first_recorded_event",
            "value": start_epoch_ms,
        }),
        _ => Value::Null,
    };
    let latest_event_epoch_ms = metering_freshness["latest_event_occurred_at_epoch_ms"].as_i64();
    let late_arrival_grace_ms = metering_freshness["late_arrival_grace_ms"].as_i64();
    let provisional_close_earliest_at_epoch_ms = latest_event_epoch_ms
        .zip(late_arrival_grace_ms)
        .map(|(latest, grace)| latest + grace);
    let window_state = if events.is_empty() {
        "empty_report_only"
    } else if metering_freshness["metering_ingest_state"].as_str() == Some("lagging") {
        "pipeline_lag_open_report_only"
    } else if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        "open_late_arrival_window_report_only"
    } else if provisional_close_candidate {
        "provisionally_stable_report_only"
    } else {
        "open_review_hold_report_only"
    };
    let close_policy_state = if events.is_empty() {
        "provisional_close_not_applicable_empty"
    } else if provisional_close_candidate {
        "provisional_close_candidate_report_only"
    } else {
        "provisional_close_blocked_report_only"
    };
    let late_arrival_policy_state = if events.is_empty() {
        "no_events_report_only"
    } else if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        "accepting_events_within_provisional_deadline"
    } else {
        "provisional_deadline_elapsed"
    };

    json!({
        "model_version": contract.statement_period_governance_version.clone(),
        "scope_code": scope_code,
        "scope_label": scope_label,
        "event_time_basis": "occurred_at_epoch_ms",
        "period_start_epoch_ms": start_epoch_ms,
        "period_end_epoch_ms": now_epoch_ms,
        "close_at_epoch_ms": Value::Null,
        "late_arrival_deadline_epoch_ms": provisional_close_earliest_at_epoch_ms,
        "provisional_close_earliest_at_epoch_ms": provisional_close_earliest_at_epoch_ms,
        "provisional_close_candidate": provisional_close_candidate,
        "provisional_close_barriers": provisional_close_barriers,
        "window_anchor": window_anchor,
        "window_state": window_state,
        "close_policy_state": close_policy_state,
        "late_arrival_policy_state": late_arrival_policy_state,
        "note": "Период по-прежнему report-only: close_at остаётся пустым до реального settlement workflow, но provisional deadline и provisional stability уже считаются по latest event и late-arrival policy."
    })
}

fn build_adjustment_request_schema_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "schema_version": contract.adjustment_request_schema_version.clone(),
        "required_fields": [
            "adjustment_id",
            "scope_code",
            "kind",
            "status",
            "reason_code",
            "created_at_epoch_ms"
        ],
        "allowed_kinds": [
            "credit_note",
            "adjustment_entry",
            "dispute_hold"
        ],
        "allowed_statuses": [
            "requested",
            "pending_review",
            "approved_but_unapplied",
            "applied_report_only",
            "disputed",
            "rejected"
        ],
        "retroactive_rewrite_policy": "forbidden_use_adjustment_entries",
        "note": "Adjustment request schema существует затем, чтобы corrections/disputes materialize-ились отдельными entries, а не тихой перезаписью старого statement."
    })
}

fn adjustment_entry_json(entry: &AdjustmentRegistryEntry) -> Value {
    json!({
        "adjustment_id": entry.adjustment_id,
        "scope_code": entry.scope_code,
        "kind": entry.kind,
        "status": entry.status,
        "reason_code": entry.reason_code,
        "created_at_epoch_ms": entry.created_at_epoch_ms,
        "tokens_delta": entry.tokens_delta,
        "amount_delta": entry.amount_delta,
        "currency_profile": entry.currency_profile,
        "related_statement_id": entry.related_statement_id,
    })
}

fn adjustment_status_matches(status: Option<&str>, expected: &[&str]) -> bool {
    let Some(status) = status else {
        return false;
    };
    expected.iter().any(|candidate| *candidate == status)
}

fn sum_adjustment_tokens(entries: &[Value], statuses: &[&str]) -> i64 {
    entries
        .iter()
        .filter(|entry| adjustment_status_matches(entry["status"].as_str(), statuses))
        .map(|entry| entry["tokens_delta"].as_i64().unwrap_or(0))
        .sum()
}

fn sum_adjustment_amount(entries: &[Value], statuses: &[&str]) -> f64 {
    entries
        .iter()
        .filter(|entry| adjustment_status_matches(entry["status"].as_str(), statuses))
        .map(|entry| entry["amount_delta"].as_f64().unwrap_or(0.0))
        .sum()
}

fn load_adjustment_registry_from_source(
    source: &Value,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut base = json!({
        "schema_version": contract.adjustment_registry_version.clone(),
        "request_schema_version": contract.adjustment_request_schema_version.clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "status": source["status"].clone(),
        "entries_count": 0,
        "pending_entries_count": 0,
        "applied_entries_count": 0,
        "disputed_entries_count": 0,
        "registry_hash": Value::Null,
        "scopes": {
            "current_session": {
                "entries_count": 0,
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0,
                "scope_hash": Value::Null,
            },
            "rolling_window": {
                "entries_count": 0,
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0,
                "scope_hash": Value::Null,
            },
            "lifetime": {
                "entries_count": 0,
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0,
                "scope_hash": Value::Null,
            }
        },
        "note": "Adjustment registry пока optional: без него report-only tokenonomics не переписывает прошлые периоды и не притворяется credit workflow."
    });

    let source_status = source["status"].as_str().unwrap_or("unknown");
    if !matches!(
        source_status,
        "configured_existing_path" | "default_existing_path"
    ) {
        return base;
    }

    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &content);
    let registry = match serde_json::from_str::<AdjustmentRegistryFile>(&content) {
        Ok(registry) => registry,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let entries = registry
        .adjustments
        .iter()
        .map(adjustment_entry_json)
        .collect::<Vec<_>>();

    let mut scope_map = serde_json::Map::new();
    for scope_code in ["current_session", "rolling_window", "lifetime"] {
        let scope_entries = registry
            .adjustments
            .iter()
            .filter(|entry| entry.scope_code == scope_code)
            .map(adjustment_entry_json)
            .collect::<Vec<_>>();
        let pending_entries_count = scope_entries
            .iter()
            .filter(|entry| {
                adjustment_status_matches(
                    entry["status"].as_str(),
                    &["requested", "pending_review", "approved_but_unapplied"],
                )
            })
            .count();
        let applied_entries_count = scope_entries
            .iter()
            .filter(|entry| {
                adjustment_status_matches(entry["status"].as_str(), &["applied_report_only"])
            })
            .count();
        let disputed_entries_count = scope_entries
            .iter()
            .filter(|entry| adjustment_status_matches(entry["status"].as_str(), &["disputed"]))
            .count();
        scope_map.insert(
            scope_code.to_string(),
            json!({
                "entries_count": scope_entries.len(),
                "pending_entries_count": pending_entries_count,
                "applied_entries_count": applied_entries_count,
                "disputed_entries_count": disputed_entries_count,
                "pending_tokens_delta": sum_adjustment_tokens(
                    &scope_entries,
                    &["requested", "pending_review", "approved_but_unapplied"],
                ),
                "pending_amount_delta": sum_adjustment_amount(
                    &scope_entries,
                    &["requested", "pending_review", "approved_but_unapplied"],
                ),
                "applied_tokens_delta": sum_adjustment_tokens(
                    &scope_entries,
                    &["applied_report_only"],
                ),
                "applied_amount_delta": sum_adjustment_amount(
                    &scope_entries,
                    &["applied_report_only"],
                ),
                "disputed_tokens_delta": sum_adjustment_tokens(
                    &scope_entries,
                    &["disputed"],
                ),
                "disputed_amount_delta": sum_adjustment_amount(
                    &scope_entries,
                    &["disputed"],
                ),
                "scope_hash": hash_line_items(&scope_entries).ok(),
            }),
        );
    }

    let pending_entries_count = entries
        .iter()
        .filter(|entry| {
            adjustment_status_matches(
                entry["status"].as_str(),
                &["requested", "pending_review", "approved_but_unapplied"],
            )
        })
        .count();
    let applied_entries_count = entries
        .iter()
        .filter(|entry| {
            adjustment_status_matches(entry["status"].as_str(), &["applied_report_only"])
        })
        .count();
    let disputed_entries_count = entries
        .iter()
        .filter(|entry| adjustment_status_matches(entry["status"].as_str(), &["disputed"]))
        .count();

    base["status"] = Value::String("loaded".to_string());
    base["source"]["binding_status"] = Value::String(if source_status == "default_existing_path" {
        "default_loaded".to_string()
    } else {
        "loaded".to_string()
    });
    base["entries_count"] = json!(entries.len());
    base["pending_entries_count"] = json!(pending_entries_count);
    base["applied_entries_count"] = json!(applied_entries_count);
    base["disputed_entries_count"] = json!(disputed_entries_count);
    base["pending_tokens_delta"] = json!(sum_adjustment_tokens(
        &entries,
        &["requested", "pending_review", "approved_but_unapplied"],
    ));
    base["pending_amount_delta"] = json!(sum_adjustment_amount(
        &entries,
        &["requested", "pending_review", "approved_but_unapplied"],
    ));
    base["applied_tokens_delta"] =
        json!(sum_adjustment_tokens(&entries, &["applied_report_only"],));
    base["applied_amount_delta"] =
        json!(sum_adjustment_amount(&entries, &["applied_report_only"],));
    base["disputed_tokens_delta"] = json!(sum_adjustment_tokens(&entries, &["disputed"]));
    base["disputed_amount_delta"] = json!(sum_adjustment_amount(&entries, &["disputed"]));
    base["registry_hash"] =
        Value::String(hash_line_items(&entries).unwrap_or_else(|_| "hash_error".to_string()));
    base["scopes"] = Value::Object(scope_map);
    base
}

fn build_adjustment_registry_json(repo_root: &Path, contract: &TokenBudgetContractConfig) -> Value {
    let source = configured_adjustment_registry_source(repo_root, contract);
    load_adjustment_registry_from_source(&source, contract)
}

fn build_adjustment_preview_json(
    scope_code: &str,
    contract: &TokenBudgetContractConfig,
    adjustment_registry: &Value,
) -> Value {
    let scope_summary = &adjustment_registry["scopes"][scope_code];
    let pending_entries = scope_summary["pending_entries_count"].as_u64().unwrap_or(0);
    let applied_entries = scope_summary["applied_entries_count"].as_u64().unwrap_or(0);
    let disputed_entries = scope_summary["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0);
    json!({
        "model_version": contract.adjustment_preview_model_version.clone(),
        "request_schema_version": contract.adjustment_request_schema_version.clone(),
        "registry_version": contract.adjustment_registry_version.clone(),
        "registry_status": adjustment_registry["status"].clone(),
        "status": match adjustment_registry["status"].as_str() {
            Some("loaded") => "loaded_report_only",
            Some(other) => other,
            None => "unknown",
        },
        "current_entries_count": scope_summary["entries_count"].clone(),
        "pending_entries_count": scope_summary["pending_entries_count"].clone(),
        "applied_entries_count": scope_summary["applied_entries_count"].clone(),
        "disputed_entries_count": scope_summary["disputed_entries_count"].clone(),
        "scope_hash": scope_summary["scope_hash"].clone(),
        "pending_tokens_delta": scope_summary["pending_tokens_delta"].clone(),
        "pending_amount_delta": scope_summary["pending_amount_delta"].clone(),
        "applied_tokens_delta": scope_summary["applied_tokens_delta"].clone(),
        "applied_amount_delta": scope_summary["applied_amount_delta"].clone(),
        "disputed_tokens_delta": scope_summary["disputed_tokens_delta"].clone(),
        "disputed_amount_delta": scope_summary["disputed_amount_delta"].clone(),
        "net_tokens_delta": scope_summary["applied_tokens_delta"].clone(),
        "net_amount_delta": scope_summary["applied_amount_delta"].clone(),
        "correction_action_state": if disputed_entries > 0 {
            "dispute_hold_open"
        } else if pending_entries > 0 {
            "pending_review"
        } else if applied_entries > 0 {
            "applied_report_only"
        } else {
            "no_adjustments"
        },
        "allowed_future_actions": [
            "credit_note",
            "adjustment_entry",
            "dispute_hold"
        ],
        "note": "Корректировки и credit semantics materialize-ятся отдельным registry слоем: report-only preview не переписывает прошлые statement задним числом."
    })
}

fn binding_currency_profile(binding: &Value) -> Value {
    if !binding["bound_currency_profile"].is_null() {
        binding["bound_currency_profile"].clone()
    } else {
        binding["currency_profile"].clone()
    }
}

fn binding_bound_version(binding: &Value) -> Value {
    for key in [
        "bound_rate_card_version",
        "bound_profile_version",
        "schema_version",
    ] {
        if !binding[key].is_null() {
            return binding[key].clone();
        }
    }
    Value::Null
}

fn build_external_truth_manifest_entry(binding: &Value) -> Value {
    json!({
        "status": binding["status"].clone(),
        "binding_status": binding["source"]["binding_status"].clone(),
        "resolved_path": binding["source"]["resolved_path"].clone(),
        "source_bytes": binding["source_bytes"].clone(),
        "source_sha256": binding["source_sha256"].clone(),
        "source_last_modified_epoch_ms": binding["source_last_modified_epoch_ms"].clone(),
        "schema_version": binding["schema_version"].clone(),
        "bound_version": binding_bound_version(binding),
        "provider": binding["provider"].clone(),
        "currency_profile": binding_currency_profile(binding),
    })
}

fn build_external_truth_manifest(
    contract: &TokenBudgetContractConfig,
    rate_card: &Value,
    infra_cost_profile: &Value,
    provider_usage_binding: &Value,
    provider_invoice_binding: &Value,
    adjustment_registry: &Value,
) -> Value {
    let entries = json!({
        "provider_usage_export": build_external_truth_manifest_entry(provider_usage_binding),
        "provider_invoice_export": build_external_truth_manifest_entry(provider_invoice_binding),
        "provider_rate_card": build_external_truth_manifest_entry(rate_card),
        "infra_cost_profile": build_external_truth_manifest_entry(infra_cost_profile),
        "token_adjustment_registry": build_external_truth_manifest_entry(adjustment_registry),
    });
    let manifest_hash = serde_json::to_vec(&entries)
        .map(|bytes| hex_sha256(&bytes))
        .unwrap_or_else(|_| "hash_error".to_string());
    json!({
        "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
        "statement_export_version": contract.contractual_statement_export_version.clone(),
        "evidence_pack_version": contract.contractual_evidence_pack_version.clone(),
        "entries": entries,
        "manifest_hash": manifest_hash,
        "note": "External truth manifest фиксирует fingerprint привязанных usage/invoice/rate-card/infra/adjustment sources. Это audit trail для contractual review, а не invoice-grade settlement."
    })
}

fn build_telemetry_surfaces_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "model_version": contract.telemetry_surface_split_version.clone(),
        "operational_surface": {
            "code": "engineering_live_telemetry",
            "intended_consumers": [
                "dashboard",
                "observability",
                "engineers"
            ],
            "fields": [
                "headline",
                "current_session",
                "rolling_window",
                "lifetime",
                "source_breakdown",
                "query_slices",
                "baseline_strategy_slices",
                "temperature_slices"
            ],
            "not_for": [
                "invoice",
                "settlement",
                "customer_billing"
            ]
        },
        "contractual_surface": {
            "code": "report_only_tokenonomics_contract",
            "intended_consumers": [
                "customer_review",
                "audit",
                "finance_preparation"
            ],
            "fields": [
                "usage_event_schema",
                "metering_freshness_contract",
                "baseline_contract",
                "billing_policy",
                "rate_card",
                "settlement_contract",
                "metering_freshness",
                "statement_previews",
                "reconciliation_contract",
                "reconciliation_previews",
                "infra_cost_profile",
                "margin_contract",
                "margin_view",
                "adjustment_request_schema",
                "adjustment_registry",
                "statement_export_previews",
                "contractual_evidence_pack"
            ],
            "state": "report_only_preview",
            "not_for": [
                "live_latency_tuning",
                "hot_path_benchmarking"
            ]
        },
        "note": "Operational telemetry и contractual tokenonomics intentionally split: dashboard live rollups нельзя трактовать как invoice или закрытый statement."
    })
}

fn build_external_truth_sources_json(repo_root: &Path) -> Value {
    json!({
        "provider_usage_export": configured_provider_usage_source(repo_root),
        "provider_invoice_export": configured_provider_invoice_source(repo_root),
        "provider_rate_card": configured_provider_rate_card_source(repo_root),
        "infra_cost_profile": configured_infra_cost_profile_source(repo_root),
    })
}

fn external_truth_source_roles(code: &str) -> Value {
    match code {
        "provider_usage_export" => json!({
            "required_for_usage_truth": true,
            "required_for_cost_truth": true,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": true,
        }),
        "provider_rate_card" => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": true,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": true,
        }),
        "provider_invoice_export" => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": false,
            "required_for_invoice_evidence": true,
            "required_for_margin_truth": false,
        }),
        "infra_cost_profile" => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": false,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": true,
        }),
        _ => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": false,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": false,
        }),
    }
}

fn source_codes_with_truth_role(external_sources: &Value, role_key: &str) -> Value {
    let mut codes = external_sources
        .as_object()
        .into_iter()
        .flat_map(|entries| entries.values())
        .filter(|source| source["truth_roles"][role_key].as_bool() == Some(true))
        .filter_map(|source| source["code"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    codes.sort();
    Value::Array(codes.into_iter().map(Value::String).collect())
}

fn missing_source_codes_json(missing_codes: Vec<&'static str>) -> Value {
    let mut codes = missing_codes
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    codes.sort();
    Value::Array(codes.into_iter().map(Value::String).collect())
}

fn provider_usage_truth_bound(status: &str) -> bool {
    matches!(status, "usage_bound" | "usage_and_cost_bound")
}

fn provider_usage_cost_truth_bound(status: &str) -> bool {
    status == "usage_and_cost_bound"
}

fn rate_card_priced_bound(status: &str) -> bool {
    status == "priced_bound"
}

fn infra_cost_profile_priced_bound(status: &str) -> bool {
    status == "priced_bound"
}

fn provider_invoice_bound(status: &str) -> bool {
    status == "invoice_bound"
}

fn configured_external_truth_source(
    repo_root: &Path,
    env_var: &str,
    code: &str,
    label: &str,
    required_for_reconciliation: bool,
) -> Value {
    let configured_value = std::env::var(env_var)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let resolved_path = configured_value.as_ref().map(|raw| {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            repo_root.join(candidate)
        }
    });
    let path_exists = resolved_path
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or(false);
    let status = match (configured_value.as_ref(), path_exists) {
        (None, _) => "not_configured",
        (Some(_), false) => "configured_path_missing",
        (Some(_), true) => "configured_existing_path",
    };
    let binding_status = match status {
        "not_configured" => "not_configured",
        "configured_path_missing" => "configured_path_missing",
        "configured_existing_path" => "configured_but_unbound",
        _ => "unknown",
    };
    json!({
        "code": code,
        "label": label,
        "env_var": env_var,
        "required_for_reconciliation": required_for_reconciliation,
        "truth_roles": external_truth_source_roles(code),
        "configured_value": configured_value,
        "resolved_path": resolved_path.map(|path| path.display().to_string()),
        "status": status,
        "binding_status": binding_status,
        "note": "Источник может уже существовать как файл, но пока Amai не привязывает его автоматически к canonical reconciliation ledger."
    })
}

fn configured_defaultable_external_truth_source(
    repo_root: &Path,
    env_var: &str,
    code: &str,
    label: &str,
    required_for_reconciliation: bool,
    default_relative_path: &str,
    note: &str,
) -> Value {
    let source = configured_external_truth_source(
        repo_root,
        env_var,
        code,
        label,
        required_for_reconciliation,
    );
    if source["status"].as_str() != Some("not_configured") {
        return source;
    }
    let default_path = repo_root.join(default_relative_path);
    let default_exists = default_path.exists();
    json!({
        "code": code,
        "label": label,
        "env_var": env_var,
        "required_for_reconciliation": required_for_reconciliation,
        "truth_roles": external_truth_source_roles(code),
        "configured_value": Value::Null,
        "resolved_path": default_path.display().to_string(),
        "status": if default_exists {
            "default_existing_path"
        } else {
            "default_path_missing"
        },
        "binding_status": if default_exists {
            "default_but_unbound"
        } else {
            "not_configured"
        },
        "note": note
    })
}

fn adjustment_registry_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/token_adjustment_registry.json")
}

fn provider_usage_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/provider_usage_export.json")
}

fn provider_invoice_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/provider_invoice_export.json")
}

fn provider_rate_card_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/provider_rate_card.json")
}

fn infra_cost_profile_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/infra_cost_profile.json")
}

fn configured_adjustment_registry_source(
    repo_root: &Path,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let source = configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH",
        "token_adjustment_registry",
        "Report-only registry для correction/credit/dispute entries",
        false,
        "state/token_adjustment_registry.json",
        "Если env-binding не задан, token adjustment registry может жить в repo-local state/token_adjustment_registry.json как operator-safe report-only ledger.",
    );
    if source["status"].as_str() == Some("not_configured") {
        return source;
    }
    let mut enriched = source;
    enriched["schema_version"] = Value::String(contract.adjustment_registry_version.clone());
    enriched
}

fn configured_provider_usage_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_PROVIDER_USAGE_EXPORT_PATH",
        "provider_usage_export",
        "Выгрузка usage/tokens от внешнего model provider",
        true,
        "state/provider_usage_export.json",
        "Если env-binding не задан, provider usage export может жить в repo-local state/provider_usage_export.json как report-only reconciliation source.",
    )
}

fn configured_provider_invoice_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_PROVIDER_INVOICE_EXPORT_PATH",
        "provider_invoice_export",
        "Invoice/export от внешнего provider",
        false,
        "state/provider_invoice_export.json",
        "Если env-binding не задан, provider invoice export может жить в repo-local state/provider_invoice_export.json как optional settlement-side evidence source.",
    )
}

fn configured_provider_rate_card_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_PROVIDER_RATE_CARD_PATH",
        "provider_rate_card",
        "Versioned rate-card для денежной конверcии tokenonomics",
        true,
        "state/provider_rate_card.json",
        "Если env-binding не задан, provider rate card может жить в repo-local state/provider_rate_card.json как versioned money conversion source.",
    )
}

fn configured_infra_cost_profile_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_INFRA_COST_PROFILE_PATH",
        "infra_cost_profile",
        "Профиль собственных infra costs для Amai",
        false,
        "state/infra_cost_profile.json",
        "Если env-binding не задан, infra cost profile может жить в repo-local state/infra_cost_profile.json как report-only margin source.",
    )
}

fn provider_usage_total_tokens(entry: &ProviderUsageScopeEntry) -> Option<u64> {
    entry
        .total_tokens
        .or_else(|| match (entry.input_tokens, entry.output_tokens) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            (Some(input), None) => Some(input),
            (None, Some(output)) => Some(output),
            (None, None) => None,
        })
}

fn provider_usage_cost_amount(entry: &ProviderUsageScopeEntry, rate_card: &Value) -> Option<f64> {
    if let Some(amount) = entry.provider_cost_amount {
        return Some(amount);
    }
    let input_rate = rate_card["default_input_cost_per_1k_tokens"].as_f64()?;
    let output_rate = rate_card["default_output_cost_per_1k_tokens"].as_f64()?;
    let input_tokens = entry.input_tokens?;
    let output_tokens = entry.output_tokens?;
    Some(
        (input_tokens as f64 / 1000.0) * input_rate + (output_tokens as f64 / 1000.0) * output_rate,
    )
}

fn internal_provider_cost_estimate_amount(
    internal_provider_billed_tokens: u64,
    rate_card: &Value,
) -> Option<f64> {
    let input_rate = rate_card["default_input_cost_per_1k_tokens"].as_f64()?;
    Some((internal_provider_billed_tokens as f64 / 1000.0) * input_rate)
}

fn amount_delta(lhs: Option<f64>, rhs: Option<f64>) -> Value {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => json!(lhs - rhs),
        _ => Value::Null,
    }
}

fn source_temporal_scope_state(
    start_epoch_ms: Option<i64>,
    end_epoch_ms: Option<i64>,
) -> &'static str {
    match (start_epoch_ms, end_epoch_ms) {
        (None, None) => "source_period_unspecified",
        (Some(_), Some(_)) => "source_period_bounded",
        _ => "source_period_partially_bound",
    }
}

fn statement_period_bounds(statement_preview: &Value) -> (Option<i64>, Option<i64>) {
    (
        statement_preview["period"]["period_start_epoch_ms"].as_i64(),
        statement_preview["period"]["period_end_epoch_ms"].as_i64(),
    )
}

fn scope_period_alignment_state(
    scope_start_epoch_ms: Option<i64>,
    scope_end_epoch_ms: Option<i64>,
    source_start_epoch_ms: Option<i64>,
    source_end_epoch_ms: Option<i64>,
) -> &'static str {
    match (scope_start_epoch_ms, scope_end_epoch_ms) {
        (Some(scope_start), Some(scope_end)) => {
            match (source_start_epoch_ms, source_end_epoch_ms) {
                (Some(source_start), Some(source_end))
                    if source_start <= scope_start && source_end >= scope_end =>
                {
                    "scope_period_aligned"
                }
                (Some(_), Some(_)) => "scope_period_mismatch",
                (None, None) => "source_period_unspecified",
                _ => "source_period_partially_bound",
            }
        }
        _ => "scope_period_unknown",
    }
}

fn combined_temporal_truth_state(states: &[&str]) -> &'static str {
    if states.contains(&"scope_period_mismatch") {
        "scope_period_mismatch"
    } else if states.contains(&"source_period_partially_bound") {
        "source_period_partially_bound"
    } else if states.contains(&"source_period_unspecified") {
        "source_period_unspecified"
    } else if states.contains(&"scope_period_unknown") {
        "scope_period_unknown"
    } else if states.iter().all(|state| *state == "scope_period_aligned") {
        "scope_period_aligned"
    } else {
        "scope_period_unchecked"
    }
}

fn bound_provider_name<'a>(binding: &'a Value, expected_statuses: &[&str]) -> Option<&'a str> {
    let status = binding["status"].as_str()?;
    if !expected_statuses.contains(&status) {
        return None;
    }
    binding["provider"].as_str()
}

fn provider_alignment_state(
    lhs_provider: Option<&str>,
    rhs_provider: Option<&str>,
) -> &'static str {
    match (lhs_provider, rhs_provider) {
        (Some(lhs), Some(rhs)) if lhs == rhs => "provider_identity_aligned",
        (Some(_), Some(_)) => "provider_identity_mismatch",
        _ => "provider_identity_unchecked",
    }
}

fn combined_provider_identity_state(states: &[&str]) -> &'static str {
    if states.contains(&"provider_identity_mismatch") {
        "provider_identity_mismatch"
    } else if states
        .iter()
        .all(|state| *state == "provider_identity_aligned")
    {
        "provider_identity_aligned"
    } else {
        "provider_identity_unchecked"
    }
}

fn base_reconciliation_blocking_reasons(
    statement_preview: &Value,
    rate_card: &Value,
    include_provider_usage_missing: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if include_provider_usage_missing {
        reasons.push("provider_usage_source_missing");
    }
    if rate_card["money_conversion_enabled"].as_bool() != Some(true) {
        reasons.push("provider_rate_card_unpriced");
    }
    reasons.push("billing_policy_report_only");
    if statement_preview["billable_lower_bound_tokens"].is_null() {
        reasons.push("billable_lower_bound_not_materialized");
    }
    reasons
}

fn usage_truth_completeness_state(provider_usage_status: &str) -> &'static str {
    if matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    ) {
        "awaiting_provider_usage_source"
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_usage_source_error"
    } else if matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        "provider_usage_bound"
    } else {
        "provider_usage_not_yet_bound"
    }
}

fn provider_cost_truth_completeness_state(
    provider_usage_status: &str,
    rate_card_status: &str,
) -> &'static str {
    if !matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        "no_external_cost_truth"
    } else if matches!(rate_card_status, "not_configured" | "default_path_missing") {
        "awaiting_rate_card_source"
    } else if matches!(
        rate_card_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_rate_card_error"
    } else if provider_usage_status == "usage_and_cost_bound" {
        "provider_cost_bound"
    } else if rate_card_status == "bound_but_unpriced" {
        "rate_card_bound_unpriced"
    } else {
        "rate_card_bound_internal_estimate_only"
    }
}

fn invoice_evidence_completeness_state(
    provider_usage_status: &str,
    provider_invoice_status: &str,
) -> &'static str {
    if !matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        "no_invoice_evidence_scope"
    } else if matches!(
        provider_invoice_status,
        "not_configured" | "default_path_missing"
    ) {
        "awaiting_provider_invoice_source"
    } else if matches!(
        provider_invoice_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_invoice_source_error"
    } else if provider_invoice_status == "invoice_bound" {
        "provider_invoice_bound"
    } else {
        "provider_invoice_not_yet_bound"
    }
}

fn money_truth_completeness_state(
    provider_cost_truth_state: &str,
    invoice_evidence_truth_state: &str,
) -> &'static str {
    match provider_cost_truth_state {
        "no_external_cost_truth" => "no_external_money_truth",
        "awaiting_rate_card_source" => "awaiting_rate_card_source",
        "provider_rate_card_error" => "provider_rate_card_error",
        "rate_card_bound_unpriced" => "rate_card_bound_unpriced",
        "provider_cost_bound" => match invoice_evidence_truth_state {
            "provider_invoice_source_error" => "provider_invoice_source_error",
            "provider_invoice_bound" => "provider_cost_and_invoice_bound",
            _ => "provider_cost_bound_without_invoice",
        },
        "rate_card_bound_internal_estimate_only" => match invoice_evidence_truth_state {
            "provider_invoice_source_error" => "provider_invoice_source_error",
            _ => "rate_card_bound_internal_estimate_only",
        },
        _ => "provider_cost_truth_not_yet_bound",
    }
}

fn rate_card_truth_completeness_state(rate_card_status: &str) -> &'static str {
    if matches!(rate_card_status, "not_configured" | "default_path_missing") {
        "awaiting_rate_card_source"
    } else if matches!(
        rate_card_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_rate_card_error"
    } else if rate_card_status == "priced_bound" {
        "rate_card_priced_bound"
    } else if rate_card_status == "bound_but_unpriced" {
        "rate_card_bound_unpriced"
    } else {
        "rate_card_not_yet_bound"
    }
}

fn infra_cost_truth_completeness_state(infra_cost_status: &str) -> &'static str {
    if matches!(infra_cost_status, "not_configured" | "default_path_missing") {
        "awaiting_infra_cost_profile"
    } else if matches!(
        infra_cost_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "infra_cost_profile_error"
    } else if infra_cost_status == "priced_bound" {
        "infra_cost_profile_priced_bound"
    } else if infra_cost_status == "bound_but_unpriced" {
        "infra_cost_profile_bound_unpriced"
    } else {
        "infra_cost_profile_not_yet_bound"
    }
}

fn pricing_truth_completeness_state(
    rate_card_truth_state: &str,
    infra_cost_truth_state: &str,
) -> &'static str {
    if matches!(rate_card_truth_state, "provider_rate_card_error")
        || matches!(infra_cost_truth_state, "infra_cost_profile_error")
    {
        "pricing_truth_source_error"
    } else if rate_card_truth_state == "rate_card_priced_bound"
        && infra_cost_truth_state == "infra_cost_profile_priced_bound"
    {
        "pricing_truth_ready"
    } else if matches!(
        rate_card_truth_state,
        "awaiting_rate_card_source" | "rate_card_not_yet_bound"
    ) && matches!(
        infra_cost_truth_state,
        "awaiting_infra_cost_profile" | "infra_cost_profile_not_yet_bound"
    ) {
        "awaiting_rate_card_and_infra_cost_profile"
    } else if matches!(
        rate_card_truth_state,
        "awaiting_rate_card_source" | "rate_card_not_yet_bound"
    ) {
        "awaiting_rate_card_source"
    } else if matches!(
        infra_cost_truth_state,
        "awaiting_infra_cost_profile" | "infra_cost_profile_not_yet_bound"
    ) {
        "awaiting_infra_cost_profile"
    } else if matches!(rate_card_truth_state, "rate_card_bound_unpriced")
        || matches!(infra_cost_truth_state, "infra_cost_profile_bound_unpriced")
    {
        "pricing_truth_bound_unpriced"
    } else {
        "pricing_truth_partially_bound"
    }
}

fn customer_savings_money_truth_completeness_state(rate_card_truth_state: &str) -> &'static str {
    match rate_card_truth_state {
        "provider_rate_card_error" => "customer_savings_money_truth_source_error",
        "awaiting_rate_card_source" | "rate_card_not_yet_bound" => "awaiting_rate_card_source",
        "rate_card_bound_unpriced" => "rate_card_bound_unpriced",
        "rate_card_priced_bound" => "customer_savings_lower_bound_ready_report_only",
        _ => "customer_savings_money_truth_not_yet_bound",
    }
}

fn amai_cost_truth_completeness_state(infra_cost_truth_state: &str) -> &'static str {
    match infra_cost_truth_state {
        "infra_cost_profile_error" => "amai_cost_truth_source_error",
        "awaiting_infra_cost_profile" | "infra_cost_profile_not_yet_bound" => {
            "awaiting_infra_cost_profile"
        }
        "infra_cost_profile_bound_unpriced" => "infra_cost_profile_bound_unpriced",
        "infra_cost_profile_priced_bound" => "amai_cost_preview_ready_report_only",
        _ => "amai_cost_truth_not_yet_bound",
    }
}

fn margin_truth_completeness_state(
    customer_savings_truth_state: &str,
    amai_cost_truth_state: &str,
) -> &'static str {
    if matches!(
        customer_savings_truth_state,
        "customer_savings_money_truth_source_error"
    ) || matches!(amai_cost_truth_state, "amai_cost_truth_source_error")
    {
        "margin_truth_source_error"
    } else if customer_savings_truth_state == "customer_savings_lower_bound_ready_report_only"
        && amai_cost_truth_state == "amai_cost_preview_ready_report_only"
    {
        "margin_preview_amounts_ready_report_only"
    } else if matches!(
        customer_savings_truth_state,
        "awaiting_rate_card_source" | "customer_savings_money_truth_not_yet_bound"
    ) && matches!(
        amai_cost_truth_state,
        "awaiting_infra_cost_profile" | "amai_cost_truth_not_yet_bound"
    ) {
        "awaiting_rate_card_and_infra_cost_profile"
    } else if matches!(
        customer_savings_truth_state,
        "awaiting_rate_card_source" | "customer_savings_money_truth_not_yet_bound"
    ) {
        "awaiting_rate_card_source"
    } else if matches!(
        amai_cost_truth_state,
        "awaiting_infra_cost_profile" | "amai_cost_truth_not_yet_bound"
    ) {
        "awaiting_infra_cost_profile"
    } else if customer_savings_truth_state == "rate_card_bound_unpriced"
        || amai_cost_truth_state == "infra_cost_profile_bound_unpriced"
    {
        "margin_truth_bound_unpriced"
    } else {
        "margin_truth_partially_bound"
    }
}

fn reconciliation_readiness_state(
    usage_truth_completeness_state: &str,
    provider_cost_truth_completeness_state: &str,
    invoice_evidence_completeness_state: &str,
) -> &'static str {
    match usage_truth_completeness_state {
        "awaiting_provider_usage_source" => "awaiting_provider_usage_source",
        "provider_usage_source_error" => "provider_usage_source_error",
        "provider_usage_not_yet_bound" => "provider_usage_not_yet_bound",
        _ => match provider_cost_truth_completeness_state {
            "awaiting_rate_card_source" => "usage_truth_bound_not_priced",
            "provider_rate_card_error" => "usage_truth_bound_rate_card_error",
            "provider_cost_bound" => match invoice_evidence_completeness_state {
                "provider_invoice_source_error" => "usage_cost_truth_ready_invoice_source_error",
                "provider_invoice_bound" => "usage_cost_and_invoice_truth_ready",
                _ => "usage_and_cost_truth_ready",
            },
            "rate_card_bound_unpriced" => "usage_truth_bound_unpriced",
            "rate_card_bound_internal_estimate_only" => "usage_truth_bound_internal_estimate_only",
            _ => "usage_truth_bound_not_priced",
        },
    }
}

fn reconciliation_governance_blocking_reasons(
    provider_usage_status: &str,
    rate_card_status: &str,
    provider_invoice_status: &str,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    ) {
        reasons.push("provider_usage_source_missing");
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        reasons.push("provider_usage_source_error");
    }
    if matches!(rate_card_status, "not_configured" | "default_path_missing") {
        reasons.push("provider_rate_card_unpriced");
    } else if matches!(
        rate_card_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        reasons.push("provider_rate_card_error");
    }
    if matches!(
        provider_invoice_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        reasons.push("provider_invoice_source_error");
    }
    reasons
}

fn load_provider_usage_binding_from_source(source: &Value, rate_card: &Value) -> Value {
    let mut base = json!({
        "status": source["status"].clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "schema_version": Value::Null,
        "provider": Value::Null,
        "bound_currency_profile": Value::Null,
        "scope_count": 0,
        "scopes": {},
        "cost_binding_status": if rate_card["money_conversion_enabled"].as_bool() == Some(true) {
            "awaiting_usage_export"
        } else {
            "unpriced_rate_card"
        },
        "note": "Provider usage binding должен показывать реальные billed tokens по scope, а не подменять их lower-bound savings."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let export = match parse_provider_usage_export_file(&raw) {
        Ok(export) => export,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let mut scope_map = serde_json::Map::new();
    let mut has_any_cost = false;
    for entry in &export.scopes {
        let total_tokens = provider_usage_total_tokens(entry);
        let cost_amount = provider_usage_cost_amount(entry, rate_card);
        if cost_amount.is_some() {
            has_any_cost = true;
        }
        scope_map.insert(
            entry.scope_code.clone(),
            json!({
                "input_tokens": entry.input_tokens,
                "output_tokens": entry.output_tokens,
                "total_tokens": total_tokens,
                "provider_cost_amount": cost_amount,
                "period_start_epoch_ms": entry.period_start_epoch_ms,
                "period_end_epoch_ms": entry.period_end_epoch_ms,
                "temporal_scope_state": source_temporal_scope_state(entry.period_start_epoch_ms, entry.period_end_epoch_ms),
                "currency_profile": entry
                    .currency_profile
                    .clone()
                    .or_else(|| export.currency_profile.clone()),
            }),
        );
    }

    base["schema_version"] = Value::String(export.schema_version);
    base["provider"] = Value::String(export.provider);
    base["bound_currency_profile"] = match export.currency_profile {
        Some(currency) => Value::String(currency),
        None => Value::Null,
    };
    base["scope_count"] = json!(scope_map.len());
    base["scopes"] = Value::Object(scope_map);
    base["status"] = Value::String(if has_any_cost {
        "usage_and_cost_bound".to_string()
    } else {
        "usage_bound".to_string()
    });
    base["source"]["binding_status"] = base["status"].clone();
    base["cost_binding_status"] = Value::String(if has_any_cost {
        "cost_bound".to_string()
    } else if rate_card["money_conversion_enabled"].as_bool() == Some(true) {
        "usage_bound_cost_unavailable".to_string()
    } else {
        "unpriced_rate_card".to_string()
    });
    base
}

fn load_provider_invoice_binding_from_source(source: &Value) -> Value {
    let mut base = json!({
        "status": source["status"].clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "schema_version": Value::Null,
        "provider": Value::Null,
        "bound_currency_profile": Value::Null,
        "scope_count": 0,
        "scopes": {},
        "note": "Provider invoice binding остаётся optional: он не подменяет usage truth, а только даёт отдельный settlement-side evidence слой."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let export = match parse_provider_invoice_export_file(&raw) {
        Ok(export) => export,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let mut scope_map = serde_json::Map::new();
    for entry in &export.scopes {
        scope_map.insert(
            entry.scope_code.clone(),
            json!({
                "invoice_amount": entry.invoice_amount,
                "period_start_epoch_ms": entry.period_start_epoch_ms,
                "period_end_epoch_ms": entry.period_end_epoch_ms,
                "temporal_scope_state": source_temporal_scope_state(entry.period_start_epoch_ms, entry.period_end_epoch_ms),
                "currency_profile": entry
                    .currency_profile
                    .clone()
                    .or_else(|| export.currency_profile.clone()),
                "invoice_id": entry.invoice_id,
            }),
        );
    }

    base["schema_version"] = Value::String(export.schema_version);
    base["provider"] = Value::String(export.provider);
    base["bound_currency_profile"] = match export.currency_profile {
        Some(currency) => Value::String(currency),
        None => Value::Null,
    };
    base["scope_count"] = json!(scope_map.len());
    base["scopes"] = Value::Object(scope_map);
    base["status"] = Value::String("invoice_bound".to_string());
    base["source"]["binding_status"] = Value::String("invoice_bound".to_string());
    base
}

fn provider_usage_scope_alignment_state(
    statement_preview: &Value,
    provider_usage_binding: &Value,
    scope_code: &str,
) -> &'static str {
    let provider_usage_status = provider_usage_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    if !matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        return "provider_usage_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    let scope_entry = &provider_usage_binding["scopes"][scope_code];
    scope_period_alignment_state(
        scope_start,
        scope_end,
        scope_entry["period_start_epoch_ms"].as_i64(),
        scope_entry["period_end_epoch_ms"].as_i64(),
    )
}

fn provider_invoice_scope_alignment_state(
    statement_preview: &Value,
    provider_invoice_binding: &Value,
    scope_code: &str,
) -> &'static str {
    let provider_invoice_status = provider_invoice_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    if provider_invoice_status != "invoice_bound" {
        return "invoice_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    let scope_entry = &provider_invoice_binding["scopes"][scope_code];
    scope_period_alignment_state(
        scope_start,
        scope_end,
        scope_entry["period_start_epoch_ms"].as_i64(),
        scope_entry["period_end_epoch_ms"].as_i64(),
    )
}

fn rate_card_scope_alignment_state(statement_preview: &Value, rate_card: &Value) -> &'static str {
    let rate_card_status = rate_card["status"].as_str().unwrap_or("not_configured");
    if !matches!(rate_card_status, "priced_bound" | "bound_but_unpriced") {
        return "rate_card_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    scope_period_alignment_state(
        scope_start,
        scope_end,
        rate_card["effective_from_epoch_ms"].as_i64(),
        rate_card["effective_to_epoch_ms"].as_i64(),
    )
}

fn infra_cost_scope_alignment_state(
    statement_preview: &Value,
    infra_cost_profile: &Value,
) -> &'static str {
    let infra_cost_status = infra_cost_profile["status"]
        .as_str()
        .unwrap_or("not_configured");
    if !matches!(infra_cost_status, "priced_bound" | "bound_but_unpriced") {
        return "infra_cost_profile_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    scope_period_alignment_state(
        scope_start,
        scope_end,
        infra_cost_profile["effective_from_epoch_ms"].as_i64(),
        infra_cost_profile["effective_to_epoch_ms"].as_i64(),
    )
}

fn build_reconciliation_contract_json(
    contract: &TokenBudgetContractConfig,
    external_sources: &Value,
    provider_usage_binding: &Value,
    provider_invoice_binding: &Value,
    rate_card: &Value,
) -> Value {
    let provider_usage_status = provider_usage_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_status = rate_card["status"].as_str().unwrap_or("not_configured");
    let provider_invoice_status = provider_invoice_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let provider_usage_missing = matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    );
    let rate_card_missing = matches!(rate_card_status, "not_configured" | "default_path_missing");
    let usage_truth_state = usage_truth_completeness_state(provider_usage_status);
    let rate_card_truth_state = rate_card_truth_completeness_state(rate_card_status);
    let provider_cost_truth_state =
        provider_cost_truth_completeness_state(provider_usage_status, rate_card_status);
    let invoice_evidence_truth_state =
        invoice_evidence_completeness_state(provider_usage_status, provider_invoice_status);
    let money_truth_state =
        money_truth_completeness_state(provider_cost_truth_state, invoice_evidence_truth_state);
    let reconciliation_readiness_state = reconciliation_readiness_state(
        usage_truth_state,
        provider_cost_truth_state,
        invoice_evidence_truth_state,
    );
    let governance_blocking_reasons = reconciliation_governance_blocking_reasons(
        provider_usage_status,
        rate_card_status,
        provider_invoice_status,
    );
    let usage_provider = bound_provider_name(
        provider_usage_binding,
        &["usage_bound", "usage_and_cost_bound"],
    );
    let rate_card_provider =
        bound_provider_name(rate_card, &["priced_bound", "bound_but_unpriced"]);
    let invoice_provider = bound_provider_name(provider_invoice_binding, &["invoice_bound"]);
    let rate_card_provider_alignment_state =
        provider_alignment_state(usage_provider, rate_card_provider);
    let invoice_provider_alignment_state =
        provider_alignment_state(usage_provider, invoice_provider);
    let provider_identity_state = combined_provider_identity_state(&[
        rate_card_provider_alignment_state,
        invoice_provider_alignment_state,
    ]);
    let required_sources_for_usage_truth =
        source_codes_with_truth_role(external_sources, "required_for_usage_truth");
    let required_sources_for_cost_truth =
        source_codes_with_truth_role(external_sources, "required_for_cost_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_usage_truth =
        missing_source_codes_json(if provider_usage_truth_bound(provider_usage_status) {
            Vec::new()
        } else {
            vec!["provider_usage_export"]
        });
    let unready_required_sources_for_cost_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_cost_truth_bound(provider_usage_status) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card_status) {
            missing.push("provider_rate_card");
        }
        missing
    });
    let unready_optional_sources_for_invoice_evidence =
        missing_source_codes_json(if provider_invoice_bound(provider_invoice_status) {
            Vec::new()
        } else {
            vec!["provider_invoice_export"]
        });
    let mut governance_blocking_reasons = governance_blocking_reasons;
    if provider_identity_state == "provider_identity_mismatch"
        && !governance_blocking_reasons.contains(&"provider_identity_mismatch")
    {
        governance_blocking_reasons.push("provider_identity_mismatch");
    }
    let ready_for_external_reconciliation = matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    );
    let status = if ready_for_external_reconciliation {
        if provider_invoice_status == "invoice_bound" {
            "usage_and_invoice_bound_report_only"
        } else if provider_usage_status == "usage_and_cost_bound" {
            "usage_and_cost_bound_report_only"
        } else {
            "usage_bound_report_only"
        }
    } else if provider_usage_missing {
        "awaiting_provider_usage_source"
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_usage_source_error"
    } else if rate_card_missing {
        "awaiting_rate_card_source"
    } else {
        "configured_sources_not_yet_bound"
    };

    json!({
        "contract_version": contract.reconciliation_contract_version.clone(),
        "status": status,
        "ready_for_external_reconciliation": ready_for_external_reconciliation,
        "usage_truth_completeness_state": usage_truth_state,
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "provider_cost_truth_completeness_state": provider_cost_truth_state,
        "invoice_evidence_completeness_state": invoice_evidence_truth_state,
        "money_truth_completeness_state": money_truth_state,
        "reconciliation_readiness_state": reconciliation_readiness_state,
        "governance_blocking_reasons": governance_blocking_reasons,
        "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
        "invoice_provider_alignment_state": invoice_provider_alignment_state,
        "provider_identity_state": provider_identity_state,
        "internal_truth_layers": [
            "token_budget_event",
            "usage_event_schema",
            "statement_previews",
            "agent_cycle_economics"
        ],
        "canonical_internal_scope": "retrieval savings floor + partial whole-agent-cycle lower bound + internal delivered token accounting",
        "external_truth_sources": external_sources.clone(),
        "external_truth_bindings": {
            "provider_usage_export": provider_usage_binding.clone(),
            "provider_invoice_export": provider_invoice_binding.clone(),
            "provider_rate_card": rate_card.clone(),
        },
        "source_requirements": {
            "required_sources_for_usage_truth": required_sources_for_usage_truth,
            "required_sources_for_cost_truth": required_sources_for_cost_truth,
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth,
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth,
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence,
        },
        "note": "Amai уже меряет внутренний lower bound честно, но external reconciliation должен сравнивать provider usage с внутренними delivered tokens, а не с saved tokens. Governance-layer отдельно показывает, дошли ли мы только до usage truth, до usage+cost truth или уже до invoice-side evidence. Это reconciliation contract, а не готовый settlement engine."
    })
}

fn build_reconciliation_preview(
    scope_code: &str,
    scope_label: &str,
    statement_preview: &Value,
    contract: &TokenBudgetContractConfig,
    external_sources: &Value,
    provider_usage_binding: &Value,
    provider_invoice_binding: &Value,
    rate_card: &Value,
) -> Value {
    let provider_usage_status = provider_usage_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_status = rate_card["status"].as_str().unwrap_or("not_configured");
    let provider_invoice_status = provider_invoice_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let provider_usage_missing = matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    );
    let rate_card_missing = matches!(rate_card_status, "not_configured" | "default_path_missing");
    let usage_truth_state = usage_truth_completeness_state(provider_usage_status);
    let rate_card_truth_state = rate_card_truth_completeness_state(rate_card_status);
    let provider_cost_truth_state =
        provider_cost_truth_completeness_state(provider_usage_status, rate_card_status);
    let invoice_evidence_truth_state =
        invoice_evidence_completeness_state(provider_usage_status, provider_invoice_status);
    let money_truth_state =
        money_truth_completeness_state(provider_cost_truth_state, invoice_evidence_truth_state);
    let readiness_state = reconciliation_readiness_state(
        usage_truth_state,
        provider_cost_truth_state,
        invoice_evidence_truth_state,
    );
    let governance_blocking_reasons = reconciliation_governance_blocking_reasons(
        provider_usage_status,
        rate_card_status,
        provider_invoice_status,
    );
    let provider_usage_alignment_state =
        provider_usage_scope_alignment_state(statement_preview, provider_usage_binding, scope_code);
    let provider_invoice_alignment_state = provider_invoice_scope_alignment_state(
        statement_preview,
        provider_invoice_binding,
        scope_code,
    );
    let rate_card_alignment_state = rate_card_scope_alignment_state(statement_preview, rate_card);
    let usage_provider = bound_provider_name(
        provider_usage_binding,
        &["usage_bound", "usage_and_cost_bound"],
    );
    let rate_card_provider =
        bound_provider_name(rate_card, &["priced_bound", "bound_but_unpriced"]);
    let invoice_provider = bound_provider_name(provider_invoice_binding, &["invoice_bound"]);
    let rate_card_provider_alignment_state =
        provider_alignment_state(usage_provider, rate_card_provider);
    let invoice_provider_alignment_state =
        provider_alignment_state(usage_provider, invoice_provider);
    let provider_identity_state = combined_provider_identity_state(&[
        rate_card_provider_alignment_state,
        invoice_provider_alignment_state,
    ]);
    let required_sources_for_usage_truth =
        source_codes_with_truth_role(external_sources, "required_for_usage_truth");
    let required_sources_for_cost_truth =
        source_codes_with_truth_role(external_sources, "required_for_cost_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_usage_truth =
        missing_source_codes_json(if provider_usage_truth_bound(provider_usage_status) {
            Vec::new()
        } else {
            vec!["provider_usage_export"]
        });
    let unready_required_sources_for_cost_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_cost_truth_bound(provider_usage_status) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card_status) {
            missing.push("provider_rate_card");
        }
        missing
    });
    let unready_optional_sources_for_invoice_evidence =
        missing_source_codes_json(if provider_invoice_bound(provider_invoice_status) {
            Vec::new()
        } else {
            vec!["provider_invoice_export"]
        });
    let mut temporal_states = vec![provider_usage_alignment_state, rate_card_alignment_state];
    if provider_invoice_status == "invoice_bound" {
        temporal_states.push(provider_invoice_alignment_state);
    }
    let temporal_truth_state = combined_temporal_truth_state(&temporal_states);
    let mut temporal_blocking_reasons = Vec::new();
    if provider_usage_alignment_state == "scope_period_mismatch" {
        temporal_blocking_reasons.push("provider_usage_scope_period_mismatch");
    }
    if provider_invoice_alignment_state == "scope_period_mismatch" {
        temporal_blocking_reasons.push("provider_invoice_scope_period_mismatch");
    }
    if rate_card_alignment_state == "scope_period_mismatch" {
        temporal_blocking_reasons.push("provider_rate_card_scope_period_mismatch");
    }
    if provider_identity_state == "provider_identity_mismatch" {
        temporal_blocking_reasons.push("provider_identity_mismatch");
    }
    if provider_usage_missing {
        let blocking_reasons =
            base_reconciliation_blocking_reasons(statement_preview, rate_card, true);
        return json!({
            "scope_code": scope_code,
            "scope_label": scope_label,
            "reconciliation_state": "awaiting_provider_usage_source",
            "usage_truth_completeness_state": usage_truth_state,
            "rate_card_truth_completeness_state": rate_card_truth_state,
            "provider_cost_truth_completeness_state": provider_cost_truth_state,
            "invoice_evidence_completeness_state": invoice_evidence_truth_state,
            "money_truth_completeness_state": money_truth_state,
            "reconciliation_readiness_state": readiness_state,
            "governance_blocking_reasons": governance_blocking_reasons,
            "usage_reconciliation_state": "awaiting_provider_usage_source",
            "invoice_reconciliation_state": if provider_invoice_status == "invoice_bound" {
                "invoice_bound_without_usage"
            } else {
                "invoice_not_bound"
            },
            "provider_usage_scope_alignment_state": provider_usage_alignment_state,
            "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
            "rate_card_scope_alignment_state": rate_card_alignment_state,
            "temporal_truth_state": temporal_truth_state,
            "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
            "invoice_provider_alignment_state": invoice_provider_alignment_state,
            "provider_identity_state": provider_identity_state,
            "coverage": statement_preview["coverage"].clone(),
            "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
            "internal_provider_cost_estimate_amount": Value::Null,
            "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
            "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
            "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
            "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
            "external_provider_usage_tokens": Value::Null,
            "external_provider_cost_amount": Value::Null,
            "external_invoice_amount": Value::Null,
            "drift_tokens": Value::Null,
            "drift_amount": Value::Null,
            "invoice_drift_amount": Value::Null,
            "currency_profile": contract.currency_profile.clone(),
            "external_truth_sources": external_sources.clone(),
            "external_truth_bindings": {
                "provider_usage_export": provider_usage_binding.clone(),
                "provider_invoice_export": provider_invoice_binding.clone(),
                "provider_rate_card": rate_card.clone(),
            },
            "required_sources_for_usage_truth": required_sources_for_usage_truth.clone(),
            "required_sources_for_cost_truth": required_sources_for_cost_truth.clone(),
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence.clone(),
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth.clone(),
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth.clone(),
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence.clone(),
            "blocking_reasons": blocking_reasons,
            "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
        });
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        let blocking_reasons =
            base_reconciliation_blocking_reasons(statement_preview, rate_card, true);
        return json!({
            "scope_code": scope_code,
            "scope_label": scope_label,
            "reconciliation_state": "provider_usage_source_error",
            "usage_truth_completeness_state": usage_truth_state,
            "rate_card_truth_completeness_state": rate_card_truth_state,
            "provider_cost_truth_completeness_state": provider_cost_truth_state,
            "invoice_evidence_completeness_state": invoice_evidence_truth_state,
            "money_truth_completeness_state": money_truth_state,
            "reconciliation_readiness_state": readiness_state,
            "governance_blocking_reasons": governance_blocking_reasons,
            "usage_reconciliation_state": "provider_usage_source_error",
            "invoice_reconciliation_state": if provider_invoice_status == "invoice_bound" {
                "invoice_bound_without_usage"
            } else {
                "invoice_not_bound"
            },
            "provider_usage_scope_alignment_state": provider_usage_alignment_state,
            "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
            "rate_card_scope_alignment_state": rate_card_alignment_state,
            "temporal_truth_state": temporal_truth_state,
            "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
            "invoice_provider_alignment_state": invoice_provider_alignment_state,
            "provider_identity_state": provider_identity_state,
            "coverage": statement_preview["coverage"].clone(),
            "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
            "internal_provider_cost_estimate_amount": Value::Null,
            "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
            "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
            "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
            "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
            "external_provider_usage_tokens": Value::Null,
            "external_provider_cost_amount": Value::Null,
            "external_invoice_amount": Value::Null,
            "drift_tokens": Value::Null,
            "drift_amount": Value::Null,
            "invoice_drift_amount": Value::Null,
            "currency_profile": contract.currency_profile.clone(),
            "external_truth_sources": external_sources.clone(),
            "external_truth_bindings": {
                "provider_usage_export": provider_usage_binding.clone(),
                "provider_invoice_export": provider_invoice_binding.clone(),
                "provider_rate_card": rate_card.clone(),
            },
            "required_sources_for_usage_truth": required_sources_for_usage_truth.clone(),
            "required_sources_for_cost_truth": required_sources_for_cost_truth.clone(),
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence.clone(),
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth.clone(),
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth.clone(),
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence.clone(),
            "blocking_reasons": blocking_reasons,
            "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
        });
    } else if rate_card_missing {
        let mut blocking_reasons =
            base_reconciliation_blocking_reasons(statement_preview, rate_card, false);
        blocking_reasons.retain(|reason| *reason != "provider_rate_card_unpriced");
        blocking_reasons.insert(0, "provider_rate_card_unpriced");
        return json!({
            "scope_code": scope_code,
            "scope_label": scope_label,
            "reconciliation_state": "awaiting_rate_card_source",
            "usage_truth_completeness_state": usage_truth_state,
            "rate_card_truth_completeness_state": rate_card_truth_state,
            "provider_cost_truth_completeness_state": provider_cost_truth_state,
            "invoice_evidence_completeness_state": invoice_evidence_truth_state,
            "money_truth_completeness_state": money_truth_state,
            "reconciliation_readiness_state": readiness_state,
            "governance_blocking_reasons": governance_blocking_reasons,
            "usage_reconciliation_state": "external_usage_bound_report_only",
            "invoice_reconciliation_state": if provider_invoice_status == "invoice_bound" {
                "invoice_bound_report_only"
            } else {
                "invoice_not_bound"
            },
            "provider_usage_scope_alignment_state": provider_usage_alignment_state,
            "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
            "rate_card_scope_alignment_state": rate_card_alignment_state,
            "temporal_truth_state": temporal_truth_state,
            "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
            "invoice_provider_alignment_state": invoice_provider_alignment_state,
            "provider_identity_state": provider_identity_state,
            "coverage": statement_preview["coverage"].clone(),
            "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
            "internal_provider_cost_estimate_amount": Value::Null,
            "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
            "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
            "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
            "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
            "external_provider_usage_tokens": provider_usage_binding["scopes"][scope_code]["total_tokens"].clone(),
            "external_provider_cost_amount": provider_usage_binding["scopes"][scope_code]["provider_cost_amount"].clone(),
            "external_invoice_amount": provider_invoice_binding["scopes"][scope_code]["invoice_amount"].clone(),
            "drift_tokens": Value::Null,
            "drift_amount": Value::Null,
            "invoice_drift_amount": Value::Null,
            "currency_profile": contract.currency_profile.clone(),
            "external_truth_sources": external_sources.clone(),
            "external_truth_bindings": {
                "provider_usage_export": provider_usage_binding.clone(),
                "provider_invoice_export": provider_invoice_binding.clone(),
                "provider_rate_card": rate_card.clone(),
            },
            "required_sources_for_usage_truth": required_sources_for_usage_truth.clone(),
            "required_sources_for_cost_truth": required_sources_for_cost_truth.clone(),
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence.clone(),
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth.clone(),
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth.clone(),
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence.clone(),
            "blocking_reasons": blocking_reasons,
            "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
        });
    };
    let mut blocking_reasons = Vec::new();
    blocking_reasons.push("billing_policy_report_only");
    if statement_preview["billable_lower_bound_tokens"].is_null() {
        blocking_reasons.push("billable_lower_bound_not_materialized");
    }
    if contract.billing_mode == "report_only" {
        blocking_reasons.push("billing_mode_report_only");
    }
    let usage_scope = &provider_usage_binding["scopes"][scope_code];
    let invoice_scope = &provider_invoice_binding["scopes"][scope_code];
    let internal_provider_billed_tokens = statement_preview["internal_provider_billed_tokens"]
        .as_u64()
        .unwrap_or(0);
    let internal_provider_cost_estimate =
        internal_provider_cost_estimate_amount(internal_provider_billed_tokens, rate_card);
    let external_provider_usage_tokens = usage_scope["total_tokens"].clone();
    let drift_tokens = match external_provider_usage_tokens.as_u64() {
        Some(external_tokens) => {
            json!(internal_provider_billed_tokens as i64 - external_tokens as i64)
        }
        None => Value::Null,
    };
    let external_provider_cost_amount = usage_scope["provider_cost_amount"].clone();
    let external_invoice_amount = invoice_scope["invoice_amount"].clone();
    let drift_amount = amount_delta(
        internal_provider_cost_estimate,
        external_provider_cost_amount.as_f64(),
    );
    let invoice_drift_amount = amount_delta(
        external_provider_cost_amount.as_f64(),
        external_invoice_amount.as_f64(),
    );
    let usage_reconciliation_state = match drift_tokens.as_i64() {
        Some(0) => "external_usage_aligned_report_only",
        Some(_) => {
            blocking_reasons.push("provider_usage_drift_detected");
            "external_usage_drift_report_only"
        }
        None => "external_usage_bound_report_only",
    };
    let invoice_reconciliation_state = if provider_invoice_status == "invoice_bound" {
        match invoice_drift_amount.as_f64() {
            Some(value) if value.abs() < 1e-9 => "invoice_aligned_report_only",
            Some(_) => {
                blocking_reasons.push("provider_invoice_drift_detected");
                "invoice_drift_report_only"
            }
            None => "invoice_bound_report_only",
        }
    } else {
        "invoice_not_bound"
    };
    let reconciliation_state = if usage_reconciliation_state == "external_usage_aligned_report_only"
    {
        if invoice_reconciliation_state == "invoice_aligned_report_only" {
            "external_usage_and_invoice_aligned_report_only"
        } else {
            "external_usage_aligned_report_only"
        }
    } else if usage_reconciliation_state == "external_usage_drift_report_only" {
        if invoice_reconciliation_state == "invoice_drift_report_only" {
            "external_usage_and_invoice_drift_report_only"
        } else {
            "external_usage_drift_report_only"
        }
    } else if provider_invoice_status == "invoice_bound" {
        "external_usage_and_invoice_bound_report_only"
    } else if provider_usage_status == "usage_and_cost_bound" {
        "external_usage_and_cost_bound_report_only"
    } else {
        "external_usage_bound_report_only"
    };
    for reason in temporal_blocking_reasons {
        if !blocking_reasons.contains(&reason) {
            blocking_reasons.push(reason);
        }
    }

    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "reconciliation_state": reconciliation_state,
        "usage_truth_completeness_state": usage_truth_state,
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "provider_cost_truth_completeness_state": provider_cost_truth_state,
        "invoice_evidence_completeness_state": invoice_evidence_truth_state,
        "money_truth_completeness_state": money_truth_state,
        "reconciliation_readiness_state": readiness_state,
        "governance_blocking_reasons": governance_blocking_reasons,
        "usage_reconciliation_state": usage_reconciliation_state,
        "invoice_reconciliation_state": invoice_reconciliation_state,
        "provider_usage_scope_alignment_state": provider_usage_alignment_state,
        "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
        "rate_card_scope_alignment_state": rate_card_alignment_state,
        "temporal_truth_state": temporal_truth_state,
        "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
        "invoice_provider_alignment_state": invoice_provider_alignment_state,
        "provider_identity_state": provider_identity_state,
        "coverage": statement_preview["coverage"].clone(),
        "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
        "internal_provider_cost_estimate_amount": internal_provider_cost_estimate,
        "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
        "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
        "internal_observed_whole_cycle_lower_bound_tokens": statement_preview["internal_observed_whole_cycle_lower_bound_tokens"].clone(),
        "verified_internal_observed_whole_cycle_lower_bound_tokens": statement_preview["verified_internal_observed_whole_cycle_lower_bound_tokens"].clone(),
        "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
        "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
        "external_provider_usage_tokens": external_provider_usage_tokens,
        "external_provider_cost_amount": external_provider_cost_amount,
        "external_invoice_amount": external_invoice_amount,
        "drift_tokens": drift_tokens,
        "drift_amount": drift_amount,
        "invoice_drift_amount": invoice_drift_amount,
        "currency_profile": usage_scope["currency_profile"]
            .as_str()
            .or_else(|| invoice_scope["currency_profile"].as_str())
            .or_else(|| rate_card["bound_currency_profile"].as_str())
            .unwrap_or(&contract.currency_profile)
            .to_string(),
        "external_truth_sources": external_sources.clone(),
        "external_truth_bindings": {
            "provider_usage_export": provider_usage_binding.clone(),
            "provider_invoice_export": provider_invoice_binding.clone(),
            "provider_rate_card": rate_card.clone(),
        },
        "required_sources_for_usage_truth": required_sources_for_usage_truth,
        "required_sources_for_cost_truth": required_sources_for_cost_truth,
        "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
        "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth,
        "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth,
        "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence,
        "blocking_reasons": blocking_reasons,
        "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
    })
}

fn build_margin_contract_json(
    contract: &TokenBudgetContractConfig,
    external_sources: &Value,
    rate_card: &Value,
    infra_cost_profile: &Value,
    reconciliation_contract: &Value,
) -> Value {
    let rate_card_priced = rate_card["money_conversion_enabled"]
        .as_bool()
        .unwrap_or(false);
    let infra_cost_status = infra_cost_profile["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_truth_state = rate_card_truth_completeness_state(
        rate_card["status"].as_str().unwrap_or("not_configured"),
    );
    let infra_cost_truth_state = infra_cost_truth_completeness_state(infra_cost_status);
    let pricing_truth_state =
        pricing_truth_completeness_state(rate_card_truth_state, infra_cost_truth_state);
    let customer_savings_truth_state =
        customer_savings_money_truth_completeness_state(rate_card_truth_state);
    let amai_cost_truth_state = amai_cost_truth_completeness_state(infra_cost_truth_state);
    let margin_truth_state =
        margin_truth_completeness_state(customer_savings_truth_state, amai_cost_truth_state);
    let provider_status = reconciliation_contract["status"]
        .as_str()
        .unwrap_or("awaiting_provider_usage_source");
    let usage_bound =
        provider_status.starts_with("usage_") || provider_status.starts_with("external_usage_");
    let provider_identity_state = reconciliation_contract["provider_identity_state"]
        .as_str()
        .unwrap_or("provider_identity_unchecked");
    let status = if !rate_card_priced {
        "awaiting_rate_card"
    } else if infra_cost_status != "priced_bound" {
        "awaiting_infra_cost_profile"
    } else if !usage_bound
        && reconciliation_contract["ready_for_external_reconciliation"].as_bool() != Some(true)
    {
        "awaiting_provider_reconciliation"
    } else if provider_identity_state == "provider_identity_mismatch" {
        "provider_identity_mismatch"
    } else {
        "priced_preview_report_only"
    };
    let required_sources_for_margin_truth =
        source_codes_with_truth_role(external_sources, "required_for_margin_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_margin_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_truth_bound(
            reconciliation_contract["external_truth_bindings"]["provider_usage_export"]["status"]
                .as_str()
                .unwrap_or("not_configured"),
        ) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card["status"].as_str().unwrap_or("not_configured")) {
            missing.push("provider_rate_card");
        }
        if !infra_cost_profile_priced_bound(infra_cost_status) {
            missing.push("infra_cost_profile");
        }
        missing
    });

    json!({
        "model_version": contract.margin_model_version.clone(),
        "infra_cost_profile_version": contract.infra_cost_profile_version.clone(),
        "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "infra_cost_truth_completeness_state": infra_cost_truth_state,
        "pricing_truth_completeness_state": pricing_truth_state,
        "customer_savings_money_truth_completeness_state": customer_savings_truth_state,
        "amai_cost_truth_completeness_state": amai_cost_truth_state,
        "margin_truth_completeness_state": margin_truth_state,
        "margin_readiness_state": status,
        "rate_card_status": rate_card["status"].clone(),
        "rate_card_temporal_scope_state": rate_card["temporal_scope_state"].clone(),
        "infra_cost_temporal_scope_state": infra_cost_profile["temporal_scope_state"].clone(),
        "provider_identity_state": provider_identity_state,
        "status": status,
        "money_margin_enabled": status == "priced_preview_report_only",
        "infra_cost_profile": infra_cost_profile.clone(),
        "source_requirements": {
            "required_sources_for_margin_truth": required_sources_for_margin_truth,
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
            "unready_required_sources_for_margin_truth": unready_required_sources_for_margin_truth,
        },
        "note": "Margin layer требует одновременно priced rate card, provider usage binding и infra cost profile. Temporal scope pricing проверяется уже на уровне scope preview, чтобы rate card и infra profile не выглядели применимыми к периоду без отдельной проверки. Даже после этого слой остаётся report-only preview, а не invoice."
    })
}

fn build_margin_scope(
    external_sources: &Value,
    scope_code: &str,
    scope_label: &str,
    statement_preview: &Value,
    reconciliation_preview: &Value,
    rate_card: &Value,
    infra_cost_profile: &Value,
) -> Value {
    let rate_card_priced = rate_card["money_conversion_enabled"]
        .as_bool()
        .unwrap_or(false);
    let infra_cost_status = infra_cost_profile["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_truth_state = rate_card_truth_completeness_state(
        rate_card["status"].as_str().unwrap_or("not_configured"),
    );
    let infra_cost_truth_state = infra_cost_truth_completeness_state(infra_cost_status);
    let pricing_truth_state =
        pricing_truth_completeness_state(rate_card_truth_state, infra_cost_truth_state);
    let customer_savings_truth_state =
        customer_savings_money_truth_completeness_state(rate_card_truth_state);
    let amai_cost_truth_state = amai_cost_truth_completeness_state(infra_cost_truth_state);
    let margin_truth_state =
        margin_truth_completeness_state(customer_savings_truth_state, amai_cost_truth_state);
    let reconciliation_state = reconciliation_preview["reconciliation_state"]
        .as_str()
        .unwrap_or("awaiting_provider_usage_source");
    let usage_bound = reconciliation_state.starts_with("external_usage_");
    let usage_drifted = reconciliation_state.contains("_drift_");
    let currency_match = rate_card["bound_currency_profile"].as_str()
        == infra_cost_profile["bound_currency_profile"].as_str();
    let provider_usage_alignment_state =
        reconciliation_preview["provider_usage_scope_alignment_state"]
            .as_str()
            .unwrap_or("provider_usage_not_bound");
    let rate_card_alignment_state = reconciliation_preview["rate_card_scope_alignment_state"]
        .as_str()
        .unwrap_or("rate_card_not_bound");
    let provider_identity_state = reconciliation_preview["provider_identity_state"]
        .as_str()
        .unwrap_or("provider_identity_unchecked");
    let infra_cost_alignment_state =
        infra_cost_scope_alignment_state(statement_preview, infra_cost_profile);
    let temporal_truth_state = combined_temporal_truth_state(&[
        provider_usage_alignment_state,
        rate_card_alignment_state,
        infra_cost_alignment_state,
    ]);
    let margin_state = if !rate_card_priced {
        "awaiting_rate_card"
    } else if infra_cost_status != "priced_bound" {
        "awaiting_infra_cost_profile"
    } else if !usage_bound {
        "awaiting_provider_reconciliation"
    } else if provider_identity_state == "provider_identity_mismatch" {
        "provider_identity_mismatch"
    } else if temporal_truth_state == "scope_period_mismatch" {
        "pricing_period_mismatch"
    } else if !currency_match {
        "currency_profile_mismatch"
    } else if usage_drifted {
        "priced_preview_with_provider_drift"
    } else if matches!(
        temporal_truth_state,
        "source_period_unspecified" | "source_period_partially_bound" | "scope_period_unknown"
    ) {
        "priced_preview_temporal_unscoped_report_only"
    } else {
        "priced_preview_report_only"
    };
    let margin_confidence_state = match margin_state {
        "priced_preview_report_only" => "aligned_report_only",
        "priced_preview_with_provider_drift" => "provider_drift_detected",
        "pricing_period_mismatch" => "pricing_period_mismatch",
        "priced_preview_temporal_unscoped_report_only" => "period_unscoped_report_only",
        "currency_profile_mismatch" => "currency_profile_mismatch",
        "awaiting_provider_reconciliation" => "awaiting_provider_reconciliation",
        "awaiting_infra_cost_profile" => "awaiting_infra_cost_profile",
        "provider_identity_mismatch" => "provider_identity_mismatch",
        _ => "awaiting_rate_card",
    };
    let margin_readiness_state = match margin_state {
        "awaiting_rate_card" | "awaiting_infra_cost_profile" => "awaiting_pricing_truth",
        "awaiting_provider_reconciliation" => "awaiting_usage_truth",
        "provider_identity_mismatch" => "provider_identity_mismatch",
        "pricing_period_mismatch" => "pricing_period_mismatch",
        "currency_profile_mismatch" => "currency_profile_mismatch",
        "priced_preview_with_provider_drift" => "provider_drift_detected",
        "priced_preview_temporal_unscoped_report_only" => "temporal_truth_unscoped_report_only",
        "priced_preview_report_only" => "preview_ready_report_only",
        _ => "awaiting_pricing_truth",
    };
    let mut blocking_reasons = Vec::new();
    if !rate_card_priced {
        blocking_reasons.push("rate_card_unpriced");
    }
    if infra_cost_status != "priced_bound" {
        blocking_reasons.push("infra_cost_profile_missing");
    }
    if !usage_bound {
        blocking_reasons.push("provider_reconciliation_not_complete");
    }
    if provider_identity_state == "provider_identity_mismatch" {
        blocking_reasons.push("provider_identity_mismatch");
    }
    if provider_usage_alignment_state == "scope_period_mismatch" {
        blocking_reasons.push("provider_usage_scope_period_mismatch");
    }
    if rate_card_alignment_state == "scope_period_mismatch" {
        blocking_reasons.push("provider_rate_card_scope_period_mismatch");
    }
    if infra_cost_alignment_state == "scope_period_mismatch" {
        blocking_reasons.push("infra_cost_scope_period_mismatch");
    }
    if !currency_match && rate_card_priced && infra_cost_status == "priced_bound" {
        blocking_reasons.push("currency_profile_mismatch");
    }
    if usage_drifted {
        blocking_reasons.push("provider_usage_drift_detected");
    }
    let customer_saved_amount_lower_bound =
        statement_preview["measured_non_billable_lower_bound_tokens"]
            .as_i64()
            .and_then(|tokens| {
                rate_card["default_input_cost_per_1k_tokens"]
                    .as_f64()
                    .map(|rate| (tokens as f64 / 1000.0) * rate)
            });
    let amai_infra_cost_amount = if rate_card_priced && infra_cost_status == "priced_bound" {
        let per_1k = infra_cost_profile["cost_per_1k_internal_billed_tokens"]
            .as_f64()
            .unwrap_or(0.0);
        let per_event = infra_cost_profile["cost_per_live_event"]
            .as_f64()
            .unwrap_or(0.0);
        let fixed_scope = infra_cost_profile["fixed_scope_cost_amount"]
            .as_f64()
            .unwrap_or(0.0);
        let internal_provider_billed_tokens = statement_preview["internal_provider_billed_tokens"]
            .as_u64()
            .unwrap_or(0);
        let included_events = statement_preview["coverage"]["included_events"]
            .as_u64()
            .unwrap_or(0);
        Some(
            (internal_provider_billed_tokens as f64 / 1000.0) * per_1k
                + (included_events as f64) * per_event
                + fixed_scope,
        )
    } else {
        None
    };
    let margin_amount = match (customer_saved_amount_lower_bound, amai_infra_cost_amount) {
        (Some(saved), Some(cost)) if currency_match => Some(saved - cost),
        _ => None,
    };
    let savings_to_cost_ratio = match (customer_saved_amount_lower_bound, amai_infra_cost_amount) {
        (Some(saved), Some(cost)) if currency_match && cost > 0.0 => Some(saved / cost),
        _ => None,
    };
    let currency_profile = if currency_match {
        rate_card["bound_currency_profile"]
            .as_str()
            .unwrap_or("unpriced")
            .to_string()
    } else {
        "mismatch".to_string()
    };
    let required_sources_for_margin_truth =
        source_codes_with_truth_role(external_sources, "required_for_margin_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_margin_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_truth_bound(
            reconciliation_preview["external_truth_bindings"]["provider_usage_export"]["status"]
                .as_str()
                .unwrap_or("not_configured"),
        ) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card["status"].as_str().unwrap_or("not_configured")) {
            missing.push("provider_rate_card");
        }
        if !infra_cost_profile_priced_bound(infra_cost_status) {
            missing.push("infra_cost_profile");
        }
        missing
    });

    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "margin_state": margin_state,
        "margin_confidence_state": margin_confidence_state,
        "margin_readiness_state": margin_readiness_state,
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "infra_cost_truth_completeness_state": infra_cost_truth_state,
        "pricing_truth_completeness_state": pricing_truth_state,
        "customer_savings_money_truth_completeness_state": customer_savings_truth_state,
        "amai_cost_truth_completeness_state": amai_cost_truth_state,
        "margin_truth_completeness_state": margin_truth_state,
        "provider_usage_scope_alignment_state": provider_usage_alignment_state,
        "rate_card_scope_alignment_state": rate_card_alignment_state,
        "infra_cost_scope_alignment_state": infra_cost_alignment_state,
        "provider_identity_state": provider_identity_state,
        "temporal_truth_state": temporal_truth_state,
        "customer_saved_tokens_lower_bound": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
        "customer_saved_amount_lower_bound": customer_saved_amount_lower_bound,
        "amai_infra_cost_amount": amai_infra_cost_amount,
        "margin_amount": margin_amount,
        "savings_to_cost_ratio": savings_to_cost_ratio,
        "currency_profile": currency_profile,
        "coverage": statement_preview["coverage"].clone(),
        "reconciliation_state": reconciliation_preview["reconciliation_state"].clone(),
        "infra_cost_profile": infra_cost_profile.clone(),
        "required_sources_for_margin_truth": required_sources_for_margin_truth,
        "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
        "unready_required_sources_for_margin_truth": unready_required_sources_for_margin_truth,
        "blocking_reasons": blocking_reasons,
        "note": "Margin preview опирается на confirmed lower bound, provider input rate и bound infra cost profile. Это всё ещё report-only preview, а не invoice."
    })
}

fn statement_lifecycle_state(adjustment_preview: &Value) -> &'static str {
    if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_non_billable_dispute_hold"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_non_billable_pending_adjustment"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_non_billable_adjusted_report_only"
    } else {
        "measured_non_billable_open"
    }
}

fn settlement_stage(
    measured_events: usize,
    adjustment_preview: &Value,
    metering_freshness: &Value,
    provisional_close_candidate: bool,
) -> &'static str {
    if measured_events == 0 {
        "empty_report_only"
    } else if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_disputed_report_only"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_pending_adjustment_report_only"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_adjusted_report_only"
    } else if metering_freshness["can_treat_scope_as_stable"].as_bool() == Some(true)
        && provisional_close_candidate
    {
        "measured_review_ready_report_only"
    } else {
        "measured_open_report_only"
    }
}

fn settlement_stage_family(stage: &str) -> &'static str {
    match stage {
        "empty_report_only" => "empty",
        "measured_disputed_report_only"
        | "measured_pending_adjustment_report_only"
        | "measured_adjusted_report_only"
        | "measured_review_ready_report_only"
        | "measured_open_report_only" => "measured_report_only",
        "billable_reserved" | "settled_reserved" | "invoiced_reserved" | "credited_reserved"
        | "disputed_reserved" | "closed_reserved" => "future_reserved",
        _ => "unknown",
    }
}

fn future_reserved_settlement_stages() -> [&'static str; 6] {
    [
        "billable_reserved",
        "settled_reserved",
        "invoiced_reserved",
        "credited_reserved",
        "disputed_reserved",
        "closed_reserved",
    ]
}

fn next_settlement_stage_candidate(
    measured_events: usize,
    metering_freshness: &Value,
    provisional_close_candidate: bool,
    billing_close_barriers: &[String],
) -> &'static str {
    if measured_events == 0 {
        "awaiting_measured_usage"
    } else if !(metering_freshness["can_treat_scope_as_stable"].as_bool() == Some(true)
        && provisional_close_candidate)
    {
        "review_ready_blocked"
    } else if !billing_close_barriers.is_empty() {
        "billable_blocked"
    } else {
        "billable_reserved"
    }
}

fn next_settlement_stage_blockers(
    measured_events: usize,
    provisional_close_barriers: &[String],
    billing_close_barriers: &[String],
) -> Vec<String> {
    if measured_events == 0 {
        return vec!["no_measured_usage_events".to_string()];
    }
    if !provisional_close_barriers.is_empty() {
        return provisional_close_barriers.to_vec();
    }
    billing_close_barriers.to_vec()
}

fn merge_string_slices(slices: &[&[String]]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for slice in slices {
        for item in *slice {
            if seen.insert(item.clone()) {
                merged.push(item.clone());
            }
        }
    }
    merged
}

fn transactional_status_entry(
    status: &str,
    boundary: &str,
    materialized: bool,
    blocking_reasons: Vec<String>,
) -> Value {
    json!({
        "status": status,
        "boundary": boundary,
        "materialized": materialized,
        "blocking_reasons": blocking_reasons,
    })
}

fn build_transactional_statuses(
    contract: &TokenBudgetContractConfig,
    measured_events: usize,
    settlement_stage: &str,
    next_stage_candidate: &str,
    next_stage_blockers: &[String],
    billing_close_barriers: &[String],
    adjustment_preview: &Value,
) -> Value {
    let no_usage_reasons = vec!["no_measured_usage_events".to_string()];
    let review_ready = settlement_stage == "measured_review_ready_report_only";
    let measured = if measured_events == 0 {
        transactional_status_entry(
            "awaiting_measured_usage",
            "not_started",
            false,
            no_usage_reasons.clone(),
        )
    } else {
        transactional_status_entry(settlement_stage, "measured_report_only", true, Vec::new())
    };
    let review = if measured_events == 0 {
        transactional_status_entry(
            "awaiting_measured_usage",
            "not_started",
            false,
            no_usage_reasons.clone(),
        )
    } else if review_ready {
        transactional_status_entry(
            "review_ready_report_only",
            "measured_report_only",
            true,
            Vec::new(),
        )
    } else {
        transactional_status_entry(
            "review_blocked_report_only",
            "measured_report_only",
            true,
            next_stage_blockers.to_vec(),
        )
    };
    let billable = if next_stage_candidate == "billable_reserved" {
        transactional_status_entry("billable_reserved", "reserved_future", false, Vec::new())
    } else if measured_events == 0 {
        transactional_status_entry(
            "awaiting_measured_usage",
            "reserved_future",
            false,
            no_usage_reasons.clone(),
        )
    } else {
        transactional_status_entry(
            "billable_blocked_reserved",
            "reserved_future",
            false,
            if next_stage_blockers.is_empty() {
                billing_close_barriers.to_vec()
            } else {
                next_stage_blockers.to_vec()
            },
        )
    };
    let reserved_follow_on_blockers = if measured_events == 0 {
        no_usage_reasons.clone()
    } else {
        merge_string_slices(&[
            billing_close_barriers,
            &vec!["billable_not_materialized".to_string()],
        ])
    };
    let disputed = if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        transactional_status_entry(
            "dispute_hold_report_only",
            "measured_report_only",
            true,
            vec!["open_dispute_entries".to_string()],
        )
    } else {
        transactional_status_entry(
            "disputed_reserved",
            "reserved_future",
            false,
            vec!["no_open_dispute_entries".to_string()],
        )
    };

    json!({
        "model_version": contract.settlement_lifecycle_model_version.clone(),
        "measured": measured,
        "review": review,
        "billable": billable,
        "settled": transactional_status_entry(
            "settled_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers.clone(),
        ),
        "invoiced": transactional_status_entry(
            "invoiced_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers.clone(),
        ),
        "credited": transactional_status_entry(
            "credited_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers.clone(),
        ),
        "disputed": disputed,
        "closed": transactional_status_entry(
            "closed_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers,
        ),
        "note": "Transactional statuses честно разделяют уже materialized measured/report-only стадии и будущие reserved money-facing стадии. Reserved не означает включённый billing workflow."
    })
}

fn provisional_close_barriers(
    summary: &Value,
    metering_freshness: &Value,
    adjustment_preview: &Value,
) -> Vec<String> {
    let mut barriers = Vec::new();
    if !matches!(
        summary["coverage"]["completeness_state"].as_str(),
        Some("confirmed" | "fully_confirmed")
    ) {
        barriers.push("coverage_not_final".to_string());
    }
    if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        barriers.push("late_arrival_window_open".to_string());
    }
    if metering_freshness["metering_ingest_state"].as_str() == Some("lagging") {
        barriers.push("metering_pipeline_lagging".to_string());
    }
    if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        barriers.push("pending_adjustment_review".to_string());
    }
    if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        barriers.push("dispute_hold_open".to_string());
    }
    barriers
}

fn freeze_status(
    events: &[TokenBudgetEvent],
    metering_freshness: &Value,
    provisional_close_candidate: bool,
) -> &'static str {
    if events.is_empty() {
        "empty"
    } else if metering_freshness["metering_ingest_state"].as_str() == Some("lagging") {
        "pipeline_lag_open"
    } else if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        "late_arrival_window_open"
    } else if provisional_close_candidate {
        "provisionally_frozen_report_only"
    } else {
        "open_review_hold_report_only"
    }
}

fn reason_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn merged_reason_strings(values: &[&Value]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for value in values {
        for reason in reason_strings(value) {
            if seen.insert(reason.clone()) {
                merged.push(reason);
            }
        }
    }
    merged
}

fn push_unique_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

fn internal_money_arithmetic_readiness(
    reconciliation_preview: &Value,
    margin_scope: &Value,
) -> (&'static str, Vec<String>) {
    let mut blockers = merged_reason_strings(&[
        &reconciliation_preview["governance_blocking_reasons"],
        &margin_scope["blocking_reasons"],
    ]);
    blockers.retain(|reason| reason != "provider_invoice_source_error");

    if reconciliation_preview["provider_identity_state"]
        .as_str()
        .unwrap_or("provider_identity_aligned")
        == "provider_identity_mismatch"
        || margin_scope["provider_identity_state"]
            .as_str()
            .unwrap_or("provider_identity_aligned")
            == "provider_identity_mismatch"
    {
        push_unique_reason(&mut blockers, "provider_identity_mismatch");
        return ("provider_identity_mismatch", blockers);
    }

    if reconciliation_preview["temporal_truth_state"]
        .as_str()
        .unwrap_or("scope_period_aligned")
        != "scope_period_aligned"
    {
        push_unique_reason(&mut blockers, "reconciliation_scope_period_not_aligned");
        return ("reconciliation_scope_period_misaligned", blockers);
    }

    if margin_scope["temporal_truth_state"]
        .as_str()
        .unwrap_or("scope_period_aligned")
        != "scope_period_aligned"
    {
        push_unique_reason(&mut blockers, "margin_scope_period_not_aligned");
        return ("margin_scope_period_misaligned", blockers);
    }

    match reconciliation_preview["usage_truth_completeness_state"].as_str() {
        Some("provider_usage_bound") => {}
        Some("provider_usage_source_error") => {
            push_unique_reason(&mut blockers, "usage_truth_source_error");
            return ("usage_truth_source_error", blockers);
        }
        Some("provider_usage_not_yet_bound") => {
            push_unique_reason(&mut blockers, "usage_truth_not_yet_bound");
            return ("usage_truth_not_yet_bound", blockers);
        }
        _ => {
            push_unique_reason(&mut blockers, "usage_truth_not_ready");
            return ("awaiting_usage_truth", blockers);
        }
    }

    match reconciliation_preview["provider_cost_truth_completeness_state"].as_str() {
        Some("provider_cost_bound") => {}
        Some("provider_rate_card_error") => {
            push_unique_reason(&mut blockers, "provider_cost_truth_source_error");
            return ("provider_cost_truth_source_error", blockers);
        }
        Some("rate_card_bound_unpriced") => {
            push_unique_reason(&mut blockers, "provider_cost_truth_unpriced");
            return ("provider_cost_truth_unpriced", blockers);
        }
        Some("rate_card_bound_internal_estimate_only") => {
            push_unique_reason(&mut blockers, "provider_cost_truth_internal_estimate_only");
            return ("provider_cost_truth_internal_estimate_only", blockers);
        }
        _ => {
            push_unique_reason(&mut blockers, "provider_cost_truth_not_ready");
            return ("awaiting_provider_cost_truth", blockers);
        }
    }

    if margin_scope["pricing_truth_completeness_state"].as_str() != Some("pricing_truth_ready") {
        push_unique_reason(&mut blockers, "pricing_truth_not_ready");
        return ("awaiting_pricing_truth", blockers);
    }

    match margin_scope["margin_truth_completeness_state"].as_str() {
        Some("margin_preview_amounts_ready_report_only") => {
            ("money_arithmetic_preview_ready_report_only", blockers)
        }
        Some("margin_truth_source_error") => {
            push_unique_reason(&mut blockers, "margin_truth_source_error");
            ("margin_truth_source_error", blockers)
        }
        Some("margin_truth_bound_unpriced") => {
            push_unique_reason(&mut blockers, "margin_truth_unpriced");
            ("margin_truth_unpriced", blockers)
        }
        _ => {
            push_unique_reason(&mut blockers, "margin_truth_not_ready");
            ("awaiting_margin_truth", blockers)
        }
    }
}

fn contractual_settlement_readiness(
    statement_preview: &Value,
    metering_freshness: &Value,
    internal_money_arithmetic_state: &str,
) -> (&'static str, Vec<String>) {
    let measured_events = statement_preview["coverage"]["measured_events"]
        .as_u64()
        .unwrap_or(0);
    let mut blockers = merged_reason_strings(&[
        &statement_preview["close_barriers"],
        &statement_preview["next_settlement_stage_blockers"],
        &metering_freshness["blocking_reasons"],
    ]);

    if internal_money_arithmetic_state != "money_arithmetic_preview_ready_report_only" {
        push_unique_reason(&mut blockers, "money_arithmetic_not_ready");
    }

    if measured_events == 0 {
        push_unique_reason(&mut blockers, "no_measured_usage_events");
        return ("empty", blockers);
    }

    let settlement_stage = statement_preview["settlement_stage"]
        .as_str()
        .unwrap_or("unknown");
    let next_stage_candidate = statement_preview["next_settlement_stage_candidate"]
        .as_str()
        .unwrap_or("unknown");

    if settlement_stage == "measured_review_ready_report_only" {
        if blockers.is_empty() && next_stage_candidate == "billable_ready" {
            ("settlement_ready_reserved", blockers)
        } else {
            (
                "customer_review_ready_settlement_activation_blocked_report_only",
                blockers,
            )
        }
    } else {
        ("review_not_yet_ready_report_only", blockers)
    }
}

fn review_surface_state(statement_export_preview: &Value) -> (&'static str, Vec<String>) {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let measured_events = statement_preview["coverage"]["measured_events"]
        .as_u64()
        .or_else(|| statement_export_preview["included_events_count"].as_u64())
        .unwrap_or(0);
    let settlement_stage = statement_preview["settlement_stage"]
        .as_str()
        .unwrap_or("unknown");
    let mut blockers =
        merged_reason_strings(&[&statement_preview["next_settlement_stage_blockers"]]);

    if measured_events == 0 {
        push_unique_reason(&mut blockers, "no_measured_usage_events");
        return ("empty_report_only", blockers);
    }

    if settlement_stage == "measured_review_ready_report_only" {
        return ("customer_review_ready_report_only", blockers);
    }

    if statement_preview["provisional_close_candidate"].as_bool() == Some(true) {
        ("provisionally_stable_report_only", blockers)
    } else {
        ("provisional_report_only", blockers)
    }
}

fn future_settlement_activation_state(contractual_settlement_state: &str) -> &'static str {
    match contractual_settlement_state {
        "settlement_ready_reserved" => "future_settlement_ready_reserved",
        "customer_review_ready_settlement_activation_blocked_report_only" => {
            "future_settlement_activation_blocked_report_only"
        }
        "review_not_yet_ready_report_only" => "review_not_yet_ready_for_future_settlement",
        "empty" => "empty_scope_report_only",
        _ => "future_settlement_state_unknown",
    }
}

fn build_customer_contractual_boundary_from_export(
    contract: &TokenBudgetContractConfig,
    surface_kind: &str,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let (review_surface_state, review_surface_blocking_reasons) =
        review_surface_state(statement_export_preview);
    let contractual_settlement_state = statement_export_preview
        .get("contractual_settlement_readiness_state")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    json!({
        "model_version": contract.customer_contractual_boundary_version.clone(),
        "surface_kind": surface_kind,
        "report_only": true,
        "self_serve_state": "self_serve_ready_report_only",
        "invoice_grade": false,
        "operational_telemetry_included": false,
        "review_surface_state": review_surface_state,
        "review_surface_blocking_reasons": review_surface_blocking_reasons,
        "future_settlement_activation_state": future_settlement_activation_state(contractual_settlement_state),
        "future_settlement_activation_blocking_reasons": statement_export_preview["contractual_settlement_blocking_reasons"].clone(),
        "settlement_stage": statement_preview["settlement_stage"].clone(),
        "settlement_stage_family": statement_preview["settlement_stage_family"].clone(),
        "next_settlement_stage_candidate": statement_preview["next_settlement_stage_candidate"].clone(),
        "contractual_readiness_model_version": statement_export_preview["contractual_readiness_model_version"].clone(),
        "contractual_settlement_readiness_state": statement_export_preview["contractual_settlement_readiness_state"].clone(),
        "note": "Этот boundary отделяет текущую review-ready report-only поверхность от более строгой будущей settlement activation semantics."
    })
}

fn settlement_activation_governance_state(statement_export_preview: &Value) -> &'static str {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];

    if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "dispute_hold_open_report_only"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "pending_adjustment_review_report_only"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "adjusted_report_only"
    } else if statement_export_preview["customer_contractual_boundary"]
        ["future_settlement_activation_state"]
        .as_str()
        == Some("future_settlement_ready_reserved")
    {
        "future_settlement_ready_reserved"
    } else {
        "activation_blocked_report_only"
    }
}

fn build_settlement_activation_governance_from_export(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let registry_status = adjustment_preview["registry_status"]
        .as_str()
        .or_else(|| adjustment_preview["status"].as_str())
        .unwrap_or("unknown");
    let adjustment_status = adjustment_preview["status"]
        .as_str()
        .or_else(|| adjustment_preview["registry_status"].as_str())
        .unwrap_or("unknown");

    json!({
        "model_version": contract.settlement_activation_governance_version.clone(),
        "governance_state": settlement_activation_governance_state(statement_export_preview),
        "future_settlement_activation_state": statement_export_preview["customer_contractual_boundary"]["future_settlement_activation_state"].clone(),
        "future_settlement_activation_blocking_reasons": statement_export_preview["customer_contractual_boundary"]["future_settlement_activation_blocking_reasons"].clone(),
        "next_settlement_stage_candidate": statement_preview["next_settlement_stage_candidate"].clone(),
        "next_settlement_stage_blockers": statement_preview["next_settlement_stage_blockers"].clone(),
        "provisional_close_state": statement_preview["provisional_close_state"].clone(),
        "provisional_close_candidate": statement_preview["provisional_close_candidate"].clone(),
        "provisional_close_barriers": statement_preview["provisional_close_barriers"].clone(),
        "billing_close_barriers": statement_preview["billing_close_barriers"].clone(),
        "close_barriers": statement_preview["close_barriers"].clone(),
        "registry_status": registry_status,
        "adjustment_status": adjustment_status,
        "correction_action_state": adjustment_preview["correction_action_state"].clone(),
        "credit_action_state": statement_export_preview["credit_action_state"].clone(),
        "dispute_action_state": statement_export_preview["dispute_action_state"].clone(),
        "pending_entries_count": adjustment_preview["pending_entries_count"].as_u64().unwrap_or(0),
        "applied_entries_count": adjustment_preview["applied_entries_count"].as_u64().unwrap_or(0),
        "disputed_entries_count": adjustment_preview["disputed_entries_count"].as_u64().unwrap_or(0),
        "allowed_future_actions": adjustment_preview["allowed_future_actions"].clone(),
        "note": "Этот governance-слой отдельно объясняет, какие barriers и adjustment semantics сейчас держат будущую settlement activation в report-only режиме."
    })
}

fn adjustment_activation_governance_state(statement_export_preview: &Value) -> &'static str {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let adjustment_status = adjustment_preview["status"].as_str().unwrap_or("unknown");

    if matches!(adjustment_status, "not_configured" | "default_path_missing") {
        "registry_not_configured_report_only"
    } else if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "dispute_hold_open_report_only"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "pending_adjustment_review_report_only"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "adjusted_report_only"
    } else {
        "future_adjustment_ready_reserved"
    }
}

fn future_adjustment_activation_state(statement_export_preview: &Value) -> &'static str {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let adjustment_status = adjustment_preview["status"].as_str().unwrap_or("unknown");

    if matches!(adjustment_status, "not_configured" | "default_path_missing") {
        "future_adjustment_registry_not_bound"
    } else if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "future_adjustment_blocked_by_dispute"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "future_adjustment_blocked_by_review"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "future_adjustment_materialized_report_only"
    } else {
        "future_adjustment_ready_reserved"
    }
}

fn future_adjustment_activation_blocking_reasons(statement_export_preview: &Value) -> Vec<String> {
    match future_adjustment_activation_state(statement_export_preview) {
        "future_adjustment_registry_not_bound" => vec!["adjustment_registry_not_bound".to_string()],
        "future_adjustment_blocked_by_dispute" => vec!["dispute_hold_open".to_string()],
        "future_adjustment_blocked_by_review" => vec!["pending_adjustment_review".to_string()],
        _ => Vec::new(),
    }
}

fn build_adjustment_activation_governance_from_export(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let registry_status = adjustment_preview["registry_status"]
        .as_str()
        .or_else(|| adjustment_preview["status"].as_str())
        .unwrap_or("unknown");
    let adjustment_status = adjustment_preview["status"]
        .as_str()
        .or_else(|| adjustment_preview["registry_status"].as_str())
        .unwrap_or("unknown");

    json!({
        "model_version": contract.adjustment_activation_governance_version.clone(),
        "governance_state": adjustment_activation_governance_state(statement_export_preview),
        "future_adjustment_activation_state": future_adjustment_activation_state(statement_export_preview),
        "future_adjustment_activation_blocking_reasons": future_adjustment_activation_blocking_reasons(statement_export_preview),
        "registry_status": registry_status,
        "adjustment_status": adjustment_status,
        "request_schema_version": adjustment_preview["request_schema_version"].clone(),
        "registry_version": adjustment_preview["registry_version"].clone(),
        "correction_action_state": adjustment_preview["correction_action_state"].clone(),
        "credit_action_state": statement_export_preview["credit_action_state"].clone(),
        "dispute_action_state": statement_export_preview["dispute_action_state"].clone(),
        "pending_entries_count": adjustment_preview["pending_entries_count"].as_u64().unwrap_or(0),
        "applied_entries_count": adjustment_preview["applied_entries_count"].as_u64().unwrap_or(0),
        "disputed_entries_count": adjustment_preview["disputed_entries_count"].as_u64().unwrap_or(0),
        "allowed_future_actions": adjustment_preview["allowed_future_actions"].clone(),
        "note": "Этот governance-слой отдельно объясняет, готов ли future adjustment path, чем он сейчас заблокирован и где report-only layer уже materialized pending/applied/disputed semantics."
    })
}

fn settlement_report_preview_from_export(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let mut settlement_report_preview =
        statement_export_preview["settlement_report_preview"].clone();
    if settlement_report_preview["customer_contractual_boundary"].is_null() {
        settlement_report_preview["customer_contractual_boundary"] =
            build_customer_contractual_boundary_from_export(
                contract,
                "customer_settlement_report_preview_report_only",
                statement_export_preview,
            );
    }
    if settlement_report_preview["adjustment_activation_governance"].is_null() {
        settlement_report_preview["adjustment_activation_governance"] =
            build_adjustment_activation_governance_from_export(contract, statement_export_preview);
    }
    settlement_report_preview
}

fn build_scope_suitability(
    contract: &TokenBudgetContractConfig,
    statement_preview: &Value,
    reconciliation_preview: &Value,
    margin_scope: &Value,
    metering_freshness: &Value,
) -> Value {
    if statement_preview.is_null()
        || reconciliation_preview.is_null()
        || margin_scope.is_null()
        || metering_freshness.is_null()
    {
        return Value::Null;
    }

    let measured_events = statement_preview["coverage"]["measured_events"]
        .as_u64()
        .unwrap_or(0);
    let confirmed_events = statement_preview["coverage"]["included_events"]
        .as_u64()
        .unwrap_or(0);
    let coverage_state = statement_preview["coverage"]["completeness_state"]
        .as_str()
        .unwrap_or("empty");
    let stable = metering_freshness["can_treat_scope_as_stable"].as_bool() == Some(true);
    let provisional_close_candidate =
        statement_preview["provisional_close_candidate"].as_bool() == Some(true);
    let provisional_close_barriers = &statement_preview["provisional_close_barriers"];
    let billing_close_barriers = &statement_preview["billing_close_barriers"];
    let governance_blocking_reasons = &reconciliation_preview["governance_blocking_reasons"];
    let review_reasons = merged_reason_strings(&[
        provisional_close_barriers,
        &metering_freshness["blocking_reasons"],
    ]);
    let billing_reasons = merged_reason_strings(&[
        billing_close_barriers,
        governance_blocking_reasons,
        &margin_scope["blocking_reasons"],
    ]);
    let compensation_reasons = {
        let mut reasons = billing_reasons.clone();
        if reconciliation_preview["money_truth_completeness_state"].as_str()
            != Some("provider_cost_and_invoice_bound")
            && !reasons.iter().any(|value| value == "money_truth_not_final")
        {
            reasons.push("money_truth_not_final".to_string());
        }
        if statement_preview["final_amount"].is_null()
            && !reasons
                .iter()
                .any(|value| value == "final_amount_unavailable")
        {
            reasons.push("final_amount_unavailable".to_string());
        }
        reasons
    };

    let operational_live = if measured_events == 0 {
        json!({
            "usable": false,
            "state": "empty",
            "blocking_reasons": ["no_measured_usage_events"]
        })
    } else {
        json!({
            "usable": true,
            "state": "live_operational",
            "blocking_reasons": []
        })
    };

    let product_kpi = if confirmed_events == 0 {
        json!({
            "usable": false,
            "state": "awaiting_confirmed_usage",
            "blocking_reasons": if coverage_state == "empty" {
                json!(["no_measured_usage_events"])
            } else {
                json!(["no_confirmed_usage"])
            }
        })
    } else if stable && provisional_close_candidate {
        json!({
            "usable": true,
            "state": "provisionally_stable_lower_bound_with_coverage",
            "blocking_reasons": review_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "provisional_lower_bound_with_coverage",
            "blocking_reasons": review_reasons
        })
    };

    let customer_review = if measured_events == 0 {
        json!({
            "usable": false,
            "state": "empty",
            "blocking_reasons": ["no_measured_usage_events"]
        })
    } else if stable && provisional_close_candidate {
        json!({
            "usable": true,
            "state": "review_ready_report_only_provisionally_stable",
            "blocking_reasons": review_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "review_ready_report_only_provisional",
            "blocking_reasons": review_reasons
        })
    };

    let contractual_export = if measured_events == 0 {
        json!({
            "usable": false,
            "state": "empty",
            "blocking_reasons": ["no_measured_usage_events"]
        })
    } else if stable && provisional_close_candidate {
        json!({
            "usable": true,
            "state": "export_ready_report_only_provisionally_stable",
            "blocking_reasons": review_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "export_ready_report_only_provisional",
            "blocking_reasons": review_reasons
        })
    };

    let billing_amount = if statement_preview["billable_lower_bound_tokens"].is_null() {
        json!({
            "usable": false,
            "state": "not_billable_report_only",
            "blocking_reasons": billing_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "billable_ready",
            "blocking_reasons": billing_reasons
        })
    };

    let compensation_pricing = if statement_preview["billable_lower_bound_tokens"].is_null()
        || statement_preview["final_amount"].is_null()
        || reconciliation_preview["money_truth_completeness_state"].as_str()
            != Some("provider_cost_and_invoice_bound")
    {
        json!({
            "usable": false,
            "state": "not_compensation_ready",
            "blocking_reasons": compensation_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "compensation_ready",
            "blocking_reasons": compensation_reasons
        })
    };

    json!({
        "model_version": contract.suitability_model_version.clone(),
        "surfaces": {
            "operational_live": operational_live,
            "product_kpi": product_kpi,
            "customer_review": customer_review,
            "contractual_export": contractual_export,
            "billing_amount": billing_amount,
            "compensation_pricing": compensation_pricing,
        },
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "note": "Suitability не маскирует отрицательную или положительную экономию. Она только фиксирует, где этот scope можно использовать без подмены смысла."
    })
}

fn build_contractual_statement_summary(
    contract: &TokenBudgetContractConfig,
    scope_code: &str,
    scope_label: &str,
    statement_preview: &Value,
    reconciliation_preview: &Value,
    margin_scope: &Value,
    metering_freshness: &Value,
) -> Value {
    if statement_preview.is_null()
        || reconciliation_preview.is_null()
        || margin_scope.is_null()
        || metering_freshness.is_null()
    {
        return Value::Null;
    }
    let rate_card_binding =
        &reconciliation_preview["external_truth_bindings"]["provider_rate_card"];
    let provider_usage_binding =
        &reconciliation_preview["external_truth_bindings"]["provider_usage_export"];
    let provider_invoice_binding =
        &reconciliation_preview["external_truth_bindings"]["provider_invoice_export"];
    let settlement_stage = statement_preview["settlement_stage"]
        .as_str()
        .unwrap_or("unknown");
    let suitability = build_scope_suitability(
        contract,
        statement_preview,
        reconciliation_preview,
        margin_scope,
        metering_freshness,
    );
    let (internal_money_arithmetic_state, internal_money_arithmetic_blockers) =
        internal_money_arithmetic_readiness(reconciliation_preview, margin_scope);
    let (contractual_settlement_state, contractual_settlement_blockers) =
        contractual_settlement_readiness(
            statement_preview,
            metering_freshness,
            internal_money_arithmetic_state,
        );
    let mut summary = serde_json::Map::new();
    let mut insert = |key: &str, value: Value| {
        summary.insert(key.to_string(), value);
    };

    insert("scope_code", json!(scope_code));
    insert("scope_label", json!(scope_label));
    insert(
        "contractual_state",
        statement_preview["contractual_state"].clone(),
    );
    insert(
        "settlement_stage",
        statement_preview["settlement_stage"].clone(),
    );
    insert(
        "settlement_stage_family",
        statement_preview["settlement_stage_family"].clone(),
    );
    insert(
        "next_settlement_stage_candidate",
        statement_preview["next_settlement_stage_candidate"].clone(),
    );
    insert(
        "next_settlement_stage_blockers",
        statement_preview["next_settlement_stage_blockers"].clone(),
    );
    insert(
        "future_reserved_settlement_stages",
        statement_preview["future_reserved_settlement_stages"].clone(),
    );
    insert(
        "contractual_readiness_model_version",
        json!(contract.contractual_readiness_model_version.clone()),
    );
    insert(
        "transactional_statuses",
        statement_preview["transactional_statuses"].clone(),
    );
    insert(
        "coverage_state",
        statement_preview["coverage"]["completeness_state"].clone(),
    );
    insert(
        "provisional_close_state",
        statement_preview["provisional_close_state"].clone(),
    );
    insert(
        "provisional_close_candidate",
        statement_preview["provisional_close_candidate"].clone(),
    );
    insert(
        "provisional_close_barriers",
        statement_preview["provisional_close_barriers"].clone(),
    );
    insert(
        "billing_close_barriers",
        statement_preview["billing_close_barriers"].clone(),
    );
    insert(
        "usage_truth_completeness_state",
        reconciliation_preview["usage_truth_completeness_state"].clone(),
    );
    insert(
        "rate_card_truth_completeness_state",
        reconciliation_preview["rate_card_truth_completeness_state"].clone(),
    );
    insert(
        "provider_cost_truth_completeness_state",
        reconciliation_preview["provider_cost_truth_completeness_state"].clone(),
    );
    insert(
        "invoice_evidence_completeness_state",
        reconciliation_preview["invoice_evidence_completeness_state"].clone(),
    );
    insert(
        "money_truth_completeness_state",
        reconciliation_preview["money_truth_completeness_state"].clone(),
    );
    insert(
        "reconciliation_readiness_state",
        reconciliation_preview["reconciliation_readiness_state"].clone(),
    );
    insert(
        "required_sources_for_usage_truth",
        reconciliation_preview["required_sources_for_usage_truth"].clone(),
    );
    insert(
        "required_sources_for_cost_truth",
        reconciliation_preview["required_sources_for_cost_truth"].clone(),
    );
    insert(
        "optional_sources_for_invoice_evidence",
        reconciliation_preview["optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_usage_truth",
        reconciliation_preview["unready_required_sources_for_usage_truth"].clone(),
    );
    insert(
        "unready_required_sources_for_cost_truth",
        reconciliation_preview["unready_required_sources_for_cost_truth"].clone(),
    );
    insert(
        "unready_optional_sources_for_invoice_evidence",
        reconciliation_preview["unready_optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "reconciliation_governance_blocking_reasons",
        reconciliation_preview["governance_blocking_reasons"].clone(),
    );
    insert("rate_card_status", rate_card_binding["status"].clone());
    insert(
        "rate_card_version",
        rate_card_binding["bound_rate_card_version"].clone(),
    );
    insert("rate_card_provider", rate_card_binding["provider"].clone());
    insert(
        "rate_card_currency_profile",
        rate_card_binding["bound_currency_profile"].clone(),
    );
    insert(
        "provider_usage_provider",
        provider_usage_binding["provider"].clone(),
    );
    insert(
        "provider_invoice_provider",
        provider_invoice_binding["provider"].clone(),
    );
    insert(
        "provider_usage_scope_alignment_state",
        reconciliation_preview["provider_usage_scope_alignment_state"].clone(),
    );
    insert(
        "provider_invoice_scope_alignment_state",
        reconciliation_preview["provider_invoice_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_scope_alignment_state",
        reconciliation_preview["rate_card_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_provider_alignment_state",
        reconciliation_preview["rate_card_provider_alignment_state"].clone(),
    );
    insert(
        "invoice_provider_alignment_state",
        reconciliation_preview["invoice_provider_alignment_state"].clone(),
    );
    insert(
        "provider_identity_state",
        reconciliation_preview["provider_identity_state"].clone(),
    );
    insert(
        "reconciliation_temporal_truth_state",
        reconciliation_preview["temporal_truth_state"].clone(),
    );
    insert(
        "metering_ingest_state",
        metering_freshness["metering_ingest_state"].clone(),
    );
    insert(
        "contractual_lag_state",
        metering_freshness["contractual_lag_state"].clone(),
    );
    insert(
        "contractual_freshness_state",
        metering_freshness["contractual_freshness_state"].clone(),
    );
    insert(
        "can_treat_scope_as_stable",
        metering_freshness["can_treat_scope_as_stable"].clone(),
    );
    insert(
        "latest_event_age_ms",
        metering_freshness["latest_event_age_ms"].clone(),
    );
    insert(
        "latest_ingest_lag_ms",
        metering_freshness["latest_ingest_lag_ms"].clone(),
    );
    insert(
        "p95_ingest_lag_ms",
        metering_freshness["p95_ingest_lag_ms"].clone(),
    );
    insert(
        "provisional_close_earliest_at_epoch_ms",
        statement_preview["period"]["provisional_close_earliest_at_epoch_ms"].clone(),
    );
    insert(
        "late_arrival_deadline_epoch_ms",
        statement_preview["period"]["late_arrival_deadline_epoch_ms"].clone(),
    );
    insert(
        "measured_non_billable_lower_bound_tokens",
        statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
    );
    insert(
        "adjusted_measured_non_billable_lower_bound_tokens",
        statement_preview["adjusted_measured_non_billable_lower_bound_tokens"].clone(),
    );
    insert(
        "billable_lower_bound_tokens",
        statement_preview["billable_lower_bound_tokens"].clone(),
    );
    insert(
        "internal_provider_billed_tokens",
        reconciliation_preview["internal_provider_billed_tokens"].clone(),
    );
    insert(
        "internal_observed_whole_cycle_lower_bound_tokens",
        reconciliation_preview["internal_observed_whole_cycle_lower_bound_tokens"].clone(),
    );
    insert(
        "verified_internal_observed_whole_cycle_lower_bound_tokens",
        reconciliation_preview["verified_internal_observed_whole_cycle_lower_bound_tokens"].clone(),
    );
    insert(
        "internal_provider_cost_estimate_amount",
        reconciliation_preview["internal_provider_cost_estimate_amount"].clone(),
    );
    insert(
        "external_provider_usage_tokens",
        reconciliation_preview["external_provider_usage_tokens"].clone(),
    );
    insert(
        "external_provider_cost_amount",
        reconciliation_preview["external_provider_cost_amount"].clone(),
    );
    insert(
        "external_invoice_amount",
        reconciliation_preview["external_invoice_amount"].clone(),
    );
    insert(
        "drift_tokens",
        reconciliation_preview["drift_tokens"].clone(),
    );
    insert(
        "drift_amount",
        reconciliation_preview["drift_amount"].clone(),
    );
    insert(
        "invoice_drift_amount",
        reconciliation_preview["invoice_drift_amount"].clone(),
    );
    insert(
        "reconciliation_state",
        reconciliation_preview["reconciliation_state"].clone(),
    );
    insert("margin_state", margin_scope["margin_state"].clone());
    insert(
        "margin_confidence_state",
        margin_scope["margin_confidence_state"].clone(),
    );
    insert(
        "margin_readiness_state",
        margin_scope["margin_readiness_state"].clone(),
    );
    insert(
        "infra_cost_truth_completeness_state",
        margin_scope["infra_cost_truth_completeness_state"].clone(),
    );
    insert(
        "pricing_truth_completeness_state",
        margin_scope["pricing_truth_completeness_state"].clone(),
    );
    insert(
        "customer_savings_money_truth_completeness_state",
        margin_scope["customer_savings_money_truth_completeness_state"].clone(),
    );
    insert(
        "amai_cost_truth_completeness_state",
        margin_scope["amai_cost_truth_completeness_state"].clone(),
    );
    insert(
        "margin_truth_completeness_state",
        margin_scope["margin_truth_completeness_state"].clone(),
    );
    insert(
        "required_sources_for_margin_truth",
        margin_scope["required_sources_for_margin_truth"].clone(),
    );
    insert(
        "optional_sources_for_margin_invoice_evidence",
        margin_scope["optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_margin_truth",
        margin_scope["unready_required_sources_for_margin_truth"].clone(),
    );
    insert(
        "margin_provider_identity_state",
        margin_scope["provider_identity_state"].clone(),
    );
    insert(
        "margin_temporal_truth_state",
        margin_scope["temporal_truth_state"].clone(),
    );
    insert(
        "infra_cost_scope_alignment_state",
        margin_scope["infra_cost_scope_alignment_state"].clone(),
    );
    insert(
        "margin_blocking_reasons",
        margin_scope["blocking_reasons"].clone(),
    );
    insert(
        "internal_money_arithmetic_readiness_state",
        json!(internal_money_arithmetic_state),
    );
    insert(
        "internal_money_arithmetic_blocking_reasons",
        json!(internal_money_arithmetic_blockers),
    );
    insert(
        "contractual_settlement_readiness_state",
        json!(contractual_settlement_state),
    );
    insert(
        "contractual_settlement_blocking_reasons",
        json!(contractual_settlement_blockers),
    );
    insert(
        "adjustment_state",
        statement_preview["adjustment_preview"]["correction_action_state"].clone(),
    );
    insert(
        "pending_adjustment_entries_count",
        statement_preview["adjustment_preview"]["pending_entries_count"].clone(),
    );
    insert(
        "applied_adjustment_entries_count",
        statement_preview["adjustment_preview"]["applied_entries_count"].clone(),
    );
    insert(
        "disputed_adjustment_entries_count",
        statement_preview["adjustment_preview"]["disputed_entries_count"].clone(),
    );
    insert(
        "close_barriers",
        statement_preview["close_barriers"].clone(),
    );
    insert(
        "blocking_reasons",
        combine_reason_arrays(&[
            &statement_preview["close_barriers"],
            &statement_preview["next_settlement_stage_blockers"],
            &reconciliation_preview["blocking_reasons"],
            &margin_scope["blocking_reasons"],
            &metering_freshness["blocking_reasons"],
        ]),
    );
    insert("suitability", suitability);
    insert("customer_review_ready", json!(true));
    insert("invoice_ready", json!(false));
    insert(
        "currency_profile",
        statement_preview["currency_profile"].clone(),
    );
    insert(
        "note",
        json!(if settlement_stage == "measured_review_ready_report_only" {
            "Это короткий customer-facing summary поверх statement/reconciliation/margin/freshness previews. Он уже review-ready, но всё ещё остаётся report-only и не является invoice."
        } else {
            "Это короткий customer-facing summary поверх statement/reconciliation/margin/freshness previews. Он пригоден для review и audit, но не для invoice."
        }),
    );

    Value::Object(summary)
}

fn build_statement_preview(
    scope_code: &str,
    scope_label: &str,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
    profile: &ResolvedProfile,
    summary: &Value,
    contract: &TokenBudgetContractConfig,
    adjustment_registry: &Value,
    rate_card: &Value,
    reconciliation_contract: &Value,
    metering_freshness: &Value,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let adjustment_preview =
        build_adjustment_preview_json(scope_code, contract, adjustment_registry);
    let provisional_close_barriers =
        provisional_close_barriers(summary, metering_freshness, &adjustment_preview);
    let provisional_close_candidate = provisional_close_barriers.is_empty();
    let mut billing_close_barriers = vec!["billing_mode_report_only".to_string()];
    if reconciliation_contract["ready_for_external_reconciliation"].as_bool() != Some(true) {
        billing_close_barriers.push("external_reconciliation_not_bound".to_string());
    }
    if rate_card["money_conversion_enabled"].as_bool() != Some(true) {
        billing_close_barriers.push("rate_card_unpriced".to_string());
    }
    if summary["verified_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0)
        <= 0
    {
        billing_close_barriers.push("no_positive_verified_lower_bound".to_string());
    }
    let mut close_barriers = billing_close_barriers.clone();
    for barrier in &provisional_close_barriers {
        if !close_barriers.contains(barrier) {
            close_barriers.push(barrier.clone());
        }
    }
    let measured_non_billable_lower_bound_tokens = summary["verified_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0);
    let applied_tokens_delta = adjustment_preview["applied_tokens_delta"]
        .as_i64()
        .unwrap_or(0);
    let lifecycle_state = statement_lifecycle_state(&adjustment_preview);
    let measured_events = events.len();
    let settlement_stage = settlement_stage(
        measured_events,
        &adjustment_preview,
        metering_freshness,
        provisional_close_candidate,
    );
    let provisional_close_state = if provisional_close_candidate {
        "report_only_preview_provisionally_stable"
    } else {
        "report_only_preview_provisional_hold"
    };
    let next_stage_candidate = next_settlement_stage_candidate(
        measured_events,
        metering_freshness,
        provisional_close_candidate,
        &billing_close_barriers,
    );
    let next_stage_blockers = next_settlement_stage_blockers(
        measured_events,
        &provisional_close_barriers,
        &billing_close_barriers,
    );
    let transactional_statuses = build_transactional_statuses(
        contract,
        measured_events,
        settlement_stage,
        next_stage_candidate,
        &next_stage_blockers,
        &billing_close_barriers,
        &adjustment_preview,
    );
    let internal_delivered_tokens = summary["delivered_tokens"].as_u64().unwrap_or(0);
    let internal_recovery_tokens = summary["recovery_tokens"].as_u64().unwrap_or(0);
    let internal_observed_whole_cycle_lower_bound_tokens =
        summary["observed_whole_cycle_with_amai_tokens"]
            .as_u64()
            .unwrap_or(internal_delivered_tokens.saturating_add(internal_recovery_tokens));
    let verified_internal_observed_whole_cycle_lower_bound_tokens =
        summary["verified_observed_whole_cycle_with_amai_tokens"]
            .as_u64()
            .unwrap_or(
                summary["verified_delivered_tokens"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(summary["verified_recovery_tokens"].as_u64().unwrap_or(0)),
            );
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "statement_status": "report_only_preview",
        "lifecycle_state": lifecycle_state,
        "settlement_stage": settlement_stage,
        "settlement_stage_family": settlement_stage_family(settlement_stage),
        "next_settlement_stage_candidate": next_stage_candidate,
        "next_settlement_stage_blockers": next_stage_blockers,
        "future_reserved_settlement_stages": future_reserved_settlement_stages(),
        "transactional_statuses": transactional_statuses,
        "operational_state": "live_measurement_open",
        "contractual_state": match lifecycle_state {
            "measured_non_billable_dispute_hold" => "report_only_preview_dispute_hold",
            "measured_non_billable_pending_adjustment" => "report_only_preview_pending_adjustment",
            "measured_non_billable_adjusted_report_only" => "report_only_preview_adjusted",
            _ => "report_only_preview_open",
        },
        "close_readiness": if provisional_close_candidate {
            "provisionally_stable_report_only"
        } else {
            "provisionally_blocked_report_only"
        },
        "close_candidate": false,
        "provisional_close_state": provisional_close_state,
        "provisional_close_candidate": provisional_close_candidate,
        "provisional_close_barriers": provisional_close_barriers,
        "billing_close_barriers": billing_close_barriers,
        "close_barriers": close_barriers,
        "freeze_status": freeze_status(events, metering_freshness, provisional_close_candidate),
        "late_arrival_mode": "accepting_events_until_contractual_close_exists",
        "correction_mode": adjustment_preview["correction_action_state"].clone(),
        "dispute_mode": if adjustment_preview["disputed_entries_count"].as_u64().unwrap_or(0) > 0 {
            Value::String("open_dispute_hold_report_only".to_string())
        } else {
            Value::String("not_open_report_only".to_string())
        },
        "period": build_statement_period_json(
            scope_code,
            scope_label,
            now_epoch_ms,
            events,
            profile,
            contract,
            metering_freshness,
            provisional_close_candidate,
            &provisional_close_barriers
        ),
        "adjustment_preview": adjustment_preview.clone(),
        "coverage": summary["coverage"],
        "client_limit_meter_alignment": build_client_limit_meter_alignment(
            contract,
            "statement_preview",
            summary,
            Some(events),
            Some(rollout_observations),
            assistant_scope,
        ),
        "freshness": metering_freshness.clone(),
        "internal_delivered_tokens": internal_delivered_tokens,
        "internal_recovery_tokens": internal_recovery_tokens,
        "internal_observed_whole_cycle_lower_bound_tokens": internal_observed_whole_cycle_lower_bound_tokens,
        "verified_internal_observed_whole_cycle_lower_bound_tokens": verified_internal_observed_whole_cycle_lower_bound_tokens,
        "internal_provider_billed_tokens": internal_observed_whole_cycle_lower_bound_tokens,
        "measured_non_billable_lower_bound_tokens": measured_non_billable_lower_bound_tokens,
        "adjusted_measured_non_billable_lower_bound_tokens": measured_non_billable_lower_bound_tokens
            .saturating_add(applied_tokens_delta),
        "billable_lower_bound_tokens": Value::Null,
        "final_amount": Value::Null,
        "currency_profile": rate_card["bound_currency_profile"]
            .as_str()
            .unwrap_or(&contract.currency_profile)
            .to_string(),
        "settlement_status": contract.settlement_status.clone(),
        "note": if settlement_stage == "measured_review_ready_report_only" {
            "Это preview measured lower bound для scope. Он уже review-ready и provisionally stable, но по-прежнему не является billable statement или суммой к оплате."
        } else {
            "Это preview measured lower bound для scope, а не закрытый statement и не сумма к оплате."
        }
    })
}

fn contractual_line_item_json(event: &TokenBudgetEvent) -> Value {
    json!({
        "event_id": event.event_id.clone(),
        "correlation_id": event.correlation_id.clone(),
        "occurred_at_epoch_ms": event.occurred_at_epoch_ms,
        "ingested_at_epoch_ms": event.ingested_at_epoch_ms,
        "project_code": event.project.clone(),
        "namespace_code": event.namespace.clone(),
        "source_kind": event.source_kind.clone(),
        "traffic_class": event.traffic_class.clone(),
        "measurement_scope": event.measurement_scope.clone(),
        "query_hash": event.query_hash.clone(),
        "query_type": event.query_type.clone(),
        "target_kind": event.target_kind.clone(),
        "baseline_strategy": event.baseline_strategy.clone(),
        "retrieval_mode": event.retrieval_mode.clone(),
        "baseline_tokens": event.naive_tokens,
        "delivered_tokens": event.context_tokens,
        "recovery_tokens": event.recovery_tokens,
        "whole_cycle_observed": {
            "client_prompt_tokens": event.client_prompt_tokens,
            "assistant_generation_tokens": event.assistant_generation_tokens,
            "tool_overhead_tokens": event.tool_overhead_tokens,
            "continuity_restore_tokens": event.continuity_restore_tokens,
        },
        "effective_saved_tokens": event.effective_saved_tokens,
        "quality_ok": event.quality_ok,
        "quality_method": event.quality_method.clone(),
        "quality_tier": event.quality_tier.clone(),
        "usage_state": {
            "lifecycle_status": usage_lifecycle_status(event),
            "reporting_layer": usage_reporting_layer(event),
            "excluded_reason_code": usage_excluded_reason_code(event),
        },
        "settlement_status": event.settlement_status.clone(),
    })
}

fn build_contractual_line_item_sets(scope_events: &[TokenBudgetEvent]) -> (Vec<Value>, Vec<Value>) {
    let included_items = scope_events
        .iter()
        .filter(|event| usage_excluded_reason_code(event).is_none())
        .map(contractual_line_item_json)
        .collect::<Vec<_>>();
    let excluded_items = scope_events
        .iter()
        .filter(|event| usage_excluded_reason_code(event).is_some())
        .map(contractual_line_item_json)
        .collect::<Vec<_>>();
    (included_items, excluded_items)
}

fn hash_line_items(items: &[Value]) -> Result<String> {
    let bytes = serde_json::to_vec(items).context("failed to encode contractual line items")?;
    Ok(hex_sha256(&bytes))
}

fn build_settlement_report_preview(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let period = &statement_preview["period"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let external_truth_manifest = &statement_export_preview["external_truth_manifest"];
    let settlement_report_identity = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        statement_export_preview["scope_code"]
            .as_str()
            .unwrap_or("unknown-scope"),
        contract.settlement_report_preview_version,
        statement_export_preview["statement_preview_id"]
            .as_str()
            .unwrap_or("missing-statement-id"),
        statement_export_preview["included_events_hash"]
            .as_str()
            .unwrap_or("missing-included-hash"),
        statement_export_preview["excluded_events_hash"]
            .as_str()
            .unwrap_or("missing-excluded-hash"),
        contract.billing_policy_version,
        contract.reconciliation_contract_version,
        external_truth_manifest["manifest_hash"]
            .as_str()
            .unwrap_or("missing-truth-manifest"),
    );
    json!({
        "model_version": contract.settlement_report_preview_version.clone(),
        "settlement_report_id": hex_sha256(settlement_report_identity.as_bytes()),
        "statement_preview_id": statement_export_preview["statement_preview_id"].clone(),
        "scope_code": statement_export_preview["scope_code"].clone(),
        "scope_label": statement_export_preview["scope_label"].clone(),
        "period_kind": period["period_kind"].clone(),
        "period_start_epoch_ms": period["period_start_epoch_ms"].clone(),
        "period_end_epoch_ms": period["period_end_epoch_ms"].clone(),
        "provisional_close_earliest_at_epoch_ms": statement_export_preview["provisional_close_earliest_at_epoch_ms"].clone(),
        "late_arrival_deadline_epoch_ms": period["late_arrival_deadline_epoch_ms"].clone(),
        "settlement_stage": statement_export_preview["settlement_stage"].clone(),
        "settlement_stage_family": statement_export_preview["settlement_stage_family"].clone(),
        "next_settlement_stage_candidate": statement_export_preview["next_settlement_stage_candidate"].clone(),
        "next_settlement_stage_blockers": statement_export_preview["next_settlement_stage_blockers"].clone(),
        "contractual_readiness_model_version": statement_export_preview["contractual_readiness_model_version"].clone(),
        "internal_money_arithmetic_readiness_state": statement_export_preview["internal_money_arithmetic_readiness_state"].clone(),
        "internal_money_arithmetic_blocking_reasons": statement_export_preview["internal_money_arithmetic_blocking_reasons"].clone(),
        "contractual_settlement_readiness_state": statement_export_preview["contractual_settlement_readiness_state"].clone(),
        "contractual_settlement_blocking_reasons": statement_export_preview["contractual_settlement_blocking_reasons"].clone(),
        "coverage_state": statement_export_preview["coverage_state"].clone(),
        "contractual_freshness_state": statement_export_preview["contractual_freshness_state"].clone(),
        "usage_truth_completeness_state": statement_export_preview["usage_truth_completeness_state"].clone(),
        "rate_card_truth_completeness_state": statement_export_preview["rate_card_truth_completeness_state"].clone(),
        "provider_cost_truth_completeness_state": statement_export_preview["provider_cost_truth_completeness_state"].clone(),
        "invoice_evidence_completeness_state": statement_export_preview["invoice_evidence_completeness_state"].clone(),
        "money_truth_completeness_state": statement_export_preview["money_truth_completeness_state"].clone(),
        "pricing_truth_completeness_state": statement_export_preview["pricing_truth_completeness_state"].clone(),
        "customer_savings_money_truth_completeness_state": statement_export_preview["customer_savings_money_truth_completeness_state"].clone(),
        "amai_cost_truth_completeness_state": statement_export_preview["amai_cost_truth_completeness_state"].clone(),
        "margin_truth_completeness_state": statement_export_preview["margin_truth_completeness_state"].clone(),
        "reconciliation_readiness_state": statement_export_preview["reconciliation_readiness_state"].clone(),
        "margin_readiness_state": statement_export_preview["margin_readiness_state"].clone(),
        "required_sources_for_usage_truth": statement_export_preview["required_sources_for_usage_truth"].clone(),
        "required_sources_for_cost_truth": statement_export_preview["required_sources_for_cost_truth"].clone(),
        "optional_sources_for_invoice_evidence": statement_export_preview["optional_sources_for_invoice_evidence"].clone(),
        "unready_required_sources_for_usage_truth": statement_export_preview["unready_required_sources_for_usage_truth"].clone(),
        "unready_required_sources_for_cost_truth": statement_export_preview["unready_required_sources_for_cost_truth"].clone(),
        "unready_optional_sources_for_invoice_evidence": statement_export_preview["unready_optional_sources_for_invoice_evidence"].clone(),
        "required_sources_for_margin_truth": statement_export_preview["required_sources_for_margin_truth"].clone(),
        "optional_sources_for_margin_invoice_evidence": statement_export_preview["optional_sources_for_margin_invoice_evidence"].clone(),
        "unready_required_sources_for_margin_truth": statement_export_preview["unready_required_sources_for_margin_truth"].clone(),
        "provider_identity_state": statement_export_preview["provider_identity_state"].clone(),
        "included_events_count": statement_export_preview["included_events_count"].clone(),
        "excluded_events_count": statement_export_preview["excluded_events_count"].clone(),
        "included_events_hash": statement_export_preview["included_events_hash"].clone(),
        "excluded_events_hash": statement_export_preview["excluded_events_hash"].clone(),
        "measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
        "adjusted_measured_non_billable_lower_bound_tokens": statement_preview["adjusted_measured_non_billable_lower_bound_tokens"].clone(),
        "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
        "final_amount": statement_preview["final_amount"].clone(),
        "currency_profile": statement_export_preview["currency_profile"].clone(),
        "external_truth_manifest_hash": external_truth_manifest["manifest_hash"].clone(),
        "customer_contractual_boundary": build_customer_contractual_boundary_from_export(
            contract,
            "customer_settlement_report_preview_report_only",
            statement_export_preview,
        ),
        "settlement_activation_governance": statement_export_preview["settlement_activation_governance"].clone(),
        "adjustment_summary": {
            "registry_status": adjustment_preview["registry_status"].clone(),
            "correction_action_state": adjustment_preview["correction_action_state"].clone(),
            "pending_entries_count": adjustment_preview["pending_entries_count"].clone(),
            "applied_entries_count": adjustment_preview["applied_entries_count"].clone(),
            "disputed_entries_count": adjustment_preview["disputed_entries_count"].clone(),
            "applied_tokens_delta": adjustment_preview["applied_tokens_delta"].clone(),
            "applied_amount_delta": adjustment_preview["applied_amount_delta"].clone(),
        },
        "policy_versions": {
            "settlement_statement_version": contract.settlement_statement_version.clone(),
            "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
            "billing_policy_version": contract.billing_policy_version.clone(),
            "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
            "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
            "correction_policy_version": contract.correction_policy_version.clone(),
            "dispute_policy_version": contract.dispute_policy_version.clone(),
            "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
            "statement_period_governance_version": contract.statement_period_governance_version.clone(),
            "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
            "adjustment_registry_version": contract.adjustment_registry_version.clone(),
            "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
            "margin_model_version": contract.margin_model_version.clone(),
            "rate_card_binding_model_version": contract.rate_card_binding_model_version.clone(),
            "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
            "contractual_readiness_model_version": contract.contractual_readiness_model_version.clone(),
        },
        "report_only": true,
        "invoice_grade": false,
        "blocking_reasons": statement_export_preview["blocking_reasons"].clone(),
        "note": "Settlement report preview собирает period anchors, hashes, policy snapshot и truth states в один review-grade object. Он пригоден для audit/review, но не является invoice или финальным settlement amount."
    })
}

fn build_statement_export_preview(
    report: &Value,
    scope_code: &str,
    scope_label: &str,
    scope_events: &[TokenBudgetEvent],
    contract: &TokenBudgetContractConfig,
    include_verify_events: bool,
) -> Result<Value> {
    let statement_preview = report["token_budget_report"]["statement_previews"][scope_code].clone();
    let reconciliation_preview =
        report["token_budget_report"]["reconciliation_previews"][scope_code].clone();
    let margin_scope = report["token_budget_report"]["margin_view"][scope_code].clone();
    let contractual_summary =
        report["token_budget_report"]["contractual_statement_summaries"][scope_code].clone();

    let (included_items, excluded_items) = build_contractual_line_item_sets(scope_events);
    let included_hash = hash_line_items(&included_items)?;
    let excluded_hash = hash_line_items(&excluded_items)?;
    let export_identity = format!(
        "{}:{}:{}:{}:{}",
        scope_code,
        contract.settlement_statement_version,
        contract.contractual_statement_export_version,
        included_hash,
        excluded_hash
    );
    let adjustment_preview = statement_preview["adjustment_preview"].clone();
    let pending_entries = adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0);
    let applied_entries = adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0);
    let disputed_entries = adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0);
    let adjustment_status = adjustment_preview["status"].as_str().unwrap_or("unknown");
    let credit_action_state =
        if matches!(adjustment_status, "not_configured" | "default_path_missing") {
            "registry_not_configured"
        } else if pending_entries > 0 {
            "pending_review"
        } else if applied_entries > 0 {
            "applied_report_only_entries_present"
        } else {
            "no_credit_entries"
        };
    let dispute_action_state = if disputed_entries > 0 {
        "open_dispute_entries"
    } else {
        "no_open_disputes"
    };

    let mut preview = serde_json::Map::new();
    let mut insert = |key: &str, value: Value| {
        preview.insert(key.to_string(), value);
    };

    insert(
        "model_version",
        json!(contract.contractual_statement_export_version.clone()),
    );
    insert("scope_code", json!(scope_code));
    insert("scope_label", json!(scope_label));
    insert(
        "statement_preview_id",
        json!(hex_sha256(export_identity.as_bytes())),
    );
    insert(
        "contractual_state",
        contractual_summary["contractual_state"].clone(),
    );
    insert(
        "settlement_stage",
        contractual_summary["settlement_stage"].clone(),
    );
    insert(
        "settlement_stage_family",
        contractual_summary["settlement_stage_family"].clone(),
    );
    insert(
        "next_settlement_stage_candidate",
        contractual_summary["next_settlement_stage_candidate"].clone(),
    );
    insert(
        "next_settlement_stage_blockers",
        contractual_summary["next_settlement_stage_blockers"].clone(),
    );
    insert(
        "future_reserved_settlement_stages",
        contractual_summary["future_reserved_settlement_stages"].clone(),
    );
    insert(
        "contractual_readiness_model_version",
        contractual_summary["contractual_readiness_model_version"].clone(),
    );
    insert(
        "transactional_statuses",
        contractual_summary["transactional_statuses"].clone(),
    );
    insert(
        "coverage_state",
        contractual_summary["coverage_state"].clone(),
    );
    insert(
        "provisional_close_state",
        contractual_summary["provisional_close_state"].clone(),
    );
    insert(
        "provisional_close_candidate",
        contractual_summary["provisional_close_candidate"].clone(),
    );
    insert(
        "provisional_close_earliest_at_epoch_ms",
        contractual_summary["provisional_close_earliest_at_epoch_ms"].clone(),
    );
    insert(
        "usage_truth_completeness_state",
        contractual_summary["usage_truth_completeness_state"].clone(),
    );
    insert(
        "rate_card_truth_completeness_state",
        contractual_summary["rate_card_truth_completeness_state"].clone(),
    );
    insert(
        "provider_cost_truth_completeness_state",
        contractual_summary["provider_cost_truth_completeness_state"].clone(),
    );
    insert(
        "invoice_evidence_completeness_state",
        contractual_summary["invoice_evidence_completeness_state"].clone(),
    );
    insert(
        "money_truth_completeness_state",
        contractual_summary["money_truth_completeness_state"].clone(),
    );
    insert(
        "reconciliation_readiness_state",
        contractual_summary["reconciliation_readiness_state"].clone(),
    );
    insert(
        "required_sources_for_usage_truth",
        contractual_summary["required_sources_for_usage_truth"].clone(),
    );
    insert(
        "required_sources_for_cost_truth",
        contractual_summary["required_sources_for_cost_truth"].clone(),
    );
    insert(
        "optional_sources_for_invoice_evidence",
        contractual_summary["optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_usage_truth",
        contractual_summary["unready_required_sources_for_usage_truth"].clone(),
    );
    insert(
        "unready_required_sources_for_cost_truth",
        contractual_summary["unready_required_sources_for_cost_truth"].clone(),
    );
    insert(
        "unready_optional_sources_for_invoice_evidence",
        contractual_summary["unready_optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "rate_card_status",
        contractual_summary["rate_card_status"].clone(),
    );
    insert(
        "rate_card_version",
        contractual_summary["rate_card_version"].clone(),
    );
    insert(
        "rate_card_provider",
        contractual_summary["rate_card_provider"].clone(),
    );
    insert(
        "rate_card_currency_profile",
        contractual_summary["rate_card_currency_profile"].clone(),
    );
    insert(
        "provider_usage_provider",
        contractual_summary["provider_usage_provider"].clone(),
    );
    insert(
        "provider_invoice_provider",
        contractual_summary["provider_invoice_provider"].clone(),
    );
    insert(
        "provider_usage_scope_alignment_state",
        contractual_summary["provider_usage_scope_alignment_state"].clone(),
    );
    insert(
        "provider_invoice_scope_alignment_state",
        contractual_summary["provider_invoice_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_scope_alignment_state",
        contractual_summary["rate_card_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_provider_alignment_state",
        contractual_summary["rate_card_provider_alignment_state"].clone(),
    );
    insert(
        "invoice_provider_alignment_state",
        contractual_summary["invoice_provider_alignment_state"].clone(),
    );
    insert(
        "provider_identity_state",
        contractual_summary["provider_identity_state"].clone(),
    );
    insert(
        "reconciliation_temporal_truth_state",
        contractual_summary["reconciliation_temporal_truth_state"].clone(),
    );
    insert(
        "contractual_freshness_state",
        contractual_summary["contractual_freshness_state"].clone(),
    );
    insert(
        "reconciliation_state",
        contractual_summary["reconciliation_state"].clone(),
    );
    insert("margin_state", contractual_summary["margin_state"].clone());
    insert(
        "margin_confidence_state",
        contractual_summary["margin_confidence_state"].clone(),
    );
    insert(
        "margin_readiness_state",
        contractual_summary["margin_readiness_state"].clone(),
    );
    insert(
        "infra_cost_truth_completeness_state",
        contractual_summary["infra_cost_truth_completeness_state"].clone(),
    );
    insert(
        "pricing_truth_completeness_state",
        contractual_summary["pricing_truth_completeness_state"].clone(),
    );
    insert(
        "customer_savings_money_truth_completeness_state",
        contractual_summary["customer_savings_money_truth_completeness_state"].clone(),
    );
    insert(
        "amai_cost_truth_completeness_state",
        contractual_summary["amai_cost_truth_completeness_state"].clone(),
    );
    insert(
        "margin_truth_completeness_state",
        contractual_summary["margin_truth_completeness_state"].clone(),
    );
    insert(
        "required_sources_for_margin_truth",
        contractual_summary["required_sources_for_margin_truth"].clone(),
    );
    insert(
        "optional_sources_for_margin_invoice_evidence",
        contractual_summary["optional_sources_for_margin_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_margin_truth",
        contractual_summary["unready_required_sources_for_margin_truth"].clone(),
    );
    insert(
        "margin_provider_identity_state",
        contractual_summary["margin_provider_identity_state"].clone(),
    );
    insert(
        "margin_temporal_truth_state",
        contractual_summary["margin_temporal_truth_state"].clone(),
    );
    insert(
        "infra_cost_scope_alignment_state",
        contractual_summary["infra_cost_scope_alignment_state"].clone(),
    );
    insert(
        "margin_blocking_reasons",
        contractual_summary["margin_blocking_reasons"].clone(),
    );
    insert(
        "internal_money_arithmetic_readiness_state",
        contractual_summary["internal_money_arithmetic_readiness_state"].clone(),
    );
    insert(
        "internal_money_arithmetic_blocking_reasons",
        contractual_summary["internal_money_arithmetic_blocking_reasons"].clone(),
    );
    insert(
        "contractual_settlement_readiness_state",
        contractual_summary["contractual_settlement_readiness_state"].clone(),
    );
    insert(
        "contractual_settlement_blocking_reasons",
        contractual_summary["contractual_settlement_blocking_reasons"].clone(),
    );
    insert("export_status", json!("review_ready_report_only"));
    insert("included_events_count", json!(included_items.len()));
    insert("excluded_events_count", json!(excluded_items.len()));
    insert("included_events_hash", json!(included_hash));
    insert("excluded_events_hash", json!(excluded_hash));
    insert("customer_review_ready", json!(true));
    insert("invoice_ready", json!(false));
    insert("credit_action_state", json!(credit_action_state));
    insert("dispute_action_state", json!(dispute_action_state));
    insert("pending_adjustment_entries_count", json!(pending_entries));
    insert("disputed_entries_count", json!(disputed_entries));
    insert(
        "export_semantics",
        json!({
            "surface_kind": "customer_review_report_only",
            "self_serve_state": "self_serve_ready_report_only",
            "invoice_grade": false,
            "operational_telemetry_included": false,
            "redaction_policy": "raw_query_text_removed_keep_query_hash_and_token_state",
            "customer_visible_sections": [
                "statement_preview_id",
                "settlement_report_preview",
                "contractual_state",
                "coverage_state",
                "external_truth_manifest",
                "transactional_statuses",
                "included_events_hash",
                "excluded_events_hash",
                "suitability",
                "evidence_pack_command"
            ]
        }),
    );
    insert(
        "blocking_reasons",
        contractual_summary["blocking_reasons"].clone(),
    );
    insert(
        "external_truth_manifest",
        report["token_budget_report"]["external_truth_manifest"].clone(),
    );
    insert("suitability", contractual_summary["suitability"].clone());
    insert("evidence_pack_available", json!(true));
    insert(
        "evidence_pack_command",
        json!(format!(
            "cargo run --release -- observe token-evidence-pack --scope {}{}",
            scope_code,
            if include_verify_events {
                " --include-verify-events true"
            } else {
                ""
            }
        )),
    );
    insert(
        "line_item_surfaces",
        json!({
            "statement_preview": statement_preview,
            "reconciliation_preview": reconciliation_preview,
            "margin_scope": margin_scope,
        }),
    );
    insert(
        "note",
        json!(
            "Это stable export preview для customer review: hashes и scope states уже зафиксированы, но invoice-grade settlement всё ещё не materialized."
        ),
    );
    let mut preview = Value::Object(preview);
    preview["customer_contractual_boundary"] = build_customer_contractual_boundary_from_export(
        contract,
        "customer_review_report_only",
        &preview,
    );
    preview["settlement_activation_governance"] =
        build_settlement_activation_governance_from_export(contract, &preview);
    preview["adjustment_activation_governance"] =
        build_adjustment_activation_governance_from_export(contract, &preview);
    preview["settlement_report_preview"] = build_settlement_report_preview(contract, &preview);
    Ok(preview)
}

fn build_contractual_evidence_pack(
    report: &Value,
    scope_code: &str,
    scope_label: &str,
    scope_events: &[TokenBudgetEvent],
    contract: &TokenBudgetContractConfig,
    profile: &ResolvedProfile,
    include_verify_events: bool,
    generated_at_epoch_ms: i64,
) -> Result<Value> {
    let (included_items, excluded_items) = build_contractual_line_item_sets(scope_events);

    let statement_preview = report["token_budget_report"]["statement_previews"][scope_code].clone();
    let reconciliation_preview =
        report["token_budget_report"]["reconciliation_previews"][scope_code].clone();
    let margin_scope = report["token_budget_report"]["margin_view"][scope_code].clone();
    let statement_export_preview =
        report["token_budget_report"]["statement_export_previews"][scope_code].clone();
    let mut customer_contractual_boundary =
        statement_export_preview["customer_contractual_boundary"].clone();
    customer_contractual_boundary["surface_kind"] = json!("customer_evidence_pack_report_only");
    let settlement_report_preview =
        settlement_report_preview_from_export(contract, &statement_export_preview);

    Ok(json!({
        "contractual_evidence_pack": {
            "pack_version": contract.contractual_evidence_pack_version.clone(),
            "generated_at_epoch_ms": generated_at_epoch_ms,
            "scope_code": scope_code,
            "scope_label": scope_label,
            "budget_profile": {
                "code": profile.code.clone(),
                "display_name": profile.display_name.clone(),
            },
        "include_verify_events": include_verify_events,
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "contract_versions": report["token_budget_report"]["contract"].clone(),
        "settlement_stage": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["settlement_stage"].clone(),
        "settlement_stage_family": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["settlement_stage_family"].clone(),
        "next_settlement_stage_candidate": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["next_settlement_stage_candidate"].clone(),
        "next_settlement_stage_blockers": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["next_settlement_stage_blockers"].clone(),
        "contractual_readiness_model_version": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["contractual_readiness_model_version"].clone(),
        "internal_money_arithmetic_readiness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["internal_money_arithmetic_readiness_state"].clone(),
        "internal_money_arithmetic_blocking_reasons": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["internal_money_arithmetic_blocking_reasons"].clone(),
        "contractual_settlement_readiness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["contractual_settlement_readiness_state"].clone(),
        "contractual_settlement_blocking_reasons": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["contractual_settlement_blocking_reasons"].clone(),
        "transactional_statuses": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["transactional_statuses"].clone(),
        "rate_card_status": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_status"].clone(),
        "rate_card_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_truth_completeness_state"].clone(),
        "provider_cost_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_cost_truth_completeness_state"].clone(),
        "invoice_evidence_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["invoice_evidence_completeness_state"].clone(),
        "required_sources_for_usage_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["required_sources_for_usage_truth"].clone(),
        "required_sources_for_cost_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["required_sources_for_cost_truth"].clone(),
        "optional_sources_for_invoice_evidence": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["optional_sources_for_invoice_evidence"].clone(),
        "unready_required_sources_for_usage_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_required_sources_for_usage_truth"].clone(),
        "unready_required_sources_for_cost_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_required_sources_for_cost_truth"].clone(),
        "unready_optional_sources_for_invoice_evidence": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_optional_sources_for_invoice_evidence"].clone(),
        "rate_card_version": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_version"].clone(),
        "rate_card_provider": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_provider"].clone(),
        "rate_card_currency_profile": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_currency_profile"].clone(),
        "provider_usage_provider": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_usage_provider"].clone(),
        "provider_invoice_provider": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_invoice_provider"].clone(),
        "provider_identity_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_identity_state"].clone(),
        "infra_cost_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["infra_cost_truth_completeness_state"].clone(),
        "pricing_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["pricing_truth_completeness_state"].clone(),
        "customer_savings_money_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["customer_savings_money_truth_completeness_state"].clone(),
        "amai_cost_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["amai_cost_truth_completeness_state"].clone(),
        "margin_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["margin_truth_completeness_state"].clone(),
        "required_sources_for_margin_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["required_sources_for_margin_truth"].clone(),
        "optional_sources_for_margin_invoice_evidence": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["optional_sources_for_margin_invoice_evidence"].clone(),
        "unready_required_sources_for_margin_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_required_sources_for_margin_truth"].clone(),
        "margin_readiness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["margin_readiness_state"].clone(),
        "external_truth_manifest": report["token_budget_report"]["external_truth_manifest"].clone(),
        "settlement_report_preview": settlement_report_preview,
        "customer_contractual_boundary": customer_contractual_boundary,
        "settlement_activation_governance": statement_export_preview["settlement_activation_governance"].clone(),
        "adjustment_activation_governance": statement_export_preview["adjustment_activation_governance"].clone(),
        "export_semantics": {
            "surface_kind": "customer_evidence_pack_report_only",
            "self_serve_state": "self_serve_ready_report_only",
            "invoice_grade": false,
            "operational_telemetry_included": false,
            "redaction_policy": "raw_query_text_removed_keep_query_hash_and_token_state",
            "customer_visible_sections": [
                "truth_guardrail",
                "contract_versions",
                "external_truth_manifest",
                "settlement_report_preview",
                "statement_preview",
                "reconciliation_preview",
                "margin_scope",
                "transactional_statuses",
                "line_items"
            ]
        },
        "statement_preview": statement_preview,
        "reconciliation_preview": reconciliation_preview,
        "margin_scope": margin_scope,
        "suitability": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["suitability"].clone(),
        "included_events_count": included_items.len(),
            "excluded_events_count": excluded_items.len(),
            "included_events_hash": hash_line_items(&included_items)?,
            "excluded_events_hash": hash_line_items(&excluded_items)?,
            "line_items": {
                "included": included_items,
                "excluded": excluded_items,
            },
            "note": "Это contractual evidence pack для report-only tokenonomics: он доказывает состав измеренного scope, но не превращает lower bound в invoice."
        }
    }))
}

fn validate_adjustment_scope(scope: &str) -> Result<()> {
    if matches!(scope, "current_session" | "rolling_window" | "lifetime") {
        Ok(())
    } else {
        bail!(
            "unsupported adjustment scope {} (expected current_session, rolling_window or lifetime)",
            scope
        )
    }
}

fn validate_adjustment_kind(kind: &str) -> Result<()> {
    if matches!(kind, "credit_note" | "adjustment_entry" | "dispute_hold") {
        Ok(())
    } else {
        bail!(
            "unsupported adjustment kind {} (expected credit_note, adjustment_entry or dispute_hold)",
            kind
        )
    }
}

fn validate_adjustment_status(status: &str) -> Result<()> {
    if matches!(
        status,
        "requested"
            | "pending_review"
            | "approved_but_unapplied"
            | "applied_report_only"
            | "disputed"
            | "rejected"
    ) {
        Ok(())
    } else {
        bail!(
            "unsupported adjustment status {} (expected requested, pending_review, approved_but_unapplied, applied_report_only, disputed or rejected)",
            status
        )
    }
}

fn adjustment_registry_write_path(repo_root: &Path) -> PathBuf {
    std::env::var("AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| adjustment_registry_default_path(repo_root))
}

fn load_adjustment_registry_file_for_write(path: &Path) -> Result<AdjustmentRegistryFile> {
    if !path.exists() {
        return Ok(AdjustmentRegistryFile::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read adjustment registry {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse adjustment registry {}", path.display()))
}

fn write_adjustment_registry_file(path: &Path, registry: &AdjustmentRegistryFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for adjustment registry {}",
                path.display()
            )
        })?;
    }
    let content =
        serde_json::to_string_pretty(registry).context("failed to encode adjustment registry")?;
    fs::write(path, content)
        .with_context(|| format!("failed to write adjustment registry {}", path.display()))
}

async fn resolve_statement_preview_id_for_scope(
    repo_root: &Path,
    config: &TokenBudgetConfigFile,
    budget_profile: Option<&str>,
    include_verify_events: Option<bool>,
    scope: &str,
) -> Result<String> {
    let cfg = AppConfig::from_env()?;
    let db = postgres::connect_admin(&cfg).await?;
    let report = collect_report(
        repo_root,
        &db,
        budget_profile,
        include_verify_events.unwrap_or(config.measurement.include_verify_events_by_default),
        None,
    )
    .await?;
    let statement_preview_id =
        report["token_budget_report"]["statement_export_previews"][scope]["statement_preview_id"]
            .as_str()
            .ok_or_else(|| anyhow!("statement preview id unavailable for scope {scope}"))?;
    Ok(statement_preview_id.to_string())
}

pub async fn print_adjustment_registry(args: &ObserveTokenAdjustmentRegistryArgs) -> Result<()> {
    if let Some(scope) = args.scope.as_deref() {
        validate_adjustment_scope(scope)?;
    }
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let registry = build_adjustment_registry_json(&repo_root, &config.contract);
    let payload = if let Some(scope) = args.scope.as_deref() {
        json!({
            "token_adjustment_registry": registry,
            "scope_code": scope,
            "scope_summary": registry["scopes"][scope].clone(),
            "adjustment_request_schema": build_adjustment_request_schema_json(&config.contract),
        })
    } else {
        json!({
            "token_adjustment_registry": registry,
            "adjustment_request_schema": build_adjustment_request_schema_json(&config.contract),
        })
    };
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn add_adjustment_entry(args: &ObserveTokenAdjustmentAddArgs) -> Result<()> {
    validate_adjustment_scope(&args.scope)?;
    validate_adjustment_kind(&args.kind)?;
    validate_adjustment_status(&args.status)?;
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let related_statement_id = match (
        args.related_statement_id.as_ref(),
        args.resolve_related_statement_id,
    ) {
        (Some(explicit), _) => Some(explicit.clone()),
        (None, true) => Some(
            resolve_statement_preview_id_for_scope(
                &repo_root,
                &config,
                args.budget_profile.as_deref(),
                args.include_verify_events,
                &args.scope,
            )
            .await?,
        ),
        (None, false) => None,
    };
    let path = adjustment_registry_write_path(&repo_root);
    let mut registry = load_adjustment_registry_file_for_write(&path)?;
    let entry = AdjustmentRegistryEntry {
        adjustment_id: args
            .adjustment_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        scope_code: args.scope.clone(),
        kind: args.kind.clone(),
        status: args.status.clone(),
        reason_code: args.reason_code.clone(),
        created_at_epoch_ms: current_epoch_ms()?,
        tokens_delta: args.tokens_delta,
        amount_delta: args.amount_delta,
        currency_profile: args.currency_profile.clone(),
        related_statement_id: related_statement_id.clone(),
    };
    registry.adjustments.push(entry.clone());
    write_adjustment_registry_file(&path, &registry)?;
    let registry_preview = build_adjustment_registry_json(&repo_root, &config.contract);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "token_adjustment_add": {
                "registry_path": path.display().to_string(),
                "entry": adjustment_entry_json(&entry),
                "scope_summary": registry_preview["scopes"][args.scope.as_str()].clone(),
                "registry_status": registry_preview["status"].clone(),
                "resolved_related_statement_id": related_statement_id,
                "request_schema_version": config.contract.adjustment_request_schema_version,
                "note": "Adjustment entry materialized отдельно от token events: historical usage не переписывается, а correction/dispute живёт как отдельный registry layer."
            }
        }))?
    );
    Ok(())
}

pub async fn print_report(db: &Client, args: &ObserveTokenReportArgs) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn select_scope_events(
    events: &[TokenBudgetEvent],
    profile: &ResolvedProfile,
    scope_code: &str,
    now_epoch_ms: i64,
) -> Result<(String, Vec<TokenBudgetEvent>)> {
    match scope_code {
        "current_session" => Ok((
            "текущая сессия".to_string(),
            current_session_events(
                events,
                profile.session_gap_minutes.saturating_mul(60_000) as i64,
            ),
        )),
        "rolling_window" => {
            let hours = profile
                .rolling_window_hours
                .ok_or_else(|| anyhow!("selected budget profile has no rolling window"))?;
            let lower_bound = now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
            Ok((
                format!("окно {}", profile.display_name),
                events
                    .iter()
                    .filter(|event| event.created_at_epoch_ms >= lower_bound)
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        }
        "lifetime" => Ok(("всё время записи".to_string(), events.to_vec())),
        _ => bail!("unknown scope for token export surface: {scope_code}"),
    }
}

fn build_contractual_sources_value(
    report: &Value,
    repo_root: &Path,
    scope_code: &str,
    scope_label: &str,
) -> Value {
    let external_truth_sources = if report["token_budget_report"]["external_truth_sources"]
        .is_null()
    {
        report["token_budget_report"]["reconciliation_contract"]["external_truth_sources"].clone()
    } else {
        report["token_budget_report"]["external_truth_sources"].clone()
    };
    let statement_export_preview =
        report["token_budget_report"]["statement_export_previews"][scope_code].clone();
    let mut customer_contractual_boundary =
        statement_export_preview["customer_contractual_boundary"].clone();
    customer_contractual_boundary["surface_kind"] =
        json!("customer_contractual_sources_report_only");
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "external_truth_sources": external_truth_sources,
        "external_truth_manifest": report["token_budget_report"]["external_truth_manifest"].clone(),
        "rate_card": report["token_budget_report"]["rate_card"].clone(),
        "infra_cost_profile": report["token_budget_report"]["infra_cost_profile"].clone(),
        "reconciliation_contract": report["token_budget_report"]["reconciliation_contract"].clone(),
        "provider_usage_binding": report["token_budget_report"]["reconciliation_contract"]["external_truth_bindings"]["provider_usage_export"].clone(),
        "provider_invoice_binding": report["token_budget_report"]["reconciliation_contract"]["external_truth_bindings"]["provider_invoice_export"].clone(),
        "statement_preview": report["token_budget_report"]["statement_previews"][scope_code].clone(),
        "reconciliation_preview": report["token_budget_report"]["reconciliation_previews"][scope_code].clone(),
        "margin_scope": report["token_budget_report"]["margin_view"][scope_code].clone(),
        "statement_export_preview": statement_export_preview,
        "settlement_report_preview": report["token_budget_report"]["settlement_report_previews"][scope_code].clone(),
        "settlement_activation_governance": report["token_budget_report"]["statement_export_previews"][scope_code]["settlement_activation_governance"].clone(),
        "adjustment_activation_governance": report["token_budget_report"]["statement_export_previews"][scope_code]["adjustment_activation_governance"].clone(),
        "transactional_statuses": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["transactional_statuses"].clone(),
        "customer_contractual_boundary": customer_contractual_boundary,
        "suggested_repo_local_paths": {
            "provider_usage_export": provider_usage_default_path(repo_root).display().to_string(),
            "provider_invoice_export": provider_invoice_default_path(repo_root).display().to_string(),
            "provider_rate_card": provider_rate_card_default_path(repo_root).display().to_string(),
            "infra_cost_profile": infra_cost_profile_default_path(repo_root).display().to_string(),
        },
        "note": "Этот inspect-layer нужен затем, чтобы provider truth sources, rate card, reconciliation и margin были видны как отдельный contractual contour, а не прятались внутри большого token report."
    })
}

pub async fn print_evidence_pack(db: &Client, args: &ObserveTokenEvidencePackArgs) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let profile = resolve_profile(&config, args.budget_profile.as_deref(), &repo_root)?;
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    let mut events = load_events(db, include_verify, None).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let events = reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
    let now_epoch_ms = current_epoch_ms()?;
    let scope_code = args.scope.as_str();
    let (scope_label, scoped_events) =
        select_scope_events(&events, &profile, scope_code, now_epoch_ms)?;

    let pack = build_contractual_evidence_pack(
        &report,
        scope_code,
        &scope_label,
        &scoped_events,
        &config.contract,
        &profile,
        include_verify,
        now_epoch_ms,
    )?;
    let rendered = serde_json::to_string_pretty(&pack)?;
    if let Some(path) = &args.output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, rendered).with_context(|| format!("failed to write {}", path.display()))?;
        println!("{}", path.display());
    } else {
        println!("{}", rendered);
    }
    Ok(())
}

pub async fn print_contractual_sources(
    db: &Client,
    args: &ObserveTokenContractualSourcesArgs,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    let scope_code = args.scope.as_str();
    let scope_label =
        report["token_budget_report"]["statement_previews"][scope_code]["scope_label"]
            .as_str()
            .ok_or_else(|| {
                anyhow!("unknown or unavailable scope for token contractual sources: {scope_code}")
            })?
            .to_string();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "token_contractual_sources": build_contractual_sources_value(
                &report,
                &repo_root,
                scope_code,
                &scope_label,
            )
        }))?
    );
    Ok(())
}

pub async fn print_statement_export_bundle(
    db: &Client,
    args: &ObserveTokenStatementExportArgs,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let profile = resolve_profile(&config, args.budget_profile.as_deref(), &repo_root)?;
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    let mut events = load_events(db, include_verify, None).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let events = reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
    let now_epoch_ms = current_epoch_ms()?;
    let scope_code = args.scope.as_str();
    let (scope_label, scoped_events) =
        select_scope_events(&events, &profile, scope_code, now_epoch_ms)?;
    let evidence_pack = build_contractual_evidence_pack(
        &report,
        scope_code,
        &scope_label,
        &scoped_events,
        &config.contract,
        &profile,
        include_verify,
        now_epoch_ms,
    )?;
    let contractual_sources =
        build_contractual_sources_value(&report, &repo_root, scope_code, &scope_label);
    let statement_export_preview =
        report["token_budget_report"]["statement_export_previews"][scope_code].clone();
    if statement_export_preview.is_null() {
        bail!("unknown or unavailable scope for token statement export: {scope_code}");
    }
    let mut customer_contractual_boundary =
        statement_export_preview["customer_contractual_boundary"].clone();
    customer_contractual_boundary["surface_kind"] = json!("customer_review_bundle_report_only");
    let settlement_report_preview =
        settlement_report_preview_from_export(&config.contract, &statement_export_preview);
    let bundle = json!({
        "token_statement_export_bundle": {
            "bundle_version": "token-statement-export-bundle-v3",
            "generated_at_epoch_ms": now_epoch_ms,
            "scope_code": scope_code,
            "scope_label": scope_label,
            "report_only": true,
            "statement_preview_id": statement_export_preview["statement_preview_id"].clone(),
            "files": {
                "manifest": "manifest.json",
                "settlement_report_preview": "settlement_report_preview.json",
                "statement_export_preview": "statement_export_preview.json",
                "contractual_evidence_pack": "contractual_evidence_pack.json",
                "token_contractual_sources": "token_contractual_sources.json",
            },
        "settlement_report_preview": settlement_report_preview,
        "statement_export_preview": statement_export_preview.clone(),
        "contractual_evidence_pack": evidence_pack["contractual_evidence_pack"].clone(),
        "token_contractual_sources": contractual_sources,
        "customer_contractual_boundary": customer_contractual_boundary.clone(),
        "settlement_activation_governance": statement_export_preview["settlement_activation_governance"].clone(),
        "adjustment_activation_governance": statement_export_preview["adjustment_activation_governance"].clone(),
        "surface_kind": "customer_review_bundle_report_only",
        "self_serve_state": "self_serve_ready_report_only",
        "invoice_grade": false,
        "operational_telemetry_included": false,
        "redaction_policy": "raw_query_text_removed_keep_query_hash_and_token_state",
        "note": "Этот bundle собирает customer-facing statement preview, evidence pack и contractual sources в один report-only export surface. Он пригоден для review/audit, но не для invoice."
        }
    });
    if let Some(output_dir) = &args.output_dir {
        fs::create_dir_all(output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let root = &bundle["token_statement_export_bundle"];
        let manifest = json!({
            "bundle_version": root["bundle_version"].clone(),
            "generated_at_epoch_ms": root["generated_at_epoch_ms"].clone(),
            "scope_code": root["scope_code"].clone(),
            "scope_label": root["scope_label"].clone(),
            "report_only": root["report_only"].clone(),
            "surface_kind": root["surface_kind"].clone(),
            "self_serve_state": root["self_serve_state"].clone(),
            "invoice_grade": root["invoice_grade"].clone(),
            "operational_telemetry_included": root["operational_telemetry_included"].clone(),
            "customer_contractual_boundary": root["customer_contractual_boundary"].clone(),
            "settlement_activation_governance": root["settlement_activation_governance"].clone(),
            "adjustment_activation_governance": root["adjustment_activation_governance"].clone(),
            "redaction_policy": root["redaction_policy"].clone(),
            "statement_preview_id": root["statement_preview_id"].clone(),
            "files": root["files"].clone(),
            "note": root["note"].clone(),
        });
        let files = [
            ("manifest.json", manifest),
            (
                "settlement_report_preview.json",
                root["settlement_report_preview"].clone(),
            ),
            (
                "statement_export_preview.json",
                root["statement_export_preview"].clone(),
            ),
            (
                "contractual_evidence_pack.json",
                root["contractual_evidence_pack"].clone(),
            ),
            (
                "token_contractual_sources.json",
                root["token_contractual_sources"].clone(),
            ),
        ];
        for (name, payload) in files {
            let path = output_dir.join(name);
            fs::write(&path, serde_json::to_string_pretty(&payload)?)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        println!("{}", output_dir.display());
    } else {
        println!("{}", serde_json::to_string_pretty(&bundle)?);
    }
    Ok(())
}

pub async fn repair_legacy_token_events(
    db: &Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], limit).await?;
    let mut scanned = 0_u64;
    let mut changed = 0_u64;

    for row in rows {
        scanned += 1;
        if let Some(payload) = repair_legacy_token_event_payload(&row.payload) {
            changed += 1;
            if apply {
                postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload)
                    .await?;
            }
        }
    }

    println!(
        "token ledger repair :: scanned={} changed={} mode={}",
        scanned,
        changed,
        if apply { "apply" } else { "dry_run" }
    );
    Ok(())
}

pub async fn reverify_legacy_live_events(
    cfg: &AppConfig,
    db: &mut Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], limit).await?;
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let measurement = config.measurement.clone();
    let contract = config.contract.clone();
    let mut scanned = 0_u64;
    let mut eligible = 0_u64;
    let mut reverified = 0_u64;
    let mut quality_ok = 0_u64;
    let mut skipped = 0_u64;
    let mut failed = 0_u64;

    for row in rows {
        scanned += 1;
        if !needs_live_reverification(&row.payload) {
            skipped += 1;
            continue;
        }
        eligible += 1;

        match reverify_live_event_payload(cfg, db, &measurement, &contract, &row).await {
            Ok(Some(payload)) => {
                let node = &payload["token_budget_event"];
                if node["quality"]["quality_ok"].as_bool().unwrap_or(false) {
                    quality_ok += 1;
                }
                reverified += 1;
                if apply {
                    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload)
                        .await?;
                }
            }
            Ok(None) => {
                skipped += 1;
            }
            Err(error) => {
                failed += 1;
                eprintln!(
                    "token ledger reverify failed: snapshot={} :: {}",
                    row.snapshot_id, error
                );
            }
        }
    }

    println!(
        "token ledger reverify :: scanned={} eligible={} reverified={} quality_ok={} skipped={} failed={} mode={}",
        scanned,
        eligible,
        reverified,
        quality_ok,
        skipped,
        failed,
        if apply { "apply" } else { "dry_run" }
    );
    Ok(())
}

pub async fn collect_default_report(db: &Client) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    collect_report(
        &repo_root,
        db,
        None,
        config.measurement.include_verify_events_by_default,
        None,
    )
    .await
}

pub async fn collect_default_report_with_overrides(
    db: &Client,
    requested_profile: Option<&str>,
    include_verify_events: Option<bool>,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    collect_report(
        &repo_root,
        db,
        requested_profile,
        include_verify_events.unwrap_or(config.measurement.include_verify_events_by_default),
        None,
    )
    .await
}

pub async fn record_context_pack_event(
    db: &Client,
    payload: &Value,
    source_kind: &str,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let traffic_class = derive_traffic_class(source_kind);
    let payload_origin = if traffic_class == "live" {
        "context_pack_token_budget"
    } else {
        source_kind
    };
    let mut event = build_event_payload(
        payload,
        &config.measurement,
        &config.contract,
        source_kind,
        payload_origin,
    )?;
    if traffic_class == "live" {
        let profile = resolve_profile(&config, None, &repo_root)?;
        enrich_live_event_payload(db, &mut event, &profile).await?;
    }
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}

pub async fn observe_context_pack_tool_overhead(
    db: &Client,
    context_pack_id: &str,
    text: &str,
    structured_content: &Value,
) -> Result<bool> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let tool_overhead_tokens =
        count_tool_overhead_tokens(&config.measurement, text, structured_content)?;
    Ok(attach_context_pack_whole_cycle_observed(
        db,
        context_pack_id,
        None,
        None,
        Some(tool_overhead_tokens),
        None,
    )
    .await?
    .is_some())
}

pub async fn observe_cli_context_pack_tool_overhead(
    db: &Client,
    context_pack_id: &str,
    output_json: &str,
) -> Result<bool> {
    let output_json = output_json.trim();
    if output_json.is_empty() {
        return Ok(false);
    }
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let Some(row) = latest_token_budget_snapshot_for_context_pack(db, context_pack_id).await?
    else {
        return Ok(false);
    };
    let delivered_tokens = row.payload["token_budget_event"]["context_pack_render"]["tokens"]
        .as_u64()
        .or_else(|| row.payload["token_budget_event"]["delivered_tokens"].as_u64())
        .unwrap_or(0);
    let tool_overhead_tokens = count_cli_context_pack_output_overhead_tokens(
        &config.measurement,
        output_json,
        delivered_tokens,
    )?;
    Ok(attach_context_pack_whole_cycle_observed(
        db,
        context_pack_id,
        None,
        None,
        Some(tool_overhead_tokens),
        None,
    )
    .await?
    .is_some_and(|value| {
        value["whole_cycle_observed_attach"]["attached"]
            .as_bool()
            .unwrap_or(false)
    }))
}

pub async fn attach_whole_cycle_observed_to_context_pack(
    db: &Client,
    context_pack_id: &str,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) -> Result<Value> {
    let Some(result) = attach_context_pack_whole_cycle_observed(
        db,
        context_pack_id,
        client_prompt_tokens,
        assistant_generation_tokens,
        tool_overhead_tokens,
        continuity_restore_tokens,
    )
    .await?
    else {
        bail!("token_budget_event not found for context_pack_id={context_pack_id}");
    };
    Ok(result)
}

pub async fn attach_whole_cycle_observed_for_context_pack(
    db: &Client,
    args: &ObserveTokenWholeCycleAttachArgs,
) -> Result<()> {
    let payload = attach_whole_cycle_observed_to_context_pack(
        db,
        &args.context_pack_id,
        args.client_prompt_tokens,
        args.assistant_generation_tokens,
        args.tool_overhead_tokens,
        args.continuity_restore_tokens,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn observe_rollout_assistant_generation(
    db: &Client,
    args: &ObserveTokenRolloutAssistantGenerationArgs,
) -> Result<()> {
    let repo_root = if let Some(path) = args.repo_root.as_ref() {
        path.clone()
    } else {
        config::discover_repo_root(None)?
    };
    let repo_root_str = repo_root
        .to_str()
        .ok_or_else(|| anyhow!("repo_root must be valid UTF-8"))?;
    let observation = codex_threads::latest_rollout_assistant_generation_observation(
        repo_root_str,
        args.rollout_path.as_deref(),
    )?
    .ok_or_else(|| {
        anyhow!(
            "no unambiguous rollout assistant-generation observation found for repo_root={}",
            repo_root.display()
        )
    })?;
    let attach = if args.apply {
        Some(
            attach_whole_cycle_observed_to_context_pack(
                db,
                &observation.context_pack_id,
                None,
                Some(observation.assistant_generation_tokens),
                None,
                None,
            )
            .await?,
        )
    } else {
        None
    };
    let payload = json!({
        "rollout_assistant_generation_observation": {
            "repo_root": repo_root.display().to_string(),
            "apply_requested": args.apply,
            "applied": attach.is_some(),
            "candidate": observation,
            "attach_result": attach,
        }
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn rollout_assistant_generation_observations_for_repo(
    repo_root: &Path,
) -> Result<Vec<codex_threads::RolloutAssistantGenerationObservation>> {
    let Some(repo_root_str) = repo_root.to_str() else {
        return Ok(Vec::new());
    };
    codex_threads::rollout_assistant_generation_observations(repo_root_str, None)
}

async fn sync_rollout_assistant_generation_for_events(
    db: &Client,
    events: &[TokenBudgetEvent],
    observations: &[codex_threads::RolloutAssistantGenerationObservation],
) -> Result<bool> {
    let target_context_pack_ids = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event.assistant_generation_tokens.is_none()
        })
        .map(|event| event.correlation_id.clone())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    if target_context_pack_ids.is_empty() {
        return Ok(false);
    }
    if observations.is_empty() {
        return Ok(false);
    }
    let latest_rows = latest_token_budget_snapshots_for_context_packs(db, &target_context_pack_ids).await?;
    let mut changed = false;
    for observation in observations {
        if !target_context_pack_ids.contains(&observation.context_pack_id) {
            continue;
        }
        let Some(row) = latest_rows.get(&observation.context_pack_id) else {
            continue;
        };
        let existing = row.payload["token_budget_event"]["whole_cycle_observed"]
            ["assistant_generation_tokens"]
            .as_u64();
        match existing {
            Some(tokens) if tokens == observation.assistant_generation_tokens => {}
            Some(_) => {}
            None => {
                let attached = attach_whole_cycle_observed_to_snapshot(
                    db,
                    row,
                    Some(json!({ "context_pack_id": observation.context_pack_id })),
                    None,
                    Some(observation.assistant_generation_tokens),
                    None,
                    None,
                )
                .await?;
                if attached
                    .as_ref()
                    .and_then(|value| value["whole_cycle_observed_attach"]["attached"].as_bool())
                    .unwrap_or(false)
                {
                    changed = true;
                }
            }
        }
    }
    Ok(changed)
}

async fn sync_context_pack_tool_overhead_for_events(
    db: &Client,
    repo_root: &Path,
    events: &[TokenBudgetEvent],
) -> Result<bool> {
    let target_context_pack_ids = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event.tool_overhead_tokens.is_none()
        })
        .map(|event| event.correlation_id.clone())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    if target_context_pack_ids.is_empty() {
        return Ok(false);
    }
    let config = load_config(repo_root)?;
    let latest_rows = latest_token_budget_snapshots_for_context_packs(db, &target_context_pack_ids).await?;
    let mut changed = false;
    for context_pack_id in target_context_pack_ids {
        let Some(row) = latest_rows.get(&context_pack_id) else {
            continue;
        };
        let existing = row.payload["token_budget_event"]["whole_cycle_observed"]["tool_overhead_tokens"]
            .as_u64();
        if existing.is_some() {
            continue;
        }
        let delivered_tokens = row.payload["token_budget_event"]["context_pack_render"]["tokens"]
            .as_u64()
            .or_else(|| row.payload["token_budget_event"]["delivered_tokens"].as_u64())
            .unwrap_or(0);
        let Some(output_json) = stored_context_pack_payload_json(db, &context_pack_id).await?
        else {
            continue;
        };
        let tool_overhead_tokens = count_cli_context_pack_output_overhead_tokens(
            &config.measurement,
            &output_json,
            delivered_tokens,
        )?;
        let attached = attach_whole_cycle_observed_to_snapshot(
            db,
            row,
            Some(json!({ "context_pack_id": context_pack_id })),
            None,
            None,
            Some(tool_overhead_tokens),
            None,
        )
        .await?;
        if attached
            .as_ref()
            .and_then(|value| value["whole_cycle_observed_attach"]["attached"].as_bool())
            .unwrap_or(false)
        {
            changed = true;
        }
    }
    Ok(changed)
}

async fn latest_working_state_context_pack_metadata(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, WorkingStateContextPackMeta>> {
    if context_pack_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut rows =
        postgres::list_observability_snapshots_by_kinds(db, &["working_state_event"], Some(4096))
            .await?;
    rows.sort_by_key(|row| row.created_at_epoch_ms);
    let mut metadata = BTreeMap::new();
    for row in rows.into_iter().rev() {
        let node = &row.payload["working_state_event"];
        if node["event_kind"].as_str() != Some("retrieval_context_pack") {
            continue;
        }
        let Some(context_pack_id) = node["context_pack_id"].as_str() else {
            continue;
        };
        if !context_pack_ids.contains(context_pack_id) || metadata.contains_key(context_pack_id) {
            continue;
        }
        let thread_id = node["thread_id"].as_str().unwrap_or_default().to_string();
        if thread_id.is_empty() {
            continue;
        }
        metadata.insert(
            context_pack_id.to_string(),
            WorkingStateContextPackMeta {
                thread_id,
                captured_at_epoch_ms: row.created_at_epoch_ms,
            },
        );
        if metadata.len() == context_pack_ids.len() {
            break;
        }
    }
    Ok(metadata)
}

async fn derive_rollout_assistant_generation_scope(
    db: &Client,
    events: &[TokenBudgetEvent],
) -> Result<AssistantGenerationScopeObservation> {
    let target_context_pack_ids = assistant_generation_missing_scope_context_pack_ids(Some(events));
    if target_context_pack_ids.is_empty() {
        return Ok(AssistantGenerationScopeObservation::default());
    }

    let metadata = latest_working_state_context_pack_metadata(db, &target_context_pack_ids).await?;
    let mut matched_context_pack_ids = BTreeSet::new();
    let mut matched_turn_ids = BTreeSet::new();
    let mut observed_tokens = 0_u64;
    let mut observed_group_count = 0_u64;
    let mut grouped_context_pack_ids = BTreeMap::<(String, String), BTreeSet<String>>::new();
    let mut turns_by_thread =
        BTreeMap::<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>::new();

    let mut by_thread = BTreeMap::<String, Vec<(String, WorkingStateContextPackMeta)>>::new();
    for context_pack_id in &target_context_pack_ids {
        if let Some(meta) = metadata.get(context_pack_id) {
            by_thread
                .entry(meta.thread_id.clone())
                .or_default()
                .push((context_pack_id.clone(), meta.clone()));
        }
    }

    for (thread_id, entries) in by_thread {
        let turns =
            codex_threads::rollout_assistant_generation_turn_observations_for_thread(&thread_id)?;
        if turns.is_empty() {
            continue;
        }
        for (context_pack_id, meta) in entries {
            let matched_turn = turns.iter().find(|turn| {
                turn.started_at_epoch_ms
                    .saturating_sub(ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS)
                    <= meta.captured_at_epoch_ms
                    && meta.captured_at_epoch_ms
                        <= turn
                            .ended_at_epoch_ms
                            .saturating_add(ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS)
            });
            let Some(turn) = matched_turn else {
                continue;
            };
            grouped_context_pack_ids
                .entry((thread_id.clone(), turn.turn_id.clone()))
                .or_default()
                .insert(context_pack_id);
        }
        turns_by_thread.insert(thread_id, turns);
    }

    let available_turns = turns_by_thread
        .values()
        .map(|turns| turns.len() as u64)
        .sum();

    for ((thread_id, turn_id), context_pack_ids) in &grouped_context_pack_ids {
        let Some(turns) = turns_by_thread.get(thread_id) else {
            continue;
        };
        let Some(turn) = turns.iter().find(|candidate| candidate.turn_id == *turn_id) else {
            continue;
        };
        if context_pack_ids.is_empty() {
            continue;
        }
        matched_turn_ids.insert(format!("{thread_id}:{turn_id}"));
        matched_context_pack_ids.extend(context_pack_ids.iter().cloned());
        observed_group_count = observed_group_count.saturating_add(1);
        observed_tokens = observed_tokens.saturating_add(turn.assistant_generation_tokens);
    }

    let unmatched_context_pack_ids = target_context_pack_ids
        .difference(&matched_context_pack_ids)
        .cloned()
        .collect::<BTreeSet<_>>();

    Ok(AssistantGenerationScopeObservation {
        target_group_count: grouped_context_pack_ids.len() as u64
            + unmatched_context_pack_ids.len() as u64,
        observed_group_count,
        observed_tokens,
        target_context_pack_ids,
        matched_context_pack_ids,
        unmatched_context_pack_ids,
        matched_turn_ids,
        available_turns,
    })
}

pub async fn record_continuity_restore_observed_event(
    db: &Client,
    project_code: &str,
    namespace_code: &str,
    prompt_text: &str,
    source_kind: &str,
) -> Result<()> {
    let prompt_text = prompt_text.trim();
    if prompt_text.is_empty() {
        return Ok(());
    }

    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let tokenizer = build_tokenizer(&config.measurement.tokenizer)?;
    let continuity_restore_tokens = tokenizer.encode_with_special_tokens(prompt_text).len() as u64;
    let traffic_class = derive_traffic_class(source_kind);
    let mut event = build_continuity_restore_observed_event(
        project_code,
        namespace_code,
        source_kind,
        &config.measurement,
        &config.contract,
        prompt_text,
        continuity_restore_tokens,
    )?;
    if traffic_class == "live" {
        let profile = resolve_profile(&config, None, &repo_root)?;
        enrich_live_event_payload(db, &mut event, &profile).await?;
    }
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}

pub async fn record_verify_context_pack_event(db: &Client, payload: &Value) -> Result<()> {
    record_context_pack_event(db, payload, "verify_context_pack").await
}

pub async fn record_verify_benchmark_event(db: &Client, benchmark_payload: &Value) -> Result<()> {
    let benchmark = benchmark_payload
        .get("token_benchmark")
        .cloned()
        .ok_or_else(|| anyhow!("token benchmark payload missing token_benchmark root"))?;
    let repo_root = config::discover_repo_root(None)?;
    let contract = load_config(&repo_root)?.contract;
    let timestamp_utc = current_epoch_ms()?;
    let event = json!({
        "token_budget_event": {
            "event_id": Uuid::new_v4(),
            "correlation_id": benchmark["context_pack_id"].clone(),
            "timestamp_utc": timestamp_utc,
            "occurred_at_epoch_ms": timestamp_utc,
            "ingested_at_epoch_ms": timestamp_utc,
            "source_kind": "verify_token_benchmark",
            "traffic_class": "verify",
            "measurement_scope": "retrieval_lower_bound",
            "payload_origin": "verify_token_benchmark",
            "contract": token_contract_metadata_json(&contract),
            "project": benchmark["project"].clone(),
            "namespace": benchmark["namespace"].clone(),
            "query": benchmark["query"].clone(),
            "query_hash": hex_sha256(benchmark["query"].as_str().unwrap_or_default().as_bytes()),
            "query_type": "unknown",
            "cold_warm_state": "benchmark",
            "baseline_strategy": "naive_top_files",
            "retrieval_mode": benchmark["retrieval_mode"].clone(),
            "tokenizer": benchmark["tokenizer"].clone(),
            "naive_limit_files": benchmark["naive_limit_files"].clone(),
            "naive_max_bytes_per_file": benchmark["naive_max_bytes_per_file"].clone(),
            "visible_projects": benchmark["visible_projects"].clone(),
            "naive_scope": benchmark["naive_scope"].clone(),
            "context_pack_render": benchmark["context_pack_render"].clone(),
            "recovery": {
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0,
            },
            "quality": {
                "quality_ok": true,
                "quality_score": 1.0,
                "quality_method": "benchmark_assumption",
                "quality_tier": "benchmark",
                "head_hit_target": true,
            },
            "shape": {
                "sources_count": 0,
                "chunks_count": 0,
            },
            "savings": benchmark["savings"].clone()
        }
    });
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}

async fn collect_report(
    repo_root: &Path,
    db: &Client,
    requested_profile: Option<&str>,
    include_verify_events: bool,
    limit: Option<i64>,
) -> Result<Value> {
    let config = load_config(repo_root)?;
    let profile = resolve_profile(&config, requested_profile, repo_root)?;
    let rollout_observations = rollout_assistant_generation_observations_for_repo(repo_root)?;
    let mut events = load_events(db, include_verify_events, limit).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let mut events =
        reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let session_gap_ms = profile.session_gap_minutes.saturating_mul(60_000) as i64;
    let mut session_events = current_session_events(&events, session_gap_ms);
    let mut rolling_window_events = profile
        .rolling_window_hours
        .map(|hours| {
            let lower_bound = now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
            events
                .iter()
                .filter(|event| event.created_at_epoch_ms >= lower_bound)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let sync_scope_events = active_same_meter_scope_events(&session_events, &rolling_window_events);
    let tool_overhead_changed =
        sync_context_pack_tool_overhead_for_events(db, repo_root, &sync_scope_events).await?;
    let assistant_generation_changed =
        sync_rollout_assistant_generation_for_events(db, &sync_scope_events, &rollout_observations)
            .await?;
    if tool_overhead_changed || assistant_generation_changed {
        let mut refreshed = load_events(db, include_verify_events, limit).await?;
        refreshed.sort_by_key(|event| event.created_at_epoch_ms);
        events =
            reconcile_followup_recovery(&refreshed, profile.session_gap_minutes as i64 * 60_000);
        session_events = current_session_events(&events, session_gap_ms);
        rolling_window_events = profile
            .rolling_window_hours
            .map(|hours| {
                let lower_bound =
                    now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
                events
                    .iter()
                    .filter(|event| event.created_at_epoch_ms >= lower_bound)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    }

    let current_session_assistant_scope =
        derive_rollout_assistant_generation_scope(db, &session_events).await?;
    let rolling_window_assistant_scope = if profile.rolling_window_hours.is_some() {
        Some(derive_rollout_assistant_generation_scope(db, &rolling_window_events).await?)
    } else {
        None
    };

    let latest_event = events
        .last()
        .map(event_to_json)
        .unwrap_or_else(|| json!(null));
    let source_breakdown = source_breakdown(&events, &config.measurement, &config.contract);
    let query_slices = query_slice_breakdown(&events, &config.measurement, &config.contract);
    let baseline_strategy_slices =
        baseline_strategy_breakdown(&events, &config.measurement, &config.contract);
    let temperature_slices =
        temperature_slice_breakdown(&events, &config.measurement, &config.contract);
    let current_session_summary = summarize_events(
        &session_events,
        now_epoch_ms,
        &config.measurement,
        &config.contract,
    );
    let rolling_window_summary = if profile.rolling_window_hours.is_some() {
        summarize_events(
            &rolling_window_events,
            now_epoch_ms,
            &config.measurement,
            &config.contract,
        )
    } else {
        json!(null)
    };
    let lifetime_summary =
        summarize_events(&events, now_epoch_ms, &config.measurement, &config.contract);
    let headline_summary = if profile.rolling_window_hours.is_some() {
        build_product_headline(
            &rolling_window_summary,
            &format!("окно {}", profile.display_name),
        )
    } else {
        build_product_headline(&lifetime_summary, "всё время записи")
    };
    let agent_cycle_economics = build_agent_cycle_economics(
        &config.measurement,
        &config.contract,
        now_epoch_ms,
        &session_events,
        profile
            .rolling_window_hours
            .map(|_| rolling_window_events.as_slice()),
        &events,
        &profile.display_name,
        &rollout_observations,
        &current_session_assistant_scope,
        rolling_window_assistant_scope.as_ref(),
        None,
    );
    let current_session_metering_freshness = build_metering_freshness_summary(
        &config.contract,
        &config.measurement,
        now_epoch_ms,
        &session_events,
    );
    let rolling_window_metering_freshness = if profile.rolling_window_hours.is_some() {
        build_metering_freshness_summary(
            &config.contract,
            &config.measurement,
            now_epoch_ms,
            &rolling_window_events,
        )
    } else {
        Value::Null
    };
    let lifetime_metering_freshness = build_metering_freshness_summary(
        &config.contract,
        &config.measurement,
        now_epoch_ms,
        &events,
    );
    let external_truth_sources = build_external_truth_sources_json(repo_root);
    let rate_card = build_rate_card_json(repo_root, &config.contract);
    let provider_usage_binding = load_provider_usage_binding_from_source(
        &external_truth_sources["provider_usage_export"],
        &rate_card,
    );
    let provider_invoice_binding = load_provider_invoice_binding_from_source(
        &external_truth_sources["provider_invoice_export"],
    );
    let reconciliation_contract = build_reconciliation_contract_json(
        &config.contract,
        &external_truth_sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let adjustment_registry = build_adjustment_registry_json(repo_root, &config.contract);
    let infra_cost_profile = build_infra_cost_profile_json(repo_root, &config.contract);
    let current_session_statement_preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        now_epoch_ms,
        &session_events,
        &profile,
        &current_session_summary,
        &config.contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &current_session_metering_freshness,
        &rollout_observations,
        Some(&current_session_assistant_scope),
    );
    let rolling_window_statement_preview = if profile.rolling_window_hours.is_some() {
        build_statement_preview(
            "rolling_window",
            &format!("окно {}", profile.display_name),
            now_epoch_ms,
            &rolling_window_events,
            &profile,
            &rolling_window_summary,
            &config.contract,
            &adjustment_registry,
            &rate_card,
            &reconciliation_contract,
            &rolling_window_metering_freshness,
            &rollout_observations,
            rolling_window_assistant_scope.as_ref(),
        )
    } else {
        Value::Null
    };
    let lifetime_statement_preview = build_statement_preview(
        "lifetime",
        "всё время записи",
        now_epoch_ms,
        &events,
        &profile,
        &lifetime_summary,
        &config.contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &lifetime_metering_freshness,
        &rollout_observations,
        None,
    );
    let current_session_reconciliation_preview = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &current_session_statement_preview,
        &config.contract,
        &external_truth_sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let rolling_window_reconciliation_preview = if profile.rolling_window_hours.is_some() {
        build_reconciliation_preview(
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_statement_preview,
            &config.contract,
            &external_truth_sources,
            &provider_usage_binding,
            &provider_invoice_binding,
            &rate_card,
        )
    } else {
        Value::Null
    };
    let lifetime_reconciliation_preview = build_reconciliation_preview(
        "lifetime",
        "всё время записи",
        &lifetime_statement_preview,
        &config.contract,
        &external_truth_sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let margin_contract = build_margin_contract_json(
        &config.contract,
        &external_truth_sources,
        &rate_card,
        &infra_cost_profile,
        &reconciliation_contract,
    );
    let current_session_margin_scope = build_margin_scope(
        &external_truth_sources,
        "current_session",
        "текущая сессия",
        &current_session_statement_preview,
        &current_session_reconciliation_preview,
        &rate_card,
        &infra_cost_profile,
    );
    let rolling_window_margin_scope = if profile.rolling_window_hours.is_some() {
        build_margin_scope(
            &external_truth_sources,
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_statement_preview,
            &rolling_window_reconciliation_preview,
            &rate_card,
            &infra_cost_profile,
        )
    } else {
        Value::Null
    };
    let lifetime_margin_scope = build_margin_scope(
        &external_truth_sources,
        "lifetime",
        "всё время записи",
        &lifetime_statement_preview,
        &lifetime_reconciliation_preview,
        &rate_card,
        &infra_cost_profile,
    );
    let current_session_contractual_summary = build_contractual_statement_summary(
        &config.contract,
        "current_session",
        "текущая сессия",
        &current_session_statement_preview,
        &current_session_reconciliation_preview,
        &current_session_margin_scope,
        &current_session_metering_freshness,
    );
    let rolling_window_contractual_summary = if profile.rolling_window_hours.is_some() {
        build_contractual_statement_summary(
            &config.contract,
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_statement_preview,
            &rolling_window_reconciliation_preview,
            &rolling_window_margin_scope,
            &rolling_window_metering_freshness,
        )
    } else {
        Value::Null
    };
    let lifetime_contractual_summary = build_contractual_statement_summary(
        &config.contract,
        "lifetime",
        "всё время записи",
        &lifetime_statement_preview,
        &lifetime_reconciliation_preview,
        &lifetime_margin_scope,
        &lifetime_metering_freshness,
    );
    let external_truth_manifest = build_external_truth_manifest(
        &config.contract,
        &rate_card,
        &infra_cost_profile,
        &provider_usage_binding,
        &provider_invoice_binding,
        &adjustment_registry,
    );
    let current_session_statement_export = build_statement_export_preview(
        &json!({
            "token_budget_report": {
                "external_truth_manifest": external_truth_manifest.clone(),
                "statement_previews": {
                    "current_session": current_session_statement_preview.clone(),
                },
                "reconciliation_previews": {
                    "current_session": current_session_reconciliation_preview.clone(),
                },
                "margin_view": {
                    "current_session": current_session_margin_scope.clone(),
                },
                "contractual_statement_summaries": {
                    "current_session": current_session_contractual_summary.clone(),
                }
            }
        }),
        "current_session",
        "текущая сессия",
        &session_events,
        &config.contract,
        include_verify_events,
    )?;
    let rolling_window_statement_export = if profile.rolling_window_hours.is_some() {
        build_statement_export_preview(
            &json!({
                "token_budget_report": {
                    "external_truth_manifest": external_truth_manifest.clone(),
                    "statement_previews": {
                        "rolling_window": rolling_window_statement_preview.clone(),
                    },
                    "reconciliation_previews": {
                        "rolling_window": rolling_window_reconciliation_preview.clone(),
                    },
                    "margin_view": {
                        "rolling_window": rolling_window_margin_scope.clone(),
                    },
                    "contractual_statement_summaries": {
                        "rolling_window": rolling_window_contractual_summary.clone(),
                    }
                }
            }),
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_events,
            &config.contract,
            include_verify_events,
        )?
    } else {
        Value::Null
    };
    let lifetime_statement_export = build_statement_export_preview(
        &json!({
            "token_budget_report": {
                "external_truth_manifest": external_truth_manifest.clone(),
                "statement_previews": {
                    "lifetime": lifetime_statement_preview.clone(),
                },
                "reconciliation_previews": {
                    "lifetime": lifetime_reconciliation_preview.clone(),
                },
                "margin_view": {
                    "lifetime": lifetime_margin_scope.clone(),
                },
                "contractual_statement_summaries": {
                    "lifetime": lifetime_contractual_summary.clone(),
                }
            }
        }),
        "lifetime",
        "всё время записи",
        &events,
        &config.contract,
        include_verify_events,
    )?;
    let current_session_settlement_report_preview =
        current_session_statement_export["settlement_report_preview"].clone();
    let rolling_window_settlement_report_preview = if profile.rolling_window_hours.is_some() {
        rolling_window_statement_export["settlement_report_preview"].clone()
    } else {
        Value::Null
    };
    let lifetime_settlement_report_preview =
        lifetime_statement_export["settlement_report_preview"].clone();

    Ok(json!({
        "token_budget_report": {
            "profile": {
                "code": profile.code,
                "display_name": profile.display_name,
                "description": profile.description,
                "session_gap_minutes": profile.session_gap_minutes,
                "rolling_window_hours": profile.rolling_window_hours,
                "metering_ingest_warning_seconds": config.measurement.metering_ingest_warning_seconds,
                "metering_ingest_slo_seconds": config.measurement.metering_ingest_slo_seconds,
                "late_arrival_grace_minutes": config.measurement.late_arrival_grace_minutes,
                "preliminary_min_events": config.measurement.preliminary_min_events,
                "preliminary_min_baseline_tokens": config.measurement.preliminary_min_baseline_tokens,
            },
            "contract": report_contract_json(&config.contract),
            "usage_event_schema": build_usage_event_schema_json(&config.contract),
            "metering_freshness_contract": build_metering_freshness_contract_json(&config.contract, &config.measurement),
            "baseline_contract": build_baseline_contract_json(&config.contract),
            "billing_policy": build_billing_policy_json(&config.contract, &config.measurement),
            "suitability_contract": build_suitability_contract_json(&config.contract),
            "rate_card": rate_card.clone(),
            "settlement_contract": build_settlement_contract_json(&config.contract),
            "telemetry_surfaces": build_telemetry_surfaces_json(&config.contract),
            "adjustment_request_schema": build_adjustment_request_schema_json(&config.contract),
            "adjustment_registry": adjustment_registry.clone(),
            "reconciliation_contract": reconciliation_contract.clone(),
            "external_truth_sources": external_truth_sources.clone(),
            "external_truth_manifest": external_truth_manifest,
            "provider_usage_binding": provider_usage_binding.clone(),
            "provider_invoice_binding": provider_invoice_binding.clone(),
            "infra_cost_profile": infra_cost_profile.clone(),
            "margin_contract": margin_contract.clone(),
            "filters": {
                "include_verify_events": include_verify_events,
            },
            "headline": headline_summary,
            "latest_event": latest_event,
            "current_session": current_session_summary,
            "rolling_window": rolling_window_summary,
            "lifetime": lifetime_summary,
            "agent_cycle_economics": agent_cycle_economics,
            "metering_freshness": {
                "current_session": current_session_metering_freshness.clone(),
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_metering_freshness.clone()
                } else {
                    Value::Null
                },
                "lifetime": lifetime_metering_freshness.clone(),
            },
            "statement_previews": {
                "current_session": current_session_statement_preview.clone(),
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_statement_preview.clone()
                } else {
                    Value::Null
                },
                "lifetime": lifetime_statement_preview.clone(),
            },
            "reconciliation_previews": {
                "current_session": current_session_reconciliation_preview.clone(),
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_reconciliation_preview.clone()
                } else {
                    Value::Null
                },
                "lifetime": lifetime_reconciliation_preview.clone(),
            },
            "margin_view": {
                "model_version": config.contract.margin_model_version.clone(),
                "status": margin_contract["status"].clone(),
                "current_session": current_session_margin_scope.clone(),
                "rolling_window": rolling_window_margin_scope.clone(),
                "lifetime": lifetime_margin_scope.clone(),
            },
            "contractual_statement_summaries": {
                "current_session": current_session_contractual_summary,
                "rolling_window": rolling_window_contractual_summary,
                "lifetime": lifetime_contractual_summary,
            },
            "statement_export_previews": {
                "current_session": current_session_statement_export,
                "rolling_window": rolling_window_statement_export,
                "lifetime": lifetime_statement_export,
            },
            "settlement_report_previews": {
                "current_session": current_session_settlement_report_preview,
                "rolling_window": rolling_window_settlement_report_preview,
                "lifetime": lifetime_settlement_report_preview,
            },
            "source_breakdown": source_breakdown,
            "query_slices": query_slices,
            "baseline_strategy_slices": baseline_strategy_slices,
            "temperature_slices": temperature_slices,
        }
    }))
}

async fn enrich_live_event_payload(
    db: &Client,
    payload: &mut Value,
    profile: &ResolvedProfile,
) -> Result<()> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
    let timestamp_utc = node
        .get("timestamp_utc")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| current_epoch_ms().unwrap_or_default());
    let current_event_id = node
        .get("event_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let project = node
        .get("project")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let namespace = node
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query = node
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_hash = node
        .get("query_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_type = node
        .get("query_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let target_kind = node
        .get("target_kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let session_gap_ms = profile.session_gap_minutes as i64 * 60_000;
    let mut events = load_events(db, false, Some(64)).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let session_id = resolve_session_id(&events, timestamp_utc, session_gap_ms);
    node.insert("session_id".to_string(), Value::String(session_id));
    node.insert(
        "rolling_window_profile".to_string(),
        Value::String(profile.code.clone()),
    );
    node.insert(
        "budget_profile".to_string(),
        Value::String(profile.code.clone()),
    );

    let current_key = FollowupEventKey {
        query: &query,
        query_hash: &query_hash,
        query_type: &query_type,
        target_kind: &target_kind,
    };

    let candidate_rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], Some(64))
            .await?;
    let mut candidates = candidate_rows
        .into_iter()
        .filter_map(|row| {
            parse_snapshot_event(&row)
                .ok()
                .flatten()
                .filter(|event| event.traffic_class == "live")
                .map(|event| (row, event))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, event)| event.created_at_epoch_ms);

    if let Some((row, previous)) = candidates.into_iter().rev().find(|(_, previous)| {
        previous.traffic_class == "live"
            && previous.needed_followup
            && previous.resolved_by_event_id.is_none()
            && previous.project == project
            && previous.namespace == namespace
            && timestamp_utc.saturating_sub(previous.created_at_epoch_ms) <= session_gap_ms
            && followup_queries_related(followup_event_key(previous), current_key)
    }) {
        let previous_cost = previous
            .context_tokens
            .saturating_add(previous.recovery_tokens);
        set_recovery_penalty(
            payload,
            previous_cost,
            previous.followup_count.saturating_add(1),
        )?;
        let exact_hits = payload["retrieval"]["exact_documents"]
            .as_array()
            .map_or(0, Vec::len);
        let symbol_hits = payload["retrieval"]["symbol_hits"]
            .as_array()
            .map_or(0, Vec::len);
        let lexical_hits = payload["retrieval"]["lexical_chunks"]
            .as_array()
            .map_or(0, Vec::len);
        let semantic_hits = payload["retrieval"]["semantic_chunks"]
            .as_array()
            .map_or(0, Vec::len);
        let target_kind_owned = payload["token_budget_event"]["target_kind"]
            .as_str()
            .unwrap_or("file")
            .to_string();
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
        let followup = ensure_nested_object(node, "followup")?;
        followup.insert(
            "followup_of_event_id".to_string(),
            Value::String(previous.event_id.clone()),
        );
        let quality = ensure_nested_object(node, "quality")?;
        let quality_ok = quality
            .get("quality_ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let head_hit_target = quality
            .get("head_hit_target")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer_like_proxy = answer_like_from_counts(
            &target_kind_owned,
            head_hit_target,
            exact_hits,
            symbol_hits,
            lexical_hits,
            semantic_hits,
        );
        quality.insert(
            "quality_method".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "hybrid_answer_success".to_string()
                } else {
                    "hybrid_task_success".to_string()
                }
            } else {
                "hybrid_followup_pending".to_string()
            }),
        );
        quality.insert(
            "quality_tier".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "answer_success_recovered".to_string()
                } else {
                    "task_success_recovered".to_string()
                }
            } else {
                "partial".to_string()
            }),
        );

        let mut previous_payload = row.payload.clone();
        let previous_node = previous_payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("previous token budget payload missing token_budget_event"))?;
        let previous_followup = ensure_nested_object(previous_node, "followup")?;
        previous_followup.insert(
            "resolved_by_event_id".to_string(),
            Value::String(current_event_id),
        );
        previous_followup.insert("recovery_resolved".to_string(), Value::Bool(true));
        previous_followup.insert(
            "recovery_resolved_at_utc".to_string(),
            Value::from(timestamp_utc),
        );
        postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &previous_payload)
            .await?;
    }

    Ok(())
}

fn load_config(repo_root: &Path) -> Result<TokenBudgetConfigFile> {
    let path = repo_root.join(CONFIG_RELATIVE_PATH);
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn resolve_profile(
    config: &TokenBudgetConfigFile,
    requested_profile: Option<&str>,
    repo_root: &Path,
) -> Result<ResolvedProfile> {
    let install_state_path = repo_root.join("state/install_state.json");
    let install_state_client = fs::read_to_string(&install_state_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value["client_key"].as_str().map(ToOwned::to_owned));
    let profile_code = if let Some(requested) = requested_profile {
        requested.to_string()
    } else if let Ok(from_env) = std::env::var("AMAI_TOKEN_BUDGET_PROFILE") {
        from_env
    } else if let Some(client_key) = install_state_client {
        config
            .client_budget_overrides
            .get(&client_key)
            .cloned()
            .unwrap_or_else(|| config.default_profile.clone())
    } else {
        config.default_profile.clone()
    };
    let profile = config
        .profiles
        .get(&profile_code)
        .ok_or_else(|| anyhow!("unknown token budget profile: {profile_code}"))?;
    Ok(ResolvedProfile {
        code: profile_code,
        display_name: profile.display_name.clone(),
        description: profile.description.clone(),
        session_gap_minutes: profile.session_gap_minutes,
        rolling_window_hours: profile.rolling_window_hours,
    })
}

async fn load_events(
    db: &Client,
    include_verify_events: bool,
    limit: Option<i64>,
) -> Result<Vec<TokenBudgetEvent>> {
    let rows = postgres::list_observability_snapshots_by_kinds(
        db,
        &["token_budget_event", "token_benchmark"],
        limit,
    )
    .await?;
    let mut events = Vec::new();
    for row in rows {
        if let Some(event) = parse_snapshot_event(&row)? {
            if !include_traffic_class_in_report(&event.traffic_class, include_verify_events) {
                continue;
            }
            events.push(event);
        }
    }
    Ok(events)
}

fn parse_snapshot_event(row: &ObservabilitySnapshotRecord) -> Result<Option<TokenBudgetEvent>> {
    let (node, fallback_source_kind) = match row.snapshot_kind.as_str() {
        "token_budget_event" => (&row.payload["token_budget_event"], None),
        "token_benchmark" => (
            &row.payload["token_benchmark"],
            Some("verify_token_benchmark_legacy"),
        ),
        _ => return Ok(None),
    };
    if !node.is_object() {
        return Ok(None);
    }
    let source_kind = node["source_kind"]
        .as_str()
        .or(fallback_source_kind)
        .unwrap_or("unknown")
        .to_string();
    let traffic_class = node["traffic_class"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_traffic_class(&source_kind));
    let project = node["project"]
        .as_str()
        .or_else(|| node["project_code"].as_str())
        .unwrap_or_default()
        .to_string();
    let namespace = node["namespace"]
        .as_str()
        .or_else(|| node["namespace_code"].as_str())
        .unwrap_or_default()
        .to_string();
    let query = node["query"].as_str().unwrap_or_default().to_string();
    let query_hash = node["query_hash"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| hex_sha256(query.as_bytes()));
    let query_type = node["query_type"]
        .as_str()
        .filter(|value| !value.is_empty() && *value != "unknown")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_query_type(&query).to_string());
    let target_kind = node["target_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let baseline_hit_target = node["baseline_hit_target"].as_bool().unwrap_or(false);
    let amai_hit_target = node["amai_hit_target"].as_bool().unwrap_or(false);
    let cold_warm_state = node["cold_warm_state"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let baseline_strategy = node["baseline_strategy"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_baseline_strategy(&query_type).to_string());
    let retrieval_mode = node["retrieval_mode"].as_str().map(ToOwned::to_owned);
    let tokenizer = node["tokenizer"].as_str().unwrap_or_default().to_string();
    let latency_ms = node["latency_ms"].as_f64().unwrap_or(0.0);
    let event_id = node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{}-{}", row.snapshot_kind, row.created_at_epoch_ms));
    let correlation_id = node["correlation_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| node["context_pack_id"].as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| event_id.clone());
    let payload_origin = node["payload_origin"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let session_id = node["session_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let rolling_window_profile = node["rolling_window_profile"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let timestamp_utc = node["timestamp_utc"]
        .as_i64()
        .unwrap_or(row.created_at_epoch_ms);
    let occurred_at_epoch_ms = node["occurred_at_epoch_ms"]
        .as_i64()
        .unwrap_or(timestamp_utc);
    let ingested_at_epoch_ms = node["ingested_at_epoch_ms"]
        .as_i64()
        .unwrap_or(row.created_at_epoch_ms);
    let measurement_scope = node["measurement_scope"]
        .as_str()
        .unwrap_or("retrieval_lower_bound")
        .to_string();
    let usage_event_schema_version = node["contract"]["usage_event_schema_version"]
        .as_str()
        .unwrap_or("billing-usage-event-v0")
        .to_string();
    let settlement_statement_version = node["contract"]["settlement_statement_version"]
        .as_str()
        .unwrap_or("settlement-preview-v0")
        .to_string();
    let metering_event_schema_version = node["contract"]["metering_event_schema_version"]
        .as_str()
        .unwrap_or("token-budget-event-v1")
        .to_string();
    let usage_lifecycle_model_version = node["contract"]["usage_lifecycle_model_version"]
        .as_str()
        .unwrap_or("usage-lifecycle-v0")
        .to_string();
    let baseline_method_version = node["contract"]["baseline_method_version"]
        .as_str()
        .unwrap_or("retrieval-baseline-v0")
        .to_string();
    let quality_method_version = node["contract"]["quality_method_version"]
        .as_str()
        .unwrap_or("quality-gate-v0")
        .to_string();
    let coverage_model_version = node["contract"]["coverage_model_version"]
        .as_str()
        .unwrap_or("token-coverage-v0")
        .to_string();
    let metering_freshness_model_version = node["contract"]["metering_freshness_model_version"]
        .as_str()
        .unwrap_or("metering-freshness-v0")
        .to_string();
    let excluded_taxonomy_version = node["contract"]["excluded_taxonomy_version"]
        .as_str()
        .unwrap_or("token-excluded-usage-v0")
        .to_string();
    let dedup_contract_version = node["contract"]["dedup_contract_version"]
        .as_str()
        .unwrap_or("event-id-source-kind-v0")
        .to_string();
    let backfill_policy_version = node["contract"]["backfill_policy_version"]
        .as_str()
        .unwrap_or("report-only-backfill-v0")
        .to_string();
    let correction_policy_version = node["contract"]["correction_policy_version"]
        .as_str()
        .unwrap_or("report-only-correction-v0")
        .to_string();
    let freeze_close_policy_version = node["contract"]["freeze_close_policy_version"]
        .as_str()
        .unwrap_or("freeze-close-v0")
        .to_string();
    let late_arrival_policy_version = node["contract"]["late_arrival_policy_version"]
        .as_str()
        .unwrap_or("late-arrival-v0")
        .to_string();
    let dispute_policy_version = node["contract"]["dispute_policy_version"]
        .as_str()
        .unwrap_or("report-only-dispute-v0")
        .to_string();
    let settlement_lifecycle_model_version = node["contract"]["settlement_lifecycle_model_version"]
        .as_str()
        .unwrap_or("settlement-lifecycle-v0")
        .to_string();
    let statement_period_governance_version =
        node["contract"]["statement_period_governance_version"]
            .as_str()
            .unwrap_or("statement-period-governance-v0")
            .to_string();
    let adjustment_preview_model_version = node["contract"]["adjustment_preview_model_version"]
        .as_str()
        .unwrap_or("adjustment-preview-v0")
        .to_string();
    let adjustment_request_schema_version = node["contract"]["adjustment_request_schema_version"]
        .as_str()
        .unwrap_or("adjustment-request-v0")
        .to_string();
    let adjustment_registry_version = node["contract"]["adjustment_registry_version"]
        .as_str()
        .unwrap_or("adjustment-registry-v0")
        .to_string();
    let rate_card_binding_model_version = node["contract"]["rate_card_binding_model_version"]
        .as_str()
        .unwrap_or("rate-card-binding-v0")
        .to_string();
    let telemetry_surface_split_version = node["contract"]["telemetry_surface_split_version"]
        .as_str()
        .unwrap_or("tokenonomics-surface-split-v0")
        .to_string();
    let event_time_policy_version = node["contract"]["event_time_policy_version"]
        .as_str()
        .unwrap_or("client-visible-ingest-v0")
        .to_string();
    let billing_policy_version = node["contract"]["billing_policy_version"]
        .as_str()
        .unwrap_or("report-only-v0")
        .to_string();
    let suitability_model_version = node["contract"]["suitability_model_version"]
        .as_str()
        .unwrap_or("token-suitability-v0")
        .to_string();
    let billing_mode = node["contract"]["billing_mode"]
        .as_str()
        .unwrap_or("report_only")
        .to_string();
    let reconciliation_contract_version = node["contract"]["reconciliation_contract_version"]
        .as_str()
        .unwrap_or("provider-reconciliation-v0")
        .to_string();
    let margin_model_version = node["contract"]["margin_model_version"]
        .as_str()
        .unwrap_or("margin-view-v0")
        .to_string();
    let infra_cost_profile_version = node["contract"]["infra_cost_profile_version"]
        .as_str()
        .unwrap_or("unpriced-infra-v0")
        .to_string();
    let contractual_evidence_pack_version = node["contract"]["contractual_evidence_pack_version"]
        .as_str()
        .unwrap_or("contractual-evidence-pack-v0")
        .to_string();
    let rate_card_version = node["contract"]["rate_card_version"]
        .as_str()
        .unwrap_or("unpriced-v0")
        .to_string();
    let currency_profile = node["contract"]["currency_profile"]
        .as_str()
        .unwrap_or("unpriced")
        .to_string();
    let settlement_status = node["contract"]["settlement_status"]
        .as_str()
        .unwrap_or("unsettled_report_only")
        .to_string();
    let saved_tokens = node["savings"]["saved_tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"]
        .as_u64()
        .or_else(|| node["baseline_tokens"].as_u64())
        .unwrap_or(0);
    let context_tokens = node["context_pack_render"]["tokens"]
        .as_u64()
        .or_else(|| node["delivered_tokens"].as_u64())
        .unwrap_or(0);
    let recovery_tokens = node["recovery"]["recovery_tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens = node["savings"]["effective_saved_tokens"]
        .as_i64()
        .unwrap_or_else(|| naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
    let savings_factor = node["savings"]["savings_factor"].as_f64().unwrap_or(0.0);
    let savings_percent = node["savings"]["savings_percent"]
        .as_f64()
        .or_else(|| node["gross_savings_pct"].as_f64())
        .unwrap_or(0.0);
    let effective_savings_percent = node["savings"]["effective_savings_percent"]
        .as_f64()
        .unwrap_or_else(|| percent_from_signed(effective_saved_tokens, naive_tokens));
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let quality_score = node["quality"]["quality_score"]
        .as_f64()
        .unwrap_or(if quality_ok { 1.0 } else { 0.0 });
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or(if node["quality"].is_object() {
            "unknown"
        } else {
            "legacy_unverified"
        })
        .to_string();
    let quality_tier = node["quality"]["quality_tier"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let head_hit_target = node["quality"]["head_hit_target"]
        .as_bool()
        .unwrap_or(false);
    let needed_followup = node["followup"]["needed_followup"]
        .as_bool()
        .unwrap_or(!quality_ok);
    let followup_count = node["followup"]["followup_count"].as_u64().unwrap_or(0);
    let followup_of_event_id = node["followup"]["followup_of_event_id"]
        .as_str()
        .map(ToOwned::to_owned);
    let resolved_by_event_id = node["followup"]["resolved_by_event_id"]
        .as_str()
        .map(ToOwned::to_owned);
    let fallback_triggered = node["recovery"]["fallback_triggered"]
        .as_bool()
        .unwrap_or(false);
    let fallback_count = node["recovery"]["fallback_count"].as_u64().unwrap_or(0);
    let document_hits = node["shape"]["document_hits"].as_u64().unwrap_or(0);
    let symbol_hits_count = node["shape"]["symbol_hits"].as_u64().unwrap_or(0);
    let file_hits = node["shape"]["file_hits"].as_u64().unwrap_or(0);
    let sources_count = node["shape"]["sources_count"].as_u64().unwrap_or(0);
    let chunks_count = node["shape"]["chunks_count"].as_u64().unwrap_or(0);
    let pack_token_count = node["shape"]["pack_token_count"]
        .as_u64()
        .unwrap_or(context_tokens);
    let deduped_token_count = node["shape"]["deduped_token_count"]
        .as_u64()
        .unwrap_or(context_tokens);
    let client_prompt_tokens = node["whole_cycle_observed"]["client_prompt_tokens"].as_u64();
    let assistant_generation_tokens =
        node["whole_cycle_observed"]["assistant_generation_tokens"].as_u64();
    let tool_overhead_tokens = node["whole_cycle_observed"]["tool_overhead_tokens"].as_u64();
    let continuity_restore_tokens =
        node["whole_cycle_observed"]["continuity_restore_tokens"].as_u64();

    Ok(Some(TokenBudgetEvent {
        created_at_epoch_ms: row.created_at_epoch_ms,
        event_id,
        correlation_id,
        payload_origin,
        session_id,
        rolling_window_profile,
        timestamp_utc,
        occurred_at_epoch_ms,
        ingested_at_epoch_ms,
        snapshot_kind: row.snapshot_kind.clone(),
        source_kind,
        traffic_class,
        measurement_scope,
        usage_event_schema_version,
        settlement_statement_version,
        metering_event_schema_version,
        usage_lifecycle_model_version,
        baseline_method_version,
        quality_method_version,
        coverage_model_version,
        metering_freshness_model_version,
        excluded_taxonomy_version,
        dedup_contract_version,
        backfill_policy_version,
        correction_policy_version,
        freeze_close_policy_version,
        late_arrival_policy_version,
        dispute_policy_version,
        settlement_lifecycle_model_version,
        statement_period_governance_version,
        adjustment_preview_model_version,
        adjustment_request_schema_version,
        adjustment_registry_version,
        rate_card_binding_model_version,
        telemetry_surface_split_version,
        event_time_policy_version,
        billing_policy_version,
        suitability_model_version,
        billing_mode,
        reconciliation_contract_version,
        margin_model_version,
        infra_cost_profile_version,
        contractual_evidence_pack_version,
        rate_card_version,
        currency_profile,
        settlement_status,
        project,
        namespace,
        query,
        query_hash,
        query_type,
        target_kind,
        baseline_hit_target,
        amai_hit_target,
        cold_warm_state,
        baseline_strategy,
        retrieval_mode,
        tokenizer,
        latency_ms,
        saved_tokens,
        naive_tokens,
        context_tokens,
        recovery_tokens,
        effective_saved_tokens,
        savings_factor,
        savings_percent,
        effective_savings_percent,
        quality_ok,
        quality_score,
        quality_method,
        quality_tier,
        head_hit_target,
        needed_followup,
        followup_count,
        followup_of_event_id,
        resolved_by_event_id,
        fallback_triggered,
        fallback_count,
        document_hits,
        symbol_hits_count,
        file_hits,
        sources_count,
        chunks_count,
        pack_token_count,
        deduped_token_count,
        client_prompt_tokens,
        assistant_generation_tokens,
        tool_overhead_tokens,
        continuity_restore_tokens,
    }))
}

fn needs_live_reverification(payload: &Value) -> bool {
    let node = &payload["token_budget_event"];
    if !node.is_object() {
        return false;
    }
    let source_kind = node["source_kind"].as_str().unwrap_or_default();
    let traffic_class = node["traffic_class"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_traffic_class(source_kind));
    if traffic_class != "live" {
        return false;
    }
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or_default();
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let needs_shape_upgrade = node["target_kind"]
        .as_str()
        .map(|value| value.is_empty() || value == "unknown")
        .unwrap_or(true)
        || node.get("latency_ms").is_none()
        || node["quality"].get("quality_tier").is_none()
        || node["quality"].get("head_hit_target").is_none()
        || node["shape"].get("pack_token_count").is_none()
        || node["shape"].get("deduped_token_count").is_none()
        || node["followup"].is_null()
        || node["shape"].get("file_hits").is_none();
    quality_method == "legacy_unverified"
        || (quality_method.is_empty() && !quality_ok)
        || needs_shape_upgrade
}

fn usage_dedup_key(source_kind: &str, event_id: &str) -> String {
    format!("{source_kind}:{event_id}")
}

fn usage_excluded_reason_code(event: &TokenBudgetEvent) -> Option<&'static str> {
    if event.traffic_class != "live" {
        return Some("non_live_other");
    }
    if event.quality_ok {
        return None;
    }
    Some(excluded_event_code(event))
}

fn usage_lifecycle_status(event: &TokenBudgetEvent) -> &'static str {
    match usage_excluded_reason_code(event) {
        None => "verified_included",
        Some("quality_gate_failed") => "excluded_quality_gate_failed",
        Some("awaiting_followup_reconciliation") => "excluded_awaiting_followup_reconciliation",
        Some("legacy_unverified") => "excluded_legacy_unverified",
        Some(_) => "excluded_non_live",
    }
}

fn usage_reporting_layer(event: &TokenBudgetEvent) -> &'static str {
    if usage_excluded_reason_code(event).is_none() {
        "measured_non_billable"
    } else {
        "excluded"
    }
}

fn usage_backfill_status(event: &TokenBudgetEvent) -> &'static str {
    if event.traffic_class != "live" {
        "synthetic_ingest"
    } else if event.payload_origin == "reverified_live_context_pack" {
        "reverified_backfill"
    } else if event.metering_event_schema_version != default_metering_event_schema_version() {
        "legacy_ingest"
    } else {
        "live_ingest"
    }
}

async fn reverify_live_event_payload(
    cfg: &AppConfig,
    db: &mut Client,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    row: &ObservabilitySnapshotRecord,
) -> Result<Option<Value>> {
    let node = &row.payload["token_budget_event"];
    if !node.is_object() {
        return Ok(None);
    }

    let project = node["project"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing project"))?;
    let namespace = node["namespace"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing namespace"))?;
    let query = node["query"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing query"))?;

    let args = ContextPackArgs {
        project: project.to_string(),
        namespace: namespace.to_string(),
        query: query.to_string(),
        retrieval_mode: node["retrieval_mode"].as_str().map(ToOwned::to_owned),
        disable_cache: false,
        limit_documents: 5,
        limit_symbols: 8,
        limit_chunks: 8,
        limit_semantic_chunks: 8,
        token_source_kind: "proof_reverify_context_pack".to_string(),
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };

    let result =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &args, false, false).await?;
    let source_kind = node["source_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("live_context_pack");
    let mut rebuilt = build_event_payload(
        &result.payload,
        measurement,
        contract,
        source_kind,
        "reverified_live_context_pack",
    )?;
    apply_reverification_metadata(&mut rebuilt, node, row.created_at_epoch_ms)?;
    Ok(Some(rebuilt))
}

fn apply_reverification_metadata(
    rebuilt_payload: &mut Value,
    original_node: &Value,
    fallback_timestamp_utc: i64,
) -> Result<()> {
    let target_kind_owned = rebuilt_payload["token_budget_event"]["target_kind"]
        .as_str()
        .unwrap_or("file")
        .to_string();
    let exact_hits = rebuilt_payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len);
    let symbol_hits = rebuilt_payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len);
    let lexical_hits = rebuilt_payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_hits = rebuilt_payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let rebuilt_node = rebuilt_payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("rebuilt token event payload missing token_budget_event object"))?;

    let event_id = original_node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp_utc = original_node["timestamp_utc"]
        .as_i64()
        .unwrap_or(fallback_timestamp_utc);
    let source_kind = original_node["source_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("live_context_pack");
    let quality_ok = rebuilt_node
        .get("quality")
        .and_then(|value| value.get("quality_ok"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reverified_at_utc = current_epoch_ms()?;

    rebuilt_node.insert("event_id".to_string(), Value::String(event_id));
    rebuilt_node.insert("timestamp_utc".to_string(), Value::from(timestamp_utc));
    rebuilt_node.insert(
        "source_kind".to_string(),
        Value::String(source_kind.to_string()),
    );
    rebuilt_node.insert(
        "traffic_class".to_string(),
        Value::String(derive_traffic_class(source_kind)),
    );
    rebuilt_node.insert(
        "payload_origin".to_string(),
        Value::String("reverified_live_context_pack".to_string()),
    );
    if let Some(quality) = rebuilt_node
        .get_mut("quality")
        .and_then(Value::as_object_mut)
    {
        let head_hit_target = quality
            .get("head_hit_target")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer_like_proxy = answer_like_from_counts(
            &target_kind_owned,
            head_hit_target,
            exact_hits,
            symbol_hits,
            lexical_hits,
            semantic_hits,
        );
        quality.insert(
            "quality_method".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "reverified_answer_proxy".to_string()
                } else if head_hit_target {
                    "reverified_task_proxy".to_string()
                } else {
                    "reverified_retrieval_parity".to_string()
                }
            } else {
                "reverified_retrieval_miss".to_string()
            }),
        );
        quality.insert(
            "quality_tier".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "answer_proxy".to_string()
                } else if head_hit_target {
                    "task_proxy".to_string()
                } else {
                    "retrieval".to_string()
                }
            } else {
                "partial".to_string()
            }),
        );
        quality.insert(
            "reverified_at_utc".to_string(),
            Value::from(reverified_at_utc),
        );
    }
    rebuilt_node.insert(
        "reverification".to_string(),
        json!({
            "reverified_at_utc": reverified_at_utc,
            "previous_quality_method": original_node["quality"]["quality_method"]
                .as_str()
                .unwrap_or("missing"),
            "previous_quality_ok": original_node["quality"]["quality_ok"]
                .as_bool()
                .unwrap_or(false),
        }),
    );
    Ok(())
}

fn repair_legacy_token_event_payload(payload: &Value) -> Option<Value> {
    let mut updated = payload.clone();
    let node = updated.get_mut("token_budget_event")?;
    let object = node.as_object_mut()?;
    let source_kind = object
        .get("source_kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let query = object
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let query_type = object
        .get("query_type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && *value != "unknown")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_query_type(query).to_string());
    let mut changed = false;

    if !object.contains_key("traffic_class") {
        object.insert(
            "traffic_class".to_string(),
            Value::String(derive_traffic_class(source_kind)),
        );
        changed = true;
    }
    if !object.contains_key("query_type") {
        object.insert("query_type".to_string(), Value::String(query_type.clone()));
        changed = true;
    }
    if !object.contains_key("baseline_strategy") {
        object.insert(
            "baseline_strategy".to_string(),
            Value::String(derive_baseline_strategy(&query_type).to_string()),
        );
        changed = true;
    }
    if !object.contains_key("recovery") {
        object.insert(
            "recovery".to_string(),
            json!({
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0
            }),
        );
        changed = true;
    }
    if !object.contains_key("shape") {
        object.insert(
            "shape".to_string(),
            json!({
                "sources_count": 0,
                "chunks_count": 0
            }),
        );
        changed = true;
    }
    if !object.contains_key("quality") {
        object.insert(
            "quality".to_string(),
            json!({
                "quality_ok": false,
                "quality_score": 0.0,
                "quality_method": "legacy_unverified",
                "quality_tier": "unverified",
                "head_hit_target": false
            }),
        );
        changed = true;
    }
    let naive_tokens = object
        .get("naive_scope")
        .and_then(|value| value.get("tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let context_tokens = object
        .get("context_pack_render")
        .and_then(|value| value.get("tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let recovery_tokens = object
        .get("recovery")
        .and_then(|value| value.get("recovery_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if let Some(savings) = object.get_mut("savings").and_then(Value::as_object_mut) {
        if !savings.contains_key("effective_saved_tokens") {
            savings.insert(
                "effective_saved_tokens".to_string(),
                Value::from(naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64)),
            );
            changed = true;
        }
        if !savings.contains_key("effective_savings_percent") {
            let effective_saved_tokens = savings
                .get("effective_saved_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
            savings.insert(
                "effective_savings_percent".to_string(),
                Value::from(percent_from_signed(effective_saved_tokens, naive_tokens)),
            );
            changed = true;
        }
    }

    changed.then_some(updated)
}

fn current_session_events(
    events: &[TokenBudgetEvent],
    session_gap_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let Some(latest) = events.last() else {
        return Vec::new();
    };
    let mut session = vec![latest.clone()];
    let mut newer_ts = latest.created_at_epoch_ms;
    for event in events.iter().rev().skip(1) {
        if newer_ts.saturating_sub(event.created_at_epoch_ms) > session_gap_ms {
            break;
        }
        session.push(event.clone());
        newer_ts = event.created_at_epoch_ms;
    }
    session.reverse();
    session
}

fn active_same_meter_scope_events(
    session_events: &[TokenBudgetEvent],
    rolling_window_events: &[TokenBudgetEvent],
) -> Vec<TokenBudgetEvent> {
    let mut seen = BTreeSet::new();
    let mut scoped = Vec::new();
    for event in session_events.iter().chain(rolling_window_events.iter()) {
        if seen.insert(event.event_id.clone()) {
            scoped.push(event.clone());
        }
    }
    scoped
}

fn resolve_session_id(events: &[TokenBudgetEvent], current_ts: i64, session_gap_ms: i64) -> String {
    events
        .iter()
        .rev()
        .find(|event| {
            event.traffic_class == "live"
                && current_ts.saturating_sub(event.created_at_epoch_ms) <= session_gap_ms
        })
        .map(|event| {
            if event.session_id.is_empty() {
                event.event_id.clone()
            } else {
                event.session_id.clone()
            }
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

fn set_recovery_penalty(
    payload: &mut Value,
    recovery_tokens: u64,
    followup_count: u64,
) -> Result<()> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
    let recovery = ensure_nested_object(node, "recovery")?;
    recovery.insert("recovery_tokens".to_string(), Value::from(recovery_tokens));
    let followup = ensure_nested_object(node, "followup")?;
    followup.insert("followup_count".to_string(), Value::from(followup_count));

    let context_tokens = node["context_pack_render"]["tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens =
        naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64);
    let effective_savings_percent = percent_from_signed(effective_saved_tokens, naive_tokens);
    let savings = ensure_nested_object(node, "savings")?;
    savings.insert(
        "effective_saved_tokens".to_string(),
        Value::from(effective_saved_tokens),
    );
    savings.insert(
        "effective_savings_percent".to_string(),
        Value::from(effective_savings_percent),
    );
    Ok(())
}

fn ensure_nested_object<'a>(
    parent: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    if !parent.get(key).is_some_and(Value::is_object) {
        parent.insert(key.to_string(), json!({}));
    }
    parent
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("payload field {key} is not an object"))
}

fn reconcile_followup_recovery(
    events: &[TokenBudgetEvent],
    session_gap_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let mut reconciled = events.to_vec();
    for current_index in 1..reconciled.len() {
        if reconciled[current_index].traffic_class != "live"
            || reconciled[current_index].followup_of_event_id.is_some()
        {
            continue;
        }
        let current_ts = reconciled[current_index].created_at_epoch_ms;
        let current_project = reconciled[current_index].project.clone();
        let current_namespace = reconciled[current_index].namespace.clone();
        let current_key = followup_event_key(&reconciled[current_index]);

        for previous_index in (0..current_index).rev() {
            if reconciled[previous_index].traffic_class != "live"
                || !reconciled[previous_index].needed_followup
                || reconciled[previous_index].resolved_by_event_id.is_some()
            {
                continue;
            }
            if current_ts.saturating_sub(reconciled[previous_index].created_at_epoch_ms)
                > session_gap_ms
            {
                break;
            }
            if reconciled[previous_index].project != current_project
                || reconciled[previous_index].namespace != current_namespace
            {
                continue;
            }
            if !followup_queries_related(
                followup_event_key(&reconciled[previous_index]),
                current_key,
            ) {
                continue;
            }
            let recovery_tokens = reconciled[current_index].recovery_tokens.saturating_add(
                reconciled[previous_index]
                    .context_tokens
                    .saturating_add(reconciled[previous_index].recovery_tokens),
            );
            reconciled[current_index].recovery_tokens = recovery_tokens;
            reconciled[current_index].followup_count =
                reconciled[previous_index].followup_count.saturating_add(1);
            reconciled[current_index].followup_of_event_id =
                Some(reconciled[previous_index].event_id.clone());
            reconciled[current_index].effective_saved_tokens = reconciled[current_index]
                .naive_tokens as i64
                - (reconciled[current_index].context_tokens as i64 + recovery_tokens as i64);
            reconciled[current_index].effective_savings_percent = percent_from_signed(
                reconciled[current_index].effective_saved_tokens,
                reconciled[current_index].naive_tokens,
            );
            reconciled[previous_index].resolved_by_event_id =
                Some(reconciled[current_index].event_id.clone());
            break;
        }
    }
    reconciled
}

fn followup_event_key(event: &TokenBudgetEvent) -> FollowupEventKey<'_> {
    FollowupEventKey {
        query: &event.query,
        query_hash: &event.query_hash,
        query_type: &event.query_type,
        target_kind: &event.target_kind,
    }
}

fn followup_queries_related(current: FollowupEventKey<'_>, follower: FollowupEventKey<'_>) -> bool {
    if !current.query_hash.is_empty() && current.query_hash == follower.query_hash {
        return true;
    }
    if current.query_type != follower.query_type {
        return false;
    }
    if current.target_kind != follower.target_kind {
        return false;
    }
    if normalized_query(current.query) == normalized_query(follower.query) {
        return true;
    }
    query_terms_overlap_count(current.query, follower.query) >= 2
}

fn query_terms_overlap_count(left: &str, right: &str) -> usize {
    let left_terms = extract_query_terms(left);
    if left_terms.is_empty() {
        return 0;
    }
    let right_terms = extract_query_terms(right);
    if right_terms.is_empty() {
        return 0;
    }
    let right_set = right_terms.into_iter().collect::<HashSet<_>>();
    left_terms
        .into_iter()
        .filter(|term| right_set.contains(term))
        .count()
}

fn normalized_query(query: &str) -> String {
    extract_query_terms(query).join(" ")
}

fn derived_client_prompt_tokens(
    event: &TokenBudgetEvent,
    tokenizer_cache: &mut HashMap<String, Option<CoreBPE>>,
) -> Option<u64> {
    if let Some(tokens) = event.client_prompt_tokens {
        return Some(tokens);
    }
    if event.query.is_empty() {
        return None;
    }
    if !tokenizer_cache.contains_key(&event.tokenizer) {
        tokenizer_cache.insert(
            event.tokenizer.clone(),
            build_tokenizer(&event.tokenizer).ok(),
        );
    }
    tokenizer_cache
        .get(&event.tokenizer)
        .and_then(|tokenizer| tokenizer.as_ref())
        .map(|tokenizer| tokenizer.encode_with_special_tokens(&event.query).len() as u64)
}

fn summarize_events(
    events: &[TokenBudgetEvent],
    now_epoch_ms: i64,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    if events.is_empty() {
        return json!({
            "events_total": 0,
            "events_count": 0,
            "live_events_count": 0,
            "non_live_events_count": 0,
            "counted_events": 0,
            "task_success_like_counted_events": 0,
            "answer_like_counted_events": 0,
            "legacy_unverified_events": 0,
            "preliminary": true,
            "baseline_tokens": 0,
            "delivered_tokens": 0,
            "recovery_tokens": 0,
            "observed_client_prompt_tokens": 0,
            "observed_assistant_generation_tokens": 0,
            "observed_tool_overhead_tokens": 0,
            "observed_continuity_restore_tokens": 0,
            "observed_client_prompt_live_events": 0,
            "observed_assistant_generation_live_events": 0,
            "observed_tool_overhead_live_events": 0,
            "observed_continuity_restore_live_events": 0,
            "observed_whole_cycle_with_amai_tokens": 0,
            "verified_observed_whole_cycle_with_amai_tokens": 0,
            "effective_saved_tokens": 0,
            "total_saved_tokens": 0,
            "total_effective_saved_tokens": 0,
            "verified_effective_saved_tokens": 0,
            "verified_task_like_saved_tokens": 0,
            "verified_answer_like_saved_tokens": 0,
            "total_naive_tokens": 0,
            "total_context_tokens": 0,
            "total_recovery_tokens": 0,
            "gross_savings_pct": 0.0,
            "effective_savings_pct": 0.0,
            "verified_effective_savings_pct": 0.0,
            "verified_task_like_savings_pct": 0.0,
            "verified_answer_like_savings_pct": 0.0,
            "savings_percent": 0.0,
            "savings_factor": 0.0,
            "avg_saved_tokens_per_event": 0.0,
            "quality_ok_rate": 0.0,
            "task_success_like_rate": 0.0,
            "answer_like_rate": 0.0,
            "fallback_rate": 0.0,
            "median_recovery_tokens": 0.0,
            "p95_latency_ms": 0.0,
            "started_at_epoch_ms": Value::Null,
            "ended_at_epoch_ms": Value::Null,
            "age_ms_since_latest": Value::Null,
            "coverage": build_coverage_summary(contract, 0, 0, 0, 0, 0),
            "excluded_breakdown": build_excluded_breakdown(contract, &[]),
        });
    }

    let mut tokenizer_cache = HashMap::<String, Option<CoreBPE>>::new();
    let total_saved_tokens = events.iter().map(|event| event.saved_tokens).sum::<u64>();
    let total_naive_tokens = events.iter().map(|event| event.naive_tokens).sum::<u64>();
    let total_context_tokens = events.iter().map(|event| event.context_tokens).sum::<u64>();
    let total_recovery_tokens = events
        .iter()
        .map(|event| event.recovery_tokens)
        .sum::<u64>();
    let observed_client_prompt_tokens = events
        .iter()
        .filter_map(|event| derived_client_prompt_tokens(event, &mut tokenizer_cache))
        .sum::<u64>();
    let observed_assistant_generation_tokens = events
        .iter()
        .filter_map(|event| event.assistant_generation_tokens)
        .sum::<u64>();
    let observed_tool_overhead_tokens = events
        .iter()
        .filter_map(|event| event.tool_overhead_tokens)
        .sum::<u64>();
    let observed_continuity_restore_tokens = events
        .iter()
        .filter_map(|event| event.continuity_restore_tokens)
        .sum::<u64>();
    let live_events_count = events
        .iter()
        .filter(|event| event.traffic_class == "live")
        .count();
    let non_live_events_count = events.len().saturating_sub(live_events_count);
    let total_effective_saved_tokens = events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_events = events
        .iter()
        .filter(|event| event.traffic_class == "live" && event.quality_ok)
        .collect::<Vec<_>>();
    let verified_effective_saved_tokens = verified_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_baseline_tokens = verified_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let verified_delivered_tokens = verified_events
        .iter()
        .map(|event| event.context_tokens)
        .sum::<u64>();
    let verified_recovery_tokens = verified_events
        .iter()
        .map(|event| event.recovery_tokens)
        .sum::<u64>();
    let verified_observed_client_prompt_tokens = verified_events
        .iter()
        .filter_map(|event| derived_client_prompt_tokens(event, &mut tokenizer_cache))
        .sum::<u64>();
    let verified_observed_assistant_generation_tokens = verified_events
        .iter()
        .filter_map(|event| event.assistant_generation_tokens)
        .sum::<u64>();
    let verified_observed_tool_overhead_tokens = verified_events
        .iter()
        .filter_map(|event| event.tool_overhead_tokens)
        .sum::<u64>();
    let verified_observed_continuity_restore_tokens = verified_events
        .iter()
        .filter_map(|event| event.continuity_restore_tokens)
        .sum::<u64>();
    let observed_client_prompt_live_events = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && derived_client_prompt_tokens(event, &mut tokenizer_cache).is_some()
        })
        .count() as u64;
    let observed_assistant_generation_live_events = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live" && event.assistant_generation_tokens.is_some()
        })
        .count() as u64;
    let observed_tool_overhead_live_events = events
        .iter()
        .filter(|event| event.traffic_class == "live" && event.tool_overhead_tokens.is_some())
        .count() as u64;
    let observed_continuity_restore_live_events = events
        .iter()
        .filter(|event| event.traffic_class == "live" && event.continuity_restore_tokens.is_some())
        .count() as u64;
    let observed_whole_cycle_with_amai_tokens = total_context_tokens
        .saturating_add(total_recovery_tokens)
        .saturating_add(observed_client_prompt_tokens)
        .saturating_add(observed_assistant_generation_tokens)
        .saturating_add(observed_tool_overhead_tokens)
        .saturating_add(observed_continuity_restore_tokens);
    let verified_observed_whole_cycle_with_amai_tokens = verified_delivered_tokens
        .saturating_add(verified_recovery_tokens)
        .saturating_add(verified_observed_client_prompt_tokens)
        .saturating_add(verified_observed_assistant_generation_tokens)
        .saturating_add(verified_observed_tool_overhead_tokens)
        .saturating_add(verified_observed_continuity_restore_tokens);
    let excluded_events = events
        .iter()
        .filter(|event| !(event.traffic_class == "live" && event.quality_ok))
        .collect::<Vec<_>>();
    let excluded_effective_saved_tokens = excluded_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let excluded_baseline_tokens = excluded_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let excluded_delivered_tokens = excluded_events
        .iter()
        .map(|event| event.context_tokens)
        .sum::<u64>();
    let excluded_recovery_tokens = excluded_events
        .iter()
        .map(|event| event.recovery_tokens)
        .sum::<u64>();
    let task_like_events = verified_events
        .iter()
        .copied()
        .filter(|event| {
            matches!(
                event.quality_tier.as_str(),
                "task_proxy"
                    | "task_success_recovered"
                    | "answer_proxy"
                    | "answer_success_recovered"
            )
        })
        .collect::<Vec<_>>();
    let answer_like_events = verified_events
        .iter()
        .copied()
        .filter(|event| is_answer_like_event(event))
        .collect::<Vec<_>>();
    let verified_task_like_saved_tokens = task_like_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_task_like_baseline_tokens = task_like_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let verified_answer_like_saved_tokens = answer_like_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_answer_like_baseline_tokens = answer_like_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let gross_savings_pct = if total_naive_tokens == 0 {
        0.0
    } else {
        total_saved_tokens as f64 * 100.0 / total_naive_tokens as f64
    };
    let effective_savings_pct =
        percent_from_signed(total_effective_saved_tokens, total_naive_tokens);
    let verified_effective_savings_pct =
        percent_from_signed(verified_effective_saved_tokens, verified_baseline_tokens);
    let verified_task_like_savings_pct = percent_from_signed(
        verified_task_like_saved_tokens,
        verified_task_like_baseline_tokens,
    );
    let verified_answer_like_savings_pct = percent_from_signed(
        verified_answer_like_saved_tokens,
        verified_answer_like_baseline_tokens,
    );
    let savings_factor = if total_context_tokens == 0 {
        total_naive_tokens as f64
    } else {
        total_naive_tokens as f64 / total_context_tokens as f64
    };
    let avg_saved_tokens_per_event = total_saved_tokens as f64 / events.len() as f64;
    let quality_ok_events = events.iter().filter(|event| event.quality_ok).count() as f64;
    let task_success_like_events = events
        .iter()
        .filter(|event| {
            matches!(
                event.quality_tier.as_str(),
                "task_proxy"
                    | "task_success_recovered"
                    | "answer_proxy"
                    | "answer_success_recovered"
            )
        })
        .count() as f64;
    let answer_like_events_rate = events
        .iter()
        .filter(|event| is_answer_like_event(event))
        .count() as f64;
    let legacy_unverified_events = events
        .iter()
        .filter(|event| event.quality_method == "legacy_unverified")
        .count();
    let fallback_events = events
        .iter()
        .filter(|event| event.fallback_triggered)
        .count() as f64;
    let mut recovery_values = events
        .iter()
        .map(|event| event.recovery_tokens as f64)
        .collect::<Vec<_>>();
    recovery_values
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let median_recovery_tokens = percentile_from_sorted(&recovery_values, 0.5);
    let mut latency_values = events
        .iter()
        .map(|event| event.latency_ms)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    latency_values
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let latency_sample_count = latency_values.len();
    let current_latency_ms = events
        .iter()
        .rev()
        .map(|event| event.latency_ms)
        .find(|value| value.is_finite())
        .unwrap_or_default();
    let p50_latency_ms = percentile_from_sorted(&latency_values, 0.50);
    let p95_latency_ms = percentile_from_sorted(&latency_values, 0.95);
    let p99_latency_ms = percentile_from_sorted(&latency_values, 0.99);
    let max_latency_ms = latency_values.last().copied().unwrap_or_default();
    let quality_ok_rate = quality_ok_events * 100.0 / events.len() as f64;
    let task_success_like_rate = task_success_like_events * 100.0 / events.len() as f64;
    let answer_like_rate = answer_like_events_rate * 100.0 / events.len() as f64;
    let fallback_rate = fallback_events * 100.0 / events.len() as f64;
    let started_at_epoch_ms = events
        .first()
        .map(|event| event.created_at_epoch_ms)
        .unwrap_or_default();
    let ended_at_epoch_ms = events
        .last()
        .map(|event| event.created_at_epoch_ms)
        .unwrap_or_default();

    let preliminary = events.len() < measurement.preliminary_min_events as usize
        && total_naive_tokens < measurement.preliminary_min_baseline_tokens;
    let coverage = build_coverage_summary(
        contract,
        events.len() as u64,
        verified_events.len() as u64,
        excluded_events.len() as u64,
        total_naive_tokens,
        verified_baseline_tokens,
    );
    let excluded_breakdown = build_excluded_breakdown(contract, &excluded_events);

    json!({
        "events_total": events.len(),
        "events_count": events.len(),
        "live_events_count": live_events_count,
        "non_live_events_count": non_live_events_count,
        "counted_events": verified_events.len(),
        "task_success_like_counted_events": task_like_events.len(),
        "answer_like_counted_events": answer_like_events.len(),
        "legacy_unverified_events": legacy_unverified_events,
        "preliminary": preliminary,
        "baseline_tokens": total_naive_tokens,
        "delivered_tokens": total_context_tokens,
        "recovery_tokens": total_recovery_tokens,
        "observed_client_prompt_tokens": observed_client_prompt_tokens,
        "observed_assistant_generation_tokens": observed_assistant_generation_tokens,
        "observed_tool_overhead_tokens": observed_tool_overhead_tokens,
        "observed_continuity_restore_tokens": observed_continuity_restore_tokens,
        "observed_client_prompt_live_events": observed_client_prompt_live_events,
        "observed_assistant_generation_live_events": observed_assistant_generation_live_events,
        "observed_tool_overhead_live_events": observed_tool_overhead_live_events,
        "observed_continuity_restore_live_events": observed_continuity_restore_live_events,
        "observed_whole_cycle_with_amai_tokens": observed_whole_cycle_with_amai_tokens,
        "verified_observed_whole_cycle_with_amai_tokens": verified_observed_whole_cycle_with_amai_tokens,
        "effective_saved_tokens": total_effective_saved_tokens,
        "total_saved_tokens": total_saved_tokens,
        "total_effective_saved_tokens": total_effective_saved_tokens,
        "verified_effective_saved_tokens": verified_effective_saved_tokens,
        "verified_baseline_tokens": verified_baseline_tokens,
        "verified_delivered_tokens": verified_delivered_tokens,
        "verified_recovery_tokens": verified_recovery_tokens,
        "verified_task_like_saved_tokens": verified_task_like_saved_tokens,
        "verified_answer_like_saved_tokens": verified_answer_like_saved_tokens,
        "excluded_events_count": excluded_events.len(),
        "excluded_effective_saved_tokens": excluded_effective_saved_tokens,
        "excluded_baseline_tokens": excluded_baseline_tokens,
        "excluded_delivered_tokens": excluded_delivered_tokens,
        "excluded_recovery_tokens": excluded_recovery_tokens,
        "total_naive_tokens": total_naive_tokens,
        "total_context_tokens": total_context_tokens,
        "total_recovery_tokens": total_recovery_tokens,
        "gross_savings_pct": gross_savings_pct,
        "effective_savings_pct": effective_savings_pct,
        "verified_effective_savings_pct": verified_effective_savings_pct,
        "verified_task_like_savings_pct": verified_task_like_savings_pct,
        "verified_answer_like_savings_pct": verified_answer_like_savings_pct,
        "savings_percent": gross_savings_pct,
        "savings_factor": savings_factor,
        "avg_saved_tokens_per_event": avg_saved_tokens_per_event,
        "quality_ok_rate": quality_ok_rate,
        "task_success_like_rate": task_success_like_rate,
        "answer_like_rate": answer_like_rate,
        "fallback_rate": fallback_rate,
        "median_recovery_tokens": median_recovery_tokens,
        "sample_count": latency_sample_count,
        "current_latency_ms": current_latency_ms,
        "p50_latency_ms": p50_latency_ms,
        "p95_latency_ms": p95_latency_ms,
        "p99_latency_ms": p99_latency_ms,
        "max_latency_ms": max_latency_ms,
        "latency_slices": latency_slice_breakdown(events),
        "started_at_epoch_ms": started_at_epoch_ms,
        "ended_at_epoch_ms": ended_at_epoch_ms,
        "age_ms_since_latest": now_epoch_ms.saturating_sub(ended_at_epoch_ms),
        "coverage": coverage,
        "excluded_breakdown": excluded_breakdown,
    })
}

fn build_coverage_summary(
    contract: &TokenBudgetContractConfig,
    measured_events: u64,
    included_events: u64,
    excluded_events: u64,
    measured_baseline_tokens: u64,
    included_baseline_tokens: u64,
) -> Value {
    let excluded_baseline_tokens =
        measured_baseline_tokens.saturating_sub(included_baseline_tokens);
    let event_coverage_pct = percent_share(included_events, measured_events);
    let baseline_token_coverage_pct =
        percent_share(included_baseline_tokens, measured_baseline_tokens);
    let completeness_state = if measured_events == 0 {
        "empty"
    } else if included_events == 0 {
        "no_confirmed_usage"
    } else if included_events == measured_events {
        "fully_confirmed"
    } else {
        "partially_confirmed"
    };
    json!({
        "model_version": contract.coverage_model_version.clone(),
        "completeness_state": completeness_state,
        "measured_events": measured_events,
        "included_events": included_events,
        "excluded_events": excluded_events,
        "event_coverage_pct": event_coverage_pct,
        "measured_baseline_tokens": measured_baseline_tokens,
        "included_baseline_tokens": included_baseline_tokens,
        "excluded_baseline_tokens": excluded_baseline_tokens,
        "baseline_token_coverage_pct": baseline_token_coverage_pct,
    })
}

fn build_excluded_breakdown(
    contract: &TokenBudgetContractConfig,
    excluded_events: &[&TokenBudgetEvent],
) -> Value {
    let mut grouped = BTreeMap::<String, (u64, u64, u64, u64, i64)>::new();
    for event in excluded_events {
        let code = excluded_event_code(event).to_string();
        let entry = grouped.entry(code).or_insert((0, 0, 0, 0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(event.naive_tokens);
        entry.2 = entry.2.saturating_add(event.context_tokens);
        entry.3 = entry.3.saturating_add(event.recovery_tokens);
        entry.4 = entry.4.saturating_add(event.effective_saved_tokens);
    }
    let items = grouped
        .into_iter()
        .map(
            |(
                code,
                (
                    events_count,
                    baseline_tokens,
                    delivered_tokens,
                    recovery_tokens,
                    effective_saved_tokens,
                ),
            )| {
                json!({
                    "code": code,
                    "label": excluded_event_label(&code),
                    "events_count": events_count,
                    "baseline_tokens": baseline_tokens,
                    "delivered_tokens": delivered_tokens,
                    "recovery_tokens": recovery_tokens,
                    "effective_saved_tokens": effective_saved_tokens,
                })
            },
        )
        .collect::<Vec<_>>();
    json!({
        "model_version": contract.excluded_taxonomy_version.clone(),
        "items": items,
    })
}

fn excluded_event_code(event: &TokenBudgetEvent) -> &'static str {
    match event.traffic_class.as_str() {
        "verify" => "synthetic_verify",
        "proof" => "synthetic_proof",
        "benchmark" => "synthetic_benchmark",
        "live" => {
            if event.quality_method == "legacy_unverified" {
                "legacy_unverified"
            } else if event.needed_followup && event.resolved_by_event_id.is_none() {
                "awaiting_followup_reconciliation"
            } else {
                "quality_gate_failed"
            }
        }
        _ => "non_live_other",
    }
}

fn excluded_event_label(code: &str) -> &'static str {
    match code {
        "synthetic_verify" => "engineering verify-событие",
        "synthetic_proof" => "engineering proof-событие",
        "synthetic_benchmark" => "benchmark-событие",
        "legacy_unverified" => "старое live-событие без quality-блока",
        "awaiting_followup_reconciliation" => "ожидает полезного follow-up или подтверждения",
        "quality_gate_failed" => "не прошло quality gate",
        _ => "другое исключённое событие",
    }
}

fn build_product_headline(summary: &Value, scope_label: &str) -> Value {
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    let legacy_unverified_events = summary["legacy_unverified_events"].as_u64().unwrap_or(0);
    let preliminary = summary["preliminary"].as_bool().unwrap_or(true);
    let verified_percent = summary["verified_effective_savings_pct"]
        .as_f64()
        .unwrap_or(0.0);
    let effective_percent = summary["effective_savings_pct"].as_f64().unwrap_or(0.0);
    let verified_saved_tokens = summary["verified_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0);
    let effective_saved_tokens = summary["total_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0);
    let quality_ok_rate = summary["quality_ok_rate"].as_f64().unwrap_or(0.0);
    let fallback_rate = summary["fallback_rate"].as_f64().unwrap_or(0.0);

    if counted_events > 0 {
        json!({
            "metric_code": "verified_effective_savings_pct",
            "title": "Проверенная реальная экономия",
            "scope_label": scope_label,
            "status": if preliminary { "alert" } else { "pass" },
            "preliminary": preliminary,
            "value_percent": verified_percent,
            "saved_tokens": verified_saved_tokens,
            "events_count": events_total,
            "counted_events": counted_events,
            "quality_ok_rate": quality_ok_rate,
            "fallback_rate": fallback_rate,
            "note": if preliminary {
                "Это уже quality-gated метрика, но выборка пока ещё маленькая."
            } else {
                "Это главный честный KPI: live-only, quality-gated и с учётом recovery."
            },
        })
    } else if events_total > 0 {
        json!({
            "metric_code": "effective_savings_pct_preliminary",
            "title": "Реальная экономия пока предварительно",
            "scope_label": scope_label,
            "status": "alert",
            "preliminary": true,
            "value_percent": effective_percent,
            "saved_tokens": effective_saved_tokens,
            "events_count": events_total,
            "counted_events": counted_events,
            "quality_ok_rate": quality_ok_rate,
            "fallback_rate": fallback_rate,
            "note": if legacy_unverified_events > 0 {
                "Проверенная выборка ещё не набрана: часть исторических live-событий была записана старым форматом без quality-блока, поэтому пока показывается общая реальная экономия."
            } else {
                "Проверенная выборка ещё не набрана, поэтому временно показывается общая реальная экономия по live-событиям."
            },
        })
    } else {
        json!({
            "metric_code": "no_live_events",
            "title": "Реальная экономия пока не накоплена",
            "scope_label": scope_label,
            "status": "unknown",
            "preliminary": true,
            "value_percent": 0.0,
            "saved_tokens": 0,
            "events_count": 0,
            "counted_events": 0,
            "quality_ok_rate": 0.0,
            "fallback_rate": 0.0,
            "note": "Amai ещё не накопил live-события для этой метрики.",
        })
    }
}

fn client_limit_meter_alignment_counts(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
) -> (u64, u64, u64, u64) {
    let events_total = summary["events_total"]
        .as_u64()
        .or_else(|| events.map(|items| items.len() as u64))
        .unwrap_or(0);
    let live_events_count = summary["live_events_count"]
        .as_u64()
        .or_else(|| {
            events.map(|items| {
                items
                    .iter()
                    .filter(|event| event.traffic_class == "live")
                    .count() as u64
            })
        })
        .unwrap_or(0);
    let non_live_events_count = summary["non_live_events_count"]
        .as_u64()
        .or_else(|| events_total.checked_sub(live_events_count))
        .unwrap_or(0);
    let counted_events = summary["counted_events"]
        .as_u64()
        .or_else(|| {
            events.map(|items| {
                items
                    .iter()
                    .filter(|event| event.traffic_class == "live" && event.quality_ok)
                    .count() as u64
            })
        })
        .unwrap_or(0);
    (
        events_total,
        live_events_count,
        non_live_events_count,
        counted_events,
    )
}

fn client_limit_component_stats(
    summary: &Value,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> [(&'static str, u64, u64); 4] {
    [
        (
            "client_prompt",
            summary["observed_client_prompt_live_events"]
                .as_u64()
                .unwrap_or(0),
            summary["observed_client_prompt_tokens"]
                .as_u64()
                .unwrap_or(0),
        ),
        (
            "assistant_generation",
            assistant_scope
                .map(|scope| scope.observed_group_count)
                .unwrap_or_else(|| {
                    summary["observed_assistant_generation_live_events"]
                        .as_u64()
                        .unwrap_or(0)
                }),
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or_else(|| {
                    summary["observed_assistant_generation_tokens"]
                        .as_u64()
                        .unwrap_or(0)
                }),
        ),
        (
            "tool_overhead_outside_retrieval",
            summary["observed_tool_overhead_live_events"]
                .as_u64()
                .unwrap_or(0),
            summary["observed_tool_overhead_tokens"]
                .as_u64()
                .unwrap_or(0),
        ),
        (
            "continuity_restore_outside_retrieval",
            summary["observed_continuity_restore_live_events"]
                .as_u64()
                .unwrap_or(0),
            summary["observed_continuity_restore_tokens"]
                .as_u64()
                .unwrap_or(0),
        ),
    ]
}

fn client_limit_component_target_scope_kind(code: &str) -> &'static str {
    match code {
        "client_prompt" => "all_live_scope",
        "assistant_generation" => "assistant_generation_turn_scope",
        "tool_overhead_outside_retrieval" => "retrieval_live_scope",
        "continuity_restore_outside_retrieval" => "continuity_restore_live_scope",
        _ => "all_live_scope",
    }
}

fn is_client_limit_component_target_event(code: &str, event: &TokenBudgetEvent) -> bool {
    if event.traffic_class != "live" {
        return false;
    }
    match code {
        "client_prompt" => true,
        "assistant_generation" | "tool_overhead_outside_retrieval" => {
            event.measurement_scope == "retrieval_lower_bound"
        }
        "continuity_restore_outside_retrieval" => {
            event.measurement_scope == "whole_cycle_observed_lower_bound"
                && (event.query_type == "continuity_restore"
                    || event.target_kind == "continuity_restore")
        }
        _ => false,
    }
}

fn client_limit_component_target_live_events(
    code: &str,
    events: Option<&[TokenBudgetEvent]>,
    live_events_count: u64,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> u64 {
    if code == "assistant_generation" {
        return assistant_scope
            .map(|scope| scope.target_group_count)
            .unwrap_or_else(|| {
                events
                    .map(|items| {
                        items.iter()
                            .filter(|event| is_client_limit_component_target_event(code, event))
                            .count() as u64
                    })
                    .unwrap_or(live_events_count)
            });
    }
    events
        .map(|items| {
            items.iter()
                .filter(|event| is_client_limit_component_target_event(code, event))
                .count() as u64
        })
        .unwrap_or(live_events_count)
}

fn client_limit_component_event_coverage(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    live_events_count: u64,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Vec<Value> {
    client_limit_component_stats(summary, assistant_scope)
        .into_iter()
        .map(|(code, observed_live_events, observed_tokens)| {
            let target_live_events_count =
                client_limit_component_target_live_events(
                    code,
                    events,
                    live_events_count,
                    assistant_scope,
                );
            json!({
                "code": code,
                "observed_live_events": observed_live_events,
                "live_events_count": live_events_count,
                "target_live_events_count": target_live_events_count,
                "target_scope_kind": client_limit_component_target_scope_kind(code),
                "target_scope_applicable": target_live_events_count > 0,
                "event_coverage_pct": percent_share(observed_live_events, target_live_events_count),
                "observed_tokens": observed_tokens,
            })
        })
        .collect()
}

fn client_limit_meter_alignment_blocking_reasons(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let (events_total, live_events_count, non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    for (code, observed_live_events, _observed_tokens) in
        client_limit_component_stats(summary, assistant_scope)
    {
        let target_live_events = client_limit_component_target_live_events(
            code,
            events,
            live_events_count,
            assistant_scope,
        );
        if target_live_events == 0 {
            continue;
        }
        if observed_live_events == 0 {
            reasons.push(format!("{code}_unmeasured"));
        } else if observed_live_events < target_live_events {
            reasons.push(format!("{code}_partially_measured"));
        }
    }

    if events_total == 0 {
        reasons.push("no_usage_observed_in_scope".to_string());
    } else {
        if live_events_count == 0 {
            reasons.push("no_live_usage_in_scope".to_string());
        }
        if non_live_events_count > 0 {
            reasons.push("non_live_events_present_in_scope".to_string());
        }
        if live_events_count > 0 && counted_events == 0 {
            reasons.push("no_confirmed_live_usage_in_scope".to_string());
        }
        if live_events_count > 0
            && client_limit_component_stats(summary, assistant_scope).into_iter().all(
                |(code, observed_live_events, _observed_tokens)| {
                    let target_live_events = client_limit_component_target_live_events(
                        code,
                        events,
                        live_events_count,
                        assistant_scope,
                    );
                    target_live_events == 0 || observed_live_events == target_live_events
                },
            )
        {
            reasons.push("same_meter_baseline_unmeasured".to_string());
        }
    }
    reasons
}

fn client_limit_meter_alignment_state(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> &'static str {
    let (events_total, live_events_count, _non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    let component_stats = client_limit_component_stats(summary, assistant_scope);
    let any_component_applicable = component_stats.iter().any(
        |(code, _observed_live_events, _observed_tokens)| {
            client_limit_component_target_live_events(
                code,
                events,
                live_events_count,
                assistant_scope,
            ) > 0
        },
    );
    let all_components_observed = live_events_count > 0
        && component_stats
            .iter()
            .all(|(code, observed_live_events, _observed_tokens)| {
                let target_live_events = client_limit_component_target_live_events(
                    code,
                    events,
                    live_events_count,
                    assistant_scope,
                );
                target_live_events == 0 || *observed_live_events == target_live_events
            });
    let any_component_observed = component_stats
        .iter()
        .any(|(_code, observed_live_events, _observed_tokens)| *observed_live_events > 0);

    if events_total == 0 {
        "no_usage_observed"
    } else if live_events_count == 0 {
        "only_non_live_scope_activity"
    } else if counted_events == 0 {
        "live_usage_unconfirmed_not_meter_equivalent"
    } else if any_component_applicable && all_components_observed {
        "whole_cycle_observed_baseline_partial"
    } else if any_component_observed {
        "whole_cycle_partially_observed_not_meter_equivalent"
    } else {
        "partial_lower_bound_not_meter_equivalent"
    }
}

fn assistant_generation_missing_scope_context_pack_ids(
    events: Option<&[TokenBudgetEvent]>,
) -> BTreeSet<String> {
    events
        .into_iter()
        .flatten()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event.assistant_generation_tokens.is_none()
        })
        .map(|event| event.correlation_id.clone())
        .filter(|value| !value.is_empty())
        .collect()
}

fn assistant_generation_observation_source_status(
    events: Option<&[TokenBudgetEvent]>,
    rollout_observations: Option<&[codex_threads::RolloutAssistantGenerationObservation]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let target_ids = assistant_generation_missing_scope_context_pack_ids(events);
    if let Some(scope) = assistant_scope {
        let state = if scope.target_context_pack_ids.is_empty() {
            "no_missing_live_retrieval_events"
        } else if scope.available_turns == 0 {
            "rollout_source_unavailable"
        } else if scope.matched_context_pack_ids.is_empty() {
            "rollout_source_no_scope_overlap"
        } else if !scope.unmatched_context_pack_ids.is_empty() {
            "rollout_source_partial_scope_overlap"
        } else {
            "rollout_source_covers_missing_scope"
        };
        return json!({
            "source_kind": "codex_rollout_turn_timeline_v1",
            "state": state,
            "usable_rollout_turns": scope.available_turns,
            "matched_turn_ids": scope.matched_turn_ids.len(),
            "target_missing_context_pack_ids": scope.target_context_pack_ids.len(),
            "matched_context_pack_ids": scope.matched_context_pack_ids.len(),
            "unmatched_context_pack_ids": scope.unmatched_context_pack_ids.len(),
            "matched_context_pack_id_sample": scope.matched_context_pack_ids.iter().take(8).cloned().collect::<Vec<_>>(),
            "unmatched_context_pack_id_sample": scope.unmatched_context_pack_ids.iter().take(8).cloned().collect::<Vec<_>>(),
            "note": "Этот слой показывает, покрывают ли rollout turn-timelines именно текущий live retrieval scope и можно ли честно привязать assistant_generation к turn-группам без дублирования токенов по каждому context pack."
        });
    }
    let available_ids = rollout_observations
        .into_iter()
        .flatten()
        .map(|observation| observation.context_pack_id.clone())
        .collect::<BTreeSet<_>>();
    let matched_ids = target_ids
        .intersection(&available_ids)
        .cloned()
        .collect::<Vec<_>>();
    let unmatched_ids = target_ids
        .difference(&available_ids)
        .cloned()
        .collect::<Vec<_>>();
    let state = if target_ids.is_empty() {
        "no_missing_live_retrieval_events"
    } else if available_ids.is_empty() {
        "rollout_source_unavailable"
    } else if matched_ids.is_empty() {
        "rollout_source_no_scope_overlap"
    } else if matched_ids.len() < target_ids.len() {
        "rollout_source_partial_scope_overlap"
    } else {
        "rollout_source_covers_missing_scope"
    };
    json!({
        "source_kind": "codex_rollout_last_token_usage_sum_v1",
        "state": state,
        "usable_rollout_context_pack_ids": available_ids.len(),
        "target_missing_context_pack_ids": target_ids.len(),
        "matched_context_pack_ids": matched_ids.len(),
        "unmatched_context_pack_ids": unmatched_ids.len(),
        "matched_context_pack_id_sample": matched_ids.into_iter().take(8).collect::<Vec<_>>(),
        "unmatched_context_pack_id_sample": unmatched_ids.into_iter().take(8).collect::<Vec<_>>(),
        "note": "Этот слой показывает не общий факт missing assistant_generation, а покрывает ли доступный rollout source именно текущий live retrieval scope."
    })
}

fn build_client_limit_meter_alignment(
    contract: &TokenBudgetContractConfig,
    surface_kind: &str,
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    rollout_observations: Option<&[codex_threads::RolloutAssistantGenerationObservation]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let (events_total, live_events_count, non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    let component_coverage = client_limit_component_event_coverage(
        summary,
        events,
        live_events_count,
        assistant_scope,
    );
    let component_stats = client_limit_component_stats(summary, assistant_scope);
    let mut measured_components = vec![
        "retrieval_payload".to_string(),
        "followup_recovery".to_string(),
    ];
    let mut not_applicable_components = Vec::new();
    let mut partially_measured_components = Vec::new();
    let mut missing_components = Vec::new();
    for (code, observed_live_events, _observed_tokens) in component_stats {
        let target_live_events = client_limit_component_target_live_events(
            code,
            events,
            live_events_count,
            assistant_scope,
        );
        if target_live_events == 0 {
            not_applicable_components.push(code.to_string());
        } else if observed_live_events == target_live_events {
            measured_components.push(code.to_string());
        } else {
            missing_components.push(code.to_string());
            if observed_live_events > 0 {
                partially_measured_components.push(code.to_string());
            }
        }
    }
    let assistant_generation_observation_source =
        assistant_generation_observation_source_status(events, rollout_observations, assistant_scope);
    let mut blocking_reasons =
        client_limit_meter_alignment_blocking_reasons(summary, events, assistant_scope);
    match assistant_generation_observation_source["state"].as_str().unwrap_or_default() {
        "rollout_source_unavailable" => {
            blocking_reasons.push("assistant_generation_rollout_source_unavailable".to_string());
        }
        "rollout_source_no_scope_overlap" => {
            blocking_reasons.push("assistant_generation_rollout_source_no_scope_overlap".to_string());
        }
        "rollout_source_partial_scope_overlap" => {
            blocking_reasons
                .push("assistant_generation_rollout_source_partial_scope_overlap".to_string());
        }
        _ => {}
    }
    json!({
        "model_version": contract.client_limit_meter_alignment_version.clone(),
        "surface_kind": surface_kind,
        "alignment_state": client_limit_meter_alignment_state(summary, events, assistant_scope),
        "same_meter_as_client_limit": false,
        "events_total": events_total,
        "live_events_count": live_events_count,
        "non_live_events_count": non_live_events_count,
        "counted_live_events": counted_events,
        "measured_components": measured_components,
        "not_applicable_components": not_applicable_components,
        "partially_measured_components": partially_measured_components,
        "missing_components": missing_components,
        "component_event_coverage": component_coverage,
        "blocking_reasons": blocking_reasons,
        "assistant_generation_observation_source": assistant_generation_observation_source,
        "note": "Этот слой честно показывает, что текущие savings пока считаются как lower-bound части агентного цикла. Whole-cycle observed components могут постепенно materialize-иться, но same meter с клиентским лимитом нельзя объявлять раньше, чем появится и полное observed покрытие, и baseline-equivalent semantics."
    })
}

fn build_agent_cycle_economics(
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    now_epoch_ms: i64,
    current_session_events: &[TokenBudgetEvent],
    rolling_window_events: Option<&[TokenBudgetEvent]>,
    lifetime_events: &[TokenBudgetEvent],
    rolling_window_label: &str,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    current_session_assistant_scope: &AssistantGenerationScopeObservation,
    rolling_window_assistant_scope: Option<&AssistantGenerationScopeObservation>,
    lifetime_assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    json!({
        "model_version": contract.agent_cycle_model_version.clone(),
        "status": "partial_lower_bound",
        "contract": {
            "scope": "lower_bound_whole_agent_cycle",
            "status": "partial_lower_bound",
            "billing_mode": contract.billing_mode.clone(),
            "billing_policy_version": contract.billing_policy_version.clone(),
            "client_limit_meter_alignment": {
                "model_version": contract.client_limit_meter_alignment_version.clone(),
                "alignment_state": "partial_lower_bound_not_meter_equivalent",
                "same_meter_as_client_limit": false,
                "measured_components": [
                    "retrieval_payload",
                    "followup_recovery"
                ],
                "partially_measured_components": [],
                "observable_components": [
                    "client_prompt",
                    "assistant_generation",
                    "tool_overhead_outside_retrieval",
                    "continuity_restore_outside_retrieval"
                ],
                "missing_components": [
                    "client_prompt",
                    "assistant_generation",
                    "tool_overhead_outside_retrieval",
                    "continuity_restore_outside_retrieval"
                ],
                "blocking_reasons": [
                    "client_prompt_unmeasured",
                    "assistant_generation_unmeasured",
                    "tool_overhead_outside_retrieval_unmeasured",
                    "continuity_restore_outside_retrieval_unmeasured"
                ],
                "note": "Даже при высокой measured lower bound current meter ещё не эквивалентен полному клиентскому лимиту сессии. Whole-cycle observed components можно materialize-ить по мере появления event-level evidence, но same-meter claim запрещён раньше baseline-equivalent semantics."
            },
            "rate_card_version": contract.rate_card_version.clone(),
            "currency_profile": contract.currency_profile.clone(),
            "settlement_status": contract.settlement_status.clone(),
            "summary": "Это не весь токеновый бюджет клиента, а подтверждённая нижняя граница полного агентного цикла.",
            "measured_components": [
                {
                    "code": "retrieval_payload",
                    "label": "Контекст, который Amai реально вернул"
                },
                {
                    "code": "followup_recovery",
                    "label": "Доуточнения после неполного ответа, которые уже видно в ledger"
                }
            ],
            "missing_components": [
                {
                    "code": "client_prompt",
                    "label": "Токены исходного запроса клиента"
                },
                {
                    "code": "assistant_generation",
                    "label": "Токены генерации итогового ответа"
                },
                {
                    "code": "tool_overhead_outside_retrieval",
                    "label": "Tool-step и orchestration вне retrieval-контура"
                },
                {
                    "code": "continuity_restore_outside_retrieval",
                    "label": "Восстановление continuity, если оно прошло вне token-ledger retrieval-событий"
                }
            ],
            "reporting_layers": {
                "billable": {
                    "status": "disabled_report_only",
                    "note": "Пока billing policy работает только в report-only режиме, подтверждённая нижняя граница не используется как money-facing начисление."
                },
                "measured_non_billable": {
                    "status": "active",
                    "note": "Подтверждённые live lower-bound измерения уже видны и пригодны для анализа, но ещё не являются contractual billing amount."
                },
                "unmeasured": {
                    "status": "active",
                    "note": "Полный agent-cycle ещё не покрыт: missing components перечислены отдельно и не маскируются под измеренную экономию."
                }
            },
            "note": "Линия 'без Amai' здесь пока означает измеренный baseline retrieval-части цикла, а линия 'с Amai' — retrieval плюс уже зафиксированные доуточнения. Это честная нижняя граница, а не полная стоимость всей агентной сессии."
        },
        "chart_contract": {
            "timeline_type": "event_cumulative",
            "x_axis": "timestamp_epoch_ms",
            "y_axes": [
                "without_amai_measured_tokens",
                "with_amai_measured_tokens",
                "measured_saved_tokens"
            ],
            "series": [
                "all_live_timeline",
                "verified_live_timeline"
            ],
            "point_limit": AGENT_CYCLE_TIMELINE_MAX_POINTS
        },
        "current_session": build_agent_cycle_scope(
            measurement,
            contract,
            now_epoch_ms,
            "current_session",
            "текущая сессия",
            current_session_events,
            AGENT_CYCLE_TIMELINE_MAX_POINTS / 2,
            rollout_observations,
            Some(current_session_assistant_scope),
        ),
        "rolling_window": rolling_window_events
            .map(|events| {
                build_agent_cycle_scope(
                    measurement,
                    contract,
                    now_epoch_ms,
                    "rolling_window",
                    &format!("окно {}", rolling_window_label),
                    events,
                    AGENT_CYCLE_TIMELINE_MAX_POINTS,
                    rollout_observations,
                    rolling_window_assistant_scope,
                )
            })
            .unwrap_or(Value::Null),
        "lifetime": build_agent_cycle_scope(
            measurement,
            contract,
            now_epoch_ms,
            "lifetime",
            "всё время записи",
            lifetime_events,
            AGENT_CYCLE_TIMELINE_MAX_POINTS,
            rollout_observations,
            lifetime_assistant_scope,
        ),
    })
}

fn build_agent_cycle_scope(
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    now_epoch_ms: i64,
    scope_code: &str,
    scope_label: &str,
    events: &[TokenBudgetEvent],
    max_points: usize,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let live_events = events
        .iter()
        .filter(|event| event.traffic_class == "live")
        .cloned()
        .collect::<Vec<_>>();
    let summary = summarize_events(&live_events, now_epoch_ms, measurement, contract);
    let with_amai_measured_tokens = summary["total_context_tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(summary["total_recovery_tokens"].as_u64().unwrap_or(0));
    let verified_with_amai_measured_tokens = summary["verified_delivered_tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(summary["verified_recovery_tokens"].as_u64().unwrap_or(0));
    let observed_whole_cycle_with_amai_tokens = summary["observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .unwrap_or(with_amai_measured_tokens);
    let verified_observed_whole_cycle_with_amai_tokens =
        summary["verified_observed_whole_cycle_with_amai_tokens"]
            .as_u64()
            .unwrap_or(verified_with_amai_measured_tokens);
    let verified_share_pct = percent_share(
        summary["counted_events"].as_u64().unwrap_or(0),
        summary["events_total"].as_u64().unwrap_or(0),
    );
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "status": "partial_lower_bound",
        "events_total": summary["events_total"].as_u64().unwrap_or(0),
        "counted_events": summary["counted_events"].as_u64().unwrap_or(0),
        "excluded_events_count": summary["excluded_events_count"].as_u64().unwrap_or(0),
        "coverage": summary["coverage"].clone(),
        "excluded_breakdown": summary["excluded_breakdown"].clone(),
        "client_limit_meter_alignment": build_client_limit_meter_alignment(
            contract,
            "agent_cycle_scope",
            &summary,
            Some(&live_events),
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
        "verified_share_pct": verified_share_pct,
        "without_amai_measured_tokens": summary["total_naive_tokens"].as_u64().unwrap_or(0),
        "with_amai_measured_tokens": with_amai_measured_tokens,
        "observed_whole_cycle_with_amai_tokens": observed_whole_cycle_with_amai_tokens.saturating_add(
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or(0)
                .saturating_sub(summary["observed_assistant_generation_tokens"].as_u64().unwrap_or(0))
        ),
        "measured_saved_tokens": summary["total_effective_saved_tokens"].as_i64().unwrap_or(0),
        "measured_saved_pct": summary["effective_savings_pct"].as_f64().unwrap_or(0.0),
        "verified_without_amai_measured_tokens": summary["verified_baseline_tokens"].as_u64().unwrap_or(0),
        "verified_with_amai_measured_tokens": verified_with_amai_measured_tokens,
        "verified_observed_whole_cycle_with_amai_tokens": verified_observed_whole_cycle_with_amai_tokens,
        "verified_measured_saved_tokens": summary["verified_effective_saved_tokens"].as_i64().unwrap_or(0),
        "verified_measured_saved_pct": summary["verified_effective_savings_pct"].as_f64().unwrap_or(0.0),
        "answer_like_counted_events": summary["answer_like_counted_events"].as_u64().unwrap_or(0),
        "answer_like_rate": summary["answer_like_rate"].as_f64().unwrap_or(0.0),
        "started_at_epoch_ms": summary["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": summary["ended_at_epoch_ms"].clone(),
        "all_live_timeline": build_agent_cycle_timeline(&live_events, false, max_points),
        "verified_live_timeline": build_agent_cycle_timeline(&live_events, true, max_points),
    })
}

fn build_agent_cycle_timeline(
    events: &[TokenBudgetEvent],
    verified_only: bool,
    max_points: usize,
) -> Value {
    let filtered = events
        .iter()
        .filter(|event| !verified_only || event.quality_ok)
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        return Value::Array(Vec::new());
    }

    let mut without_amai_cumulative = 0_u64;
    let mut with_amai_cumulative = 0_u64;
    let mut points = Vec::with_capacity(filtered.len());
    for (index, event) in filtered.into_iter().enumerate() {
        without_amai_cumulative = without_amai_cumulative.saturating_add(event.naive_tokens);
        with_amai_cumulative = with_amai_cumulative
            .saturating_add(event.context_tokens)
            .saturating_add(event.recovery_tokens);
        let measured_saved_tokens = without_amai_cumulative as i64 - with_amai_cumulative as i64;
        points.push(json!({
            "point_index": index + 1,
            "timestamp_epoch_ms": event.created_at_epoch_ms,
            "event_id": event.event_id,
            "query_type": event.query_type,
            "cold_warm_state": event.cold_warm_state,
            "answer_like": is_answer_like_event(event),
            "without_amai_measured_tokens": without_amai_cumulative,
            "with_amai_measured_tokens": with_amai_cumulative,
            "measured_saved_tokens": measured_saved_tokens,
            "measured_saved_pct": percent_from_signed(measured_saved_tokens, without_amai_cumulative),
        }));
    }
    downsample_timeline(points, max_points)
}

fn downsample_timeline(points: Vec<Value>, max_points: usize) -> Value {
    if points.len() <= max_points || max_points < 2 {
        return Value::Array(points);
    }

    let last_index = points.len() - 1;
    let step = last_index as f64 / (max_points - 1) as f64;
    let mut sampled = Vec::with_capacity(max_points);
    let mut last_taken = None::<usize>;
    for bucket in 0..max_points {
        let index = if bucket == max_points - 1 {
            last_index
        } else {
            (bucket as f64 * step).round() as usize
        }
        .min(last_index);
        if last_taken == Some(index) {
            continue;
        }
        sampled.push(points[index].clone());
        last_taken = Some(index);
    }
    if last_taken != Some(last_index) {
        sampled.push(points[last_index].clone());
    }
    Value::Array(sampled)
}

fn percent_share(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

fn source_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.source_kind.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(source_kind, items)| {
                json!({
                    "source_kind": source_kind,
                    "summary": summarize_events(
                        &items,
                        items.last()
                            .map(|item| item.created_at_epoch_ms)
                            .unwrap_or_default(),
                        measurement,
                        contract,
                    ),
                })
            })
            .collect(),
    )
}

fn query_slice_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.query_type.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(query_type, items)| {
                let summary = summarize_events(
                    &items,
                    items
                        .last()
                        .map(|item| item.created_at_epoch_ms)
                        .unwrap_or_default(),
                    measurement,
                    contract,
                );
                json!({
                    "query_type": query_type,
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "task_success_like_counted_events": summary["task_success_like_counted_events"],
                    "answer_like_counted_events": summary["answer_like_counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "verified_task_like_savings_pct": summary["verified_task_like_savings_pct"],
                    "verified_answer_like_savings_pct": summary["verified_answer_like_savings_pct"],
                    "quality_ok_rate": summary["quality_ok_rate"],
                    "task_success_like_rate": summary["task_success_like_rate"],
                    "answer_like_rate": summary["answer_like_rate"],
                    "fallback_rate": summary["fallback_rate"],
                    "sample_count": summary["sample_count"],
                    "current_latency_ms": summary["current_latency_ms"],
                    "p50_latency_ms": summary["p50_latency_ms"],
                    "p95_latency_ms": summary["p95_latency_ms"],
                    "p99_latency_ms": summary["p99_latency_ms"],
                    "max_latency_ms": summary["max_latency_ms"],
                })
            })
            .collect(),
    )
}

fn baseline_strategy_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let allowed = allowed_baseline_classes()
        .into_iter()
        .collect::<HashSet<_>>();
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.baseline_strategy.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(baseline_strategy, items)| {
                let summary = summarize_events(
                    &items,
                    items
                        .last()
                        .map(|item| item.created_at_epoch_ms)
                        .unwrap_or_default(),
                    measurement,
                    contract,
                );
                json!({
                    "baseline_strategy": baseline_strategy,
                    "allowed_class": allowed.contains(baseline_strategy.as_str()),
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "quality_ok_rate": summary["quality_ok_rate"],
                    "coverage": summary["coverage"],
                })
            })
            .collect(),
    )
}

fn temperature_slice_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.cold_warm_state.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(state, items)| {
                let summary = summarize_events(
                    &items,
                    items
                        .last()
                        .map(|item| item.created_at_epoch_ms)
                        .unwrap_or_default(),
                    measurement,
                    contract,
                );
                json!({
                    "state": state,
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "median_recovery_tokens": summary["median_recovery_tokens"],
                    "sample_count": summary["sample_count"],
                    "current_latency_ms": summary["current_latency_ms"],
                    "p50_latency_ms": summary["p50_latency_ms"],
                    "p95_latency_ms": summary["p95_latency_ms"],
                    "p99_latency_ms": summary["p99_latency_ms"],
                    "max_latency_ms": summary["max_latency_ms"],
                })
            })
            .collect(),
    )
}

fn latency_slice_breakdown(events: &[TokenBudgetEvent]) -> Value {
    let mut grouped = BTreeMap::<String, Vec<f64>>::new();
    let mut current_latency = BTreeMap::<String, f64>::new();

    for event in events {
        if !event.latency_ms.is_finite() {
            continue;
        }
        grouped
            .entry("mixed".to_string())
            .or_default()
            .push(event.latency_ms);
        current_latency.insert("mixed".to_string(), event.latency_ms);

        let state = normalize_latency_state(&event.cold_warm_state);
        grouped
            .entry(state.to_string())
            .or_default()
            .push(event.latency_ms);
        current_latency.insert(state.to_string(), event.latency_ms);
    }

    let order = ["mixed", "hot", "cold", "benchmark"];
    let mut slices = Vec::new();
    for state in order {
        if let Some(values) = grouped.get(state) {
            slices.push(latency_slice_json(
                state,
                current_latency.get(state).copied().unwrap_or_default(),
                values,
            ));
        }
    }

    for (state, values) in grouped {
        if order.contains(&state.as_str()) {
            continue;
        }
        slices.push(latency_slice_json(
            &state,
            current_latency.get(&state).copied().unwrap_or_default(),
            &values,
        ));
    }

    Value::Array(slices)
}

fn latency_slice_json(state: &str, current_latency_ms: f64, values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    json!({
        "state": state,
        "display_name": latency_state_display_name(state),
        "sample_count": sorted.len(),
        "current_latency_ms": current_latency_ms,
        "p50_latency_ms": percentile_from_sorted(&sorted, 0.50),
        "p95_latency_ms": percentile_from_sorted(&sorted, 0.95),
        "p99_latency_ms": percentile_from_sorted(&sorted, 0.99),
        "max_latency_ms": sorted.last().copied().unwrap_or_default(),
    })
}

fn normalize_latency_state(state: &str) -> &'static str {
    match state {
        "warm" => "hot",
        "cold" => "cold",
        "benchmark" => "benchmark",
        _ => "mixed",
    }
}

fn latency_state_display_name(state: &str) -> &'static str {
    match state {
        "mixed" => "mix",
        "hot" => "hot",
        "cold" => "cold",
        "benchmark" => "benchmark",
        _ => "other",
    }
}

fn percentile_from_sorted(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let percentile = percentile.clamp(0.0, 1.0);
    let index = ((values.len() - 1) as f64 * percentile).ceil() as usize;
    values[index.min(values.len() - 1)]
}

fn event_to_json(event: &TokenBudgetEvent) -> Value {
    let excluded_reason_code = usage_excluded_reason_code(event);
    let mut object = serde_json::Map::new();
    object.insert(
        "created_at_epoch_ms".to_string(),
        Value::from(event.created_at_epoch_ms),
    );
    object.insert(
        "event_id".to_string(),
        Value::String(event.event_id.clone()),
    );
    object.insert(
        "correlation_id".to_string(),
        Value::String(event.correlation_id.clone()),
    );
    object.insert(
        "payload_origin".to_string(),
        Value::String(event.payload_origin.clone()),
    );
    object.insert(
        "session_id".to_string(),
        Value::String(event.session_id.clone()),
    );
    object.insert(
        "rolling_window_profile".to_string(),
        Value::String(event.rolling_window_profile.clone()),
    );
    object.insert(
        "timestamp_utc".to_string(),
        Value::from(event.timestamp_utc),
    );
    object.insert(
        "occurred_at_epoch_ms".to_string(),
        Value::from(event.occurred_at_epoch_ms),
    );
    object.insert(
        "ingested_at_epoch_ms".to_string(),
        Value::from(event.ingested_at_epoch_ms),
    );
    object.insert(
        "snapshot_kind".to_string(),
        Value::String(event.snapshot_kind.clone()),
    );
    object.insert(
        "source_kind".to_string(),
        Value::String(event.source_kind.clone()),
    );
    object.insert(
        "traffic_class".to_string(),
        Value::String(event.traffic_class.clone()),
    );
    object.insert(
        "measurement_scope".to_string(),
        Value::String(event.measurement_scope.clone()),
    );
    object.insert(
        "contract".to_string(),
        json!({
            "usage_event_schema_version": event.usage_event_schema_version.clone(),
            "settlement_statement_version": event.settlement_statement_version.clone(),
            "metering_event_schema_version": event.metering_event_schema_version.clone(),
            "usage_lifecycle_model_version": event.usage_lifecycle_model_version.clone(),
            "baseline_method_version": event.baseline_method_version.clone(),
            "quality_method_version": event.quality_method_version.clone(),
            "coverage_model_version": event.coverage_model_version.clone(),
            "metering_freshness_model_version": event.metering_freshness_model_version.clone(),
            "excluded_taxonomy_version": event.excluded_taxonomy_version.clone(),
            "dedup_contract_version": event.dedup_contract_version.clone(),
            "backfill_policy_version": event.backfill_policy_version.clone(),
            "correction_policy_version": event.correction_policy_version.clone(),
            "freeze_close_policy_version": event.freeze_close_policy_version.clone(),
            "late_arrival_policy_version": event.late_arrival_policy_version.clone(),
            "dispute_policy_version": event.dispute_policy_version.clone(),
            "settlement_lifecycle_model_version": event.settlement_lifecycle_model_version.clone(),
            "statement_period_governance_version": event.statement_period_governance_version.clone(),
            "adjustment_preview_model_version": event.adjustment_preview_model_version.clone(),
            "adjustment_request_schema_version": event.adjustment_request_schema_version.clone(),
            "adjustment_registry_version": event.adjustment_registry_version.clone(),
            "rate_card_binding_model_version": event.rate_card_binding_model_version.clone(),
            "telemetry_surface_split_version": event.telemetry_surface_split_version.clone(),
            "event_time_policy_version": event.event_time_policy_version.clone(),
            "billing_policy_version": event.billing_policy_version.clone(),
            "suitability_model_version": event.suitability_model_version.clone(),
            "billing_mode": event.billing_mode.clone(),
            "reconciliation_contract_version": event.reconciliation_contract_version.clone(),
            "margin_model_version": event.margin_model_version.clone(),
            "infra_cost_profile_version": event.infra_cost_profile_version.clone(),
            "contractual_evidence_pack_version": event.contractual_evidence_pack_version.clone(),
            "rate_card_version": event.rate_card_version.clone(),
            "currency_profile": event.currency_profile.clone(),
            "settlement_status": event.settlement_status.clone(),
        }),
    );
    object.insert(
        "usage_identity".to_string(),
        json!({
            "dedup_key": usage_dedup_key(&event.source_kind, &event.event_id),
            "idempotency_scope": "source_kind + event_id",
            "canonical_window_time_field": "occurred_at_epoch_ms",
            "event_id": event.event_id.clone(),
            "correlation_id": event.correlation_id.clone(),
        }),
    );
    object.insert(
        "usage_state".to_string(),
        json!({
            "lifecycle_status": usage_lifecycle_status(event),
            "reporting_layer": usage_reporting_layer(event),
            "included_in_verified_rollup": excluded_reason_code.is_none(),
            "excluded_reason_code": excluded_reason_code,
            "backfill_status": usage_backfill_status(event),
            "settlement_status": event.settlement_status.clone(),
        }),
    );
    object.insert("project".to_string(), Value::String(event.project.clone()));
    object.insert(
        "project_code".to_string(),
        Value::String(event.project.clone()),
    );
    object.insert(
        "namespace".to_string(),
        Value::String(event.namespace.clone()),
    );
    object.insert(
        "namespace_code".to_string(),
        Value::String(event.namespace.clone()),
    );
    object.insert("query".to_string(), Value::String(event.query.clone()));
    object.insert(
        "query_hash".to_string(),
        Value::String(event.query_hash.clone()),
    );
    object.insert(
        "query_type".to_string(),
        Value::String(event.query_type.clone()),
    );
    object.insert(
        "target_kind".to_string(),
        Value::String(event.target_kind.clone()),
    );
    object.insert(
        "baseline_hit_target".to_string(),
        Value::Bool(event.baseline_hit_target),
    );
    object.insert(
        "amai_hit_target".to_string(),
        Value::Bool(event.amai_hit_target),
    );
    object.insert(
        "cold_warm_state".to_string(),
        Value::String(event.cold_warm_state.clone()),
    );
    object.insert(
        "baseline_strategy".to_string(),
        Value::String(event.baseline_strategy.clone()),
    );
    object.insert(
        "retrieval_mode".to_string(),
        event
            .retrieval_mode
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "tokenizer".to_string(),
        Value::String(event.tokenizer.clone()),
    );
    object.insert("latency_ms".to_string(), Value::from(event.latency_ms));
    object.insert("saved_tokens".to_string(), Value::from(event.saved_tokens));
    object.insert("naive_tokens".to_string(), Value::from(event.naive_tokens));
    object.insert(
        "baseline_tokens".to_string(),
        Value::from(event.naive_tokens),
    );
    object.insert(
        "context_tokens".to_string(),
        Value::from(event.context_tokens),
    );
    object.insert(
        "delivered_tokens".to_string(),
        Value::from(event.context_tokens),
    );
    object.insert(
        "recovery_tokens".to_string(),
        Value::from(event.recovery_tokens),
    );
    object.insert(
        "effective_saved_tokens".to_string(),
        Value::from(event.effective_saved_tokens),
    );
    object.insert(
        "savings_factor".to_string(),
        Value::from(event.savings_factor),
    );
    object.insert(
        "savings_percent".to_string(),
        Value::from(event.savings_percent),
    );
    object.insert(
        "gross_savings_pct".to_string(),
        Value::from(event.savings_percent),
    );
    object.insert(
        "effective_savings_percent".to_string(),
        Value::from(event.effective_savings_percent),
    );
    object.insert("quality_ok".to_string(), Value::Bool(event.quality_ok));
    object.insert(
        "quality_score".to_string(),
        Value::from(event.quality_score),
    );
    object.insert(
        "answer_like_proxy".to_string(),
        Value::Bool(is_answer_like_event(event)),
    );
    object.insert(
        "quality_method".to_string(),
        Value::String(event.quality_method.clone()),
    );
    object.insert(
        "quality_tier".to_string(),
        Value::String(event.quality_tier.clone()),
    );
    object.insert(
        "head_hit_target".to_string(),
        Value::Bool(event.head_hit_target),
    );
    object.insert(
        "needed_followup".to_string(),
        Value::Bool(event.needed_followup),
    );
    object.insert(
        "followup_count".to_string(),
        Value::from(event.followup_count),
    );
    object.insert(
        "followup_of_event_id".to_string(),
        event
            .followup_of_event_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "resolved_by_event_id".to_string(),
        event
            .resolved_by_event_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "fallback_triggered".to_string(),
        Value::Bool(event.fallback_triggered),
    );
    object.insert(
        "fallback_count".to_string(),
        Value::from(event.fallback_count),
    );
    object.insert(
        "document_hits".to_string(),
        Value::from(event.document_hits),
    );
    object.insert(
        "symbol_hits_count".to_string(),
        Value::from(event.symbol_hits_count),
    );
    object.insert("file_hits".to_string(), Value::from(event.file_hits));
    object.insert(
        "sources_count".to_string(),
        Value::from(event.sources_count),
    );
    object.insert("chunks_count".to_string(), Value::from(event.chunks_count));
    object.insert(
        "pack_token_count".to_string(),
        Value::from(event.pack_token_count),
    );
    object.insert(
        "deduped_token_count".to_string(),
        Value::from(event.deduped_token_count),
    );
    object.insert(
        "whole_cycle_observed".to_string(),
        json!({
            "client_prompt_tokens": event.client_prompt_tokens,
            "assistant_generation_tokens": event.assistant_generation_tokens,
            "tool_overhead_tokens": event.tool_overhead_tokens,
            "continuity_restore_tokens": event.continuity_restore_tokens,
        }),
    );
    Value::Object(object)
}

fn build_event_payload(
    payload: &Value,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    source_kind: &str,
    payload_origin: &str,
) -> Result<Value> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let query = payload["query"].as_str().unwrap_or_default();
    let query_type = derive_query_type(query);
    let baseline_strategy = derive_baseline_strategy(query_type);
    let naive_scope = collect_naive_scope(
        payload,
        measurement.naive_limit_files,
        measurement.naive_max_bytes_per_file,
        baseline_strategy,
        query,
    )?;
    let naive_prompt = render_naive_scope_prompt(payload, &naive_scope);
    let context_prompt = render_context_pack_prompt(payload);
    let naive_tokens = tokenizer.encode_with_special_tokens(&naive_prompt).len();
    let context_tokens = tokenizer.encode_with_special_tokens(&context_prompt).len();
    let saved_tokens = naive_tokens.saturating_sub(context_tokens);
    let recovery_tokens = 0_u64;
    let effective_saved_tokens =
        naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64);
    let savings_factor = if context_tokens == 0 {
        naive_tokens as f64
    } else {
        naive_tokens as f64 / context_tokens as f64
    };
    let savings_percent = if naive_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / naive_tokens as f64
    };
    let effective_savings_percent =
        percent_from_signed(effective_saved_tokens, naive_tokens as u64);
    let quality = derive_quality_verdict(payload, query_type, &naive_scope);
    let fallback_count = count_lexical_fallback_chunks(payload) as u64;
    let fallback_triggered = fallback_count > 0;
    let document_hits = payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len) as u64;
    let symbol_hits = payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len) as u64;
    let file_hits = unique_file_hit_count(payload) as u64;
    let sources_count = count_sources(payload) as u64;
    let chunks_count = count_chunks(payload) as u64;
    let traffic_class = derive_traffic_class(source_kind);
    let context_pack_id = payload["context_pack_id"].as_str().map(ToOwned::to_owned);
    let event_id = Uuid::new_v4().to_string();
    let timestamp_utc = current_epoch_ms()?;
    let correlation_id = context_pack_id.clone().unwrap_or_else(|| event_id.clone());
    let latency_ms = total_latency_ms(payload);
    let whole_cycle_observed = &payload["whole_cycle_observed"];
    let client_prompt_tokens = whole_cycle_observed["client_prompt_tokens"]
        .as_u64()
        .or_else(|| {
            if query.is_empty() {
                None
            } else {
                Some(tokenizer.encode_with_special_tokens(query).len() as u64)
            }
        });
    let assistant_generation_tokens = whole_cycle_observed["assistant_generation_tokens"].as_u64();
    let tool_overhead_tokens = whole_cycle_observed["tool_overhead_tokens"].as_u64();
    let continuity_restore_tokens = whole_cycle_observed["continuity_restore_tokens"].as_u64();

    Ok(json!({
        "token_budget_event": {
            "event_id": event_id,
            "correlation_id": correlation_id,
            "context_pack_id": context_pack_id,
            "timestamp_utc": timestamp_utc,
            "occurred_at_epoch_ms": timestamp_utc,
            "ingested_at_epoch_ms": timestamp_utc,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "measurement_scope": "retrieval_lower_bound",
            "payload_origin": payload_origin,
            "contract": token_contract_metadata_json(contract),
            "project": payload["project"]["code"].clone(),
            "project_code": payload["project"]["code"].clone(),
            "namespace": payload["namespace"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "query": payload["query"].clone(),
            "query_hash": hex_sha256(query.as_bytes()),
            "query_type": query_type,
            "target_kind": quality.target_kind,
            "baseline_hit_target": quality.baseline_hit_target,
            "amai_hit_target": quality.amai_hit_target,
            "cold_warm_state": if payload["retrieval_runtime"]["cache_hit"].as_bool().unwrap_or(false) {
                "warm"
            } else {
                "cold"
            },
            "baseline_strategy": baseline_strategy,
            "retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "tokenizer": measurement.tokenizer,
            "latency_ms": latency_ms,
            "baseline_tokens": naive_tokens,
            "delivered_tokens": context_tokens,
            "gross_savings_pct": savings_percent,
            "naive_limit_files": measurement.naive_limit_files,
            "naive_max_bytes_per_file": measurement.naive_max_bytes_per_file,
            "visible_projects": payload["visible_projects"].clone(),
            "naive_scope": {
                "files_considered": naive_scope.files.len(),
                "files": naive_scope.files,
                "rendered_bytes": naive_prompt.len(),
                "tokens": naive_tokens,
            },
            "context_pack_render": {
                "rendered_bytes": context_prompt.len(),
                "tokens": context_tokens,
            },
            "whole_cycle_observed": {
                "client_prompt_tokens": client_prompt_tokens,
                "assistant_generation_tokens": assistant_generation_tokens,
                "tool_overhead_tokens": tool_overhead_tokens,
                "continuity_restore_tokens": continuity_restore_tokens,
            },
            "recovery": {
                "recovery_tokens": recovery_tokens,
                "fallback_triggered": fallback_triggered,
                "fallback_count": fallback_count,
            },
            "quality": {
                "quality_ok": quality.quality_ok,
                "quality_score": quality.quality_score,
                "quality_method": quality.quality_method,
                "quality_tier": quality.quality_tier,
                "head_hit_target": quality.head_hit_target,
            },
            "followup": {
                "needed_followup": quality.needed_followup,
                "followup_count": quality.followup_count,
                "followup_of_event_id": Value::Null,
                "resolved_by_event_id": Value::Null,
            },
            "shape": {
                "document_hits": document_hits,
                "symbol_hits": symbol_hits,
                "file_hits": file_hits,
                "sources_count": sources_count,
                "chunks_count": chunks_count,
                "pack_token_count": context_tokens,
                "deduped_token_count": context_tokens,
            },
            "savings": {
                "saved_tokens": saved_tokens,
                "effective_saved_tokens": effective_saved_tokens,
                "savings_factor": savings_factor,
                "savings_percent": savings_percent,
                "effective_savings_percent": effective_savings_percent,
            }
        }
    }))
}

fn observed_tool_overhead_payload(text: &str, structured_content: &Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": text
        }],
        "structuredContent": structured_content
    })
}

fn count_tool_overhead_tokens(
    measurement: &MeasurementConfig,
    text: &str,
    structured_content: &Value,
) -> Result<u64> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let payload = observed_tool_overhead_payload(text, structured_content);
    let rendered =
        serde_json::to_string(&payload).context("failed to serialize tool overhead payload")?;
    Ok(tokenizer.encode_with_special_tokens(&rendered).len() as u64)
}

fn count_cli_context_pack_output_overhead_tokens(
    measurement: &MeasurementConfig,
    output_json: &str,
    delivered_tokens: u64,
) -> Result<u64> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let total_output_tokens = tokenizer.encode_with_special_tokens(output_json).len() as u64;
    Ok(total_output_tokens.saturating_sub(delivered_tokens))
}

async fn latest_token_budget_snapshot_for_context_pack(
    db: &Client,
    context_pack_id: &str,
) -> Result<Option<ObservabilitySnapshotRecord>> {
    let mut rows = latest_token_budget_snapshots_for_context_packs(
        db,
        &std::iter::once(context_pack_id.to_string()).collect(),
    )
    .await?;
    Ok(rows.remove(context_pack_id))
}

async fn latest_token_budget_snapshots_for_context_packs(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, ObservabilitySnapshotRecord>> {
    if context_pack_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], Some(4096))
            .await?;
    let mut latest = BTreeMap::<String, ObservabilitySnapshotRecord>::new();
    for row in rows {
        let Some(context_pack_id) = row.payload["token_budget_event"]["context_pack_id"]
            .as_str()
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        if !context_pack_ids.contains(&context_pack_id) {
            continue;
        }
        match latest.get(&context_pack_id) {
            Some(existing) if existing.created_at_epoch_ms >= row.created_at_epoch_ms => {}
            _ => {
                latest.insert(context_pack_id, row);
            }
        }
    }
    Ok(latest)
}

async fn stored_context_pack_payload_json(
    db: &Client,
    context_pack_id: &str,
) -> Result<Option<String>> {
    let Ok(context_pack_uuid) = Uuid::parse_str(context_pack_id) else {
        return Ok(None);
    };
    let row = db
        .query_opt(
            "SELECT payload FROM ami.context_packs WHERE context_pack_id = $1 LIMIT 1",
            &[&context_pack_uuid],
        )
        .await
        .context("failed to load stored context pack payload")?;
    let Some(row) = row else {
        return Ok(None);
    };
    let payload: Value = row.get(0);
    Ok(Some(
        serde_json::to_string(&payload).context("failed to serialize stored context pack")?,
    ))
}

async fn attach_context_pack_whole_cycle_observed(
    db: &Client,
    context_pack_id: &str,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) -> Result<Option<Value>> {
    if context_pack_id.trim().is_empty() {
        bail!("context_pack_id must not be empty");
    }
    if client_prompt_tokens.is_none()
        && assistant_generation_tokens.is_none()
        && tool_overhead_tokens.is_none()
        && continuity_restore_tokens.is_none()
    {
        bail!("whole-cycle attach requires at least one observed token field");
    }
    let Some(row) = latest_token_budget_snapshot_for_context_pack(db, context_pack_id).await? else {
        return Ok(None);
    };
    attach_whole_cycle_observed_to_snapshot(
        db,
        &row,
        Some(json!({ "context_pack_id": context_pack_id })),
        client_prompt_tokens,
        assistant_generation_tokens,
        tool_overhead_tokens,
        continuity_restore_tokens,
    )
    .await
}

async fn attach_whole_cycle_observed_to_snapshot(
    db: &Client,
    row: &ObservabilitySnapshotRecord,
    selector: Option<Value>,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) -> Result<Option<Value>> {
    let mut payload = row.payload.clone();
    let (
        event_id,
        correlation_id,
        source_kind,
        traffic_class,
        measurement_scope,
        updated_fields,
        retained_fields,
        whole_cycle_observed,
        attached,
    ) = {
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event"))?;
        let mut updated_fields = Vec::new();
        let mut retained_fields = Vec::new();
        {
            let whole_cycle = ensure_nested_object(node, "whole_cycle_observed")?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "client_prompt_tokens",
                client_prompt_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "assistant_generation_tokens",
                assistant_generation_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "tool_overhead_tokens",
                tool_overhead_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "continuity_restore_tokens",
                continuity_restore_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
        }
        (
            node.get("event_id").cloned().unwrap_or(Value::Null),
            node.get("correlation_id").cloned().unwrap_or(Value::Null),
            node.get("source_kind").cloned().unwrap_or(Value::Null),
            node.get("traffic_class").cloned().unwrap_or(Value::Null),
            node.get("measurement_scope")
                .cloned()
                .unwrap_or(Value::Null),
            updated_fields.clone(),
            retained_fields.clone(),
            node.get("whole_cycle_observed")
                .cloned()
                .unwrap_or(Value::Null),
            !updated_fields.is_empty(),
        )
    };
    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload).await?;
    Ok(Some(json!({
        "whole_cycle_observed_attach": {
            "selector": selector.unwrap_or(Value::Null),
            "snapshot_id": row.snapshot_id,
            "event_id": event_id,
            "correlation_id": correlation_id,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "measurement_scope": measurement_scope,
            "updated_fields": updated_fields,
            "retained_fields": retained_fields,
            "whole_cycle_observed": whole_cycle_observed,
            "attached": attached,
            "note": "Conflicting overwrite is fail-closed; reattaching the same observed value is allowed."
        }
    })))
}

fn apply_whole_cycle_observed_token(
    whole_cycle: &mut serde_json::Map<String, Value>,
    field: &str,
    new_value: Option<u64>,
    updated_fields: &mut Vec<String>,
    retained_fields: &mut Vec<String>,
) -> Result<()> {
    let Some(new_value) = new_value else {
        return Ok(());
    };
    match whole_cycle.get(field).and_then(Value::as_u64) {
        Some(existing) if existing == new_value => {
            retained_fields.push(field.to_string());
        }
        Some(existing) => {
            bail!(
                "conflicting whole-cycle observed overwrite for {}: existing={} new={}",
                field,
                existing,
                new_value
            );
        }
        None => {
            whole_cycle.insert(field.to_string(), Value::from(new_value));
            updated_fields.push(field.to_string());
        }
    }
    Ok(())
}

fn build_continuity_restore_observed_event(
    project_code: &str,
    namespace_code: &str,
    source_kind: &str,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    prompt_text: &str,
    continuity_restore_tokens: u64,
) -> Result<Value> {
    let timestamp_utc = current_epoch_ms()?;
    let event_id = Uuid::new_v4().to_string();
    let traffic_class = derive_traffic_class(source_kind);
    Ok(json!({
        "token_budget_event": {
            "event_id": event_id,
            "correlation_id": event_id,
            "context_pack_id": Value::Null,
            "timestamp_utc": timestamp_utc,
            "occurred_at_epoch_ms": timestamp_utc,
            "ingested_at_epoch_ms": timestamp_utc,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "measurement_scope": "whole_cycle_observed_lower_bound",
            "payload_origin": "continuity_startup_observed_lower_bound",
            "contract": token_contract_metadata_json(contract),
            "project": project_code,
            "project_code": project_code,
            "namespace": namespace_code,
            "namespace_code": namespace_code,
            "query": "CHAT_START_RESTORE",
            "query_hash": hex_sha256(prompt_text.as_bytes()),
            "query_type": "continuity_restore",
            "target_kind": "continuity_restore",
            "baseline_hit_target": false,
            "amai_hit_target": true,
            "cold_warm_state": "observed_only",
            "baseline_strategy": "observed_only",
            "retrieval_mode": Value::Null,
            "tokenizer": measurement.tokenizer,
            "latency_ms": 0,
            "baseline_tokens": 0,
            "delivered_tokens": 0,
            "gross_savings_pct": 0.0,
            "naive_limit_files": measurement.naive_limit_files,
            "naive_max_bytes_per_file": measurement.naive_max_bytes_per_file,
            "visible_projects": [project_code],
            "naive_scope": {
                "files_considered": 0,
                "files": [],
                "rendered_bytes": 0,
                "tokens": 0,
            },
            "context_pack_render": {
                "rendered_bytes": 0,
                "tokens": 0,
            },
            "whole_cycle_observed": {
                "client_prompt_tokens": Value::Null,
                "assistant_generation_tokens": Value::Null,
                "tool_overhead_tokens": Value::Null,
                "continuity_restore_tokens": continuity_restore_tokens,
            },
            "recovery": {
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0,
            },
            "quality": {
                "quality_ok": true,
                "quality_score": 1.0,
                "quality_method": "continuity_restore_observed",
                "quality_tier": "observed_only",
                "head_hit_target": true,
            },
            "followup": {
                "needed_followup": false,
                "followup_count": 0,
                "followup_of_event_id": Value::Null,
                "resolved_by_event_id": Value::Null,
            },
            "shape": {
                "document_hits": 0,
                "symbol_hits": 0,
                "file_hits": 0,
                "sources_count": 0,
                "chunks_count": 0,
                "pack_token_count": 0,
                "deduped_token_count": 0,
            },
            "savings": {
                "saved_tokens": 0,
                "effective_saved_tokens": 0,
                "savings_factor": 0.0,
                "savings_percent": 0.0,
                "effective_savings_percent": 0.0,
            },
            "continuity_restore_prompt_length_chars": prompt_text.len(),
            "continuity_restore_prompt_sha256": hex_sha256(prompt_text.as_bytes()),
        }
    }))
}

fn derive_traffic_class(source_kind: &str) -> String {
    if source_kind.starts_with("live_") {
        "live".to_string()
    } else if source_kind.starts_with("verify_") {
        "verify".to_string()
    } else if source_kind.starts_with("proof_") {
        "proof".to_string()
    } else if source_kind.starts_with("benchmark_") {
        "benchmark".to_string()
    } else {
        "unknown".to_string()
    }
}

fn include_traffic_class_in_report(traffic_class: &str, include_verify_events: bool) -> bool {
    include_verify_events || traffic_class == "live"
}

pub(crate) fn derive_baseline_strategy(query_type: &str) -> &'static str {
    match query_type {
        "onboarding_query" => "legacy_pre_amai",
        "config_lookup" | "symbol_lookup" | "code_lookup" => "ide_search_top_files",
        "docs_lookup" | "cross_file_trace" => "grep_top_files",
        "architecture_question" | "bugfix_context" => "semantic_top_k",
        _ => "naive_top_files",
    }
}

pub(crate) fn derive_query_type(query: &str) -> &'static str {
    let lowered = query.to_lowercase();

    if [
        "onboarding",
        "getting started",
        "setup",
        "install",
        "как подключ",
        "как установить",
        "как запустить",
        "как начать",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "onboarding_query"
    } else if [
        "config",
        "конфиг",
        "настрой",
        ".env",
        "yaml",
        "toml",
        "json",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "config_lookup"
    } else if [
        "bug",
        "fix",
        "ошиб",
        "не работает",
        "падает",
        "сломал",
        "почин",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "bugfix_context"
    } else if ["архитект", "architecture", "контур", "как устроен", "зачем"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        "architecture_question"
    } else if [
        "trace",
        "call stack",
        "flow",
        "цепоч",
        "где вызыва",
        "откуда приходит",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "cross_file_trace"
    } else if [
        "symbol",
        "struct",
        "enum",
        "trait",
        "type",
        "тип",
        "функц",
        "method",
        "класс",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "symbol_lookup"
    } else if ["docs", "readme", "guide", "док", "документац"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        "docs_lookup"
    } else {
        "code_lookup"
    }
}

fn derive_quality_verdict(
    payload: &Value,
    query_type: &str,
    naive_scope: &NaiveScope,
) -> QualityVerdict {
    let exact_hits = payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len);
    let symbol_hits = payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len);
    let lexical_hits = payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_hits = payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_guard_abstained = payload["quality"]["semantic_guard"]["abstained"]
        .as_bool()
        .unwrap_or(false);
    let total_hits = exact_hits + symbol_hits + lexical_hits + semantic_hits;
    let query_terms = extract_query_terms(payload["query"].as_str().unwrap_or_default());
    let target_kind = match query_type {
        "onboarding_query" | "docs_lookup" => "document",
        "config_lookup" | "code_lookup" => "file",
        "symbol_lookup" => "symbol",
        "cross_file_trace" => "cross_file_trace",
        "architecture_question" | "bugfix_context" => "evidence_bundle",
        _ => "file",
    };
    let baseline_hit_target = !naive_scope.files.is_empty();
    let amai_hit_target = match target_kind {
        "document" => exact_hits > 0 || lexical_hits > 0,
        "file" => exact_hits > 0 || lexical_hits > 0 || symbol_hits > 0,
        "symbol" => symbol_hits > 0,
        "cross_file_trace" => {
            (symbol_hits > 0 && lexical_hits > 0)
                || (symbol_hits + lexical_hits + semantic_hits >= 2)
        }
        "evidence_bundle" => total_hits >= 2,
        _ => total_hits > 0,
    };
    let head_hit_target = top_hit_matches_task(payload, target_kind, &query_terms);
    let quality_ok = baseline_hit_target && amai_hit_target && !semantic_guard_abstained;
    let task_success_proxy = quality_ok
        && match target_kind {
            "document" | "file" | "symbol" => head_hit_target,
            "cross_file_trace" => head_hit_target && total_hits >= 2,
            "evidence_bundle" => head_hit_target && total_hits >= 3,
            _ => head_hit_target,
        };
    let answer_like_proxy = answer_like_from_counts(
        target_kind,
        head_hit_target,
        exact_hits,
        symbol_hits,
        lexical_hits,
        semantic_hits,
    ) && task_success_proxy;
    let quality_score = match target_kind {
        "cross_file_trace" => {
            if answer_like_proxy {
                1.0
            } else if task_success_proxy {
                0.92
            } else if quality_ok {
                0.85
            } else if total_hits > 0 && !semantic_guard_abstained {
                0.5
            } else {
                0.0
            }
        }
        "evidence_bundle" => {
            if answer_like_proxy {
                1.0
            } else if task_success_proxy {
                0.94
            } else if quality_ok {
                0.9
            } else if total_hits > 0 && !semantic_guard_abstained {
                0.6
            } else {
                0.0
            }
        }
        _ => {
            if answer_like_proxy {
                1.0
            } else if task_success_proxy {
                0.9
            } else if quality_ok {
                0.8
            } else if total_hits > 0 && !semantic_guard_abstained {
                0.4
            } else {
                0.0
            }
        }
    };
    let (quality_method, quality_tier) = if answer_like_proxy {
        ("hybrid_answer_proxy", "answer_proxy")
    } else if task_success_proxy {
        ("hybrid_task_proxy", "task_proxy")
    } else if quality_ok {
        ("hybrid_retrieval_parity", "retrieval")
    } else if total_hits > 0 && !semantic_guard_abstained {
        ("hybrid_partial_retrieval", "partial")
    } else {
        ("hybrid_retrieval_parity", "retrieval")
    };
    QualityVerdict {
        target_kind,
        baseline_hit_target,
        amai_hit_target,
        quality_ok,
        quality_score,
        quality_method,
        quality_tier,
        head_hit_target,
        needed_followup: !quality_ok,
        followup_count: 0,
    }
}

fn answer_like_from_counts(
    target_kind: &str,
    head_hit_target: bool,
    exact_hits: usize,
    symbol_hits: usize,
    lexical_hits: usize,
    semantic_hits: usize,
) -> bool {
    if !head_hit_target {
        return false;
    }
    let total_hits = exact_hits + symbol_hits + lexical_hits + semantic_hits;
    let nonzero_sections = [exact_hits, symbol_hits, lexical_hits, semantic_hits]
        .into_iter()
        .filter(|count| *count > 0)
        .count();
    match target_kind {
        "document" => exact_hits > 0,
        "file" => exact_hits > 0 || lexical_hits > 0,
        "symbol" => symbol_hits > 0,
        "cross_file_trace" => symbol_hits > 0 && lexical_hits > 0 && total_hits >= 3,
        "evidence_bundle" => total_hits >= 4 && nonzero_sections >= 2,
        _ => total_hits > 0,
    }
}

fn is_answer_like_event(event: &TokenBudgetEvent) -> bool {
    if !event.quality_ok {
        return false;
    }
    if matches!(
        event.quality_tier.as_str(),
        "answer_proxy" | "answer_success_recovered"
    ) {
        return true;
    }
    match event.target_kind.as_str() {
        "document" => event.head_hit_target && event.document_hits > 0,
        "file" => event.head_hit_target && event.file_hits > 0,
        "symbol" => event.head_hit_target && event.symbol_hits_count > 0,
        "cross_file_trace" => {
            event.head_hit_target && event.symbol_hits_count > 0 && event.chunks_count >= 2
        }
        "evidence_bundle" => {
            event.head_hit_target && event.sources_count >= 2 && event.chunks_count >= 3
        }
        _ => event.head_hit_target && event.sources_count > 0,
    }
}

fn top_hit_matches_task(payload: &Value, target_kind: &str, query_terms: &[String]) -> bool {
    let items = top_retrieval_items(payload, 3);
    items
        .into_iter()
        .any(|item| retrieval_item_matches_task(item, target_kind, query_terms))
}

fn top_retrieval_items(payload: &Value, limit: usize) -> Vec<&Value> {
    let retrieval = &payload["retrieval"];
    let mut items = Vec::new();
    for section in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
    ] {
        for item in retrieval[section].as_array().into_iter().flatten() {
            items.push(item);
            if items.len() >= limit {
                return items;
            }
        }
    }
    items
}

fn retrieval_item_matches_task(item: &Value, target_kind: &str, query_terms: &[String]) -> bool {
    let kind_matches = match target_kind {
        "document" => {
            item.get("snippet").is_some()
                || item.get("content").is_some()
                || ledger_item_relative_path(item).is_some_and(is_document_like_path)
        }
        "file" => ledger_item_relative_path(item).is_some(),
        "symbol" => item["name"].as_str().is_some(),
        "cross_file_trace" => {
            ledger_item_relative_path(item).is_some() || item["name"].as_str().is_some()
        }
        "evidence_bundle" => {
            ledger_item_relative_path(item).is_some() || item["content"].as_str().is_some()
        }
        _ => true,
    };
    kind_matches && retrieval_item_matches_query(item, query_terms)
}

fn retrieval_item_matches_query(item: &Value, query_terms: &[String]) -> bool {
    if query_terms.is_empty() {
        return false;
    }
    let mut haystacks = Vec::new();
    if let Some(value) = ledger_item_relative_path(item) {
        haystacks.push(value.to_lowercase());
    }
    if let Some(value) = item["name"].as_str() {
        haystacks.push(value.to_lowercase());
    }
    if let Some(value) = item["snippet"].as_str() {
        haystacks.push(value.to_lowercase());
    }
    if let Some(value) = item["content"].as_str() {
        haystacks.push(value.to_lowercase());
    }
    haystacks
        .into_iter()
        .any(|haystack| query_terms.iter().any(|term| haystack.contains(term)))
}

fn is_document_like_path(path: &str) -> bool {
    let lowered = path.to_lowercase();
    lowered.ends_with(".md")
        || lowered.ends_with(".txt")
        || lowered.contains("readme")
        || lowered.contains("docs/")
        || lowered.contains("guide")
}

fn count_lexical_fallback_chunks(payload: &Value) -> usize {
    payload["retrieval"]["semantic_chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|chunk| chunk["retrieval_strategy"].as_str() == Some("lexical_fallback"))
        .count()
}

fn count_sources(payload: &Value) -> usize {
    let retrieval = &payload["retrieval"];
    retrieval["exact_documents"].as_array().map_or(0, Vec::len)
        + retrieval["symbol_hits"].as_array().map_or(0, Vec::len)
        + retrieval["lexical_chunks"].as_array().map_or(0, Vec::len)
        + retrieval["semantic_chunks"].as_array().map_or(0, Vec::len)
}

fn unique_file_hit_count(payload: &Value) -> usize {
    let mut files = HashSet::new();
    for section in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
    ] {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let project_code = item["project_code"]
                .as_str()
                .or_else(|| item["provenance"]["source_project"].as_str())
                .unwrap_or_default();
            let relative_path = item["relative_path"]
                .as_str()
                .or_else(|| item["provenance"]["path"].as_str())
                .unwrap_or_default();
            if !project_code.is_empty() || !relative_path.is_empty() {
                files.insert(format!("{project_code}::{relative_path}"));
            }
        }
    }
    files.len()
}

fn count_chunks(payload: &Value) -> usize {
    let retrieval = &payload["retrieval"];
    retrieval["lexical_chunks"].as_array().map_or(0, Vec::len)
        + retrieval["semantic_chunks"].as_array().map_or(0, Vec::len)
}

fn current_epoch_ms() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64)
}

fn total_latency_ms(payload: &Value) -> f64 {
    let runtime = &payload["retrieval_runtime"];
    if let Some(value) = runtime["total_ms"].as_f64() {
        return value;
    }
    [
        "resolve_scope_ms",
        "cache_lookup_ms",
        "exact_lookup_ms",
        "symbol_lookup_ms",
        "lexical_lookup_ms",
        "query_embed_ms",
        "semantic_search_ms",
        "semantic_hydrate_ms",
        "serialize_ms",
        "persist_ms",
    ]
    .iter()
    .map(|key| runtime[*key].as_f64().unwrap_or(0.0))
    .sum()
}

fn percent_from_signed(saved_tokens: i64, baseline_tokens: u64) -> f64 {
    if baseline_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / baseline_tokens as f64
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn collect_naive_scope(
    payload: &Value,
    limit_files: usize,
    max_bytes_per_file: usize,
    baseline_strategy: &str,
    query: &str,
) -> Result<NaiveScope> {
    let mut files = Vec::new();
    let strategy_files =
        collect_payload_scope_files_by_strategy(payload, baseline_strategy, limit_files)?;
    if !strategy_files.is_empty() {
        for (project_code, repo_root, path) in strategy_files {
            files.push(read_scope_file(
                &project_code,
                &repo_root,
                &path,
                max_bytes_per_file,
            )?);
        }
    } else {
        for project in payload["visible_projects"].as_array().into_iter().flatten() {
            let Some(project_code) = project["project_code"].as_str() else {
                continue;
            };
            let Some(repo_root) = project["repo_root"].as_str() else {
                continue;
            };
            for path in collect_scope_files_by_strategy(
                Path::new(repo_root),
                query,
                baseline_strategy,
                limit_files,
                max_bytes_per_file.min(16 * 1024),
            )? {
                files.push(read_scope_file(
                    project_code,
                    Path::new(repo_root),
                    &path,
                    max_bytes_per_file,
                )?);
            }
        }
    }

    files.sort_by(|left, right| {
        left.project_code
            .cmp(&right.project_code)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    if limit_files > 0 {
        files.truncate(limit_files);
    }

    let metadata = files
        .iter()
        .map(|file| {
            json!({
                "project_code": file.project_code,
                "relative_path": file.relative_path,
                "original_bytes": file.original_bytes,
                "bytes_used": file.bytes_used,
                "truncated": file.truncated,
            })
        })
        .collect();

    Ok(NaiveScope {
        files: metadata,
        rendered_files: files,
    })
}

fn read_scope_file(
    project_code: &str,
    repo_root: &Path,
    path: &Path,
    max_bytes_per_file: usize,
) -> Result<NaiveScopeFile> {
    let relative_path = path
        .strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string();
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read naive scope file {}", path.display()))?;
    let original_bytes = bytes.len();
    let bytes_used = original_bytes.min(max_bytes_per_file);
    let content = safe_lossy_prefix(&bytes, bytes_used);
    Ok(NaiveScopeFile {
        project_code: project_code.to_string(),
        relative_path,
        original_bytes,
        bytes_used: content.len(),
        truncated: original_bytes > content.len(),
        content,
    })
}

fn collect_payload_scope_files_by_strategy(
    payload: &Value,
    baseline_strategy: &str,
    limit_files: usize,
) -> Result<Vec<(String, PathBuf, PathBuf)>> {
    let sections: &[&str] = match baseline_strategy {
        "ide_search_top_files" => &["exact_documents", "symbol_hits", "lexical_chunks"],
        "semantic_top_k" => &["semantic_chunks"],
        _ => return Ok(Vec::new()),
    };
    let repo_roots = payload["visible_projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            Some((
                project["project_code"].as_str()?.to_string(),
                PathBuf::from(project["repo_root"].as_str()?),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    for section in sections {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(project_code) = ledger_item_project_code(item) else {
                continue;
            };
            let Some(relative_path) = ledger_item_relative_path(item) else {
                continue;
            };
            let Some(repo_root) = repo_roots.get(project_code) else {
                continue;
            };
            let path = repo_root.join(relative_path);
            if !path.is_file() {
                continue;
            }
            if seen.insert(format!("{project_code}::{relative_path}")) {
                files.push((project_code.to_string(), repo_root.clone(), path));
            }
        }
    }
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_scope_files_by_strategy(
    root: &Path,
    query: &str,
    baseline_strategy: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    match baseline_strategy {
        "grep_top_files" => {
            collect_grep_scope_files(root, query, limit_files, score_bytes_per_file)
        }
        "legacy_pre_amai" => {
            collect_legacy_scope_files(root, query, limit_files, score_bytes_per_file)
        }
        _ => collect_scope_files(root, limit_files),
    }
}

fn collect_scope_files(root: &Path, limit_files: usize) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        bail!("visible project root does not exist: {}", root.display());
    }
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);
    let mut files = builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .filter(|path| language::detect(path).is_some())
        .collect::<Vec<_>>();
    files.sort();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_grep_scope_files(
    root: &Path,
    query: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    let files = collect_scope_files(root, 0)?;
    let terms = extract_query_terms(query);
    if terms.is_empty() {
        return collect_scope_files(root, limit_files);
    }

    let mut scored = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string()
            .to_lowercase();
        let mut score = text_match_score(&relative, &terms) * 8;

        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read grep scope file {}", path.display()))?;
        let content = safe_lossy_prefix(&bytes, score_bytes_per_file).to_lowercase();
        score += text_match_score(&content, &terms);

        if score > 0 {
            scored.push((score, path));
        }
    }

    if scored.is_empty() {
        return collect_scope_files(root, limit_files);
    }

    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut files = scored.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_legacy_scope_files(
    root: &Path,
    query: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    let files = collect_scope_files(root, 0)?;
    let terms = extract_query_terms(query);
    let mut scored = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string()
            .to_lowercase();
        let docs_bias = if relative.contains("readme")
            || relative.contains("docs/")
            || relative.contains("guide")
            || relative.contains("install")
            || relative.contains("setup")
        {
            12
        } else {
            0
        };
        let mut score = docs_bias + text_match_score(&relative, &terms) * 6;
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read legacy scope file {}", path.display()))?;
        let content = safe_lossy_prefix(&bytes, score_bytes_per_file).to_lowercase();
        score += text_match_score(&content, &terms);
        if score > 0 {
            scored.push((score, path));
        }
    }
    if scored.is_empty() {
        return collect_scope_files(root, limit_files);
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut files = scored.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn ledger_item_project_code(item: &Value) -> Option<&str> {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
}

fn ledger_item_relative_path(item: &Value) -> Option<&str> {
    item["relative_path"]
        .as_str()
        .or_else(|| item["provenance"]["path"].as_str())
}

fn extract_query_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '.')
        .filter(|term| term.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn text_match_score(haystack: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .map(|term| haystack.match_indices(term).count())
        .sum()
}

fn safe_lossy_prefix(bytes: &[u8], max_bytes: usize) -> String {
    let slice = &bytes[..bytes.len().min(max_bytes)];
    String::from_utf8_lossy(slice).into_owned()
}

fn render_naive_scope_prompt(payload: &Value, scope: &NaiveScope) -> String {
    let mut prompt = String::new();
    prompt.push_str("NAIVE_SCOPE\n");
    prompt.push_str(
        "This bundle represents the visible project scope without retrieval reduction.\n",
    );
    prompt.push_str("Query: ");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push_str("\nVisible projects:\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push_str("- ");
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str(" :: ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    for file in &scope.rendered_files {
        prompt.push_str("## PROJECT ");
        prompt.push_str(&file.project_code);
        prompt.push('\n');
        prompt.push_str("### FILE ");
        prompt.push_str(&file.relative_path);
        prompt.push('\n');
        prompt.push_str(&file.content);
        prompt.push_str("\n\n");
    }
    prompt
}

fn render_context_pack_prompt(payload: &Value) -> String {
    let mut excerpt_paths = HashSet::new();
    let mut exact_lines = Vec::new();
    let mut symbol_lines = Vec::new();
    let mut seen_symbols = HashSet::new();
    for item in payload["retrieval"]["symbol_hits"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let line = format!(
            "[{}] {} :: {} :: {}",
            item["provenance"]["source_project"]
                .as_str()
                .unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["name"].as_str().unwrap_or_default(),
            item["kind"].as_str().unwrap_or_default(),
        );
        if seen_symbols.insert(line.clone()) {
            symbol_lines.push(line);
        }
    }

    let mut excerpt_lines = Vec::new();
    let mut seen_excerpts = HashSet::new();
    for section in ["lexical_chunks", "semantic_chunks"] {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let line = format!(
                "[{}] {} :: {}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
                item["content"].as_str().unwrap_or_default(),
            );
            if seen_excerpts.insert(line.clone()) {
                excerpt_lines.push(line);
            }
            excerpt_paths.insert(format!(
                "{}::{}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default()
            ));
        }
    }

    let mut seen_exact = HashSet::new();
    for item in payload["retrieval"]["exact_documents"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let key = format!(
            "{}::{}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default()
        );
        if excerpt_paths.contains(&key) {
            continue;
        }
        let line = format!(
            "[{}] {} {}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["snippet"].as_str().unwrap_or_default(),
        );
        if seen_exact.insert(line.clone()) {
            exact_lines.push(line);
        }
    }

    let mut prompt = String::new();
    prompt.push_str("Q:");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push('\n');
    prompt.push_str("M:");
    prompt.push_str(
        payload["effective_retrieval_mode"]
            .as_str()
            .unwrap_or_default(),
    );
    prompt.push('\n');
    prompt.push_str("P\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push('[');
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str("] ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    push_compact_lines(&mut prompt, "D", &exact_lines);
    push_compact_lines(&mut prompt, "S", &symbol_lines);
    push_compact_lines(&mut prompt, "E", &excerpt_lines);
    prompt
}

fn push_compact_lines(prompt: &mut String, title: &str, lines: &[String]) {
    prompt.push_str(title);
    prompt.push('\n');
    for line in lines {
        prompt.push_str(line);
        prompt.push('\n');
    }
    prompt.push('\n');
}

fn build_tokenizer(name: &str) -> Result<CoreBPE> {
    match name {
        "o200k_base" => o200k_base().context("failed to initialize o200k_base tokenizer"),
        "cl100k_base" => cl100k_base().context("failed to initialize cl100k_base tokenizer"),
        other => Err(anyhow!("unsupported tokenizer: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MeasurementConfig, NaiveScope, TokenBudgetContractConfig, TokenBudgetEvent,
        apply_reverification_metadata, baseline_strategy_breakdown,
        bind_infra_cost_profile_json_from_source, bind_rate_card_json_from_source,
        build_adjustment_registry_json, build_adjustment_request_schema_json,
        build_baseline_contract_json, build_billing_policy_json,
        build_continuity_restore_observed_event, build_contractual_evidence_pack,
        build_contractual_statement_summary, build_event_payload,
        build_external_truth_sources_json, build_margin_contract_json, build_margin_scope,
        build_metering_freshness_contract_json, build_metering_freshness_summary,
        build_product_headline, build_rate_card_json, build_reconciliation_contract_json,
        build_reconciliation_preview, build_settlement_contract_json,
        build_statement_export_preview, build_statement_preview, build_telemetry_surfaces_json,
        build_usage_event_schema_json, configured_provider_rate_card_source,
        configured_provider_usage_source, contractual_line_item_json,
        count_cli_context_pack_output_overhead_tokens, count_tool_overhead_tokens,
        default_adjustment_preview_model_version, default_adjustment_registry_version,
        default_adjustment_request_schema_version, default_backfill_policy_version,
        default_baseline_method_version, default_billing_mode, default_billing_policy_version,
        default_contractual_evidence_pack_version, default_correction_policy_version,
        default_coverage_model_version, default_currency_profile, default_dedup_contract_version,
        default_dispute_policy_version, default_event_time_policy_version,
        default_excluded_taxonomy_version, default_freeze_close_policy_version,
        default_infra_cost_profile_version, default_late_arrival_policy_version,
        default_margin_model_version, default_metering_freshness_model_version,
        default_quality_method_version, default_rate_card_binding_model_version,
        default_rate_card_version, default_reconciliation_contract_version,
        default_settlement_lifecycle_model_version, default_settlement_statement_version,
        default_settlement_status, default_statement_period_governance_version,
        default_suitability_model_version, default_telemetry_surface_split_version,
        derive_baseline_strategy, derive_quality_verdict, derive_query_type, derive_traffic_class,
        event_to_json, followup_queries_related, hex_sha256, include_traffic_class_in_report,
        latency_slice_breakdown, load_adjustment_registry_from_source,
        load_provider_invoice_binding_from_source, load_provider_usage_binding_from_source,
        needs_live_reverification, parse_infra_cost_profile_file, parse_rate_card_file,
        parse_snapshot_event, provider_rate_card_default_path, provider_usage_default_path,
        reconcile_followup_recovery, repair_legacy_token_event_payload, report_contract_json,
        summarize_events,
    };
    use crate::postgres::ObservabilitySnapshotRecord;
    use serde_json::json;
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
        build_adjustment_registry_json(Path::new("/tmp/amai-no-adjustments"), contract)
    }

    fn rate_card_fixture(contract: &TokenBudgetContractConfig) -> serde_json::Value {
        build_rate_card_json(Path::new("/tmp/amai-no-rate-card"), contract)
    }

    fn provider_usage_binding_fixture(rate_card: &serde_json::Value) -> serde_json::Value {
        load_provider_usage_binding_from_source(
            &json!({
                "status": "not_configured",
                "binding_status": "not_configured",
            }),
            rate_card,
        )
    }

    fn provider_invoice_binding_fixture() -> serde_json::Value {
        load_provider_invoice_binding_from_source(&json!({
            "status": "not_configured",
            "binding_status": "not_configured",
        }))
    }

    macro_rules! token_event {
        ($($field:ident : $value:expr,)+) => {
            {
                let mut event = TokenBudgetEvent {
                    created_at_epoch_ms: 0,
                    event_id: "event-default".to_string(),
                    correlation_id: "event-default".to_string(),
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
                };
                $(event.$field = $value;)+
                event
            }
        };
    }

    #[test]
    fn traffic_class_comes_from_source_kind_prefix() {
        assert_eq!(derive_traffic_class("live_context_pack"), "live");
        assert_eq!(derive_traffic_class("verify_context_pack"), "verify");
        assert_eq!(derive_traffic_class("verify_token_benchmark"), "verify");
        assert_eq!(derive_traffic_class("proof_hostile"), "proof");
        assert_eq!(derive_traffic_class("benchmark_hot_path"), "benchmark");
        assert_eq!(derive_traffic_class("custom_unknown"), "unknown");
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
            "client-limit-meter-alignment-v5"
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
            "contractual-evidence-pack-v18"
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
        let rate_card = build_rate_card_json(Path::new("/tmp/amai-no-rate-card"), &contract);
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
        let parsed = parse_rate_card_file(
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
        let rate_card = bind_rate_card_json_from_source(&source, &contract);
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
        let parsed = parse_infra_cost_profile_file(
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
        let binding = bind_infra_cost_profile_json_from_source(&source, &contract);
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
        let registry = load_adjustment_registry_from_source(&source, &contract);
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

        let binding = load_provider_usage_binding_from_source(&source, &rate_card);
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

        let binding = load_provider_invoice_binding_from_source(&source);
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
        let sources = build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
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
            "settlement-preview-v5"
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
            "client-limit-meter-alignment-v5"
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
            &build_external_truth_sources_json(Path::new("/tmp/amai-no-sources")),
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
            &build_external_truth_sources_json(Path::new("/tmp/amai-no-sources")),
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
        let schema = build_adjustment_request_schema_json(&contract);
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
        let sources = build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
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
        let sources = build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
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
        let sources = build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
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
        let sources = build_external_truth_sources_json(Path::new("/tmp/amai-no-sources"));
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

        assert_eq!(preview["model_version"], "contractual-statement-export-v18");
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
            "settlement-report-preview-v9"
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
                        "settlement_report_preview": {
                            "model_version": "settlement-report-preview-v9",
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
        assert_eq!(payload["pack_version"], "contractual-evidence-pack-v18");
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
            "settlement-report-preview-v9"
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
                "fallback_rate": 3.8
            }),
            "окно Codex 5 часов",
        );
        assert_eq!(headline["metric_code"], "verified_effective_savings_pct");
        assert_eq!(headline["value_percent"], 28.4);
        assert_eq!(headline["saved_tokens"], 184220);
        assert_eq!(headline["status"], "pass");
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

        assert_eq!(economics["model_version"], "agent-cycle-lower-bound-v3");
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
            "client-limit-meter-alignment-v5"
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
        let source = configured_provider_usage_source(&repo_root);
        assert_eq!(source["status"], "default_path_missing");
        assert_eq!(source["binding_status"], "not_configured");
        assert_eq!(
            source["resolved_path"],
            provider_usage_default_path(&repo_root)
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
        let default_path = provider_rate_card_default_path(&repo_root);
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
        let source = configured_provider_rate_card_source(&repo_root);
        assert_eq!(source["status"], "default_existing_path");
        assert_eq!(source["binding_status"], "default_but_unbound");
        assert_eq!(source["resolved_path"], default_path.display().to_string());
        fs::remove_dir_all(&repo_root).expect("cleanup repo root");
    }
}
