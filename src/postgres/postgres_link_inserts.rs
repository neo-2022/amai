use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MemoryLinkDecisionInsert<'a> {
    pub task_node_id: Option<Uuid>,
    pub retrieval_trace_id: Option<Uuid>,
    pub candidate_task_node_id: Option<Uuid>,
    pub decision_outcome: &'a str,
    pub legality_passed: bool,
    pub scope_filter_passed: bool,
    pub evidence_sufficient: bool,
    pub classifier_label: Option<&'a str>,
    pub classifier_score: Option<f64>,
    pub decision_reason: Option<&'a str>,
    pub decision_payload: &'a Value,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub recorded_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PendingLinkProposalInsert<'a> {
    pub task_node_id: Option<Uuid>,
    pub retrieval_trace_id: Option<Uuid>,
    pub candidate_task_node_id: Option<Uuid>,
    pub proposal_state: Option<&'a str>,
    pub proposal_reason: &'a str,
    pub evidence_request: Option<&'a str>,
    pub evidence_payload: &'a Value,
    pub classifier_score: Option<f64>,
    pub ttl_epoch_ms: Option<i64>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct MemoryEdgeInsert<'a> {
    pub source_memory_item_id: Uuid,
    pub target_memory_item_id: Uuid,
    pub edge_kind: &'a str,
    pub edge_state: Option<&'a str>,
    pub trust_state: Option<&'a str>,
    pub validity_basis: Option<&'a str>,
    pub score: Option<f64>,
    pub evidence: &'a Value,
    pub source_kind: Option<&'a str>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MemoryConflictInsert<'a> {
    pub left_memory_item_id: Option<Uuid>,
    pub right_memory_item_id: Option<Uuid>,
    pub conflict_kind: &'a str,
    pub conflict_state: Option<&'a str>,
    pub severity: Option<&'a str>,
    pub summary: &'a str,
    pub evidence: &'a Value,
    pub source_kind: Option<&'a str>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub resolution: Option<&'a Value>,
    pub detected_at_epoch_ms: Option<i64>,
    pub resolved_at_epoch_ms: Option<i64>,
}
