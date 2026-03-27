use crate::chat_question;
use crate::cli::{
    ContinuityAnswerArgs, ContinuityHandoffArgs, ContinuityImportArgs, ContinuityRotateChatArgs,
    ContinuityStartupArgs, ContinuityStartupStateArgs, ContinuityThreadIndexEnrichArgs,
    VerifyContinuityArgs,
};
use crate::codex_threads;
use crate::config::AppConfig;
use crate::eval_verdict::{self, EvalPattern, EvalSignals};
use crate::mcp;
use crate::postgres::{self, ChunkRecord, DocumentRecord, NamespaceRecord, ProjectRecord};
use crate::retrieval_science;
use crate::s3;
use crate::token_budget;
use crate::working_state;
use crate::workspace_graph;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct ContinuitySource {
    original_path: PathBuf,
    relative_path: String,
    source_kind: String,
    artifact_bucket: String,
    artifact_kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ContinuityThreadIndexFile {
    #[serde(default)]
    threads: Vec<ContinuityThreadIndexEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ContinuityThreadIndexEntry {
    #[serde(default)]
    thread_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    first_user_message: String,
    #[serde(default)]
    source_rollout: String,
    #[serde(default)]
    raw_mirror: String,
    #[serde(default)]
    rendered_transcript: String,
    #[serde(default)]
    started_at: String,
    #[serde(default)]
    ended_at: String,
    #[serde(default)]
    messages_count: usize,
    #[serde(default)]
    last_user_message: String,
    #[serde(default)]
    last_assistant_message: String,
    #[serde(default)]
    summary_headline: String,
    #[serde(default)]
    summary_next_step: String,
    #[serde(default)]
    time_slices: Vec<codex_threads::ThreadTimeSliceSummary>,
    #[serde(default)]
    created_at_epoch_s: i64,
    #[serde(default)]
    updated_at_epoch_s: i64,
}

const MAX_SEARCHABLE_CONTINUITY_BYTES: usize = 12_000;

struct ContinuityStartupContext {
    project: ProjectRecord,
    namespace: NamespaceRecord,
    continuity: Value,
    handoff_summary: Value,
    restore: Option<Value>,
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
struct ContinuityEvalProbe {
    name: &'static str,
    expected_verdict_class: &'static str,
    verdict_class: String,
    verdict_reason: String,
    details: Value,
}

async fn connect_bootstrapped_admin(cfg: &AppConfig) -> Result<Client> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    Ok(db)
}

pub(crate) fn startup_runtime_state_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/continuity/project-chat-startup-state.json")
}

fn load_startup_runtime_state_artifact(repo_root: &Path) -> Result<Option<Value>> {
    let output_path = startup_runtime_state_artifact_path(repo_root);
    if !output_path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&output_path)
        .with_context(|| format!("failed to read {}", output_path.display()))?;
    let payload: Value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", output_path.display()))?;
    Ok(Some(payload))
}

fn evaluate_startup_execution_gate_consistency(
    summary: &Value,
    startup_execution_gate: &Value,
    prompt_text_present: Option<bool>,
) -> Option<bool> {
    let action_kind = summary["startup_next_action"]["action_kind"].as_str();
    let lease_owner_state = summary["execctl_active_lease"]["lease_owner_state"].as_str();
    let required_return_task_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("required_return_task")),
    );
    let must_follow_startup_next_action =
        startup_execution_gate["must_follow_startup_next_action"].as_bool();
    let unrelated_work_allowed = startup_execution_gate["unrelated_work_allowed"].as_bool();
    let must_read_prompt_text_before_reply =
        startup_execution_gate["must_read_prompt_text_before_reply"].as_bool();
    let required_action_kind_when_resume_required =
        startup_execution_gate["required_action_kind_when_resume_required"].as_str();
    let no_silent_drop = startup_execution_gate["no_silent_drop"].as_bool();
    let gate_required_return_task_present =
        startup_execution_gate["required_return_task_present"].as_bool();
    let gate_required_return_task_headline =
        startup_execution_gate["required_return_task_headline"].as_str();
    let gate_required_return_task_next_step =
        startup_execution_gate["required_return_task_next_step"].as_str();
    let blocking = startup_execution_gate["blocking"].as_bool();
    let required_return_task_headline = summary["required_return_task"]["headline"].as_str();
    let required_return_task_next_step = summary["required_return_task"]["next_step"].as_str();
    let gate_contract = mcp::project_chat_startup_contract();
    let gate_enforcement = &gate_contract["startup_execution_gate_enforcement"];
    let resume_enforcement = &gate_contract["resume_enforcement"];
    let previous_session_owner_value = resume_enforcement["previous_session_owner_value"]
        .as_str()
        .unwrap_or("previous_session_owner");

    match (
        must_follow_startup_next_action,
        unrelated_work_allowed,
        must_read_prompt_text_before_reply,
        required_action_kind_when_resume_required,
        no_silent_drop,
        action_kind,
        gate_required_return_task_present,
        blocking,
    ) {
        (
            Some(must_follow),
            Some(unrelated_allowed),
            Some(must_read_prompt),
            Some(required_action_kind),
            Some(no_silent_drop_value),
            Some(startup_action_kind),
            Some(gate_required_return_present),
            Some(blocking_value),
        ) => {
            let mut ok = true;
            if gate_enforcement["blocking_true_requires_must_follow"].as_bool() == Some(true)
                && blocking_value
                && !must_follow
            {
                ok = false;
            }
            if gate_enforcement["blocking_true_blocks_unrelated_work"].as_bool() == Some(true)
                && blocking_value
                && unrelated_allowed
            {
                ok = false;
            }
            if gate_enforcement["must_follow_true_blocks_unrelated_work"].as_bool() == Some(true)
                && must_follow
                && unrelated_allowed
            {
                ok = false;
            }
            if gate_enforcement["must_read_prompt_text_true_requires_prompt_before_reply"].as_bool()
                == Some(true)
                && must_read_prompt
                && prompt_text_present != Some(true)
            {
                ok = false;
            }
            if gate_enforcement["required_action_kind_resume_required_value"]
                .as_str()
                .is_some_and(|expected| expected != required_action_kind)
            {
                ok = false;
            }
            if gate_enforcement["no_silent_drop_must_be_true"].as_bool() == Some(true)
                && !no_silent_drop_value
            {
                ok = false;
            }
            if startup_action_kind == required_action_kind {
                if required_return_task_field_present != Some(true)
                    || !gate_required_return_present
                    || required_return_task_headline.is_none()
                    || required_return_task_next_step.is_none()
                    || gate_required_return_task_headline != required_return_task_headline
                    || gate_required_return_task_next_step != required_return_task_next_step
                    || !blocking_value
                {
                    ok = false;
                }
            }
            if lease_owner_state == Some(previous_session_owner_value)
                && resume_enforcement["previous_session_owner_must_follow_startup_next_action"]
                    .as_bool()
                    == Some(true)
                && !must_follow
            {
                ok = false;
            }
            Some(ok)
        }
        _ => None,
    }
}

pub(crate) fn inspect_startup_runtime_state(repo_root: &Path) -> Result<StartupRuntimeStateAudit> {
    let output_path = startup_runtime_state_artifact_path(repo_root);
    let Some(payload) = load_startup_runtime_state_artifact(repo_root)? else {
        return Ok(StartupRuntimeStateAudit {
            status: "not_materialized".to_string(),
            output_path,
            artifact_exists: false,
            startup_contract_sha_matches_current_contract: None,
            source_summary_field_matches: None,
            prompt_text_present: None,
            startup_next_action_present: None,
            startup_execution_gate_present: None,
            required_return_task_field_present: None,
            execctl_active_lease_field_present: None,
            project_task_tree_field_present: None,
            project_task_tree_summary_field_present: None,
            project_task_ledger_field_present: None,
            project_task_ledger_summary_field_present: None,
            resume_state: None,
            action_kind: None,
            lease_owner_state: None,
            must_follow_startup_next_action: None,
            unrelated_work_allowed: None,
            must_read_prompt_text_before_reply: None,
            required_action_kind_when_resume_required: None,
            no_silent_drop: None,
            artifact_gate_semantics_consistent_present: None,
            artifact_gate_semantics_consistent_matches_recomputed: None,
            gate_semantics_consistent: None,
        });
    };
    let expected_contract_sha =
        hex_sha256(&serde_json::to_vec(&mcp::project_chat_startup_contract())?);
    let summary = &payload["continuity_startup_summary"];
    let startup_contract_sha_matches_current_contract =
        Some(payload["startup_contract_sha256"].as_str() == Some(expected_contract_sha.as_str()));
    let source_summary_field_matches =
        Some(payload["source_summary_field"].as_str() == Some("continuity_startup_summary"));
    let prompt_text_present = Some(
        payload["chat_start_restore"]["prompt_text"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
    );
    let startup_next_action_present = Some(summary["startup_next_action"].is_object());
    let startup_execution_gate_present = Some(payload["startup_execution_gate"].is_object());
    let required_return_task_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("required_return_task")),
    );
    let execctl_active_lease_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("execctl_active_lease")),
    );
    let project_task_tree_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("project_task_tree")),
    );
    let project_task_tree_summary_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("project_task_tree_summary")),
    );
    let project_task_ledger_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("project_task_ledger")),
    );
    let project_task_ledger_summary_field_present = Some(
        summary
            .as_object()
            .is_some_and(|object| object.contains_key("project_task_ledger_summary")),
    );
    let resume_state = summary["execctl_resume_state"]
        .as_str()
        .map(ToOwned::to_owned);
    let action_kind = summary["startup_next_action"]["action_kind"]
        .as_str()
        .map(ToOwned::to_owned);
    let lease_owner_state = summary["execctl_active_lease"]["lease_owner_state"]
        .as_str()
        .map(ToOwned::to_owned);
    let must_follow_startup_next_action =
        payload["startup_execution_gate"]["must_follow_startup_next_action"].as_bool();
    let unrelated_work_allowed =
        payload["startup_execution_gate"]["unrelated_work_allowed"].as_bool();
    let must_read_prompt_text_before_reply =
        payload["startup_execution_gate"]["must_read_prompt_text_before_reply"].as_bool();
    let required_action_kind_when_resume_required =
        payload["startup_execution_gate"]["required_action_kind_when_resume_required"]
            .as_str()
            .map(ToOwned::to_owned);
    let no_silent_drop = payload["startup_execution_gate"]["no_silent_drop"].as_bool();
    let gate_semantics_consistent = evaluate_startup_execution_gate_consistency(
        summary,
        &payload["startup_execution_gate"],
        prompt_text_present,
    );
    let artifact_gate_semantics_consistent = payload["gate_semantics_consistent"].as_bool();
    let artifact_gate_semantics_consistent_present =
        Some(artifact_gate_semantics_consistent.is_some());
    let artifact_gate_semantics_consistent_matches_recomputed = match (
        artifact_gate_semantics_consistent,
        gate_semantics_consistent,
    ) {
        (Some(observed), Some(recomputed)) => Some(observed == recomputed),
        _ => None,
    };

    let status = if payload["artifact_version"].as_str()
        != Some("workspace-startup-runtime-state-v3")
        || payload["source_tool"].as_str() != Some("amai_continuity_startup")
        || startup_contract_sha_matches_current_contract != Some(true)
        || source_summary_field_matches != Some(true)
        || prompt_text_present != Some(true)
        || startup_next_action_present != Some(true)
        || startup_execution_gate_present != Some(true)
        || required_return_task_field_present != Some(true)
        || execctl_active_lease_field_present != Some(true)
        || project_task_tree_field_present != Some(true)
        || project_task_tree_summary_field_present != Some(true)
        || project_task_ledger_field_present != Some(true)
        || project_task_ledger_summary_field_present != Some(true)
        || must_follow_startup_next_action.is_none()
        || unrelated_work_allowed.is_none()
        || must_read_prompt_text_before_reply.is_none()
        || required_action_kind_when_resume_required.is_none()
        || no_silent_drop.is_none()
        || artifact_gate_semantics_consistent_present != Some(true)
        || artifact_gate_semantics_consistent_matches_recomputed != Some(true)
        || gate_semantics_consistent != Some(true)
    {
        "startup_runtime_state_drift".to_string()
    } else {
        "ok".to_string()
    };

    Ok(StartupRuntimeStateAudit {
        status,
        output_path,
        artifact_exists: true,
        startup_contract_sha_matches_current_contract,
        source_summary_field_matches,
        prompt_text_present,
        startup_next_action_present,
        startup_execution_gate_present,
        required_return_task_field_present,
        execctl_active_lease_field_present,
        project_task_tree_field_present,
        project_task_tree_summary_field_present,
        project_task_ledger_field_present,
        project_task_ledger_summary_field_present,
        resume_state,
        action_kind,
        lease_owner_state,
        must_follow_startup_next_action,
        unrelated_work_allowed,
        must_read_prompt_text_before_reply,
        required_action_kind_when_resume_required,
        no_silent_drop,
        artifact_gate_semantics_consistent_present,
        artifact_gate_semantics_consistent_matches_recomputed,
        gate_semantics_consistent,
    })
}

pub fn print_startup_runtime_state(args: &ContinuityStartupStateArgs) -> Result<()> {
    let repo_root = canonical_path(&args.repo_root)?;
    let audit = inspect_startup_runtime_state(&repo_root)?;
    let artifact_payload = load_startup_runtime_state_artifact(&repo_root)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&build_startup_runtime_state_cli_json(
                &audit,
                artifact_payload.as_ref(),
            ))?
        );
        return Ok(());
    }
    println!("Amai continuity startup runtime state");
    println!("Workspace repo root: {}", repo_root.display());
    println!("Artifact path: {}", audit.output_path.display());
    println!("Status: {}", audit.status);
    println!("Artifact present: {}", audit.artifact_exists);
    println!(
        "Contract hash matches current startup contract: {}",
        audit
            .startup_contract_sha_matches_current_contract
            .unwrap_or(false)
    );
    println!(
        "Source summary field matches continuity_startup_summary: {}",
        audit.source_summary_field_matches.unwrap_or(false)
    );
    println!(
        "Prompt text present: {}",
        audit.prompt_text_present.unwrap_or(false)
    );
    println!(
        "startup_next_action present: {}",
        audit.startup_next_action_present.unwrap_or(false)
    );
    println!(
        "startup_execution_gate present: {}",
        audit.startup_execution_gate_present.unwrap_or(false)
    );
    println!(
        "required_return_task field present: {}",
        audit.required_return_task_field_present.unwrap_or(false)
    );
    println!(
        "execctl_active_lease field present: {}",
        audit.execctl_active_lease_field_present.unwrap_or(false)
    );
    println!(
        "project_task_tree field present: {}",
        audit.project_task_tree_field_present.unwrap_or(false)
    );
    println!(
        "project_task_tree_summary field present: {}",
        audit
            .project_task_tree_summary_field_present
            .unwrap_or(false)
    );
    println!(
        "project_task_ledger field present: {}",
        audit.project_task_ledger_field_present.unwrap_or(false)
    );
    println!(
        "project_task_ledger_summary field present: {}",
        audit
            .project_task_ledger_summary_field_present
            .unwrap_or(false)
    );
    println!(
        "Resume state: {}",
        audit.resume_state.as_deref().unwrap_or("n/a")
    );
    println!(
        "Action kind: {}",
        audit.action_kind.as_deref().unwrap_or("n/a")
    );
    println!(
        "Lease owner state: {}",
        audit.lease_owner_state.as_deref().unwrap_or("n/a")
    );
    println!(
        "Must follow startup_next_action: {}",
        audit.must_follow_startup_next_action.unwrap_or(false)
    );
    println!(
        "Unrelated work allowed: {}",
        audit.unrelated_work_allowed.unwrap_or(false)
    );
    println!(
        "Must read prompt_text before reply: {}",
        audit.must_read_prompt_text_before_reply.unwrap_or(false)
    );
    println!(
        "Required action kind when resume is required: {}",
        audit
            .required_action_kind_when_resume_required
            .as_deref()
            .unwrap_or("n/a")
    );
    println!("No silent drop: {}", audit.no_silent_drop.unwrap_or(false));
    println!(
        "Artifact gate_semantics_consistent present: {}",
        audit
            .artifact_gate_semantics_consistent_present
            .unwrap_or(false)
    );
    println!(
        "Artifact gate_semantics_consistent matches recomputed audit: {}",
        audit
            .artifact_gate_semantics_consistent_matches_recomputed
            .unwrap_or(false)
    );
    println!(
        "Gate semantics consistent: {}",
        audit.gate_semantics_consistent.unwrap_or(false)
    );
    if let Some(payload) = artifact_payload.as_ref() {
        println!(
            "Immediate gate action kind: {}",
            payload["startup_execution_gate"]["action_kind"]
                .as_str()
                .unwrap_or("n/a")
        );
        println!(
            "Immediate gate required return: {}",
            payload["startup_execution_gate"]["required_return_task_headline"]
                .as_str()
                .unwrap_or("n/a")
        );
    }
    if audit.status != "ok" {
        println!(
            "Repair: rerun cargo run -- continuity startup --repo-root {} --namespace continuity --json >/dev/null",
            repo_root.display()
        );
    }
    Ok(())
}

fn startup_runtime_state_audit_json(
    audit: &StartupRuntimeStateAudit,
    artifact_payload: Option<&Value>,
) -> Value {
    json!({
        "status": audit.status,
        "output_path": audit.output_path.display().to_string(),
        "artifact_exists": audit.artifact_exists,
        "startup_contract_sha_matches_current_contract": audit.startup_contract_sha_matches_current_contract,
        "source_summary_field_matches": audit.source_summary_field_matches,
        "prompt_text_present": audit.prompt_text_present,
        "startup_next_action_present": audit.startup_next_action_present,
        "startup_execution_gate_present": audit.startup_execution_gate_present,
        "required_return_task_field_present": audit.required_return_task_field_present,
        "execctl_active_lease_field_present": audit.execctl_active_lease_field_present,
        "project_task_tree_field_present": audit.project_task_tree_field_present,
        "project_task_tree_summary_field_present": audit.project_task_tree_summary_field_present,
        "project_task_ledger_field_present": audit.project_task_ledger_field_present,
        "project_task_ledger_summary_field_present": audit.project_task_ledger_summary_field_present,
        "resume_state": audit.resume_state,
        "action_kind": audit.action_kind,
        "lease_owner_state": audit.lease_owner_state,
        "must_follow_startup_next_action": audit.must_follow_startup_next_action,
        "unrelated_work_allowed": audit.unrelated_work_allowed,
        "must_read_prompt_text_before_reply": audit.must_read_prompt_text_before_reply,
        "required_action_kind_when_resume_required": audit.required_action_kind_when_resume_required,
        "no_silent_drop": audit.no_silent_drop,
        "artifact_gate_semantics_consistent_present": audit.artifact_gate_semantics_consistent_present,
        "artifact_gate_semantics_consistent_matches_recomputed": audit.artifact_gate_semantics_consistent_matches_recomputed,
        "gate_semantics_consistent": audit.gate_semantics_consistent,
        "client_budget_guard": artifact_payload.map(|payload| payload["client_budget_guard"].clone()).unwrap_or(Value::Null),
        "reply_execution_gate": artifact_payload.map(|payload| payload["reply_execution_gate"].clone()).unwrap_or(Value::Null),
        "blocking_reply_contract": artifact_payload.map(|payload| payload["blocking_reply_contract"].clone()).unwrap_or(Value::Null),
        "startup_execution_gate": artifact_payload.map(|payload| payload["startup_execution_gate"].clone()).unwrap_or(Value::Null),
        "startup_next_action": artifact_payload.map(|payload| payload["continuity_startup_summary"]["startup_next_action"].clone()).unwrap_or(Value::Null),
        "required_return_task": artifact_payload.map(|payload| payload["continuity_startup_summary"]["required_return_task"].clone()).unwrap_or(Value::Null),
        "execctl_active_lease": artifact_payload.map(|payload| payload["continuity_startup_summary"]["execctl_active_lease"].clone()).unwrap_or(Value::Null),
        "project_task_tree": artifact_payload.map(|payload| payload["continuity_startup_summary"]["project_task_tree"].clone()).unwrap_or(Value::Null),
        "project_task_ledger": artifact_payload.map(|payload| payload["continuity_startup_summary"]["project_task_ledger"].clone()).unwrap_or(Value::Null),
    })
}

fn build_startup_runtime_state_cli_json(
    audit: &StartupRuntimeStateAudit,
    artifact_payload: Option<&Value>,
) -> Value {
    let audit_json = startup_runtime_state_audit_json(audit, artifact_payload);
    let mut root = artifact_payload
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let root_object = root.as_object_mut().expect("startup runtime cli root object");
    root_object.insert("startup_runtime_state".to_string(), audit_json.clone());
    root_object.insert("startup_runtime_state_audit".to_string(), audit_json);
    root
}

