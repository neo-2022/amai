use crate::mcp;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use super::{
    ContinuityStartupStateArgs, StartupRuntimeStateAudit, canonical_path,
    compact_project_task_ledger_for_startup, compact_project_task_tree_for_startup,
    compact_startup_runtime_client_budget_guard, compact_startup_runtime_execctl_active_lease,
    compact_startup_runtime_execctl_resume_obligation,
    compact_startup_runtime_required_return_task, compact_startup_runtime_startup_next_action,
    compact_startup_runtime_summary_startup_execution_gate,
    compact_startup_state_cli_blocking_reply_contract,
    compact_startup_state_cli_client_budget_guard, compact_startup_state_cli_reply_execution_gate,
    compact_startup_state_cli_required_return_task,
    compact_startup_state_cli_startup_execution_gate,
    compact_startup_state_cli_startup_next_action, copy_if_present, hex_sha256, now_epoch_ms,
    prune_startup_runtime_summary,
};

pub(crate) const STARTUP_RUNTIME_STATE_ARTIFACT_VERSION: &str =
    "workspace-startup-runtime-state-v4";

pub(crate) fn startup_runtime_state_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/continuity/project-chat-startup-state.json")
}

pub(super) fn load_startup_runtime_state_artifact(repo_root: &Path) -> Result<Option<Value>> {
    let output_path = startup_runtime_state_artifact_path(repo_root);
    if !output_path.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&output_path)
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
        != Some(STARTUP_RUNTIME_STATE_ARTIFACT_VERSION)
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

