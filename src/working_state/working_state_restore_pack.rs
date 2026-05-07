use serde_json::{Value, json};
use std::collections::BTreeSet;

use super::{
    MAX_ACTIVE_FILES, MAX_OPEN_QUESTIONS, MAX_RECENT_ACTIONS, WORKSPACE_RESTORE_PACK_VERSION,
};

fn workspace_restore_pack_active_commitments(restore: &Value) -> Value {
    let mut items = Vec::new();
    if let Some(nodes) = restore["project_task_tree"]["nodes"].as_array() {
        for node in nodes.iter().filter(|item| {
            if item["task_role"].as_str() != Some("active") {
                return false;
            }
            let task_state = item["task_state"].as_str().unwrap_or_default();
            let resume_state = item["resume_state"].as_str().unwrap_or_default();
            !matches!(
                task_state,
                "blocked" | "waiting" | "waiting_external" | "in_review"
            ) && !matches!(
                resume_state,
                "blocked" | "waiting" | "waiting_external" | "in_review"
            )
        }) {
            items.push(json!({
                "task_id": node["task_id"].clone(),
                "headline": node["headline"].clone(),
                "next_step": node["next_step"].clone(),
                "task_state": node["task_state"].clone(),
                "resume_state": node["resume_state"].clone(),
                "source_kind": node["source_kind"].clone(),
            }));
        }
    }
    Value::Array(items)
}

fn workspace_restore_pack_blocked_waiting_items(restore: &Value) -> Value {
    let mut items = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for source in [
        restore["project_task_tree"]["nodes"].as_array(),
        restore["project_task_ledger"]["entries"].as_array(),
    ]
    .into_iter()
    .flatten()
    {
        for item in source {
            let task_state = item["task_state"].as_str().unwrap_or_default();
            let resume_state = item["resume_state"].as_str().unwrap_or_default();
            let is_blocked_or_waiting = matches!(
                task_state,
                "blocked" | "waiting" | "waiting_external" | "in_review"
            ) || matches!(
                resume_state,
                "blocked" | "waiting" | "waiting_external" | "in_review"
            );
            if !is_blocked_or_waiting {
                continue;
            }
            let dedupe_key = item["task_id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .unwrap_or(task_state);
            if !seen_ids.insert(dedupe_key.to_string()) {
                continue;
            }
            items.push(json!({
                "task_id": item["task_id"].clone(),
                "headline": item["headline"].clone(),
                "next_step": item["next_step"].clone(),
                "task_state": item["task_state"].clone(),
                "resume_state": item["resume_state"].clone(),
                "source_kind": item["source_kind"].clone(),
            }));
        }
    }
    Value::Array(items)
}

fn workspace_restore_pack_paused_branches(restore: &Value) -> Value {
    let items = restore["pending_return_queue"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "task_id": item["task_id"].clone(),
                "headline": item["headline"].clone(),
                "next_step": item["next_step"].clone(),
                "queued_reason": item["queued_reason"].clone(),
                "resume_state": item["resume_state"].clone(),
                "queued_at_epoch_ms": item["queued_at_epoch_ms"].clone(),
            })
        })
        .collect::<Vec<_>>();
    Value::Array(items)
}