pub async fn import_sources(cfg: &AppConfig, args: &ContinuityImportArgs) -> Result<()> {
    let payload = import_sources_payload(cfg, args).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn import_sources_payload(cfg: &AppConfig, args: &ContinuityImportArgs) -> Result<Value> {
    let mut db = postgres::connect_admin(cfg).await?;
    let s3_client = s3::connect(cfg).await?;
    let repo_root = canonical_string(&args.repo_root)?;
    let bootstrap_path = canonical_path(&args.bootstrap_file)?;
    let thread_index_path: Option<PathBuf> = args
        .thread_index_file
        .as_ref()
        .map(|path| canonical_path(path.as_path()))
        .transpose()?;
    let active_workline_path: Option<PathBuf> = args
        .active_workline_file
        .as_ref()
        .map(|path| canonical_path(path.as_path()))
        .transpose()?;
    let project = postgres::upsert_project(
        &db,
        &args.project,
        &args.display_name,
        &repo_root,
        Some("main"),
        &cfg.default_retrieval_mode,
    )
    .await?;
    let namespace = postgres::ensure_namespace(
        &db,
        project.project_id,
        &args.namespace,
        Some("Continuity"),
        "local_strict",
    )
    .await?;

    let sources = collect_sources(cfg, args)?;
    if sources.is_empty() {
        bail!("no continuity sources were found to import");
    }

    let _deleted = postgres::delete_namespace_documents(&db, namespace.namespace_id).await?;
    let _ = postgres::delete_observability_snapshots_by_scope(
        &db,
        "continuity_thread_index",
        "continuity_thread_index",
        &project.code,
        &namespace.code,
    )
    .await?;
    if let Some(thread_index_path) = &thread_index_path {
        import_thread_index_snapshots(&db, &project, &namespace, thread_index_path).await?;
    }

    let import_started_epoch_ms = now_epoch_ms()?;
    let import_batch_id = Uuid::new_v4();
    let mut imported = Vec::new();

    for source in &sources {
        let content = fs::read_to_string(&source.original_path)
            .with_context(|| format!("failed to read {}", source.original_path.display()))?;
        let (searchable_content, truncated_bytes) =
            truncate_utf8_by_bytes(&content, MAX_SEARCHABLE_CONTINUITY_BYTES);

        if thread_index_path.is_none()
            && source.source_kind == "continuity_rendered_transcript"
            && let Some(summary) = codex_threads::rendered_transcript_summary(
                &content,
                &source.original_path.display().to_string(),
                Some(&project.repo_root),
            )
        {
            let payload = json!({
                "continuity_thread_index": {
                    "project": {
                        "code": project.code,
                        "display_name": project.display_name,
                        "repo_root": project.repo_root,
                    },
                    "namespace": {
                        "code": namespace.code,
                        "display_name": namespace.display_name,
                    },
                    "thread_id": summary["thread_id"].clone(),
                    "title": summary["title"].clone(),
                    "cwd": summary["cwd"].clone(),
                    "first_user_message": summary["first_user_message"].clone(),
                    "started_at": summary["started_at"].clone(),
                    "ended_at": summary["ended_at"].clone(),
                    "messages_count": summary["messages_count"].clone(),
                    "last_user_message": summary["last_user_message"].clone(),
                    "last_assistant_message": summary["last_assistant_message"].clone(),
                    "summary_headline": summary["summary_headline"].clone(),
                    "summary_next_step": summary["summary_next_step"].clone(),
                    "time_slices": summary["time_slices"].clone(),
                    "rendered_transcript": summary["rendered_transcript"].clone(),
                    "source_rollout": summary["source_rollout"].clone(),
                    "created_at_epoch_s": summary["created_at_epoch_s"].clone(),
                    "updated_at_epoch_s": summary["updated_at_epoch_s"].clone(),
                }
            });
            let _ =
                postgres::insert_observability_snapshot(&db, "continuity_thread_index", &payload)
                    .await?;
        }

        let metadata = json!({
            "continuity_kind": source.source_kind,
            "original_path": source.original_path.display().to_string(),
            "imported_at_epoch_ms": import_started_epoch_ms,
            "import_batch_id": import_batch_id,
            "continuity_full_bytes": content.len(),
            "continuity_searchable_bytes": searchable_content.len(),
            "continuity_content_truncated": truncated_bytes > 0,
            "continuity_truncated_bytes": truncated_bytes,
        });

        let object_key = format!(
            "continuity/{}/{}/{}-{}.json",
            project.code,
            namespace.code,
            source.source_kind,
            hex_sha256(source.original_path.display().to_string().as_bytes())
        );
        let artifact_body = serde_json::to_string_pretty(&json!({
            "project_code": project.code,
            "namespace_code": namespace.code,
            "source_kind": source.source_kind,
            "original_path": source.original_path.display().to_string(),
            "relative_path": source.relative_path,
            "content": content,
        }))?;
        s3::put_json_object(
            &s3_client,
            &source.artifact_bucket,
            &object_key,
            &artifact_body,
        )
        .await?;
        let artifact_ref_id = postgres::insert_artifact_ref(
            &db,
            &postgres::ArtifactRefInsert {
                project_id: project.project_id,
                namespace_id: namespace.namespace_id,
                artifact_kind: &source.artifact_kind,
                bucket: &source.artifact_bucket,
                object_key: &object_key,
                content_type: Some("application/json"),
                metadata: &metadata,
            },
        )
        .await?;

        let document = build_document_record(
            &project,
            &namespace,
            source,
            &content,
            &searchable_content,
            metadata.clone(),
        )?;
        let chunks = build_chunks(cfg, &searchable_content);
        postgres::replace_document_index(&mut db, &document, &[], &chunks).await?;

        imported.push(json!({
            "source_kind": source.source_kind,
            "relative_path": source.relative_path,
            "original_path": source.original_path.display().to_string(),
            "artifact_bucket": source.artifact_bucket,
            "artifact_key": object_key,
            "artifact_ref_id": artifact_ref_id,
            "bytes": content.len(),
        }));
    }

    postgres::touch_project_updated_at(&db, project.project_id).await?;

    let bootstrap_text = fs::read_to_string(&bootstrap_path)
        .with_context(|| format!("failed to read {}", bootstrap_path.display()))?;
    let session_files = sources
        .iter()
        .filter(|source| source.source_kind == "continuity_session_memory")
        .count();
    let transcript_files = sources
        .iter()
        .filter(|source| source.source_kind == "continuity_rendered_transcript")
        .count();
    let active_summary = if let Some(active_workline_path) = &active_workline_path {
        let active_workline_text = fs::read_to_string(active_workline_path)
            .with_context(|| format!("failed to read {}", active_workline_path.display()))?;
        summarize_active_workline(&active_workline_text)
    } else {
        latest_handoff_summary(&db, &project, &namespace)
            .await?
            .unwrap_or_else(|| json!({"headline":"ещё нет данных","next_step":"ещё нет данных"}))
    };
    let bootstrap_summary = summarize_bootstrap(&bootstrap_text);

    let payload = json!({
        "continuity_import": {
            "project": {
                "code": project.code,
                "display_name": project.display_name,
                "repo_root": project.repo_root,
            },
            "namespace": {
                "code": namespace.code,
                "display_name": namespace.display_name,
            },
            "import_batch_id": import_batch_id,
            "imported_at_epoch_ms": import_started_epoch_ms,
            "documents_imported": imported.len(),
            "session_memory_files": session_files,
            "rendered_transcript_files": transcript_files,
            "bootstrap_summary": {
                "bootstrap_file": bootstrap_path.display().to_string(),
                "details": bootstrap_summary,
            },
            "active_workline_summary": {
                "active_workline_file": active_workline_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                "details": active_summary,
            },
            "sources": imported,
        }
    });
    let _ = postgres::insert_observability_snapshot(&db, "continuity_import", &payload).await?;
    Ok(payload)
}

pub fn enrich_thread_index_file(args: &ContinuityThreadIndexEnrichArgs) -> Result<()> {
    let input_path = canonical_path(&args.input)?;
    let output_path = args
        .output
        .as_ref()
        .map(|path| resolve_output_path(path))
        .transpose()?
        .unwrap_or_else(|| input_path.clone());
    let raw = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;
    let mut index: ContinuityThreadIndexFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", input_path.display()))?;
    let mut enriched_threads = 0usize;

    for entry in &mut index.threads {
        let summary = codex_threads::derive_thread_index_summary(
            Some(&entry.cwd),
            non_empty_path(&entry.rendered_transcript),
            non_empty_path(&entry.source_rollout),
            non_empty_path(&entry.raw_mirror),
        )?;
        let Some(summary) = summary else {
            continue;
        };
        entry.started_at = summary.started_at;
        entry.ended_at = summary.ended_at;
        entry.messages_count = summary.messages_count;
        entry.last_user_message = summary.last_user_message;
        entry.last_assistant_message = summary.last_assistant_message;
        entry.summary_headline = summary.summary_headline;
        entry.summary_next_step = summary.summary_next_step;
        entry.time_slices = summary.time_slices;
        entry.created_at_epoch_s = summary.created_at_epoch_s;
        entry.updated_at_epoch_s = summary.updated_at_epoch_s;
        enriched_threads += 1;
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(
        &output_path,
        serde_json::to_string_pretty(&index)
            .context("failed to serialize enriched thread index")?
            + "\n",
    )
    .with_context(|| format!("failed to write {}", output_path.display()))?;

    let payload = json!({
        "thread_index_enrich": {
            "input": input_path.display().to_string(),
            "output": output_path.display().to_string(),
            "threads_seen": index.threads.len(),
            "threads_enriched": enriched_threads,
        }
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn import_thread_index_snapshots(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    thread_index_path: &Path,
) -> Result<()> {
    let raw = fs::read_to_string(thread_index_path)
        .with_context(|| format!("failed to read {}", thread_index_path.display()))?;
    let index: ContinuityThreadIndexFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", thread_index_path.display()))?;
    let mut seen = BTreeSet::new();

    for entry in index.threads {
        if entry.thread_id.is_empty() || !entry.cwd.starts_with(&project.repo_root) {
            continue;
        }
        if !seen.insert(entry.thread_id.clone()) {
            continue;
        }

        let summary = if entry.summary_headline.is_empty()
            && entry.summary_next_step.is_empty()
            && entry.started_at.is_empty()
            && entry.ended_at.is_empty()
            && entry.messages_count == 0
            && entry.last_user_message.is_empty()
            && entry.last_assistant_message.is_empty()
            && entry.created_at_epoch_s == 0
            && entry.updated_at_epoch_s == 0
        {
            codex_threads::derive_thread_index_summary(
                Some(&entry.cwd),
                non_empty_path(&entry.rendered_transcript),
                non_empty_path(&entry.source_rollout),
                non_empty_path(&entry.raw_mirror),
            )?
            .map(|summary| {
                json!({
                    "started_at": summary.started_at,
                    "ended_at": summary.ended_at,
                    "messages_count": summary.messages_count,
                    "last_user_message": summary.last_user_message,
                    "last_assistant_message": summary.last_assistant_message,
                    "summary_headline": summary.summary_headline,
                    "summary_next_step": summary.summary_next_step,
                    "time_slices": summary.time_slices,
                    "created_at_epoch_s": summary.created_at_epoch_s,
                    "updated_at_epoch_s": summary.updated_at_epoch_s,
                })
            })
        } else {
            None
        };
        let payload = json!({
            "continuity_thread_index": {
                "project": {
                    "code": project.code,
                    "display_name": project.display_name,
                    "repo_root": project.repo_root,
                },
                "namespace": {
                    "code": namespace.code,
                    "display_name": namespace.display_name,
                },
                "thread_id": entry.thread_id,
                "title": json!(entry.title),
                "cwd": json!(entry.cwd),
                "first_user_message": json!(entry.first_user_message),
                "started_at": summary.as_ref().map(|value| value["started_at"].clone()).unwrap_or_else(|| json!(entry.started_at)),
                "ended_at": summary.as_ref().map(|value| value["ended_at"].clone()).unwrap_or_else(|| json!(entry.ended_at)),
                "messages_count": summary.as_ref().map(|value| value["messages_count"].clone()).unwrap_or_else(|| json!(entry.messages_count)),
                "last_user_message": summary.as_ref().map(|value| value["last_user_message"].clone()).unwrap_or_else(|| json!(entry.last_user_message)),
                "last_assistant_message": summary.as_ref().map(|value| value["last_assistant_message"].clone()).unwrap_or_else(|| json!(entry.last_assistant_message)),
                "summary_headline": summary.as_ref().map(|value| value["summary_headline"].clone()).unwrap_or_else(|| json!(entry.summary_headline)),
                "summary_next_step": summary.as_ref().map(|value| value["summary_next_step"].clone()).unwrap_or_else(|| json!(entry.summary_next_step)),
                "time_slices": summary.as_ref().map(|value| value["time_slices"].clone()).unwrap_or_else(|| json!(entry.time_slices)),
                "rendered_transcript": if entry.rendered_transcript.is_empty() { json!("") } else { json!(entry.rendered_transcript) },
                "source_rollout": if entry.source_rollout.is_empty() { json!("") } else { json!(entry.source_rollout) },
                "raw_rollout": if entry.raw_mirror.is_empty() { json!("") } else { json!(entry.raw_mirror) },
                "created_at_epoch_s": summary.as_ref().map(|value| value["created_at_epoch_s"].clone()).unwrap_or_else(|| json!(entry.created_at_epoch_s)),
                "updated_at_epoch_s": summary.as_ref().map(|value| value["updated_at_epoch_s"].clone()).unwrap_or_else(|| json!(entry.updated_at_epoch_s)),
            }
        });
        let _ = postgres::insert_observability_snapshot(db, "continuity_thread_index", &payload)
            .await?;
    }
    Ok(())
}

pub async fn print_startup(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<()> {
    let db = connect_bootstrapped_admin(cfg).await?;
    let context = load_startup_context(&db, args).await?;
    if args.json {
        let payload = startup_payload_with_context(&db, &context, args).await?;
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    let chat_start_restore = build_chat_start_restore(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        context.restore.as_ref(),
    );
    token_budget::record_continuity_restore_observed_event(
        &db,
        &context.project.code,
        &context.namespace.code,
        chat_start_restore["chat_start_restore"]["prompt_text"]
            .as_str()
            .unwrap_or_default(),
        &args.token_source_kind,
    )
    .await?;
    let startup_payload = build_continuity_startup_payload(&context, &chat_start_restore)?;
    persist_startup_runtime_state_artifact(
        Path::new(&context.project.repo_root),
        &startup_payload,
    )?;
    println!("Amai continuity startup");
    println!();
    println!(
        "Проект: {} ({})",
        context.project.display_name, context.project.code
    );
    println!("Корень проекта: {}", context.project.repo_root);
    println!("Namespace continuity: {}", context.namespace.code);
    println!(
        "Последний импорт continuity: {}",
        human_epoch_ms(context.continuity["imported_at_epoch_ms"].as_u64())
    );
    println!(
        "Импортировано документов: {}",
        context.continuity["documents_imported"]
            .as_u64()
            .unwrap_or(0)
    );
    println!(
        "Continuity snapshot: {}",
        context.continuity["bootstrap_summary"]["bootstrap_file"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    let bridge_files = context.continuity["session_memory_files"]
        .as_u64()
        .unwrap_or(0);
    if bridge_files > 0 {
        println!("Дополнительные bridge-notes: {}", bridge_files);
    }
    println!(
        "Rendered transcripts: {}",
        context.continuity["rendered_transcript_files"]
            .as_u64()
            .unwrap_or(0)
    );
    println!();
    let startup_next_step = context.handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    println!("Текущая активная линия:");
    println!(
        "- {}",
        context.handoff_summary["headline"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    println!("- Ближайший обязательный следующий шаг: {startup_next_step}");
    println!();
    print_chat_start_restore_human(&chat_start_restore);
    if let Some(restore) = context.restore.as_ref() {
        println!();
        working_state::print_restore_bundle_human(restore);
    }
    println!();
    println!("Bootstrap continuity:");
    println!(
        "- Thread count: {}",
        context.continuity["bootstrap_summary"]["details"]["thread_count"]
            .as_u64()
            .unwrap_or(0)
    );
    println!(
        "- Последний rendered transcript: {}",
        context.continuity["bootstrap_summary"]["details"]["latest_rendered_transcript"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    println!();
    let mut import_command = format!(
        "cargo run -- continuity import --project {} --display-name '{}' --repo-root {} --namespace {} --bootstrap-file {}",
        context.project.code,
        context.project.display_name.replace('\'', "\\'"),
        shell_quote(&context.project.repo_root),
        context.namespace.code,
        shell_quote(
            context.continuity["bootstrap_summary"]["bootstrap_file"]
                .as_str()
                .unwrap_or_default()
        ),
    );
    let active_workline_arg = context.continuity["active_workline_summary"]["active_workline_file"]
        .as_str()
        .unwrap_or_default();
    if !active_workline_arg.is_empty() {
        import_command.push_str(" --active-workline-file ");
        import_command.push_str(&shell_quote(active_workline_arg));
    }
    println!("Как использовать дальше:");
    println!(
        "- Для project-scoped retrieval: cargo run -- context pack --project {} --namespace {} --query 'ваш вопрос'",
        context.project.code, context.namespace.code
    );
    println!("- Для обновления continuity после новых изменений: {import_command}");
    Ok(())
}

pub async fn startup_payload(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<Value> {
    let db = connect_bootstrapped_admin(cfg).await?;
    let context = load_startup_context(&db, args).await?;
    startup_payload_with_context(&db, &context, args).await
}

async fn startup_payload_with_context(
    db: &Client,
    context: &ContinuityStartupContext,
    args: &ContinuityStartupArgs,
) -> Result<Value> {
    let chat_start_restore = build_chat_start_restore(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        context.restore.as_ref(),
    );
    token_budget::record_continuity_restore_observed_event(
        db,
        &context.project.code,
        &context.namespace.code,
        chat_start_restore["chat_start_restore"]["prompt_text"]
            .as_str()
            .unwrap_or_default(),
        &args.token_source_kind,
    )
    .await?;
    let payload = build_continuity_startup_payload(context, &chat_start_restore)?;
    persist_startup_runtime_state_artifact(Path::new(&context.project.repo_root), &payload)?;
    Ok(payload)
}

fn build_startup_runtime_state_artifact(
    repo_root: &Path,
    payload: &Value,
    generated_at_epoch_ms: u64,
) -> Result<Value> {
    let startup_contract_sha256 =
        hex_sha256(&serde_json::to_vec(&mcp::project_chat_startup_contract())?);
    let continuity_startup_summary = mcp::continuity_startup_summary_json(payload);
    let startup_execution_gate = build_startup_execution_gate(payload);
    let prompt_text_present = Some(
        payload["chat_start_restore"]["prompt_text"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
    );
    let gate_semantics_consistent = evaluate_startup_execution_gate_consistency(
        &continuity_startup_summary,
        &startup_execution_gate,
        prompt_text_present,
    );
    let client_budget_guard = if payload["working_state_restore"]["client_budget_guard"].is_object()
    {
        payload["working_state_restore"]["client_budget_guard"].clone()
    } else {
        Value::Null
    };
    let reply_execution_gate = if client_budget_guard["reply_execution_gate"].is_object() {
        client_budget_guard["reply_execution_gate"].clone()
    } else {
        Value::Null
    };
    let blocking_reply_contract = if reply_execution_gate["blocking_reply_contract"].is_object() {
        reply_execution_gate["blocking_reply_contract"].clone()
    } else {
        Value::Null
    };
    Ok(json!({
        "artifact_version": "workspace-startup-runtime-state-v3",
        "repo_root": repo_root.display().to_string(),
        "generated_at_epoch_ms": generated_at_epoch_ms,
        "source_tool": "amai_continuity_startup",
        "source_summary_field": "continuity_startup_summary",
        "startup_contract_sha256": startup_contract_sha256,
        "continuity_startup_summary": continuity_startup_summary,
        "startup_execution_gate": startup_execution_gate,
        "gate_semantics_consistent": gate_semantics_consistent,
        "client_budget_guard": client_budget_guard,
        "reply_execution_gate": reply_execution_gate,
        "blocking_reply_contract": blocking_reply_contract,
        "chat_start_restore": {
            "headline": payload["chat_start_restore"]["headline"].clone(),
            "next_step": payload["chat_start_restore"]["next_step"].clone(),
            "restore_confidence": payload["chat_start_restore"]["restore_confidence"].clone(),
            "prompt_text": payload["chat_start_restore"]["prompt_text"].clone(),
        },
        "working_state_restore_lineage": if payload["working_state_restore"]["state_lineage"].is_object() {
            payload["working_state_restore"]["state_lineage"].clone()
        } else {
            Value::Null
        }
    }))
}

fn build_startup_execution_gate(payload: &Value) -> Value {
    let contract = mcp::project_chat_startup_contract();
    let resume_enforcement = &contract["resume_enforcement"];
    let action_kind = payload["chat_start_restore"]["startup_next_action"]["action_kind"]
        .as_str()
        .unwrap_or("continue_active_workline");
    let lease_owner_state =
        payload["chat_start_restore"]["execctl_active_lease"]["lease_owner_state"].as_str();
    let previous_session_owner_value = resume_enforcement["previous_session_owner_value"]
        .as_str()
        .unwrap_or("previous_session_owner");
    let must_resume_before_unrelated =
        resume_enforcement["must_resume_required_return_task_before_unrelated_work"]
            .as_bool()
            .unwrap_or(false);
    let required_action_kind = resume_enforcement["required_action_kind_when_resume_required"]
        .as_str()
        .unwrap_or("resume_required_return_task");
    let blocking = payload["chat_start_restore"]["startup_next_action"]["blocking"]
        .as_bool()
        .unwrap_or(false);
    let must_follow = blocking
        || (must_resume_before_unrelated && action_kind == required_action_kind)
        || lease_owner_state == Some(previous_session_owner_value);

    json!({
        "gate_version": "startup-execution-gate-v1",
        "action_kind": action_kind,
        "blocking": blocking,
        "resume_state": payload["chat_start_restore"]["execctl_resume_state"]
            .as_str()
            .unwrap_or("clear"),
        "required_return_task_present": payload["chat_start_restore"]["required_return_task"].is_object(),
        "required_return_task_headline": payload["chat_start_restore"]["required_return_task"]["headline"]
            .as_str(),
        "required_return_task_next_step": payload["chat_start_restore"]["required_return_task"]["next_step"]
            .as_str(),
        "lease_owner_state": lease_owner_state,
        "must_follow_startup_next_action": must_follow,
        "unrelated_work_allowed": !must_follow,
        "must_read_prompt_text_before_reply": payload["chat_start_restore"]["prompt_text"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "required_action_kind_when_resume_required": required_action_kind,
        "no_silent_drop": resume_enforcement["no_silent_drop"]
            .as_bool()
            .unwrap_or(false),
    })
}

fn persist_startup_runtime_state_artifact(repo_root: &Path, payload: &Value) -> Result<()> {
    let output_path = startup_runtime_state_artifact_path(repo_root);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let artifact = build_startup_runtime_state_artifact(repo_root, payload, now_epoch_ms()?)?;
    let content = serde_json::to_string_pretty(&artifact)
        .context("failed to serialize startup runtime state artifact")?;
    fs::write(&output_path, content)
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    Ok(())
}

pub async fn print_restore(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<()> {
    let db = connect_bootstrapped_admin(cfg).await?;
    let context = load_startup_context(&db, args).await?;
    let restore = context.restore.ok_or_else(|| {
        anyhow!(
            "no working-state restore bundle found for {}::{}",
            context.project.code,
            context.namespace.code
        )
    })?;
    let chat_start_restore = build_chat_start_restore(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        Some(&restore),
    );
    let payload = build_continuity_restore_payload(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        &restore,
        &chat_start_restore,
    )?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn build_continuity_startup_payload(
    context: &ContinuityStartupContext,
    chat_start_restore: &Value,
) -> Result<Value> {
    let chat_start_node = &chat_start_restore["chat_start_restore"];
    let working_state_node = context
        .restore
        .as_ref()
        .and_then(|value| value.get("working_state_restore"));
    let startup_next_step = context.handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let prompt_text = chat_start_node["prompt_text"].as_str().unwrap_or_default();
    let start_headline = chat_start_node["headline"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let start_next_step = chat_start_node["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let working_state_expected = working_state_node.is_some_and(|node| {
        node["current_goal"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
            && node["next_step"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            && node["state_lineage"]["authoritative_event_id"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
    });
    let probes = vec![
        build_continuity_eval_probe(
            "startup_summary_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": context.continuity["imported_at_epoch_ms"].as_u64().unwrap_or(0) > 0
                    && context.continuity["documents_imported"].as_u64().unwrap_or(0) > 0
                    && context.handoff_summary["headline"].as_str().is_some_and(|value| !value.is_empty())
                    && !startup_next_step.is_empty(),
                "unexpected_present": false,
                "imported_at_epoch_ms": context.continuity["imported_at_epoch_ms"],
                "documents_imported": context.continuity["documents_imported"],
                "headline": context.handoff_summary["headline"],
                "next_step": startup_next_step,
            }),
        )?,
        build_continuity_eval_probe(
            "chat_start_restore_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": !prompt_text.is_empty()
                    && prompt_text.contains("CHAT_START_RESTORE")
                    && prompt_text.contains(start_headline)
                    && prompt_text.contains(&start_next_step),
                "unexpected_present": false,
                "headline": start_headline,
                "next_step": start_next_step,
                "prompt_text": prompt_text,
            }),
        )?,
        build_continuity_eval_probe(
            "working_state_restore_recovered_useful",
            if working_state_node.is_some() {
                "recovered_useful"
            } else {
                "under_retrieved"
            },
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": working_state_expected,
                "unexpected_present": false,
                "current_goal": working_state_node.and_then(|node| node["current_goal"].as_str()).unwrap_or(""),
                "next_step": working_state_node.and_then(|node| node["next_step"].as_str()).unwrap_or(""),
                "restore_confidence": working_state_node.map(|node| node["restore_confidence"].clone()).unwrap_or_else(|| json!("missing")),
                "authoritative_event_id": working_state_node.and_then(|node| node["state_lineage"]["authoritative_event_id"].as_str()).unwrap_or(""),
            }),
        )?,
    ];
    let canonical_eval = build_continuity_canonical_eval(&probes)?;
    let mut payload = serde_json::Map::new();
    payload.insert(
        "continuity_startup".to_string(),
        json!({
            "project": {
                "code": context.project.code,
                "display_name": context.project.display_name,
                "repo_root": context.project.repo_root,
            },
            "namespace": {
                "code": context.namespace.code,
                "display_name": context.namespace.display_name,
            },
            "imported_at_epoch_ms": context.continuity["imported_at_epoch_ms"],
            "documents_imported": context.continuity["documents_imported"],
            "rendered_transcript_files": context.continuity["rendered_transcript_files"],
            "handoff_summary": {
                "headline": context.handoff_summary["headline"],
                "next_step": context.handoff_summary["next_step"],
            },
            "canonical_eval": canonical_eval,
        }),
    );
    payload.insert("chat_start_restore".to_string(), chat_start_node.clone());
    if let Some(node) = working_state_node {
        payload.insert("working_state_restore".to_string(), node.clone());
    }
    payload.insert(
        "retrieval_science".to_string(),
        retrieval_science::suite_metadata("continuity_startup")?,
    );
    payload.insert(
        "degradation_policy".to_string(),
        retrieval_science::degradation_policy_json()?,
    );
    Ok(Value::Object(payload))
}

pub async fn verify_continuity(cfg: &AppConfig, args: &VerifyContinuityArgs) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    let startup_args = ContinuityStartupArgs {
        project: args.project.clone(),
        repo_root: args.repo_root.clone(),
        namespace: args.namespace.clone(),
        json: false,
        token_source_kind: "verify_continuity_startup".to_string(),
    };
    let context = load_startup_context(&db, &startup_args).await?;
    let direct_handoff_summary =
        latest_handoff_summary(&db, &context.project, &context.namespace).await?;
    let chat_start_restore = build_chat_start_restore(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        context.restore.as_ref(),
    );
    let chat_start_node = &chat_start_restore["chat_start_restore"];
    let working_state_restore = context
        .restore
        .as_ref()
        .and_then(|value| value.get("working_state_restore"))
        .cloned();
    let handoff_summary_present = direct_handoff_summary
        .as_ref()
        .and_then(|value| value["headline"].as_str())
        .is_some_and(|value| !value.trim().is_empty() && value != "ещё нет данных");
    let working_state_restore_present = working_state_restore.is_some();
    let chat_start_prompt_present = chat_start_node["prompt_text"]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty());

    let mut probes = vec![
        build_continuity_eval_probe(
            "handoff_summary_present",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": handoff_summary_present,
                "unexpected_present": false,
                "source": if direct_handoff_summary.is_some() { "continuity_handoff" } else { "continuity_import_fallback" },
                "headline": context.handoff_summary["headline"].as_str().unwrap_or(""),
            }),
        )?,
        build_continuity_eval_probe(
            "working_state_restore_present",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": working_state_restore_present,
                "unexpected_present": false,
                "restore_confidence": working_state_restore
                    .as_ref()
                    .and_then(|value| value["restore_confidence"].as_str())
                    .unwrap_or("missing"),
                "current_goal": working_state_restore
                    .as_ref()
                    .and_then(|value| value["current_goal"].as_str())
                    .unwrap_or(""),
            }),
        )?,
        build_continuity_eval_probe(
            "chat_start_prompt_present",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": chat_start_prompt_present,
                "unexpected_present": false,
                "prompt_length": chat_start_node["prompt_text"]
                    .as_str()
                    .map(|value| value.chars().count())
                    .unwrap_or(0),
            }),
        )?,
    ];
    probes.extend(continuity_replay_guard_probes()?);
    probes.extend(continuity_temporal_lookup_probes()?);
    let canonical_eval = build_continuity_canonical_eval(&probes)?;
    let failing_probes = probes
        .iter()
        .filter(|probe| probe.verdict_class != probe.expected_verdict_class)
        .map(|probe| {
            format!(
                "{}={} (expected {})",
                probe.name, probe.verdict_class, probe.expected_verdict_class
            )
        })
        .collect::<Vec<_>>();
    let verification_status = if failing_probes.is_empty() {
        "pass"
    } else {
        "critical"
    };
    let verified_probes = probes.len().saturating_sub(failing_probes.len()) as u64;

    let verification_run_id = Uuid::new_v4();
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let payload = json!({
        "_observability": {
            "source_event_id": verification_run_id,
            "source_kind": "continuity_verification_run",
            "scope_project_code": context.project.code,
            "scope_namespace_code": context.namespace.code,
            "captured_at_epoch_ms": captured_at_epoch_ms
        },
        "continuity_verification": {
            "verification_run_id": verification_run_id,
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "project": {
                "code": context.project.code,
                "display_name": context.project.display_name,
                "repo_root": context.project.repo_root,
            },
            "namespace": {
                "code": context.namespace.code,
                "display_name": context.namespace.display_name,
            },
            "handoff_summary_source": if direct_handoff_summary.is_some() { "continuity_handoff" } else { "continuity_import_fallback" },
            "handoff_summary": context.handoff_summary,
            "working_state_restore_present": working_state_restore_present,
            "working_state_restore": working_state_restore,
            "chat_start_restore": chat_start_node,
            "verification_status": verification_status,
            "probe_count": probes.len(),
            "verified_probes": verified_probes,
            "failed_probes": failing_probes,
            "canonical_eval": canonical_eval
        },
        "retrieval_science": retrieval_science::suite_metadata("continuity_verification")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    });
    let _ =
        postgres::insert_observability_snapshot(&db, "continuity_verification", &payload).await?;
    if verification_status != "pass" {
        return Err(anyhow!(
            "continuity verification failed: {}",
            payload["continuity_verification"]["failed_probes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn print_answer(cfg: &AppConfig, args: &ContinuityAnswerArgs) -> Result<()> {
    let db = connect_bootstrapped_admin(cfg).await?;
    let project = resolve_project(&db, &args.startup).await?;
    let namespace =
        postgres::find_namespace_by_code(&db, project.project_id, &args.startup.namespace)
            .await?
            .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.startup.namespace))?;
    let snapshots =
        postgres::list_observability_snapshots_by_kinds(&db, &["continuity_import"], Some(50))
            .await?;
    let latest = snapshots
        .into_iter()
        .find(|snapshot| {
            snapshot.payload["continuity_import"]["project"]["code"].as_str()
                == Some(project.code.as_str())
                && snapshot.payload["continuity_import"]["namespace"]["code"].as_str()
                    == Some(namespace.code.as_str())
        })
        .ok_or_else(|| {
            anyhow!(
                "no continuity import found for {}::{}",
                project.code,
                namespace.code
            )
        })?;
    let continuity = &latest.payload["continuity_import"];
    let handoff_summary = latest_handoff_summary(&db, &project, &namespace)
        .await?
        .unwrap_or_else(|| continuity["active_workline_summary"]["details"].clone());
    let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
    let current_thread_id = codex_threads::current_thread_id();
    let parsed_question = args.question.as_deref().and_then(|question| {
        chat_question::interpret(question, chat_question::current_local_now())
    });
    let messages_count = if args.messages_count != 2 {
        args.messages_count
    } else {
        parsed_question
            .as_ref()
            .map(|value| value.messages_count)
            .unwrap_or(args.messages_count)
    };
    let chat_reference = args.chat_reference.clone().or_else(|| {
        parsed_question
            .as_ref()
            .and_then(|value| value.chat_reference.clone())
    });
    let parsed_chat_reference = chat_reference
        .as_deref()
        .map(parse_chat_reference_spec)
        .unwrap_or(("current", 1));
    let at_time_rfc3339 = args.at_time_rfc3339.clone().or_else(|| {
        parsed_question
            .as_ref()
            .and_then(|value| value.at_time_rfc3339.clone())
    });
    let intent = resolve_answer_intent(
        &args.intent,
        parsed_question.as_ref().map(|value| value.intent.as_str()),
        Some(parsed_chat_reference.0),
        at_time_rfc3339.is_some(),
    );
    let include_chat_messages = if args.include_chat_messages {
        true
    } else {
        parsed_question
            .as_ref()
            .map(|value| value.include_chat_messages)
            .unwrap_or(false)
    };
    let wants_chat_lookup = include_chat_messages
        || at_time_rfc3339.is_some()
        || chat_reference.is_some()
        || intent == "previous_chat"
        || intent == "chat_at_time";
    let chat_tail = if wants_chat_lookup {
        let thread_index_snapshots = postgres::list_observability_snapshots_by_kinds(
            &db,
            &["continuity_thread_index"],
            Some(200),
        )
        .await?;
        if let Some(at_time_rfc3339) = &at_time_rfc3339 {
            codex_threads::chat_tail_at_time(&project.repo_root, at_time_rfc3339, messages_count)?
                .or(codex_threads::chat_tail_at_time_from_snapshots(
                    &thread_index_snapshots,
                    &project.code,
                    &namespace.code,
                    at_time_rfc3339,
                    messages_count,
                )?)
        } else {
            let (chat_reference_kind, chat_reference_offset) = if chat_reference.is_some() {
                parsed_chat_reference
            } else if intent == "previous_chat" {
                ("previous", 1)
            } else {
                ("current", 1)
            };
            match chat_reference_kind {
                "previous" => codex_threads::nth_previous_chat_tail(
                    &project.repo_root,
                    chat_reference_offset,
                    messages_count,
                )?
                .or(codex_threads::nth_previous_chat_tail_from_snapshots(
                    &thread_index_snapshots,
                    &project.code,
                    &namespace.code,
                    current_thread_id.as_deref(),
                    chat_reference_offset,
                    messages_count,
                )),
                "current" => codex_threads::current_chat_tail(&project.repo_root, messages_count)?
                    .or(codex_threads::current_chat_tail_from_snapshots(
                        &thread_index_snapshots,
                        &project.code,
                        &namespace.code,
                        current_thread_id.as_deref(),
                        messages_count,
                    )),
                _ => None,
            }
        }
    } else {
        None
    };
    let previous_chat_offset = if wants_chat_lookup && parsed_chat_reference.0 == "previous" {
        parsed_chat_reference.1
    } else {
        1
    };
    let answer_text = render_direct_answer(
        &handoff_summary,
        restore.as_ref(),
        chat_tail.as_ref(),
        &intent,
        at_time_rfc3339.as_deref(),
        previous_chat_offset,
    );
    if args.startup.json {
        let payload = build_continuity_answer_payload(
            &project,
            &namespace,
            &handoff_summary,
            restore.as_ref(),
            chat_tail.as_ref(),
            &intent,
            args.question.as_deref(),
            chat_reference.as_deref(),
            at_time_rfc3339.as_deref(),
            messages_count,
            include_chat_messages,
            previous_chat_offset,
            &answer_text,
        )?;
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{answer_text}");
    }
    Ok(())
}

pub async fn capture_handoff(cfg: &AppConfig, args: &ContinuityHandoffArgs) -> Result<()> {
    let mut db = connect_bootstrapped_admin(cfg).await?;
    let project = postgres::get_project_by_code(&db, &args.project).await?;
    let namespace = postgres::find_namespace_by_code(&db, project.project_id, &args.namespace)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.namespace))?;
    let details = read_optional_details_file(args.details_file.as_ref())?;
    let payload = capture_handoff_payload(
        cfg,
        &mut db,
        &project,
        &namespace,
        &args.headline,
        &args.next_step,
        &details,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn rotate_chat(cfg: &AppConfig, args: &ContinuityRotateChatArgs) -> Result<()> {
    let mut db = connect_bootstrapped_admin(cfg).await?;
    let startup_args = ContinuityStartupArgs {
        project: args.project.clone(),
        repo_root: args.repo_root.clone(),
        namespace: args.namespace.clone(),
        json: false,
        token_source_kind: "operator_continuity_rotate_chat".to_string(),
    };
    let (context, continuity_import_missing) = match load_startup_context(&db, &startup_args).await {
        Ok(context) => (context, false),
        Err(error)
            if error
                .to_string()
                .contains("no continuity import found for") =>
        {
            let project = resolve_project(&db, &startup_args).await?;
            let namespace = postgres::ensure_namespace(
                &db,
                project.project_id,
                &args.namespace,
                Some("Continuity"),
                "local_strict",
            )
            .await?;
            let handoff_summary = latest_handoff_summary(&db, &project, &namespace)
                .await?
                .unwrap_or_else(|| {
                    json!({
                        "headline": "ещё нет данных",
                        "next_step": "ещё нет данных",
                    })
                });
            let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
            (
                ContinuityStartupContext {
                    project,
                    namespace,
                    continuity: json!({
                        "imported_at_epoch_ms": 0,
                        "documents_imported": 0,
                        "rendered_transcript_files": 0,
                    }),
                    handoff_summary,
                    restore,
                },
                true,
            )
        }
        Err(error) => return Err(error),
    };
    let restore_node = context
        .restore
        .as_ref()
        .and_then(|value| value.get("working_state_restore"));
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(&db, context.restore.as_ref())
            .await?;
    let should_rotate_chat = client_budget_guard["should_rotate_chat_now"].as_bool() == Some(true)
        || client_budget_guard["should_rotate_chat_soon"].as_bool() == Some(true);
    if !should_rotate_chat && !args.force {
        let status_label = client_budget_guard["status_label"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or("guard не требует rotate");
        bail!(
            "client-budget rotate helper refused because guard is not active: {status_label}; pass --force only if you intentionally want to rotate despite clear guard"
        );
    }

    let recommended_headline = restore_node
        .and_then(|value| value["current_goal"].as_str())
        .filter(|value| is_meaningful_restore_value(value))
        .or_else(|| {
            context.handoff_summary["headline"]
                .as_str()
                .filter(|value| is_meaningful_restore_value(value))
        })
        .unwrap_or("Продолжить активную рабочую линию");
    let recommended_next_step = restore_node
        .and_then(|value| value["next_step"].as_str())
        .and_then(normalize_next_step_value)
        .filter(|value| is_meaningful_restore_value(value))
        .or_else(|| {
            context.handoff_summary["next_step"]
                .as_str()
                .and_then(normalize_next_step_value)
                .filter(|value| is_meaningful_restore_value(value))
        })
        .unwrap_or_else(|| "продолжить работу в свежем чате через continuity startup".to_string());
    let headline = args
        .headline
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(recommended_headline);
    let next_step = args
        .next_step
        .as_deref()
        .and_then(normalize_next_step_value)
        .unwrap_or(recommended_next_step);
    let preserves_return_obligation = restore_node
        .and_then(|value| value["execctl_resume_state"].as_str())
        .unwrap_or("clear")
        != "clear";
    let action_bundle = working_state::build_rotate_chat_action_bundle(
        Some(context.project.code.as_str()),
        Some(context.namespace.code.as_str()),
        Some(context.project.repo_root.as_str()),
        preserves_return_obligation,
        Some(headline),
        Some(&next_step),
    );
    let details = if let Some(details_file) = args.details_file.as_ref() {
        read_optional_details_file(Some(details_file))?
    } else {
        build_rotate_chat_details(&client_budget_guard, headline, &next_step)
    };
    let handoff_payload = capture_handoff_payload(
        cfg,
        &mut db,
        &context.project,
        &context.namespace,
        headline,
        &next_step,
        &details,
    )
    .await?;
    let blocking_reply_contract = client_budget_guard["reply_execution_gate"]["blocking_reply_contract"]
        .clone();
    let blocked_reply_text = blocking_reply_contract["template"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE)
        .to_string();
    let rotate_helper_command = action_bundle["operator_flow"]["rotate_helper_command"]
        .as_str()
        .map(ToOwned::to_owned);
    let startup_command = action_bundle["operator_flow"]["startup_command"]
        .as_str()
        .map(ToOwned::to_owned);
    let handoff_command = action_bundle["operator_flow"]["handoff_command"]
        .as_str()
        .map(ToOwned::to_owned);
    let continuity_import = if continuity_import_missing {
        let bootstrap_file =
            write_project_bootstrap_snapshot(&context.project.code, &context.project.repo_root)?;
        let import_args = ContinuityImportArgs {
            project: context.project.code.clone(),
            display_name: context.project.display_name.clone(),
            repo_root: PathBuf::from(&context.project.repo_root),
            namespace: context.namespace.code.clone(),
            bootstrap_file,
            thread_index_file: None,
            active_workline_file: None,
            memory_dir: None,
            transcript_limit: Some(3),
        };
        Some(import_sources_payload(cfg, &import_args).await?["continuity_import"].clone())
    } else {
        None
    };
    let payload = json!({
        "continuity_rotate_chat": {
            "status": if should_rotate_chat { "ready" } else { "forced" },
            "forced": args.force,
            "project": {
                "code": context.project.code,
                "display_name": context.project.display_name,
                "repo_root": context.project.repo_root,
            },
            "namespace": {
                "code": context.namespace.code,
                "display_name": context.namespace.display_name,
            },
            "client_budget_guard": client_budget_guard,
            "blocking_reply_contract": blocking_reply_contract,
            "blocked_reply_text": blocked_reply_text,
            "action_bundle": action_bundle,
            "handoff": handoff_payload["continuity_handoff"].clone(),
            "startup_requires_continuity_import": false,
            "continuity_import": continuity_import,
            "operator_flow": {
                "rotate_helper_command": rotate_helper_command,
                "handoff_command": handoff_command,
                "startup_command": startup_command,
            }
        }
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    let status_label = payload["continuity_rotate_chat"]["client_budget_guard"]["status_label"]
        .as_str()
        .unwrap_or("новый чат рекомендован");
    let last_request = payload["continuity_rotate_chat"]["client_budget_guard"]["last_request"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let client_limits = payload["continuity_rotate_chat"]["client_budget_guard"]["client_limits"]
        .as_str()
        .unwrap_or("ещё нет данных");
    println!("Amai continuity rotate-chat");
    println!();
    println!(
        "Проект: {} ({})",
        context.project.display_name, context.project.code
    );
    println!("Namespace continuity: {}", context.namespace.code);
    println!("Статус client-budget guard: {status_label}");
    println!("Последний запрос в модель: {last_request}");
    println!("Лимит клиента сейчас: {client_limits}");
    println!("Разрешённый короткий ответ в старом чате: {blocked_reply_text}");
    println!();
    println!("Handoff записан:");
    println!("- headline: {headline}");
    println!("- next_step: {next_step}");
    println!(
        "- local_path: {}",
        payload["continuity_rotate_chat"]["handoff"]["local_path"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    println!();
    println!("Готовые действия:");
    if let Some(imported_at) = payload["continuity_rotate_chat"]["continuity_import"]["imported_at_epoch_ms"]
        .as_u64()
    {
        println!(
            "- Continuity import materialized: {}",
            human_epoch_ms(Some(imported_at))
        );
    }
    if let Some(command) = payload["continuity_rotate_chat"]["operator_flow"]["startup_command"]
        .as_str()
    {
        println!("- После открытия свежего чата запусти: {command}");
    }
    if let Some(command) = payload["continuity_rotate_chat"]["operator_flow"]["rotate_helper_command"]
        .as_str()
    {
        println!("- One-shot helper: {command}");
    }
    Ok(())
}

fn read_optional_details_file(details_file: Option<&PathBuf>) -> Result<String> {
    if let Some(details_file) = details_file {
        return fs::read_to_string(details_file)
            .with_context(|| format!("failed to read {}", details_file.display()));
    }
    Ok(String::new())
}

fn build_rotate_chat_details(client_budget_guard: &Value, headline: &str, next_step: &str) -> String {
    let mut lines = Vec::new();
    if let Some(status_label) = client_budget_guard["status_label"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Client-budget guard: {status_label}."));
    }
    if let Some(last_request) = client_budget_guard["last_request"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Последний запрос в модель: {last_request}."));
    }
    if let Some(client_limits) = client_budget_guard["client_limits"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Лимит клиента сейчас: {client_limits}."));
    }
    if let Some(note) = client_budget_guard["note"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Почему rotate обязателен: {note}."));
    }
    lines.push(format!(
        "Продолжить ту же рабочую линию: {headline}."
    ));
    lines.push(format!(
        "Ближайший обязательный следующий шаг в свежем чате: {next_step}."
    ));
    lines.join("\n")
}

fn write_project_bootstrap_snapshot(project_code: &str, repo_root: &str) -> Result<PathBuf> {
    let memory_home = std::env::var("MEMORY_HOME")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(".memory")
        });
    let thread_index_path = memory_home
        .join("transcripts")
        .join("codex")
        .join("thread_index.json");
    let index = if thread_index_path.is_file() {
        let raw = fs::read_to_string(&thread_index_path)
            .with_context(|| format!("failed to read {}", thread_index_path.display()))?;
        let mut index: ContinuityThreadIndexFile = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", thread_index_path.display()))?;
        index.threads.retain(|entry| entry.cwd.starts_with(repo_root));
        index.threads.sort_by(|left, right| right.source_rollout.cmp(&left.source_rollout));
        index
    } else {
        ContinuityThreadIndexFile {
            threads: Vec::new(),
        }
    };
    let amai_repo_root = crate::config::discover_repo_root(None)?;
    let output_path = amai_repo_root
        .join("state")
        .join("continuity-imports")
        .join(project_code)
        .join(format!(
            "continuity-snapshot-{}.md",
            slugify_repo_root(repo_root)
        ));
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let handoff_path = amai_repo_root
        .join("state")
        .join("continuity-imports")
        .join(project_code)
        .join("live-handoff.md");
    let mut lines = vec![
        "# Amai Project Continuity Snapshot".to_string(),
        String::new(),
        format!("- `cwd_prefix`: `{repo_root}`"),
        format!("- `thread_count`: `{}`", index.threads.len()),
        "- `purpose`: локальный transcript-based continuity snapshot для проекта. Он импортируется в `Amai` и служит refresh-evidence, а не отдельной системой памяти.".to_string(),
        "- `important`: канонический current handoff теперь живёт в `Amai`; полная история остаётся в `raw_mirror` и `rendered_transcript`.".to_string(),
        format!(
            "- `current_handoff_snapshot`: `{}`",
            handoff_path.display()
        ),
        String::new(),
        "## Индекс потоков проекта".to_string(),
        String::new(),
    ];
    for entry in &index.threads {
        lines.push(format!("### {}", entry.thread_id));
        lines.push(String::new());
        lines.push(format!("- `title`: `{}`", entry.title));
        lines.push(format!("- `cwd`: `{}`", entry.cwd));
        if !entry.first_user_message.is_empty() {
            lines.push(format!(
                "- `first_user_message`: `{}`",
                entry.first_user_message
            ));
        }
        lines.push(format!("- `source_rollout`: `{}`", entry.source_rollout));
        if !entry.raw_mirror.is_empty() {
            lines.push(format!("- `raw_mirror`: `{}`", entry.raw_mirror));
        }
        lines.push(format!(
            "- `rendered_transcript`: `{}`",
            entry.rendered_transcript
        ));
        lines.push(String::new());
    }
    fs::write(&output_path, lines.join("\n") + "\n")
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    Ok(output_path)
}

fn slugify_repo_root(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if keep {
            slug.push(ch);
            previous_was_dash = false;
        } else if !previous_was_dash {
            slug.push('-');
            previous_was_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

fn is_meaningful_restore_value(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed != "ещё нет данных"
}

async fn capture_handoff_payload(
    cfg: &AppConfig,
    db: &mut Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    headline: &str,
    next_step: &str,
    details: &str,
) -> Result<Value> {
    let captured_at_epoch_ms = now_epoch_ms()?;
    let body = render_handoff_markdown(headline, next_step, details);
    let amai_repo_root = crate::config::discover_repo_root(None)?;
    let local_handoff_path = amai_repo_root
        .join("state")
        .join("continuity-imports")
        .join(&project.code)
        .join("live-handoff.md");
    if let Some(parent) = local_handoff_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&local_handoff_path, &body)
        .with_context(|| format!("failed to write {}", local_handoff_path.display()))?;
    let document = build_document_record(
        &project,
        &namespace,
        &ContinuitySource {
            original_path: PathBuf::from("Amai continuity handoff"),
            relative_path: ".amai-continuity/live-handoff/HANDOFF.md".to_string(),
            source_kind: "continuity_handoff".to_string(),
            artifact_bucket: cfg.s3_bucket_artifacts.clone(),
            artifact_kind: "continuity_handoff".to_string(),
        },
        &body,
        &body,
        json!({
            "continuity_kind": "continuity_handoff",
            "captured_at_epoch_ms": captured_at_epoch_ms,
        }),
    )?;
    let chunks = build_chunks(cfg, &body);
    postgres::replace_document_index(db, &document, &[], &chunks).await?;
    let payload = json!({
        "continuity_handoff": {
            "project": {
                "code": project.code,
                "display_name": project.display_name,
                "repo_root": project.repo_root,
            },
            "namespace": {
                "code": namespace.code,
                "display_name": namespace.display_name,
            },
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "headline": headline,
            "next_step": next_step,
            "details": details,
            "relative_path": ".amai-continuity/live-handoff/HANDOFF.md",
            "local_path": local_handoff_path.display().to_string(),
        }
    });
    let _ = postgres::insert_observability_snapshot(&db, "continuity_handoff", &payload).await?;
    working_state::record_handoff_event(
        &db,
        &project,
        &namespace,
        headline,
        next_step,
        &details,
        &local_handoff_path.display().to_string(),
    )
    .await?;
    Ok(payload)
}

fn build_continuity_answer_payload(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    handoff_summary: &Value,
    restore: Option<&Value>,
    chat_tail: Option<&codex_threads::ChatTail>,
    intent: &str,
    question: Option<&str>,
    chat_reference: Option<&str>,
    at_time_rfc3339: Option<&str>,
    messages_count: usize,
    include_chat_messages: bool,
    previous_chat_offset: usize,
    answer_text: &str,
) -> Result<Value> {
    let probe = build_continuity_answer_probe(
        handoff_summary,
        chat_tail,
        intent,
        at_time_rfc3339,
        previous_chat_offset,
        answer_text,
    )?;
    let canonical_eval = build_continuity_canonical_eval(std::slice::from_ref(&probe))?;
    let restore_node = restore.map(|value| &value["working_state_restore"]);
    let included_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "included")
    });
    let excluded_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "not_included")
    });
    Ok(json!({
        "continuity_answer": {
            "project": {
                "code": project.code,
                "display_name": project.display_name,
                "repo_root": project.repo_root,
            },
            "namespace": {
                "code": namespace.code,
                "display_name": namespace.display_name,
            },
            "question": question,
            "intent": intent,
            "chat_reference": chat_reference,
            "at_time_rfc3339": at_time_rfc3339,
            "messages_count": messages_count,
            "include_chat_messages": include_chat_messages,
            "previous_chat_offset": previous_chat_offset,
            "handoff_summary": {
                "headline": handoff_summary["headline"].as_str().unwrap_or("ещё нет данных"),
                "next_step": handoff_summary["next_step"].as_str().unwrap_or("ещё нет данных"),
            },
            "restore_present": restore.is_some(),
            "included_reasons_summary": included_reasons_summary,
            "excluded_reasons_summary": excluded_reasons_summary,
            "chat_lookup": {
                "found": chat_tail.is_some(),
                "thread_id": chat_tail.map(|value| value.thread_id.as_str()),
                "title": chat_tail.map(|value| value.title.as_str()),
                "summary_headline": chat_tail.and_then(|value| value.summary_headline.as_deref()),
                "summary_next_step": chat_tail.and_then(|value| value.summary_next_step.as_deref()),
                "messages_count": chat_tail.map(|value| value.messages.len()).unwrap_or(0),
                "selected_time_slice": chat_tail
                    .and_then(|value| value.selected_time_slice.as_ref())
                    .map(|slice| {
                        json!({
                            "started_at": slice.started_at,
                            "ended_at": slice.ended_at,
                            "summary_headline": slice.summary_headline,
                            "summary_next_step": slice.summary_next_step,
                            "user_anchor": slice.user_anchor,
                            "assistant_anchor": slice.assistant_anchor,
                        })
                    }),
            },
            "answer_text": answer_text,
            "canonical_eval": canonical_eval,
        },
        "retrieval_science": retrieval_science::suite_metadata("continuity_answer")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    }))
}

fn build_continuity_restore_payload(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    continuity: &Value,
    handoff_summary: &Value,
    restore: &Value,
    chat_start_restore: &Value,
) -> Result<Value> {
    let chat_start_node = &chat_start_restore["chat_start_restore"];
    let working_state_node = &restore["working_state_restore"];
    let prompt_text = chat_start_node["prompt_text"].as_str().unwrap_or_default();
    let start_headline = chat_start_node["headline"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let start_next_step = chat_start_node["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let current_goal = working_state_node["current_goal"]
        .as_str()
        .unwrap_or_default();
    let restore_next_step = working_state_node["next_step"].as_str().unwrap_or_default();
    let authoritative_event_id = working_state_node["state_lineage"]["authoritative_event_id"]
        .as_str()
        .unwrap_or_default();
    let probes = vec![
        build_continuity_eval_probe(
            "chat_start_restore_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": !prompt_text.is_empty()
                    && prompt_text.contains("CHAT_START_RESTORE")
                    && prompt_text.contains(start_headline)
                    && prompt_text.contains(&start_next_step),
                "unexpected_present": false,
                "headline": start_headline,
                "next_step": start_next_step,
                "prompt_text": prompt_text,
            }),
        )?,
        build_continuity_eval_probe(
            "working_state_restore_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": !current_goal.is_empty()
                    && !restore_next_step.is_empty()
                    && !authoritative_event_id.is_empty(),
                "unexpected_present": false,
                "current_goal": current_goal,
                "next_step": restore_next_step,
                "restore_confidence": working_state_node["restore_confidence"],
                "authoritative_event_id": authoritative_event_id,
            }),
        )?,
    ];
    let canonical_eval = build_continuity_canonical_eval(&probes)?;
    Ok(json!({
        "continuity_restore": {
            "project": {
                "code": project.code,
                "display_name": project.display_name,
                "repo_root": project.repo_root,
            },
            "namespace": {
                "code": namespace.code,
                "display_name": namespace.display_name,
            },
            "imported_at_epoch_ms": continuity["imported_at_epoch_ms"],
            "handoff_summary": {
                "headline": handoff_summary["headline"],
                "next_step": handoff_summary["next_step"],
            },
            "canonical_eval": canonical_eval,
        },
        "chat_start_restore": chat_start_node.clone(),
        "working_state_restore": working_state_node.clone(),
        "retrieval_science": retrieval_science::suite_metadata("continuity_restore")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    }))
}

fn build_continuity_answer_probe(
    handoff_summary: &Value,
    chat_tail: Option<&codex_threads::ChatTail>,
    intent: &str,
    at_time_rfc3339: Option<&str>,
    previous_chat_offset: usize,
    answer_text: &str,
) -> Result<ContinuityEvalProbe> {
    let project_headline = handoff_summary["headline"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let project_next_step = handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let first_line = answer_text.lines().next().unwrap_or_default();
    match intent {
        "previous_chat" => {
            if let Some(chat_tail) = chat_tail {
                let expected_fragments = continuity_answer_expected_fragments(chat_tail);
                let expected_present = expected_fragments
                    .iter()
                    .any(|fragment| !fragment.is_empty() && answer_text.contains(fragment));
                let stale_substitution = first_line.contains(project_headline)
                    && !expected_fragments
                        .iter()
                        .any(|fragment| !fragment.is_empty() && first_line.contains(fragment));
                return build_continuity_eval_probe(
                    "previous_chat_answer_recovered_useful",
                    "recovered_useful",
                    EvalPattern::RecoveryTarget,
                    true,
                    json!({
                        "expected_present": expected_present,
                        "unexpected_present": stale_substitution,
                        "intent": "previous_chat",
                        "offset": previous_chat_offset,
                        "expected_fragments": expected_fragments,
                        "answer": answer_text,
                    }),
                );
            }
            let fail_closed_ok = answer_text.contains(
                "На чём закончился прошлый чат: для такого смещения назад нет известного чата.",
            ) && answer_text.contains(project_headline)
                && answer_text.contains(&project_next_step);
            return build_continuity_eval_probe(
                "previous_chat_answer_fail_closed",
                "hit_correct_target",
                EvalPattern::IsolationBoundary,
                false,
                json!({
                    "boundary_clean": fail_closed_ok,
                    "fail_closed_ok": fail_closed_ok,
                    "unexpected_present": answer_mentions_temporal_match(answer_text),
                    "intent": "previous_chat",
                    "offset": previous_chat_offset,
                    "answer": answer_text,
                }),
            );
        }
        "chat_at_time" => {
            if let Some(chat_tail) = chat_tail {
                let expected_fragments = continuity_answer_expected_fragments(chat_tail);
                let expected_present = expected_fragments
                    .iter()
                    .any(|fragment| !fragment.is_empty() && answer_text.contains(fragment))
                    && at_time_rfc3339.is_none_or(|target_time| answer_text.contains(target_time));
                let stale_substitution = first_line.contains(project_headline)
                    && !expected_fragments
                        .iter()
                        .any(|fragment| !fragment.is_empty() && first_line.contains(fragment));
                return build_continuity_eval_probe(
                    "exact_time_answer_recovered_useful",
                    "recovered_useful",
                    EvalPattern::RecoveryTarget,
                    true,
                    json!({
                        "expected_present": expected_present,
                        "unexpected_present": stale_substitution,
                        "intent": "chat_at_time",
                        "target_time": at_time_rfc3339,
                        "expected_fragments": expected_fragments,
                        "answer": answer_text,
                    }),
                );
            }
            let fail_closed_ok = answer_text.contains(
                "Что было в чате на этот момент: для этого момента нет точного совпадения в известных чатах.",
            ) && answer_text.contains(project_headline)
                && answer_text.contains(&project_next_step)
                && at_time_rfc3339
                    .is_none_or(|target_time| answer_text.contains(target_time));
            return build_continuity_eval_probe(
                "exact_time_answer_fail_closed",
                "hit_correct_target",
                EvalPattern::IsolationBoundary,
                false,
                json!({
                    "boundary_clean": fail_closed_ok,
                    "fail_closed_ok": fail_closed_ok,
                    "unexpected_present": answer_mentions_temporal_match(answer_text),
                    "intent": "chat_at_time",
                    "target_time": at_time_rfc3339,
                    "answer": answer_text,
                }),
            );
        }
        _ => {}
    }
    build_continuity_eval_probe(
        "continuity_answer_recovered_useful",
        "recovered_useful",
        EvalPattern::RecoveryTarget,
        true,
        json!({
            "expected_present": answer_text.contains(project_headline)
                && answer_text.contains(&project_next_step),
            "unexpected_present": false,
            "intent": intent,
            "answer": answer_text,
        }),
    )
}

fn continuity_answer_expected_fragments(chat_tail: &codex_threads::ChatTail) -> Vec<String> {
    let answer_headline = summarize_chat_tail_headline(chat_tail).unwrap_or_default();
    let answer_next_step = extract_chat_tail_next_step(chat_tail).unwrap_or_default();
    let mut fragments = vec![answer_headline.clone(), answer_next_step];
    if let Some(label) = select_chat_tail_label(chat_tail, &answer_headline) {
        fragments.push(label);
    }
    if let Some(slice) = chat_tail.selected_time_slice.as_ref() {
        fragments.push(collapse_answer_text(&slice.user_anchor, 220));
        fragments.push(collapse_answer_text(&slice.summary_headline, 220));
    }
    for message in &chat_tail.messages {
        fragments.push(collapse_answer_text(&message.text, 320));
    }
    fragments.retain(|fragment| !fragment.trim().is_empty());
    fragments
}

fn answer_mentions_temporal_match(answer_text: &str) -> bool {
    [
        "Предыдущий чат по времени:",
        "Подходящий chat thread:",
        "Найденный chat thread:",
        "Последние сообщения предыдущего чата:",
        "Ближайшие сообщения к этому моменту:",
        "Смысловой срез времени:",
    ]
    .iter()
    .any(|needle| answer_text.contains(needle))
}

fn continuity_strategy_label(strategy: &str) -> &str {
    match strategy {
        "exact_documents" => "точные совпадения",
        "symbol_hits" => "совпадения по символам",
        "lexical_chunks" => "текстовые фрагменты",
        "semantic_chunks" => "смысловые фрагменты",
        _ => strategy,
    }
}

fn continuity_decision_trace_summary(trace: Option<&Value>, key: &str) -> Option<String> {
    let items = trace?.get(key)?.as_array()?;
    let parts = items
        .iter()
        .take(3)
        .filter_map(|item| {
            let reason = item["reason"].as_str()?.trim();
            if reason.is_empty() {
                return None;
            }
            let strategy = continuity_strategy_label(item["strategy"].as_str().unwrap_or_default());
            let count = item["count"].as_u64();
            Some(match count {
                Some(value) if value > 0 => format!("{strategy} ({value}) — {reason}"),
                _ => format!("{strategy} — {reason}"),
            })
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(collapse_answer_text(&parts.join(" • "), 260))
    }
}

fn render_direct_answer(
    handoff_summary: &Value,
    restore: Option<&Value>,
    chat_tail: Option<&codex_threads::ChatTail>,
    intent: &str,
    at_time_rfc3339: Option<&str>,
    previous_chat_offset: usize,
) -> String {
    let heading = match intent {
        "continue" => "Продолжаем с этой линии:",
        "handoff" => "Текущий handoff в Amai:",
        "previous_chat" => "На чём закончился прошлый чат:",
        "chat_at_time" => "Что было в чате на этот момент:",
        _ => "На чём остановились:",
    };
    let headline = handoff_summary["headline"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let project_next_step = handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let thread_headline = if matches!(intent, "previous_chat" | "chat_at_time") {
        chat_tail.and_then(summarize_chat_tail_headline)
    } else {
        None
    };
    let thread_next_step = if matches!(intent, "previous_chat" | "chat_at_time") {
        chat_tail.and_then(extract_chat_tail_next_step)
    } else {
        None
    };
    if intent == "chat_at_time" && chat_tail.is_none() {
        let mut lines = vec![format!(
            "{heading} для этого момента нет точного совпадения в известных чатах."
        )];
        if let Some(at_time_rfc3339) = at_time_rfc3339 {
            lines.push(format!("Целевой момент времени: {at_time_rfc3339}"));
        }
        lines.push(format!("Текущая активная линия проекта сейчас: {headline}"));
        lines.push(format!(
            "Ближайший обязательный следующий шаг: {project_next_step}"
        ));
        return lines.join("\n");
    }
    if intent == "previous_chat" && chat_tail.is_none() {
        let mut lines = vec![
            "На чём закончился прошлый чат: для такого смещения назад нет известного чата."
                .to_string(),
        ];
        if previous_chat_offset > 1 {
            lines.push(format!("Смещение назад по чатам: {previous_chat_offset}"));
        }
        lines.push(format!("Текущая активная линия проекта сейчас: {headline}"));
        lines.push(format!(
            "Ближайший обязательный следующий шаг: {project_next_step}"
        ));
        return lines.join("\n");
    }
    let answer_headline = thread_headline.as_deref().unwrap_or(headline);
    let answer_next_step = thread_next_step.as_deref().unwrap_or(&project_next_step);

    let mut lines = vec![format!("{heading} {answer_headline}")];
    if intent == "previous_chat" && previous_chat_offset > 1 {
        lines.push(format!("Смещение назад по чатам: {previous_chat_offset}"));
    }
    if intent != "previous_chat"
        && intent != "chat_at_time"
        && let Some(restore_node) = restore.map(|value| &value["working_state_restore"])
    {
        if let Some(summary) = summarize_startup_materialized_notes(restore_node) {
            lines.push(format!("Что уже materialized: {summary}"));
        }
        if let Some(summary) = summarize_recent_actions(&restore_node["recent_actions"]) {
            lines.push(format!("Последние действия: {summary}"));
        }
        if let Some(summary) = summarize_string_list(&restore_node["active_files"], 3) {
            lines.push(format!("Активные файлы: {summary}"));
        }
        if let Some(summary) = continuity_decision_trace_summary(
            Some(&restore_node["latest_decision_trace"]),
            "included",
        ) {
            lines.push(format!("Почему вошёл текущий контекст: {summary}"));
        }
        if let Some(summary) = continuity_decision_trace_summary(
            Some(&restore_node["latest_decision_trace"]),
            "not_included",
        ) {
            lines.push(format!("Почему часть не вошла: {summary}"));
        }
        if restore_node["restore_confidence"].as_str() == Some("preliminary") {
            lines.push(
                "Статус continuity: предварительно, потому что живая выборка ещё маленькая."
                    .to_string(),
            );
        }
    }
    if let Some(chat_tail) = chat_tail {
        let title = select_chat_tail_label(chat_tail, answer_headline);
        if let Some(at_time_rfc3339) = at_time_rfc3339 {
            lines.push(format!("Целевой момент времени: {at_time_rfc3339}"));
            if let Some(slice) = chat_tail.selected_time_slice.as_ref() {
                let from = if slice.started_at.is_empty() {
                    "неизвестно"
                } else {
                    slice.started_at.as_str()
                };
                let to = if slice.ended_at.is_empty() {
                    "неизвестно"
                } else {
                    slice.ended_at.as_str()
                };
                lines.push(format!("Смысловой срез времени: {from} -> {to}"));
                if !slice.user_anchor.is_empty() {
                    lines.push(format!(
                        "О чём шёл разговор в этот момент: {}",
                        collapse_answer_text(&slice.user_anchor, 220)
                    ));
                }
            }
            if let Some(title) = title.as_deref() {
                lines.push(format!("Подходящий chat thread: {title}"));
            }
        } else if intent == "previous_chat" {
            if let Some(title) = title.as_deref() {
                lines.push(format!("Предыдущий чат по времени: {title}"));
            }
        } else {
            if let Some(title) = title.as_deref() {
                lines.push(format!("Найденный chat thread: {title}"));
            }
        }
        if !chat_tail.messages.is_empty() {
            let label = if at_time_rfc3339.is_some() {
                "Ближайшие сообщения к этому моменту:"
            } else if intent == "previous_chat" {
                "Последние сообщения предыдущего чата:"
            } else {
                "Последние сообщения этого чата:"
            };
            lines.push(label.to_string());
            for message in &chat_tail.messages {
                let role = if message.role == "user" {
                    "Ваше"
                } else {
                    "Моё"
                };
                lines.push(format!(
                    "- {role}: {}",
                    collapse_answer_text(&message.text, 320)
                ));
            }
        }
    }
    if matches!(intent, "previous_chat" | "chat_at_time")
        && headline != "ещё нет данных"
        && headline != answer_headline
    {
        lines.push(format!("Текущая активная линия проекта сейчас: {headline}"));
    }
    lines.push(format!(
        "Ближайший обязательный следующий шаг: {answer_next_step}"
    ));
    lines.join("\n")
}

pub fn degradation_proof_scenarios() -> Result<Vec<Value>> {
    let handoff = json!({
        "headline": "Partial refresh line",
        "next_step": "Finish continuity refresh."
    });
    let partial_refresh_answer =
        render_direct_answer(&handoff, None, None, "previous_chat", None, 2);
    let partial_refresh_pass = partial_refresh_answer
        .contains("На чём закончился прошлый чат: для такого смещения назад нет известного чата.")
        && partial_refresh_answer
            .contains("Текущая активная линия проекта сейчас: Partial refresh line")
        && partial_refresh_answer
            .contains("Ближайший обязательный следующий шаг: Finish continuity refresh.");

    let partial_thread_snapshots = vec![postgres::ObservabilitySnapshotRecord {
        snapshot_id: Uuid::new_v4(),
        snapshot_kind: "continuity_thread_index".to_string(),
        created_at_epoch_ms: 1_744_087_814_000,
        payload: json!({
            "continuity_thread_index": {
                "project": {"code": "art"},
                "namespace": {"code": "continuity"},
                "thread_id": "thread-wide",
                "title": "длинный thread",
                "started_at": "2026-03-18T11:00:00+03:00",
                "ended_at": "2026-03-21T12:00:00+03:00",
                "created_at_epoch_s": 1742284800,
                "updated_at_epoch_s": 1742557200,
                "time_slices": [
                    {
                        "started_at": "2026-03-21T02:25:33.619Z",
                        "ended_at": "2026-03-21T02:27:31.157Z",
                        "started_at_epoch_s": 1742523933,
                        "ended_at_epoch_s": 1742524051,
                        "user_anchor": "шумный вопрос",
                        "assistant_anchor": "шумный ответ",
                        "summary_headline": "слишком далёкий смысловой срез",
                        "summary_next_step": ""
                    }
                ]
            }
        }),
    }];
    let target_time = "2026-03-18T12:00:00+03:00";
    let partial_thread_tail = codex_threads::chat_tail_at_time_from_snapshots(
        &partial_thread_snapshots,
        "art",
        "continuity",
        target_time,
        2,
    )?;
    let partial_thread_answer = render_direct_answer(
        &handoff,
        None,
        partial_thread_tail.as_ref(),
        "chat_at_time",
        Some(target_time),
        1,
    );
    let partial_thread_index_pass = partial_thread_tail.is_none()
        && partial_thread_answer.contains(
            "Что было в чате на этот момент: для этого момента нет точного совпадения в известных чатах.",
        )
        && partial_thread_answer.contains(target_time);

    Ok(vec![
        json!({
            "class_key": "partial_refresh",
            "title": "Неполный refresh",
            "status": if partial_refresh_pass { "pass" } else { "critical" },
            "reason": if partial_refresh_pass {
                "continuity answer не маскирует неполный refresh под найденный previous chat и честно сообщает, что такого смещения назад пока нет."
            } else {
                "continuity answer подменил неполный refresh выдуманным previous-chat ответом."
            },
            "details": {
                "answer": partial_refresh_answer,
            }
        }),
        json!({
            "class_key": "partial_thread_index",
            "title": "Неполный thread index",
            "status": if partial_thread_index_pass { "pass" } else { "critical" },
            "reason": if partial_thread_index_pass {
                "temporal lookup fail-closed отбрасывает слишком далёкий time-slice и честно пишет, что точного совпадения нет."
            } else {
                "temporal lookup подменил неполный thread index случайным соседним chat slice."
            },
            "details": {
                "target_time": target_time,
                "answer": partial_thread_answer,
            }
        }),
    ])
}

fn build_continuity_eval_probe(
    name: &'static str,
    expected_verdict_class: &'static str,
    pattern: EvalPattern,
    has_expected_target: bool,
    details: Value,
) -> Result<ContinuityEvalProbe> {
    let signals = EvalSignals::from_details(&details, has_expected_target);
    let verdict = eval_verdict::derive_eval_verdict(pattern, &signals)?;
    Ok(ContinuityEvalProbe {
        name,
        expected_verdict_class,
        verdict_class: verdict.class_key,
        verdict_reason: verdict.reason,
        details,
    })
}

fn build_continuity_canonical_eval(probes: &[ContinuityEvalProbe]) -> Result<Value> {
    let mut summary = eval_verdict::summarize_eval_layer(
        probes.iter().map(|probe| probe.verdict_class.as_str()),
    )?;
    summary["probes"] = json!(
        probes
            .iter()
            .map(|probe| {
                json!({
                    "name": probe.name,
                    "expected_eval_verdict_class": probe.expected_verdict_class,
                    "eval_verdict_class": probe.verdict_class,
                    "eval_reason": probe.verdict_reason,
                    "details": probe.details,
                })
            })
            .collect::<Vec<_>>()
    );
    Ok(summary)
}

fn continuity_replay_guard_probes() -> Result<Vec<ContinuityEvalProbe>> {
    let replayed_stale = fake_continuity_handoff_snapshot(3_000, 1_000, "Stale replay");
    let fresh = fake_continuity_handoff_snapshot(2_000, 2_000, "Fresh handoff");
    let handoff_snapshots = vec![replayed_stale, fresh];
    let selected_handoff = latest_scoped_snapshot(
        &handoff_snapshots,
        "continuity_handoff",
        "art",
        "continuity",
        |root| {
            !is_meta_continuity_handoff(
                root["headline"].as_str().unwrap_or_default(),
                root["next_step"].as_str().unwrap_or_default(),
                root["details"].as_str().unwrap_or_default(),
            )
        },
    )
    .ok_or_else(|| anyhow!("synthetic continuity handoff replay proof selected nothing"))?;
    let handoff_replay_rejected = selected_handoff.payload["continuity_handoff"]["headline"]
        .as_str()
        == Some("Fresh handoff")
        && continuity_snapshot_semantic_epoch_ms(selected_handoff, "continuity_handoff") == 2_000;

    let replayed_stale = fake_continuity_import_snapshot(3_000, 1_000, "Stale import");
    let fresh = fake_continuity_import_snapshot(2_000, 2_000, "Fresh import");
    let import_snapshots = vec![replayed_stale, fresh];
    let selected_import = latest_scoped_snapshot(
        &import_snapshots,
        "continuity_import",
        "art",
        "continuity",
        |_| true,
    )
    .ok_or_else(|| anyhow!("synthetic continuity import replay proof selected nothing"))?;
    let import_replay_rejected = selected_import.payload["continuity_import"]["active_workline_summary"]["details"]["headline"]
        .as_str()
        == Some("Fresh import")
        && continuity_snapshot_semantic_epoch_ms(selected_import, "continuity_import") == 2_000;

    Ok(vec![
        build_continuity_eval_probe(
            "handoff_replay_rejected",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": handoff_replay_rejected,
                "unexpected_present": !handoff_replay_rejected,
                "selected_headline": selected_handoff.payload["continuity_handoff"]["headline"]
                    .as_str()
                    .unwrap_or(""),
                "selected_semantic_epoch_ms": continuity_snapshot_semantic_epoch_ms(
                    selected_handoff,
                    "continuity_handoff"
                ),
                "expected_headline": "Fresh handoff",
                "expected_semantic_epoch_ms": 2_000,
            }),
        )?,
        build_continuity_eval_probe(
            "import_replay_rejected",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": import_replay_rejected,
                "unexpected_present": !import_replay_rejected,
                "selected_headline": selected_import.payload["continuity_import"]["active_workline_summary"]["details"]["headline"]
                    .as_str()
                    .unwrap_or(""),
                "selected_semantic_epoch_ms": continuity_snapshot_semantic_epoch_ms(
                    selected_import,
                    "continuity_import"
                ),
                "expected_headline": "Fresh import",
                "expected_semantic_epoch_ms": 2_000,
            }),
        )?,
    ])
}

fn continuity_temporal_lookup_probes() -> Result<Vec<ContinuityEvalProbe>> {
    let handoff = json!({
        "headline": "Current project line",
        "next_step": "Current project next step."
    });
    let previous_chat = codex_threads::ChatTail {
        thread_id: "thread-2".to_string(),
        title: "чат про continuity".to_string(),
        summary_headline: Some("Закончили на temporal contour.".to_string()),
        summary_next_step: Some("Проверить новый чат ещё раз.".to_string()),
        selected_time_slice: None,
        messages: vec![
            codex_threads::TranscriptMessage {
                role: "user".to_string(),
                text: "на чем закончили?".to_string(),
            },
            codex_threads::TranscriptMessage {
                role: "assistant".to_string(),
                text: "Закончили на temporal contour.\nБлижайший обязательный следующий шаг: Проверить новый чат ещё раз.".to_string(),
            },
        ],
    };
    let previous_answer = render_direct_answer(
        &handoff,
        None,
        Some(&previous_chat),
        "previous_chat",
        None,
        1,
    );
    let previous_chat_recovered = previous_answer
        .contains("На чём закончился прошлый чат: Закончили на temporal contour.")
        && previous_answer
            .contains("Ближайший обязательный следующий шаг: Проверить новый чат ещё раз.")
        && previous_answer.contains("Предыдущий чат по времени: чат про continuity");

    let exact_time = "2026-03-19T12:00:00+03:00";
    let exact_time_chat = codex_threads::ChatTail {
        thread_id: "thread-1".to_string(),
        title: "чат про continuity".to_string(),
        summary_headline: Some("про temporal lookup".to_string()),
        summary_next_step: None,
        selected_time_slice: Some(codex_threads::ThreadTimeSliceSummary {
            started_at: "2026-03-19T11:59:20+03:00".to_string(),
            ended_at: "2026-03-19T12:01:10+03:00".to_string(),
            started_at_epoch_s: 1,
            ended_at_epoch_s: 2,
            user_anchor: "разбирали temporal lookup и его точный смысловой ответ".to_string(),
            assistant_anchor: "про temporal lookup".to_string(),
            summary_headline: "про temporal lookup".to_string(),
            summary_next_step: String::new(),
        }),
        messages: vec![
            codex_threads::TranscriptMessage {
                role: "user".to_string(),
                text: "о чём говорили?".to_string(),
            },
            codex_threads::TranscriptMessage {
                role: "assistant".to_string(),
                text: "про temporal lookup".to_string(),
            },
        ],
    };
    let exact_time_answer = render_direct_answer(
        &handoff,
        None,
        Some(&exact_time_chat),
        "chat_at_time",
        Some(exact_time),
        1,
    );
    let exact_time_recovered = exact_time_answer
        .contains("Что было в чате на этот момент: про temporal lookup")
        && exact_time_answer.contains(
            "Смысловой срез времени: 2026-03-19T11:59:20+03:00 -> 2026-03-19T12:01:10+03:00",
        )
        && exact_time_answer.contains("Подходящий chat thread: чат про continuity");

    let missing_previous_answer =
        render_direct_answer(&handoff, None, None, "previous_chat", None, 30);
    let missing_previous_fail_closed = missing_previous_answer
        .contains("На чём закончился прошлый чат: для такого смещения назад нет известного чата.")
        && missing_previous_answer.contains("Смещение назад по чатам: 30")
        && missing_previous_answer
            .contains("Текущая активная линия проекта сейчас: Current project line");

    let missing_exact_time = "2099-01-01T12:00:00Z";
    let missing_exact_time_answer = render_direct_answer(
        &handoff,
        None,
        None,
        "chat_at_time",
        Some(missing_exact_time),
        1,
    );
    let missing_exact_time_fail_closed = missing_exact_time_answer.contains(
        "Что было в чате на этот момент: для этого момента нет точного совпадения в известных чатах.",
    ) && missing_exact_time_answer.contains("Целевой момент времени: 2099-01-01T12:00:00Z")
        && missing_exact_time_answer.contains("Текущая активная линия проекта сейчас: Current project line");

    Ok(vec![
        build_continuity_eval_probe(
            "previous_chat_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": previous_chat_recovered,
                "unexpected_present": false,
                "intent": "previous_chat",
                "answer": previous_answer,
            }),
        )?,
        build_continuity_eval_probe(
            "exact_time_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": exact_time_recovered,
                "unexpected_present": false,
                "intent": "chat_at_time",
                "target_time": exact_time,
                "answer": exact_time_answer,
            }),
        )?,
        build_continuity_eval_probe(
            "missing_previous_chat_fail_closed",
            "hit_correct_target",
            EvalPattern::IsolationBoundary,
            false,
            json!({
                "boundary_clean": missing_previous_fail_closed,
                "fail_closed_ok": missing_previous_fail_closed,
                "unexpected_present": false,
                "intent": "previous_chat",
                "offset": 30,
                "answer": missing_previous_answer,
            }),
        )?,
        build_continuity_eval_probe(
            "missing_exact_time_fail_closed",
            "hit_correct_target",
            EvalPattern::IsolationBoundary,
            false,
            json!({
                "boundary_clean": missing_exact_time_fail_closed,
                "fail_closed_ok": missing_exact_time_fail_closed,
                "unexpected_present": false,
                "intent": "chat_at_time",
                "target_time": missing_exact_time,
                "answer": missing_exact_time_answer,
            }),
        )?,
    ])
}

fn select_chat_tail_label(
    chat_tail: &codex_threads::ChatTail,
    answer_headline: &str,
) -> Option<String> {
    let _thread_id = chat_tail.thread_id.trim();
    let normalized_answer = collapse_answer_text(answer_headline, 220);
    let normalized_answer = normalized_answer.trim();
    let selected_user_anchor = chat_tail
        .selected_time_slice
        .as_ref()
        .map(|slice| collapse_answer_text(&slice.user_anchor, 220));
    for candidate in [
        chat_tail.title.trim(),
        chat_tail.summary_headline.as_deref().unwrap_or("").trim(),
    ] {
        if candidate.is_empty() {
            continue;
        }
        let normalized = collapse_answer_text(candidate, 220);
        if normalized.trim().is_empty()
            || normalized.trim() == normalized_answer
            || selected_user_anchor
                .as_deref()
                .is_some_and(|anchor| anchor.trim() == normalized.trim())
            || (chat_tail.selected_time_slice.is_some() && normalized.chars().count() > 140)
            || looks_like_noisy_chat_label(&normalized)
        {
            continue;
        }
        return Some(normalized);
    }
    None
}

fn looks_like_noisy_chat_label(label: &str) -> bool {
    let normalized = label.trim().to_lowercase();
    normalized.starts_with("agents.md прочитан")
        || normalized.contains("agents.md прочитан")
        || normalized.starts_with("продолжай строго")
        || normalized.contains("продолжай строго")
        || normalized.starts_with("# context from my ide setup")
        || normalized.contains("## active file:")
        || normalized.contains("## open tabs:")
        || normalized.contains("перед любой содержательной работой")
        || normalized.contains("<instructions>")
        || normalized.ends_with('?')
        || normalized.chars().count() < 4
}

fn parse_chat_reference_spec(value: &str) -> (&str, usize) {
    let trimmed = value.trim();
    if let Some(offset) = trimmed.strip_prefix("previous:") {
        let parsed = offset.parse::<usize>().ok().filter(|value| *value > 0);
        return ("previous", parsed.unwrap_or(1));
    }
    (trimmed, 1)
}

fn resolve_answer_intent(
    requested_intent: &str,
    parsed_intent: Option<&str>,
    chat_reference_kind: Option<&str>,
    has_at_time: bool,
) -> String {
    let mut intent = if requested_intent != "last_chat" {
        requested_intent.to_string()
    } else {
        parsed_intent.unwrap_or(requested_intent).to_string()
    };
    if intent == "last_chat" && chat_reference_kind == Some("previous") {
        intent = "previous_chat".to_string();
    }
    if intent == "last_chat" && has_at_time {
        intent = "chat_at_time".to_string();
    }
    intent
}

async fn load_startup_context(
    db: &Client,
    args: &ContinuityStartupArgs,
) -> Result<ContinuityStartupContext> {
    let project = resolve_project(db, args).await?;
    let namespace = postgres::find_namespace_by_code(db, project.project_id, &args.namespace)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.namespace))?;
    let snapshots =
        postgres::list_observability_snapshots_by_kinds(db, &["continuity_import"], Some(50))
            .await?;
    let latest = latest_scoped_snapshot(
        &snapshots,
        "continuity_import",
        &project.code,
        &namespace.code,
        |_| true,
    )
    .ok_or_else(|| {
        anyhow!(
            "no continuity import found for {}::{}",
            project.code,
            namespace.code
        )
    })?;
    let continuity = latest.payload["continuity_import"].clone();
    let handoff_summary = latest_handoff_summary(db, &project, &namespace)
        .await?
        .unwrap_or_else(|| continuity["active_workline_summary"]["details"].clone());
    let restore = working_state::build_restore_bundle(db, &project, &namespace).await?;
    Ok(ContinuityStartupContext {
        project,
        namespace,
        continuity,
        handoff_summary,
        restore,
    })
}

