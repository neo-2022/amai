use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceRecord {
    pub workspace_id: Uuid,
    pub code: String,
    pub display_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRecord {
    pub project_id: Uuid,
    pub code: String,
    pub display_name: String,
    pub repo_root: String,
    pub visibility_scope: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamRecord {
    pub team_id: Uuid,
    pub workspace_code: String,
    pub code: String,
    pub display_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRoleRecord {
    pub role_id: Uuid,
    pub workspace_code: String,
    pub code: String,
    pub display_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRecord {
    pub agent_id: Uuid,
    pub workspace_code: String,
    pub team_code: Option<String>,
    pub role_code: Option<String>,
    pub code: String,
    pub display_name: String,
    pub visibility_scope: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct NamespaceRecord {
    pub namespace_id: Uuid,
    pub code: String,
    pub display_name: String,
    pub retrieval_mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferPolicyRecord {
    pub transfer_policy_id: Uuid,
    pub workspace_code: String,
    pub code: String,
    pub display_name: String,
    pub default_decision: String,
    pub allow_cross_project_read: bool,
    pub allow_import: bool,
    pub allow_verified_writeback: bool,
    pub requires_human_approval: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessPolicyRecord {
    pub access_policy_id: Uuid,
    pub workspace_code: String,
    pub team_code: Option<String>,
    pub project_code: Option<String>,
    pub role_code: Option<String>,
    pub code: String,
    pub display_name: String,
    pub object_class: String,
    pub scope_type: String,
    pub precedence: i32,
    pub can_read: bool,
    pub can_write: bool,
    pub can_link: bool,
    pub can_import: bool,
    pub can_promote: bool,
    pub can_share_further: bool,
    pub can_archive: bool,
    pub can_delete: bool,
    pub can_quarantine: bool,
    pub can_approve_transfer: bool,
    pub human_override: bool,
    pub override_reason: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPacketRecord {
    pub import_packet_id: Uuid,
    pub source_project_code: String,
    pub target_project_code: String,
    pub transfer_policy_code: Option<String>,
    pub requested_by_agent_code: Option<String>,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub status: String,
    pub summary: Option<String>,
    pub allowed_by_project_link: bool,
    pub reason: Option<String>,
    pub imported_by_agent_scope: String,
    pub trust_state: String,
    pub verification_state: String,
    pub borrowed_status: String,
    pub can_promote_after_verification: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPacketQuarantineDecisionRecord {
    pub quarantine_item_id: Uuid,
    pub import_packet_id: Uuid,
    pub workspace_code: String,
    pub source_project_code: String,
    pub target_project_code: String,
    pub decision: String,
    pub action_applied: bool,
    pub quarantine_reason: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPacketQuarantineResolutionSummary {
    pub apply: bool,
    pub scanned: usize,
    pub released: usize,
    pub rejected: usize,
    pub held: usize,
    pub decisions: Vec<ImportPacketQuarantineDecisionRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedAssetRecord {
    pub shared_asset_id: Uuid,
    pub workspace_code: String,
    pub code: String,
    pub display_name: String,
    pub asset_kind: String,
    pub source_project_code: Option<String>,
    pub transfer_policy_code: Option<String>,
    pub source_kind: Option<String>,
    pub source_event_ids: Value,
    pub artifact_refs: Value,
    pub message_refs: Value,
    pub evidence_span: Value,
    pub derivation_kind: String,
    pub schema_version: String,
    pub visibility_scope: String,
    pub status: String,
}
