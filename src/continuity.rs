use crate::chat_question;
use crate::cli::{
    ContinuityAnswerArgs, ContinuityClientBudgetTargetArgs, ContinuityCompactChatArgs,
    ContinuityHandoffArgs, ContinuityImportArgs, ContinuityRotateChatArgs, ContinuityStartupArgs,
    ContinuityStartupStateArgs, ContinuityThreadIndexEnrichArgs, VerifyContinuityArgs,
};
use crate::codex_threads;
use crate::config::AppConfig;
use crate::dashboard::client_turn_pressure_display_status_label;
use crate::eval_verdict::{self, EvalPattern, EvalSignals};
use crate::onboarding;
use crate::postgres::{self, ChunkRecord, DocumentRecord, NamespaceRecord, ProjectRecord};
use crate::retrieval_science;
use crate::s3;
use crate::token_budget;
use crate::working_state;
use crate::workspace_graph;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;
use uuid::Uuid;

mod continuity_compact_chat_helpers;
mod continuity_profile;
mod continuity_startup_runtime_state;
mod continuity_types;

pub(crate) use self::continuity_compact_chat_helpers::maybe_launch_compact_chat_host;
use self::continuity_compact_chat_helpers::*;
use self::continuity_profile::*;
use self::continuity_startup_runtime_state::*;
pub(crate) use self::continuity_startup_runtime_state::{
    inspect_startup_runtime_state, print_startup_runtime_state,
};
use self::continuity_types::*;

const MAX_SEARCHABLE_CONTINUITY_BYTES: usize = 12_000;
const COMPACT_CHAT_PROMPT_ARTIFACT_RELATIVE_PATH: &str = ".amai/continuity/compact-chat-prompt.txt";
const MAX_COMPACT_CHAT_PENDING_RETURN_QUEUE_PREVIEW: usize = 3;

pub(crate) fn compact_chat_manual_fallback_steps(client_surface: &Value) -> Vec<String> {
    continuity_compact_chat_helpers::compact_chat_manual_fallback_steps(client_surface)
}

pub(crate) fn compact_chat_clean_launch_surface(
    client_surface: &Value,
    repo_root: &Path,
    prompt_path: Option<&Path>,
) -> Value {
    continuity_compact_chat_helpers::build_compact_chat_clean_launch_surface(
        client_surface,
        repo_root,
        prompt_path,
    )
}
const CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_KIND: &str =
    "continuity_handoff_document_index_refresh";
// Handoff truth is captured immediately in observability/restore state, so the
// searchable document index can refresh on a slower cadence.
const CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_MIN_INTERVAL_MS: u64 = 300_000;

async fn connect_bootstrapped_admin(cfg: &AppConfig) -> Result<Client> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    Ok(db)
}

pub(crate) async fn handoff_payload_from_parts_with_db(
    db: &mut Client,
    cfg: &AppConfig,
    project_code: &str,
    namespace_code: &str,
    headline: &str,
    next_step: &str,
    details: &str,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
) -> Result<Value> {
    let project = postgres::get_project_by_code(db, project_code).await?;
    let namespace = postgres::find_namespace_by_code(db, project.project_id, namespace_code)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", namespace_code))?;
    let previous_restore =
        working_state::load_recent_restore_bundle_without_live_guard(db, &project, &namespace)
            .await?;
    capture_handoff_payload(
        cfg,
        db,
        &project,
        &namespace,
        previous_restore.as_ref(),
        headline,
        next_step,
        details,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
    )
    .await
}

pub(crate) fn allowed_client_budget_target_values() -> Vec<u64> {
    (0..=working_state::MAX_CLIENT_BUDGET_TARGET_PERCENT)
        .step_by(working_state::CLIENT_BUDGET_TARGET_STEP_PERCENT as usize)
        .collect()
}

pub(crate) const CLIENT_BUDGET_TARGET_CHAT_COMMAND_PREFIX: &str = "экономия_";
pub(crate) const CLIENT_BUDGET_COMPACT_CHAT_COMMAND: &str = "компакт_чат";
pub(crate) fn client_budget_target_chat_command(target_percent: u64) -> String {
    format!("{CLIENT_BUDGET_TARGET_CHAT_COMMAND_PREFIX}{target_percent}%")
}

pub(crate) fn client_budget_target_chat_command_pattern() -> String {
    let values = allowed_client_budget_target_values()
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("|");
    format!("^{}({values})%$", CLIENT_BUDGET_TARGET_CHAT_COMMAND_PREFIX)
}

fn compact_project_task_tree_for_startup(tree: &Value) -> Value {
    let Some(tree_object) = tree.as_object() else {
        return Value::Null;
    };
    let nodes = tree["nodes"].as_array().cloned().unwrap_or_default();
    let edges = tree["edges"].as_array().cloned().unwrap_or_default();
    json!({
        "open_tasks_count": tree["open_tasks_count"].clone(),
        "pending_return_count": tree["pending_return_count"].clone(),
        "nodes_total": nodes.len(),
        "edges_total": edges.len(),
        "summary_only": true,
        "full_shape_preserved_in_working_state_restore": tree_object.contains_key("nodes")
            && tree_object.contains_key("edges"),
    })
}

fn format_optional_u64_for_human(value: Option<u64>) -> String {
    value
        .map(|count| count.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_u64_for_prompt(value: Option<u64>) -> String {
    value
        .map(|count| count.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn format_optional_text_for_human(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ещё нет данных")
        .to_string()
}

fn optional_non_empty_text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalized_handoff_summary_json(headline: Option<&str>, next_step: Option<&str>) -> Value {
    json!({
        "headline": format_optional_text_for_human(headline),
        "next_step": normalize_next_step_value(next_step.unwrap_or_default())
            .unwrap_or_else(|| "ещё нет данных".to_string()),
    })
}

fn normalized_thread_summary_snapshot_fields(
    summary_headline: Option<&str>,
    summary_next_step: Option<&str>,
) -> Value {
    json!({
        "summary_headline": optional_non_empty_text(summary_headline),
        "summary_next_step": optional_non_empty_text(summary_next_step),
    })
}

fn normalized_optional_text_json(value: Option<&str>) -> Value {
    optional_non_empty_text(value)
        .map(|value| json!(value))
        .unwrap_or(Value::Null)
}

fn normalized_working_state_restore_projection(value: Option<&Value>) -> Value {
    let Some(node) = value else {
        return Value::Null;
    };
    let mut normalized = node.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.insert(
            "current_goal".to_string(),
            normalized_optional_text_json(node["current_goal"].as_str()),
        );
        object.insert(
            "next_step".to_string(),
            normalized_optional_text_json(node["next_step"].as_str()),
        );
        if let Some(lineage) = object
            .get_mut("state_lineage")
            .and_then(serde_json::Value::as_object_mut)
        {
            lineage.insert(
                "authoritative_event_id".to_string(),
                normalized_optional_text_json(
                    node["state_lineage"]["authoritative_event_id"].as_str(),
                ),
            );
        }
    }
    normalized
}

fn normalized_workspace_restore_pack_projection(value: Option<&Value>) -> Value {
    let Some(node) = value else {
        return Value::Null;
    };
    let mut normalized = node.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.insert(
            "summary".to_string(),
            normalized_optional_text_json(node["summary"].as_str()),
        );
    }
    normalized
}

fn normalized_restore_confidence_value(working_state_restore: Option<&Value>) -> Value {
    working_state_restore
        .map(|node| node["restore_confidence"].clone())
        .unwrap_or(Value::Null)
}

fn normalized_optional_char_count_json(value: Option<&str>) -> Value {
    optional_non_empty_text(value)
        .map(|text| json!(text.chars().count()))
        .unwrap_or(Value::Null)
}

fn format_startup_surface_label(path: &str, mode: Option<&str>) -> String {
    let mode = optional_non_empty_text(mode).unwrap_or("ещё нет данных");
    format!("Startup surface: {path} ({mode})")
}

fn format_compact_restore_prompt_text(prompt_text: Option<&str>) -> &str {
    optional_non_empty_text(prompt_text).unwrap_or("ещё нет данных")
}

fn format_project_scope_for_human(display_name: Option<&str>, code: Option<&str>) -> String {
    format!(
        "{} ({})",
        format_optional_text_for_human(display_name),
        format_optional_text_for_human(code)
    )
}

fn format_client_budget_status_label_for_human(guard: &Value) -> String {
    display_client_budget_status_label(guard)
        .map(|value| value.into_owned())
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn append_working_state_warning_to_message(base_message: &str, write_status: &Value) -> String {
    let warning = write_status["warning"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match warning {
        Some(warning) => format!("{base_message} {warning}"),
        None => base_message.to_string(),
    }
}

fn format_reply_prefix_for_human(reply_prefix: Option<&str>) -> String {
    optional_non_empty_text(reply_prefix).unwrap_or("ещё нет данных").to_string()
}

fn format_optional_text_for_prompt(value: Option<&str>) -> String {
    optional_non_empty_text(value).unwrap_or("?").to_string()
}

fn recommended_workline_headline_from_restore_or_handoff(
    restore_node: Option<&Value>,
    handoff_summary: &Value,
) -> String {
    restore_node
        .and_then(|value| value["current_goal"].as_str())
        .filter(|value| is_meaningful_restore_value(value))
        .or_else(|| {
            handoff_summary["headline"]
                .as_str()
                .filter(|value| is_meaningful_restore_value(value))
        })
        .unwrap_or("ещё нет данных")
        .to_string()
}

fn recommended_workline_next_step_from_restore_or_handoff(
    restore_node: Option<&Value>,
    handoff_summary: &Value,
) -> String {
    restore_node
        .and_then(|value| value["next_step"].as_str())
        .and_then(normalize_next_step_value)
        .filter(|value| is_meaningful_restore_value(value))
        .or_else(|| {
            handoff_summary["next_step"]
                .as_str()
                .and_then(normalize_next_step_value)
                .filter(|value| is_meaningful_restore_value(value))
        })
        .unwrap_or_else(|| "ещё нет данных".to_string())
        .to_string()
}

fn compact_chat_runtime_scope_fields(
    summary: &Value,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let project_code = optional_non_empty_text(summary["project_code"].as_str()).map(str::to_string);
    let namespace_code =
        optional_non_empty_text(summary["namespace_code"].as_str()).map(str::to_string);
    let project_display_name = project_code.clone();
    let namespace_display_name = namespace_code.clone();
    (
        project_code,
        namespace_code,
        project_display_name,
        namespace_display_name,
    )
}

fn compact_project_task_ledger_for_startup(ledger: &Value) -> Value {
    let Some(ledger_object) = ledger.as_object() else {
        return Value::Null;
    };
    let entries = ledger["entries"].as_array().cloned().unwrap_or_default();
    let active_entries = entries
        .iter()
        .filter(|entry| entry["task_role"].as_str() == Some("active"))
        .count();
    let pending_return_entries = entries
        .iter()
        .filter(|entry| entry["task_role"].as_str() == Some("pending_return"))
        .count();
    json!({
        "open_tasks_count": ledger["open_tasks_count"].clone(),
        "historical_handoffs_count": ledger["historical_handoffs_count"].clone(),
        "entries_count": entries.len(),
        "active_entries_count": active_entries,
        "pending_return_entries_count": pending_return_entries,
        "summary_only": true,
        "full_shape_preserved_in_working_state_restore": ledger_object.contains_key("entries"),
    })
}

fn compact_pending_return_queue_for_compact_chat(queue: &Value) -> (Value, usize, bool) {
    let Some(items) = queue.as_array() else {
        return (Value::Null, 0, false);
    };
    let preview = items
        .iter()
        .take(MAX_COMPACT_CHAT_PENDING_RETURN_QUEUE_PREVIEW)
        .map(|item| {
            let mut compact = serde_json::Map::new();
            copy_if_present(
                &mut compact,
                item,
                &["task_id", "headline", "next_step", "resume_state"],
            );
            Value::Object(compact)
        })
        .collect::<Vec<_>>();
    (
        Value::Array(preview),
        items.len(),
        items.len() <= MAX_COMPACT_CHAT_PENDING_RETURN_QUEUE_PREVIEW,
    )
}

fn compact_workspace_restore_pack_for_startup(pack: &Value) -> Value {
    let Some(pack_object) = pack.as_object() else {
        return Value::Null;
    };
    let count = |field: &str| {
        pack[field]
            .as_array()
            .map(|items| items.len())
            .unwrap_or_default()
    };
    json!({
        "pack_version": pack["pack_version"].clone(),
        "active_commitments_count": count("active_commitments"),
        "blocked_waiting_items_count": count("blocked_waiting_items"),
        "paused_branches_count": count("paused_branches"),
        "recently_closed_count": count("recently_closed"),
        "relevant_semantic_facts_count": count("relevant_semantic_facts"),
        "recent_episodic_traces_count": count("recent_episodic_traces"),
        "active_constraints_count": count("active_constraints"),
        "important_artifacts_count": count("important_artifacts"),
        "unresolved_conflicts_count": count("unresolved_conflicts"),
        "relevant_procedures_count": count("relevant_procedures"),
        "summary": pack["summary"].clone(),
        "procedural_restore_policy": pack["procedural_restore_policy"].clone(),
        "summary_only": true,
        "full_shape_preserved_in_working_state_restore": pack_object.contains_key("active_commitments")
            && pack_object.contains_key("active_constraints")
            && pack_object.contains_key("important_artifacts"),
    })
}

fn compact_client_surface_for_compact_chat(client_surface: &Value) -> Value {
    if !client_surface.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        client_surface,
        &[
            "client_key",
            "display_name",
            "startup_instruction_path",
            "startup_instruction_mode",
            "fresh_chat_assist_summary",
            "delivery_surface_assist_summary",
            "reconnect_shell_command",
            "reconnect_bootstrap_command",
        ],
    );
    Value::Object(compact)
}

pub(crate) fn compact_working_state_restore_for_startup_output(restore: &Value) -> Value {
    if !restore.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    compact.insert("summary_only".to_string(), Value::Bool(true));
    copy_if_present(
        &mut compact,
        restore,
        &[
            "thread_id",
            "session_id",
            "agent_scope",
            "captured_at_epoch_ms",
            "restore_confidence",
            "restore_freshness_state",
            "is_preliminary",
            "current_goal",
            "next_step",
            "next_step_state",
            "current_hypothesis",
            "current_focus",
            "last_results_summary",
            "source_summary",
            "client_budget_target_percent",
            "skill_execution_card_summary",
        ],
    );
    if restore["skill_execution_card"].is_object() {
        compact.insert(
            "skill_execution_card".to_string(),
            restore["skill_execution_card"].clone(),
        );
    }
    if let Some(authoritative_event_id) =
        restore["state_lineage"]["authoritative_event_id"].as_str()
    {
        if !authoritative_event_id.is_empty() {
            compact.insert(
                "state_lineage".to_string(),
                json!({
                    "authoritative_event_id": authoritative_event_id,
                }),
            );
        }
    }
    Value::Object(compact)
}

pub(crate) fn compact_chat_start_restore_for_startup_output(restore: &Value) -> Value {
    if !restore.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    compact.insert("summary_only".to_string(), Value::Bool(true));
    copy_if_present(&mut compact, restore, &["prompt_text"]);
    Value::Object(compact)
}

pub(crate) fn compact_continuity_startup_public_payload(payload: &Value) -> Value {
    let mut compact = payload.clone();
    let Some(root) = compact.as_object_mut() else {
        return compact;
    };
    if root
        .get("continuity_startup")
        .is_some_and(|value| value.is_object())
    {
        let mut startup = serde_json::Map::new();
        startup.insert("summary_only".to_string(), Value::Bool(true));
        copy_if_present(
            &mut startup,
            &payload["continuity_startup"],
            &[
                "project",
                "namespace",
                "imported_at_epoch_ms",
                "documents_imported",
                "rendered_transcript_files",
                "continuity_source",
                "handoff_summary",
                "canonical_eval",
            ],
        );
        root.insert("continuity_startup".to_string(), Value::Object(startup));
    }
    if root
        .get("chat_start_restore")
        .is_some_and(|value| value.is_object())
    {
        root.insert(
            "chat_start_restore".to_string(),
            compact_chat_start_restore_for_startup_output(&payload["chat_start_restore"]),
        );
    }
    if root
        .get("working_state_restore")
        .is_some_and(|value| value.is_object())
    {
        root.insert(
            "working_state_restore".to_string(),
            compact_working_state_restore_for_startup_output(&payload["working_state_restore"]),
        );
    }
    root.remove("degradation_policy");
    compact
}

pub async fn import_sources(cfg: &AppConfig, args: &ContinuityImportArgs) -> Result<()> {
    let payload = import_sources_payload(cfg, args).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub(crate) async fn import_sources_payload(
    cfg: &AppConfig,
    args: &ContinuityImportArgs,
) -> Result<Value> {
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
        "default",
        "project_shared",
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
                    "title": normalized_optional_text_json(summary["title"].as_str()),
                    "cwd": summary["cwd"].clone(),
                    "first_user_message": normalized_optional_text_json(summary["first_user_message"].as_str()),
                    "started_at": summary["started_at"].clone(),
                    "ended_at": summary["ended_at"].clone(),
                    "messages_count": summary["messages_count"].clone(),
                    "last_user_message": normalized_optional_text_json(summary["last_user_message"].as_str()),
                    "last_assistant_message": normalized_optional_text_json(summary["last_assistant_message"].as_str()),
                    "summary_headline": optional_non_empty_text(summary["summary_headline"].as_str()),
                    "summary_next_step": optional_non_empty_text(summary["summary_next_step"].as_str()),
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
                source_kind: Some(&source.source_kind),
                source_event_ids: None,
                message_refs: None,
                evidence_span: Some(&json!({
                    "original_path": source.original_path.display().to_string(),
                    "relative_path": source.relative_path,
                    "source_kind": source.source_kind,
                })),
                derivation_kind: Some("extract"),
                schema_version: Some("artifact-ref-envelope-v1"),
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
    let normalized_active_summary = normalized_handoff_summary_json(
        active_summary["headline"].as_str(),
        active_summary["next_step"].as_str(),
    );
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
                    .map(|path| path.display().to_string()),
                "details": normalized_active_summary,
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
        let normalized_summary_fields = normalized_thread_summary_snapshot_fields(
            summary
                .as_ref()
                .and_then(|value| value["summary_headline"].as_str())
                .or_else(|| optional_non_empty_text(Some(&entry.summary_headline))),
            summary
                .as_ref()
                .and_then(|value| value["summary_next_step"].as_str())
                .or_else(|| optional_non_empty_text(Some(&entry.summary_next_step))),
        );
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
                "title": normalized_optional_text_json(Some(&entry.title)),
                "cwd": json!(entry.cwd),
                "first_user_message": normalized_optional_text_json(Some(&entry.first_user_message)),
                "started_at": summary.as_ref().map(|value| value["started_at"].clone()).unwrap_or_else(|| json!(entry.started_at)),
                "ended_at": summary.as_ref().map(|value| value["ended_at"].clone()).unwrap_or_else(|| json!(entry.ended_at)),
                "messages_count": summary.as_ref().map(|value| value["messages_count"].clone()).unwrap_or_else(|| json!(entry.messages_count)),
                "last_user_message": summary
                    .as_ref()
                    .map(|value| value["last_user_message"].clone())
                    .unwrap_or_else(|| normalized_optional_text_json(Some(&entry.last_user_message))),
                "last_assistant_message": summary
                    .as_ref()
                    .map(|value| value["last_assistant_message"].clone())
                    .unwrap_or_else(|| normalized_optional_text_json(Some(&entry.last_assistant_message))),
                "summary_headline": normalized_summary_fields["summary_headline"].clone(),
                "summary_next_step": normalized_summary_fields["summary_next_step"].clone(),
                "time_slices": summary.as_ref().map(|value| value["time_slices"].clone()).unwrap_or_else(|| json!(entry.time_slices)),
                "rendered_transcript": normalized_optional_text_json(Some(&entry.rendered_transcript)),
                "source_rollout": normalized_optional_text_json(Some(&entry.source_rollout)),
                "raw_rollout": normalized_optional_text_json(Some(&entry.raw_mirror)),
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
    if args.runtime_state_json {
        let _ = startup_payload_with_context(&db, &context, args).await?;
        let repo_root = canonical_path(Path::new(&context.project.repo_root))?;
        let audit = inspect_startup_runtime_state(&repo_root)?;
        let artifact_payload = load_startup_runtime_state_artifact(&repo_root)?;
        println!(
            "{}",
            serde_json::to_string(&startup_runtime_state_audit_json(
                &audit,
                artifact_payload.as_ref(),
            ))?
        );
        return Ok(());
    }
    if args.json {
        let payload = startup_payload_with_context(&db, &context, args).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&compact_continuity_startup_public_payload(&payload))?
        );
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
    let startup_payload =
        build_continuity_startup_payload(&context, context.restore.as_ref(), &chat_start_restore)?;
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
    let continuity_source_mode = context.continuity["continuity_source_mode"].as_str();
    let continuity_source_namespace_code = context.continuity["continuity_source_namespace_code"]
        .as_str()
        .filter(|_| continuity_source_mode.is_some());
    let continuity_source_summary = match continuity_source_mode {
        Some("continuity_namespace_fallback_import") => format!(
            "fallback import из namespace {}",
            continuity_source_namespace_code.unwrap_or("ещё нет данных")
        ),
        Some("working_state_fallback") => {
            "runtime fallback из working_state + handoff".to_string()
        }
        Some(_) => format!(
            "scoped import из namespace {}",
            continuity_source_namespace_code.unwrap_or("ещё нет данных")
        ),
        None => "ещё нет данных".to_string(),
    };
    println!("Continuity source: {continuity_source_summary}");
    println!(
        "Последний импорт continuity: {}",
        human_epoch_ms(context.continuity["imported_at_epoch_ms"].as_u64())
    );
    println!(
        "Импортировано документов: {}",
        format_optional_u64_for_human(context.continuity["documents_imported"].as_u64())
    );
    let continuity_snapshot_path =
        format_optional_text_for_human(context.continuity["bootstrap_summary"]["bootstrap_file"].as_str());
    println!("Continuity snapshot: {continuity_snapshot_path}");
    let bridge_files = context.continuity["session_memory_files"].as_u64();
    match bridge_files {
        Some(count) if count > 0 => println!("Дополнительные bridge-notes: {}", count),
        None => println!("Дополнительные bridge-notes: ещё нет данных"),
        _ => {}
    }
    println!(
        "Rendered transcripts: {}",
        format_optional_u64_for_human(context.continuity["rendered_transcript_files"].as_u64())
    );
    println!();
    let startup_next_step = context.handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    println!("Текущая активная линия:");
    println!(
        "- {}",
        format_optional_text_for_human(context.handoff_summary["headline"].as_str())
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
        format_optional_u64_for_human(
            context.continuity["bootstrap_summary"]["details"]["thread_count"].as_u64(),
        )
    );
    println!(
        "- Последний rendered transcript: {}",
        format_optional_text_for_human(
            context.continuity["bootstrap_summary"]["details"]["latest_rendered_transcript"]
                .as_str()
        )
    );
    println!();
    println!("Как использовать дальше:");
    println!(
        "- Для project-scoped retrieval: ./scripts/amai_exec.sh context pack --project {} --namespace {} --query 'ваш вопрос'",
        context.project.code, context.namespace.code
    );
    if let Some(bootstrap_file) = context.continuity["bootstrap_summary"]["bootstrap_file"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        let mut import_command = format!(
            "./scripts/amai_exec.sh continuity import --project {} --display-name '{}' --repo-root {} --namespace {} --bootstrap-file {}",
            context.project.code,
            context.project.display_name.replace('\'', "\\'"),
            shell_quote(&context.project.repo_root),
            context.namespace.code,
            shell_quote(bootstrap_file),
        );
        let active_workline_arg = context.continuity["active_workline_summary"]
            ["active_workline_file"]
            .as_str()
            .and_then(|value| optional_non_empty_text(Some(value)));
        if let Some(active_workline_arg) = active_workline_arg {
            import_command.push_str(" --active-workline-file ");
            import_command.push_str(&shell_quote(active_workline_arg));
        }
        println!("- Для обновления continuity после новых изменений: {import_command}");
    } else {
        println!(
            "- Для materialize continuity import сначала дай bootstrap/active workline source; текущий startup уже восстановлен из runtime fallback."
        );
    }
    Ok(())
}

pub async fn startup_payload(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<Value> {
    let db = connect_bootstrapped_admin(cfg).await?;
    let context = load_startup_context(&db, args).await?;
    startup_payload_with_context(&db, &context, args).await
}

fn prepare_continuity_restore_observed_resources(
    context: &ContinuityStartupContext,
) -> Result<ContinuityRestoreObservedResources> {
    prepare_continuity_restore_observed_resources_for_repo_root(Path::new(
        &context.project.repo_root,
    ))
}

fn prepare_continuity_restore_observed_resources_for_repo_root(
    repo_root: &Path,
) -> Result<ContinuityRestoreObservedResources> {
    let repo_root = repo_root.to_path_buf();
    let token_budget_config = token_budget::continuity_restore_observed_config(&repo_root)?;
    let tokenizer_name = token_budget_config.measurement.tokenizer.clone();
    let tokenizer_prewarm = tokio::task::spawn_blocking(move || {
        token_budget::prewarm_shared_tokenizer(&tokenizer_name)
    });
    Ok(ContinuityRestoreObservedResources {
        repo_root,
        token_budget_config,
        tokenizer_prewarm,
    })
}

async fn startup_payload_with_context(
    db: &Client,
    context: &ContinuityStartupContext,
    args: &ContinuityStartupArgs,
) -> Result<Value> {
    startup_payload_with_context_and_resources(db, context, args, None).await
}

async fn startup_payload_with_context_and_resources(
    db: &Client,
    context: &ContinuityStartupContext,
    args: &ContinuityStartupArgs,
    resources: Option<ContinuityRestoreObservedResources>,
) -> Result<Value> {
    let total_started = Instant::now();
    let ContinuityRestoreObservedResources {
        repo_root,
        token_budget_config,
        tokenizer_prewarm,
    } = match resources {
        Some(resources) => resources,
        None => prepare_continuity_restore_observed_resources(context)?,
    };
    let step_started = Instant::now();
    let refreshed_restore = working_state::refresh_same_thread_execctl_active_lease_for_startup(
        db,
        &context.project,
        &context.namespace,
        context.restore.as_ref(),
    )
    .await?;
    continuity_profile_log(
        "startup_payload_with_context.refresh_same_thread_execctl_active_lease_for_startup",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    let step_started = Instant::now();
    let chat_start_restore = build_chat_start_restore(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        refreshed_restore.as_ref(),
    );
    continuity_profile_log(
        "startup_payload_with_context.build_chat_start_restore",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    let step_started = Instant::now();
    tokenizer_prewarm
        .await
        .context("failed to join continuity restore tokenizer prewarm task")??;
    token_budget::record_continuity_restore_observed_event_with_config(
        db,
        &context.project.code,
        &context.namespace.code,
        chat_start_restore["chat_start_restore"]["prompt_text"]
            .as_str()
            .unwrap_or_default(),
        &args.token_source_kind,
        &repo_root,
        &token_budget_config,
    )
    .await?;
    continuity_profile_log(
        "startup_payload_with_context.record_continuity_restore_observed_event",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    let step_started = Instant::now();
    let payload =
        build_continuity_startup_payload(context, refreshed_restore.as_ref(), &chat_start_restore)?;
    continuity_profile_log(
        "startup_payload_with_context.build_continuity_startup_payload",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    let step_started = Instant::now();
    persist_startup_runtime_state_artifact(Path::new(&context.project.repo_root), &payload)?;
    continuity_profile_log(
        "startup_payload_with_context.persist_startup_runtime_state_artifact",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    continuity_profile_log(
        "startup_payload_with_context.total",
        total_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    Ok(payload)
}

fn copy_if_present(target: &mut serde_json::Map<String, Value>, source: &Value, fields: &[&str]) {
    for field in fields {
        let value = &source[*field];
        if !value.is_null() {
            target.insert((*field).to_string(), value.clone());
        }
    }
}

fn compact_startup_runtime_client_budget_guard(guard: &Value) -> Value {
    if !guard.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        guard,
        &["status_label", "client_budget_target_percent"],
    );
    let reply_execution_gate =
        compact_startup_runtime_reply_execution_gate(&guard["reply_execution_gate"]);
    if !reply_execution_gate.is_null() {
        compact.insert("reply_execution_gate".to_string(), reply_execution_gate);
    }
    Value::Object(compact)
}

fn compact_startup_state_cli_client_budget_guard(guard: &Value) -> Value {
    if !guard.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        guard,
        &["status_label", "client_budget_target_percent"],
    );
    Value::Object(compact)
}

fn compact_startup_state_cli_reply_execution_gate(reply_execution_gate: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        reply_execution_gate,
        &[
            "action_kind",
            "blocking",
            "must_rotate_before_reply",
            "must_wait_for_budget_recovery_before_reply",
            "reply_budget_mode",
            "reply_prefix",
        ],
    );
    Value::Object(compact)
}

fn compact_startup_runtime_startup_next_action(action: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        action,
        &[
            "action_version",
            "action_kind",
            "blocking",
            "reason",
            "resume_state",
            "no_silent_drop",
            "headline",
            "next_step",
            "client_budget_status_label",
            "preserves_return_obligation",
            "required_task_set",
            "required_task_set_summary",
        ],
    );
    let action_bundle = compact_startup_runtime_startup_action_bundle(&action["action_bundle"]);
    if !action_bundle.is_null() {
        compact.insert("action_bundle".to_string(), action_bundle);
    }
    Value::Object(compact)
}

fn compact_startup_runtime_summary_startup_execution_gate(gate: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        gate,
        &[
            "gate_version",
            "action_kind",
            "blocking",
            "resume_state",
            "required_return_task_present",
            "required_task_set_count",
            "required_task_set_present",
            "must_preserve_required_task_set",
            "lease_owner_state",
            "must_follow_startup_next_action",
            "unrelated_work_allowed",
            "must_read_prompt_text_before_reply",
            "required_action_kind_when_resume_required",
            "no_silent_drop",
        ],
    );
    Value::Object(compact)
}

fn compact_startup_state_cli_startup_execution_gate(gate: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        gate,
        &[
            "action_kind",
            "blocking",
            "required_task_set_count",
            "required_task_set_present",
            "must_preserve_required_task_set",
            "must_follow_startup_next_action",
            "unrelated_work_allowed",
            "must_read_prompt_text_before_reply",
            "required_action_kind_when_resume_required",
            "no_silent_drop",
        ],
    );
    Value::Object(compact)
}

fn compact_startup_runtime_required_return_task(task: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        task,
        &[
            "headline",
            "next_step",
            "resume_state",
            "task_id",
            "task_role",
            "task_state",
        ],
    );
    Value::Object(compact)
}

fn compact_startup_runtime_execctl_active_lease(lease: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        lease,
        &[
            "lease_version",
            "lease_owner_state",
            "lease_state",
            "headline",
            "next_step",
            "storage_lane",
        ],
    );
    Value::Object(compact)
}

fn compact_startup_runtime_execctl_resume_obligation(obligation: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        obligation,
        &[
            "resume_state",
            "no_silent_drop",
            "pending_return_count",
            "required_return_headline",
            "required_return_next_step",
            "required_task_set_count",
            "required_task_set",
            "required_task_set_summary",
        ],
    );
    Value::Object(compact)
}

fn compact_compact_chat_chat_start_restore(node: &Value) -> Value {
    if !node.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    let (pending_return_queue, pending_return_queue_total, pending_queue_full_shape_preserved) =
        compact_pending_return_queue_for_compact_chat(&node["pending_return_queue"]);
    copy_if_present(
        &mut compact,
        node,
        &[
            "headline",
            "next_step",
            "restore_confidence",
            "prompt_text",
            "execctl_resume_state",
            "pending_return_summary",
            "project_task_tree_summary",
            "project_task_ledger_summary",
            "required_task_set",
            "required_task_set_summary",
        ],
    );
    if !pending_return_queue.is_null() {
        compact.insert("pending_return_queue".to_string(), pending_return_queue);
        compact.insert(
            "pending_return_queue_total".to_string(),
            json!(pending_return_queue_total),
        );
        compact.insert(
            "pending_return_queue_full_shape_preserved_in_working_state_restore".to_string(),
            json!(pending_queue_full_shape_preserved),
        );
    }
    let compact_tree = compact_project_task_tree_for_startup(&node["project_task_tree"]);
    if !compact_tree.is_null() {
        compact.insert("project_task_tree".to_string(), compact_tree);
    }
    let compact_ledger = compact_project_task_ledger_for_startup(&node["project_task_ledger"]);
    if !compact_ledger.is_null() {
        compact.insert("project_task_ledger".to_string(), compact_ledger);
    }
    Value::Object(compact)
}

fn compact_compact_chat_host_current_thread_control(surface: &Value) -> Value {
    if !surface.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        surface,
        &["command_id", "button_label", "control_kind", "summary"],
    );
    if surface["external_uri_launch"].is_object() {
        let mut external = serde_json::Map::new();
        copy_if_present(
            &mut external,
            &surface["external_uri_launch"],
            &["uri", "platform_launch_command"],
        );
        if !external.is_empty() {
            compact.insert("external_uri_launch".to_string(), Value::Object(external));
        }
    }
    Value::Object(compact)
}