fn build_chat_start_restore(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    continuity: &Value,
    handoff_summary: &Value,
    restore: Option<&Value>,
) -> Value {
    let restore_node = restore.map(|value| &value["working_state_restore"]);
    let headline = handoff_summary["headline"]
        .as_str()
        .unwrap_or("ещё нет данных")
        .to_string();
    let next_step = handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let current_goal = restore_node
        .and_then(|value| value["current_goal"].as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(headline.as_str())
        .to_string();
    let restore_confidence = restore_node
        .and_then(|value| value["restore_confidence"].as_str())
        .unwrap_or("preliminary")
        .to_string();
    let materialized_summary =
        restore_node.and_then(summarize_startup_materialized_notes);
    let recent_actions_summary =
        restore_node.and_then(|value| summarize_recent_actions(&value["recent_actions"]));
    let active_files_summary =
        restore_node.and_then(|value| summarize_string_list(&value["active_files"], 4));
    let open_questions_summary =
        restore_node.and_then(|value| summarize_string_list(&value["open_questions"], 3));
    let workspace_graph_summary =
        restore_node.and_then(|value| workspace_graph::human_summary(&value["workspace_graph"]));
    let pending_return_summary = restore_node
        .and_then(|value| value["pending_return_summary"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let execctl_resume_contract_summary = restore_node
        .and_then(|value| value["execctl_resume_contract_summary"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let execctl_resume_obligation = restore_node
        .map(|value| summarize_execctl_resume_obligation(&value["execctl_resume_contract"]))
        .unwrap_or_else(|| default_execctl_resume_obligation(None, "clear"));
    let startup_next_action = restore_node
        .filter(|value| value["startup_next_action"].is_object())
        .map(|value| value["startup_next_action"].clone())
        .unwrap_or_else(|| {
            default_startup_next_action(
                &current_goal,
                &next_step,
                project,
                namespace,
                &execctl_resume_obligation,
                restore_node.and_then(|value| value.get("client_budget_guard")),
            )
        });
    let startup_next_action_summary = restore_node
        .and_then(|value| value["startup_next_action_summary"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| summarize_startup_next_action(&startup_next_action));
    let execctl_active_lease = restore_node
        .filter(|value| value["execctl_active_lease"].is_object())
        .map(|value| value["execctl_active_lease"].clone());
    let execctl_active_lease_summary = restore_node
        .and_then(|value| value["execctl_active_lease_summary"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let project_task_tree_summary = restore_node
        .and_then(|value| value["project_task_tree_summary"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let project_task_tree = restore_node
        .filter(|value| value["project_task_tree"].is_object())
        .map(|value| value["project_task_tree"].clone());
    let project_task_ledger_summary = restore_node
        .and_then(|value| value["project_task_ledger_summary"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let project_task_ledger = restore_node
        .filter(|value| value["project_task_ledger"].is_object())
        .map(|value| value["project_task_ledger"].clone());
    let required_return_task = restore_node
        .filter(|value| value["execctl_resume_contract"]["required_return_task"].is_object())
        .map(|value| value["execctl_resume_contract"]["required_return_task"].clone())
        .or_else(|| {
            let headline = execctl_resume_obligation["required_return_headline"]
                .as_str()
                .filter(|value| !value.is_empty())?;
            Some(json!({
                "headline": headline,
                "next_step": execctl_resume_obligation["required_return_next_step"]
            }))
        });
    let execctl_resume_state = restore_node
        .and_then(|value| value["execctl_resume_state"].as_str())
        .unwrap_or("clear")
        .to_string();
    let included_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "included")
    });
    let excluded_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "not_included")
    });
    let active_files = restore_node
        .and_then(|value| value["active_files"].as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.is_empty())
                .take(4)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let open_questions = restore_node
        .and_then(|value| value["open_questions"].as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.is_empty())
                .take(3)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let thread_count = continuity["bootstrap_summary"]["details"]["thread_count"]
        .as_u64()
        .unwrap_or(0);
    let latest_transcript =
        continuity["bootstrap_summary"]["details"]["latest_rendered_transcript"]
            .as_str()
            .unwrap_or_default()
            .to_string();
    json!({
        "chat_start_restore": {
            "project": {
                "code": project.code,
                "display_name": project.display_name,
                "repo_root": project.repo_root,
            },
            "namespace": {
                "code": namespace.code,
                "display_name": namespace.display_name,
            },
            "headline": headline,
            "next_step": next_step,
            "current_goal": current_goal,
            "restore_confidence": restore_confidence,
            "thread_count": thread_count,
            "latest_rendered_transcript": latest_transcript,
            "materialized_summary": materialized_summary,
            "recent_actions_summary": recent_actions_summary,
            "active_files_summary": active_files_summary,
            "open_questions_summary": open_questions_summary,
            "workspace_graph_summary": workspace_graph_summary,
            "pending_return_summary": pending_return_summary,
            "execctl_resume_contract_summary": execctl_resume_contract_summary,
            "execctl_resume_obligation": execctl_resume_obligation,
            "startup_next_action": startup_next_action,
            "startup_next_action_summary": startup_next_action_summary,
            "execctl_active_lease": execctl_active_lease,
            "execctl_active_lease_summary": execctl_active_lease_summary,
            "required_return_task": required_return_task,
            "project_task_tree": project_task_tree,
            "project_task_tree_summary": project_task_tree_summary,
            "project_task_ledger": project_task_ledger,
            "project_task_ledger_summary": project_task_ledger_summary,
            "execctl_resume_state": execctl_resume_state,
            "included_reasons_summary": included_reasons_summary,
            "excluded_reasons_summary": excluded_reasons_summary,
            "active_files": active_files,
            "open_questions": open_questions,
            "prompt_text": render_chat_start_prompt(
                project,
                namespace,
                handoff_summary,
                restore_node,
                thread_count,
            ),
        }
    })
}

fn default_execctl_resume_obligation(
    required_return_task: Option<&Value>,
    resume_state: &str,
) -> Value {
    let required_headline = required_return_task
        .and_then(|task| task["headline"].as_str())
        .filter(|value| !value.is_empty());
    let required_next_step = required_return_task
        .and_then(|task| task["next_step"].as_str())
        .filter(|value| !value.is_empty());
    json!({
        "resume_state": resume_state,
        "no_silent_drop": true,
        "pending_return_count": if required_headline.is_some() { 1 } else { 0 },
        "active_task_headline": Value::Null,
        "required_return_headline": required_headline,
        "required_return_next_step": required_next_step,
    })
}

fn summarize_execctl_resume_obligation(contract: &Value) -> Value {
    if !contract.is_object() {
        return default_execctl_resume_obligation(None, "clear");
    }
    let active_task = &contract["active_task"];
    let required_return_task = &contract["required_return_task"];
    json!({
        "resume_state": contract["resume_state"].as_str().unwrap_or("clear"),
        "no_silent_drop": contract["no_silent_drop"].as_bool().unwrap_or(true),
        "pending_return_count": contract["pending_return_count"].as_u64().unwrap_or_default(),
        "active_task_headline": active_task["headline"].as_str().filter(|value| !value.is_empty()),
        "required_return_headline": required_return_task["headline"].as_str().filter(|value| !value.is_empty()),
        "required_return_next_step": required_return_task["next_step"].as_str().filter(|value| !value.is_empty()),
    })
}

fn default_startup_next_action(
    current_goal: &str,
    next_step: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    execctl_resume_obligation: &Value,
    client_budget_guard: Option<&Value>,
) -> Value {
    let resume_state = execctl_resume_obligation["resume_state"]
        .as_str()
        .unwrap_or("clear");
    let no_silent_drop = execctl_resume_obligation["no_silent_drop"]
        .as_bool()
        .unwrap_or(true);
    let active_headline = execctl_resume_obligation["active_task_headline"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(current_goal);
    let required_headline = execctl_resume_obligation["required_return_headline"]
        .as_str()
        .filter(|value| !value.is_empty());
    let required_next_step = execctl_resume_obligation["required_return_next_step"]
        .as_str()
        .filter(|value| !value.is_empty());
    let should_rotate_chat = client_budget_guard
        .map(|guard| {
            guard["should_rotate_chat_now"].as_bool() == Some(true)
                || guard["should_rotate_chat_soon"].as_bool() == Some(true)
        })
        .unwrap_or(false);
    let client_budget_status = client_budget_guard
        .and_then(|guard| guard["status_label"].as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("новый чат рекомендован");
    let client_budget_note = client_budget_guard
        .and_then(|guard| guard["note"].as_str())
        .filter(|value| !value.is_empty());
    if should_rotate_chat {
        let preserves_return_obligation = resume_state != "clear";
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "rotate_chat_for_client_budget",
            "blocking": true,
            "reason": "client_budget_guard_pressure",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": format!("Клиентский лимит: {client_budget_status}"),
            "next_step": "сохрани handoff и продолжай только в свежем чате через continuity startup",
            "client_budget_status_label": client_budget_status,
            "client_budget_note": client_budget_note,
            "preserves_return_obligation": preserves_return_obligation,
            "action_bundle": working_state::build_rotate_chat_action_bundle(
                Some(project.code.as_str()),
                Some(namespace.code.as_str()),
                Some(project.repo_root.as_str()),
                preserves_return_obligation,
                Some(current_goal),
                Some(next_step),
            ),
        })
    } else if resume_state != "clear" && required_headline.is_some() {
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "resume_required_return_task",
            "blocking": true,
            "reason": "execctl_return_required",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": required_headline,
            "next_step": required_next_step,
        })
    } else {
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "continue_active_workline",
            "blocking": false,
            "reason": "active_workline_restored",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": active_headline,
            "next_step": next_step,
        })
    }
}

fn summarize_startup_next_action(value: &Value) -> Option<String> {
    let action_kind = value["action_kind"]
        .as_str()
        .filter(|item| !item.is_empty())?;
    let headline = value["headline"]
        .as_str()
        .filter(|item| !item.is_empty())
        .unwrap_or("ещё нет данных");
    let next_step = value["next_step"]
        .as_str()
        .filter(|item| !item.is_empty())
        .unwrap_or("ещё нет данных");
    Some(format!("{action_kind}: {headline} -> {next_step}"))
}

fn compact_prompt_fragment(value: &str, max_chars: usize) -> String {
    collapse_answer_text(value, max_chars)
}

fn summarize_startup_next_action_for_prompt(value: &Value) -> Option<String> {
    let action_kind = value["action_kind"]
        .as_str()
        .filter(|item| !item.is_empty())?;
    let headline = value["headline"]
        .as_str()
        .filter(|item| !item.is_empty())
        .unwrap_or("ещё нет данных");
    let compact_headline = compact_prompt_fragment(headline, 64);
    match action_kind {
        "resume_required_return_task" => {
            Some(format!("Сначала: вернись к линии: {compact_headline}"))
        }
        "continue_active_workline" => None,
        _ => Some(format!("Сначала: {action_kind} -> {compact_headline}")),
    }
}

fn render_chat_start_prompt(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    handoff_summary: &Value,
    restore_node: Option<&Value>,
    _thread_count: u64,
) -> String {
    let headline = handoff_summary["headline"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let compact_headline = compact_prompt_fragment(headline, 80);
    let next_step = handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let compact_next_step = compact_prompt_fragment(&next_step, 120);
    let current_goal = restore_node
        .and_then(|value| value["current_goal"].as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(headline);
    let compact_current_goal = compact_prompt_fragment(current_goal, 80);
    let materialized_summary =
        restore_node.and_then(summarize_startup_materialized_notes);
    let compact_materialized_summary = materialized_summary
        .as_deref()
        .map(|value| compact_prompt_fragment(value, 88));
    let execctl_resume_obligation = restore_node
        .map(|value| summarize_execctl_resume_obligation(&value["execctl_resume_contract"]))
        .unwrap_or_else(|| default_execctl_resume_obligation(None, "clear"));
    let startup_next_action = restore_node
        .filter(|value| value["startup_next_action"].is_object())
        .map(|value| value["startup_next_action"].clone())
        .unwrap_or_else(|| {
            default_startup_next_action(
                current_goal,
                &next_step,
                project,
                namespace,
                &execctl_resume_obligation,
                restore_node.and_then(|value| value.get("client_budget_guard")),
            )
        });
    let blocked_reply_text = restore_node
        .and_then(|value| value["client_budget_guard"]["reply_execution_gate"]["blocking_reply_contract"]["template"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    let execctl_resume_state = restore_node
        .and_then(|value| value["execctl_resume_state"].as_str())
        .unwrap_or("clear");
    let restore_confidence = restore_node
        .and_then(|value| value["restore_confidence"].as_str())
        .unwrap_or("preliminary");
    let mut lines = vec![
        "CHAT_START_RESTORE".to_string(),
        format!("Project: {} ({})", project.display_name, project.code),
        format!("Namespace: {}", namespace.code),
        format!("Линия: {compact_headline}"),
        format!("Шаг: {compact_next_step}"),
    ];
    if compact_current_goal != compact_headline {
        lines.push(format!("Цель: {compact_current_goal}"));
    }
    if let Some(value) = compact_materialized_summary {
        lines.push(format!("Сделано: {value}"));
    }
    if let Some(value) = summarize_startup_next_action_for_prompt(&startup_next_action) {
        lines.push(value);
    }
    if startup_next_action["action_kind"].as_str() == Some("rotate_chat_for_client_budget") {
        lines.push("В старом чате разрешён только короткий rotate-ответ.".to_string());
        lines.push(format!(
            "Разрешённый ответ: {}",
            blocked_reply_text
                .as_deref()
                .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE)
        ));
    }
    if execctl_resume_state == "pending_return_queue_present" {
        lines.push("Не переключайся до возврата.".to_string());
    }
    if restore_confidence == "preliminary" {
        lines.push("Recovery: preliminary.".to_string());
    }
    lines.push("Правило: follow pack; continuity не поднимай.".to_string());
    lines.join("\n")
}

fn print_chat_start_restore_human(value: &Value) {
    let node = &value["chat_start_restore"];
    println!("Chat-start restore pack:");
    println!(
        "- Этот блок уже нужно считать восстановленным рабочим контекстом для первого содержательного ответа."
    );
    println!(
        "- Продолжаем с линии: {}",
        node["headline"].as_str().unwrap_or("ещё нет данных")
    );
    println!(
        "- Обязательный следующий шаг: {}",
        node["next_step"].as_str().unwrap_or("ещё нет данных")
    );
    if let Some(value) = node["materialized_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Что уже materialized: {value}");
    }
    if let Some(value) = node["recent_actions_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Недавние действия: {value}");
    }
    if let Some(value) = node["active_files_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Активные файлы: {value}");
    }
    if let Some(value) = node["workspace_graph_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Структурный граф рабочей области: {value}");
    }
    if let Some(value) = node["pending_return_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Незавершённые линии к возврату: {value}");
    }
    if let Some(value) = node["execctl_resume_contract_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Контракт возврата ExecCtl: {value}");
    }
    if let Some(value) = node["startup_next_action_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Первое обязательное действие после startup: {value}");
    }
    if let Some(value) = node["execctl_active_lease_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Активный lease ExecCtl: {value}");
    }
    if let Some(value) = node["included_reasons_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Почему вошёл последний контекст: {value}");
    }
    if let Some(value) = node["excluded_reasons_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Почему часть не вошла: {value}");
    }
    if let Some(value) = node["open_questions_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Открытые вопросы: {value}");
    }
    println!(
        "- Thread count в temporal index: {}",
        node["thread_count"].as_u64().unwrap_or(0)
    );
    if let Some(prompt_text) = node["prompt_text"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Prompt-text restore уже готов для первого содержательного ответа:");
        for line in prompt_text.lines() {
            println!("  {line}");
        }
    }
}