pub(crate) fn print_startup_runtime_state(args: &ContinuityStartupStateArgs) -> Result<()> {
    let repo_root = canonical_path(&args.repo_root)?;
    let audit = inspect_startup_runtime_state(&repo_root)?;
    let artifact_payload = load_startup_runtime_state_artifact(&repo_root)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&build_startup_runtime_state_cli_json(
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
        "project_task_tree summary field present: {}",
        audit
            .project_task_tree_summary_field_present
            .unwrap_or(false)
    );
    println!(
        "project_task_ledger field present: {}",
        audit.project_task_ledger_field_present.unwrap_or(false)
    );
    println!(
        "project_task_ledger summary field present: {}",
        audit
            .project_task_ledger_summary_field_present
            .unwrap_or(false)
    );
    println!(
        "resume_state: {}",
        audit
            .resume_state
            .clone()
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!(
        "action_kind: {}",
        audit
            .action_kind
            .clone()
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!(
        "lease_owner_state: {}",
        audit
            .lease_owner_state
            .clone()
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!(
        "must_follow_startup_next_action: {}",
        audit.must_follow_startup_next_action.unwrap_or(false)
    );
    println!(
        "unrelated_work_allowed: {}",
        audit.unrelated_work_allowed.unwrap_or(false)
    );
    println!(
        "must_read_prompt_text_before_reply: {}",
        audit.must_read_prompt_text_before_reply.unwrap_or(false)
    );
    println!(
        "required_action_kind_when_resume_required: {}",
        audit
            .required_action_kind_when_resume_required
            .clone()
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!("no_silent_drop: {}", audit.no_silent_drop.unwrap_or(false));
    println!(
        "artifact_gate_semantics_consistent_present: {}",
        audit
            .artifact_gate_semantics_consistent_present
            .unwrap_or(false)
    );
    println!(
        "artifact_gate_semantics_consistent_matches_recomputed: {}",
        audit
            .artifact_gate_semantics_consistent_matches_recomputed
            .unwrap_or(false)
    );
    println!(
        "gate_semantics_consistent: {}",
        audit.gate_semantics_consistent.unwrap_or(false)
    );
    Ok(())
}

pub(super) fn startup_runtime_state_audit_json(
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
        "client_budget_guard": artifact_payload
            .map(|payload| {
                compact_startup_state_cli_client_budget_guard(
                    &payload["client_budget_guard"],
                )
            })
            .unwrap_or(Value::Null),
        "reply_execution_gate": artifact_payload
            .map(|payload| {
                compact_startup_state_cli_reply_execution_gate(
                    &payload["reply_execution_gate"],
                )
            })
            .unwrap_or(Value::Null),
        "blocking_reply_contract": artifact_payload
            .map(|payload| {
                compact_startup_state_cli_blocking_reply_contract(
                    &payload["blocking_reply_contract"],
                )
            })
            .unwrap_or(Value::Null),
        "startup_execution_gate": artifact_payload
            .map(|payload| {
                compact_startup_state_cli_startup_execution_gate(
                    &payload["startup_execution_gate"],
                )
            })
            .unwrap_or(Value::Null),
        "startup_next_action": artifact_payload
            .map(|payload| {
                compact_startup_state_cli_startup_next_action(
                    &payload["continuity_startup_summary"]["startup_next_action"],
                )
            })
            .unwrap_or(Value::Null),
        "required_return_task": artifact_payload
            .map(|payload| {
                compact_startup_state_cli_required_return_task(
                    &payload["continuity_startup_summary"]["required_return_task"],
                )
            })
            .unwrap_or(Value::Null),
        "execctl_active_lease": artifact_payload
            .map(|payload| {
                compact_startup_runtime_execctl_active_lease(
                    &payload["continuity_startup_summary"]["execctl_active_lease"],
                )
            })
            .unwrap_or(Value::Null),
        "project_task_tree": artifact_payload.map(|payload| payload["continuity_startup_summary"]["project_task_tree"].clone()).unwrap_or(Value::Null),
        "project_task_ledger": artifact_payload.map(|payload| payload["continuity_startup_summary"]["project_task_ledger"].clone()).unwrap_or(Value::Null),
    })
}

pub(super) fn build_startup_runtime_state_cli_json(
    audit: &StartupRuntimeStateAudit,
    artifact_payload: Option<&Value>,
) -> Value {
    let runtime_state = startup_runtime_state_audit_json(audit, artifact_payload);
    let startup_runtime_state_audit = json!({
        "status": runtime_state["status"].clone(),
        "action_kind": runtime_state["action_kind"].clone(),
        "lease_owner_state": runtime_state["lease_owner_state"].clone(),
        "gate_semantics_consistent": runtime_state["gate_semantics_consistent"].clone(),
    });
    json!({
        "startup_runtime_state": runtime_state,
        "startup_runtime_state_audit": startup_runtime_state_audit,
    })
}

pub(super) fn build_startup_runtime_state_artifact(
    repo_root: &Path,
    payload: &Value,
    generated_at_epoch_ms: u64,
) -> Result<Value> {
    let startup_contract_sha256 =
        hex_sha256(&serde_json::to_vec(&mcp::project_chat_startup_contract())?);
    let startup_execution_gate = build_startup_execution_gate(payload);
    let mut continuity_startup_summary = mcp::continuity_startup_summary_json(payload);
    if let Some(summary) = continuity_startup_summary.as_object_mut() {
        summary.insert(
            "startup_execution_gate".to_string(),
            compact_startup_runtime_summary_startup_execution_gate(&startup_execution_gate),
        );
        summary.insert(
            "startup_next_action".to_string(),
            compact_startup_runtime_startup_next_action(
                summary.get("startup_next_action").unwrap_or(&Value::Null),
            ),
        );
        summary.insert(
            "required_return_task".to_string(),
            compact_startup_runtime_required_return_task(
                summary.get("required_return_task").unwrap_or(&Value::Null),
            ),
        );
        summary.insert(
            "execctl_resume_obligation".to_string(),
            compact_startup_runtime_execctl_resume_obligation(
                summary
                    .get("execctl_resume_obligation")
                    .unwrap_or(&Value::Null),
            ),
        );
        let execctl_active_lease_source = summary
            .get("execctl_active_lease")
            .filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
            .cloned()
            .unwrap_or_else(|| payload["chat_start_restore"]["execctl_active_lease"].clone());
        summary.insert(
            "execctl_active_lease".to_string(),
            compact_startup_runtime_execctl_active_lease(&execctl_active_lease_source),
        );
        summary.insert(
            "project_task_tree".to_string(),
            compact_project_task_tree_for_startup(
                summary.get("project_task_tree").unwrap_or(&Value::Null),
            ),
        );
        summary.insert(
            "project_task_ledger".to_string(),
            compact_project_task_ledger_for_startup(
                summary.get("project_task_ledger").unwrap_or(&Value::Null),
            ),
        );
        prune_startup_runtime_summary(summary);
    }
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
    let raw_client_budget_guard = &payload["working_state_restore"]["client_budget_guard"];
    let client_budget_guard = compact_startup_runtime_client_budget_guard(raw_client_budget_guard);
    let reply_execution_gate = if client_budget_guard["reply_execution_gate"].is_object() {
        client_budget_guard["reply_execution_gate"].clone()
    } else {
        Value::Null
    };
    let blocking_reply_contract =
        if raw_client_budget_guard["reply_execution_gate"]["blocking_reply_contract"].is_object() {
            raw_client_budget_guard["reply_execution_gate"]["blocking_reply_contract"].clone()
        } else if reply_execution_gate["blocking_reply_contract"].is_object() {
            reply_execution_gate["blocking_reply_contract"].clone()
        } else {
            Value::Null
        };
    let execctl_resume_state = continuity_startup_summary["execctl_resume_state"].clone();
    let execctl_resume_contract_summary =
        continuity_startup_summary["execctl_resume_contract_summary"].clone();
    let execctl_resume_obligation = continuity_startup_summary["execctl_resume_obligation"].clone();
    let startup_next_action = continuity_startup_summary["startup_next_action"].clone();
    let execctl_active_lease = continuity_startup_summary["execctl_active_lease"].clone();
    let required_return_task = continuity_startup_summary["required_return_task"].clone();
    Ok(json!({
        "artifact_version": STARTUP_RUNTIME_STATE_ARTIFACT_VERSION,
        "repo_root": repo_root.display().to_string(),
        "generated_at_epoch_ms": generated_at_epoch_ms,
        "source_tool": "amai_continuity_startup",
        "source_summary_field": "continuity_startup_summary",
        "startup_contract_sha256": startup_contract_sha256,
        "continuity_startup_summary": continuity_startup_summary,
        "startup_execution_gate": startup_execution_gate,
        "execctl_resume_state": execctl_resume_state,
        "execctl_resume_contract_summary": execctl_resume_contract_summary,
        "execctl_resume_obligation": execctl_resume_obligation,
        "startup_next_action": startup_next_action,
        "execctl_active_lease": execctl_active_lease,
        "required_return_task": required_return_task,
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
            compact_startup_runtime_state_lineage(&payload["working_state_restore"]["state_lineage"])
        } else {
            Value::Null
        }
    }))
}

pub(super) fn compact_startup_runtime_state_lineage(lineage: &Value) -> Value {
    let Some(source) = lineage.as_object() else {
        return Value::Null;
    };
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        lineage,
        &[
            "authoritative_event_id",
            "authoritative_event_kind",
            "authoritative_source_kind",
            "authoritative_local_path",
            "lineage_model_version",
        ],
    );
    if let Some(nodes) = source.get("nodes").and_then(Value::as_array) {
        if !nodes.is_empty() {
            compact.insert("nodes_total".to_string(), Value::from(nodes.len() as u64));
            if let Some(authoritative_headline) = nodes.iter().find_map(|node| {
                (node["authoritative"].as_bool() == Some(true))
                    .then(|| node["headline"].as_str())
                    .flatten()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            }) {
                compact.insert(
                    "authoritative_headline".to_string(),
                    Value::from(authoritative_headline),
                );
            }
        }
    }
    if let Some(edges) = source.get("edges").and_then(Value::as_array) {
        if !edges.is_empty() {
            compact.insert("edges_total".to_string(), Value::from(edges.len() as u64));
        }
    }
    if let Some(supporting_event_ids) = source.get("supporting_event_ids").and_then(Value::as_array)
    {
        if !supporting_event_ids.is_empty() {
            compact.insert(
                "supporting_event_count".to_string(),
                Value::from(supporting_event_ids.len() as u64),
            );
        }
    }
    Value::Object(compact)
}

