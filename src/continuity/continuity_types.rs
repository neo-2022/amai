use crate::codex_threads;
use crate::postgres::{NamespaceRecord, ProjectRecord};
use crate::token_budget;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(super) struct ContinuitySource {
    pub(super) original_path: PathBuf,
    pub(super) relative_path: String,
    pub(super) source_kind: String,
    pub(super) artifact_bucket: String,
    pub(super) artifact_kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ContinuityThreadIndexFile {
    #[serde(default)]
    pub(super) threads: Vec<ContinuityThreadIndexEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ContinuityThreadIndexEntry {
    #[serde(default)]
    pub(super) thread_id: String,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) cwd: String,
    #[serde(default)]
    pub(super) first_user_message: String,
    #[serde(default)]
    pub(super) source_rollout: String,
    #[serde(default)]
    pub(super) raw_mirror: String,
    #[serde(default)]
    pub(super) rendered_transcript: String,
    #[serde(default)]
    pub(super) started_at: String,
    #[serde(default)]
    pub(super) ended_at: String,
    #[serde(default)]
    pub(super) messages_count: usize,
    #[serde(default)]
    pub(super) last_user_message: String,
    #[serde(default)]
    pub(super) last_assistant_message: String,
    #[serde(default)]
    pub(super) summary_headline: String,
    #[serde(default)]
    pub(super) summary_next_step: String,
    #[serde(default)]
    pub(super) time_slices: Vec<codex_threads::ThreadTimeSliceSummary>,
    #[serde(default)]
    pub(super) created_at_epoch_s: i64,
    #[serde(default)]
    pub(super) updated_at_epoch_s: i64,
}

pub(super) struct ContinuityStartupContext {
    pub(super) project: ProjectRecord,
    pub(super) namespace: NamespaceRecord,
    pub(super) continuity: Value,
    pub(super) handoff_summary: Value,
    pub(super) restore: Option<Value>,
}

pub(super) struct ContinuityRestoreObservedResources {
    pub(super) repo_root: PathBuf,
    pub(super) token_budget_config: token_budget::TokenBudgetConfigFile,
    pub(super) tokenizer_prewarm: tokio::task::JoinHandle<Result<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupRuntimeStateAudit {
    pub status: String,
    pub output_path: PathBuf,
    pub artifact_exists: bool,
    pub startup_contract_sha_matches_current_contract: Option<bool>,
    pub source_summary_field_matches: Option<bool>,
    pub prompt_text_present: Option<bool>,
    pub startup_next_action_present: Option<bool>,
    pub startup_execution_gate_present: Option<bool>,
    pub required_return_task_field_present: Option<bool>,
    pub execctl_active_lease_field_present: Option<bool>,
    pub project_task_tree_field_present: Option<bool>,
    pub project_task_tree_summary_field_present: Option<bool>,
    pub project_task_ledger_field_present: Option<bool>,
    pub project_task_ledger_summary_field_present: Option<bool>,
    pub resume_state: Option<String>,
    pub action_kind: Option<String>,
    pub lease_owner_state: Option<String>,
    pub must_follow_startup_next_action: Option<bool>,
    pub unrelated_work_allowed: Option<bool>,
    pub must_read_prompt_text_before_reply: Option<bool>,
    pub required_action_kind_when_resume_required: Option<String>,
    pub no_silent_drop: Option<bool>,
    pub artifact_gate_semantics_consistent_present: Option<bool>,
    pub artifact_gate_semantics_consistent_matches_recomputed: Option<bool>,
    pub gate_semantics_consistent: Option<bool>,
}

#[derive(Debug)]
pub(super) struct ContinuityEvalProbe {
    pub(super) name: &'static str,
    pub(super) expected_verdict_class: &'static str,
    pub(super) verdict_class: String,
    pub(super) verdict_reason: String,
    pub(super) details: Value,
}