fn summarize_chat_tail_headline(chat_tail: &codex_threads::ChatTail) -> Option<String> {
    if let Some(value) = chat_tail
        .selected_time_slice
        .as_ref()
        .map(|slice| slice.summary_headline.as_str())
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    if let Some(value) = chat_tail
        .summary_headline
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    let assistant = chat_tail
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")?;
    Some(collapse_answer_text(&assistant.text, 220))
}

fn extract_chat_tail_next_step(chat_tail: &codex_threads::ChatTail) -> Option<String> {
    if let Some(value) = chat_tail
        .selected_time_slice
        .as_ref()
        .map(|slice| slice.summary_next_step.as_str())
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    if let Some(value) = chat_tail
        .summary_next_step
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    let assistant = chat_tail
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")?;
    extract_next_step_from_text(&assistant.text)
}

fn collapse_answer_text(text: &str, max_chars: usize) -> String {
    let stripped = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != "AGENTS.md прочитан" && trimmed != "AGENTS.md не прочитан"
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    for prefix in ["AGENTS.md прочитан", "AGENTS.md не прочитан"] {
        if let Some(value) = collapsed.strip_prefix(prefix) {
            collapsed = value
                .trim_start_matches(|ch: char| {
                    ch == '.' || ch == ':' || ch == '-' || ch.is_whitespace()
                })
                .trim()
                .to_string();
            break;
        }
    }
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    collapsed.chars().take(max_chars).collect::<String>() + "..."
}

