use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ArtifactRefInsert<'a> {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub artifact_kind: &'a str,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub content_type: Option<&'a str>,
    pub source_kind: Option<&'a str>,
    pub source_event_ids: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub metadata: &'a Value,
}

#[derive(Debug, Clone)]
pub struct ContextPackInsert<'a> {
    pub context_pack_id: Uuid,
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub retrieval_mode: &'a str,
    pub query_text: &'a str,
    pub visible_projects: &'a Value,
    pub payload: &'a Value,
    #[allow(dead_code)]
    pub artifact_ref_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct RetrievalTraceInsert {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub namespace_id: Uuid,
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
    pub derivation_kind: Option<String>,
    pub schema_version: Option<String>,
    pub final_decision: String,
    pub temporal_query_epoch_ms: Option<i64>,
    pub trace_payload: Value,
}

#[derive(Debug, Clone)]
pub struct RestorePackSourceSnapshotHint<'a> {
    pub snapshot_kind: &'a str,
    pub scope_project_code: Option<&'a str>,
    pub scope_namespace_code: Option<&'a str>,
    pub verified_exists: bool,
}

#[derive(Debug, Clone)]
pub struct RestorePackInsert<'a> {
    pub agent_scope: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub source_snapshot_id: Option<Uuid>,
    pub source_snapshot_hint: Option<RestorePackSourceSnapshotHint<'a>>,
    pub pack_kind: &'a str,
    pub source_kind: Option<&'a str>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub headline: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub payload: &'a Value,
    pub captured_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MemoryProvenanceInsert<'a> {
    pub memory_item_id: Option<Uuid>,
    pub source_kind: &'a str,
    pub source_event_id: Option<&'a str>,
    pub source_snapshot_id: Option<Uuid>,
    pub artifact_ref_id: Option<Uuid>,
    pub trust_level: Option<&'a str>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub observed_at_epoch_ms: Option<i64>,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub schema_version: Option<&'a str>,
    pub details: &'a Value,
}

#[derive(Debug, Clone)]
pub struct PolicyRuleInsert<'a> {
    pub project_code: Option<&'a str>,
    pub namespace_code: Option<&'a str>,
    pub rule_code: &'a str,
    pub rule_scope: &'a str,
    pub rule_kind: &'a str,
    pub rule_status: Option<&'a str>,
    pub precedence: Option<i32>,
    pub source_kind: Option<&'a str>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub rule_payload: &'a Value,
}

#[derive(Debug, Clone)]
pub struct QuarantineItemInsert<'a> {
    pub project_code: Option<&'a str>,
    pub namespace_code: Option<&'a str>,
    pub entity_kind: &'a str,
    pub entity_id: Option<Uuid>,
    pub quarantine_reason: &'a str,
    pub quarantine_state: Option<&'a str>,
    pub evidence: &'a Value,
    pub source_kind: Option<&'a str>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub quarantined_at_epoch_ms: Option<i64>,
    pub released_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ExecCtlTaskLedgerEntryInsert<'a> {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub agent_scope: &'a str,
    pub session_id: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: &'a str,
    pub event_kind: &'a str,
    pub source_kind: &'a str,
    pub headline: &'a str,
    pub next_step: &'a str,
    pub summary: &'a str,
    pub active_files: &'a Value,
    pub open_questions: &'a Value,
    pub materialized_notes: &'a Value,
    pub pending_return_queue: &'a Value,
    pub local_path: Option<&'a str>,
    pub recorded_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ExecCtlTaskLeaseInsert<'a> {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub agent_scope: &'a str,
    pub owner_session_id: Option<&'a str>,
    pub owner_thread_id: Option<&'a str>,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: &'a str,
    pub source_kind: &'a str,
    pub lease_state: &'a str,
    pub headline: &'a str,
    pub next_step: &'a str,
    pub local_path: Option<&'a str>,
    pub acquired_at_epoch_ms: i64,
    pub heartbeat_at_epoch_ms: i64,
    pub expires_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct MemoryItemInsert<'a> {
    pub source_project_code: Option<&'a str>,
    pub import_packet_id: Option<Uuid>,
    pub owner_agent_code: Option<&'a str>,
    pub item_kind: &'a str,
    pub identity_key: Option<&'a str>,
    pub title: &'a str,
    pub summary: Option<&'a str>,
    pub body: Option<&'a str>,
    pub sensitivity_class: Option<&'a str>,
    pub truth_state: Option<&'a str>,
    pub trust_state: Option<&'a str>,
    pub verification_state: Option<&'a str>,
    pub lifecycle_state: Option<&'a str>,
    pub source_event_ids: &'a [String],
    pub artifact_refs: &'a [String],
    pub message_refs: &'a [String],
    pub evidence_span: &'a Value,
    pub derivation_kind: Option<&'a str>,
    pub observed_at_epoch_ms: Option<i64>,
    pub recorded_at_epoch_ms: Option<i64>,
    pub valid_from_epoch_ms: Option<i64>,
    pub valid_to_epoch_ms: Option<i64>,
    pub last_verified_at_epoch_ms: Option<i64>,
    pub object_version: Option<i64>,
    pub causation_id: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
    pub utility_score: Option<f64>,
    pub freshness_score: Option<f64>,
    pub retention_class: Option<&'a str>,
    pub ttl_epoch_ms: Option<i64>,
    pub decay_policy: Option<&'a str>,
    pub consolidation_status: Option<&'a str>,
    pub imported_from: Option<&'a Value>,
    pub schema_version: Option<&'a str>,
    pub superseded_by_memory_item_id: Option<Uuid>,
    pub metadata: &'a Value,
}

#[derive(Debug, Clone)]
pub struct MemoryItemUpdate<'a> {
    pub memory_item_id: Uuid,
    pub summary: Option<&'a str>,
    pub superseded_by_memory_item_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct TaskNodeInsert<'a> {
    pub parent_task_node_id: Option<Uuid>,
    pub memory_item_id: Option<Uuid>,
    pub task_key: Option<&'a str>,
    pub task_role: Option<&'a str>,
    pub headline: &'a str,
    pub summary: Option<&'a str>,
    pub next_step: Option<&'a str>,
    pub execution_state: Option<&'a str>,
    pub lifecycle_state: Option<&'a str>,
    pub confidence: Option<f64>,
    pub current_score: Option<f64>,
    pub reopened_count: Option<i32>,
    pub child_count: Option<i32>,
    pub closed_child_count: Option<i32>,
    pub pending_return_count: Option<i32>,
    pub source_event_ids: Option<&'a Value>,
    pub artifact_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub status_payload: &'a Value,
    pub metadata: &'a Value,
    pub opened_at_epoch_ms: Option<i64>,
    pub closed_at_epoch_ms: Option<i64>,
    pub archived_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TaskEventInsert<'a> {
    pub task_node_id: Uuid,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: Option<&'a str>,
    pub event_kind: &'a str,
    pub prior_execution_state: Option<&'a str>,
    pub next_execution_state: Option<&'a str>,
    pub prior_lifecycle_state: Option<&'a str>,
    pub next_lifecycle_state: Option<&'a str>,
    pub source_kind: Option<&'a str>,
    pub artifact_refs: Option<&'a Value>,
    pub message_refs: Option<&'a Value>,
    pub evidence_span: Option<&'a Value>,
    pub derivation_kind: Option<&'a str>,
    pub schema_version: Option<&'a str>,
    pub event_payload: &'a Value,
    pub recorded_at_epoch_ms: Option<i64>,
}