pub(super) fn persist_startup_runtime_state_artifact(
    repo_root: &Path,
    payload: &Value,
) -> Result<()> {
    let output_path = startup_runtime_state_artifact_path(repo_root);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let artifact = build_startup_runtime_state_artifact(repo_root, payload, now_epoch_ms()?)?;
    let content = serde_json::to_string(&artifact)
        .context("failed to serialize startup runtime state artifact")?;
    std::fs::write(&output_path, content)
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    Ok(())
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
    let required_task_set_count = payload["chat_start_restore"]["required_task_set"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or_default();
    let must_follow = blocking
        || (must_resume_before_unrelated && action_kind == required_action_kind)
        || lease_owner_state == Some(previous_session_owner_value);

    json!({
        "gate_version": "startup-execution-gate-v1",
        "action_kind": action_kind,
        "blocking": blocking,
        "required_task_set_count": required_task_set_count,
        "resume_state": payload["chat_start_restore"]["execctl_resume_state"]
            .as_str()
            .unwrap_or("clear"),
        "required_return_task_present": payload["chat_start_restore"]["required_return_task"].is_object(),
        "required_return_task_headline": payload["chat_start_restore"]["required_return_task"]["headline"]
            .as_str(),
        "required_return_task_next_step": payload["chat_start_restore"]["required_return_task"]["next_step"]
            .as_str(),
        "required_task_set_present": required_task_set_count > 0,
        "must_preserve_required_task_set": required_task_set_count > 0,
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