fn normalize_next_step_value(value: &str) -> Option<String> {
    let mut normalized = value.trim().to_string();
    for _ in 0..3 {
        let mut stripped = false;
        for label in [
            "Ближайший обязательный следующий шаг:",
            "Ближайший обязательный следующий шаг был такой:",
            "Следующий обязательный следующий шаг:",
            "Следующий обязательный шаг:",
            "Nearest mandatory next step:",
        ] {
            if let Some(rest) = normalized.strip_prefix(label) {
                normalized = rest
                    .trim_start_matches(|ch: char| ch == ':' || ch == '-' || ch.is_whitespace())
                    .trim()
                    .to_string();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    let normalized = normalized
        .trim_end_matches(['`', '"', '\'', '«', '»', '|'])
        .trim()
        .to_string();
    (!normalized.is_empty()).then_some(normalized)
}

fn extract_next_step_from_text(text: &str) -> Option<String> {
    for label in [
        "Ближайший обязательный следующий шаг:",
        "Ближайший обязательный следующий шаг был такой:",
        "Следующий обязательный следующий шаг:",
        "Следующий обязательный шаг:",
        "Nearest mandatory next step:",
    ] {
        if let Some((_, value)) = text.split_once(label)
            && let Some(next_step) = normalize_next_step_value(value.lines().next().unwrap_or(""))
        {
            return Some(next_step);
        }
    }
    None
}

fn startup_materialized_notes(restore_node: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(guard) = restore_node.get("client_budget_guard") {
        if let Some(status_label) = guard["status_label"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            notes.push(format!("Client-budget guard: {status_label}."));
        }
        if let Some(last_request) = guard["last_request"].as_str().filter(|value| !value.is_empty())
        {
            notes.push(format!("Последний запрос в модель: {last_request}."));
        }
        if let Some(client_limits) = guard["client_limits"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            notes.push(format!("Лимит клиента сейчас: {client_limits}."));
        }
    }
    if let Some(items) = restore_node["materialized_notes"].as_array() {
        for item in items.iter().filter_map(Value::as_str) {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("Client-budget guard:")
                || trimmed.starts_with("Последний запрос в модель:")
                || trimmed.starts_with("Лимит клиента сейчас:")
            {
                continue;
            }
            notes.push(trimmed.to_string());
        }
    }
    notes
}

fn summarize_startup_materialized_notes(restore_node: &Value) -> Option<String> {
    let notes = startup_materialized_notes(restore_node);
    if notes.is_empty() {
        None
    } else {
        Some(notes.into_iter().take(2).collect::<Vec<_>>().join("; "))
    }
}

fn summarize_recent_actions(value: &Value) -> Option<String> {
    let items = value.as_array()?;
    let mut entries = Vec::new();
    for item in items.iter().take(2) {
        if let Some(text) = item["headline"].as_str().filter(|value| !value.is_empty()) {
            entries.push(text.to_string());
        } else if let Some(text) = item["summary"].as_str().filter(|value| !value.is_empty()) {
            entries.push(text.to_string());
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries.join("; "))
    }
}

fn summarize_string_list(value: &Value, limit: usize) -> Option<String> {
    let items = value.as_array()?;
    let values = items
        .iter()
        .filter_map(|item| item.as_str())
        .filter(|item| !item.is_empty())
        .take(limit)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.join("; "))
    }
}

fn collect_sources(cfg: &AppConfig, args: &ContinuityImportArgs) -> Result<Vec<ContinuitySource>> {
    let mut sources = Vec::new();
    let bootstrap_path = canonical_path(&args.bootstrap_file)?;
    sources.push(ContinuitySource {
        original_path: bootstrap_path.clone(),
        relative_path: ".amai-continuity/bootstrap/continuity-snapshot.md".to_string(),
        source_kind: "continuity_bootstrap".to_string(),
        artifact_bucket: cfg.s3_bucket_artifacts.clone(),
        artifact_kind: "continuity_bootstrap".to_string(),
    });
    if let Some(active_workline_file) = &args.active_workline_file {
        let active_workline_path = canonical_path(active_workline_file)?;
        sources.push(ContinuitySource {
            original_path: active_workline_path,
            relative_path: ".amai-continuity/active-workline/ACTIVE_WORKLINE.md".to_string(),
            source_kind: "continuity_active_workline".to_string(),
            artifact_bucket: cfg.s3_bucket_artifacts.clone(),
            artifact_kind: "continuity_active_workline".to_string(),
        });
    }

    if let Some(memory_dir) = &args.memory_dir {
        let memory_dir = canonical_path(memory_dir)?;
        let mut entries = fs::read_dir(&memory_dir)
            .with_context(|| format!("failed to read {}", memory_dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| anyhow!("invalid session memory file name: {}", path.display()))?
                .to_string();
            sources.push(ContinuitySource {
                original_path: path,
                relative_path: format!(".amai-continuity/external-memory-bridge/{file_name}"),
                source_kind: "continuity_session_memory".to_string(),
                artifact_bucket: cfg.s3_bucket_artifacts.clone(),
                artifact_kind: "continuity_session_memory".to_string(),
            });
        }
    }

    let bootstrap_text = fs::read_to_string(&bootstrap_path)
        .with_context(|| format!("failed to read {}", bootstrap_path.display()))?;
    let mut transcript_paths = parse_rendered_transcripts(&bootstrap_text);
    if let Some(limit) = args.transcript_limit
        && transcript_paths.len() > limit
    {
        transcript_paths = transcript_paths
            .into_iter()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    for path in transcript_paths {
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("invalid transcript file name: {}", path.display()))?
            .to_string();
        sources.push(ContinuitySource {
            original_path: path,
            relative_path: format!(".amai-continuity/rendered-transcripts/{file_name}"),
            source_kind: "continuity_rendered_transcript".to_string(),
            artifact_bucket: cfg.s3_bucket_transcripts.clone(),
            artifact_kind: "continuity_rendered_transcript".to_string(),
        });
    }

    let mut dedup = BTreeSet::new();
    sources.retain(|source| dedup.insert(source.original_path.clone()));
    Ok(sources)
}

fn build_document_record(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    source: &ContinuitySource,
    content: &str,
    searchable_content: &str,
    metadata: Value,
) -> Result<DocumentRecord> {
    let line_count = content.lines().count() as i32;
    Ok(DocumentRecord {
        project_id: project.project_id,
        namespace_id: namespace.namespace_id,
        repo_root: project.repo_root.clone(),
        absolute_path: source.original_path.display().to_string(),
        relative_path: source.relative_path.clone(),
        language: Some("markdown".to_string()),
        source_kind: source.source_kind.clone(),
        git_commit_sha: None,
        file_sha256: hex_sha256(content.as_bytes()),
        line_count,
        byte_count: content.len() as i64,
        content: searchable_content.to_string(),
        metrics: json!({
            "line_count": line_count,
            "byte_count": content.len(),
            "searchable_byte_count": searchable_content.len(),
        }),
        structure: json!([]),
        imports: json!([]),
        exports: json!([]),
        diagnostics: json!([]),
        metadata,
    })
}

async fn latest_handoff_summary(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<Value>> {
    let snapshots =
        postgres::list_observability_snapshots_by_kinds(db, &["continuity_handoff"], Some(50))
            .await?;
    Ok(latest_scoped_snapshot(
        &snapshots,
        "continuity_handoff",
        &project.code,
        &namespace.code,
        |root| {
            !is_meta_continuity_handoff(
                root["headline"].as_str().unwrap_or_default(),
                root["next_step"].as_str().unwrap_or_default(),
                root["details"].as_str().unwrap_or_default(),
            )
        },
    )
    .map(|snapshot| {
        json!({
            "headline": snapshot.payload["continuity_handoff"]["headline"]
                .as_str()
                .unwrap_or("ещё нет данных"),
            "next_step": snapshot.payload["continuity_handoff"]["next_step"]
                .as_str()
                .unwrap_or("ещё нет данных"),
            "local_path": snapshot.payload["continuity_handoff"]["local_path"]
                .as_str()
                .unwrap_or_default(),
        })
    }))
}

fn latest_scoped_snapshot<'a, F>(
    snapshots: &'a [postgres::ObservabilitySnapshotRecord],
    root_key: &str,
    project_code: &str,
    namespace_code: &str,
    extra_filter: F,
) -> Option<&'a postgres::ObservabilitySnapshotRecord>
where
    F: Fn(&Value) -> bool,
{
    snapshots
        .iter()
        .filter_map(|snapshot| {
            let root = snapshot.payload.get(root_key)?;
            if root["project"]["code"].as_str() != Some(project_code)
                || root["namespace"]["code"].as_str() != Some(namespace_code)
                || !extra_filter(root)
            {
                return None;
            }
            Some(snapshot)
        })
        .max_by_key(|snapshot| {
            (
                continuity_snapshot_semantic_epoch_ms(snapshot, root_key),
                snapshot.created_at_epoch_ms,
            )
        })
}

fn continuity_snapshot_semantic_epoch_ms(
    snapshot: &postgres::ObservabilitySnapshotRecord,
    root_key: &str,
) -> i64 {
    let root = snapshot.payload.get(root_key).unwrap_or(&Value::Null);
    root["captured_at_epoch_ms"]
        .as_i64()
        .or_else(|| root["imported_at_epoch_ms"].as_i64())
        .or_else(|| {
            root["created_at_epoch_s"]
                .as_i64()
                .map(|value| value * 1000)
        })
        .or_else(|| snapshot.payload["_observability"]["captured_at_epoch_ms"].as_i64())
        .unwrap_or(snapshot.created_at_epoch_ms)
}

fn is_meta_continuity_handoff(headline: &str, next_step: &str, details: &str) -> bool {
    let headline_lc = headline.to_lowercase();
    let next_step_lc = next_step.to_lowercase();
    let details_lc = details.to_lowercase();
    headline_lc.contains("continuity restored")
        || headline_lc.contains("continuity reported")
        || headline_lc.contains("restored and reported for new chat")
        || headline_lc.contains("reported for new chat")
        || next_step_lc.contains("ждать указание пользователя")
        || details_lc.contains("пользователь спросил, на чем остановились")
        || details_lc.contains("пользователь спросил, на чём остановились")
        || details_lc.contains("ответить именно по последней зафиксированной точке")
}

fn render_handoff_markdown(headline: &str, next_step: &str, details: &str) -> String {
    let mut lines = vec![
        "# Amai Continuity Handoff".to_string(),
        String::new(),
        format!("- headline: {headline}"),
        format!("- next_step: {next_step}"),
    ];
    if !details.trim().is_empty() {
        lines.push(String::new());
        lines.push("## Details".to_string());
        lines.push(String::new());
        lines.push(details.trim().to_string());
    }
    lines.join("\n") + "\n"
}

fn build_chunks(cfg: &AppConfig, content: &str) -> Vec<ChunkRecord> {
    let mut chunks = Vec::new();
    let mut start_line = 1_i32;
    let mut current_line = 1_i32;
    let mut start_byte = 0_i32;
    let mut current_byte = 0_i32;
    let mut buffer = String::new();

    for line in content.lines() {
        let rendered = format!("{line}\n");
        let rendered_len = rendered.len() as i32;
        if !buffer.is_empty() && buffer.len() + rendered.len() > cfg.chunk_max_bytes {
            chunks.push(chunk_record(
                start_line,
                current_line - 1,
                start_byte,
                current_byte,
                std::mem::take(&mut buffer),
            ));
            start_line = current_line;
            start_byte = current_byte;
        }
        buffer.push_str(&rendered);
        current_line += 1;
        current_byte += rendered_len;
    }

    if !buffer.is_empty() {
        chunks.push(chunk_record(
            start_line,
            current_line - 1,
            start_byte,
            current_byte,
            buffer,
        ));
    }

    let total_chunks = chunks.len() as i32;
    for (index, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_index = index as i32;
        chunk.total_chunks = total_chunks;
    }
    chunks
}

fn chunk_record(
    start_line: i32,
    end_line: i32,
    start_byte: i32,
    end_byte: i32,
    content: String,
) -> ChunkRecord {
    ChunkRecord {
        chunk_id: Uuid::new_v4(),
        qdrant_point_id: None,
        qdrant_collection_alias: None,
        chunk_index: 0,
        total_chunks: 0,
        start_line,
        end_line,
        start_byte,
        end_byte,
        content,
        metadata: json!({ "continuity_chunk": true }),
    }
}

async fn resolve_project(db: &Client, args: &ContinuityStartupArgs) -> Result<ProjectRecord> {
    if let Some(project) = &args.project {
        let mut record = postgres::get_project_by_code(db, project).await?;
        if let Some(repo_root) = args.repo_root.as_ref() {
            let repo_root = canonical_string(repo_root)?;
            if repo_root != record.repo_root {
                let is_bound = postgres::project_has_repo_root(db, record.project_id, &repo_root)
                    .await
                    .with_context(|| {
                        format!(
                            "failed to verify whether repo_root {} belongs to project {}",
                            repo_root, project
                        )
                    })?;
                if !is_bound {
                    return Err(anyhow!(
                        "repo_root {} is not bound to project {}; run `amai project register --code {} --display-name '{}' --repo-root {}` first or pass the already bound project root",
                        repo_root,
                        project,
                        project,
                        record.display_name,
                        shell_quote(&repo_root)
                    ));
                }
                record.repo_root = repo_root;
            }
        }
        return Ok(record);
    }
    let repo_root = args
        .repo_root
        .as_ref()
        .ok_or_else(|| anyhow!("continuity startup requires --project or --repo-root"))?;
    let repo_root = canonical_string(repo_root)?;
    postgres::get_project_by_repo_root(db, &repo_root).await
}

fn summarize_active_workline(text: &str) -> Value {
    let headline = extract_first_bullet_after(text, "## Текущая активная линия")
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let next_step = extract_last_block_after_labels(
        text,
        &[
            "- nearest mandatory next step:",
            "- ближайший обязательный следующий шаг:",
            "- ближайший обязательный следующий шаг без отклонений:",
            "- ближайший обязательный следующий шаг без угадывания:",
        ],
    )
    .unwrap_or_else(|| "ещё нет данных".to_string());
    json!({
        "headline": headline,
        "next_step": next_step,
    })
}

fn summarize_bootstrap(text: &str) -> Value {
    let transcripts = parse_rendered_transcripts(text);
    let thread_count = text
        .lines()
        .find_map(|line| line.strip_prefix("- `thread_count`: `"))
        .and_then(|rest| rest.strip_suffix('`'))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(transcripts.len() as u64);
    json!({
        "thread_count": thread_count,
        "latest_rendered_transcript": transcripts
            .last()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "ещё нет данных".to_string()),
    })
}