fn compact_compact_chat_handoff(handoff: &Value) -> Value {
    if !handoff.is_object() {
        return Value::Null;
    }
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        handoff,
        &["headline", "next_step", "local_path", "relative_path"],
    );
    Value::Object(compact)
}

fn compact_startup_runtime_startup_action_bundle(action_bundle: &Value) -> Value {
    let Some(bundle) = action_bundle.as_object() else {
        return Value::Null;
    };
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        action_bundle,
        &[
            "bundle_version",
            "ready_for_automation",
            "preserves_return_obligation",
        ],
    );
    if let Some(missing_inputs) = bundle
        .get("missing_inputs")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
    {
        compact.insert(
            "missing_inputs".to_string(),
            Value::Array(missing_inputs.clone()),
        );
    }
    if action_bundle["host_current_thread_control"].is_object() {
        compact.insert(
            "host_current_thread_control".to_string(),
            working_state::compact_host_current_thread_control_surface_for_runtime(
                &action_bundle["host_current_thread_control"],
            ),
        );
    }
    if action_bundle["operator_flow"].is_object() {
        let mut operator_flow = serde_json::Map::new();
        for field in [
            "primary_command",
            "rotate_helper_command",
            "startup_command",
            "startup_after_recovery_command",
        ] {
            if let Some(command) = action_bundle["operator_flow"][field]
                .as_str()
                .map(normalize_compact_startup_runtime_command)
                .filter(|value| !value.is_empty())
            {
                operator_flow.insert(field.to_string(), Value::from(command));
            }
        }
        copy_if_present(
            &mut operator_flow,
            &action_bundle["operator_flow"],
            &[
                "primary_command_kind",
                "wait_summary",
                "resume_after_recovery_summary",
            ],
        );
        if !operator_flow.is_empty() {
            compact.insert("operator_flow".to_string(), Value::Object(operator_flow));
        }
    }
    Value::Object(compact)
}

fn compact_startup_state_cli_blocking_reply_contract(contract: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        contract,
        &[
            "active",
            "contract_version",
            "response_kind",
            "max_sentences",
            "must_avoid_substantive_work",
            "must_use_action_bundle_operator_flow",
        ],
    );
    Value::Object(compact)
}

fn compact_startup_state_cli_startup_next_action(action: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        action,
        &[
            "action_kind",
            "blocking",
            "reason",
            "resume_state",
            "no_silent_drop",
            "client_budget_status_label",
            "preserves_return_obligation",
        ],
    );
    let action_bundle = compact_startup_runtime_startup_action_bundle(&action["action_bundle"]);
    if !action_bundle.is_null() {
        compact.insert("action_bundle".to_string(), action_bundle);
    }
    Value::Object(compact)
}

fn compact_startup_state_cli_required_return_task(task: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    copy_if_present(
        &mut compact,
        task,
        &["headline", "next_step", "resume_state"],
    );
    Value::Object(compact)
}

