use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(super) struct MemoryCardState {
    pub(super) project_id: Uuid,
    pub(super) namespace_id: Uuid,
    pub(super) truth_state: String,
    pub(super) verification_state: String,
    pub(super) status: String,
    pub(super) valid_from_epoch_ms: Option<i64>,
    pub(super) valid_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservabilityInsertMeta {
    pub(super) event_key: String,
    pub(super) source_event_id: Option<String>,
    pub(super) source_kind: String,
    pub(super) source_class: String,
    pub(super) scope_project_code: Option<String>,
    pub(super) scope_namespace_code: Option<String>,
    pub(super) captured_at_epoch_ms: Option<i64>,
    pub(super) payload_sha256: String,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryItemCandidateExtraction {
    pub(super) source_basis_status: String,
    pub(super) source_event_count: usize,
    pub(super) artifact_ref_count: usize,
    pub(super) message_ref_count: usize,
    pub(super) has_evidence_span: bool,
    pub(super) source_kind: Option<String>,
    pub(super) imported_from: Value,
    pub(super) raw_event_kind: String,
    pub(super) raw_event_payload: Value,
    pub(super) candidate_class: String,
    pub(super) hot_path_write_eligible: bool,
    pub(super) background_consolidation_recommended: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryItemPolicyScopeFilter {
    pub(super) visibility_scope: String,
    pub(super) sensitivity_class: String,
    pub(super) workspace_code: String,
    pub(super) project_code: String,
    pub(super) namespace_code: Option<String>,
    pub(super) owner_agent_required: bool,
    pub(super) owner_agent_present: bool,
    pub(super) private_contour_violation: bool,
    pub(super) quarantine_contour_violation: bool,
    pub(super) cross_project_basis_present: bool,
    pub(super) source_project_bound: bool,
    pub(super) import_packet_present: bool,
    pub(super) import_packet_found: bool,
    pub(super) import_packet_source_matches: bool,
    pub(super) import_packet_target_matches: bool,
    pub(super) import_packet_status: Option<String>,
    pub(super) controlled_transfer_required: bool,
    pub(super) controlled_transfer_valid: bool,
    pub(super) scope_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryItemVerificationConflictCheck {
    pub(super) evidence_present: bool,
    pub(super) current_truth_conflict: bool,
    pub(super) poisoned_detected: bool,
    pub(super) private_contour_violation: bool,
    pub(super) truth_state: String,
    pub(super) trust_state: String,
    pub(super) verification_state: String,
    pub(super) superseded_by_memory_item_id: Option<Uuid>,
    pub(super) write_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TaskNodeCandidateExtraction {
    pub(super) source_basis_status: String,
    pub(super) source_event_count: usize,
    pub(super) artifact_ref_count: usize,
    pub(super) has_evidence_span: bool,
    pub(super) candidate_class: String,
    pub(super) derivation_kind: String,
    pub(super) source_kind: Option<String>,
    pub(super) hot_path_write_eligible: bool,
    pub(super) background_consolidation_recommended: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TaskNodePolicyScopeFilter {
    pub(super) visibility_scope: String,
    pub(super) project_code: String,
    pub(super) namespace_code: String,
    pub(super) owner_agent_required: bool,
    pub(super) owner_agent_present: bool,
    pub(super) private_contour_violation: bool,
    pub(super) scope_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TaskNodeVerificationConflictCheck {
    pub(super) evidence_present: bool,
    pub(super) duplicate_task_key_conflict: bool,
    pub(super) poisoned_detected: bool,
    pub(super) private_contour_violation: bool,
    pub(super) task_key: Option<String>,
    pub(super) write_allowed: bool,
}