fn parse_rendered_transcripts(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in text.lines() {
        if !line.trim_start().starts_with("- `rendered_transcript`:") {
            continue;
        }
        let start = line.rfind('`');
        let prefix = "- `rendered_transcript`: `";
        if let Some(rest) = line.trim_start().strip_prefix(prefix)
            && let Some(value) = rest.strip_suffix('`')
        {
            paths.push(PathBuf::from(value));
        } else if let Some(end) = start {
            let start_index = prefix.len();
            if line.len() > start_index && end > start_index {
                paths.push(PathBuf::from(&line.trim_start()[start_index..end]));
            }
        }
    }
    paths
}

fn extract_first_bullet_after(text: &str, heading: &str) -> Option<String> {
    let section = text.split(heading).nth(1)?;
    section
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("- ").map(str::trim))
        .map(ToOwned::to_owned)
}

fn extract_last_block_after_labels(text: &str, labels: &[&str]) -> Option<String> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut matches = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if labels.contains(&trimmed) {
            matches.push(index);
        }
    }
    let start_index = *matches.last()?;
    let mut collected = Vec::new();
    for line in lines.into_iter().skip(start_index + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() && !collected.is_empty() {
            break;
        }
        if trimmed.starts_with("Обновление continuity") && !collected.is_empty() {
            break;
        }
        if trimmed.starts_with("- ")
            || trimmed.starts_with("  - ")
            || trimmed.starts_with("    - ")
            || trimmed.starts_with("1.")
        {
            collected.push(trimmed.trim_start_matches('-').trim().to_string());
        } else if !collected.is_empty() {
            break;
        }
    }
    if collected.is_empty() {
        None
    } else {
        Some(collected.join(" | "))
    }
}