fn normalize_compact_startup_runtime_command(command: &str) -> String {
    command
        .replace('\'', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn prune_startup_runtime_summary(summary: &mut serde_json::Map<String, Value>) {
    for field in ["excluded_reasons_summary", "included_reasons_summary"] {
        if summary.get(field).is_some_and(Value::is_null) {
            summary.remove(field);
        }
    }
    if summary.get("thread_count").and_then(Value::as_u64) == Some(0) {
        summary.remove("thread_count");
    }
}

fn compact_startup_runtime_reply_execution_gate(reply_execution_gate: &Value) -> Value {
    if !reply_execution_gate.is_object() {
        return Value::Null;
    }
    let preserves_return_obligation =
        reply_execution_gate["action_bundle"]["preserves_return_obligation"]
            .as_bool()
            .or_else(|| reply_execution_gate["preserves_return_obligation"].as_bool());
    json!({
        "gate_version": reply_execution_gate["gate_version"].clone(),
        "action_kind": reply_execution_gate["action_kind"].clone(),
        "blocking": reply_execution_gate["blocking"].clone(),
        "must_rotate_before_reply": reply_execution_gate["must_rotate_before_reply"].clone(),
        "must_wait_for_budget_recovery_before_reply":
            reply_execution_gate["must_wait_for_budget_recovery_before_reply"].clone(),
        "reply_budget_mode": reply_execution_gate["reply_budget_mode"].clone(),
        "reply_prefix": reply_execution_gate["reply_prefix"].clone(),
        "rotate_now": reply_execution_gate["rotate_now"].clone(),
        "rotate_soon": reply_execution_gate["rotate_soon"].clone(),
        "preserves_return_obligation": preserves_return_obligation,
    })
}

pub async fn print_restore(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<()> {
    let payload = restore_payload(cfg, args).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn restore_payload(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<Value> {
    let db = connect_bootstrapped_admin(cfg).await?;
    restore_payload_with_db(&db, args).await
}

pub(crate) async fn restore_payload_with_db(
    db: &Client,
    args: &ContinuityStartupArgs,
) -> Result<Value> {
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
    Ok(payload)
}

fn build_continuity_startup_payload(
    context: &ContinuityStartupContext,
    restore: Option<&Value>,
    chat_start_restore: &Value,
) -> Result<Value> {
    let chat_start_node = &chat_start_restore["chat_start_restore"];
    let working_state_node = restore.and_then(|value| value.get("working_state_restore"));
    let workspace_restore_pack_node = restore
        .and_then(|value| value.get("workspace_restore_pack"))
        .or_else(|| working_state_node.and_then(|node| node.get("workspace_restore_pack")));
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
    let compact_start_headline = compact_prompt_fragment(&start_headline, 64);
    let compact_start_next_step = compact_prompt_fragment(&start_next_step, 80);
    let continuity_source_mode = context.continuity["continuity_source_mode"].as_str();
    let continuity_source_namespace_code = context.continuity["continuity_source_namespace_code"]
        .as_str()
        .filter(|_| continuity_source_mode.is_some());
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
                "expected_present": (
                    context.continuity["imported_at_epoch_ms"].as_u64().unwrap_or(0) > 0
                        && context.continuity["documents_imported"].as_u64().unwrap_or(0) > 0
                ) || (
                    continuity_source_mode == Some("working_state_fallback")
                        && context.handoff_summary["headline"].as_str().is_some_and(|value| !value.is_empty())
                        && !startup_next_step.is_empty()
                        && working_state_expected
                ),
                "unexpected_present": false,
                "imported_at_epoch_ms": context.continuity["imported_at_epoch_ms"],
                "documents_imported": context.continuity["documents_imported"],
                "continuity_source_mode": normalized_optional_text_json(
                    continuity_source_mode
                ),
                "continuity_source_namespace_code": normalized_optional_text_json(
                    continuity_source_namespace_code
                ),
                "headline": normalized_optional_text_json(
                    context.handoff_summary["headline"].as_str()
                ),
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
                    && prompt_text.contains(&compact_start_headline)
                    && prompt_text.contains(&compact_start_next_step),
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
                "current_goal": normalized_optional_text_json(
                    working_state_node.and_then(|node| node["current_goal"].as_str())
                ),
                "next_step": normalized_optional_text_json(
                    working_state_node.and_then(|node| node["next_step"].as_str())
                ),
                "restore_confidence": normalized_restore_confidence_value(working_state_node),
                "authoritative_event_id": normalized_optional_text_json(
                    working_state_node.and_then(|node| node["state_lineage"]["authoritative_event_id"].as_str())
                ),
            }),
        )?,
        build_continuity_eval_probe(
            "workspace_restore_pack_recovered_useful",
            if workspace_restore_pack_node.is_some() {
                "recovered_useful"
            } else {
                "under_retrieved"
            },
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": workspace_restore_pack_node.is_some_and(|node| {
                    node["active_commitments"].as_array().is_some()
                        && node["active_constraints"].as_array().is_some()
                        && node["important_artifacts"].as_array().is_some()
                        && node["procedural_restore_policy"]["raw_procedural_archive_forbidden"].as_bool() == Some(true)
                }),
                "unexpected_present": false,
                "summary": normalized_optional_text_json(
                    workspace_restore_pack_node.and_then(|node| node["summary"].as_str())
                ),
                "procedural_surface": normalized_optional_text_json(
                    workspace_restore_pack_node
                        .and_then(|node| {
                            node["procedural_restore_policy"]["materialized_surface"].as_str()
                        })
                ),
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
            "continuity_source": {
                "mode": normalized_optional_text_json(continuity_source_mode),
                "source_namespace_code": normalized_optional_text_json(
                    continuity_source_namespace_code
                ),
            },
            "handoff_summary": normalized_handoff_summary_json(
                context.handoff_summary["headline"].as_str(),
                context.handoff_summary["next_step"].as_str(),
            ),
            "canonical_eval": canonical_eval,
        }),
    );
    payload.insert("chat_start_restore".to_string(), chat_start_node.clone());
    payload.insert(
        "delivery_surface_restore".to_string(),
        chat_start_node.clone(),
    );
    if let Some(node) = working_state_node {
        payload.insert(
            "working_state_restore".to_string(),
            normalized_working_state_restore_projection(Some(node)),
        );
    }
    if let Some(node) = workspace_restore_pack_node {
        payload.insert(
            "workspace_restore_pack".to_string(),
            normalized_workspace_restore_pack_projection(Some(node)),
        );
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
        runtime_state_json: false,
        token_source_kind: "verify_continuity_startup".to_string(),
        skip_live_client_budget_guard: false,
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
                "headline": normalized_optional_text_json(
                    context.handoff_summary["headline"].as_str()
                ),
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
                "restore_confidence": normalized_restore_confidence_value(
                    working_state_restore.as_ref()
                ),
                "current_goal": normalized_optional_text_json(
                    working_state_restore
                        .as_ref()
                        .and_then(|value| value["current_goal"].as_str())
                ),
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
                "prompt_length": normalized_optional_char_count_json(
                    chat_start_node["prompt_text"].as_str()
                ),
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
            "handoff_summary": normalized_handoff_summary_json(
                context.handoff_summary["headline"].as_str(),
                context.handoff_summary["next_step"].as_str(),
            ),
            "working_state_restore_present": working_state_restore_present,
            "working_state_restore": normalized_working_state_restore_projection(
                working_state_restore.as_ref()
            ),
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
    let context = load_startup_context(&db, &args.startup).await?;
    let project = context.project.clone();
    let namespace = context.namespace.clone();
    let handoff_summary = context.handoff_summary.clone();
    let restore = context.restore.clone();
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
        let thread_index_snapshots = postgres::list_observability_snapshots_by_kind_for_scope(
            &db,
            "continuity_thread_index",
            "continuity_thread_index",
            &project.code,
            &namespace.code,
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
    if continuity_answer_requires_live_reply_gate(&args.startup.token_source_kind) {
        let client_budget_guard =
            token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
        if client_budget_guard_blocks_reply(&client_budget_guard) {
            let blocking_reply_contract =
                client_budget_guard["reply_execution_gate"]["blocking_reply_contract"].clone();
            let blocked_reply_text = blocking_reply_contract["template"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE)
                .to_string();
            if args.startup.json {
                let payload = build_blocked_continuity_answer_payload(
                    &project,
                    &namespace,
                    &handoff_summary,
                    restore.as_ref(),
                    &intent,
                    args.question.as_deref(),
                    chat_reference.as_deref(),
                    at_time_rfc3339.as_deref(),
                    messages_count,
                    include_chat_messages,
                    previous_chat_offset,
                    &blocked_reply_text,
                    &client_budget_guard,
                    &args.startup.token_source_kind,
                )?;
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                println!("{blocked_reply_text}");
            }
            return Ok(());
        }
    }
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
    let payload = handoff_payload(cfg, args).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn handoff_payload(cfg: &AppConfig, args: &ContinuityHandoffArgs) -> Result<Value> {
    let details = read_optional_details_file(args.details_file.as_ref())?;
    handoff_payload_from_parts(
        cfg,
        &args.project,
        &args.namespace,
        &args.headline,
        &args.next_step,
        &details,
        args.resolve_current_goal,
        &args.resolved_headlines,
        &args.resolved_task_ids,
    )
    .await
}

pub async fn handoff_payload_from_parts(
    cfg: &AppConfig,
    project_code: &str,
    namespace_code: &str,
    headline: &str,
    next_step: &str,
    details: &str,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
) -> Result<Value> {
    let mut db = connect_bootstrapped_admin(cfg).await?;
    handoff_payload_from_parts_with_db(
        &mut db,
        cfg,
        project_code,
        namespace_code,
        headline,
        next_step,
        details,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
    )
    .await
}

pub async fn client_budget_target_payload(
    cfg: &AppConfig,
    args: &ContinuityClientBudgetTargetArgs,
    thread_id_hint: Option<&str>,
) -> Result<Value> {
    let total_started = Instant::now();
    let connect_started = Instant::now();
    let db = connect_bootstrapped_admin(cfg).await?;
    continuity_profile_log(
        "client_budget_target_payload.connect_bootstrapped_admin",
        connect_started.elapsed().as_millis(),
        &format!("namespace={} percent={}", args.namespace, args.percent),
    );
    let startup_args = ContinuityStartupArgs {
        project: args.project.clone(),
        repo_root: args.repo_root.clone(),
        namespace: args.namespace.clone(),
        json: false,
        runtime_state_json: false,
        token_source_kind: "operator_client_budget_target".to_string(),
        skip_live_client_budget_guard: false,
    };
    let resolve_project_started = Instant::now();
    let project = resolve_project(&db, &startup_args).await?;
    continuity_profile_log(
        "client_budget_target_payload.resolve_project",
        resolve_project_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, args.namespace),
    );
    let ensure_namespace_started = Instant::now();
    let namespace = postgres::ensure_namespace(
        &db,
        project.project_id,
        &args.namespace,
        Some("Continuity"),
        "local_strict",
    )
    .await?;
    continuity_profile_log(
        "client_budget_target_payload.ensure_namespace",
        ensure_namespace_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let target_percent = working_state::normalize_client_budget_target_percent(args.percent)
        .ok_or_else(|| {
            anyhow!(
                "client budget target must be one of {}",
                allowed_client_budget_target_values()
                    .into_iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let record_started = Instant::now();
    let working_state_write_status = if thread_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        working_state::record_client_budget_target_event_with_thread_hint(
            &db,
            &project,
            &namespace,
            target_percent,
            thread_id_hint,
        )
        .await?
    } else {
        working_state::record_client_budget_target_event(&db, &project, &namespace, target_percent)
            .await?
    };
    continuity_profile_log(
        "client_budget_target_payload.record_event",
        record_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    let restore_started = Instant::now();
    let restore =
        working_state::load_recent_restore_bundle_without_live_guard(&db, &project, &namespace)
            .await?;
    continuity_profile_log(
        "client_budget_target_payload.load_recent_restore_bundle_without_live_guard",
        restore_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    let guard_started = Instant::now();
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
    continuity_profile_log(
        "client_budget_target_payload.collect_live_current_session_budget_guard",
        guard_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    let exact_chat_command = client_budget_target_chat_command(target_percent);
    let working_state_write_status_value = serde_json::to_value(&working_state_write_status)?;
    let operator_message_text = append_working_state_warning_to_message(
        &format!("Режим целевой экономии переключён на {target_percent}%."),
        &working_state_write_status_value,
    );
    continuity_profile_log(
        "client_budget_target_payload.total",
        total_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    Ok(json!({
        "client_budget_target_update": {
            "project": {
                "code": project.code.clone(),
                "display_name": project.display_name.clone(),
                "repo_root": project.repo_root.clone(),
            },
            "namespace": {
                "code": namespace.code.clone(),
                "display_name": namespace.display_name.clone(),
            },
            "target_percent": target_percent,
            "allowed_target_percents": allowed_client_budget_target_values(),
            "working_state_write_status": working_state_write_status_value.clone(),
            "working_state_restore": normalized_working_state_restore_projection(
                restore
                    .as_ref()
                    .and_then(|value| value.get("working_state_restore"))
            ),
            "client_budget_guard": client_budget_guard,
            "operator_notice": {
                "kind": "client_budget_target_changed",
                "message_text": operator_message_text,
                "reply_prefix": client_budget_guard["reply_prefix"].clone(),
                "exact_chat_command": exact_chat_command,
                "chat_command_pattern": client_budget_target_chat_command_pattern(),
                "thread_id": thread_id_hint,
                "allowed_target_percents": allowed_client_budget_target_values(),
                "working_state_write_status": working_state_write_status_value,
            }
        }
    }))
}

pub async fn client_budget_target(
    cfg: &AppConfig,
    args: &ContinuityClientBudgetTargetArgs,
) -> Result<()> {
    let payload = client_budget_target_payload(cfg, args, None).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    let target_percent = payload["client_budget_target_update"]["target_percent"]
        .as_u64()
        .unwrap_or(args.percent);
    let project = &payload["client_budget_target_update"]["project"];
    let namespace = &payload["client_budget_target_update"]["namespace"];
    let status_label = format_client_budget_status_label_for_human(
        &payload["client_budget_target_update"]["client_budget_guard"],
    );
    println!("Amai continuity client-budget-target");
    println!();
    println!(
        "Проект: {}",
        format_project_scope_for_human(
            project["display_name"].as_str(),
            project["code"].as_str()
        )
    );
    println!(
        "Корень проекта: {}",
        format_optional_text_for_human(project["repo_root"].as_str())
    );
    println!(
        "Namespace continuity: {}",
        format_optional_text_for_human(namespace["code"].as_str())
    );
    println!("Целевой target клиентской экономии: {}%", target_percent);
    println!(
        "Разрешённые значения: {}",
        allowed_client_budget_target_values()
            .into_iter()
            .map(|value| format!("{value}%"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Live client-budget guard: {status_label}");
    if let Some(reply_prefix) =
        payload["client_budget_target_update"]["client_budget_guard"]["reply_prefix"].as_str()
    {
        if !reply_prefix.trim().is_empty() {
            println!("Текущий reply prefix: {reply_prefix}");
        }
    }
    if let Some(exact_chat_command) =
        payload["client_budget_target_update"]["operator_notice"]["exact_chat_command"].as_str()
    {
        println!("Exact chat-команда: {exact_chat_command}");
    }
    if let Some(restore_node) =
        payload["client_budget_target_update"]["working_state_restore"].as_object()
    {
        if let Some(current_goal) = restore_node
            .get("current_goal")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            println!("Активная линия: {current_goal}");
        }
        if let Some(next_step) = restore_node
            .get("next_step")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            println!("Следующий шаг: {next_step}");
        }
    }
    if let Some(warning) = payload["client_budget_target_update"]["working_state_write_status"]
        ["warning"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        println!("Degraded write-state: {warning}");
    }
    Ok(())
}

pub async fn compact_chat_payload(
    cfg: &AppConfig,
    args: &ContinuityCompactChatArgs,
    thread_id_hint: Option<&str>,
) -> Result<Value> {
    let total_started = Instant::now();
    if args.runtime_fallback || args.skip_handoff {
        if let Some(payload) = compact_chat_payload_from_runtime_artifact(args, thread_id_hint)? {
            continuity_profile_log(
                "compact_chat_payload.runtime_artifact_short_circuit",
                total_started.elapsed().as_millis(),
                &format!(
                    "namespace={} skip_handoff={} runtime_fallback={} launch_host={}",
                    args.namespace, args.skip_handoff, args.runtime_fallback, args.launch_host
                ),
            );
            return Ok(payload);
        }
        if args.runtime_fallback {
            bail!("compact_chat runtime fallback unavailable: startup runtime artifact missing");
        }
    }
    let connect_started = Instant::now();
    let mut db = connect_bootstrapped_admin(cfg).await?;
    continuity_profile_log(
        "compact_chat_payload.connect_bootstrapped_admin",
        connect_started.elapsed().as_millis(),
        &format!(
            "namespace={} skip_handoff={} launch_host={}",
            args.namespace, args.skip_handoff, args.launch_host
        ),
    );
    let startup_args = ContinuityStartupArgs {
        project: args.project.clone(),
        repo_root: args.repo_root.clone(),
        namespace: args.namespace.clone(),
        json: false,
        runtime_state_json: false,
        token_source_kind: "operator_continuity_compact_chat".to_string(),
        skip_live_client_budget_guard: false,
    };
    let mut restore_observed_resources = args
        .repo_root
        .as_deref()
        .map(prepare_continuity_restore_observed_resources_for_repo_root)
        .transpose()?;
    let context_started = Instant::now();
    let context = match tokio::time::timeout(
        Duration::from_secs(3),
        load_startup_context(&db, &startup_args),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            continuity_profile_log(
                "compact_chat_payload.load_startup_context.timeout",
                context_started.elapsed().as_millis(),
                &format!(
                    "namespace={} skip_handoff={} launch_host={}",
                    args.namespace, args.skip_handoff, args.launch_host
                ),
            );
            if let Some(payload) = compact_chat_payload_from_runtime_artifact(args, thread_id_hint)?
            {
                return Ok(payload);
            }
            bail!("compact_chat runtime fallback unavailable: startup runtime artifact missing");
        }
    };
    continuity_profile_log(
        "compact_chat_payload.load_startup_context",
        context_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    let restore_node = context
        .restore
        .as_ref()
        .and_then(|value| value.get("working_state_restore"));
    let guard_started = Instant::now();
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(&db, context.restore.as_ref())
            .await?;
    continuity_profile_log(
        "compact_chat_payload.collect_live_current_session_budget_guard",
        guard_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            context.project.code, context.namespace.code
        ),
    );
    let recommended_headline =
        recommended_workline_headline_from_restore_or_handoff(restore_node, &context.handoff_summary);
    let recommended_next_step =
        recommended_workline_next_step_from_restore_or_handoff(restore_node, &context.handoff_summary);
    let headline = args
        .headline
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(recommended_headline.as_str());
    let next_step = args
        .next_step
        .as_deref()
        .and_then(normalize_next_step_value)
        .unwrap_or(recommended_next_step);
    let details = if let Some(details_file) = args.details_file.as_ref() {
        read_optional_details_file(Some(details_file))?
    } else {
        build_compact_chat_details(&client_budget_guard, headline, &next_step)
    };
    let restore_observed_resources = match restore_observed_resources.take() {
        Some(resources) => resources,
        None => prepare_continuity_restore_observed_resources(&context)?,
    };
    let (handoff_summary, startup_context) = if args.skip_handoff {
        continuity_profile_log(
            "compact_chat_payload.skip_handoff",
            0,
            &format!(
                "project={} namespace={}",
                context.project.code, context.namespace.code
            ),
        );
        (context.handoff_summary.clone(), context)
    } else {
        let handoff_started = Instant::now();
        let handoff_payload = capture_handoff_payload(
            cfg,
            &mut db,
            &context.project,
            &context.namespace,
            context.restore.as_ref(),
            headline,
            &next_step,
            &details,
            false,
            &[],
            &[],
        )
        .await?;
        continuity_profile_log(
            "compact_chat_payload.capture_handoff_payload",
            handoff_started.elapsed().as_millis(),
            &format!(
                "project={} namespace={} headline={}",
                context.project.code,
                context.namespace.code,
                collapse_answer_text(headline, 48)
            ),
        );
        continuity_profile_log(
            "compact_chat_payload.reuse_startup_context_after_handoff",
            0,
            &format!(
                "project={} namespace={}",
                context.project.code, context.namespace.code
            ),
        );
        (
            handoff_payload["continuity_handoff"].clone(),
            ContinuityStartupContext {
                project: context.project,
                namespace: context.namespace,
                continuity: context.continuity,
                handoff_summary: handoff_payload["continuity_handoff"].clone(),
                restore: context.restore,
            },
        )
    };
    let startup_payload_started = Instant::now();
    let startup_payload = startup_payload_with_context_and_resources(
        &db,
        &startup_context,
        &startup_args,
        Some(restore_observed_resources),
    )
    .await?;
    continuity_profile_log(
        "compact_chat_payload.startup_payload_with_context",
        startup_payload_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={}",
            startup_context.project.code, startup_context.namespace.code
        ),
    );
    let repo_root = Path::new(startup_context.project.repo_root.as_str());
    let prompt_text = startup_payload["chat_start_restore"]["prompt_text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let prompt_started = Instant::now();
    let prompt_artifact_path = if prompt_text.trim().is_empty() {
        None
    } else {
        Some(write_compact_chat_prompt_artifact(repo_root, &prompt_text)?)
    };
    continuity_profile_log(
        "compact_chat_payload.write_compact_chat_prompt_artifact",
        prompt_started.elapsed().as_millis(),
        &format!("prompt_empty={}", prompt_text.trim().is_empty()),
    );
    let prompt_artifact_path_string = prompt_artifact_path
        .as_ref()
        .map(|path| path.display().to_string());
    let client_surface =
        onboarding::describe_client_surface(repo_root, None).unwrap_or_else(|_| {
            json!({
                "client_key": "unknown",
                "display_name": "Unknown client",
                "startup_instruction_path": Value::Null,
                "startup_instruction_mode": Value::Null,
            })
        });
    let clean_chat_launch = build_compact_chat_clean_launch_surface(
        &client_surface,
        repo_root,
        prompt_artifact_path.as_deref(),
    );
    let launch_clean_chat_command = clean_chat_launch["launch_clean_chat_command"]
        .as_str()
        .map(str::to_string);
    let launch_clean_chat_fallback_command =
        clean_chat_launch["launch_clean_chat_fallback_command"]
            .as_str()
            .map(str::to_string);
    let has_launch_clean_chat_command = launch_clean_chat_command.is_some();
    let current_thread_id = codex_threads::current_thread_id();
    let compact_chat_notice_thread_id = thread_id_hint
        .or(current_thread_id.as_deref())
        .map(str::to_string);
    let host_current_thread_control =
        working_state::build_host_current_thread_control_surface_for_thread(
            thread_id_hint.or(current_thread_id.as_deref()),
        );
    let compact_client_budget_guard =
        compact_startup_runtime_client_budget_guard(&client_budget_guard);
    let compact_chat_start_restore =
        compact_compact_chat_chat_start_restore(&startup_payload["chat_start_restore"]);
    let compact_host_current_thread_control =
        compact_compact_chat_host_current_thread_control(&host_current_thread_control);
    let compact_handoff = compact_compact_chat_handoff(&handoff_summary);
    let compact_client_surface = compact_client_surface_for_compact_chat(&client_surface);
    let startup_command = shell_join_command(&[
        "amai",
        "continuity",
        "startup",
        "--project",
        startup_context.project.code.as_str(),
        "--namespace",
        startup_context.namespace.code.as_str(),
        "--repo-root",
        startup_context.project.repo_root.as_str(),
        "--token-source-kind",
        "live_continuity_startup",
        "--json",
    ]);
    continuity_profile_log(
        "compact_chat_payload.total",
        total_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} skip_handoff={}",
            startup_context.project.code, startup_context.namespace.code, args.skip_handoff
        ),
    );
    let mut compact_chat = serde_json::Map::new();
    compact_chat.insert(
        "project".to_string(),
        json!({
            "code": startup_context.project.code,
            "display_name": startup_context.project.display_name,
            "repo_root": startup_context.project.repo_root,
        }),
    );
    compact_chat.insert(
        "namespace".to_string(),
        json!({
            "code": startup_context.namespace.code,
            "display_name": startup_context.namespace.display_name,
        }),
    );
    copy_if_present(
        &mut compact_chat,
        &json!({
            "client_budget_guard": compact_client_budget_guard,
            "handoff": compact_handoff,
            "chat_start_restore": compact_chat_start_restore,
            "delivery_surface_restore": compact_chat_start_restore,
            "startup_execution_gate": startup_payload["startup_execution_gate"].clone(),
            "startup_next_action": startup_payload["startup_next_action"].clone(),
            "required_return_task": startup_payload["required_return_task"].clone(),
            "reply_execution_gate": startup_payload["reply_execution_gate"].clone(),
            "host_current_thread_control": compact_host_current_thread_control.clone(),
            "client_surface": compact_client_surface.clone(),
        }),
        &[
            "client_budget_guard",
            "handoff",
            "chat_start_restore",
            "delivery_surface_restore",
            "startup_execution_gate",
            "startup_next_action",
            "required_return_task",
            "reply_execution_gate",
            "host_current_thread_control",
            "client_surface",
        ],
    );
    compact_chat.insert(
        "operator_notice".to_string(),
        json!({
            "kind": "client_budget_compact_chat_requested",
            "message_text": client_budget_compact_chat_notice_message(),
            "exact_chat_command": CLIENT_BUDGET_COMPACT_CHAT_COMMAND,
            "reply_prefix": startup_payload["reply_execution_gate"]["reply_prefix"].clone(),
            "thread_id": compact_chat_notice_thread_id.clone(),
            "prompt_file": prompt_artifact_path_string.clone(),
            "launch_clean_chat_command": launch_clean_chat_command.clone(),
            "launch_clean_chat_fallback_command": launch_clean_chat_fallback_command.clone(),
            "launch_clean_chat_command_kind": if has_launch_clean_chat_command { clean_chat_launch["command_kind"].clone() } else { Value::Null },
            "clean_chat_launch": clean_chat_launch.clone(),
            "manual_fallback_steps": continuity_compact_chat_helpers::compact_chat_manual_fallback_steps(&client_surface),
            "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
            "note": compact_chat_manual_fallback_note(&client_surface),
        }),
    );
    compact_chat.insert(
        "operator_flow".to_string(),
        json!({
            "startup_command": startup_command,
        }),
    );
    Ok(json!({
        "continuity_compact_chat": Value::Object(compact_chat)
    }))
}

fn compact_chat_payload_from_runtime_artifact(
    args: &ContinuityCompactChatArgs,
    thread_id_hint: Option<&str>,
) -> Result<Option<Value>> {
    let repo_root = args
        .repo_root
        .as_ref()
        .ok_or_else(|| anyhow!("compact_chat runtime fallback requires repo_root"))?;
    let repo_root_path = Path::new(repo_root);
    let Some(artifact) = load_startup_runtime_state_artifact(repo_root_path)? else {
        return Ok(None);
    };
    let summary = artifact
        .get("continuity_startup_summary")
        .unwrap_or(&Value::Null);
    let (project_code, namespace_code, project_display_name, namespace_display_name) =
        compact_chat_runtime_scope_fields(summary);
    let handoff_summary = normalized_handoff_summary_json(
        summary["headline"].as_str(),
        summary["next_step"].as_str(),
    );
    let prompt_text = artifact["chat_start_restore"]["prompt_text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let prompt_artifact_path = if prompt_text.trim().is_empty() {
        None
    } else {
        Some(write_compact_chat_prompt_artifact(
            repo_root_path,
            &prompt_text,
        )?)
    };
    let prompt_artifact_path_string = prompt_artifact_path
        .as_ref()
        .map(|path| path.display().to_string());
    let client_surface =
        onboarding::describe_client_surface(repo_root_path, None).unwrap_or_else(|_| {
            json!({
                "client_key": "unknown",
                "display_name": "Unknown client",
                "startup_instruction_path": Value::Null,
                "startup_instruction_mode": Value::Null,
            })
        });
    let clean_chat_launch = build_compact_chat_clean_launch_surface(
        &client_surface,
        repo_root_path,
        prompt_artifact_path.as_deref(),
    );
    let launch_clean_chat_command = clean_chat_launch["launch_clean_chat_command"]
        .as_str()
        .map(str::to_string);
    let launch_clean_chat_fallback_command =
        clean_chat_launch["launch_clean_chat_fallback_command"]
            .as_str()
            .map(str::to_string);
    let has_launch_clean_chat_command = launch_clean_chat_command.is_some();
    let current_thread_id = codex_threads::current_thread_id();
    let compact_chat_notice_thread_id = thread_id_hint
        .or(current_thread_id.as_deref())
        .map(str::to_string);
    let host_current_thread_control =
        working_state::build_host_current_thread_control_surface_for_thread(
            thread_id_hint.or(current_thread_id.as_deref()),
        );
    let compact_host_current_thread_control =
        compact_compact_chat_host_current_thread_control(&host_current_thread_control);
    let compact_client_surface = compact_client_surface_for_compact_chat(&client_surface);
    let repo_root_string = repo_root.display().to_string();
    let startup_command = project_code.as_deref().and_then(|project_code| {
        namespace_code.as_deref().map(|namespace_code| {
            shell_join_command(&[
                "amai",
                "continuity",
                "startup",
                "--project",
                project_code,
                "--namespace",
                namespace_code,
                "--repo-root",
                repo_root_string.as_str(),
                "--token-source-kind",
                "live_continuity_startup",
                "--json",
            ])
        })
    });
    let mut compact_chat = serde_json::Map::new();
    compact_chat.insert(
        "source_mode".to_string(),
        json!("startup_runtime_artifact_fallback"),
    );
    compact_chat.insert(
        "project".to_string(),
        json!({
            "code": project_code,
            "display_name": project_display_name,
            "repo_root": repo_root,
        }),
    );
    compact_chat.insert(
        "namespace".to_string(),
        json!({
            "code": namespace_code,
            "display_name": namespace_display_name,
        }),
    );
    copy_if_present(
        &mut compact_chat,
        &json!({
            "client_budget_guard": artifact["client_budget_guard"].clone(),
            "handoff": compact_compact_chat_handoff(&handoff_summary),
            "chat_start_restore": compact_compact_chat_chat_start_restore(&artifact["chat_start_restore"]),
            "delivery_surface_restore": compact_compact_chat_chat_start_restore(&artifact["chat_start_restore"]),
            "startup_execution_gate": artifact["startup_execution_gate"].clone(),
            "startup_next_action": artifact["startup_next_action"].clone(),
            "required_return_task": artifact["required_return_task"].clone(),
            "reply_execution_gate": artifact["reply_execution_gate"].clone(),
            "host_current_thread_control": compact_host_current_thread_control.clone(),
            "client_surface": compact_client_surface.clone(),
        }),
        &[
            "client_budget_guard",
            "handoff",
            "chat_start_restore",
            "delivery_surface_restore",
            "startup_execution_gate",
            "startup_next_action",
            "required_return_task",
            "reply_execution_gate",
            "host_current_thread_control",
            "client_surface",
        ],
    );
    compact_chat.insert(
        "operator_notice".to_string(),
        json!({
            "kind": "client_budget_compact_chat_requested",
            "message_text": client_budget_compact_chat_notice_message(),
            "exact_chat_command": CLIENT_BUDGET_COMPACT_CHAT_COMMAND,
            "reply_prefix": artifact["reply_execution_gate"]["reply_prefix"].clone(),
            "thread_id": compact_chat_notice_thread_id.clone(),
            "prompt_file": prompt_artifact_path_string.clone(),
            "launch_clean_chat_command": launch_clean_chat_command.clone(),
            "launch_clean_chat_fallback_command": launch_clean_chat_fallback_command.clone(),
            "launch_clean_chat_command_kind": if has_launch_clean_chat_command { clean_chat_launch["command_kind"].clone() } else { Value::Null },
            "clean_chat_launch": clean_chat_launch.clone(),
            "manual_fallback_steps": continuity_compact_chat_helpers::compact_chat_manual_fallback_steps(&client_surface),
            "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
            "note": compact_chat_runtime_artifact_note(),
        }),
    );
    compact_chat.insert(
        "operator_flow".to_string(),
        json!({
            "startup_command": startup_command,
        }),
    );
    Ok(Some(json!({
        "continuity_compact_chat": Value::Object(compact_chat)
    })))
}

pub async fn compact_chat(cfg: &AppConfig, args: &ContinuityCompactChatArgs) -> Result<()> {
    let mut payload = compact_chat_payload(cfg, args, None).await?;
    let launched =
        maybe_launch_compact_chat_host(&mut payload, args.launch_host, !args.json).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    let project = &payload["continuity_compact_chat"]["project"];
    let namespace = &payload["continuity_compact_chat"]["namespace"];
    let notice = &payload["continuity_compact_chat"]["operator_notice"];
    let host_current_thread_control =
        &payload["continuity_compact_chat"]["host_current_thread_control"];
    println!("Amai continuity compact-chat");
    println!();
    println!(
        "Проект: {}",
        format_project_scope_for_human(
            project["display_name"].as_str(),
            project["code"].as_str()
        )
    );
    println!(
        "Корень проекта: {}",
        format_optional_text_for_human(project["repo_root"].as_str())
    );
    println!(
        "Namespace continuity: {}",
        format_optional_text_for_human(namespace["code"].as_str())
    );
    println!(
        "Текущий reply prefix: {}",
        format_reply_prefix_for_human(notice["reply_prefix"].as_str())
    );
    println!("Exact chat-команда: {}", CLIENT_BUDGET_COMPACT_CHAT_COMMAND);
    if let Some(required_host_action) = notice["required_host_action"].as_str() {
        println!("Что должен сделать host/client: {required_host_action}");
    } else if launched {
        println!(
            "Что сделал host/client: automatic clean-surface launch был запрошен, но новая рабочая поверхность всё ещё требует проверки."
        );
    }
    if let Some(summary) = host_current_thread_control["summary"].as_str() {
        println!("Closest same-thread host surface: {summary}");
    }
    if let Some(command_id) = host_current_thread_control["command_id"].as_str() {
        println!("Host internal command id: {command_id}");
    }
    if let Some(uri) = host_current_thread_control["external_uri_launch"]["uri"].as_str() {
        println!("VS Code URI launch: {uri}");
    }
    if let Some(command) =
        host_current_thread_control["external_uri_launch"]["platform_launch_command"].as_str()
    {
        println!("Same-thread shell launch: {command}");
    }
    if let Some(prompt_file) = notice["prompt_file"].as_str() {
        println!("Prompt artifact: {prompt_file}");
    }
    if let Some(command) = notice["launch_clean_chat_command"].as_str() {
        println!("Host launch command: {command}");
    }
    if let Some(command) =
        payload["continuity_compact_chat"]["operator_flow"]["startup_command"].as_str()
    {
        println!("Канонический startup command: {command}");
    }
    let client_surface = &payload["continuity_compact_chat"]["client_surface"];
    if let Some(display_name) = client_surface["display_name"].as_str() {
        println!("Клиент: {display_name}");
    }
    if let Some(path) = client_surface["startup_instruction_path"].as_str() {
        println!(
            "{}",
            format_startup_surface_label(
                path,
                client_surface["startup_instruction_mode"].as_str()
            )
        );
    }
    if let Some(command) = client_surface["reconnect_shell_command"].as_str() {
        println!("Reconnect shell command: {command}");
    }
    if let Some(command) = client_surface["reconnect_bootstrap_command"].as_str() {
        println!("Reconnect bootstrap command: {command}");
    }
    if launched {
        println!("Host launch: requested via VS Code `code chat`.");
    }
    println!();
    println!("Prompt-text для compact restore:");
    println!(
        "{}",
        format_compact_restore_prompt_text(
            payload["continuity_compact_chat"]["chat_start_restore"]["prompt_text"].as_str()
        )
    );
    Ok(())
}

pub async fn rotate_chat(cfg: &AppConfig, args: &ContinuityRotateChatArgs) -> Result<()> {
    let mut db = connect_bootstrapped_admin(cfg).await?;
    let startup_args = ContinuityStartupArgs {
        project: args.project.clone(),
        repo_root: args.repo_root.clone(),
        namespace: args.namespace.clone(),
        json: false,
        runtime_state_json: false,
        token_source_kind: "operator_continuity_rotate_chat".to_string(),
        skip_live_client_budget_guard: false,
    };
    let (context, continuity_import_missing) = match load_startup_context(&db, &startup_args).await
    {
        Ok(context) => (context, false),
        Err(error) if error.to_string().contains("no continuity import found for") => {
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
                        "imported_at_epoch_ms": Value::Null,
                        "documents_imported": Value::Null,
                        "rendered_transcript_files": Value::Null,
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
        let status_label = display_client_budget_status_label(&client_budget_guard)
            .unwrap_or_else(|| Cow::Borrowed("guard не требует rotate"));
        bail!(
            "client-budget rotate helper refused because guard is not active: {status_label}; pass --force only if you intentionally want to rotate despite clear guard"
        );
    }

    let recommended_headline =
        recommended_workline_headline_from_restore_or_handoff(restore_node, &context.handoff_summary);
    let recommended_next_step =
        recommended_workline_next_step_from_restore_or_handoff(restore_node, &context.handoff_summary);
    let headline = args
        .headline
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(recommended_headline.as_str());
    let next_step = args
        .next_step
        .as_deref()
        .and_then(normalize_next_step_value)
        .unwrap_or(recommended_next_step);
    let preserves_return_obligation = restore_node
        .and_then(|value| value["execctl_resume_state"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .is_some_and(|value| value != "clear");
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
        context.restore.as_ref(),
        headline,
        &next_step,
        &details,
        false,
        &[],
        &[],
    )
    .await?;
    let blocking_reply_contract =
        client_budget_guard["reply_execution_gate"]["blocking_reply_contract"].clone();
    let blocked_reply_text = blocking_reply_contract["template"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
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
    let status_label = format_client_budget_status_label_for_human(
        &payload["continuity_rotate_chat"]["client_budget_guard"],
    );
    let last_request = format_optional_text_for_human(
        payload["continuity_rotate_chat"]["client_budget_guard"]["last_request"].as_str(),
    );
    let client_limits = format_optional_text_for_human(
        payload["continuity_rotate_chat"]["client_budget_guard"]["client_limits"].as_str(),
    );
    println!("Amai continuity rotate-chat");
    println!();
    println!(
        "Проект: {} ({})",
        context.project.display_name, context.project.code
    );
    println!("Namespace continuity: {}", context.namespace.code);
    println!("Статус client-budget guard: {status_label}");
    println!("Последний запрос в модель: {last_request}");
    println!(
        "{}: {client_limits}",
        client_limits_surface_label(&payload["continuity_rotate_chat"]["client_budget_guard"])
    );
    println!("Разрешённый короткий ответ в старом чате: {blocked_reply_text}");
    println!();
    println!("Handoff записан:");
    println!("- headline: {headline}");
    println!("- next_step: {next_step}");
    println!(
        "- local_path: {}",
        format_optional_text_for_human(
            payload["continuity_rotate_chat"]["handoff"]["local_path"].as_str(),
        )
    );
    println!();
    println!("Готовые действия:");
    if let Some(imported_at) =
        payload["continuity_rotate_chat"]["continuity_import"]["imported_at_epoch_ms"].as_u64()
    {
        println!(
            "- Continuity import materialized: {}",
            human_epoch_ms(Some(imported_at))
        );
    }
    if let Some(command) =
        payload["continuity_rotate_chat"]["operator_flow"]["startup_command"].as_str()
    {
        println!("- После открытия свежего чата запусти: {command}");
    }
    if let Some(command) =
        payload["continuity_rotate_chat"]["operator_flow"]["rotate_helper_command"].as_str()
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

fn client_limits_surface_label(client_budget_guard: &Value) -> &'static str {
    if client_budget_guard["client_live_meter_current_thread_bound"].as_bool() == Some(false)
        || client_budget_guard["client_live_meter_thread_binding_state"].as_str()
            == Some("no_current_thread_binding")
        || client_budget_guard["client_limits"]
            .as_str()
            .is_some_and(|value| value.trim_start().starts_with("последнее observed:"))
    {
        "Последний observed лимит клиента"
    } else {
        "Лимит клиента сейчас"
    }
}

fn normalize_client_budget_status_label(status_label: &str) -> &str {
    client_turn_pressure_display_status_label(status_label.trim(), true)
}

// Human-facing CLI and startup notes should prefer the neutral delivery-surface
// label when runtime surfaces already provide it, but they must stay compatible
// with older payloads that only carry the legacy status_label field.
fn display_client_budget_status_label<'a>(guard: &'a Value) -> Option<Cow<'a, str>> {
    if let Some(status_label) = guard["delivery_surface_status_label"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(Cow::Borrowed(status_label));
    }
    guard["status_label"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|status_label| Cow::Borrowed(normalize_client_budget_status_label(status_label)))
}

fn normalize_client_budget_advisory_text(text: &str) -> String {
    text.replace("Client-budget guard:", "Client-budget advisory signal:")
        .replace("новый чат нужен сейчас", "сожми текущий чат сейчас")
        .replace("новый чат рекомендован", "сожми текущий чат")
}

fn build_rotate_chat_details(
    client_budget_guard: &Value,
    headline: &str,
    next_step: &str,
) -> String {
    let mut lines = Vec::new();
    if let Some(status_label) = display_client_budget_status_label(client_budget_guard) {
        lines.push(format!("Client-budget advisory signal: {}.", status_label));
    }
    if let Some(last_request) = client_budget_guard["last_request"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        lines.push(format!("Последний запрос в модель: {last_request}."));
    }
    if let Some(client_limits) = client_budget_guard["client_limits"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        lines.push(format!(
            "{}: {client_limits}.",
            client_limits_surface_label(client_budget_guard)
        ));
    }
    if let Some(note) = client_budget_guard["note"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        lines.push(format!(
            "Почему advisory-сигнал рекомендует rotate/compact: {note}."
        ));
    }
    lines.push(format!("Продолжить ту же рабочую линию: {headline}."));
    lines.push(format!(
        "Ближайший обязательный следующий шаг в новой рабочей поверхности: {next_step}."
    ));
    lines.join("\n")
}

fn build_compact_chat_details(
    client_budget_guard: &Value,
    headline: &str,
    next_step: &str,
) -> String {
    let mut lines = Vec::new();
    if let Some(status_label) = display_client_budget_status_label(client_budget_guard) {
        lines.push(format!("Client-budget advisory signal: {}.", status_label));
    }
    if let Some(last_request) = client_budget_guard["last_request"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        lines.push(format!("Последний запрос в модель: {last_request}."));
    }
    if let Some(client_limits) = client_budget_guard["client_limits"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        lines.push(format!(
            "{}: {client_limits}.",
            client_limits_surface_label(client_budget_guard)
        ));
    }
    lines.push(
        "Запрошен compact-chat control для переноса рабочей линии на новую clean surface."
            .to_string(),
    );
    lines.push(format!("Сохраняем текущую рабочую линию: {headline}."));
    lines.push(format!(
        "После clean-surface restore обязательный следующий шаг: {next_step}."
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
        index
            .threads
            .retain(|entry| entry.cwd.starts_with(repo_root));
        index
            .threads
            .sort_by(|left, right| right.source_rollout.cmp(&left.source_rollout));
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
        lines.push(format!(
            "- `title`: `{}`",
            format_optional_text_for_human(Some(&entry.title))
        ));
        lines.push(format!("- `cwd`: `{}`", entry.cwd));
        if let Some(first_user_message) = optional_non_empty_text(Some(&entry.first_user_message)) {
            lines.push(format!(
                "- `first_user_message`: `{}`",
                first_user_message
            ));
        }
        if let Some(source_rollout) = optional_non_empty_text(Some(&entry.source_rollout)) {
            lines.push(format!("- `source_rollout`: `{}`", source_rollout));
        }
        if let Some(raw_mirror) = optional_non_empty_text(Some(&entry.raw_mirror)) {
            lines.push(format!("- `raw_mirror`: `{}`", raw_mirror));
        }
        if let Some(rendered_transcript) = optional_non_empty_text(Some(&entry.rendered_transcript))
        {
            lines.push(format!(
                "- `rendered_transcript`: `{}`",
                rendered_transcript
            ));
        }
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
    !trimmed.is_empty()
        && trimmed != "ещё нет данных"
        && trimmed != "Продолжить активную рабочую линию"
}

async fn capture_handoff_payload(
    cfg: &AppConfig,
    db: &mut Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    previous_restore: Option<&Value>,
    headline: &str,
    next_step: &str,
    details: &str,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
) -> Result<Value> {
    let captured_at_epoch_ms = now_epoch_ms()?;
    let body = render_handoff_markdown(headline, next_step, details);
    let body_sha256 = hex_sha256(body.as_bytes());
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
    let local_handoff_path_text = local_handoff_path.display().to_string();
    let semantic_replay_noop = working_state::handoff_semantic_replay_matches_previous_restore(
        previous_restore,
        &local_handoff_path_text,
        headline,
        next_step,
        details,
        captured_at_epoch_ms,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
    );
    let document_index_refresh_performed = if semantic_replay_noop {
        false
    } else {
        continuity_handoff_document_index_refresh_due(db, project, namespace, captured_at_epoch_ms)
            .await?
    };
    if document_index_refresh_performed {
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
        let refresh_payload = json!({
            "continuity_handoff_document_index_refresh": {
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
                "relative_path": ".amai-continuity/live-handoff/HANDOFF.md",
                "file_sha256": body_sha256,
            }
        });
        let _ = postgres::insert_observability_snapshot(
            db,
            CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_KIND,
            &refresh_payload,
        )
        .await?;
    }
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
            "resolve_current_goal": resolve_current_goal,
            "resolved_pending_return_headlines": resolved_headlines,
            "resolved_pending_return_task_ids": resolved_task_ids,
            "relative_path": ".amai-continuity/live-handoff/HANDOFF.md",
            "local_path": local_handoff_path_text.clone(),
            "document_index_refresh_performed": document_index_refresh_performed,
            "semantic_replay_noop": semantic_replay_noop,
        }
    });
    if !semantic_replay_noop {
        let _ = postgres::insert_observability_snapshot(&db, "continuity_handoff", &payload).await?;
    }
    working_state::record_handoff_event_with_previous_restore(
        &db,
        &project,
        &namespace,
        headline,
        next_step,
        &details,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
        &local_handoff_path_text,
        previous_restore,
    )
    .await?;
    Ok(payload)
}

fn continuity_handoff_document_index_refresh_due_from_previous(
    previous_refresh_epoch_ms: Option<u64>,
    captured_at_epoch_ms: u64,
) -> bool {
    previous_refresh_epoch_ms
        .map(|value| {
            captured_at_epoch_ms.saturating_sub(value)
                > CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_MIN_INTERVAL_MS
        })
        .unwrap_or(true)
}

async fn continuity_handoff_document_index_refresh_due(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    captured_at_epoch_ms: u64,
) -> Result<bool> {
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_KIND,
        &project.code,
        &namespace.code,
        Some(1),
    )
    .await?;
    let previous_refresh_epoch_ms = snapshots.first().map(|snapshot| {
        continuity_snapshot_semantic_epoch_ms(
            snapshot,
            CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_KIND,
        )
        .max(0) as u64
    });
    Ok(continuity_handoff_document_index_refresh_due_from_previous(
        previous_refresh_epoch_ms,
        captured_at_epoch_ms,
    ))
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
            "handoff_summary": normalized_handoff_summary_json(
                handoff_summary["headline"].as_str(),
                handoff_summary["next_step"].as_str(),
            ),
            "restore_present": restore.is_some(),
            "included_reasons_summary": included_reasons_summary,
            "excluded_reasons_summary": excluded_reasons_summary,
            "chat_lookup": {
                "found": chat_tail.is_some(),
                "thread_id": chat_tail.map(|value| value.thread_id.as_str()),
                "title": chat_tail.map(|value| value.title.as_str()),
                "summary_headline": chat_tail
                    .and_then(|value| optional_non_empty_text(value.summary_headline.as_deref())),
                "summary_next_step": chat_tail
                    .and_then(|value| optional_non_empty_text(value.summary_next_step.as_deref())),
                "messages_count": chat_tail.map(|value| value.messages.len()),
                "selected_time_slice": chat_tail
                    .and_then(|value| value.selected_time_slice.as_ref())
                    .map(|slice| {
                        json!({
                            "started_at": slice.started_at,
                            "ended_at": slice.ended_at,
                            "summary_headline": optional_non_empty_text(Some(&slice.summary_headline)),
                            "summary_next_step": optional_non_empty_text(Some(&slice.summary_next_step)),
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

fn continuity_answer_requires_live_reply_gate(token_source_kind: &str) -> bool {
    token_source_kind.starts_with("live_")
}

fn client_budget_guard_blocks_reply(guard: &Value) -> bool {
    working_state::client_budget_guard_blocks_reply(guard)
}

fn build_blocked_continuity_answer_payload(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    handoff_summary: &Value,
    restore: Option<&Value>,
    intent: &str,
    question: Option<&str>,
    chat_reference: Option<&str>,
    at_time_rfc3339: Option<&str>,
    messages_count: usize,
    include_chat_messages: bool,
    previous_chat_offset: usize,
    answer_text: &str,
    client_budget_guard: &Value,
    token_source_kind: &str,
) -> Result<Value> {
    let restore_node = restore.map(|value| &value["working_state_restore"]);
    let included_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "included")
    });
    let excluded_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "not_included")
    });
    let blocking_reply_contract =
        client_budget_guard["reply_execution_gate"]["blocking_reply_contract"].clone();
    let expected_reply_text = blocking_reply_contract["template"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE);
    let fail_closed_ok = answer_text.trim() == expected_reply_text;
    let probe = build_continuity_eval_probe(
        "continuity_answer_client_budget_guard_blocked",
        "hit_correct_target",
        EvalPattern::IsolationBoundary,
        false,
        json!({
            "boundary_clean": fail_closed_ok,
            "fail_closed_ok": fail_closed_ok,
            "unexpected_present": answer_mentions_temporal_match(answer_text),
            "intent": intent,
            "source_kind": token_source_kind,
            "must_rotate_before_reply": client_budget_guard["reply_execution_gate"]["must_rotate_before_reply"],
            "response_kind": blocking_reply_contract["response_kind"],
            "answer": answer_text,
        }),
    )?;
    let canonical_eval = build_continuity_canonical_eval(std::slice::from_ref(&probe))?;
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
            "handoff_summary": normalized_handoff_summary_json(
                handoff_summary["headline"].as_str(),
                handoff_summary["next_step"].as_str(),
            ),
            "restore_present": restore.is_some(),
            "included_reasons_summary": included_reasons_summary,
            "excluded_reasons_summary": excluded_reasons_summary,
            "chat_lookup": {
                "found": false,
                "thread_id": Value::Null,
                "title": Value::Null,
                "summary_headline": Value::Null,
                "summary_next_step": Value::Null,
                "messages_count": Value::Null,
                "selected_time_slice": Value::Null,
            },
            "blocked_by_client_budget_guard": true,
            "client_budget_guard": client_budget_guard,
            "blocking_reply_contract": blocking_reply_contract,
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
    let workspace_restore_pack_node = restore
        .get("workspace_restore_pack")
        .filter(|value| value.is_object())
        .unwrap_or(&working_state_node["workspace_restore_pack"]);
    let prompt_text = chat_start_node["prompt_text"].as_str().unwrap_or_default();
    let start_headline = format_optional_text_for_human(chat_start_node["headline"].as_str());
    let start_next_step = normalize_next_step_value(
        chat_start_node["next_step"].as_str().unwrap_or_default(),
    )
    .unwrap_or_else(|| "ещё нет данных".to_string());
    let compact_start_headline = compact_prompt_fragment(&start_headline, 64);
    let compact_start_next_step = compact_prompt_fragment(&start_next_step, 80);
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
                    && prompt_text.contains(&compact_start_headline)
                    && prompt_text.contains(&compact_start_next_step),
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
                "current_goal": normalized_optional_text_json(Some(current_goal)),
                "next_step": normalized_optional_text_json(Some(restore_next_step)),
                "restore_confidence": working_state_node["restore_confidence"],
                "authoritative_event_id": normalized_optional_text_json(Some(authoritative_event_id)),
            }),
        )?,
        build_continuity_eval_probe(
            "workspace_restore_pack_recovered_useful",
            "recovered_useful",
            EvalPattern::RecoveryTarget,
            true,
            json!({
                "expected_present": workspace_restore_pack_node["active_commitments"].as_array().is_some()
                    && workspace_restore_pack_node["active_constraints"].as_array().is_some()
                    && workspace_restore_pack_node["important_artifacts"].as_array().is_some()
                    && workspace_restore_pack_node["procedural_restore_policy"]["raw_procedural_archive_forbidden"].as_bool() == Some(true),
                "unexpected_present": false,
                "summary": normalized_optional_text_json(
                    workspace_restore_pack_node["summary"].as_str()
                ),
                "procedural_surface": normalized_optional_text_json(
                    workspace_restore_pack_node["procedural_restore_policy"]
                        ["materialized_surface"]
                        .as_str()
                ),
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
            "handoff_summary": normalized_handoff_summary_json(
                handoff_summary["headline"].as_str(),
                handoff_summary["next_step"].as_str(),
            ),
            "canonical_eval": canonical_eval,
        },
        "chat_start_restore": chat_start_node.clone(),
        "working_state_restore": normalized_working_state_restore_projection(
            Some(working_state_node)
        ),
        "workspace_restore_pack": normalized_workspace_restore_pack_projection(
            Some(workspace_restore_pack_node)
        ),
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
    let project_headline = format_optional_text_for_human(handoff_summary["headline"].as_str());
    let project_next_step = normalize_next_step_value(
        handoff_summary["next_step"].as_str().unwrap_or_default(),
    )
    .unwrap_or_else(|| "ещё нет данных".to_string());
    let first_line = answer_text.lines().next().unwrap_or_default();
    match intent {
        "previous_chat" => {
            if let Some(chat_tail) = chat_tail {
                let expected_fragments = continuity_answer_expected_fragments(chat_tail);
                let expected_present = expected_fragments
                    .iter()
                    .any(|fragment| !fragment.is_empty() && answer_text.contains(fragment));
                let stale_substitution = first_line.contains(&project_headline)
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
            ) && answer_text.contains(&project_headline)
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
                let stale_substitution = first_line.contains(&project_headline)
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
            ) && answer_text.contains(&project_headline)
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
            "expected_present": answer_text.contains(&project_headline)
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
    let headline = format_optional_text_for_human(handoff_summary["headline"].as_str());
    let project_next_step = normalize_next_step_value(
        handoff_summary["next_step"].as_str().unwrap_or_default(),
    )
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
    let answer_headline = thread_headline.as_deref().unwrap_or(&headline);
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
                "selected_headline": normalized_optional_text_json(
                    selected_handoff.payload["continuity_handoff"]["headline"].as_str()
                ),
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
                "selected_headline": normalized_optional_text_json(
                    selected_import.payload["continuity_import"]["active_workline_summary"]["details"]["headline"]
                        .as_str()
                ),
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

async fn continuity_namespace_fallback(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<NamespaceRecord>> {
    if namespace.code == "continuity" {
        return Ok(None);
    }
    postgres::find_namespace_by_code(db, project.project_id, "continuity").await
}

fn continuity_with_source_metadata(
    continuity: Value,
    mode: &str,
    source_namespace_code: &str,
) -> Value {
    let mut continuity = continuity;
    if let Some(node) = continuity.as_object_mut() {
        node.insert(
            "continuity_source_mode".to_string(),
            Value::String(mode.to_string()),
        );
        node.insert(
            "continuity_source_namespace_code".to_string(),
            Value::String(source_namespace_code.to_string()),
        );
    }
    continuity
}

fn synthetic_continuity_from_runtime(
    namespace: &NamespaceRecord,
    handoff_summary: Option<&Value>,
    restore: Option<&Value>,
) -> Value {
    let restore_node = restore.and_then(|value| value.get("working_state_restore"));
    let headline = handoff_summary
        .and_then(|value| value["headline"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .or_else(|| {
            restore_node
                .and_then(|value| value["current_goal"].as_str())
                .and_then(|value| optional_non_empty_text(Some(value)))
        });
    let next_step = handoff_summary
        .and_then(|value| value["next_step"].as_str())
        .and_then(normalize_next_step_value)
        .or_else(|| {
            restore_node
                .and_then(|value| value["next_step"].as_str())
                .and_then(normalize_next_step_value)
        });
    continuity_with_source_metadata(
        json!({
            "imported_at_epoch_ms": Value::Null,
            "documents_imported": Value::Null,
            "rendered_transcript_files": Value::Null,
            "session_memory_files": Value::Null,
            "bootstrap_summary": {
                "bootstrap_file": Value::Null,
                "details": {
                    "thread_count": Value::Null,
                    "latest_rendered_transcript": Value::Null
                }
            },
            "active_workline_summary": {
                "active_workline_file": Value::Null,
                "details": {
                    "headline": headline,
                    "next_step": next_step
                }
            }
        }),
        "working_state_fallback",
        &namespace.code,
    )
}

async fn load_startup_context(
    db: &Client,
    args: &ContinuityStartupArgs,
) -> Result<ContinuityStartupContext> {
    let step_started = Instant::now();
    let project = resolve_project(db, args).await?;
    continuity_profile_log(
        "load_startup_context.resolve_project",
        step_started.elapsed().as_millis(),
        &format!("project_code={}", project.code),
    );
    let step_started = Instant::now();
    let namespace = postgres::find_namespace_by_code(db, project.project_id, &args.namespace)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.namespace))?;
    continuity_profile_log(
        "load_startup_context.find_namespace",
        step_started.elapsed().as_millis(),
        &format!("namespace_code={}", namespace.code),
    );
    let step_started = Instant::now();
    let requested_restore = working_state::build_restore_bundle_with_options(
        db,
        &project,
        &namespace,
        args.skip_live_client_budget_guard,
    )
    .await?;
    continuity_profile_log(
        "load_startup_context.requested_restore",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} has_restore={}",
            project.code,
            namespace.code,
            requested_restore.is_some()
        ),
    );
    let step_started = Instant::now();
    let requested_handoff = latest_handoff_summary(db, &project, &namespace).await?;
    continuity_profile_log(
        "load_startup_context.requested_handoff",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} has_handoff={}",
            project.code,
            namespace.code,
            requested_handoff.is_some()
        ),
    );
    let step_started = Instant::now();
    let fallback_namespace = continuity_namespace_fallback(db, &project, &namespace).await?;
    continuity_profile_log(
        "load_startup_context.fallback_namespace",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} fallback={}",
            project.code,
            namespace.code,
            fallback_namespace
                .as_ref()
                .map(|value| value.code.as_str())
                .unwrap_or("none")
        ),
    );
    let fallback_restore = if requested_restore.is_none() {
        if let Some(fallback_namespace) = fallback_namespace.as_ref() {
            let step_started = Instant::now();
            let restore = working_state::build_restore_bundle_with_options(
                db,
                &project,
                fallback_namespace,
                args.skip_live_client_budget_guard,
            )
            .await?;
            continuity_profile_log(
                "load_startup_context.fallback_restore",
                step_started.elapsed().as_millis(),
                &format!(
                    "project={} namespace={} has_restore={}",
                    project.code,
                    fallback_namespace.code,
                    restore.is_some()
                ),
            );
            restore
        } else {
            None
        }
    } else {
        None
    };
    let fallback_handoff = if requested_handoff.is_none() {
        if let Some(fallback_namespace) = fallback_namespace.as_ref() {
            let step_started = Instant::now();
            let handoff = latest_handoff_summary(db, &project, fallback_namespace).await?;
            continuity_profile_log(
                "load_startup_context.fallback_handoff",
                step_started.elapsed().as_millis(),
                &format!(
                    "project={} namespace={} has_handoff={}",
                    project.code,
                    fallback_namespace.code,
                    handoff.is_some()
                ),
            );
            handoff
        } else {
            None
        }
    } else {
        None
    };
    let continuity = if let Some(snapshot) = {
        let step_started = Instant::now();
        let snapshot = latest_continuity_import_snapshot(db, &project, &namespace).await?;
        continuity_profile_log(
            "load_startup_context.continuity_snapshot_scoped",
            step_started.elapsed().as_millis(),
            &format!(
                "project={} namespace={} has_snapshot={}",
                project.code,
                namespace.code,
                snapshot.is_some()
            ),
        );
        snapshot
    } {
        continuity_with_source_metadata(
            snapshot.payload["continuity_import"].clone(),
            "scoped_import",
            &namespace.code,
        )
    } else if let Some(fallback_namespace) = fallback_namespace.as_ref() {
        if let Some(snapshot) = {
            let step_started = Instant::now();
            let snapshot =
                latest_continuity_import_snapshot(db, &project, fallback_namespace).await?;
            continuity_profile_log(
                "load_startup_context.continuity_snapshot_fallback",
                step_started.elapsed().as_millis(),
                &format!(
                    "project={} namespace={} has_snapshot={}",
                    project.code,
                    fallback_namespace.code,
                    snapshot.is_some()
                ),
            );
            snapshot
        } {
            continuity_with_source_metadata(
                snapshot.payload["continuity_import"].clone(),
                "continuity_namespace_fallback_import",
                &fallback_namespace.code,
            )
        } else {
            synthetic_continuity_from_runtime(
                &namespace,
                requested_handoff.as_ref().or(fallback_handoff.as_ref()),
                requested_restore.as_ref().or(fallback_restore.as_ref()),
            )
        }
    } else {
        synthetic_continuity_from_runtime(
            &namespace,
            requested_handoff.as_ref(),
            requested_restore.as_ref(),
        )
    };
    let handoff_summary_source = requested_handoff
        .or(fallback_handoff)
        .unwrap_or_else(|| continuity["active_workline_summary"]["details"].clone());
    let handoff_summary = normalized_handoff_summary_json(
        handoff_summary_source["headline"].as_str(),
        handoff_summary_source["next_step"].as_str(),
    );
    let restore = requested_restore.or(fallback_restore);
    if continuity["continuity_source_mode"].as_str() == Some("working_state_fallback")
        && handoff_summary["headline"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .is_none()
        && restore.is_none()
    {
        return Err(anyhow!(
            "no continuity import found for {}::{} and no working-state/handoff fallback available",
            project.code,
            namespace.code
        ));
    }
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
    let headline = format_optional_text_for_human(handoff_summary["headline"].as_str());
    let next_step =
        normalize_next_step_value(handoff_summary["next_step"].as_str().unwrap_or_default())
            .unwrap_or_else(|| "ещё нет данных".to_string());
    let current_goal = restore_node
        .and_then(|value| value["current_goal"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .unwrap_or(headline.as_str())
        .to_string();
    let restore_confidence = normalized_restore_confidence_value(restore_node);
    let materialized_summary = restore_node.and_then(summarize_startup_materialized_notes);
    let recent_actions_summary =
        restore_node.and_then(|value| summarize_recent_actions(&value["recent_actions"]));
    let active_files_summary =
        restore_node.and_then(|value| summarize_string_list(&value["active_files"], 4));
    let open_questions_summary =
        restore_node.and_then(|value| summarize_string_list(&value["open_questions"], 3));
    let workspace_graph_summary =
        restore_node.and_then(|value| workspace_graph::human_summary(&value["workspace_graph"]));
    let workspace_restore_pack_summary = restore_node
        .and_then(|value| value["workspace_restore_pack_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let workspace_restore_pack = restore_node
        .filter(|value| value["workspace_restore_pack"].is_object())
        .map(|value| compact_workspace_restore_pack_for_startup(&value["workspace_restore_pack"]));
    let skill_execution_card_summary = restore_node
        .and_then(|value| value["skill_execution_card_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let skill_execution_card = restore_node
        .filter(|value| value["skill_execution_card"].is_object())
        .map(|value| value["skill_execution_card"].clone());
    let pending_return_summary = restore_node
        .and_then(|value| value["pending_return_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let execctl_resume_contract_summary = restore_node
        .and_then(|value| value["execctl_resume_contract_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let execctl_resume_obligation = restore_node
        .map(|value| summarize_execctl_resume_obligation(&value["execctl_resume_contract"]))
        .unwrap_or_else(|| default_execctl_resume_obligation(None, None));
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
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned)
        .or_else(|| summarize_startup_next_action(&startup_next_action));
    let execctl_active_lease = restore_node
        .filter(|value| value["execctl_active_lease"].is_object())
        .map(|value| value["execctl_active_lease"].clone());
    let execctl_active_lease_summary = restore_node
        .and_then(|value| value["execctl_active_lease_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let project_task_tree_summary = restore_node
        .and_then(|value| value["project_task_tree_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let project_task_tree = restore_node
        .filter(|value| value["project_task_tree"].is_object())
        .map(|value| compact_project_task_tree_for_startup(&value["project_task_tree"]));
    let project_task_ledger_summary = restore_node
        .and_then(|value| value["project_task_ledger_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let project_task_ledger = restore_node
        .filter(|value| value["project_task_ledger"].is_object())
        .map(|value| compact_project_task_ledger_for_startup(&value["project_task_ledger"]));
    let required_task_set = restore_node
        .filter(|value| value["required_task_set"].is_array())
        .map(|value| value["required_task_set"].clone());
    let required_task_set_summary = restore_node
        .and_then(|value| value["required_task_set_summary"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let pending_return_queue = restore_node
        .filter(|value| value["pending_return_queue"].is_array())
        .map(|value| value["pending_return_queue"].clone());
    let required_return_task = restore_node
        .filter(|value| value["execctl_resume_contract"]["required_return_task"].is_object())
        .map(|value| value["execctl_resume_contract"]["required_return_task"].clone())
        .or_else(|| {
            let headline = execctl_resume_obligation["required_return_headline"]
                .as_str()
                .and_then(|value| optional_non_empty_text(Some(value)))?;
            Some(json!({
                "headline": headline,
                "next_step": execctl_resume_obligation["required_return_next_step"]
            }))
        });
    let included_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "included")
    });
    let excluded_reasons_summary = restore_node.and_then(|value| {
        continuity_decision_trace_summary(Some(&value["latest_decision_trace"]), "not_included")
    });
    let thread_count = continuity["bootstrap_summary"]["details"]["thread_count"].as_u64();
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
            "restore_confidence": restore_confidence,
            "thread_count": thread_count,
            "materialized_summary": materialized_summary,
            "recent_actions_summary": recent_actions_summary,
            "active_files_summary": active_files_summary,
            "open_questions_summary": open_questions_summary,
            "workspace_graph_summary": workspace_graph_summary,
            "workspace_restore_pack_summary": workspace_restore_pack_summary,
            "workspace_restore_pack": workspace_restore_pack,
            "skill_execution_card_summary": skill_execution_card_summary,
            "skill_execution_card": skill_execution_card,
            "pending_return_summary": pending_return_summary,
            "pending_return_queue": pending_return_queue,
            "execctl_resume_contract_summary": execctl_resume_contract_summary,
            "execctl_resume_obligation": execctl_resume_obligation,
            "startup_next_action": startup_next_action,
            "startup_next_action_summary": startup_next_action_summary,
            "execctl_active_lease": execctl_active_lease,
            "execctl_active_lease_summary": execctl_active_lease_summary,
            "required_return_task": required_return_task,
            "required_task_set": required_task_set,
            "required_task_set_summary": required_task_set_summary,
            "project_task_tree": project_task_tree,
            "project_task_tree_summary": project_task_tree_summary,
            "project_task_ledger": project_task_ledger,
            "project_task_ledger_summary": project_task_ledger_summary,
            "execctl_resume_state": execctl_resume_obligation["resume_state"].clone(),
            "included_reasons_summary": included_reasons_summary,
            "excluded_reasons_summary": excluded_reasons_summary,
            "prompt_text": render_chat_start_prompt(
                project,
                namespace,
                handoff_summary,
                restore_node,
            ),
        }
    })
}

fn default_execctl_resume_obligation(
    required_return_task: Option<&Value>,
    resume_state: Option<&str>,
) -> Value {
    let required_headline = required_return_task
        .and_then(|task| task["headline"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)));
    let required_next_step = required_return_task
        .and_then(|task| task["next_step"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)));
    json!({
        "resume_state": resume_state,
        "no_silent_drop": true,
        "pending_return_count": if required_headline.is_some() { 1 } else { 0 },
        "active_task_headline": Value::Null,
        "required_return_headline": required_headline,
        "required_return_next_step": required_next_step,
        "required_task_set_count": 0,
        "required_task_set": Value::Array(Vec::new()),
        "required_task_set_summary": Value::Null,
    })
}

fn summarize_execctl_resume_obligation(contract: &Value) -> Value {
    if !contract.is_object() {
        return default_execctl_resume_obligation(None, None);
    }
    let active_task = &contract["active_task"];
    let required_return_task = &contract["required_return_task"];
    let resume_state = contract["resume_state"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    let required_task_set_summary = contract["required_task_set_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    json!({
        "resume_state": resume_state,
        "no_silent_drop": contract["no_silent_drop"].as_bool().unwrap_or(true),
        "pending_return_count": contract["pending_return_count"].as_u64(),
        "active_task_headline": active_task["headline"]
            .as_str()
            .and_then(|value| optional_non_empty_text(Some(value))),
        "required_return_headline": required_return_task["headline"]
            .as_str()
            .and_then(|value| optional_non_empty_text(Some(value))),
        "required_return_next_step": required_return_task["next_step"]
            .as_str()
            .and_then(|value| optional_non_empty_text(Some(value))),
        "required_task_set_count": contract["required_task_set_count"].as_u64(),
        "required_task_set": contract["required_task_set"].clone(),
        "required_task_set_summary": required_task_set_summary,
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
        .and_then(|value| optional_non_empty_text(Some(value)));
    let no_silent_drop = execctl_resume_obligation["no_silent_drop"]
        .as_bool()
        .unwrap_or(true);
    let active_headline = execctl_resume_obligation["active_task_headline"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
        .unwrap_or(current_goal);
    let required_headline = execctl_resume_obligation["required_return_headline"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    let required_next_step = execctl_resume_obligation["required_return_next_step"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    let required_task_set = execctl_resume_obligation["required_task_set"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let required_task_set_summary = execctl_resume_obligation["required_task_set_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    let required_task_set_next_step = required_task_set
        .get(0)
        .and_then(|item| item.as_str())
        .and_then(|value| optional_non_empty_text(Some(value)));
    let _ = project;
    let _ = namespace;
    let _ = client_budget_guard;
    if resume_state.is_some_and(|value| value != "clear") && required_headline.is_some() {
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "resume_required_return_task",
            "blocking": true,
            "reason": "execctl_return_required",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": required_headline,
            "next_step": required_next_step,
            "required_task_set": required_task_set,
            "required_task_set_summary": required_task_set_summary,
        })
    } else if !required_task_set.is_empty() {
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "honor_required_task_set",
            "blocking": true,
            "reason": "required_task_set_present",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": active_headline,
            "next_step": required_task_set_next_step.unwrap_or(next_step),
            "required_task_set": required_task_set,
            "required_task_set_summary": required_task_set_summary,
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
        .and_then(|item| optional_non_empty_text(Some(item)))
        .map(normalize_client_budget_advisory_text)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let next_step = value["next_step"]
        .as_str()
        .and_then(|item| optional_non_empty_text(Some(item)))
        .map(normalize_client_budget_advisory_text)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let mut summary = format!("{action_kind}: {headline} -> {next_step}");
    if action_kind == "honor_required_task_set" {
        if let Some(task_summary) = value["required_task_set_summary"]
            .as_str()
            .and_then(|item| optional_non_empty_text(Some(item)))
        {
            summary.push_str(&format!(" ({})", compact_prompt_fragment(task_summary, 72)));
        }
    }
    Some(summary)
}

fn compact_prompt_fragment(value: &str, max_chars: usize) -> String {
    collapse_answer_text(value, max_chars)
}

fn summarize_required_task_set_for_prompt(restore_node: Option<&Value>) -> Option<String> {
    if let Some(summary) = restore_node
        .and_then(|value| value["required_task_set_summary"].as_str())
        .filter(|value| !value.trim().is_empty())
    {
        return Some(compact_prompt_fragment(summary, 96));
    }
    let tasks = restore_node
        .and_then(|value| value["required_task_set"].as_array())
        .filter(|items| !items.is_empty())?;
    let first = tasks
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())?;
    if tasks.len() == 1 {
        Some(compact_prompt_fragment(first, 96))
    } else {
        Some(compact_prompt_fragment(
            &format!("{} задач(и): {}", tasks.len(), first),
            96,
        ))
    }
}

fn summarize_required_task_set_for_human_surface(node: &Value) -> Option<String> {
    if let Some(summary) = node["required_task_set_summary"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(summary.trim().to_string());
    }
    let tasks = node["required_task_set"]
        .as_array()
        .filter(|items| !items.is_empty())?;
    let first = tasks
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())?;
    if tasks.len() == 1 {
        Some(first.to_string())
    } else {
        Some(format!("{} задач(и): {}", tasks.len(), first))
    }
}

fn summarize_pending_return_for_prompt(restore_node: Option<&Value>) -> Option<String> {
    if let Some(summary) = restore_node
        .and_then(|value| value["pending_return_summary"].as_str())
        .filter(|value| !value.trim().is_empty())
    {
        return Some(compact_prompt_fragment(summary, 96));
    }
    let queue = restore_node
        .and_then(|value| value["pending_return_queue"].as_array())
        .filter(|items| !items.is_empty())?;
    let first = queue
        .iter()
        .find_map(|item| item["headline"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if queue.len() == 1 {
        Some(compact_prompt_fragment(
            &format!("pending_return(1): {}", first),
            96,
        ))
    } else {
        Some(compact_prompt_fragment(
            &format!(
                "pending_return({}): {}; +{} more",
                queue.len(),
                first,
                queue.len() - 1
            ),
            96,
        ))
    }
}

fn summarize_execctl_resume_contract_for_prompt(
    restore_node: Option<&Value>,
    execctl_resume_obligation: &Value,
) -> Option<String> {
    if let Some(summary) = restore_node
        .and_then(|value| value["execctl_resume_contract_summary"].as_str())
        .filter(|value| !value.trim().is_empty())
    {
        return Some(compact_prompt_fragment(summary, 96));
    }
    let resume_state = execctl_resume_obligation["resume_state"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    let required_headline = execctl_resume_obligation["required_return_headline"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))?;
    let required_next_step = execctl_resume_obligation["required_return_next_step"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
        .unwrap_or("ещё нет данных");
    let pending_return_count = execctl_resume_obligation["pending_return_count"]
        .as_u64();
    Some(compact_prompt_fragment(
        &format!(
            "{}({}): {} -> {}",
            format_optional_text_for_prompt(resume_state),
            format_optional_u64_for_prompt(pending_return_count),
            required_headline,
            required_next_step
        ),
        96,
    ))
}

fn summarize_startup_next_action_for_prompt(value: &Value) -> Option<String> {
    let action_kind = value["action_kind"]
        .as_str()
        .filter(|item| !item.is_empty())?;
    let headline = value["headline"]
        .as_str()
        .and_then(|item| optional_non_empty_text(Some(item)))
        .map(normalize_client_budget_advisory_text)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let compact_headline = compact_prompt_fragment(&headline, 48);
    match action_kind {
        "resume_required_return_task" => Some(format!("Сначала: вернись: {compact_headline}")),
        "wait_for_global_client_budget_recovery" => {
            Some(format!("Сначала: жди budget: {compact_headline}"))
        }
        "continue_active_workline" => None,
        _ => Some(format!("Сначала: {compact_headline}")),
    }
}

fn render_chat_start_prompt(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    handoff_summary: &Value,
    restore_node: Option<&Value>,
) -> String {
    let headline = format_optional_text_for_human(handoff_summary["headline"].as_str());
    let compact_headline = compact_prompt_fragment(&headline, 64);
    let next_step = normalize_next_step_value(handoff_summary["next_step"].as_str().unwrap_or_default())
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let compact_next_step = compact_prompt_fragment(&next_step, 80);
    let current_goal = restore_node
        .and_then(|value| value["current_goal"].as_str())
        .and_then(|value| optional_non_empty_text(Some(value)))
        .unwrap_or(&headline);
    let materialized_summary = restore_node.and_then(summarize_startup_materialized_notes);
    let execctl_resume_obligation = restore_node
        .map(|value| summarize_execctl_resume_obligation(&value["execctl_resume_contract"]))
        .unwrap_or_else(|| default_execctl_resume_obligation(None, None));
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
        .and_then(|value| optional_non_empty_text(Some(value)))
        .map(ToOwned::to_owned);
    let skill_execution_card_summary = restore_node
        .and_then(|value| value["skill_execution_card_summary"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| compact_prompt_fragment(value, 96));
    let workspace_restore_pack_summary = restore_node
        .and_then(|value| value["workspace_restore_pack_summary"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| compact_prompt_fragment(value, 96));
    let pending_return_prompt_summary = summarize_pending_return_for_prompt(restore_node);
    let execctl_contract_prompt_summary =
        summarize_execctl_resume_contract_for_prompt(restore_node, &execctl_resume_obligation);
    let required_task_set_prompt_summary = summarize_required_task_set_for_prompt(restore_node);
    let execctl_resume_state = execctl_resume_obligation["resume_state"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)));
    let action_kind = startup_next_action["action_kind"].as_str();
    let compact_materialized_summary = materialized_summary
        .as_deref()
        .map(|value| compact_prompt_fragment(value, 56))
        .filter(|_| action_kind == Some("continue_active_workline"));
    let mut lines = vec![
        "CHAT_START_RESTORE".to_string(),
        format!("Линия: {compact_headline}"),
        format!("Шаг: {compact_next_step}"),
    ];
    if let Some(value) = skill_execution_card_summary {
        lines.push(format!("Карточка: {value}"));
    }
    if let Some(value) = workspace_restore_pack_summary {
        lines.push(format!("Workspace: {value}"));
    }
    if let Some(value) = pending_return_prompt_summary {
        lines.push(format!("Возврат: {value}"));
    }
    if let Some(value) = execctl_contract_prompt_summary {
        lines.push(format!("Контракт: {value}"));
    }
    if let Some(value) = required_task_set_prompt_summary {
        lines.push(format!("Задачи: {value}"));
    }
    if let Some(value) = compact_materialized_summary {
        lines.push(format!("Сделано: {value}"));
    }
    if let Some(value) = summarize_startup_next_action_for_prompt(&startup_next_action) {
        lines.push(value);
    }
    if let Some(value) = blocked_reply_text
        .as_deref()
        .map(normalize_client_budget_advisory_text)
        .map(|value| compact_prompt_fragment(&value, 96))
        .filter(|_| {
            matches!(
                action_kind,
                Some("rotate_chat_for_client_budget")
                    | Some("wait_for_global_client_budget_recovery")
            )
        })
    {
        lines.push(format!("Только rotate: {value}"));
    }
    let _ = action_kind;
    if execctl_resume_state == Some("pending_return_queue_present") {
        lines.push("Сначала закрой возврат.".to_string());
    }
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
        format_optional_text_for_human(node["headline"].as_str())
    );
    println!(
        "- Обязательный следующий шаг: {}",
        format_optional_text_for_human(node["next_step"].as_str())
    );
    if let Some(value) = node["materialized_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Что уже materialized: {value}");
    }
    if let Some(value) = node["skill_execution_card_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Исполнимая procedural-карточка: {value}");
    }
    if let Some(value) = node["recent_actions_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Недавние действия: {value}");
    }
    if let Some(value) = node["active_files_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Активные файлы: {value}");
    }
    if let Some(value) = node["workspace_graph_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Структурный граф рабочей области: {value}");
    }
    if let Some(value) = node["workspace_restore_pack_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Workspace restore pack: {value}");
    }
    if let Some(value) = node["pending_return_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Незавершённые линии к возврату: {value}");
    }
    if let Some(value) = node["execctl_resume_contract_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Контракт возврата ExecCtl: {value}");
    }
    if let Some(value) = summarize_required_task_set_for_human_surface(node) {
        println!("- Обязательный набор задач из machine-readable restore: {value}");
    }
    if let Some(value) = node["startup_next_action_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Первое обязательное действие после startup: {value}");
    }
    if let Some(value) = node["execctl_active_lease_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Активный lease ExecCtl: {value}");
    }
    if let Some(value) = node["included_reasons_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Почему вошёл последний контекст: {value}");
    }
    if let Some(value) = node["excluded_reasons_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Почему часть не вошла: {value}");
    }
    if let Some(value) = node["open_questions_summary"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        println!("- Открытые вопросы: {value}");
    }
    println!(
        "- Thread count в temporal index: {}",
        format_optional_u64_for_human(node["thread_count"].as_u64())
    );
    if let Some(prompt_text) = node["prompt_text"]
        .as_str()
        .and_then(|value| optional_non_empty_text(Some(value)))
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
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        return Some(value.to_string());
    }
    if let Some(value) = chat_tail
        .summary_headline
        .as_deref()
        .and_then(|value| optional_non_empty_text(Some(value)))
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
        .and_then(|value| optional_non_empty_text(Some(value)))
    {
        return Some(value.to_string());
    }
    if let Some(value) = chat_tail
        .summary_next_step
        .as_deref()
        .and_then(|value| optional_non_empty_text(Some(value)))
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
        if let Some(status_label) = display_client_budget_status_label(guard) {
            notes.push(format!("Client-budget advisory signal: {}.", status_label));
        }
        if let Some(last_request) = optional_non_empty_text(guard["last_request"].as_str()) {
            notes.push(format!("Последний запрос в модель: {last_request}."));
        }
        if let Some(client_limits) = optional_non_empty_text(guard["client_limits"].as_str()) {
            notes.push(format!(
                "{}: {client_limits}.",
                client_limits_surface_label(guard)
            ));
        }
    }
    if let Some(items) = restore_node["materialized_notes"].as_array() {
        for item in items.iter().filter_map(Value::as_str) {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("Client-budget advisory signal:")
                || trimmed.starts_with("Client-budget guard:")
                || trimmed.starts_with("Последний запрос в модель:")
                || trimmed.starts_with("Лимит клиента сейчас:")
                || trimmed.starts_with("Последний observed лимит клиента:")
            {
                continue;
            }
            notes.push(normalize_client_budget_advisory_text(trimmed));
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
        if let Some(text) = optional_non_empty_text(item["headline"].as_str()) {
            entries.push(text.to_string());
        } else if let Some(text) = optional_non_empty_text(item["summary"].as_str()) {
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
        .filter_map(|item| optional_non_empty_text(Some(item)))
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
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope(
        db,
        "continuity_handoff",
        "continuity_handoff",
        &project.code,
        &namespace.code,
        Some(200),
    )
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
        let mut summary = normalized_handoff_summary_json(
            snapshot.payload["continuity_handoff"]["headline"].as_str(),
            snapshot.payload["continuity_handoff"]["next_step"].as_str(),
        );
        summary["local_path"] = snapshot.payload["continuity_handoff"]["local_path"].clone();
        summary
    }))
}

async fn latest_continuity_import_snapshot(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<postgres::ObservabilitySnapshotRecord>> {
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope(
        db,
        "continuity_import",
        "continuity_import",
        &project.code,
        &namespace.code,
        Some(200),
    )
    .await?;
    Ok(latest_scoped_snapshot(
        &snapshots,
        "continuity_import",
        &project.code,
        &namespace.code,
        |_| true,
    )
    .cloned())
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
    let repo_root = match args.repo_root.as_ref() {
        Some(repo_root) => repo_root.clone(),
        None => crate::config::discover_repo_root(None)
            .map_err(|_| anyhow!("continuity startup requires --project or --repo-root"))?,
    };
    let repo_root = canonical_string(&repo_root)?;
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
            .map(|path| path.display().to_string()),
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
            if let Some(path) = optional_non_empty_text(Some(value)) {
                paths.push(PathBuf::from(path));
            }
        } else if let Some(end) = start {
            let start_index = prefix.len();
            if line.len() > start_index && end > start_index {
                if let Some(path) =
                    optional_non_empty_text(Some(&line.trim_start()[start_index..end]))
                {
                    paths.push(PathBuf::from(path));
                }
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

fn shell_join_command(args: &[&str]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use crate::cli::ContinuityCompactChatArgs;
    use super::{
        COMPACT_CHAT_AUTO_LAUNCH_ENV, ContinuityStartupContext, StartupRuntimeStateAudit,
        apply_compact_chat_host_launch_completion, apply_compact_chat_host_launch_failed,
        build_blocked_continuity_answer_payload, build_chat_start_restore,
        build_compact_chat_clean_launch_surface_with_vscode_contracts,
        build_continuity_answer_payload, build_continuity_canonical_eval,
        build_continuity_restore_payload, build_continuity_startup_payload,
        build_startup_runtime_state_artifact, build_startup_runtime_state_cli_json,
        build_vscode_code_chat_launch_command_with_binary, client_budget_guard_blocks_reply,
        compact_chat_prompt_artifact_path, compact_startup_runtime_execctl_resume_obligation,
        compact_startup_runtime_startup_next_action, continuity_answer_requires_live_reply_gate,
        continuity_replay_guard_probes, continuity_snapshot_semantic_epoch_ms,
        continuity_temporal_lookup_probes, degradation_proof_scenarios, enrich_thread_index_file,
        extract_next_step_from_text, fake_continuity_handoff_snapshot,
        fake_continuity_import_snapshot, inspect_startup_runtime_state, is_meta_continuity_handoff,
        latest_scoped_snapshot, maybe_launch_compact_chat_host,
        maybe_launch_compact_chat_host_with_auto_launch_policy, parse_chat_reference_spec,
        render_direct_answer, resolve_answer_intent, shell_quote,
        startup_runtime_state_artifact_path, summarize_execctl_resume_obligation,
        summarize_required_task_set_for_human_surface, workspace_bound_vscode_chat_profile_name,
        write_compact_chat_prompt_artifact,
    };
    use crate::cli::ContinuityThreadIndexEnrichArgs;
    use crate::codex_threads::{ChatTail, ThreadTimeSliceSummary, TranscriptMessage};
    use crate::postgres::{NamespaceRecord, ProjectRecord};
    use crate::working_state;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};

    struct EnvVarRestore {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => {
                    // SAFETY: tests mutate this process-wide env var only while holding
                    // compact_chat_auto_launch_env_lock.
                    unsafe { std::env::set_var(self.key, value) };
                }
                None => {
                    // SAFETY: tests mutate this process-wide env var only while holding
                    // compact_chat_auto_launch_env_lock.
                    unsafe { std::env::remove_var(self.key) };
                }
            }
        }
    }

    fn compact_chat_auto_launch_env_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn set_env_var_for_test(key: &'static str, value: &str) -> EnvVarRestore {
        let previous = std::env::var_os(key);
        // SAFETY: tests mutate this process-wide env var only while holding
        // compact_chat_auto_launch_env_lock.
        unsafe { std::env::set_var(key, value) };
        EnvVarRestore { key, previous }
    }

    #[test]
    fn compact_chat_launch_command_uses_code_chat_cli_and_prompt_artifact() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let command = build_vscode_code_chat_launch_command_with_binary(
            "code",
            repo_root,
            &prompt_path,
            true,
        );
        assert!(command.contains("code chat --mode agent --new-window --profile "));
        assert!(command.contains("/home/art/agent-memory-index"));
        assert!(command.contains(".amai/continuity/compact-chat-prompt.txt"));
        assert!(command.contains("Amai-agent-memory-index-"));
    }

    #[test]
    fn compact_chat_clean_launch_surface_is_vscode_only_command_contract() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let vscode_surface = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "vscode", "display_name": "VS Code"}),
            repo_root,
            Some(&prompt_path),
            Some("code"),
            None,
            false,
            false,
        );

        assert_eq!(vscode_surface["status"], json!("launch_command_available"));
        assert_eq!(vscode_surface["supported_auto_launch"], json!(true));
        assert_eq!(
            vscode_surface["command_kind"],
            json!("vscode_code_chat_cli")
        );
        assert_eq!(
            vscode_surface["proof_boundary"],
            json!("command_contract_only")
        );
        assert_eq!(
            vscode_surface["ux_verdict"],
            json!("not_seamless_until_live_client_proof")
        );
        assert!(
            vscode_surface["launch_clean_chat_command"]
                .as_str()
                .is_some_and(|value| value.contains("code chat --mode agent --new-window"))
        );
        assert!(
            vscode_surface["launch_clean_chat_fallback_command"]
                .as_str()
                .is_some_and(|value| value.contains("code chat --mode agent --reuse-window"))
        );

        let codex_surface = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "codex", "display_name": "Codex"}),
            repo_root,
            Some(&prompt_path),
            Some("code"),
            None,
            false,
            false,
        );
        assert_eq!(codex_surface["status"], json!("manual_only"));
        assert_eq!(codex_surface["supported_auto_launch"], json!(false));
        assert!(codex_surface["launch_clean_chat_command"].is_null());
        assert_eq!(
            codex_surface["unavailable_reason"],
            json!("client_has_no_automatic_clean_chat_bridge")
        );
        assert_eq!(
            codex_surface["ux_verdict"],
            json!("not_seamless_until_live_client_proof")
        );
    }

    #[test]
    fn compact_chat_clean_launch_surface_reports_missing_vscode_cli_and_prompt() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let missing_cli = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "vscode", "display_name": "VS Code"}),
            repo_root,
            Some(&prompt_path),
            None,
            None,
            false,
            false,
        );

        assert_eq!(missing_cli["status"], json!("bridge_unavailable"));
        assert_eq!(missing_cli["supported_auto_launch"], json!(true));
        assert_eq!(
            missing_cli["unavailable_reason"],
            json!("vscode_code_cli_unavailable")
        );
        assert!(missing_cli["launch_clean_chat_command"].is_null());

        let missing_prompt = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "vscode", "display_name": "VS Code"}),
            repo_root,
            None,
            Some("code"),
            None,
            false,
            false,
        );
        assert_eq!(missing_prompt["status"], json!("bridge_unavailable"));
        assert_eq!(
            missing_prompt["unavailable_reason"],
            json!("prompt_artifact_unavailable")
        );
        assert!(missing_prompt["launch_clean_chat_command"].is_null());
    }

    #[test]
    fn compact_chat_clean_launch_surface_recovers_when_prompt_contract_is_rebuilt() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let client_surface = json!({"client_key": "vscode", "display_name": "VS Code"});

        let missing_prompt = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &client_surface,
            repo_root,
            None,
            Some("code"),
            None,
            false,
            false,
        );
        assert_eq!(missing_prompt["status"], json!("bridge_unavailable"));
        assert_eq!(
            missing_prompt["unavailable_reason"],
            json!("prompt_artifact_unavailable")
        );

        let rebuilt_prompt = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &client_surface,
            repo_root,
            Some(&prompt_path),
            Some("code"),
            None,
            false,
            false,
        );
        assert_eq!(rebuilt_prompt["status"], json!("launch_command_available"));
        assert!(rebuilt_prompt["unavailable_reason"].is_null());
        assert!(
            rebuilt_prompt["launch_clean_chat_command"]
                .as_str()
                .is_some_and(|value| value.contains(".amai/continuity/compact-chat-prompt.txt"))
        );
    }

    #[test]
    fn workspace_bound_vscode_chat_profile_name_is_repo_scoped_and_deterministic() {
        let first =
            workspace_bound_vscode_chat_profile_name(Path::new("/home/art/agent-memory-index"));
        let second = workspace_bound_vscode_chat_profile_name(Path::new("/home/art/Bug-Bounty"));
        assert_ne!(first, second);
        assert!(first.starts_with("Amai-agent-memory-index-"));
        assert!(second.starts_with("Amai-Bug-Bounty-"));
        assert_eq!(
            first,
            workspace_bound_vscode_chat_profile_name(Path::new("/home/art/agent-memory-index"))
        );
    }

    #[test]
    fn compact_chat_clean_launch_surface_keeps_public_vscode_uri_bridge_fail_closed_without_live_verification() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let surface = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "vscode", "display_name": "VS Code"}),
            repo_root,
            Some(&prompt_path),
            Some("code"),
            Some("xdg-open"),
            true,
            false,
        );

        assert_eq!(surface["status"], json!("launch_command_available"));
        assert_eq!(surface["supported_auto_launch"], json!(true));
        assert_eq!(surface["command_kind"], json!("vscode_code_chat_cli"));
        assert_eq!(surface["proof_boundary"], json!("command_contract_only"));
        assert_eq!(surface["bridge_live_verification_state"], json!("missing"));
        assert!(
            surface["launch_clean_chat_command"]
                .as_str()
                .is_some_and(|value| value.contains("code chat --mode agent --new-window"))
        );
    }

    #[test]
    fn compact_chat_clean_launch_surface_prefers_public_vscode_uri_bridge_when_live_verified() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let surface = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "vscode", "display_name": "VS Code"}),
            repo_root,
            Some(&prompt_path),
            Some("code"),
            Some("xdg-open"),
            true,
            true,
        );

        assert_eq!(surface["status"], json!("launch_command_available"));
        assert_eq!(surface["supported_auto_launch"], json!(true));
        assert_eq!(surface["command_kind"], json!("vscode_uri_amai_bridge"));
        assert_eq!(surface["proof_boundary"], json!("public_bridge_live_contract_only"));
        assert_eq!(surface["bridge_live_verification_state"], json!("verified"));
        assert!(
            surface["launch_clean_chat_command"]
                .as_str()
                .is_some_and(|value| value.contains("code --open-url 'vscode://amai.amai-vscode-bridge/open-clean-chat"))
        );
        assert!(
            surface["launch_clean_chat_fallback_command"]
                .as_str()
                .is_some_and(|value| value.contains("code chat --mode agent --reuse-window"))
        );
        assert!(
            surface["bridge_result_file"]
                .as_str()
                .is_some_and(|value| value.contains(".amai/continuity/compact-chat-launch-result.json"))
        );
    }

    #[test]
    fn compact_chat_clean_launch_surface_uses_xdg_open_fallback_when_code_binary_is_missing() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let prompt_path = compact_chat_prompt_artifact_path(repo_root);
        let surface = build_compact_chat_clean_launch_surface_with_vscode_contracts(
            &json!({"client_key": "vscode", "display_name": "VS Code"}),
            repo_root,
            Some(&prompt_path),
            None,
            Some("xdg-open"),
            true,
            true,
        );

        assert_eq!(surface["status"], json!("launch_command_available"));
        assert_eq!(surface["command_kind"], json!("vscode_uri_amai_bridge"));
        assert!(
            surface["launch_clean_chat_command"]
                .as_str()
                .is_some_and(|value| value.contains("xdg-open 'vscode://amai.amai-vscode-bridge/open-clean-chat"))
        );
        assert!(surface["launch_clean_chat_fallback_command"].is_null());
    }

    #[test]
    fn write_compact_chat_prompt_artifact_persists_prompt_text() {
        let unique = format!(
            "amai-compact-chat-prompt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let repo_root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo_root).expect("repo root");
        let prompt_text = "CHAT_START_RESTORE\nЛиния: test";
        let prompt_path =
            write_compact_chat_prompt_artifact(repo_root.as_path(), prompt_text).expect("write");
        assert_eq!(fs::read_to_string(&prompt_path).expect("read"), prompt_text);
        fs::remove_dir_all(&repo_root).expect("cleanup temp repo");
    }

    #[test]
    fn apply_compact_chat_host_launch_completion_clears_manual_host_action() {
        let mut payload = json!({
            "continuity_compact_chat": {
                "operator_notice": {
                    "message_text": "Подготовлен compact restore",
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                    "note": "manual step still required",
                    "launch_clean_chat_command": "code chat ..."
                },
                "operator_flow": {}
            }
        });

        apply_compact_chat_host_launch_completion(&mut payload, "implicit_default", false);

        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("requested")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["mode"],
            json!("implicit_default")
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"].is_null()
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"],
            json!(
                "Новая чистая рабочая поверхность запрошена автоматически; startup restore уже передан в новую рабочую поверхность."
            )
        );
        assert!(
            payload["continuity_compact_chat"]["operator_flow"]["fresh_surface_summary"].is_null()
        );
    }

    #[test]
    fn apply_compact_chat_host_launch_failed_restores_manual_fallback_truthfully() {
        let mut payload = json!({
            "continuity_compact_chat": {
                "operator_notice": {
                    "message_text": "Подготовлен compact restore",
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                    "note": "manual step still required",
                    "launch_clean_chat_command": "code chat ..."
                },
                "operator_flow": {}
            }
        });

        let error = anyhow::anyhow!("code chat launch failed");
        apply_compact_chat_host_launch_failed(&mut payload, "implicit_default", &error);

        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("launch_failed")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["reason"],
            json!("code chat launch failed")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"]
                .as_str()
                .is_some_and(|value| value.contains("manual fallback"))
        );
        assert!(
            payload["continuity_compact_chat"]["operator_flow"]["fresh_surface_summary"].is_null()
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_respects_policy_no_auto_launch() {
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "client_key": "codex",
                    "display_name": "Codex",
                    "startup_instruction_path": "/home/art/agent-memory-index/AGENTS.md",
                    "startup_instruction_mode": "managed_append_block",
                    "fresh_chat_assist_summary": "Для front-door новой чистой рабочей поверхности используй ./scripts/reconnect_local.sh --client codex или ./scripts/amai_exec.sh bootstrap reconnect --client codex --yes.",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client codex",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client codex --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": "code chat ...",
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                }
            }
        });

        let launched = maybe_launch_compact_chat_host_with_auto_launch_policy(
            &mut payload,
            true,
            false,
            false,
        )
        .await
        .expect("policy block should not fail");

        assert!(!launched);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("disabled_by_policy")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["reason"],
            json!("auto_launch_disabled_by_policy")
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"]
                .as_str()
                .is_some_and(|value| value.contains("политик"))
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_marks_bridge_unavailable_without_command() {
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "display_name": "Codex",
                    "startup_instruction_path": "/home/art/agent-memory-index/AGENTS.md",
                    "startup_instruction_mode": "managed_append_block",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client codex",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client codex --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": Value::Null,
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                }
            }
        });

        let launched =
            maybe_launch_compact_chat_host_with_auto_launch_policy(&mut payload, true, false, true)
                .await
                .expect("no-command path should not fail");

        assert!(!launched);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("bridge_unavailable")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["reason"],
            json!("launch_command_unavailable")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["attempted"],
            json!(false)
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"]
                .as_str()
                .is_some_and(|value| value.contains("недоступен"))
        );
        assert_eq!(
            payload["continuity_compact_chat"]["client_surface"]["startup_instruction_path"],
            json!("/home/art/agent-memory-index/AGENTS.md")
        );
        assert!(
            payload["continuity_compact_chat"]["client_surface"]["reconnect_shell_command"]
                .as_str()
                .is_some_and(|value| value.contains("./scripts/reconnect_local.sh --client codex"))
        );
        assert!(
            payload["continuity_compact_chat"]["operator_flow"]["fresh_surface_summary"].is_null()
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_preserves_client_launch_gap_reason() {
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "client_key": "hermes",
                    "display_name": "Hermes",
                    "startup_instruction_path": "/home/art/agent-memory-index/.hermes.md",
                    "startup_instruction_mode": "managed_workspace_file",
                    "fresh_chat_assist_summary": "Для front-door новой чистой рабочей поверхности используй ./scripts/reconnect_local.sh --client hermes или ./scripts/amai_exec.sh bootstrap reconnect --client hermes --yes.",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client hermes",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client hermes --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": Value::Null,
                    "clean_chat_launch": {
                        "status": "manual_only",
                        "unavailable_reason": "client_has_no_automatic_clean_chat_bridge",
                        "ux_verdict": "not_seamless_until_live_client_proof"
                    },
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                }
            }
        });

        let launched =
            maybe_launch_compact_chat_host_with_auto_launch_policy(&mut payload, true, false, true)
                .await
                .expect("manual-only client launch gap should not fail");

        assert!(!launched);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("bridge_unavailable")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["reason"],
            json!("client_has_no_automatic_clean_chat_bridge")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_marks_launch_not_requested() {
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "client_key": "generic",
                    "display_name": "Generic",
                    "startup_instruction_path": "/home/art/agent-memory-index/tmp/onboarding/generic-amai-startup.md",
                    "startup_instruction_mode": "manual_snippet_only",
                    "fresh_chat_assist_summary": "Для front-door новой чистой рабочей поверхности используй ./scripts/reconnect_local.sh --client generic или ./scripts/amai_exec.sh bootstrap reconnect --client generic --yes.",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client generic",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client generic --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": "code chat ...",
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                }
            }
        });

        let launched = maybe_launch_compact_chat_host_with_auto_launch_policy(
            &mut payload,
            false,
            false,
            true,
        )
        .await
        .expect("non-launch path should not fail");

        assert!(!launched);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("available_not_requested")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["reason"],
            json!("launch_not_requested")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"]
                .as_str()
                .is_some_and(|value| value.contains("не запрошен"))
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["note"]
                .as_str()
                .is_some_and(|value| value.contains("не просил"))
        );
        assert_eq!(
            payload["continuity_compact_chat"]["client_surface"]["startup_instruction_path"],
            json!("/home/art/agent-memory-index/tmp/onboarding/generic-amai-startup.md")
        );
        assert!(
            payload["continuity_compact_chat"]["client_surface"]["reconnect_shell_command"]
                .as_str()
                .is_some_and(
                    |value| value.contains("./scripts/reconnect_local.sh --client generic")
                )
        );
        assert!(
            payload["continuity_compact_chat"]["operator_flow"]["fresh_surface_summary"].is_null()
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_marks_requested_when_opt_in_command_succeeds() {
        let unique = format!(
            "amai-compact-chat-launch-success-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let marker_path = std::env::temp_dir().join(unique);
        let marker_display = marker_path.display().to_string();
        let launch_command = if cfg!(windows) {
            format!("echo launched>{marker_display}")
        } else {
            format!("printf launched > {}", shell_quote(&marker_display))
        };
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "display_name": "Codex",
                    "startup_instruction_path": "/home/art/agent-memory-index/AGENTS.md",
                    "startup_instruction_mode": "managed_append_block",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client codex",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client codex --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": launch_command,
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                },
                "operator_flow": {}
            }
        });

        let launched =
            maybe_launch_compact_chat_host_with_auto_launch_policy(&mut payload, true, false, true)
                .await
                .expect("successful opt-in command should not fail");

        assert!(launched);
        assert_eq!(
            fs::read_to_string(&marker_path).expect("launch marker"),
            "launched"
        );
        let _ = fs::remove_file(&marker_path);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("requested")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["attempted"],
            json!(true)
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["mode"],
            json!("implicit_default")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"]
                .as_str()
                .is_some_and(|value| value.contains("проверь"))
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["note"]
                .as_str()
                .is_some_and(|value| value.contains("bounded proof"))
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_public_wrapper_reads_auto_launch_env() {
        let _guard = compact_chat_auto_launch_env_lock().lock().await;
        let _restore = set_env_var_for_test(COMPACT_CHAT_AUTO_LAUNCH_ENV, "1");
        let unique = format!(
            "amai-compact-chat-wrapper-launch-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let marker_path = std::env::temp_dir().join(unique);
        let marker_display = marker_path.display().to_string();
        let launch_command = if cfg!(windows) {
            format!("echo wrapper-launched>{marker_display}")
        } else {
            format!("printf wrapper-launched > {}", shell_quote(&marker_display))
        };
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "display_name": "Codex",
                    "startup_instruction_path": "/home/art/agent-memory-index/AGENTS.md",
                    "startup_instruction_mode": "managed_append_block",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client codex",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client codex --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": launch_command,
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                },
                "operator_flow": {}
            }
        });

        let launched = maybe_launch_compact_chat_host(&mut payload, true, false)
            .await
            .expect("wrapper should honor opt-in env");

        assert!(launched);
        assert_eq!(
            fs::read_to_string(&marker_path).expect("wrapper launch marker"),
            "wrapper-launched"
        );
        let _ = fs::remove_file(&marker_path);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("requested")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
    }

    #[tokio::test]
    async fn maybe_launch_compact_chat_host_marks_launch_failed_when_opt_in_command_fails() {
        let launch_command = if cfg!(windows) {
            "echo launch-failed 1>&2 & exit /B 7".to_string()
        } else {
            "printf launch-failed >&2; exit 7".to_string()
        };
        let mut payload = json!({
            "continuity_compact_chat": {
                "client_surface": {
                    "display_name": "Codex",
                    "startup_instruction_path": "/home/art/agent-memory-index/AGENTS.md",
                    "startup_instruction_mode": "managed_append_block",
                    "reconnect_shell_command": "./scripts/reconnect_local.sh --client codex",
                    "reconnect_bootstrap_command": "./scripts/amai_exec.sh bootstrap reconnect --client codex --yes"
                },
                "project": {
                    "repo_root": "/home/art/agent-memory-index"
                },
                "operator_notice": {
                    "prompt_file": "/tmp/compact-chat-prompt.txt",
                    "launch_clean_chat_command": launch_command,
                    "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
                },
                "operator_flow": {}
            }
        });

        let launched =
            maybe_launch_compact_chat_host_with_auto_launch_policy(&mut payload, true, false, true)
                .await
                .expect("failed opt-in command is represented in payload");

        assert!(!launched);
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["status"],
            json!("launch_failed")
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["attempted"],
            json!(true)
        );
        assert_eq!(
            payload["continuity_compact_chat"]["host_launch"]["mode"],
            json!("implicit_default")
        );
        assert!(
            payload["continuity_compact_chat"]["host_launch"]["reason"]
                .as_str()
                .is_some_and(|value| value.contains("launch-failed"))
        );
        assert_eq!(
            payload["continuity_compact_chat"]["operator_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            payload["continuity_compact_chat"]["operator_notice"]["message_text"]
                .as_str()
                .is_some_and(|value| value.contains("manual fallback"))
        );
    }

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
                    {"headline": "Проверили новую чистую рабочую поверхность на реальном старте"},
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
        assert!(answer.contains("Последние действия: Проверили новую чистую рабочую поверхность на реальном старте; Усилили startup restore pack"));
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
    fn render_direct_answer_ignores_whitespace_thread_summary_fields() {
        let handoff = json!({
            "headline": "Current project line",
            "next_step": "Current project next step."
        });
        let chat_tail = ChatTail {
            thread_id: "thread-2".to_string(),
            title: "чат про continuity".to_string(),
            summary_headline: Some("   ".to_string()),
            summary_next_step: Some("   ".to_string()),
            selected_time_slice: Some(ThreadTimeSliceSummary {
                started_at: "2026-03-19T11:59:20+03:00".to_string(),
                ended_at: "2026-03-19T12:01:10+03:00".to_string(),
                started_at_epoch_s: 1,
                ended_at_epoch_s: 2,
                user_anchor: "разбирали temporal lookup".to_string(),
                assistant_anchor: "про temporal lookup".to_string(),
                summary_headline: "   ".to_string(),
                summary_next_step: "   ".to_string(),
            }),
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
    fn continuity_handoff_document_index_refresh_due_without_previous_refresh() {
        assert!(super::continuity_handoff_document_index_refresh_due_from_previous(None, 10_000));
    }

    #[test]
    fn continuity_handoff_document_index_refresh_due_blocks_recent_refresh() {
        let previous_refresh_epoch_ms = 10_000;
        let still_recent_epoch_ms = previous_refresh_epoch_ms
            + super::CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_MIN_INTERVAL_MS
            - 1;
        assert!(
            !super::continuity_handoff_document_index_refresh_due_from_previous(
                Some(previous_refresh_epoch_ms),
                still_recent_epoch_ms
            )
        );
    }

    #[test]
    fn continuity_handoff_document_index_refresh_due_allows_refresh_after_interval() {
        let previous_refresh_epoch_ms = 10_000;
        let due_epoch_ms = previous_refresh_epoch_ms
            + super::CONTINUITY_HANDOFF_DOCUMENT_INDEX_REFRESH_MIN_INTERVAL_MS
            + 1;
        assert!(
            super::continuity_handoff_document_index_refresh_due_from_previous(
                Some(previous_refresh_epoch_ms),
                due_epoch_ms
            )
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
            visibility_scope: "project_shared".to_string(),
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
        assert!(payload["continuity_answer"]["chat_lookup"]["messages_count"].is_null());
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
            visibility_scope: "project_shared".to_string(),
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
    fn continuity_answer_payload_normalizes_empty_projection_text_fields() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });
        let chat_tail = ChatTail {
            thread_id: "thread-2".to_string(),
            title: "чат про continuity".to_string(),
            summary_headline: Some("   ".to_string()),
            summary_next_step: Some(String::new()),
            selected_time_slice: Some(ThreadTimeSliceSummary {
                started_at: "2026-03-21T12:00:00Z".to_string(),
                ended_at: "2026-03-21T12:05:00Z".to_string(),
                started_at_epoch_s: 1,
                ended_at_epoch_s: 2,
                summary_headline: "   ".to_string(),
                summary_next_step: String::new(),
                user_anchor: "пользователь спросил".to_string(),
                assistant_anchor: "агент ответил".to_string(),
            }),
            messages: vec![TranscriptMessage {
                role: "assistant".to_string(),
                text: "Закончили на temporal contour.".to_string(),
            }],
        };

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
            "answer",
        )
        .expect("payload");

        assert_eq!(
            payload["continuity_answer"]["handoff_summary"]["headline"],
            json!("ещё нет данных")
        );
        assert_eq!(
            payload["continuity_answer"]["handoff_summary"]["next_step"],
            json!("ещё нет данных")
        );
        assert_eq!(
            payload["continuity_answer"]["chat_lookup"]["summary_headline"],
            Value::Null
        );
        assert_eq!(
            payload["continuity_answer"]["chat_lookup"]["summary_next_step"],
            Value::Null
        );
        assert_eq!(
            payload["continuity_answer"]["chat_lookup"]["selected_time_slice"]["summary_headline"],
            Value::Null
        );
        assert_eq!(
            payload["continuity_answer"]["chat_lookup"]["selected_time_slice"]["summary_next_step"],
            Value::Null
        );
    }

    #[test]
    fn continuity_answer_json_marks_missing_exact_time_as_fail_closed_hit() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
    fn blocked_continuity_answer_gate_ignores_rotate_soon_advisory_only() {
        let client_budget_guard = json!({
            "status_label": "сожми текущий чат",
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "must_rotate_before_reply": false,
                "blocking": false
            }
        });

        assert!(!client_budget_guard_blocks_reply(&client_budget_guard));
    }

    #[test]
    fn continuity_answer_live_reply_gate_is_only_required_for_live_source_kinds() {
        assert!(continuity_answer_requires_live_reply_gate(
            "live_continuity_startup"
        ));
        assert!(!continuity_answer_requires_live_reply_gate(
            "operator_continuity_startup"
        ));
        assert!(!continuity_answer_requires_live_reply_gate(
            "proof_continuity_startup"
        ));
        assert!(!continuity_answer_requires_live_reply_gate(
            "verify_continuity_startup"
        ));
    }

    #[test]
    fn continuity_answer_blocked_payload_surfaces_rotate_only_contract() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
        let restore = json!({
            "working_state_restore": {
                "latest_decision_trace": {
                    "included": [],
                    "not_included": []
                }
            }
        });
        let client_budget_guard = json!({
            "status_label": "сожми текущий чат сейчас",
            "should_rotate_chat_now": true,
            "reply_execution_gate": {
                "must_rotate_before_reply": true,
                "blocking_reply_contract": {
                    "response_kind": "rotate_chat_only",
                    "template": working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE
                }
            }
        });

        assert!(!client_budget_guard_blocks_reply(&client_budget_guard));

        let payload = build_blocked_continuity_answer_payload(
            &project,
            &namespace,
            &handoff,
            Some(&restore),
            "last_chat",
            Some("на чем остановились"),
            None,
            None,
            2,
            false,
            1,
            working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE,
            &client_budget_guard,
            "live_continuity_startup",
        )
        .expect("payload");

        assert_eq!(
            payload["continuity_answer"]["blocked_by_client_budget_guard"],
            json!(true)
        );
        assert_eq!(
            payload["continuity_answer"]["blocking_reply_contract"]["response_kind"],
            json!("rotate_chat_only")
        );
        assert_eq!(
            payload["continuity_answer"]["canonical_eval"]["verdict_counts"]["hit_correct_target"]
                .as_u64(),
            Some(1)
        );
    }

    #[test]
    fn blocked_continuity_answer_payload_normalizes_empty_handoff_summary() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });
        let client_budget_guard = json!({
            "status_label": "сожми текущий чат сейчас",
            "should_rotate_chat_now": true,
            "reply_execution_gate": {
                "must_rotate_before_reply": true,
                "blocking_reply_contract": {
                    "response_kind": "rotate_chat_only",
                    "template": working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE
                }
            }
        });

        let payload = build_blocked_continuity_answer_payload(
            &project,
            &namespace,
            &handoff,
            None,
            "last_chat",
            Some("на чем остановились"),
            None,
            None,
            2,
            false,
            1,
            working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE,
            &client_budget_guard,
            "live_continuity_startup",
        )
        .expect("payload");

        assert_eq!(
            payload["continuity_answer"]["handoff_summary"]["headline"],
            json!("ещё нет данных")
        );
        assert_eq!(
            payload["continuity_answer"]["handoff_summary"]["next_step"],
            json!("ещё нет данных")
        );
        assert!(payload["continuity_answer"]["chat_lookup"]["messages_count"].is_null());
    }

    #[test]
    fn continuity_restore_payload_normalizes_empty_handoff_summary() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
                    "thread_count": 18
                }
            }
        });
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "goal",
                "next_step": "step",
                "restore_confidence": "high",
                "state_lineage": {
                    "authoritative_event_id": "event-1"
                }
            },
            "workspace_restore_pack": {
                "active_commitments": [],
                "active_constraints": [],
                "important_artifacts": [],
                "procedural_restore_policy": {
                    "raw_procedural_archive_forbidden": true
                }
            }
        });
        let chat_start_restore = json!({
            "chat_start_restore": {
                "headline": "   ",
                "next_step": "",
                "prompt_text": "CHAT_START_RESTORE\nещё нет данных\nещё нет данных"
            }
        });

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
            payload["continuity_restore"]["handoff_summary"]["headline"],
            json!("ещё нет данных")
        );
        assert_eq!(
            payload["continuity_restore"]["handoff_summary"]["next_step"],
            json!("ещё нет данных")
        );
    }

    #[test]
    fn continuity_restore_payload_keeps_recovered_useful_eval_layer() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "headline": "Temporal lookup materialized for the continuity restore compact prompt contour with enough detail to exceed the prompt headline limit",
            "next_step": "Проверить lookup по точному времени на реальном новом чате и убедиться, что compact prompt сохраняет рабочую линию даже после обрезки длинного следующего шага."
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Temporal lookup materialized for the continuity restore compact prompt contour with enough detail to exceed the prompt headline limit",
                "next_step": "Проверить lookup по точному времени на реальном новом чате и убедиться, что compact prompt сохраняет рабочую линию даже после обрезки длинного следующего шага.",
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
        assert!(payload["workspace_restore_pack"].is_null());
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
    fn render_direct_answer_normalizes_empty_project_handoff_text() {
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });

        let answer = render_direct_answer(&handoff, None, None, "previous_chat", None, 30);

        assert!(answer.contains("Текущая активная линия проекта сейчас: ещё нет данных"));
        assert!(answer.contains("Ближайший обязательный следующий шаг: ещё нет данных"));
    }

    #[test]
    fn continuity_startup_payload_keeps_recovered_useful_eval_layer() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "headline": "Temporal lookup materialized for the startup compact prompt contour with enough detail to exceed the prompt headline limit",
            "next_step": "Проверить lookup по точному времени на реальном новом чате и убедиться, что compact prompt сохраняет рабочую линию даже после обрезки длинного следующего шага."
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Temporal lookup materialized for the startup compact prompt contour with enough detail to exceed the prompt headline limit",
                "next_step": "Проверить lookup по точному времени на реальном новом чате и убедиться, что compact prompt сохраняет рабочую линию даже после обрезки длинного следующего шага.",
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
        let payload = build_continuity_startup_payload(
            &context,
            context.restore.as_ref(),
            &chat_start_restore,
        )
        .expect("payload");
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
            visibility_scope: "project_shared".to_string(),
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
                "required_task_set": [
                    "Вернуть same-meter spend control в активную линию",
                    "Проверить auto-injection в новом чате"
                ],
                "required_task_set_summary": "2 задач(и): Вернуть same-meter spend control в активную линию",
                "pending_return_queue": [
                    {
                        "headline": "Same-meter spend control",
                        "next_step": "Materialize live assistant generation source."
                    }
                ],
                "materialized_notes": [
                    "Enriched temporal summaries теперь пишутся upstream."
                ],
                "workspace_restore_pack_summary": "active(1); paused(1); facts(1); constraints(2); artifacts(3); procedures(1)",
                "workspace_restore_pack": {
                    "pack_version": "workspace-restore-pack-v1",
                    "active_commitments": [{"headline": "Amai upstream thread-index enrich materialized"}],
                    "blocked_waiting_items": [],
                    "paused_branches": [{"headline": "Same-meter spend control"}],
                    "recently_closed": [{"headline": "Older handoff"}],
                    "relevant_semantic_facts": [{"summary": "Enriched temporal summaries теперь пишутся upstream."}],
                    "recent_episodic_traces": [{"headline": "Проверили previous chat lookup"}],
                    "active_constraints": [{"summary": "return_required(1)"}],
                    "important_artifacts": [{"path": "/home/art/agent-memory-index/src/continuity.rs"}],
                    "unresolved_conflicts": [{"summary": "Как сделать auto-injection без дополнительного helper-обхода?"}],
                    "relevant_procedures": [{"summary": "Restore Continuity Card [trial] -> inspect startup gate"}],
                    "procedural_restore_policy": {
                        "raw_procedural_archive_forbidden": true,
                        "materialized_surface": "compact_execution_card"
                    },
                    "summary": "active(1); paused(1); facts(1); constraints(2); artifacts(3); procedures(1)"
                },
                "skill_execution_card_summary": "Restore Continuity Card [trial] -> inspect startup gate",
                "skill_execution_card": {
                    "skill_title": "Restore Continuity Card",
                    "skill_execution_steps": ["inspect startup gate"],
                    "skill_trust_state": "trial"
                },
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
        assert!(
            prompt.contains("Карточка: Restore Continuity Card [trial] -> inspect startup gate")
        );
        assert!(prompt.contains(
            "Workspace: active(1); paused(1); facts(1); constraints(2); artifacts(3); procedures(1)"
        ));
        assert!(prompt.contains(
            "Возврат: Same-meter spend control -> Materialize live assistant generation source."
        ));
        assert!(prompt.contains("Контракт: return_required(1): Same-meter spend control -> Materialize live assistant generation source."));
        assert!(
            prompt
                .contains("Задачи: 2 задач(и): Вернуть same-meter spend control в активную линию")
        );
        assert!(!prompt.contains("Project:"));
        assert!(!prompt.contains("Namespace:"));
        assert!(!prompt.contains("Сделано:"));
        assert!(prompt.contains("Сначала: вернись: Same-meter spend control"));
        assert!(!prompt.contains("Не переключайся до возврата."));
        assert!(!prompt.contains("Правило: follow pack; continuity не поднимай."));
        assert!(!prompt.contains("Недавнее:"));
        assert!(!prompt.contains("Файлы:"));
        assert!(!prompt.contains("Вопросы:"));
        assert!(!prompt.contains("Thread count in continuity index"));
        assert!(!prompt.contains("Активный lease ExecCtl"));
        assert_eq!(node["thread_count"], json!(16));
        assert_eq!(
            node["workspace_restore_pack"]["active_commitments_count"],
            json!(1)
        );
        assert!(node["current_goal"].is_null());
        assert!(node["latest_rendered_transcript"].is_null());
        assert!(node["active_files"].is_null());
        assert!(node["open_questions"].is_null());
        assert_eq!(
            node["skill_execution_card_summary"],
            json!("Restore Continuity Card [trial] -> inspect startup gate")
        );
        assert_eq!(
            node["skill_execution_card"]["skill_title"],
            json!("Restore Continuity Card")
        );
        assert_eq!(node["project_task_tree"]["summary_only"], json!(true));
        assert!(node["project_task_tree"]["nodes"].is_null());
        assert!(node["project_task_tree"]["edges"].is_null());
        assert_eq!(node["project_task_ledger"]["summary_only"], json!(true));
        assert!(node["project_task_ledger"]["entries"].is_null());
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
        assert_eq!(
            node["required_task_set"][0],
            json!("Вернуть same-meter spend control в активную линию")
        );
        assert_eq!(
            node["pending_return_queue"][0]["headline"],
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
    fn build_chat_start_restore_normalizes_blank_handoff_text() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
                    "thread_count": 0,
                    "latest_rendered_transcript": ""
                }
            }
        });
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });

        let pack = build_chat_start_restore(&project, &namespace, &continuity, &handoff, None);
        let node = &pack["chat_start_restore"];
        let prompt = node["prompt_text"].as_str().expect("prompt text");

        assert_eq!(node["headline"], json!("ещё нет данных"));
        assert_eq!(node["next_step"], json!("ещё нет данных"));
        assert_eq!(node["execctl_resume_state"], Value::Null);
        assert_eq!(node["execctl_resume_obligation"]["resume_state"], Value::Null);
        assert!(prompt.contains("Линия: ещё нет данных"));
        assert!(prompt.contains("Шаг: ещё нет данных"));
    }

    #[test]
    fn summarize_execctl_resume_contract_for_prompt_preserves_missing_state() {
        let summary = super::summarize_execctl_resume_contract_for_prompt(
            None,
            &json!({
                "required_return_headline": "Pending line"
            }),
        )
        .expect("prompt summary");

        assert_eq!(summary, "?(?): Pending line -> ещё нет данных");
    }

    #[test]
    fn summarize_execctl_resume_contract_for_prompt_ignores_whitespace_only_state() {
        let summary = super::summarize_execctl_resume_contract_for_prompt(
            None,
            &json!({
                "resume_state": "   ",
                "required_return_headline": "   ",
                "required_return_next_step": "   "
            }),
        );

        assert!(summary.is_none());
    }

    #[test]
    fn build_chat_start_restore_preserves_missing_thread_count_as_null() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
                    "latest_rendered_transcript": "/tmp/rendered.md"
                }
            }
        });
        let handoff = json!({
            "headline": "Current active line",
            "next_step": "Continue foundation work."
        });

        let pack = build_chat_start_restore(&project, &namespace, &continuity, &handoff, None);
        let node = &pack["chat_start_restore"];

        assert!(node["thread_count"].is_null());
        assert!(node["restore_confidence"].is_null());
    }

    #[test]
    fn synthetic_continuity_from_runtime_preserves_missing_counts_as_null() {
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });

        let continuity =
            super::synthetic_continuity_from_runtime(&namespace, Some(&handoff), None);

        assert!(continuity["documents_imported"].is_null());
        assert!(continuity["rendered_transcript_files"].is_null());
        assert!(continuity["session_memory_files"].is_null());
        assert!(continuity["bootstrap_summary"]["details"]["thread_count"].is_null());
        assert!(continuity["bootstrap_summary"]["details"]["latest_rendered_transcript"].is_null());
        assert!(continuity["active_workline_summary"]["details"]["headline"].is_null());
        assert!(continuity["active_workline_summary"]["details"]["next_step"].is_null());
    }

    #[test]
    fn build_chat_start_restore_prefers_live_client_budget_summary_over_stale_notes() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "next_step": "продолжить работу в новой рабочей поверхности через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "restore_confidence": "high",
                "client_budget_guard": {
                    "status_label": "сожми текущий чат сейчас",
                    "last_request": "162594 из 258400, остаётся 37.08% · raw 23:27:06 MSK",
                    "client_limits": "5ч остаётся 8.00%, 7д остаётся 72.00% · raw 23:27:06 MSK"
                },
                "materialized_notes": [
                    "Client-budget advisory signal: сожми текущий чат.",
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

        assert!(
            prompt.contains("Сделано: Client-budget advisory signal: сожми текущий чат сейчас.")
        );
        assert!(!prompt.contains("190690 из 258400"));
        assert!(materialized.contains("162594 из 258400"));
        assert!(materialized.contains("23:27:06 MSK"));
        assert!(!materialized.contains("23:15:46 MSK"));
    }

    #[test]
    fn build_rotate_chat_details_prefers_delivery_surface_status_label() {
        let guard = json!({
            "status_label": "новый чат нужен сейчас",
            "delivery_surface_status_label": "новая чистая рабочая поверхность нужна сейчас",
            "last_request": "162594 из 258400",
            "client_limits": "5ч остаётся 8.00%"
        });

        let details = super::build_rotate_chat_details(
            &guard,
            "Продолжить активную рабочую линию",
            "Сначала сожми текущий чат",
        );

        assert!(details.contains(
            "Client-budget advisory signal: новая чистая рабочая поверхность нужна сейчас."
        ));
        assert!(!details.contains("Client-budget advisory signal: сожми текущий чат сейчас."));
    }

    #[test]
    fn startup_materialized_notes_fall_back_to_normalized_status_label() {
        let restore = json!({
            "client_budget_guard": {
                "status_label": "новый чат нужен сейчас",
                "last_request": "162594 из 258400"
            }
        });

        let notes = super::startup_materialized_notes(&restore);

        assert!(
            notes
                .iter()
                .any(|note| note == "Client-budget advisory signal: сожми текущий чат сейчас.")
        );
    }

    #[test]
    fn startup_materialized_notes_and_summaries_ignore_whitespace_only_text() {
        let restore = json!({
            "client_budget_guard": {
                "status_label": "новый чат нужен сейчас",
                "last_request": "   ",
                "client_limits": "\n\t"
            },
            "recent_actions": [
                {
                    "headline": "   ",
                    "summary": "   "
                },
                {
                    "headline": "Поднять continuity runtime audit",
                    "summary": "   "
                }
            ],
            "active_files": ["   ", "src/continuity.rs", ""]
        });

        let notes = super::startup_materialized_notes(&restore);
        let actions = super::summarize_recent_actions(&restore["recent_actions"]);
        let files = super::summarize_string_list(&restore["active_files"], 3);

        assert!(
            notes
                .iter()
                .all(|note| !note.contains("Последний запрос в модель:"))
        );
        assert!(
            notes
                .iter()
                .all(|note| !note.contains("Лимит клиента сейчас:"))
        );
        assert_eq!(actions.as_deref(), Some("Поднять continuity runtime audit"));
        assert_eq!(files.as_deref(), Some("src/continuity.rs"));
    }

    #[test]
    fn build_rotate_chat_details_ignores_empty_delivery_surface_status_label() {
        let guard = json!({
            "status_label": "новый чат нужен сейчас",
            "delivery_surface_status_label": "   ",
            "last_request": "162594 из 258400"
        });

        let details = super::build_rotate_chat_details(
            &guard,
            "Продолжить активную рабочую линию",
            "Сначала сожми текущий чат",
        );

        assert!(details.contains("Client-budget advisory signal: сожми текущий чат сейчас."));
    }

    #[test]
    fn rotate_and_startup_surfaces_ignore_whitespace_only_summary_text() {
        let guard = json!({
            "status_label": "новый чат нужен сейчас",
            "last_request": "   ",
            "client_limits": "\n\t",
            "note": "   "
        });

        let rotate_details = super::build_rotate_chat_details(
            &guard,
            "Продолжить активную рабочую линию",
            "Сначала сожми текущий чат",
        );
        let compact_details = super::build_compact_chat_details(
            &guard,
            "Продолжить активную рабочую линию",
            "Сначала сожми текущий чат",
        );

        assert!(!rotate_details.contains("Последний запрос в модель:"));
        assert!(!rotate_details.contains("Лимит клиента сейчас:"));
        assert!(!rotate_details.contains("Почему advisory-сигнал рекомендует rotate/compact:"));
        assert!(!compact_details.contains("Последний запрос в модель:"));
        assert!(!compact_details.contains("Лимит клиента сейчас:"));

        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "next_step": "продолжить работу в новой рабочей поверхности через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "   ",
                "restore_confidence": "high",
                "workspace_restore_pack_summary": "   ",
                "skill_execution_card_summary": "   ",
                "pending_return_summary": "   ",
                "execctl_resume_contract_summary": "   ",
                "client_budget_guard": {
                    "status_label": "новый чат нужен сейчас"
                }
            }
        });

        let pack =
            build_chat_start_restore(&project, &namespace, &continuity, &handoff, Some(&restore));
        let node = &pack["chat_start_restore"];
        let prompt = node["prompt_text"].as_str().expect("prompt text");

        assert_eq!(
            node["headline"],
            json!("Продолжить активную рабочую линию")
        );
        assert_eq!(node["workspace_restore_pack_summary"], Value::Null);
        assert_eq!(node["skill_execution_card_summary"], Value::Null);
        assert_eq!(node["pending_return_summary"], Value::Null);
        assert_eq!(node["execctl_resume_contract_summary"], Value::Null);
        assert!(!prompt.contains("Карточка:"));
        assert!(!prompt.contains("Workspace:"));
        assert!(!prompt.contains("Контракт:"));
    }

    #[test]
    fn chat_start_restore_ignores_whitespace_only_startup_summary_fields() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "next_step": "продолжить работу в новой рабочей поверхности через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "restore_confidence": "high",
                "startup_next_action_summary": "   ",
                "execctl_active_lease_summary": "   ",
                "project_task_tree_summary": "   ",
                "project_task_ledger_summary": "   ",
                "required_task_set_summary": "   ",
                "execctl_resume_state": "   ",
                "execctl_resume_contract": {
                    "resume_state": "   ",
                    "required_return_task": {
                        "headline": "   ",
                        "next_step": "   "
                    }
                },
                "execctl_resume_obligation": {
                    "required_return_headline": "   ",
                    "required_return_next_step": "   "
                }
            }
        });

        let pack =
            build_chat_start_restore(&project, &namespace, &continuity, &handoff, Some(&restore));
        let node = &pack["chat_start_restore"];
        let prompt = node["prompt_text"].as_str().expect("prompt text");

        assert_eq!(
            node["startup_next_action_summary"],
            json!("continue_active_workline: Продолжить активную рабочую линию -> продолжить работу в новой рабочей поверхности через continuity startup")
        );
        assert!(node["execctl_active_lease_summary"].is_null());
        assert!(node["project_task_tree_summary"].is_null());
        assert!(node["project_task_ledger_summary"].is_null());
        assert!(node["required_task_set_summary"].is_null());
        assert!(node["execctl_resume_state"].is_null());
        assert!(!prompt.contains("Сначала закрой возврат."));
    }

    #[test]
    fn display_client_budget_status_label_returns_none_when_all_labels_missing() {
        let guard = json!({
            "status_label": "   ",
            "delivery_surface_status_label": ""
        });

        assert!(super::display_client_budget_status_label(&guard).is_none());
    }

    #[test]
    fn build_chat_start_restore_prompt_includes_blocked_reply_text_for_rotate_path() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "next_step": "продолжить работу в новой рабочей поверхности через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "restore_confidence": "high",
                "startup_next_action": {
                    "action_kind": "rotate_chat_for_client_budget",
                    "blocking": true,
                    "headline": "Клиентский лимит: сожми текущий чат сейчас",
                    "next_step": "сначала сожми текущий чат; continuity startup используй только как fallback"
                },
                "client_budget_guard": {
                    "reply_execution_gate": {
                        "blocking_reply_contract": {
                            "template": "Этот чат жжёт внешний лимит клиента: сначала сожми текущий чат; continuity startup используй только если fallback действительно нужен."
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

        assert!(prompt.contains("Сначала: Клиентский лимит: сожми текущий чат сейчас"));
        assert!(prompt.contains("Только rotate: Этот чат жжёт внешний лимит клиента:"));
    }

    #[test]
    fn startup_prompt_and_rotate_blocked_reply_ignore_whitespace_only_text() {
        let summary = super::summarize_startup_next_action_for_prompt(&json!({
            "action_kind": "honor_required_task_set",
            "headline": "   ",
            "next_step": "   ",
            "required_task_set_summary": "   "
        }))
        .expect("summary");
        assert_eq!(summary, "Сначала: ещё нет данных");

        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "amai".to_string(),
            display_name: "Amai".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "next_step": "продолжить работу в новой рабочей поверхности через continuity startup"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "restore_confidence": "high",
                "startup_next_action": {
                    "action_kind": "rotate_chat_for_client_budget",
                    "blocking": true,
                    "headline": "Клиентский лимит: сожми текущий чат сейчас",
                    "next_step": "сначала сожми текущий чат; continuity startup используй только как fallback"
                },
                "client_budget_guard": {
                    "reply_execution_gate": {
                        "blocking_reply_contract": {
                            "template": "   "
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

        assert!(!prompt.contains("Только rotate:"));
    }

    #[test]
    fn compact_continuity_startup_public_payload_compacts_heavy_restore_surfaces() {
        let payload = json!({
            "continuity_startup": {
                "project": {"code": "amai"},
                "namespace": {"code": "continuity"}
            },
            "chat_start_restore": {
                "headline": "Current line",
                "next_step": "Keep lowering burn",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE",
                "included_reasons_summary": "exact_documents (1)",
                "excluded_reasons_summary": "semantic_chunks"
            },
            "working_state_restore": {
                "project": {"code": "amai"},
                "namespace": {"code": "continuity"},
                "current_goal": "Lower client burn",
                "next_step": "Compact recurring startup payloads",
                "current_focus": "Сейчас основной фокус: src/continuity.rs по запросу «continuity startup».",
                "restore_confidence": "high",
                "source_summary": "Источник истины: continuity_handoff (/tmp/live-handoff.md). Главный подтверждающий артефакт: src/continuity.rs.",
                "state_lineage": {
                    "authoritative_event_id": "event-1",
                    "authoritative_event_kind": "working_state_restore",
                    "nodes": [{"id": "n1"}],
                    "edges": [{"from": "n1", "to": "n2"}]
                },
                "client_budget_guard": {
                    "status_label": "сожми текущий чат сейчас",
                    "client_budget_target_percent": 90,
                    "reply_execution_gate": {
                        "gate_version": "client-reply-budget-gate-v1",
                        "action_kind": "rotate_chat_for_client_budget",
                        "blocking": false,
                        "must_rotate_before_reply": false,
                        "must_wait_for_budget_recovery_before_reply": false,
                        "reply_budget_mode": "compact_high_signal",
                        "reply_prefix": "5ч KPI: переплата 12.34%"
                    }
                },
                "execctl_resume_contract": {
                    "contract_version": "execctl-resume-contract-v1",
                    "resume_state": "return_required",
                    "no_silent_drop": true,
                    "pending_return_count": 2,
                    "required_return_task": {
                        "headline": "Pending task",
                        "next_step": "Finish the compact startup path",
                        "resume_state": "pending_return",
                        "task_id": "task-1",
                        "task_role": "pending_return",
                        "task_state": "suspended"
                    },
                    "active_task": {
                        "headline": "Current line",
                        "next_step": "Keep lowering burn",
                        "resume_state": "active",
                        "task_id": "task-0",
                        "task_role": "active",
                        "task_state": "active"
                    }
                },
                "execctl_active_lease": {
                    "lease_version": "execctl-lease-v1",
                    "lease_owner_state": "previous_session_owner",
                    "lease_state": "active",
                    "headline": "Current line",
                    "next_step": "Keep lowering burn",
                    "storage_lane": "ami.execctl_task_leases",
                    "owner_thread_id": "thread-1"
                },
                "startup_next_action": {
                    "action_version": "startup-next-action-v1",
                    "action_kind": "resume_required_return_task",
                    "blocking": true,
                    "reason": "resume_required_return_task",
                    "resume_state": "return_required",
                    "no_silent_drop": true,
                    "headline": "Pending task",
                    "next_step": "Finish the compact startup path"
                },
                "project_task_tree": {
                    "open_tasks_count": 2,
                    "pending_return_count": 1,
                    "nodes": [
                        {"task_id": "task-0"},
                        {"task_id": "task-1"}
                    ],
                    "edges": [
                        {"from_task_id": "root", "to_task_id": "task-0"}
                    ]
                },
                "project_task_ledger": {
                    "open_tasks_count": 2,
                    "historical_handoffs_count": 4,
                    "entries": [
                        {"task_role": "active", "headline": "Current line"},
                        {"task_role": "pending_return", "headline": "Pending task"}
                    ]
                },
                "pending_return_queue": [
                    {
                        "headline": "Pending task",
                        "next_step": "Finish the compact startup path",
                        "resume_state": "pending_return",
                        "queued_reason": "interrupted_by_new_handoff",
                        "authoritative_event_id": "event-1",
                        "authoritative_local_path": "/tmp/handoff.md"
                    }
                ]
            },
            "retrieval_science": {"suite_key": "continuity_startup"},
            "degradation_policy": {"status": "ok"}
        });

        let compact = super::compact_continuity_startup_public_payload(&payload);
        let startup = &compact["continuity_startup"];
        let chat_start_restore = &compact["chat_start_restore"];
        let delivery_surface_restore = &compact["delivery_surface_restore"];
        let restore = &compact["working_state_restore"];
        let compact_len = serde_json::to_string(&compact)
            .expect("compact payload serialization")
            .len();
        let raw_len = serde_json::to_string(&payload)
            .expect("raw payload serialization")
            .len();

        assert_eq!(startup["summary_only"], json!(true));
        assert_eq!(startup["project"]["code"], json!("amai"));
        assert_eq!(
            startup["canonical_eval"]["verdict_counts"]["recovered_useful"],
            json!(null)
        );
        assert_eq!(chat_start_restore["summary_only"], json!(true));
        assert_eq!(
            chat_start_restore["prompt_text"],
            json!("CHAT_START_RESTORE")
        );
        assert_eq!(delivery_surface_restore, chat_start_restore);
        assert!(chat_start_restore["headline"].is_null());
        assert!(chat_start_restore["included_reasons_summary"].is_null());
        assert_eq!(restore["summary_only"], json!(true));
        assert_eq!(restore["agent_scope"], json!(null));
        assert_eq!(restore["current_goal"], json!("Lower client burn"));
        assert_eq!(
            restore["next_step"],
            json!("Compact recurring startup payloads")
        );
        assert_eq!(
            restore["current_focus"],
            json!("Сейчас основной фокус: src/continuity.rs по запросу «continuity startup».")
        );
        assert_eq!(restore["restore_confidence"], json!("high"));
        assert_eq!(
            restore["source_summary"],
            json!(
                "Источник истины: continuity_handoff (/tmp/live-handoff.md). Главный подтверждающий артефакт: src/continuity.rs."
            )
        );
        assert!(restore["project"].is_null());
        assert!(restore["namespace"].is_null());
        assert_eq!(
            restore["state_lineage"]["authoritative_event_id"],
            json!("event-1")
        );
        assert!(restore["client_budget_guard"].is_null());
        assert!(restore["startup_next_action"].is_null());
        assert!(restore["execctl_active_lease"].is_null());
        assert!(restore["execctl_resume_contract"].is_null());
        assert!(restore["project_task_tree"].is_null());
        assert!(restore["project_task_ledger"].is_null());
        assert!(restore["pending_return_queue"].is_null());
        assert_eq!(
            compact["retrieval_science"]["suite_key"],
            json!("continuity_startup")
        );
        assert!(compact["degradation_policy"].is_null());
        assert!(compact_len < raw_len / 2);
    }

    #[test]
    fn compact_compact_chat_chat_start_restore_keeps_multi_line_obligations() {
        let node = json!({
            "headline": "Current active line",
            "next_step": "Finish the continuity rebase",
            "restore_confidence": "high",
            "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
            "execctl_resume_state": "pending_return_queue_present",
            "pending_return_summary": "pending_return(2): Pending line; +1 more",
            "project_task_tree": {
                "open_tasks_count": 3,
                "pending_return_count": 2,
                "nodes": [
                    {"task_id": "t0"},
                    {"task_id": "t1"},
                    {"task_id": "t2"}
                ],
                "edges": [
                    {"from_task_id": "root", "to_task_id": "t0"},
                    {"from_task_id": "root", "to_task_id": "t1"}
                ]
            },
            "project_task_tree_summary": "active: Current active line; pending_return(2): Pending line; +1 more",
            "project_task_ledger": {
                "open_tasks_count": 3,
                "historical_handoffs_count": 7,
                "entries": [
                    {"task_role": "active"},
                    {"task_role": "pending_return"},
                    {"task_role": "pending_return"},
                    {"task_role": "historical_handoff"}
                ]
            },
            "project_task_ledger_summary": "active: Current active line; pending_return(2); historical_handoffs(1)",
            "required_task_set": [
                "Fix first card",
                "Verify second card"
            ],
            "required_task_set_summary": "2 задач(и): Fix first card",
            "pending_return_queue": [
                {"headline": "Pending line", "next_step": "Close same-meter live gap."},
                {"headline": "Older line", "next_step": "Keep compact."}
            ],
            "startup_next_action": {
                "action_kind": "resume_required_return_task",
                "headline": "Pending line",
                "next_step": "Close same-meter live gap."
            },
            "required_return_task": {
                "headline": "Pending line",
                "next_step": "Close same-meter live gap."
            }
        });

        let compact = super::compact_compact_chat_chat_start_restore(&node);

        assert_eq!(
            compact["execctl_resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            compact["pending_return_summary"],
            json!("pending_return(2): Pending line; +1 more")
        );
        assert_eq!(compact["project_task_tree"]["summary_only"], json!(true));
        assert_eq!(compact["project_task_tree"]["open_tasks_count"], json!(3));
        assert_eq!(
            compact["project_task_tree"]["pending_return_count"],
            json!(2)
        );
        assert_eq!(compact["project_task_tree"]["nodes_total"], json!(3));
        assert_eq!(compact["project_task_ledger"]["summary_only"], json!(true));
        assert_eq!(compact["project_task_ledger"]["open_tasks_count"], json!(3));
        assert_eq!(
            compact["project_task_ledger"]["pending_return_entries_count"],
            json!(2)
        );
        assert_eq!(compact["required_task_set"][0], json!("Fix first card"));
        assert_eq!(
            compact["pending_return_queue"][1]["headline"],
            json!("Older line")
        );
        assert_eq!(compact["pending_return_queue_total"], json!(2));
        assert_eq!(
            compact["pending_return_queue_full_shape_preserved_in_working_state_restore"],
            json!(true)
        );
        assert!(compact["startup_next_action"].is_null());
        assert!(compact["required_return_task"].is_null());
    }

    #[test]
    fn compact_compact_chat_chat_start_restore_bounds_pending_return_queue_preview() {
        let node = json!({
            "headline": "Current active line",
            "next_step": "Finish the continuity rebase",
            "restore_confidence": "high",
            "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
            "execctl_resume_state": "pending_return_queue_present",
            "pending_return_summary": "pending_return(5): Pending line 0; +4 more",
            "pending_return_queue": [
                {
                    "task_id": "task-0",
                    "headline": "Pending line 0",
                    "next_step": "Close gap 0",
                    "resume_state": "pending_return",
                    "queued_at_epoch_ms": 100,
                    "queued_reason": "interrupted_by_new_handoff",
                    "authoritative_event_id": "event-0",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/h0.md",
                    "extra_field": "must not survive"
                },
                {
                    "task_id": "task-1",
                    "headline": "Pending line 1",
                    "next_step": "Close gap 1",
                    "resume_state": "pending_return",
                    "queued_at_epoch_ms": 101,
                    "queued_reason": "interrupted_by_new_handoff",
                    "authoritative_event_id": "event-1",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/h1.md"
                },
                {
                    "task_id": "task-2",
                    "headline": "Pending line 2",
                    "next_step": "Close gap 2",
                    "resume_state": "pending_return",
                    "queued_at_epoch_ms": 102,
                    "queued_reason": "interrupted_by_new_handoff",
                    "authoritative_event_id": "event-2",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/h2.md"
                },
                {
                    "task_id": "task-3",
                    "headline": "Pending line 3",
                    "next_step": "Close gap 3",
                    "resume_state": "pending_return",
                    "queued_at_epoch_ms": 103,
                    "queued_reason": "interrupted_by_new_handoff",
                    "authoritative_event_id": "event-3",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/h3.md"
                },
                {
                    "task_id": "task-4",
                    "headline": "Pending line 4",
                    "next_step": "Close gap 4",
                    "resume_state": "pending_return",
                    "queued_at_epoch_ms": 104,
                    "queued_reason": "interrupted_by_new_handoff",
                    "authoritative_event_id": "event-4",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/h4.md"
                }
            ]
        });

        let compact = super::compact_compact_chat_chat_start_restore(&node);
        let queue = compact["pending_return_queue"]
            .as_array()
            .expect("pending return queue preview");

        assert_eq!(
            queue.len(),
            super::MAX_COMPACT_CHAT_PENDING_RETURN_QUEUE_PREVIEW
        );
        assert_eq!(compact["pending_return_queue_total"], json!(5));
        assert_eq!(
            compact["pending_return_queue_full_shape_preserved_in_working_state_restore"],
            json!(false)
        );
        assert_eq!(queue[0]["headline"], json!("Pending line 0"));
        assert_eq!(queue[2]["headline"], json!("Pending line 2"));
        assert!(queue[0]["extra_field"].is_null());
        assert!(queue[0]["authoritative_event_id"].is_null());
        assert!(queue[0]["authoritative_local_path"].is_null());
        assert!(queue[0]["queued_at_epoch_ms"].is_null());
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
        assert!(!super::is_meaningful_restore_value(
            "Продолжить активную рабочую линию"
        ));
        assert!(super::is_meaningful_restore_value(
            "Same-meter spend control"
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
                "required_task_set": [],
                "required_task_set_summary": Value::Null,
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "tree_version": "project-task-tree-v1",
                    "project_code": "art",
                    "namespace_code": "continuity",
                    "root_task_id": "root",
                    "open_tasks_count": 2,
                    "pending_return_count": 1,
                    "nodes": [
                        {"task_id": "t1", "task_role": "active", "task_state": "active", "resume_state": "active", "headline": "Current active line", "next_step": "Ship runtime return enforcement."},
                        {"task_id": "t2", "task_role": "pending_return", "task_state": "suspended", "resume_state": "pending_return", "headline": "Pending line", "next_step": "Close same-meter live gap."},
                        {"task_id": "t3", "task_role": "pending_return", "task_state": "suspended", "resume_state": "pending_return", "headline": "Older line", "next_step": "Keep compact."},
                        {"task_id": "t4", "task_role": "pending_return", "task_state": "suspended", "resume_state": "pending_return", "headline": "Another line", "next_step": "Keep compact."},
                        {"task_id": "t5", "task_role": "pending_return", "task_state": "suspended", "resume_state": "pending_return", "headline": "Extra line", "next_step": "Should truncate."}
                    ],
                    "edges": [
                        {"from_task_id": "root", "to_task_id": "t1", "relation": "tracks_open_task", "priority_rank": 0},
                        {"from_task_id": "root", "to_task_id": "t2", "relation": "tracks_open_task", "priority_rank": 1},
                        {"from_task_id": "root", "to_task_id": "t3", "relation": "tracks_open_task", "priority_rank": 2},
                        {"from_task_id": "root", "to_task_id": "t4", "relation": "tracks_open_task", "priority_rank": 3},
                        {"from_task_id": "root", "to_task_id": "t5", "relation": "tracks_open_task", "priority_rank": 4}
                    ]
                },
                "project_task_tree_summary": "active: Current active line; pending_return(1): Pending line",
                "project_task_ledger": {
                    "ledger_version": "project-task-ledger-v2",
                    "project_code": "art",
                    "namespace_code": "continuity",
                    "open_tasks_count": 2,
                    "historical_handoffs_count": 4,
                    "persistence_state": "durable_postgres",
                    "storage_lane": "ami.execctl_task_ledger_entries",
                    "entries": [
                        {
                            "headline": "Current active line",
                            "next_step": "Ship runtime return enforcement.",
                            "resume_state": "active",
                            "task_role": "active",
                            "task_state": "active",
                            "recorded_at_epoch_ms": 1,
                            "task_id": "t1",
                            "agent_scope": "art::continuity::default",
                            "active_files": ["/tmp/a", "/tmp/b", "/tmp/c", "/tmp/d"],
                            "materialized_notes": ["n1", "n2", "n3", "n4"],
                            "pending_return_queue": [
                                {"headline": "Pending line", "next_step": "Close same-meter live gap.", "resume_state": "pending_return", "queued_at_epoch_ms": 2},
                                {"headline": "Older pending", "next_step": "Keep compact.", "resume_state": "pending_return", "queued_at_epoch_ms": 3},
                                {"headline": "Another pending", "next_step": "Keep compact.", "resume_state": "pending_return", "queued_at_epoch_ms": 4},
                                {"headline": "Extra pending", "next_step": "Should truncate.", "resume_state": "pending_return", "queued_at_epoch_ms": 5}
                            ]
                        },
                        {
                            "headline": "Pending line",
                            "next_step": "Close same-meter live gap.",
                            "resume_state": "pending_return",
                            "task_role": "pending_return",
                            "task_state": "suspended",
                            "recorded_at_epoch_ms": 6,
                            "task_id": "t2",
                            "agent_scope": "art::continuity::default",
                            "active_files": ["/tmp/e"],
                            "materialized_notes": ["n5"],
                            "pending_return_queue": []
                        },
                        {
                            "headline": "Older line",
                            "next_step": "Keep compact.",
                            "resume_state": "historical_only",
                            "task_role": "historical_handoff",
                            "task_state": "superseded",
                            "recorded_at_epoch_ms": 7,
                            "task_id": "t3",
                            "agent_scope": "art::continuity::default",
                            "active_files": [],
                            "materialized_notes": [],
                            "pending_return_queue": []
                        },
                        {
                            "headline": "Extra line",
                            "next_step": "Should truncate.",
                            "resume_state": "historical_only",
                            "task_role": "historical_handoff",
                            "task_state": "superseded",
                            "recorded_at_epoch_ms": 8,
                            "task_id": "t4",
                            "agent_scope": "art::continuity::default",
                            "active_files": [],
                            "materialized_notes": [],
                            "pending_return_queue": []
                        }
                    ]
                },
                "project_task_ledger_summary": "active: Current active line; historical_handoffs(1)"
            },
            "working_state_restore": {
                "client_budget_guard": {
                    "status_label": "сожми текущий чат сейчас",
                    "reply_prefix": "5ч KPI: переплата 2.46%",
                    "reply_execution_gate": {
                        "gate_version": "client-reply-budget-gate-v1",
                        "must_rotate_before_reply": true,
                        "reply_prefix": "5ч KPI: переплата 2.46%",
                        "blocking_reply_contract": {
                            "contract_version": "client-budget-blocked-reply-v1",
                            "response_kind": "rotate_chat_only",
                            "template": "Лимит клиента почти исчерпан. Сохрани handoff и продолжай только в новой рабочей поверхности через continuity startup."
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
            json!(super::STARTUP_RUNTIME_STATE_ARTIFACT_VERSION)
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
            artifact["execctl_resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            artifact["execctl_resume_contract_summary"],
            artifact["continuity_startup_summary"]["execctl_resume_contract_summary"]
        );
        assert_eq!(
            artifact["execctl_resume_obligation"]["resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            artifact["startup_next_action"]["action_kind"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            artifact["execctl_active_lease"]["lease_owner_state"],
            json!("previous_session_owner")
        );
        assert_eq!(
            artifact["execctl_active_lease"]["headline"],
            json!("Current active line")
        );
        assert!(artifact["execctl_active_lease"]["lease_id"].is_null());
        assert!(artifact["execctl_active_lease"]["owner_session_id"].is_null());
        assert_eq!(
            artifact["execctl_active_lease"],
            artifact["continuity_startup_summary"]["execctl_active_lease"]
        );
        assert_eq!(
            artifact["required_return_task"]["headline"],
            json!("Pending line")
        );
        assert_eq!(
            artifact["client_budget_guard"]["status_label"],
            json!("сожми текущий чат сейчас")
        );
        assert!(artifact["client_budget_guard"]["reply_prefix"].is_null());
        assert!(artifact["client_budget_guard"]["observed_at_epoch_ms"].is_null());
        assert!(artifact["client_budget_guard"]["max_guard_age_seconds"].is_null());
        assert_eq!(
            artifact["client_budget_guard"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 2.46%")
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
            artifact["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 2.46%")
        );
        assert!(artifact["reply_execution_gate"]["action_bundle"].is_null());
        assert_eq!(
            artifact["reply_execution_gate"]["preserves_return_obligation"],
            json!(false)
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
            artifact["continuity_startup_summary"]["startup_execution_gate"]["gate_version"],
            json!("startup-execution-gate-v1")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["must_follow_startup_next_action"],
            json!(true)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["unrelated_work_allowed"],
            json!(false)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["required_return_task_present"],
            json!(true)
        );
        assert!(artifact["continuity_startup_summary"]["startup_execution_gate"]["required_return_task_headline"].is_null());
        assert!(artifact["continuity_startup_summary"]["startup_execution_gate"]["required_return_task_next_step"].is_null());
        assert_eq!(
            artifact["continuity_startup_summary"]["required_return_task"]["headline"],
            json!("Pending line")
        );
        assert!(artifact["continuity_startup_summary"]["required_return_task"]["authoritative_event_id"].is_null());
        assert!(
            artifact["continuity_startup_summary"]["required_return_task"]["parent_task_id"]
                .is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["required_return_task"]["queued_at_epoch_ms"]
                .is_null()
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["execctl_resume_obligation"]["resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["execctl_resume_obligation"]["pending_return_count"],
            json!(0)
        );
        assert_eq!(
            artifact["startup_next_action"],
            artifact["continuity_startup_summary"]["startup_next_action"]
        );
        assert_eq!(
            artifact["execctl_active_lease"],
            artifact["continuity_startup_summary"]["execctl_active_lease"]
        );
        assert_eq!(
            artifact["required_return_task"],
            artifact["continuity_startup_summary"]["required_return_task"]
        );
        assert!(artifact["continuity_startup_summary"]["execctl_resume_obligation"]["active_task_headline"].is_null());
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_tree_summary"],
            json!("active: Current active line; pending_return(1): Pending line")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_tree"]["summary_only"],
            json!(true)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_tree"]["nodes_total"],
            json!(5)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_tree"]["edges_total"],
            json!(5)
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_tree"]["tree_version"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_tree"]["project_code"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_tree"]["namespace_code"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_tree"]["root_task_id"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_tree"]["nodes_preview"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_tree"]["edges_preview"].is_null()
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_tree"]["full_shape_preserved_in_working_state_restore"],
            json!(true)
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
            artifact["continuity_startup_summary"]["project_task_ledger"]["summary_only"],
            json!(true)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["entries_count"],
            json!(4)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["active_entries_count"],
            json!(1)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["pending_return_entries_count"],
            json!(1)
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["ledger_version"]
                .is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["project_code"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["namespace_code"]
                .is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["persistence_state"]
                .is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["storage_lane"].is_null()
        );
        assert!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["entries_preview"]
                .is_null()
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["project_task_ledger"]["full_shape_preserved_in_working_state_restore"],
            json!(true)
        );
        assert_eq!(
            artifact["working_state_restore_lineage"]["authoritative_event_id"],
            json!("evt_123")
        );
        assert!(artifact["working_state_restore_lineage"]["session_id"].is_null());
        assert!(artifact["working_state_restore_lineage"]["nodes_total"].is_null());
        assert!(artifact["working_state_restore_lineage"]["edges_total"].is_null());
        assert!(artifact["working_state_restore_lineage"]["supporting_event_count"].is_null());
        assert!(artifact["working_state_restore_lineage"]["authoritative_headline"].is_null());
        assert!(artifact["working_state_restore_lineage"]["nodes"].is_null());
        assert!(artifact["working_state_restore_lineage"]["edges"].is_null());
        assert!(artifact["working_state_restore_lineage"]["supporting_event_ids"].is_null());
        assert!(artifact["continuity_startup_summary"]["included_reasons_summary"].is_null());
        assert!(artifact["continuity_startup_summary"]["excluded_reasons_summary"].is_null());
    }

    #[test]
    fn startup_runtime_artifact_keeps_blocking_rotate_gate_consistent_in_summary_and_top_level() {
        let payload = json!({
            "chat_start_restore": {
                "headline": "Rotate policy now blocks large weak exact-pair turns when 5h saving is below target",
                "next_step": "Open a fresh chat via continuity startup.",
                "restore_confidence": "high",
                "thread_count": 1,
                "prompt_text": "CHAT_START_RESTORE\nRotate now",
                "execctl_resume_state": "pending_return_queue_present",
                "execctl_resume_contract_summary": "return_required(1): keep the >90% 5h KPI line honest",
                "execctl_resume_obligation": {
                    "resume_state": "pending_return_queue_present",
                    "no_silent_drop": true,
                    "pending_return_count": 1
                },
                "startup_next_action": {
                    "action_kind": "rotate_chat_for_client_budget",
                    "blocking": true,
                    "reason": "client_budget_guard_pressure",
                    "resume_state": "return_required",
                    "no_silent_drop": true,
                    "headline": "Клиентский лимит: сожми текущий чат сейчас",
                    "next_step": "сначала сожми текущий чат; continuity startup используй только как fallback"
                },
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner",
                    "storage_lane": "ami.execctl_task_leases"
                },
                "required_return_task": {
                    "headline": "MCP context pack now replaces verified legacy tool-overhead with truthful structured-content tokens",
                    "next_step": "Continue the >90% 5h KPI line from a clean thread."
                },
                "project_task_tree": {
                    "open_tasks_count": 1
                },
                "project_task_ledger": {
                    "open_tasks_count": 1
                },
                "required_task_set": [
                    "Fix first card",
                    "Check remaining cards"
                ]
            },
            "working_state_restore": {
                "client_budget_guard": {
                    "status_label": "сожми текущий чат сейчас",
                    "reply_prefix": "5ч KPI: экономия 19.20%",
                    "reply_execution_gate": {
                        "gate_version": "client-reply-budget-gate-v1",
                        "must_rotate_before_reply": true,
                        "reply_prefix": "5ч KPI: экономия 19.20%",
                        "blocking_reply_contract": {
                            "contract_version": "client-budget-blocked-reply-v1",
                            "response_kind": "rotate_chat_only",
                            "template": "Этот чат жжёт внешний лимит клиента: сначала сожми текущий чат; continuity startup используй только если fallback действительно нужен."
                        }
                    }
                },
                "state_lineage": {
                    "authoritative_event_id": "evt_rotate",
                    "session_id": "sess_rotate"
                }
            }
        });

        let artifact =
            build_startup_runtime_state_artifact(Path::new("/tmp/amai-art"), &payload, 77)
                .expect("startup runtime state artifact");

        assert_eq!(
            artifact["startup_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(artifact["startup_execution_gate"]["blocking"], json!(true));
        assert_eq!(
            artifact["startup_execution_gate"]["required_task_set_count"],
            json!(2)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["required_task_set_present"],
            json!(true)
        );
        assert_eq!(
            artifact["startup_execution_gate"]["must_preserve_required_task_set"],
            json!(true)
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
            artifact["continuity_startup_summary"]["startup_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["blocking"],
            json!(true)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["required_task_set_count"],
            json!(2)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["must_follow_startup_next_action"],
            json!(true)
        );
        assert_eq!(
            artifact["continuity_startup_summary"]["startup_execution_gate"]["unrelated_work_allowed"],
            json!(false)
        );
        assert_eq!(
            artifact["startup_next_action"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            artifact["execctl_resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            artifact["required_return_task"]["headline"],
            json!(
                "MCP context pack now replaces verified legacy tool-overhead with truthful structured-content tokens"
            )
        );
        assert_eq!(
            artifact["required_task_set"],
            json!(["Fix first card", "Check remaining cards"])
        );
        assert!(
            artifact["required_task_set_summary"].is_null(),
            "top-level runtime mirror must preserve missing summary as null"
        );
    }

    #[test]
    fn startup_runtime_state_artifact_compacts_startup_next_action_bundle() {
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
                    "action_version": "startup-next-action-v1",
                    "action_kind": "rotate_chat_for_client_budget",
                    "blocking": true,
                    "reason": "client_budget_guard_pressure",
                    "resume_state": "return_required",
                    "no_silent_drop": true,
                    "headline": "Клиентский лимит: сожми текущий чат сейчас",
                    "next_step": "сначала сожми текущий чат; continuity startup используй только как fallback",
                    "client_budget_status_label": "сожми текущий чат сейчас",
                    "preserves_return_obligation": true,
                    "action_bundle": working_state::build_rotate_chat_action_bundle(
                        Some("art"),
                        Some("continuity"),
                        Some("/tmp/amai-art"),
                        true,
                        Some("Current active line"),
                        Some("Ship runtime return enforcement."),
                    )
                },
                "required_return_task": {
                    "headline": "Pending line",
                    "next_step": "Close same-meter live gap."
                },
                "required_task_set": [
                    "Pending line",
                    "Close same-meter live gap."
                ],
                "required_task_set_summary": "2 задач(и): Pending line",
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "summary_only": true,
                    "summary": "active: Current active line; pending_return(1): Pending line"
                },
                "project_task_tree_summary": "active: Current active line; pending_return(1): Pending line",
                "project_task_ledger": {
                    "summary_only": true,
                    "summary": "active: Current active line; historical_handoffs(1)"
                },
                "project_task_ledger_summary": "active: Current active line; historical_handoffs(1)"
            },
            "working_state_restore": {
                "client_budget_guard": {
                    "status_label": "сожми текущий чат сейчас",
                    "reply_execution_gate": {
                        "gate_version": "client-reply-budget-gate-v1",
                        "must_rotate_before_reply": true,
                        "blocking_reply_contract": {
                            "contract_version": "client-budget-blocked-reply-v1",
                            "response_kind": "rotate_chat_only",
                            "template": "Лимит клиента почти исчерпан. Сохрани handoff и продолжай только в новой рабочей поверхности через continuity startup."
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
        let bundle =
            &artifact["continuity_startup_summary"]["startup_next_action"]["action_bundle"];

        assert_eq!(
            bundle["bundle_version"],
            json!("rotate-chat-action-bundle-v1")
        );
        assert_eq!(bundle["ready_for_automation"], json!(true));
        assert_eq!(bundle["preserves_return_obligation"], json!(true));
        assert_eq!(
            bundle["host_current_thread_control"]["command_id"],
            json!("thread-overlay-open-current")
        );
        assert!(bundle["capture_continuity_handoff"].is_null());
        assert!(bundle["open_fresh_chat"].is_null());
        assert!(bundle["open_delivery_surface"].is_null());
        assert!(bundle["run_continuity_startup"].is_null());
        assert!(bundle["order"].is_null());
        assert!(
            bundle["operator_flow"]["primary_command_kind"]
                .as_str()
                .is_some_and(|value| value == "same_thread_host_control_launch_command")
        );
        assert!(
            bundle["operator_flow"]["primary_command"]
                .as_str()
                .is_some_and(|value| value.contains("ctl-launch"))
        );
        assert!(
            bundle["operator_flow"]["rotate_helper_command"]
                .as_str()
                .is_some_and(|value| value.contains("continuity rotate-chat"))
        );
        assert!(
            bundle["operator_flow"]["startup_command"]
                .as_str()
                .is_some_and(|value| value.contains("continuity startup"))
        );
        assert!(bundle["operator_flow"]["handoff_command"].is_null());
        assert!(bundle["recommended_handoff"].is_null());
    }

    #[test]
    fn compact_startup_runtime_reply_execution_gate_preserves_missing_return_flag_as_null() {
        let gate = super::compact_startup_runtime_reply_execution_gate(&json!({
            "gate_version": "client-reply-budget-gate-v1",
            "action_kind": "continue_current_chat",
            "blocking": false,
            "must_rotate_before_reply": false,
            "must_wait_for_budget_recovery_before_reply": false,
            "reply_budget_mode": "compact_high_signal",
            "reply_prefix": "5ч KPI: переплата 178.24%",
            "rotate_now": false,
            "rotate_soon": false,
            "action_bundle": {}
        }));

        assert!(gate["preserves_return_obligation"].is_null());
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
                "required_task_set": [],
                "required_task_set_summary": Value::Null,
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "tree_version": "project-task-tree-v1",
                    "project_code": "art",
                    "namespace_code": "continuity",
                    "root_task_id": "root",
                    "open_tasks_count": 2,
                    "pending_return_count": 1,
                    "nodes": [
                        {"task_id": "t1", "task_role": "active", "task_state": "active", "resume_state": "active", "headline": "Current active line", "next_step": "Ship runtime return enforcement."},
                        {"task_id": "t2", "task_role": "pending_return", "task_state": "suspended", "resume_state": "pending_return", "headline": "Pending line", "next_step": "Close same-meter live gap."}
                    ],
                    "edges": [
                        {"from_task_id": "root", "to_task_id": "t1", "relation": "tracks_open_task", "priority_rank": 0},
                        {"from_task_id": "root", "to_task_id": "t2", "relation": "tracks_open_task", "priority_rank": 1}
                    ]
                },
                "project_task_tree_summary": "active: Current active line; pending_return(1): Pending line",
                "project_task_ledger": {
                    "ledger_version": "project-task-ledger-v2",
                    "project_code": "art",
                    "namespace_code": "continuity",
                    "open_tasks_count": 2,
                    "historical_handoffs_count": 1,
                    "persistence_state": "durable_postgres",
                    "storage_lane": "ami.execctl_task_ledger_entries",
                    "entries": [
                        {
                            "headline": "Current active line",
                            "next_step": "Ship runtime return enforcement.",
                            "resume_state": "active",
                            "task_role": "active",
                            "task_state": "active",
                            "recorded_at_epoch_ms": 1,
                            "task_id": "t1",
                            "agent_scope": "art::continuity::default",
                            "active_files": ["/tmp/a"],
                            "materialized_notes": ["n1"],
                            "pending_return_queue": [
                                {"headline": "Pending line", "next_step": "Close same-meter live gap.", "resume_state": "pending_return", "queued_at_epoch_ms": 2}
                            ]
                        },
                        {
                            "headline": "Pending line",
                            "next_step": "Close same-meter live gap.",
                            "resume_state": "pending_return",
                            "task_role": "pending_return",
                            "task_state": "suspended",
                            "recorded_at_epoch_ms": 6,
                            "task_id": "t2",
                            "agent_scope": "art::continuity::default",
                            "active_files": ["/tmp/e"],
                            "materialized_notes": ["n5"],
                            "pending_return_queue": []
                        }
                    ]
                },
                "project_task_ledger_summary": "active: Current active line; historical_handoffs(1)"
            },
            "working_state_restore": {
                "client_budget_guard": {
                    "status_label": "сожми текущий чат сейчас",
                    "reply_prefix": "5ч KPI: переплата 2.46%",
                    "reply_execution_gate": {
                        "gate_version": "client-reply-budget-gate-v1",
                        "must_rotate_before_reply": true,
                        "reply_prefix": "5ч KPI: переплата 2.46%",
                        "blocking_reply_contract": {
                            "contract_version": "client-budget-blocked-reply-v1",
                            "response_kind": "rotate_chat_only",
                            "template": "Лимит клиента почти исчерпан. Сохрани handoff и продолжай только в новой рабочей поверхности через continuity startup."
                        }
                    }
                },
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
        assert_eq!(audit.required_task_set_field_present, Some(true));
        assert_eq!(audit.required_task_set_summary_field_present, Some(true));
        assert_eq!(audit.required_task_set_count_consistent, Some(true));
        assert_eq!(audit.required_task_set_presence_consistent, Some(true));
        assert_eq!(audit.must_preserve_required_task_set_consistent, Some(true));
        assert_eq!(audit.project_task_tree_summary_field_present, Some(true));
        assert_eq!(audit.project_task_ledger_summary_field_present, Some(true));

        fs::remove_dir_all(&repo).expect("cleanup temp repo");
    }

    #[test]
    fn format_optional_u64_for_human_preserves_missing_state() {
        assert_eq!(super::format_optional_u64_for_human(Some(0)), "0");
        assert_eq!(super::format_optional_u64_for_human(Some(3)), "3");
        assert_eq!(super::format_optional_u64_for_human(None), "n/a");
        assert_eq!(super::format_optional_u64_for_prompt(Some(0)), "0");
        assert_eq!(super::format_optional_u64_for_prompt(Some(3)), "3");
        assert_eq!(super::format_optional_u64_for_prompt(None), "?");
    }

    #[test]
    fn format_optional_text_for_human_preserves_missing_state() {
        assert_eq!(
            super::format_optional_text_for_human(Some("/home/art/project")),
            "/home/art/project"
        );
        assert_eq!(
            super::format_optional_text_for_human(Some("   ")),
            "ещё нет данных"
        );
        assert_eq!(
            super::format_optional_text_for_human(None),
            "ещё нет данных"
        );
    }

    #[test]
    fn optional_non_empty_text_drops_blank_values() {
        assert_eq!(
            super::optional_non_empty_text(Some(" useful text ")),
            Some("useful text")
        );
        assert_eq!(super::optional_non_empty_text(Some("   ")), None);
        assert_eq!(super::optional_non_empty_text(None), None);
    }

    #[test]
    fn normalized_handoff_summary_json_normalizes_blank_values() {
        assert_eq!(
            super::normalized_handoff_summary_json(Some("   "), Some("")),
            json!({
                "headline": "ещё нет данных",
                "next_step": "ещё нет данных",
            })
        );
    }

    #[test]
    fn normalized_thread_summary_snapshot_fields_drops_blank_values() {
        assert_eq!(
            super::normalized_thread_summary_snapshot_fields(Some("  headline  "), Some("")),
            json!({
                "summary_headline": "headline",
                "summary_next_step": Value::Null,
            })
        );
        assert_eq!(
            super::normalized_thread_summary_snapshot_fields(Some("   "), Some("  ")),
            json!({
                "summary_headline": Value::Null,
                "summary_next_step": Value::Null,
            })
        );
    }

    #[test]
    fn normalized_optional_text_json_drops_blank_values() {
        assert_eq!(
            super::normalized_optional_text_json(Some(" /tmp/file.md ")),
            json!("/tmp/file.md")
        );
        assert_eq!(super::normalized_optional_text_json(Some("   ")), Value::Null);
        assert_eq!(super::normalized_optional_text_json(None), Value::Null);
    }

    #[test]
    fn normalized_working_state_restore_projection_preserves_missing_text_as_null() {
        assert!(super::normalized_working_state_restore_projection(None).is_null());

        let normalized = super::normalized_working_state_restore_projection(Some(&json!({
            "current_goal": "   ",
            "next_step": "",
            "restore_confidence": "high",
            "state_lineage": {
                "authoritative_event_id": "   ",
                "session_id": "sess-1"
            }
        })));

        assert!(normalized["current_goal"].is_null());
        assert!(normalized["next_step"].is_null());
        assert_eq!(normalized["restore_confidence"], json!("high"));
        assert!(normalized["state_lineage"]["authoritative_event_id"].is_null());
        assert_eq!(normalized["state_lineage"]["session_id"], json!("sess-1"));
    }

    #[test]
    fn normalized_workspace_restore_pack_projection_preserves_missing_summary_as_null() {
        assert!(super::normalized_workspace_restore_pack_projection(None).is_null());

        let normalized = super::normalized_workspace_restore_pack_projection(Some(&json!({
            "summary": "   ",
            "active_commitments": [],
            "active_constraints": [],
            "important_artifacts": [],
            "procedural_restore_policy": {
                "raw_procedural_archive_forbidden": true
            }
        })));

        assert!(normalized["summary"].is_null());
        assert_eq!(normalized["active_commitments"], json!([]));
        assert_eq!(normalized["active_constraints"], json!([]));
        assert_eq!(normalized["important_artifacts"], json!([]));
        assert_eq!(
            normalized["procedural_restore_policy"]["raw_procedural_archive_forbidden"],
            json!(true)
        );
    }

    #[test]
    fn normalized_restore_confidence_value_preserves_missing_state_as_null() {
        assert!(super::normalized_restore_confidence_value(None).is_null());
        assert_eq!(
            super::normalized_restore_confidence_value(Some(&json!({
                "restore_confidence": "high"
            }))),
            json!("high")
        );
        assert!(super::normalized_restore_confidence_value(Some(&json!({}))).is_null());
    }

    #[test]
    fn normalized_optional_char_count_json_preserves_missing_state_as_null() {
        assert!(super::normalized_optional_char_count_json(None).is_null());
        assert!(super::normalized_optional_char_count_json(Some("   ")).is_null());
        assert_eq!(
            super::normalized_optional_char_count_json(Some("abc")),
            json!(3)
        );
    }

    #[test]
    fn format_startup_surface_label_preserves_missing_mode_as_human_unknown() {
        assert_eq!(
            super::format_startup_surface_label("/tmp/startup.md", Some("managed_append_block")),
            "Startup surface: /tmp/startup.md (managed_append_block)"
        );
        assert_eq!(
            super::format_startup_surface_label("/tmp/startup.md", None),
            "Startup surface: /tmp/startup.md (ещё нет данных)"
        );
        assert_eq!(
            super::format_startup_surface_label("/tmp/startup.md", Some("   ")),
            "Startup surface: /tmp/startup.md (ещё нет данных)"
        );
    }

    #[test]
    fn format_compact_restore_prompt_text_preserves_missing_state_as_human_unknown() {
        assert_eq!(
            super::format_compact_restore_prompt_text(Some("CHAT_START_RESTORE\nNext step")),
            "CHAT_START_RESTORE\nNext step"
        );
        assert_eq!(
            super::format_compact_restore_prompt_text(Some("   ")),
            "ещё нет данных"
        );
        assert_eq!(
            super::format_compact_restore_prompt_text(None),
            "ещё нет данных"
        );
    }

    #[test]
    fn format_project_scope_for_human_preserves_missing_state() {
        assert_eq!(
            super::format_project_scope_for_human(Some("Amai"), Some("amai")),
            "Amai (amai)"
        );
        assert_eq!(
            super::format_project_scope_for_human(Some("   "), None),
            "ещё нет данных (ещё нет данных)"
        );
    }

    #[test]
    fn format_client_budget_status_label_for_human_preserves_missing_state() {
        assert_eq!(
            super::format_client_budget_status_label_for_human(&json!({
                "status_label": "сожми текущий чат сейчас"
            })),
            "сожми текущий чат сейчас"
        );
        assert_eq!(
            super::format_client_budget_status_label_for_human(&json!({
                "status_label": "   "
            })),
            "ещё нет данных"
        );
        assert_eq!(
            super::format_client_budget_status_label_for_human(&json!({})),
            "ещё нет данных"
        );
    }

    #[test]
    fn format_reply_prefix_for_human_preserves_missing_state() {
        assert_eq!(
            super::format_reply_prefix_for_human(Some("5ч KPI: переплата 2.46%")),
            "5ч KPI: переплата 2.46%"
        );
        assert_eq!(
            super::format_reply_prefix_for_human(Some("   ")),
            "ещё нет данных"
        );
        assert_eq!(super::format_reply_prefix_for_human(None), "ещё нет данных");
    }

    #[test]
    fn append_working_state_warning_to_message_preserves_base_without_warning() {
        assert_eq!(
            super::append_working_state_warning_to_message(
                "Режим целевой экономии переключён на 40%.",
                &json!({})
            ),
            "Режим целевой экономии переключён на 40%."
        );
    }

    #[test]
    fn append_working_state_warning_to_message_appends_degraded_warning() {
        assert_eq!(
            super::append_working_state_warning_to_message(
                "Режим целевой экономии переключён на 40%.",
                &json!({
                    "status": "degraded_after_primary_write",
                    "warning": "client_budget_target.refresh_restore_snapshot degraded"
                })
            ),
            "Режим целевой экономии переключён на 40%. client_budget_target.refresh_restore_snapshot degraded"
        );
    }

    #[test]
    fn append_working_state_warning_to_message_ignores_whitespace_warning() {
        assert_eq!(
            super::append_working_state_warning_to_message(
                "Режим целевой экономии переключён на 40%.",
                &json!({
                    "warning": "   "
                })
            ),
            "Режим целевой экономии переключён на 40%."
        );
    }

    #[test]
    fn append_working_state_warning_to_message_ignores_empty_warning() {
        assert_eq!(
            super::append_working_state_warning_to_message(
                "Режим целевой экономии переключён на 40%.",
                &json!({
                    "warning": ""
                })
            ),
            "Режим целевой экономии переключён на 40%."
        );
    }

    #[test]
    fn format_optional_text_for_prompt_preserves_missing_state() {
        assert_eq!(
            super::format_optional_text_for_prompt(Some("return_required")),
            "return_required"
        );
        assert_eq!(super::format_optional_text_for_prompt(Some("   ")), "?");
        assert_eq!(super::format_optional_text_for_prompt(None), "?");
    }

    #[test]
    fn recommended_workline_headline_from_restore_or_handoff_preserves_missing_state() {
        let handoff = json!({
            "headline": "   "
        });
        assert_eq!(
            super::recommended_workline_headline_from_restore_or_handoff(None, &handoff),
            "ещё нет данных"
        );

        let restore = json!({
            "current_goal": "Same-meter spend control"
        });
        assert_eq!(
            super::recommended_workline_headline_from_restore_or_handoff(Some(&restore), &handoff),
            "Same-meter spend control"
        );
    }

    #[test]
    fn recommended_workline_next_step_from_restore_or_handoff_preserves_missing_state() {
        let handoff = json!({
            "next_step": "   "
        });
        assert_eq!(
            super::recommended_workline_next_step_from_restore_or_handoff(None, &handoff),
            "ещё нет данных"
        );

        let restore = json!({
            "next_step": "Materialize live assistant generation source."
        });
        assert_eq!(
            super::recommended_workline_next_step_from_restore_or_handoff(Some(&restore), &handoff),
            "Materialize live assistant generation source."
        );
    }

    #[test]
    fn compact_chat_runtime_scope_fields_preserve_missing_scope_as_null() {
        let (project_code, namespace_code, project_display_name, namespace_display_name) =
            super::compact_chat_runtime_scope_fields(&json!({
                "project_code": "   ",
                "namespace_code": ""
            }));
        assert!(project_code.is_none());
        assert!(namespace_code.is_none());
        assert!(project_display_name.is_none());
        assert!(namespace_display_name.is_none());

        let (project_code, namespace_code, project_display_name, namespace_display_name) =
            super::compact_chat_runtime_scope_fields(&json!({
                "project_code": "amai",
                "namespace_code": "continuity"
            }));
        assert_eq!(project_code.as_deref(), Some("amai"));
        assert_eq!(namespace_code.as_deref(), Some("continuity"));
        assert_eq!(project_display_name.as_deref(), Some("amai"));
        assert_eq!(namespace_display_name.as_deref(), Some("continuity"));
    }

    #[test]
    fn compact_chat_runtime_fallback_preserves_missing_scope_as_null() {
        let repo = std::env::temp_dir().join(format!(
            "amai-compact-runtime-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(repo.join(".amai/continuity"))
            .expect("create runtime artifact directory");
        let artifact = json!({
            "artifact_version": super::STARTUP_RUNTIME_STATE_ARTIFACT_VERSION,
            "chat_start_restore": {
                "prompt_text": "   "
            },
            "reply_execution_gate": {
                "reply_prefix": "5ч KPI: переплата 2.46%"
            },
            "continuity_startup_summary": {
                "project_code": "   ",
                "namespace_code": "",
                "headline": "   ",
                "next_step": ""
            }
        });
        fs::write(
            startup_runtime_state_artifact_path(repo.as_path()),
            serde_json::to_string_pretty(&artifact).expect("serialize artifact"),
        )
        .expect("write artifact");

        let args = ContinuityCompactChatArgs {
            project: None,
            repo_root: Some(repo.clone()),
            namespace: "continuity".to_string(),
            headline: None,
            next_step: None,
            details_file: None,
            launch_host: false,
            runtime_fallback: true,
            skip_handoff: false,
            json: true,
        };
        let payload = super::compact_chat_payload_from_runtime_artifact(&args, None)
            .expect("compact runtime fallback")
            .expect("payload");

        assert_eq!(
            payload["continuity_compact_chat"]["source_mode"],
            json!("startup_runtime_artifact_fallback")
        );
        assert!(payload["continuity_compact_chat"]["project"]["code"].is_null());
        assert!(payload["continuity_compact_chat"]["project"]["display_name"].is_null());
        assert!(payload["continuity_compact_chat"]["namespace"]["code"].is_null());
        assert!(payload["continuity_compact_chat"]["namespace"]["display_name"].is_null());
        assert!(payload["continuity_compact_chat"]["operator_flow"]["startup_command"].is_null());

        fs::remove_dir_all(&repo).expect("cleanup temp repo");
    }

    #[test]
    fn parse_rendered_transcripts_ignores_blank_projection_entries() {
        let text = "\
- `rendered_transcript`: ``\n\
- `rendered_transcript`: `   `\n\
- `rendered_transcript`: `/tmp/rendered.md`\n";

        let paths = super::parse_rendered_transcripts(text);

        assert_eq!(paths, vec![PathBuf::from("/tmp/rendered.md")]);
        assert_eq!(
            super::summarize_bootstrap(text),
            json!({
                "thread_count": 1,
                "latest_rendered_transcript": "/tmp/rendered.md",
            })
        );
    }

    #[test]
    fn summarize_bootstrap_preserves_missing_latest_rendered_transcript_as_null() {
        let text = "- `thread_count`: `0`\n";

        assert_eq!(
            super::summarize_bootstrap(text),
            json!({
                "thread_count": 0,
                "latest_rendered_transcript": Value::Null,
            })
        );
    }

    #[test]
    fn continuity_eval_probe_details_preserve_missing_state_as_null() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "documents_imported": 1,
            "bootstrap_summary": {
                "details": {
                    "thread_count": 0,
                    "latest_rendered_transcript": ""
                }
            }
        });
        let handoff = json!({
            "headline": "   ",
            "next_step": ""
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "   ",
                "next_step": "",
                "restore_confidence": "missing",
                "state_lineage": {
                    "authoritative_event_id": "   "
                }
            },
            "workspace_restore_pack": {
                "summary": "   ",
                "active_commitments": [],
                "active_constraints": [],
                "important_artifacts": [],
                "procedural_restore_policy": {
                    "raw_procedural_archive_forbidden": true
                }
            }
        });

        let context = ContinuityStartupContext {
            project: project.clone(),
            namespace: namespace.clone(),
            continuity: continuity.clone(),
            handoff_summary: handoff.clone(),
            restore: Some(restore.clone()),
        };
        let chat_start_restore = build_chat_start_restore(
            &context.project,
            &context.namespace,
            &context.continuity,
            &context.handoff_summary,
            context.restore.as_ref(),
        );
        let startup_payload = build_continuity_startup_payload(
            &context,
            context.restore.as_ref(),
            &chat_start_restore,
        )
        .expect("startup payload");
        let startup_probes = startup_payload["continuity_startup"]["canonical_eval"]["probes"]
            .as_array()
            .expect("startup probes");
        assert_eq!(
            startup_payload["continuity_startup"]["handoff_summary"],
            json!({
                "headline": "ещё нет данных",
                "next_step": "ещё нет данных",
            })
        );
        let working_state_probe = startup_probes
            .iter()
            .find(|probe| probe["name"] == json!("working_state_restore_recovered_useful"))
            .expect("working state probe");
        assert!(working_state_probe["details"]["current_goal"].is_null());
        assert!(working_state_probe["details"]["next_step"].is_null());
        assert!(working_state_probe["details"]["authoritative_event_id"].is_null());
        let startup_summary_probe = startup_probes
            .iter()
            .find(|probe| probe["name"] == json!("startup_summary_recovered_useful"))
            .expect("startup summary probe");
        assert!(startup_summary_probe["details"]["headline"].is_null());
        let workspace_probe = startup_probes
            .iter()
            .find(|probe| probe["name"] == json!("workspace_restore_pack_recovered_useful"))
            .expect("workspace probe");
        assert!(workspace_probe["details"]["summary"].is_null());
        assert!(workspace_probe["details"]["procedural_surface"].is_null());
        let startup_restore_presence_probe = startup_probes
            .iter()
            .find(|probe| probe["name"] == json!("working_state_restore_recovered_useful"))
            .expect("startup working_state_restore_recovered_useful probe");
        assert_eq!(
            startup_restore_presence_probe["details"]["restore_confidence"],
            json!("missing")
        );

        let replay_probes = continuity_replay_guard_probes().expect("replay probes");
        let replay_summary = build_continuity_canonical_eval(&replay_probes).expect("replay summary");
        let replay_probe = replay_summary["probes"]
            .as_array()
            .expect("replay summary probes")
            .iter()
            .find(|probe| probe["name"] == json!("handoff_replay_rejected"))
            .expect("handoff replay probe");
        assert_eq!(
            replay_probe["details"]["selected_headline"],
            json!("Fresh handoff")
        );

        let restore_payload = build_continuity_restore_payload(
            &project,
            &namespace,
            &continuity,
            &handoff,
            &restore,
            &chat_start_restore,
        )
        .expect("restore payload");
        let restore_probes = restore_payload["continuity_restore"]["canonical_eval"]["probes"]
            .as_array()
            .expect("restore probes");
        let restore_probe = restore_probes
            .iter()
            .find(|probe| probe["name"] == json!("working_state_restore_recovered_useful"))
            .expect("restore working state probe");
        assert!(restore_probe["details"]["current_goal"].is_null());
        let restore_workspace_probe = restore_probes
            .iter()
            .find(|probe| probe["name"] == json!("workspace_restore_pack_recovered_useful"))
            .expect("restore workspace probe");
        assert!(restore_workspace_probe["details"]["procedural_surface"].is_null());

        let no_restore_context = ContinuityStartupContext {
            project,
            namespace,
            continuity,
            handoff_summary: handoff,
            restore: None,
        };
        let no_restore_chat_start_restore = build_chat_start_restore(
            &no_restore_context.project,
            &no_restore_context.namespace,
            &no_restore_context.continuity,
            &no_restore_context.handoff_summary,
            no_restore_context.restore.as_ref(),
        );
        let no_restore_startup_payload = build_continuity_startup_payload(
            &no_restore_context,
            no_restore_context.restore.as_ref(),
            &no_restore_chat_start_restore,
        )
        .expect("startup payload without restore");
        let no_restore_startup_probe = no_restore_startup_payload["continuity_startup"]
            ["canonical_eval"]["probes"]
            .as_array()
            .expect("no-restore startup probes")
            .iter()
            .find(|probe| probe["name"] == json!("working_state_restore_recovered_useful"))
            .expect("no-restore working state probe");
        assert!(no_restore_startup_probe["details"]["restore_confidence"].is_null());

        let unknown_source_context = ContinuityStartupContext {
            project: no_restore_context.project.clone(),
            namespace: no_restore_context.namespace.clone(),
            continuity: json!({
                "imported_at_epoch_ms": 1_234_567,
                "documents_imported": 1
            }),
            handoff_summary: no_restore_context.handoff_summary.clone(),
            restore: no_restore_context.restore.clone(),
        };
        let unknown_source_chat_start_restore = build_chat_start_restore(
            &unknown_source_context.project,
            &unknown_source_context.namespace,
            &unknown_source_context.continuity,
            &unknown_source_context.handoff_summary,
            unknown_source_context.restore.as_ref(),
        );
        let unknown_source_startup_payload = build_continuity_startup_payload(
            &unknown_source_context,
            unknown_source_context.restore.as_ref(),
            &unknown_source_chat_start_restore,
        )
        .expect("startup payload without continuity source mode");
        assert!(
            unknown_source_startup_payload["continuity_startup"]["continuity_source"]["mode"]
                .is_null()
        );
        assert!(
            unknown_source_startup_payload["continuity_startup"]["continuity_source"]
                ["source_namespace_code"]
                .is_null()
        );
        let unknown_source_probe = unknown_source_startup_payload["continuity_startup"]
            ["canonical_eval"]["probes"]
            .as_array()
            .expect("unknown-source startup probes")
            .iter()
            .find(|probe| probe["name"] == json!("startup_summary_recovered_useful"))
            .expect("startup summary probe for unknown source");
        assert!(unknown_source_probe["details"]["continuity_source_mode"].is_null());
        assert!(
            unknown_source_probe["details"]["continuity_source_namespace_code"].is_null()
        );
    }

    #[test]
    fn continuity_restore_payload_normalizes_blank_working_state_projection() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
                    "thread_count": 0,
                    "latest_rendered_transcript": ""
                }
            }
        });
        let handoff = json!({
            "headline": "Current line",
            "next_step": "Next step"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "   ",
                "next_step": "",
                "restore_confidence": "high",
                "state_lineage": {
                    "authoritative_event_id": "   ",
                    "session_id": "sess-1"
                }
            },
            "workspace_restore_pack": {
                "active_commitments": [],
                "active_constraints": [],
                "important_artifacts": [],
                "procedural_restore_policy": {
                    "raw_procedural_archive_forbidden": true
                }
            }
        });
        let chat_start_restore = json!({
            "chat_start_restore": {
                "headline": "Current line",
                "next_step": "Next step",
                "prompt_text": "CHAT_START_RESTORE\nCurrent line\nNext step"
            }
        });

        let payload = build_continuity_restore_payload(
            &project,
            &namespace,
            &continuity,
            &handoff,
            &restore,
            &chat_start_restore,
        )
        .expect("payload");

        assert!(payload["working_state_restore"]["current_goal"].is_null());
        assert!(payload["working_state_restore"]["next_step"].is_null());
        assert!(
            payload["working_state_restore"]["state_lineage"]["authoritative_event_id"].is_null()
        );
        assert_eq!(
            payload["working_state_restore"]["state_lineage"]["session_id"],
            json!("sess-1")
        );
        assert_eq!(
            payload["working_state_restore"]["restore_confidence"],
            json!("high")
        );
    }

    #[test]
    fn continuity_restore_payload_normalizes_blank_workspace_restore_pack_projection() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
                    "thread_count": 0,
                    "latest_rendered_transcript": ""
                }
            }
        });
        let handoff = json!({
            "headline": "Current line",
            "next_step": "Next step"
        });
        let restore = json!({
            "working_state_restore": {
                "restore_confidence": "high",
                "state_lineage": {
                    "session_id": "sess-1"
                }
            },
            "workspace_restore_pack": {
                "summary": "   ",
                "active_commitments": [],
                "active_constraints": [],
                "important_artifacts": [],
                "procedural_restore_policy": {
                    "raw_procedural_archive_forbidden": true
                }
            }
        });
        let chat_start_restore = json!({
            "chat_start_restore": {
                "headline": "Current line",
                "next_step": "Next step",
                "prompt_text": "CHAT_START_RESTORE\nCurrent line\nNext step"
            }
        });

        let payload = build_continuity_restore_payload(
            &project,
            &namespace,
            &continuity,
            &handoff,
            &restore,
            &chat_start_restore,
        )
        .expect("payload");

        assert!(payload["workspace_restore_pack"]["summary"].is_null());
        assert_eq!(payload["workspace_restore_pack"]["active_commitments"], json!([]));
        assert_eq!(payload["workspace_restore_pack"]["active_constraints"], json!([]));
        assert_eq!(payload["workspace_restore_pack"]["important_artifacts"], json!([]));
    }

    #[test]
    fn continuity_startup_payload_normalizes_blank_working_state_projection() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "documents_imported": 1,
            "bootstrap_summary": {
                "details": {
                    "thread_count": 0,
                    "latest_rendered_transcript": ""
                }
            }
        });
        let handoff = json!({
            "headline": "Current line",
            "next_step": "Next step"
        });
        let restore = json!({
            "working_state_restore": {
                "current_goal": "   ",
                "next_step": "",
                "restore_confidence": "high",
                "state_lineage": {
                    "authoritative_event_id": "   ",
                    "session_id": "sess-1"
                }
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

        let payload = build_continuity_startup_payload(
            &context,
            context.restore.as_ref(),
            &chat_start_restore,
        )
        .expect("payload");

        assert!(payload["working_state_restore"]["current_goal"].is_null());
        assert!(payload["working_state_restore"]["next_step"].is_null());
        assert!(
            payload["working_state_restore"]["state_lineage"]["authoritative_event_id"].is_null()
        );
        assert_eq!(
            payload["working_state_restore"]["state_lineage"]["session_id"],
            json!("sess-1")
        );
        assert_eq!(
            payload["working_state_restore"]["restore_confidence"],
            json!("high")
        );
    }

    #[test]
    fn continuity_startup_payload_normalizes_blank_workspace_restore_pack_projection() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
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
            "documents_imported": 1,
            "bootstrap_summary": {
                "details": {
                    "thread_count": 0,
                    "latest_rendered_transcript": ""
                }
            }
        });
        let handoff = json!({
            "headline": "Current line",
            "next_step": "Next step"
        });
        let restore = json!({
            "working_state_restore": {
                "restore_confidence": "high",
                "state_lineage": {
                    "session_id": "sess-1"
                }
            },
            "workspace_restore_pack": {
                "summary": "   ",
                "active_commitments": [],
                "active_constraints": [],
                "important_artifacts": [],
                "procedural_restore_policy": {
                    "raw_procedural_archive_forbidden": true
                }
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

        let payload = build_continuity_startup_payload(
            &context,
            context.restore.as_ref(),
            &chat_start_restore,
        )
        .expect("payload");

        assert!(payload["workspace_restore_pack"]["summary"].is_null());
        assert_eq!(payload["workspace_restore_pack"]["active_commitments"], json!([]));
        assert_eq!(payload["workspace_restore_pack"]["active_constraints"], json!([]));
        assert_eq!(payload["workspace_restore_pack"]["important_artifacts"], json!([]));
    }

    #[test]
    fn continuity_import_payload_normalizes_blank_active_workline_summary() {
        let payload = json!({
            "continuity_import": {
                "active_workline_summary": {
                    "active_workline_file": Value::Null,
                    "details": {
                        "headline": "   ",
                        "next_step": ""
                    }
                }
            }
        });

        let normalized = json!({
            "continuity_import": {
                "active_workline_summary": {
                    "active_workline_file": Value::Null,
                    "details": super::normalized_handoff_summary_json(
                        payload["continuity_import"]["active_workline_summary"]["details"]["headline"].as_str(),
                        payload["continuity_import"]["active_workline_summary"]["details"]["next_step"].as_str(),
                    )
                }
            }
        });

        assert_eq!(
            normalized["continuity_import"]["active_workline_summary"]["details"],
            json!({
                "headline": "ещё нет данных",
                "next_step": "ещё нет данных",
            })
        );
    }

    #[test]
    fn inspect_startup_runtime_state_fails_closed_on_required_task_set_gate_drift() {
        let unique = format!(
            "amai-startup-runtime-audit-drift-{}",
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
                "next_step": "Finish multi-line obligation restore.",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
                "execctl_resume_state": "clear",
                "startup_next_action": {
                    "action_kind": "honor_required_task_set",
                    "blocking": true,
                    "headline": "Current active line",
                    "next_step": "Fix card A",
                    "required_task_set": [
                        "Fix card A",
                        "Fix card B"
                    ],
                    "required_task_set_summary": "2 задач(и): Fix card A"
                },
                "required_return_task": Value::Null,
                "required_task_set": [
                    "Fix card A",
                    "Fix card B"
                ],
                "required_task_set_summary": "2 задач(и): Fix card A",
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "open_tasks_count": 2
                },
                "project_task_tree_summary": "active: Current active line + required_task_set(2)",
                "project_task_ledger": {
                    "open_tasks_count": 2
                },
                "project_task_ledger_summary": "active: Current active line; historical_handoffs(1)"
            },
            "working_state_restore": {
                "state_lineage": {
                    "authoritative_event_id": "evt_456",
                    "session_id": "sess_456"
                }
            }
        });
        let mut artifact = build_startup_runtime_state_artifact(repo.as_path(), &payload, 42)
            .expect("startup runtime state artifact");
        artifact["startup_execution_gate"]["required_task_set_count"] = json!(1);
        artifact["gate_semantics_consistent"] = json!(false);
        fs::write(
            startup_runtime_state_artifact_path(repo.as_path()),
            serde_json::to_string_pretty(&artifact).expect("serialize artifact"),
        )
        .expect("write artifact");

        let audit = inspect_startup_runtime_state(repo.as_path()).expect("startup runtime audit");
        assert_eq!(audit.status, "startup_runtime_state_drift");
        assert_eq!(audit.required_task_set_count_consistent, Some(false));
        assert_eq!(audit.required_task_set_presence_consistent, Some(true));
        assert_eq!(audit.must_preserve_required_task_set_consistent, Some(true));
        assert_eq!(
            audit.artifact_gate_semantics_consistent_matches_recomputed,
            Some(true)
        );

        fs::remove_dir_all(&repo).expect("cleanup temp repo");
    }

    #[test]
    fn inspect_startup_runtime_state_fails_closed_on_missing_required_task_set() {
        let unique = format!(
            "amai-startup-runtime-audit-missing-task-set-{}",
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
                "next_step": "Continue foundation work.",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
                "execctl_resume_state": "clear",
                "startup_next_action": {
                    "action_kind": "continue_active_workline",
                    "blocking": false,
                    "headline": "Current active line",
                    "next_step": "Continue foundation work."
                },
                "required_return_task": Value::Null,
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner",
                    "headline": "Current active line"
                },
                "project_task_tree": {
                    "open_tasks_count": 1
                },
                "project_task_tree_summary": "active: Current active line",
                "project_task_ledger": {
                    "open_tasks_count": 1
                },
                "project_task_ledger_summary": "active: Current active line"
            },
            "working_state_restore": {
                "state_lineage": {
                    "authoritative_event_id": "evt_missing_task_set",
                    "session_id": "sess_missing_task_set"
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
        assert_eq!(audit.status, "startup_runtime_state_drift");
        assert_eq!(audit.required_task_set_field_present, Some(true));
        assert_eq!(audit.required_task_set_count_consistent, Some(false));
        assert_eq!(audit.required_task_set_presence_consistent, Some(false));
        assert_eq!(
            audit.must_preserve_required_task_set_consistent,
            Some(false)
        );
        assert_eq!(audit.gate_semantics_consistent, Some(false));

        fs::remove_dir_all(&repo).expect("cleanup temp repo");
    }

    #[test]
    fn human_surface_required_task_set_summary_is_projection_only() {
        let node = json!({
            "required_task_set": [
                "Fix card A",
                "Fix card B"
            ],
            "required_task_set_summary": Value::Null
        });

        assert_eq!(
            summarize_required_task_set_for_human_surface(&node).as_deref(),
            Some("2 задач(и): Fix card A")
        );

        let missing = json!({});
        assert!(summarize_required_task_set_for_human_surface(&missing).is_none());
    }

    #[test]
    fn compact_runtime_surfaces_preserve_required_task_set_projection() {
        let action = json!({
            "action_kind": "honor_required_task_set",
            "blocking": true,
            "required_task_set": ["Fix card A", "Fix card B"],
            "required_task_set_summary": "2 задач(и): Fix card A"
        });
        let compact_action = compact_startup_runtime_startup_next_action(&action);
        assert_eq!(
            compact_action["required_task_set"],
            json!(["Fix card A", "Fix card B"])
        );
        assert_eq!(
            compact_action["required_task_set_summary"],
            json!("2 задач(и): Fix card A")
        );

        let obligation = json!({
            "resume_state": "clear",
            "required_task_set_count": 2,
            "required_task_set": ["Fix card A", "Fix card B"],
            "required_task_set_summary": "2 задач(и): Fix card A"
        });
        let compact_obligation = compact_startup_runtime_execctl_resume_obligation(&obligation);
        assert_eq!(compact_obligation["required_task_set_count"], json!(2));
        assert_eq!(
            compact_obligation["required_task_set"],
            json!(["Fix card A", "Fix card B"])
        );
        assert_eq!(
            compact_obligation["required_task_set_summary"],
            json!("2 задач(и): Fix card A")
        );
    }

    #[test]
    fn summarize_execctl_resume_obligation_preserves_missing_required_task_set_as_null() {
        let obligation = summarize_execctl_resume_obligation(&json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "required_return_task": {
                "headline": "Pending line",
                "next_step": "Close drift."
            }
        }));

        assert!(obligation["pending_return_count"].is_null());
        assert_eq!(obligation["required_task_set_count"], Value::Null);
        assert_eq!(obligation["required_task_set"], Value::Null);
        assert_eq!(obligation["required_task_set_summary"], Value::Null);
    }

    #[test]
    fn summarize_execctl_resume_obligation_preserves_missing_resume_state_as_null() {
        let obligation = summarize_execctl_resume_obligation(&json!({
            "resume_state": "   ",
            "no_silent_drop": true,
            "pending_return_count": 1,
            "required_return_task": {
                "headline": "Pending line",
                "next_step": "Close drift."
            }
        }));

        assert!(obligation["resume_state"].is_null());
    }

    #[test]
    fn summarize_execctl_resume_obligation_preserves_missing_contract_as_null_resume_state() {
        let obligation = summarize_execctl_resume_obligation(&Value::Null);

        assert!(obligation["resume_state"].is_null());
        assert_eq!(obligation["pending_return_count"], json!(0));
        assert_eq!(obligation["required_task_set_count"], json!(0));
    }

    #[test]
    fn execctl_resume_summaries_ignore_whitespace_only_text() {
        let obligation = summarize_execctl_resume_obligation(&json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "pending_return_count": 1,
            "active_task": {
                "headline": "   "
            },
            "required_return_task": {
                "headline": "   ",
                "next_step": "  "
            },
            "required_task_set_count": 1,
            "required_task_set": ["   "],
            "required_task_set_summary": "   "
        }));

        assert!(obligation["active_task_headline"].is_null());
        assert!(obligation["required_return_headline"].is_null());
        assert!(obligation["required_return_next_step"].is_null());
        assert!(obligation["required_task_set_summary"].is_null());

        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };

        let action = super::default_startup_next_action(
            "Fallback current goal",
            "Fallback next step",
            &project,
            &namespace,
            &obligation,
            None,
        );

        assert_eq!(action["headline"], json!("Fallback current goal"));
        assert_eq!(action["next_step"], json!("Fallback next step"));
    }

    #[test]
    fn startup_state_cli_json_embeds_runtime_state_without_top_level_artifact_duplication() {
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
            required_task_set_field_present: Some(true),
            required_task_set_summary_field_present: Some(true),
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
            required_task_set_count_consistent: Some(true),
            required_task_set_presence_consistent: Some(true),
            must_preserve_required_task_set_consistent: Some(true),
            required_action_kind_when_resume_required: Some(
                "resume_required_return_task".to_string(),
            ),
            no_silent_drop: Some(true),
            artifact_gate_semantics_consistent_present: Some(true),
            artifact_gate_semantics_consistent_matches_recomputed: Some(true),
            gate_semantics_consistent: Some(true),
        };
        let artifact = json!({
            "artifact_version": super::STARTUP_RUNTIME_STATE_ARTIFACT_VERSION,
            "client_budget_guard": {
                "status_label": "сожми текущий чат сейчас"
            },
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "action_kind": "rotate_chat_for_client_budget"
            },
            "blocking_reply_contract": {
                "response_kind": "rotate_chat_only"
            },
            "startup_execution_gate": {
                "gate_version": "startup-execution-gate-v1",
                "action_kind": "rotate_chat_for_client_budget"
            },
            "continuity_startup_summary": {
                "startup_next_action": {
                    "action_kind": "rotate_chat_for_client_budget"
                },
                "required_return_task": Value::Null,
                "required_task_set": ["Fix card A", "Fix card B"],
                "required_task_set_summary": "2 задач(и): Fix card A",
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

        assert!(payload["client_budget_guard"].is_null());
        assert_eq!(
            payload["startup_runtime_state"]["client_budget_guard"]["status_label"],
            json!("сожми текущий чат сейчас")
        );
        assert_eq!(
            payload["startup_runtime_state"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            payload["startup_runtime_state"]["blocking_reply_contract"]["response_kind"],
            json!("rotate_chat_only")
        );
        assert_eq!(
            payload["startup_runtime_state"]["startup_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            payload["startup_runtime_state"]["required_task_set_field_present"],
            json!(true)
        );
        assert_eq!(
            payload["startup_runtime_state"]["required_task_set"],
            json!(["Fix card A", "Fix card B"])
        );
        assert_eq!(
            payload["startup_runtime_state"]["required_task_set_summary"],
            json!("2 задач(и): Fix card A")
        );
        assert_eq!(
            payload["startup_runtime_state"]["required_task_set_summary_field_present"],
            json!(true)
        );
        assert_eq!(
            payload["startup_runtime_state"]["required_task_set_count_consistent"],
            json!(true)
        );
        assert_eq!(
            payload["startup_runtime_state"]["required_task_set_presence_consistent"],
            json!(true)
        );
        assert_eq!(
            payload["startup_runtime_state"]["must_preserve_required_task_set_consistent"],
            json!(true)
        );
        assert_eq!(
            payload["startup_runtime_state_audit"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            payload["startup_runtime_state_audit"]["status"],
            json!("ok")
        );
        assert_eq!(
            payload["startup_runtime_state_audit"]["required_task_set_field_present"],
            json!(true)
        );
        assert_eq!(
            payload["startup_runtime_state_audit"]["required_task_set_count_consistent"],
            json!(true)
        );
    }

    #[test]
    fn startup_runtime_state_artifact_normalizes_blank_chat_start_restore_projection() {
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
                },
                "headline": "Current active line",
                "next_step": "Continue foundation work.",
                "restore_confidence": "high",
                "prompt_text_present": true,
                "execctl_resume_state": "clear",
                "execctl_resume_contract_summary": "clear(0)",
                "execctl_resume_obligation": {
                    "resume_state": "clear",
                    "required_task_set_count": 0,
                    "required_task_set": [],
                    "required_task_set_summary": Value::Null
                },
                "startup_execution_gate": {
                    "action_kind": "continue_active_workline",
                    "blocking": false,
                    "must_follow_startup_next_action": true,
                    "unrelated_work_allowed": false,
                    "must_read_prompt_text_before_reply": true,
                    "required_action_kind_when_resume_required": "resume_required_return_task",
                    "no_silent_drop": true,
                    "required_return_task_present": false,
                    "required_task_set_count": 0,
                    "required_task_set_present": false,
                    "must_preserve_required_task_set": false
                },
                "startup_next_action": {
                    "action_kind": "continue_active_workline",
                    "blocking": false,
                    "headline": "Current active line",
                    "next_step": "Continue foundation work."
                },
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner",
                    "headline": "Current active line"
                },
                "required_return_task": Value::Null,
                "required_task_set": [],
                "required_task_set_summary": Value::Null,
                "project_task_tree": {
                    "open_tasks_count": 1
                },
                "project_task_tree_summary": "active: Current active line",
                "project_task_ledger": {
                    "open_tasks_count": 1
                },
                "project_task_ledger_summary": "active: Current active line"
            },
            "chat_start_restore": {
                "headline": "   ",
                "next_step": "",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line"
            },
            "working_state_restore": {
                "state_lineage": {
                    "authoritative_event_id": "evt_123",
                    "session_id": "sess_123"
                }
            }
        });

        let artifact =
            build_startup_runtime_state_artifact(Path::new("/tmp/amai-art"), &payload, 42)
                .expect("startup runtime state artifact");

        assert!(artifact["chat_start_restore"]["headline"].is_null());
        assert!(artifact["chat_start_restore"]["next_step"].is_null());
        assert_eq!(
            artifact["chat_start_restore"]["restore_confidence"],
            json!("high")
        );
    }

    #[test]
    fn startup_runtime_state_artifact_preserves_missing_gate_resume_state_as_null() {
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
                },
                "headline": "Current active line",
                "next_step": "Continue foundation work.",
                "restore_confidence": "high",
                "prompt_text_present": true,
                "execctl_resume_state": Value::Null,
                "execctl_resume_contract_summary": Value::Null,
                "execctl_resume_obligation": {
                    "resume_state": Value::Null,
                    "required_task_set_count": Value::Null,
                    "required_task_set": Value::Null,
                    "required_task_set_summary": Value::Null
                },
                "startup_execution_gate": {
                    "action_kind": "continue_active_workline",
                    "blocking": false,
                    "must_follow_startup_next_action": false,
                    "unrelated_work_allowed": true,
                    "must_read_prompt_text_before_reply": true,
                    "required_action_kind_when_resume_required": "resume_required_return_task",
                    "no_silent_drop": true,
                    "required_return_task_present": false,
                    "required_task_set_count": Value::Null,
                    "required_task_set_present": Value::Null,
                    "must_preserve_required_task_set": Value::Null
                },
                "startup_next_action": {
                    "action_kind": "continue_active_workline",
                    "blocking": false,
                    "headline": "Current active line",
                    "next_step": "Continue foundation work."
                },
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner",
                    "headline": "Current active line"
                },
                "required_return_task": {
                    "headline": "   ",
                    "next_step": ""
                },
                "required_task_set": Value::Null,
                "required_task_set_summary": Value::Null,
                "project_task_tree": {
                    "open_tasks_count": 1
                },
                "project_task_tree_summary": "active: Current active line",
                "project_task_ledger": {
                    "open_tasks_count": 1
                },
                "project_task_ledger_summary": "active: Current active line"
            },
            "chat_start_restore": {
                "headline": "Current active line",
                "next_step": "Continue foundation work.",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
                "execctl_resume_state": "   ",
                "required_return_task": {
                    "headline": "   ",
                    "next_step": ""
                },
                "startup_next_action": {
                    "action_kind": "continue_active_workline",
                    "blocking": false,
                    "headline": "Current active line",
                    "next_step": "Continue foundation work."
                },
                "required_task_set": Value::Null
            },
            "working_state_restore": {
                "state_lineage": {
                    "authoritative_event_id": "evt_123",
                    "session_id": "sess_123"
                }
            }
        });

        let artifact =
            build_startup_runtime_state_artifact(Path::new("/tmp/amai-art"), &payload, 42)
                .expect("startup runtime state artifact");

        assert!(artifact["startup_execution_gate"]["resume_state"].is_null());
        assert!(artifact["startup_execution_gate"]["required_return_task_headline"].is_null());
        assert!(artifact["startup_execution_gate"]["required_return_task_next_step"].is_null());
    }

    #[test]
    fn startup_runtime_state_artifact_preserves_missing_startup_action_as_null_gate_fields() {
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
                },
                "headline": "Current active line",
                "next_step": "Continue foundation work.",
                "restore_confidence": "high",
                "prompt_text_present": true,
                "execctl_resume_state": Value::Null,
                "execctl_resume_contract_summary": Value::Null,
                "execctl_resume_obligation": {
                    "resume_state": Value::Null,
                    "required_task_set_count": Value::Null,
                    "required_task_set": Value::Null,
                    "required_task_set_summary": Value::Null
                },
                "startup_execution_gate": {
                    "action_kind": Value::Null,
                    "blocking": Value::Null,
                    "must_follow_startup_next_action": Value::Null,
                    "unrelated_work_allowed": Value::Null,
                    "must_read_prompt_text_before_reply": true,
                    "required_action_kind_when_resume_required": "resume_required_return_task",
                    "no_silent_drop": true,
                    "required_return_task_present": false,
                    "required_task_set_count": Value::Null,
                    "required_task_set_present": Value::Null,
                    "must_preserve_required_task_set": Value::Null
                },
                "startup_next_action": Value::Null,
                "execctl_active_lease": {
                    "lease_owner_state": "same_session_owner",
                    "headline": "Current active line"
                },
                "required_return_task": Value::Null,
                "required_task_set": Value::Null,
                "required_task_set_summary": Value::Null,
                "project_task_tree": {
                    "open_tasks_count": 1
                },
                "project_task_tree_summary": "active: Current active line",
                "project_task_ledger": {
                    "open_tasks_count": 1
                },
                "project_task_ledger_summary": "active: Current active line"
            },
            "chat_start_restore": {
                "headline": "Current active line",
                "next_step": "Continue foundation work.",
                "restore_confidence": "high",
                "prompt_text": "CHAT_START_RESTORE\nCurrent active line",
                "required_task_set": Value::Null
            },
            "working_state_restore": {
                "state_lineage": {
                    "authoritative_event_id": "evt_123",
                    "session_id": "sess_123"
                }
            }
        });

        let artifact =
            build_startup_runtime_state_artifact(Path::new("/tmp/amai-art"), &payload, 42)
                .expect("startup runtime state artifact");

        assert!(artifact["startup_execution_gate"]["action_kind"].is_null());
        assert!(artifact["startup_execution_gate"]["blocking"].is_null());
        assert!(artifact["startup_execution_gate"]["must_follow_startup_next_action"].is_null());
        assert!(artifact["startup_execution_gate"]["unrelated_work_allowed"].is_null());
    }

    #[test]
    fn default_startup_next_action_keeps_rotate_now_warning_as_resume_obligation() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let execctl_resume_obligation = json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "active_task_headline": "Project relocation contour",
            "required_return_headline": "Same-meter spend control",
            "required_return_next_step": "Materialize live assistant generation source."
        });
        let client_budget_guard = json!({
            "should_rotate_chat_now": true,
            "should_rotate_chat_soon": true,
            "status_label": "сожми текущий чат сейчас",
            "note": "live client budget is already under pressure"
        });

        let action = super::default_startup_next_action(
            "Project relocation contour",
            "Dovetail runtime auto-start guarantees.",
            &project,
            &namespace,
            &execctl_resume_obligation,
            Some(&client_budget_guard),
        );

        assert_eq!(action["action_kind"], json!("resume_required_return_task"));
        assert_eq!(action["blocking"], json!(true));
        assert_eq!(action["resume_state"], json!("return_required"));
        assert!(action["preserves_return_obligation"].is_null());
        assert!(action["action_bundle"].is_null());
    }

    #[test]
    fn default_startup_next_action_keeps_rotate_soon_as_advisory() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let execctl_resume_obligation = json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "active_task_headline": "Project relocation contour",
            "required_return_headline": "Same-meter spend control",
            "required_return_next_step": "Materialize live assistant generation source."
        });
        let client_budget_guard = json!({
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true,
            "status_label": "сожми текущий чат",
            "note": "soft rotate recommendation only",
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false
            }
        });

        let action = super::default_startup_next_action(
            "Project relocation contour",
            "Dovetail runtime auto-start guarantees.",
            &project,
            &namespace,
            &execctl_resume_obligation,
            Some(&client_budget_guard),
        );

        assert_eq!(action["action_kind"], json!("resume_required_return_task"));
        assert_eq!(action["blocking"], json!(true));
    }

    #[test]
    fn default_startup_next_action_keeps_global_budget_wait_as_advisory() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let execctl_resume_obligation = json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "active_task_headline": "Project relocation contour",
            "required_return_headline": Value::Null,
            "required_return_next_step": Value::Null
        });
        let client_budget_guard = json!({
            "status_label": "глобальный лимит клиента почти исчерпан",
            "note": "global client budget is almost exhausted",
            "reply_execution_gate": {
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true
            }
        });

        let action = super::default_startup_next_action(
            "Project relocation contour",
            "Dovetail runtime auto-start guarantees.",
            &project,
            &namespace,
            &execctl_resume_obligation,
            Some(&client_budget_guard),
        );

        assert_eq!(action["action_kind"], json!("continue_active_workline"));
        assert_eq!(action["blocking"], json!(false));
    }

    #[test]
    fn default_startup_next_action_preserves_missing_resume_state_as_null() {
        let project = ProjectRecord {
            project_id: uuid::Uuid::new_v4(),
            code: "art".to_string(),
            display_name: "Art".to_string(),
            repo_root: "/home/art/Art".to_string(),
            visibility_scope: "project_shared".to_string(),
            updated_at: String::new(),
        };
        let namespace = NamespaceRecord {
            namespace_id: uuid::Uuid::new_v4(),
            code: "continuity".to_string(),
            display_name: "Continuity".to_string(),
            retrieval_mode: "local_strict".to_string(),
        };
        let execctl_resume_obligation = json!({
            "resume_state": "   ",
            "no_silent_drop": true,
            "active_task_headline": "Project relocation contour",
            "required_return_headline": "Same-meter spend control",
            "required_return_next_step": "Materialize live assistant generation source."
        });

        let action = super::default_startup_next_action(
            "Project relocation contour",
            "Dovetail runtime auto-start guarantees.",
            &project,
            &namespace,
            &execctl_resume_obligation,
            None,
        );

        assert_eq!(action["action_kind"], json!("continue_active_workline"));
        assert_eq!(action["blocking"], json!(false));
        assert!(action["resume_state"].is_null());
    }

    #[test]
    fn compact_startup_runtime_action_bundle_trims_exhausted_same_thread_surface() {
        let bundle = json!({
            "bundle_version": "rotate-chat-action-bundle-v1",
            "ready_for_automation": true,
            "preserves_return_obligation": true,
            "host_current_thread_control": {
                "available": false,
                "automation_ready": false,
                "button_label": "Open compact window",
                "command_id": "hotkey-window-open-current",
                "control_kind": "hotkey_window_open_current",
                "thread_id": "thread-current",
                "host_context_compaction_stage": "preserve",
                "feedback_pending": false,
                "measurement_pending": false,
                "retry_allowed": false,
                "retry_blocked_reason": "rotate fallback already primary",
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "effect_summary": "Long prose that should not survive compact startup runtime output.",
                "selection_reason": "protect_recent_host_compaction_gain",
                "availability_state": "exhausted_after_verified_failure",
                "surface_exhausted_after_verified_failure": true,
                "alternate_controls": [
                    {
                        "button_label": "Open thread overlay",
                        "command_id": "thread-overlay-open-current",
                        "control_kind": "thread_overlay_open_current"
                    }
                ]
            },
            "operator_flow": {
                "primary_command_kind": "rotate_helper_command",
                "primary_command": "'amai' 'continuity' 'rotate-chat'",
                "rotate_helper_command": "'amai' 'continuity' 'rotate-chat'",
                "startup_command": "'amai' 'continuity' 'startup'"
            }
        });

        let compact = super::compact_startup_runtime_startup_action_bundle(&bundle);

        assert_eq!(
            compact["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            compact["host_current_thread_control"]["availability_state"],
            json!("exhausted_after_verified_failure")
        );
        assert!(
            compact["host_current_thread_control"]
                .get("effect_summary")
                .is_none()
        );
        assert!(
            compact["host_current_thread_control"]
                .get("alternate_controls")
                .is_none()
        );
        assert_eq!(
            compact["operator_flow"]["rotate_helper_command"],
            json!("amai continuity rotate-chat")
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