fn workspace_restore_pack_recently_closed(restore: &Value) -> Value {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(entries) = restore["project_task_ledger"]["entries"].as_array() {
        for entry in entries.iter().filter(|item| {
            matches!(
                item["task_role"].as_str().unwrap_or_default(),
                "historical_handoff"
            ) || matches!(
                item["task_state"].as_str().unwrap_or_default(),
                "done" | "resolved" | "superseded" | "canceled"
            )
        }) {
            let key = entry["task_id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .or_else(|| entry["headline"].as_str())
                .unwrap_or_default();
            if key.is_empty() || !seen.insert(key.to_string()) {
                continue;
            }
            items.push(json!({
                "task_id": entry["task_id"].clone(),
                "headline": entry["headline"].clone(),
                "next_step": entry["next_step"].clone(),
                "task_role": entry["task_role"].clone(),
                "task_state": entry["task_state"].clone(),
                "recorded_at_epoch_ms": entry["recorded_at_epoch_ms"].clone(),
                "source_kind": entry["source_kind"].clone(),
            }));
            if items.len() >= 5 {
                break;
            }
        }
    }
    Value::Array(items)
}

fn workspace_restore_pack_semantic_facts(restore: &Value) -> Value {
    let mut items = Vec::new();
    if let Some(summary) = restore["source_summary"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(json!({
            "fact_kind": "source_summary",
            "summary": summary,
        }));
    }
    if let Some(hypothesis) = restore["current_hypothesis"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(json!({
            "fact_kind": "current_hypothesis",
            "summary": hypothesis,
        }));
    }
    if let Some(notes) = restore["materialized_notes"].as_array() {
        for note in notes.iter().filter_map(Value::as_str).take(6) {
            let trimmed = note.trim();
            if trimmed.is_empty() {
                continue;
            }
            items.push(json!({
                "fact_kind": "materialized_note",
                "summary": trimmed,
            }));
        }
    }
    Value::Array(items)
}

fn workspace_restore_pack_recent_episodic_traces(restore: &Value) -> Value {
    let mut items = Vec::new();
    if let Some(actions) = restore["recent_actions"].as_array() {
        for action in actions.iter().take(5) {
            items.push(json!({
                "trace_kind": "recent_action",
                "headline": action["headline"].clone(),
                "summary": action["summary"].clone(),
                "event_kind": action["event_kind"].clone(),
                "execution_state": action["execution_state"].clone(),
                "recorded_at_epoch_ms": action["recorded_at_epoch_ms"].clone(),
                "authoritative": action["authoritative"].clone(),
            }));
        }
    }
    if let Some(traces) = restore["recent_decision_traces"].as_array() {
        for trace in traces.iter().take(3) {
            items.push(json!({
                "trace_kind": "decision_trace",
                "trace": trace.clone(),
            }));
        }
    }
    Value::Array(items)
}

fn workspace_restore_pack_active_constraints(restore: &Value) -> Value {
    let mut items = Vec::new();
    if restore["execctl_resume_contract"].is_object() {
        items.push(json!({
            "constraint_kind": "execctl_resume_contract",
            "resume_state": restore["execctl_resume_contract"]["resume_state"].clone(),
            "summary": restore["execctl_resume_contract_summary"].clone(),
            "required_return_task": restore["required_return_task"].clone(),
        }));
    }
    if restore["startup_next_action"].is_object() {
        items.push(json!({
            "constraint_kind": "startup_next_action",
            "action_kind": restore["startup_next_action"]["action_kind"].clone(),
            "blocking": restore["startup_next_action"]["blocking"].clone(),
            "summary": restore["startup_next_action_summary"].clone(),
        }));
    }
    if restore["client_budget_guard"].is_object() {
        items.push(json!({
            "constraint_kind": "client_budget_guard",
            "status": restore["client_budget_guard"]["status"].clone(),
            "status_label": restore["client_budget_guard"]["status_label"].clone(),
            "reply_execution_gate": restore["client_budget_guard"]["reply_execution_gate"].clone(),
        }));
    }
    if restore["skill_execution_card"].is_object() {
        items.push(json!({
            "constraint_kind": "procedural_binding",
            "summary": restore["skill_execution_card_summary"].clone(),
            "runtime_constraints": restore["skill_execution_card"]["skill_runtime_constraints"].clone(),
            "model_constraints": restore["skill_execution_card"]["skill_model_constraints"].clone(),
            "tool_constraints": restore["skill_execution_card"]["skill_tool_constraints"].clone(),
            "context_constraints": restore["skill_execution_card"]["skill_context_constraints"].clone(),
        }));
    }
    Value::Array(items)
}

fn workspace_restore_pack_permission_summary(restore: &Value) -> Value {
    let visible_projects = restore["visible_projects"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    json!({
        "project_code": restore["project"]["code"].clone(),
        "namespace_code": restore["namespace"]["code"].clone(),
        "project_visibility_scope": restore["project"]["visibility_scope"].clone(),
        "namespace_retrieval_mode": restore["namespace"]["retrieval_mode"].clone(),
        "agent_scope": restore["agent_scope"].clone(),
        "thread_id": restore["thread_id"].clone(),
        "visible_projects": visible_projects,
        "visible_projects_count": restore["visible_projects"].as_array().map(|items| items.len()).unwrap_or(0),
        "latest_decision_scope": restore["latest_decision_trace"]["scope"].clone(),
        "authoritative_source_kind": restore["state_lineage"]["authoritative_source_kind"].clone(),
    })
}

fn workspace_restore_pack_important_artifacts(restore: &Value) -> Value {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    let mut push_path = |path: &str, artifact_kind: &str, source: &str| {
        let trimmed = path.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            return;
        }
        items.push(json!({
            "artifact_kind": artifact_kind,
            "path": trimmed,
            "source": source,
        }));
    };
    if let Some(path) = restore["state_lineage"]["authoritative_local_path"].as_str() {
        push_path(path, "authoritative_local_path", "state_lineage");
    }
    if let Some(files) = restore["active_files"].as_array() {
        for path in files
            .iter()
            .filter_map(Value::as_str)
            .take(MAX_ACTIVE_FILES)
        {
            push_path(path, "active_file", "active_files");
        }
    }
    if let Some(actions) = restore["recent_actions"].as_array() {
        for path in actions
            .iter()
            .filter_map(|item| item["local_path"].as_str())
            .take(MAX_RECENT_ACTIONS)
        {
            push_path(path, "recent_action_path", "recent_actions");
        }
    }
    Value::Array(items)
}

fn workspace_restore_pack_unresolved_conflicts(restore: &Value) -> Value {
    let mut items = Vec::new();
    if let Some(questions) = restore["open_questions"].as_array() {
        for question in questions
            .iter()
            .filter_map(Value::as_str)
            .take(MAX_OPEN_QUESTIONS)
        {
            let trimmed = question.trim();
            if trimmed.is_empty() {
                continue;
            }
            items.push(json!({
                "conflict_kind": "open_question",
                "summary": trimmed,
            }));
        }
    }
    if let Some(rejected) = restore["rejected_hypotheses"].as_array() {
        for hypothesis in rejected.iter().filter_map(Value::as_str).take(4) {
            let trimmed = hypothesis.trim();
            if trimmed.is_empty() {
                continue;
            }
            items.push(json!({
                "conflict_kind": "rejected_hypothesis",
                "summary": trimmed,
            }));
        }
    }
    if let Some(summary) = restore["excluded_reasons_summary"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        items.push(json!({
            "conflict_kind": "excluded_reasons",
            "summary": summary,
        }));
    }
    Value::Array(items)
}

fn workspace_restore_pack_relevant_procedures(restore: &Value) -> Value {
    if !restore["skill_execution_card"].is_object() {
        return Value::Array(Vec::new());
    }
    Value::Array(vec![json!({
        "procedure_kind": "compact_execution_card",
        "raw_procedural_archive_included": false,
        "summary": restore["skill_execution_card_summary"].clone(),
        "card": restore["skill_execution_card"].clone(),
        "binding": restore["skill_execution_card_binding"].clone(),
    })])
}

fn summarize_workspace_restore_bucket(label: &str, value: &Value) -> Option<String> {
    let count = value.as_array().map(|items| items.len()).unwrap_or(0);
    (count > 0).then(|| format!("{label}({count})"))
}

pub(crate) fn build_workspace_restore_pack(restore: &Value) -> Value {
    let active_commitments = workspace_restore_pack_active_commitments(restore);
    let blocked_waiting_items = workspace_restore_pack_blocked_waiting_items(restore);
    let paused_branches = workspace_restore_pack_paused_branches(restore);
    let recently_closed = workspace_restore_pack_recently_closed(restore);
    let relevant_semantic_facts = workspace_restore_pack_semantic_facts(restore);
    let recent_episodic_traces = workspace_restore_pack_recent_episodic_traces(restore);
    let active_constraints = workspace_restore_pack_active_constraints(restore);
    let permission_summary = workspace_restore_pack_permission_summary(restore);
    let important_artifacts = workspace_restore_pack_important_artifacts(restore);
    let unresolved_conflicts = workspace_restore_pack_unresolved_conflicts(restore);
    let relevant_procedures = workspace_restore_pack_relevant_procedures(restore);
    let summary = [
        summarize_workspace_restore_bucket("active", &active_commitments),
        summarize_workspace_restore_bucket("blocked", &blocked_waiting_items),
        summarize_workspace_restore_bucket("paused", &paused_branches),
        summarize_workspace_restore_bucket("facts", &relevant_semantic_facts),
        summarize_workspace_restore_bucket("constraints", &active_constraints),
        summarize_workspace_restore_bucket("artifacts", &important_artifacts),
        summarize_workspace_restore_bucket("procedures", &relevant_procedures),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ");
    json!({
        "pack_version": WORKSPACE_RESTORE_PACK_VERSION,
        "pack_kind": "workspace_restore_pack",
        "current_goal": restore["current_goal"].clone(),
        "next_step": restore["next_step"].clone(),
        "restore_confidence": restore["restore_confidence"].clone(),
        "restore_freshness_state": restore["restore_freshness_state"].clone(),
        "active_commitments": active_commitments,
        "blocked_waiting_items": blocked_waiting_items,
        "paused_branches": paused_branches,
        "recently_closed": recently_closed,
        "relevant_semantic_facts": relevant_semantic_facts,
        "recent_episodic_traces": recent_episodic_traces,
        "active_constraints": active_constraints,
        "permission_summary": permission_summary,
        "important_artifacts": important_artifacts,
        "unresolved_conflicts": unresolved_conflicts,
        "relevant_procedures": relevant_procedures,
        "procedural_restore_policy": {
            "raw_procedural_archive_forbidden": true,
            "materialized_surface": if restore["skill_execution_card"].is_object() {
                "compact_execution_card"
            } else {
                "none"
            },
        },
        "summary": if summary.is_empty() { Value::Null } else { Value::String(summary) },
    })
}

pub(super) fn overlay_workspace_restore_pack(bundle: &mut Value) {
    let Some(restore) = bundle.get("working_state_restore") else {
        return;
    };
    let pack = build_workspace_restore_pack(restore);
    let summary = pack["summary"].clone();
    bundle["workspace_restore_pack"] = pack.clone();
    if let Some(node) = bundle
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    {
        node.insert("workspace_restore_pack".to_string(), pack);
        node.insert("workspace_restore_pack_summary".to_string(), summary);
    }
}

pub(crate) fn ensure_runtime_workspace_restore_pack(bundle: &mut Value) {
    overlay_workspace_restore_pack(bundle);
}