fn canonical_path(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))
}

fn resolve_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    Ok(cwd.join(path))
}

fn fake_continuity_handoff_snapshot(
    created_at_epoch_ms: i64,
    captured_at_epoch_ms: i64,
    headline: &str,
) -> postgres::ObservabilitySnapshotRecord {
    postgres::ObservabilitySnapshotRecord {
        snapshot_id: Uuid::new_v4(),
        snapshot_kind: "continuity_handoff".to_string(),
        created_at_epoch_ms,
        payload: json!({
            "_observability": {
                "captured_at_epoch_ms": captured_at_epoch_ms,
            },
            "continuity_handoff": {
                "project": {"code": "art"},
                "namespace": {"code": "continuity"},
                "captured_at_epoch_ms": captured_at_epoch_ms,
                "headline": headline,
                "next_step": "Next step",
                "details": "",
                "local_path": "/tmp/handoff.md"
            }
        }),
    }
}

fn fake_continuity_import_snapshot(
    created_at_epoch_ms: i64,
    imported_at_epoch_ms: i64,
    headline: &str,
) -> postgres::ObservabilitySnapshotRecord {
    postgres::ObservabilitySnapshotRecord {
        snapshot_id: Uuid::new_v4(),
        snapshot_kind: "continuity_import".to_string(),
        created_at_epoch_ms,
        payload: json!({
            "_observability": {
                "captured_at_epoch_ms": imported_at_epoch_ms,
            },
            "continuity_import": {
                "project": {"code": "art"},
                "namespace": {"code": "continuity"},
                "imported_at_epoch_ms": imported_at_epoch_ms,
                "active_workline_summary": {
                    "details": {
                        "headline": headline,
                        "next_step": "Next step"
                    }
                }
            }
        }),
    }
}

fn canonical_string(path: &Path) -> Result<String> {
    Ok(canonical_path(path)?.display().to_string())
}

fn now_epoch_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64)
}

