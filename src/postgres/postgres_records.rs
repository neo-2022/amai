use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::ProjectRecord;

#[derive(Debug, Clone, Serialize)]
pub struct SkillCardRecord {
    pub skill_card_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: String,
    pub skill_id: String,
    pub skill_version: i32,
    pub skill_title: String,
    pub skill_goal: String,
    pub skill_trigger_conditions: Value,
    pub skill_preconditions: Value,
    pub skill_execution_steps: Value,
    pub skill_stop_conditions: Value,
    pub skill_forbidden_when: Value,
    pub skill_expected_outcome: Option<String>,
    pub skill_scope_type: String,
    pub skill_owner_scope: String,
    pub skill_trust_state: String,
    pub skill_verification_state: String,
    pub skill_runtime_constraints: Value,
    pub skill_model_constraints: Value,
    pub skill_tool_constraints: Value,
    pub skill_context_constraints: Value,
    pub skill_source_event_ids: Value,
    pub skill_artifact_refs: Value,
    pub skill_evidence_span: Value,
    pub skill_candidate_class: String,
    pub skill_derivation_kind: String,
    pub skill_source_kind: Option<String>,
    pub skill_hot_path_write_eligible: bool,
    pub skill_background_consolidation_recommended: bool,
    pub skill_success_count: i32,
    pub skill_failure_count: i32,
    pub skill_reuse_count: i32,
    pub skill_shadow_pass_count: i32,
    pub skill_shadow_fail_count: i32,
    pub skill_last_used_at: Option<String>,
    pub skill_last_verified_at: Option<String>,
    pub skill_patch_parent_id: Option<Uuid>,
    pub skill_merge_group_id: Option<Uuid>,
    pub skill_shared_promotion_state: String,
    pub skill_shared_approved_by: Option<String>,
    pub skill_shared_approval_reason: Option<String>,
    pub skill_shared_approved_at: Option<String>,
    pub skill_utility_score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillEvidenceBundleRecord {
    pub skill_evidence_bundle_id: Uuid,
    pub skill_card_id: Uuid,
    pub evidence_kind: String,
    pub summary: Option<String>,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTriggerMatchRecord {
    pub skill_trigger_match_id: Uuid,
    pub skill_card_id: Uuid,
    pub match_scope: String,
    pub matched: bool,
    pub summary: Option<String>,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTrialRunRecord {
    pub skill_trial_run_id: Uuid,
    pub skill_card_id: Uuid,
    pub application_mode: String,
    pub matched: bool,
    pub applied: bool,
    pub outcome: String,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillEvalRecord {
    pub skill_eval_id: Uuid,
    pub skill_card_id: Uuid,
    pub verdict: String,
    pub safe_to_apply: bool,
    pub quality_ok: bool,
    pub truth_ok: bool,
    pub utility_delta: f64,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillReuseLogRecord {
    pub skill_reuse_log_id: Uuid,
    pub skill_card_id: Uuid,
    pub reuse_mode: String,
    pub outcome: String,
    pub summary: Option<String>,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryCardRecord {
    pub memory_card_id: Uuid,
    pub project_code: String,
    pub namespace_code: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub tags: Value,
    pub provenance: Value,
    pub fact_subject: Option<String>,
    pub fact_predicate: Option<String>,
    pub fact_object: Option<String>,
    pub truth_state: String,
    pub verification_state: String,
    pub status: String,
    pub derivation_kind: String,
    pub candidate_class: String,
    pub source_kind: Option<String>,
    pub hot_path_write_eligible: bool,
    pub background_consolidation_recommended: bool,
    pub observed_at_epoch_ms: Option<i64>,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub last_verified_at_epoch_ms: Option<i64>,
    pub superseded_by_memory_card_id: Option<Uuid>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryCardSearchTemporalStats {
    pub prefilter_match_count: i64,
    pub admissible_match_count: i64,
    pub excluded_by_temporal_window: i64,
    pub excluded_by_current_truth_state: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryCardTemporalExclusionDiagnostic {
    pub memory_card_id: Uuid,
    pub title: String,
    pub truth_state: String,
    pub status: String,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub exclusion_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRelationEdgeRecord {
    pub memory_relation_edge_id: Uuid,
    pub project_code: String,
    pub namespace_code: String,
    pub source_memory_card_id: Uuid,
    pub target_memory_card_id: Uuid,
    pub relation_type: String,
    pub relation_state: String,
    pub evidence: Value,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryItemRecord {
    pub memory_item_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub source_project_code: Option<String>,
    pub import_packet_id: Option<Uuid>,
    pub owner_agent_id: Option<Uuid>,
    pub visibility_scope: String,
    pub item_kind: String,
    pub identity_key: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub sensitivity_class: String,
    pub truth_state: String,
    pub trust_state: String,
    pub verification_state: String,
    pub lifecycle_state: String,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub observed_at_epoch_ms: Option<i64>,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub last_verified_at_epoch_ms: Option<i64>,
    pub ingest_seq: i64,
    pub object_version: i64,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub utility_score: f64,
    pub freshness_score: f64,
    pub retention_class: String,
    pub ttl_epoch_ms: Option<i64>,
    pub access_count: i32,
    pub last_accessed_at: Option<String>,
    pub decay_policy: String,
    pub consolidation_status: String,
    pub imported_from: Value,
    pub schema_version: String,
    pub superseded_by_memory_item_id: Option<Uuid>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawEvidenceSearchTemporalStats {
    pub prefilter_match_count: i64,
    pub admissible_match_count: i64,
    pub excluded_by_temporal_window: i64,
    pub excluded_by_current_truth_state: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawEvidenceTemporalExclusionDiagnostic {
    pub memory_item_id: Uuid,
    pub title: String,
    pub truth_state: String,
    pub verification_state: String,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub exclusion_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryEdgeRecord {
    pub memory_edge_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub source_memory_item_id: Uuid,
    pub target_memory_item_id: Uuid,
    pub edge_kind: String,
    pub edge_state: String,
    pub trust_state: String,
    pub validity_basis: String,
    pub score: Option<f64>,
    pub evidence: Value,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryConflictRecord {
    pub memory_conflict_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub left_memory_item_id: Option<Uuid>,
    pub right_memory_item_id: Option<Uuid>,
    pub conflict_kind: String,
    pub conflict_state: String,
    pub severity: String,
    pub summary: String,
    pub evidence: Value,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub resolution: Value,
    pub detected_at_epoch_ms: Option<i64>,
    pub resolved_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryProvenanceRecord {
    pub memory_provenance_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub memory_item_id: Option<Uuid>,
    pub source_kind: String,
    pub source_event_id: Option<String>,
    pub source_snapshot_id: Option<Uuid>,
    pub artifact_ref_id: Option<Uuid>,
    pub trust_level: String,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub observed_at_epoch_ms: Option<i64>,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub schema_version: String,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRefRecord {
    pub artifact_ref_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: String,
    pub artifact_kind: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: Option<String>,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRawEventRecord {
    pub memory_raw_event_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: String,
    pub source_project_code: Option<String>,
    pub import_packet_id: Option<Uuid>,
    pub owner_agent_id: Option<Uuid>,
    pub event_kind: String,
    pub item_kind: String,
    pub visibility_scope: String,
    pub sensitivity_class: String,
    pub derivation_kind: String,
    pub truth_state: String,
    pub trust_state: String,
    pub verification_state: String,
    pub lifecycle_state: String,
    pub identity_key: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub source_epoch_ns: Option<i64>,
    pub source_monotonic_ns: Option<i64>,
    pub server_received_at_epoch_ms: i64,
    pub server_order_seq: i64,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWriteOutboxRecord {
    pub memory_write_outbox_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: String,
    pub memory_raw_event_id: Uuid,
    pub memory_item_id: Uuid,
    pub subject: String,
    pub delivery_kind: String,
    pub delivery_state: String,
    pub payload: Value,
    pub attempt_count: i32,
    pub last_error: Option<String>,
    pub published_at_epoch_ms: Option<i64>,
    pub acknowledged_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPackRecord {
    pub context_pack_id: Uuid,
    pub project_code: String,
    pub namespace_code: String,
    pub retrieval_mode: String,
    pub query_text: String,
    pub visible_projects: Value,
    pub payload: Value,
    pub artifact_ref_id: Option<Uuid>,
    pub artifact_bucket: Option<String>,
    pub artifact_object_key: Option<String>,
    pub artifact_state: String,
    pub artifact_last_error: Option<String>,
    pub artifact_updated_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct RawEvidenceRecord {
    pub memory_item_id: Uuid,
    pub memory_provenance_id: Option<Uuid>,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub content: String,
    pub source_kind: String,
    pub source_event_id: Option<String>,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub details: Value,
    pub derivation_kind: String,
    pub truth_state: String,
    pub trust_state: String,
    pub verification_state: String,
    pub observed_at_epoch_ms: Option<i64>,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub last_verified_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievalTraceRecord {
    pub retrieval_trace_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub context_pack_id: Option<Uuid>,
    pub query_text: String,
    pub requested_mode: Option<String>,
    pub effective_mode: Option<String>,
    pub scope_filter: Value,
    pub candidate_summary: Value,
    pub rerank_summary: Value,
    pub evidence_sufficiency: Value,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub final_decision: String,
    pub temporal_query_epoch_ms: Option<i64>,
    pub trace_payload: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestorePackRecord {
    pub restore_pack_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub agent_scope: Option<String>,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub source_snapshot_id: Option<Uuid>,
    pub pack_kind: String,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub headline: Option<String>,
    pub summary: Option<String>,
    pub payload: Value,
    pub captured_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyRuleRecord {
    pub policy_rule_id: Uuid,
    pub workspace_code: String,
    pub project_code: Option<String>,
    pub namespace_code: Option<String>,
    pub rule_code: String,
    pub rule_scope: String,
    pub rule_kind: String,
    pub rule_status: String,
    pub precedence: i32,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub rule_payload: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuarantineItemRecord {
    pub quarantine_item_id: Uuid,
    pub workspace_code: String,
    pub project_code: Option<String>,
    pub namespace_code: Option<String>,
    pub entity_kind: String,
    pub entity_id: Option<Uuid>,
    pub quarantine_reason: String,
    pub quarantine_state: String,
    pub evidence: Value,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub quarantined_at_epoch_ms: Option<i64>,
    pub released_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskNodeRecord {
    pub task_node_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub parent_task_node_id: Option<Uuid>,
    pub memory_item_id: Option<Uuid>,
    pub task_key: Option<String>,
    pub task_role: String,
    pub headline: String,
    pub summary: Option<String>,
    pub next_step: Option<String>,
    pub execution_state: String,
    pub lifecycle_state: String,
    pub confidence: Option<f64>,
    pub current_score: Option<f64>,
    pub reopened_count: i32,
    pub child_count: i32,
    pub closed_child_count: i32,
    pub pending_return_count: i32,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub evidence_span: Value,
    pub candidate_class: String,
    pub derivation_kind: String,
    pub source_kind: Option<String>,
    pub hot_path_write_eligible: bool,
    pub background_consolidation_recommended: bool,
    pub status_payload: Value,
    pub metadata: Value,
    pub opened_at_epoch_ms: Option<i64>,
    pub closed_at_epoch_ms: Option<i64>,
    pub archived_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskEventRecord {
    pub task_event_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub task_node_id: Uuid,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: Option<String>,
    pub event_kind: String,
    pub prior_execution_state: Option<String>,
    pub next_execution_state: Option<String>,
    pub prior_lifecycle_state: Option<String>,
    pub next_lifecycle_state: Option<String>,
    pub source_kind: Option<String>,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub event_payload: Value,
    pub recorded_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryLinkDecisionRecord {
    pub memory_link_decision_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub task_node_id: Option<Uuid>,
    pub retrieval_trace_id: Option<Uuid>,
    pub candidate_task_node_id: Option<Uuid>,
    pub decision_outcome: String,
    pub legality_passed: bool,
    pub scope_filter_passed: bool,
    pub evidence_sufficient: bool,
    pub classifier_label: Option<String>,
    pub classifier_score: Option<f64>,
    pub decision_reason: Option<String>,
    pub decision_payload: Value,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub recorded_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingLinkProposalRecord {
    pub pending_link_proposal_id: Uuid,
    pub workspace_code: String,
    pub project_code: String,
    pub namespace_code: Option<String>,
    pub task_node_id: Option<Uuid>,
    pub retrieval_trace_id: Option<Uuid>,
    pub candidate_task_node_id: Option<Uuid>,
    pub proposal_state: String,
    pub proposal_reason: String,
    pub evidence_request: Option<String>,
    pub evidence_payload: Value,
    pub classifier_score: Option<f64>,
    pub ttl_epoch_ms: Option<i64>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
}

#[derive(Debug, Clone)]
pub struct VisibleProjectRecord {
    pub project: ProjectRecord,
    pub relation_type: String,
    pub project_link_type: String,
    pub shared_contour: String,
    pub visibility_scope: String,
    pub relation_status: String,
    pub requires_approval: bool,
    pub transfer_policy_code: Option<String>,
    pub access_mode: String,
}

#[derive(Debug, Clone)]
pub struct SymbolRecord {
    pub name: String,
    pub kind: String,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub chunk_id: Uuid,
    pub qdrant_point_id: Option<Uuid>,
    pub qdrant_collection_alias: Option<String>,
    pub chunk_index: i32,
    pub total_chunks: i32,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub content: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DocumentRecord {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub repo_root: String,
    pub absolute_path: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub file_sha256: String,
    pub line_count: i32,
    pub byte_count: i64,
    pub content: String,
    pub metrics: Value,
    pub structure: Value,
    pub imports: Value,
    pub exports: Value,
    pub diagnostics: Value,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DocumentStructureRecord {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub structure: Value,
    pub imports: Value,
    pub exports: Value,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DocumentScopedSymbolRecord {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub name: String,
    pub kind: String,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservabilitySnapshotRecord {
    pub snapshot_id: Uuid,
    pub snapshot_kind: String,
    pub payload: Value,
    pub created_at_epoch_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservabilitySnapshotKindSummary {
    pub snapshot_kind: String,
    pub snapshots_count: i64,
    pub latest_created_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ExecCtlTaskLedgerEntryRecord {
    pub ledger_entry_id: Uuid,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: String,
    pub event_kind: String,
    pub source_kind: String,
    pub agent_scope: String,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub headline: String,
    pub next_step: String,
    pub summary: String,
    pub active_files: Value,
    pub open_questions: Value,
    pub materialized_notes: Value,
    pub pending_return_queue: Value,
    pub local_path: Option<String>,
    pub recorded_at_epoch_ms: i64,
    pub created_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ExecCtlTaskLeaseRecord {
    pub lease_id: Uuid,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: String,
    pub source_kind: String,
    pub agent_scope: String,
    pub owner_session_id: Option<String>,
    pub owner_thread_id: Option<String>,
    pub lease_state: String,
    pub headline: String,
    pub next_step: String,
    pub local_path: Option<String>,
    pub acquired_at_epoch_ms: i64,
    pub heartbeat_at_epoch_ms: i64,
    pub expires_at_epoch_ms: i64,
    pub created_at_epoch_ms: i64,
    pub updated_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ObservabilityRetentionCandidate {
    pub snapshot_id: Uuid,
    pub snapshot_kind: String,
    pub payload: Value,
    pub source_kind: String,
    pub source_class: String,
    pub created_at_epoch_ms: i64,
    pub captured_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct DocumentHit {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub score: f32,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct SymbolHit {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub name: String,
    pub kind: String,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub score: f32,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub chunk_id: Uuid,
    pub chunk_index: i32,
    pub start_line: i32,
    pub end_line: i32,
    pub score: f32,
    pub content: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingContextPackArtifactRecord {
    pub context_pack_id: Uuid,
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub bucket: String,
    pub object_key: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct MemoryWriteOutboxDelivery {
    pub memory_write_outbox_id: Uuid,
    pub subject: String,
    pub payload: Value,
}