fn truncate_utf8_by_bytes(content: &str, max_bytes: usize) -> (String, usize) {
    if content.len() <= max_bytes {
        return (content.to_string(), 0);
    }
    let mut cutoff = 0usize;
    for (index, _) in content.char_indices() {
        if index > max_bytes {
            break;
        }
        cutoff = index;
    }
    if cutoff == 0 {
        return (String::new(), content.len());
    }
    (
        content[..cutoff].to_string(),
        content.len().saturating_sub(cutoff),
    )
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn non_empty_path(value: &str) -> Option<&Path> {
    (!value.is_empty()).then(|| Path::new(value))
}

fn human_epoch_ms(value: Option<u64>) -> String {
    value
        .filter(|value| *value > 0)
        .map(|value| format!("epoch {}", value / 1000))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else if value
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || matches!(char, '/' | '.' | '_' | '-'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContinuityStartupContext, build_chat_start_restore, build_continuity_answer_payload,
        build_continuity_canonical_eval, build_continuity_restore_payload,
        build_continuity_startup_payload, build_startup_runtime_state_artifact,
        build_startup_runtime_state_cli_json, StartupRuntimeStateAudit,
        continuity_replay_guard_probes, continuity_snapshot_semantic_epoch_ms,
        continuity_temporal_lookup_probes, degradation_proof_scenarios, enrich_thread_index_file,
        extract_next_step_from_text, fake_continuity_handoff_snapshot,
        fake_continuity_import_snapshot, inspect_startup_runtime_state, is_meta_continuity_handoff,
        latest_scoped_snapshot, parse_chat_reference_spec, render_direct_answer,
        resolve_answer_intent, startup_runtime_state_artifact_path,
    };
    use crate::cli::ContinuityThreadIndexEnrichArgs;
    use crate::codex_threads::{ChatTail, ThreadTimeSliceSummary, TranscriptMessage};
    use crate::postgres::{NamespaceRecord, ProjectRecord};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn render_direct_answer_prefers_concise_restore_bundle() {
        let handoff = json!({
            "headline": "Amai startup restore pack enriched and committed",
            "next_step": "Сделать auto-injection restore pack прямо в chat-start prompt."
        });
        let restore = json!({
            "working_state_restore": {
                "materialized_notes": [
                    "startup теперь поднимает materialized решения из handoff details",
                    "показывает недавние действия, а не только headline и next step"
                ],
                "recent_actions": [
                    {"headline": "Проверили новый чат на реальном старте"},
                    {"headline": "Усилили startup restore pack"}
                ],
                "active_files": [
                    "/home/art/agent-memory-index/src/continuity.rs",
                    "/home/art/Art/AGENTS.md"
                ],
                "restore_confidence": "high"
            }
        });

        let answer = render_direct_answer(&handoff, Some(&restore), None, "last_chat", None, 1);

        assert!(
            answer
                .contains("На чём остановились: Amai startup restore pack enriched and committed")
        );
        assert!(answer.contains("Что уже materialized: startup теперь поднимает materialized решения из handoff details; показывает недавние действия, а не только headline и next step"));
        assert!(answer.contains("Последние действия: Проверили новый чат на реальном старте; Усилили startup restore pack"));
        assert!(answer.contains("Активные файлы: /home/art/agent-memory-index/src/continuity.rs; /home/art/Art/AGENTS.md"));
        assert!(answer.contains("Ближайший обязательный следующий шаг: Сделать auto-injection restore pack прямо в chat-start prompt."));
    }

    #[test]
    fn meta_continuity_handoff_is_detected() {
        assert!(is_meta_continuity_handoff(
            "Continuity restored and reported for new chat",
            "Ждать указание пользователя",
            "Пользователь спросил, на чём остановились в прошлом чате."
        ));
        assert!(!is_meta_continuity_handoff(
            "Amai startup restore pack enriched and committed",
            "Сделать auto-injection restore pack прямо в chat-start prompt.",
            "Materialized рабочий контур в continuity."
        ));
    }

    #[test]
    fn render_direct_answer_formats_time_addressable_chat_lookup() {
        let handoff = json!({
            "headline": "Temporal lookup materialized",
            "next_step": "Проверить lookup по точному времени на реальном новом чате."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-1".to_string(),
            title: "чат про continuity".to_string(),
            summary_headline: Some("про temporal lookup".to_string()),
            summary_next_step: None,
            selected_time_slice: Some(ThreadTimeSliceSummary {
                started_at: "2026-03-19T11:59:20+03:00".to_string(),
                ended_at: "2026-03-19T12:01:10+03:00".to_string(),
                started_at_epoch_s: 1,
                ended_at_epoch_s: 2,
                user_anchor: "разбирали temporal lookup и его точный смысловой ответ".to_string(),
                assistant_anchor: "про temporal lookup".to_string(),
                summary_headline: "про temporal lookup".to_string(),
                summary_next_step: String::new(),
            }),
            messages: vec![
                TranscriptMessage {
                    role: "user".to_string(),
                    text: "о чём говорили?".to_string(),
                },
                TranscriptMessage {
                    role: "assistant".to_string(),
                    text: "про temporal lookup".to_string(),
                },
            ],
        };

        let answer = render_direct_answer(
            &handoff,
            None,
            Some(&chat_tail),
            "chat_at_time",
            Some("2026-03-19T12:00:00+03:00"),
            1,
        );

        assert!(answer.contains("Что было в чате на этот момент: про temporal lookup"));
        assert!(answer.contains("Целевой момент времени: 2026-03-19T12:00:00+03:00"));
        assert!(answer.contains(
            "Смысловой срез времени: 2026-03-19T11:59:20+03:00 -> 2026-03-19T12:01:10+03:00"
        ));
        assert!(answer.contains(
            "О чём шёл разговор в этот момент: разбирали temporal lookup и его точный смысловой ответ"
        ));
        assert!(answer.contains("Подходящий chat thread: чат про continuity"));
        assert!(
            answer.contains("Текущая активная линия проекта сейчас: Temporal lookup materialized")
        );
        assert!(answer.contains("Ближайшие сообщения к этому моменту:"));
        assert!(answer.contains("- Ваше: о чём говорили?"));
        assert!(answer.contains("- Моё: про temporal lookup"));
    }

    #[test]
    fn render_direct_answer_uses_thread_next_step_when_available() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-2".to_string(),
            title: "чат про continuity".to_string(),
            summary_headline: Some("Закончили на temporal contour.".to_string()),
            summary_next_step: Some("Проверить новый чат ещё раз.".to_string()),
            selected_time_slice: None,
            messages: vec![
                TranscriptMessage {
                    role: "user".to_string(),
                    text: "на чем закончили?".to_string(),
                },
                TranscriptMessage {
                    role: "assistant".to_string(),
                    text: "Закончили на temporal contour.\nБлижайший обязательный следующий шаг: Проверить новый чат ещё раз.".to_string(),
                },
            ],
        };

        let answer =
            render_direct_answer(&handoff, None, Some(&chat_tail), "previous_chat", None, 1);

        assert!(answer.contains("На чём закончился прошлый чат: Закончили на temporal contour."));
        assert!(
            answer.contains("Ближайший обязательный следующий шаг: Проверить новый чат ещё раз.")
        );
        assert!(answer.contains("Текущая активная линия проекта сейчас: Current project line"));
    }

    #[test]
    fn render_direct_answer_reports_missing_exact_time_match_without_fake_chat() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });

        let answer = render_direct_answer(
            &handoff,
            None,
            None,
            "chat_at_time",
            Some("2099-01-01T12:00:00Z"),
            1,
        );

        assert!(answer.contains(
            "Что было в чате на этот момент: для этого момента нет точного совпадения в известных чатах."
        ));
        assert!(answer.contains("Целевой момент времени: 2099-01-01T12:00:00Z"));
        assert!(answer.contains("Текущая активная линия проекта сейчас: Current project line"));
        assert!(
            answer.contains("Ближайший обязательный следующий шаг: Current project next step.")
        );
    }

    #[test]
    fn render_direct_answer_reports_missing_previous_chat_match_without_fake_fallback() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });

        let answer = render_direct_answer(&handoff, None, None, "previous_chat", None, 30);

        assert!(answer.contains(
            "На чём закончился прошлый чат: для такого смещения назад нет известного чата."
        ));
        assert!(answer.contains("Смещение назад по чатам: 30"));
        assert!(answer.contains("Текущая активная линия проекта сейчас: Current project line"));
        assert!(
            answer.contains("Ближайший обязательный следующий шаг: Current project next step.")
        );
    }

    #[test]
    fn degradation_proof_scenarios_cover_temporal_gap_classes() {
        let scenarios = degradation_proof_scenarios().expect("degradation proof scenarios");
        assert_eq!(scenarios.len(), 2);
        assert!(
            scenarios
                .iter()
                .all(|scenario| scenario["status"].as_str() == Some("pass"))
        );
        assert!(scenarios.iter().any(|scenario| {
            scenario["class_key"].as_str() == Some("partial_thread_index")
                && scenario["details"]["answer"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("нет точного совпадения")
        }));
    }

    #[test]
    fn latest_handoff_selection_prefers_semantic_capture_time_over_replay_created_at() {
        let replayed_stale = fake_continuity_handoff_snapshot(3_000, 1_000, "Stale replay");
        let fresh = fake_continuity_handoff_snapshot(2_000, 2_000, "Fresh handoff");
        let snapshots = vec![replayed_stale, fresh];

        let selected = latest_scoped_snapshot(
            &snapshots,
            "continuity_handoff",
            "art",
            "continuity",
            |root| {
                !is_meta_continuity_handoff(
                    root["headline"].as_str().unwrap_or_default(),
                    root["next_step"].as_str().unwrap_or_default(),
                    root["details"].as_str().unwrap_or_default(),
                )
            },
        )
        .expect("selected handoff");

        assert_eq!(
            continuity_snapshot_semantic_epoch_ms(selected, "continuity_handoff"),
            2_000
        );
        assert_eq!(
            selected.payload["continuity_handoff"]["headline"],
            json!("Fresh handoff")
        );
    }

    #[test]
    fn latest_import_selection_prefers_semantic_import_time_over_replay_created_at() {
        let replayed_stale = fake_continuity_import_snapshot(3_000, 1_000, "Stale import");
        let fresh = fake_continuity_import_snapshot(2_000, 2_000, "Fresh import");
        let snapshots = vec![replayed_stale, fresh];

        let selected =
            latest_scoped_snapshot(&snapshots, "continuity_import", "art", "continuity", |_| {
                true
            })
            .expect("selected import");

        assert_eq!(
            continuity_snapshot_semantic_epoch_ms(selected, "continuity_import"),
            2_000
        );
        assert_eq!(
            selected.payload["continuity_import"]["active_workline_summary"]["details"]["headline"],
            json!("Fresh import")
        );
    }

    #[test]
    fn continuity_replay_guard_probes_are_recovered_useful() {
        let probes = continuity_replay_guard_probes().expect("probes");
        assert_eq!(probes.len(), 2);
        assert!(
            probes
                .iter()
                .all(|probe| probe.verdict_class == "recovered_useful")
        );
        let summary = build_continuity_canonical_eval(&probes).expect("summary");
        assert_eq!(
            summary["verdict_counts"]["recovered_useful"].as_u64(),
            Some(2)
        );
    }

    #[test]
    fn continuity_temporal_lookup_probes_cover_useful_and_fail_closed_paths() {
        let probes = continuity_temporal_lookup_probes().expect("probes");
        assert_eq!(probes.len(), 4);
        let summary = build_continuity_canonical_eval(&probes).expect("summary");
        assert_eq!(
            summary["verdict_counts"]["recovered_useful"].as_u64(),
            Some(2)
        );
        assert_eq!(
            summary["verdict_counts"]["hit_correct_target"].as_u64(),
            Some(2)
        );
    }

    #[test]
    fn continuity_answer_json_marks_last_chat_as_recovered_useful() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let handoff = json!({
            "headline": "Temporal lookup materialized",
            "next_step": "Проверить lookup по точному времени на реальном новом чате."
        });
        let restore = json!({
            "working_state_restore": {
                "materialized_notes": ["temporal lookup materialized"],
                "recent_actions": [],
                "active_files": ["src/continuity.rs"],
                "restore_confidence": "high",
                "latest_decision_trace": {
                    "included": [{
                        "strategy": "exact_documents",
                        "count": 1,
                        "reason": "Нашлись точные совпадения по continuity."
                    }],
                    "not_included": [{
                        "strategy": "semantic_chunks",
                        "reason": "Semantic layer честно abstained и не добавил фрагменты."
                    }]
                }
            }
        });
        let answer = render_direct_answer(&handoff, Some(&restore), None, "last_chat", None, 1);
        let payload = build_continuity_answer_payload(
            &project,
            &namespace,
            &handoff,
            Some(&restore),
            None,
            "last_chat",
            Some("на чем остановились"),
            None,
            None,
            2,
            false,
            1,
            &answer,
        )
        .expect("payload");
        assert_eq!(
            payload["continuity_answer"]["canonical_eval"]["verdict_counts"]["recovered_useful"]
                .as_u64(),
            Some(1)
        );
        assert_eq!(
            payload["retrieval_science"]["suite_key"].as_str(),
            Some("continuity_answer")
        );
        assert_eq!(
            payload["continuity_answer"]["included_reasons_summary"].as_str(),
            Some("точные совпадения (1) — Нашлись точные совпадения по continuity.")
        );
        assert!(
            payload["continuity_answer"]["answer_text"]
                .as_str()
                .is_some_and(|value| value.contains("Почему вошёл текущий контекст:"))
        );
    }

    #[test]
    fn continuity_answer_json_marks_previous_chat_as_recovered_useful() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-2".to_string(),
            title: "чат про continuity".to_string(),
            summary_headline: Some("Закончили на temporal contour.".to_string()),
            summary_next_step: Some("Проверить новый чат ещё раз.".to_string()),
            selected_time_slice: None,
            messages: vec![TranscriptMessage {
                role: "assistant".to_string(),
                text: "Закончили на temporal contour.".to_string(),
            }],
        };
        let answer =
            render_direct_answer(&handoff, None, Some(&chat_tail), "previous_chat", None, 1);
        let payload = build_continuity_answer_payload(
            &project,
            &namespace,
            &handoff,
            None,
            Some(&chat_tail),
            "previous_chat",
            Some("что было в прошлом чате"),
            Some("previous"),
            None,
            2,
            true,
            1,
            &answer,
        )
        .expect("payload");
        assert_eq!(
            payload["continuity_answer"]["canonical_eval"]["verdict_counts"]["recovered_useful"]
                .as_u64(),
            Some(1)
        );
    }

    #[test]
    fn continuity_answer_json_marks_missing_exact_time_as_fail_closed_hit() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let answer = render_direct_answer(
            &handoff,
            None,
            None,
            "chat_at_time",
            Some("2099-01-01T12:00:00Z"),
            1,
        );
        let payload = build_continuity_answer_payload(
            &project,
            &namespace,
            &handoff,
            None,
            None,
            "chat_at_time",
            Some("что было в прошлую среду"),
            None,
            Some("2099-01-01T12:00:00Z"),
            2,
            true,
            1,
            &answer,
        )
        .expect("payload");
        assert_eq!(
            payload["continuity_answer"]["canonical_eval"]["verdict_counts"]["hit_correct_target"]
                .as_u64(),
            Some(1)
        );
    }

    #[test]
    fn continuity_restore_payload_keeps_recovered_useful_eval_layer() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let continuity = json!({
            "imported_at_epoch_ms": 1_234_567,
            "bootstrap_summary": {
                "details": {
                    "thread_count": 18,
                    "latest_rendered_transcript": "/tmp/rendered.md"
                }
            }
        });
        let handoff = json!({
            "headline": "Temporal lookup materialized",
            "next_step": "Проверить lookup по точному времени на реальном новом чате."
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Temporal lookup materialized",
                "next_step": "Проверить lookup по точному времени на реальном новом чате.",
                "restore_confidence": "high",
                "state_lineage": {
                    "authoritative_event_id": "event-1"
                },
                "materialized_notes": ["temporal lookup materialized"],
                "recent_actions": [],
                "active_files": [],
                "open_questions": [],
                "latest_decision_trace": {
                    "included": [{
                        "strategy": "exact_documents",
                        "count": 1,
                        "reason": "Нашлись точные совпадения по continuity."
                    }],
                    "not_included": [{
                        "strategy": "semantic_chunks",
                        "reason": "Semantic layer честно abstained и не добавил фрагменты."
                    }]
                },
                "workspace_graph": json!({
                    "summary": {"file_count": 0, "structure_item_count": 0, "symbol_count": 0, "chunk_count": 0, "import_count": 0, "export_count": 0, "call_count": 0, "edge_count": 0}
                }),
            }
        });
        let chat_start_restore =
            build_chat_start_restore(&project, &namespace, &continuity, &handoff, Some(&restore));
        let payload = build_continuity_restore_payload(
            &project,
            &namespace,
            &continuity,
            &handoff,
            &restore,
            &chat_start_restore,
        )
        .expect("payload");
        assert_eq!(
            payload["continuity_restore"]["canonical_eval"]["verdict_counts"]["recovered_useful"]
                .as_u64(),
            Some(2)
        );
        assert_eq!(
            payload["retrieval_science"]["suite_key"].as_str(),
            Some("continuity_restore")
        );
        assert_eq!(
            payload["chat_start_restore"]["included_reasons_summary"].as_str(),
            Some("точные совпадения (1) — Нашлись точные совпадения по continuity.")
        );
        assert!(
            payload["chat_start_restore"]["prompt_text"]
                .as_str()
                .is_some_and(|value| !value.contains("Почему вошёл последний контекст:"))
        );
    }

    #[test]
    fn continuity_startup_payload_keeps_recovered_useful_eval_layer() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let continuity = json!({
            "imported_at_epoch_ms": 1_234_567,
            "documents_imported": 4,
            "rendered_transcript_files": 3,
            "bootstrap_summary": {
                "details": {
                    "thread_count": 18,
                    "latest_rendered_transcript": "/tmp/rendered.md"
                }
            }
        });
        let handoff = json!({
            "headline": "Temporal lookup materialized",
            "next_step": "Проверить lookup по точному времени на реальном новом чате."
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Temporal lookup materialized",
                "next_step": "Проверить lookup по точному времени на реальном новом чате.",
                "restore_confidence": "high",
                "state_lineage": {
                    "authoritative_event_id": "event-1"
                },
                "materialized_notes": ["temporal lookup materialized"],
                "recent_actions": [],
                "active_files": [],
                "open_questions": [],
                "latest_decision_trace": {
                    "included": [{
                        "strategy": "exact_documents",
                        "count": 1,
                        "reason": "Нашлись точные совпадения по continuity."
                    }],
                    "not_included": [{
                        "strategy": "semantic_chunks",
                        "reason": "Semantic layer честно abstained и не добавил фрагменты."
                    }]
                },
                "workspace_graph": json!({
                    "summary": {"file_count": 0, "structure_item_count": 0, "symbol_count": 0, "chunk_count": 0, "import_count": 0, "export_count": 0, "call_count": 0, "edge_count": 0}
                }),
            }
        });
        let context = ContinuityStartupContext {
            project,
            namespace,
            continuity,
            handoff_summary: handoff,
            restore: Some(restore),
        };
        let chat_start_restore = build_chat_start_restore(
            &context.project,
            &context.namespace,
            &context.continuity,
            &context.handoff_summary,
            context.restore.as_ref(),
        );
        let payload =
            build_continuity_startup_payload(&context, &chat_start_restore).expect("payload");
        assert_eq!(
            payload["continuity_startup"]["canonical_eval"]["verdict_counts"]["recovered_useful"]
                .as_u64(),
            Some(3)
        );
        assert_eq!(
            payload["retrieval_science"]["suite_key"].as_str(),
            Some("continuity_startup")
        );
        assert_eq!(
            payload["chat_start_restore"]["included_reasons_summary"].as_str(),
            Some("точные совпадения (1) — Нашлись точные совпадения по continuity.")
        );
    }

    #[test]
    fn render_direct_answer_marks_ordinal_previous_chat_lookup() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-2".to_string(),
            title: "чат про continuity".to_string(),
            summary_headline: Some("Закончили на temporal contour.".to_string()),
            summary_next_step: Some("Проверить новый чат ещё раз.".to_string()),
            selected_time_slice: None,
            messages: vec![],
        };

        let answer =
            render_direct_answer(&handoff, None, Some(&chat_tail), "previous_chat", None, 3);

        assert!(answer.contains("Смещение назад по чатам: 3"));
    }

    #[test]
    fn render_direct_answer_omits_redundant_noisy_chat_label() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-3".to_string(),
            title: "AGENTS.md прочитан. Продолжай строго из `/home/art/Art`.".to_string(),
            summary_headline: Some(
                "Amai ordinal chat recovery and prompt restore materialized".to_string(),
            ),
            summary_next_step: None,
            selected_time_slice: None,
            messages: vec![TranscriptMessage {
                role: "assistant".to_string(),
                text: "Amai ordinal chat recovery and prompt restore materialized".to_string(),
            }],
        };

        let answer = render_direct_answer(&handoff, None, Some(&chat_tail), "last_chat", None, 1);

        assert!(
            !answer.contains("Найденный chat thread: AGENTS.md прочитан"),
            "noisy chat label must be suppressed"
        );
    }

    #[test]
    fn render_direct_answer_omits_continue_strictly_label() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-4".to_string(),
            title: "Продолжай строго из `/home/art/Art` и строго по `/home/art/Art/AGENTS.md`."
                .to_string(),
            summary_headline: Some("Human continuity summary".to_string()),
            summary_next_step: None,
            selected_time_slice: None,
            messages: vec![TranscriptMessage {
                role: "assistant".to_string(),
                text: "Human continuity summary".to_string(),
            }],
        };

        let answer = render_direct_answer(&handoff, None, Some(&chat_tail), "last_chat", None, 1);

        assert!(
            !answer.contains("Найденный chat thread: Продолжай строго"),
            "system-style chat label must be suppressed"
        );
    }

    #[test]
    fn render_direct_answer_omits_overlong_exact_time_thread_label() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-5".to_string(),
            title: "теперь у нас реализован механизм отслеживания корреляции реадми в доменах и поддоменах и в docs? чтобы мы видели если где изменился документ по дате, то нужно посмотреть соответствующий раздел".to_string(),
            summary_headline: Some("Проверю текущее покрытие механизма и сразу закреплю правило в `AGENTS.md`.".to_string()),
            summary_next_step: None,
            selected_time_slice: Some(ThreadTimeSliceSummary {
                started_at: "2026-03-11T06:19:47.067Z".to_string(),
                ended_at: "2026-03-11T06:19:58.631Z".to_string(),
                started_at_epoch_s: 1,
                ended_at_epoch_s: 2,
                user_anchor: "теперь у нас реализован механизм отслеживания корреляции реадми в доменах и поддоменах и в docs? чтобы мы видели если где изменился документ по дате".to_string(),
                assistant_anchor: "Проверю текущее покрытие механизма и сразу закреплю правило в `AGENTS.md`.".to_string(),
                summary_headline: "Проверю текущее покрытие механизма и сразу закреплю правило в `AGENTS.md`.".to_string(),
                summary_next_step: String::new(),
            }),
            messages: vec![
                TranscriptMessage {
                    role: "user".to_string(),
                    text: "теперь у нас реализован механизм отслеживания корреляции реадми...".to_string(),
                },
                TranscriptMessage {
                    role: "assistant".to_string(),
                    text: "Проверю текущее покрытие механизма и сразу закреплю правило в `AGENTS.md`.".to_string(),
                },
            ],
        };

        let answer = render_direct_answer(
            &handoff,
            None,
            Some(&chat_tail),
            "chat_at_time",
            Some("2026-03-11T12:00:00+03:00"),
            1,
        );

        assert!(!answer.contains("Подходящий chat thread:"));
    }

    #[test]
    fn extract_next_step_normalizes_nested_labels() {
        let text = "Сводка.\nБлижайший обязательный следующий шаг: Следующий обязательный шаг: materialize compact thread summaries.`|";
        let next_step = extract_next_step_from_text(text).expect("next step");
        assert_eq!(next_step, "materialize compact thread summaries.");
    }

    #[test]
    fn parse_chat_reference_spec_supports_previous_offsets() {
        assert_eq!(parse_chat_reference_spec("previous"), ("previous", 1));
        assert_eq!(parse_chat_reference_spec("previous:2"), ("previous", 2));
        assert_eq!(parse_chat_reference_spec("current"), ("current", 1));
    }

    #[test]
    fn resolve_answer_intent_promotes_previous_and_exact_time_paths() {
        assert_eq!(
            resolve_answer_intent("last_chat", None, Some("previous"), false),
            "previous_chat"
        );
        assert_eq!(
            resolve_answer_intent("last_chat", None, Some("current"), true),
            "chat_at_time"
        );
        assert_eq!(
            resolve_answer_intent("last_chat", Some("previous_chat"), Some("current"), false),
            "previous_chat"
        );
    }

    #[test]
    fn build_chat_start_restore_emits_prompt_and_compact_summaries() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let continuity = json!({
            "bootstrap_summary": {
                "details": {
                    "thread_count": 16,
                    "latest_rendered_transcript": "/tmp/rendered.md"
                }
            }
        });
        let handoff = json!({
            "headline": "Amai upstream thread-index enrich materialized",
            "next_step": "Сделать auto-injection restore pack прямо в chat-start prompt."
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Amai upstream thread-index enrich materialized",
                "restore_confidence": "high",
                "execctl_resume_state": "pending_return_queue_present",
                "pending_return_summary": "Same-meter spend control -> Materialize live assistant generation source.",
                "execctl_resume_contract": {
                    "resume_state": "return_required",
                    "no_silent_drop": true,
                    "pending_return_count": 1,
                    "active_task": {
                        "headline": "Amai upstream thread-index enrich materialized"
                    },
                    "required_return_task": {
                        "headline": "Same-meter spend control",
                        "next_step": "Materialize live assistant generation source."
                    }
                },
                "execctl_resume_contract_summary": "return_required(1): Same-meter spend control -> Materialize live assistant generation source.",
                "startup_next_action": {
                    "action_kind": "resume_required_return_task",
                    "blocking": true,
                    "headline": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                },
                "startup_next_action_summary": "resume_required_return_task: Same-meter spend control -> Materialize live assistant generation source.",
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Amai upstream thread-index enrich materialized",
                    "next_step": "Сделать auto-injection restore pack прямо в chat-start prompt.",
                    "storage_lane": "ami.execctl_task_leases"
                },
                "execctl_active_lease_summary": "previous_session_owner: Amai upstream thread-index enrich materialized -> Сделать auto-injection restore pack прямо в chat-start prompt.",
                "project_task_tree": {
                    "open_tasks_count": 2,
                    "nodes": [
                        {"task_role": "active", "headline": "Amai upstream thread-index enrich materialized"},
                        {"task_role": "pending_return", "headline": "Same-meter spend control"}
                    ]
                },
                "project_task_tree_summary": "active: Amai upstream thread-index enrich materialized; pending_return(1): Same-meter spend control -> Materialize live assistant generation source.",
                "project_task_ledger": {
                    "open_tasks_count": 2,
                    "historical_handoffs_count": 3,
                    "entries": [
                        {"task_role": "active", "headline": "Amai upstream thread-index enrich materialized"}
                    ]
                },
                "materialized_notes": [
                    "Enriched temporal summaries теперь пишутся upstream."
                ],
                "recent_actions": [
                    {"headline": "Проверили previous chat lookup"},
                    {"headline": "Проверили exact-time lookup"}
                ],
                "active_files": [
                    "/home/art/agent-memory-index/src/continuity.rs",
                    "/home/art/Art/scripts/tools/amai_art_continuity_startup.sh"
                ],
                "open_questions": [
                    "Как сделать auto-injection без дополнительного helper-обхода?"
                ]
            }
        });

        let pack =
            build_chat_start_restore(&project, &namespace, &continuity, &handoff, Some(&restore));
        let node = &pack["chat_start_restore"];
        let prompt = node["prompt_text"].as_str().expect("prompt text");
        assert!(prompt.contains("CHAT_START_RESTORE"));
        assert!(prompt.contains("Линия: Amai upstream thread-index enrich materialized"));
        assert!(
            prompt.contains("Шаг: Сделать auto-injection restore pack прямо в chat-start prompt.")
        );
        assert!(prompt.contains("Сделано: Enriched temporal summaries теперь пишутся upstream."));
        assert!(prompt.contains("Сначала: вернись к линии: Same-meter spend control"));
        assert!(prompt.contains("Не переключайся до возврата."));
        assert!(prompt.contains("Правило: follow pack; continuity не поднимай."));
        assert!(!prompt.contains("Недавнее:"));
        assert!(!prompt.contains("Файлы:"));
        assert!(!prompt.contains("Задачи:"));
        assert!(!prompt.contains("Возврат:"));
        assert!(!prompt.contains("Вопросы:"));
        assert!(!prompt.contains("Thread count in continuity index"));
        assert!(!prompt.contains("Контракт возврата ExecCtl"));
        assert!(!prompt.contains("Активный lease ExecCtl"));
        assert_eq!(node["thread_count"], json!(16));
        assert_eq!(
            node["execctl_resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            node["execctl_resume_obligation"]["resume_state"],
            json!("return_required")
        );
        assert_eq!(
            node["execctl_resume_obligation"]["required_return_headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(
            node["startup_next_action"]["action_kind"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            node["startup_next_action"]["headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(
            node["required_return_task"]["headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(node["project_task_tree"]["open_tasks_count"], json!(2));
        assert_eq!(
            node["project_task_ledger"]["historical_handoffs_count"],
            json!(3)
        );
        assert_eq!(
            node["execctl_active_lease"]["storage_lane"],
            json!("ami.execctl_task_leases")
        );
    }

    #[test]
    fn build_chat_start_restore_prefers_live_client_budget_summary_over_stale_notes() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let continuity = json!({
            "bootstrap_summary": {
                "details": {
                    "thread_count": 3,
                    "latest_rendered_transcript": "/tmp/rendered.md"
                }
            }
        });
        let handoff = json!({
            "headline": "Продолжить активную рабочую линию",
            "next_step": "продолжить работу в свежем чате через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "restore_confidence": "high",
                "client_budget_guard": {
                    "status_label": "новый чат нужен сейчас",
                    "last_request": "162594 из 258400, остаётся 37.08% · raw 23:27:06 MSK",
                    "client_limits": "5ч остаётся 8.00%, 7д остаётся 72.00% · raw 23:27:06 MSK"
                },
                "materialized_notes": [
                    "Client-budget guard: новый чат рекомендован.",
                    "Последний запрос в модель: 190690 из 258400, остаётся 26.20% · raw 23:15:46 MSK.",
                    "Лимит клиента сейчас: 5ч остаётся 8.00%, 7д остаётся 72.00% · raw 23:15:46 MSK.",
                    "Продолжить ту же рабочую линию: Продолжить активную рабочую линию."
                ]
            }
        });

        let pack =
            build_chat_start_restore(&project, &namespace, &continuity, &handoff, Some(&restore));
        let node = &pack["chat_start_restore"];
        let prompt = node["prompt_text"].as_str().expect("prompt text");
        let materialized = node["materialized_summary"]
            .as_str()
            .expect("materialized summary");

        assert!(prompt.contains("Client-budget guard: новый чат нужен сейчас."));
        assert!(!prompt.contains("190690 из 258400"));
        assert!(materialized.contains("162594 из 258400"));
        assert!(materialized.contains("23:27:06 MSK"));
        assert!(!materialized.contains("23:15:46 MSK"));
    }

    #[test]
    fn build_chat_start_restore_prompt_includes_blocked_reply_text_for_rotate_path() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let continuity = json!({
            "bootstrap_summary": {
                "details": {
                    "thread_count": 3,
                    "latest_rendered_transcript": "/tmp/rendered.md"
                }
            }
        });
        let handoff = json!({
            "headline": "Продолжить активную рабочую линию",
            "next_step": "продолжить работу в свежем чате через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "restore_confidence": "high",
                "startup_next_action": {
                    "action_kind": "rotate_chat_for_client_budget",
                    "blocking": true,
                    "headline": "Клиентский лимит: новый чат нужен сейчас",
                    "next_step": "сохрани handoff и продолжай только в свежем чате через continuity startup"
                },
                "client_budget_guard": {
                    "reply_execution_gate": {
                        "blocking_reply_contract": {
                            "template": "Этот чат уже жжёт внешний лимит клиента. Сохрани handoff, открой новый чат и запусти continuity startup."
                        }
                    }
                }
            }
        });

        let pack =
            build_chat_start_restore(&project, &namespace, &continuity, &handoff, Some(&restore));
        let prompt = pack["chat_start_restore"]["prompt_text"]
            .as_str()
            .expect("prompt text");

        assert!(prompt.contains("В старом чате разрешён только короткий rotate-ответ."));
        assert!(prompt.contains("Разрешённый ответ: Этот чат уже жжёт внешний лимит клиента."));
    }

    #[test]
    fn compact_prompt_fragment_truncates_long_text() {
        let value = "Rolling-window token card now isolates historical startup drag from fresh profitable startup";
        let compact = super::compact_prompt_fragment(value, 36);
        assert!(compact.ends_with("..."));
        assert!(compact.chars().count() <= 39);
    }

    #[test]
    fn meaningful_restore_value_rejects_placeholder_text() {
        assert!(!super::is_meaningful_restore_value(""));
        assert!(!super::is_meaningful_restore_value("   "));
        assert!(!super::is_meaningful_restore_value("ещё нет данных"));
        assert!(super::is_meaningful_restore_value(
            "Продолжить активную рабочую линию"
        ));
    }

    #[test]
    fn startup_runtime_state_artifact_surfaces_machine_readable_return_fields() {
        let payload = json!({
            "continuity_startup": {
                "project": {
                    "code": "art",
                    "display_name": "Art",
                    "repo_root": "/tmp/amai-art"
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                }
            },
            "chat_start_restore": {
                "headline": "Current active line",
                "next_step": "Ship runtime return enforcement.",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
                "execctl_resume_state": "pending_return_queue_present",
                "startup_next_action": {
                    "action_kind": "resume_required_return_task",
                    "blocking": true,
                    "headline": "Pending line",
                    "next_step": "Close same-meter live gap."
                },
                "required_return_task": {
                    "headline": "Pending line",
                    "next_step": "Close same-meter live gap."
                },
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "open_tasks_count": 2
                },
                "project_task_tree_summary": "active: Current active line; pending_return(1): Pending line",
                "project_task_ledger": {
                    "open_tasks_count": 2
                },
                "project_task_ledger_summary": "active: Current active line; historical_handoffs(1)"
            },
            "working_state_restore": {
                "client_budget_guard": {
                    "status_label": "новый чат нужен сейчас",
                    "reply_execution_gate": {
                        "gate_version": "client-reply-budget-gate-v1",
                        "must_rotate_before_reply": true,
                        "blocking_reply_contract": {
                            "contract_version": "client-budget-blocked-reply-v1",
                            "response_kind": "rotate_chat_only",
                            "template": "Лимит клиента почти исчерпан. Сохрани handoff и продолжай только в свежем чате через continuity startup."
                        }
                    }
                },
                "state_lineage": {
                    "authoritative_event_id": "evt_123",
                    "session_id": "sess_123"
                }
            }
        });

        let artifact =
            build_startup_runtime_state_artifact(Path::new("/tmp/amai-art"), &payload, 42)
                .expect("startup runtime state artifact");

        assert_eq!(
            artifact["artifact_version"],
            json!("workspace-startup-runtime-state-v3")
        );
        assert_eq!(artifact["source_tool"], json!("amai_continuity_startup"));
        assert_eq!(
            artifact["source_summary_field"],
            json!("continuity_startup_summary")
        );
        assert_eq!(
            artifact["startup_execution_gate"]["gate_version"],
            json!("startup-execution-gate-v1")
        );
        assert_eq!(
            artifact["startup_execution_gate"]["must_follow_startup_next_action"],
            json!(true)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["unrelated_work_allowed"],
            json!(false)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["must_read_prompt_text_before_reply"],
            json!(true)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["required_action_kind_when_resume_required"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            artifact["startup_execution_gate"]["no_silent_drop"],
            json!(true)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["required_return_task_present"],
            json!(true)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["required_return_task_headline"],
            json!("Pending line")
        );
        assert_eq!(
            artifact["startup_execution_gate"]["required_return_task_next_step"],
            json!("Close same-meter live gap.")
        );
        assert_eq!(
            artifact["client_budget_guard"]["status_label"],
            json!("новый чат нужен сейчас")
        );
        assert_eq!(
            artifact["reply_execution_gate"]["gate_version"],
            json!("client-reply-budget-gate-v1")
        );
        assert_eq!(
            artifact["reply_execution_gate"]["must_rotate_before_reply"],
            json!(true)
        );
        assert_eq!(
            artifact["blocking_reply_contract"]["response_kind"],
            json!("rotate_chat_only")
        );
        assert_eq!(artifact["gate_semantics_consistent"], json!(true));
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_next_action"]["action_kind"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["required_return_task"]["headline"],
            json!("Pending line")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_tree_summary"],
            json!("active: Current active line; pending_return(1): Pending line")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["execctl_active_lease"]["lease_owner_state"],
            json!("previous_session_owner")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_ledger_summary"],
            json!("active: Current active line; historical_handoffs(1)")
        );
        assert_eq!(
            artifact["working_state_restore_lineage"]["authoritative_event_id"],
            json!("evt_123")
        );
    }

    #[test]
    fn inspect_startup_runtime_state_reports_gate_semantics_consistent() {
        let unique = format!(
            "amai-startup-runtime-audit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let repo = std::env::temp_dir().join(unique);
        fs::create_dir_all(repo.join(".amai/continuity")).expect("runtime state dir");

        let payload = json!({
            "continuity_startup": {
                "project": {
                    "code": "art",
                    "display_name": "Art",
                    "repo_root": repo.display().to_string()
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                }
            },
            "chat_start_restore": {
                "headline": "Current active line",
                "next_step": "Ship runtime return enforcement.",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
                "execctl_resume_state": "pending_return_queue_present",
                "startup_next_action": {
                    "action_kind": "resume_required_return_task",
                    "blocking": true,
                    "headline": "Pending line",
                    "next_step": "Close same-meter live gap."
                },
                "required_return_task": {
                    "headline": "Pending line",
                    "next_step": "Close same-meter live gap."
                },
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "open_tasks_count": 2
                },
                "project_task_tree_summary": "active: Current active line; pending_return(1): Pending line",
                "project_task_ledger": {
                    "open_tasks_count": 2
                },
                "project_task_ledger_summary": "active: Current active line; historical_handoffs(1)"
            },
            "working_state_restore": {
                "state_lineage": {
                    "authoritative_event_id": "evt_123",
                    "session_id": "sess_123"
                }
            }
        });
        let artifact = build_startup_runtime_state_artifact(repo.as_path(), &payload, 42)
            .expect("startup runtime state artifact");
        fs::write(
            startup_runtime_state_artifact_path(repo.as_path()),
            serde_json::to_string_pretty(&artifact).expect("serialize artifact"),
        )
        .expect("write artifact");

        let audit = inspect_startup_runtime_state(repo.as_path()).expect("startup runtime audit");
        assert_eq!(audit.status, "ok");
        assert_eq!(audit.artifact_gate_semantics_consistent_present, Some(true));
        assert_eq!(
            audit.artifact_gate_semantics_consistent_matches_recomputed,
            Some(true)
        );
        assert_eq!(audit.gate_semantics_consistent, Some(true));
        assert_eq!(audit.project_task_tree_summary_field_present, Some(true));
        assert_eq!(audit.project_task_ledger_summary_field_present, Some(true));

        fs::remove_dir_all(&repo).expect("cleanup temp repo");
    }

    #[test]
    fn startup_state_cli_json_surfaces_raw_artifact_fields_at_top_level() {
        let audit = StartupRuntimeStateAudit {
            status: "ok".to_string(),
            output_path: PathBuf::from("/tmp/project-chat-startup-state.json"),
            artifact_exists: true,
            startup_contract_sha_matches_current_contract: Some(true),
            source_summary_field_matches: Some(true),
            prompt_text_present: Some(true),
            startup_next_action_present: Some(true),
            startup_execution_gate_present: Some(true),
            required_return_task_field_present: Some(false),
            execctl_active_lease_field_present: Some(true),
            project_task_tree_field_present: Some(true),
            project_task_tree_summary_field_present: Some(true),
            project_task_ledger_field_present: Some(true),
            project_task_ledger_summary_field_present: Some(true),
            resume_state: Some("clear".to_string()),
            action_kind: Some("rotate_chat_for_client_budget".to_string()),
            lease_owner_state: Some("same_session_owner".to_string()),
            must_follow_startup_next_action: Some(true),
            unrelated_work_allowed: Some(false),
            must_read_prompt_text_before_reply: Some(true),
            required_action_kind_when_resume_required: Some(
                "resume_required_return_task".to_string(),
            ),
            no_silent_drop: Some(true),
            artifact_gate_semantics_consistent_present: Some(true),
            artifact_gate_semantics_consistent_matches_recomputed: Some(true),
            gate_semantics_consistent: Some(true),
        };
        let artifact = json!({
            "artifact_version": "workspace-startup-runtime-state-v3",
            "client_budget_guard": {
                "status_label": "новый чат нужен сейчас"
            },
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1"
            },
            "blocking_reply_contract": {
                "response_kind": "rotate_chat_only"
            },
            "startup_execution_gate": {
                "gate_version": "startup-execution-gate-v1"
            },
            "continuity_startup_summary": {
                "startup_next_action": {
                    "action_kind": "rotate_chat_for_client_budget"
                },
                "required_return_task": Value::Null,
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner"
                },
                "project_task_tree": {
                    "open_tasks_count": 1
                },
                "project_task_ledger": {
                    "open_tasks_count": 1
                }
            }
        });

        let payload = build_startup_runtime_state_cli_json(&audit, Some(&artifact));

        assert_eq!(
            payload["client_budget_guard"]["status_label"],
            json!("новый чат нужен сейчас")
        );
        assert_eq!(
            payload["reply_execution_gate"]["gate_version"],
            json!("client-reply-budget-gate-v1")
        );
        assert_eq!(
            payload["blocking_reply_contract"]["response_kind"],
            json!("rotate_chat_only")
        );
        assert_eq!(
            payload["startup_execution_gate"]["gate_version"],
            json!("startup-execution-gate-v1")
        );
        assert_eq!(
            payload["startup_runtime_state"]["startup_execution_gate"]["gate_version"],
            json!("startup-execution-gate-v1")
        );
        assert_eq!(
            payload["startup_runtime_state_audit"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
    }

    #[test]
    fn enrich_thread_index_file_writes_compact_summary_fields() {
        let temp_root =
            std::env::temp_dir().join(format!("amai-thread-index-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_root);
        let rollout_path = temp_root.join("rollout.jsonl");
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-03-21T12:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"на чем закончили?"}]}}
{"timestamp":"2026-03-21T12:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"По `Amai` активная линия тогда была `Amai compact thread summaries materialized`.\nБлижайший обязательный следующий шаг: Вынести summary_headline и summary_next_step вверх."}]}}
"#,
        )
        .expect("write rollout");
        let input_path = temp_root.join("thread_index.json");
        let output_path = temp_root.join("thread_index.enriched.json");
        fs::write(
            &input_path,
            format!(
                "{{\"threads\":[{{\"thread_id\":\"thread-1\",\"title\":\"test\",\"cwd\":\"/home/art/Art\",\"source_rollout\":\"{}\",\"raw_mirror\":\"{}\",\"rendered_transcript\":\"\"}}]}}\n",
                rollout_path.display(),
                rollout_path.display()
            ),
        )
        .expect("write thread index");

        enrich_thread_index_file(&ContinuityThreadIndexEnrichArgs {
            input: input_path.clone(),
            output: Some(output_path.clone()),
        })
        .expect("enrich");

        let enriched: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output_path).expect("read output"))
                .expect("parse output");
        assert_eq!(
            enriched["threads"][0]["summary_headline"],
            json!("Amai compact thread summaries materialized")
        );
        assert_eq!(
            enriched["threads"][0]["summary_next_step"],
            json!("Вынести summary_headline и summary_next_step вверх.")
        );
        assert_eq!(enriched["threads"][0]["messages_count"], json!(2));

        let _ = fs::remove_file(&output_path);
        let _ = fs::remove_file(&input_path);
        let _ = fs::remove_file(&rollout_path);
        let _ = fs::remove_dir_all(&temp_root);
    }
}
