use crate::codex_threads;
use crate::config::discover_repo_root;
use crate::postgres::{
    self, ExecCtlTaskLeaseRecord, ExecCtlTaskLedgerEntryRecord, NamespaceRecord,
    ObservabilitySnapshotRecord, ProjectRecord,
};
use crate::retrieval_science;
use crate::token_budget;
use crate::workspace_graph;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::env;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;
use uuid::Uuid;

const WORKING_STATE_EVENT_KIND: &str = "working_state_event";
const WORKING_STATE_RESTORE_KIND: &str = "working_state_restore";
const SESSION_GAP_MS: u64 = 30 * 60 * 1000;
const MAX_RESTORE_EVENTS: i64 = 120;
const MAX_RECENT_ACTIONS: usize = 8;
const MAX_RECENT_QUERIES: usize = 6;
const MAX_ACTIVE_FILES: usize = 8;
const MAX_OPEN_QUESTIONS: usize = 6;
const MAX_MATERIALIZED_NOTES: usize = 6;
const MAX_REQUIRED_TASK_SET: usize = 12;
const MAX_RECENT_DECISION_TRACES: usize = 3;
const MAX_PENDING_RETURN_QUEUE: usize = 6;
const MAX_EXECCTL_LEDGER_ENTRIES: i64 = 256;
const MAX_PERSISTED_PROJECT_TASK_LEDGER_HISTORICAL_ENTRIES: usize = 5;
const RESTORE_EXECUTION_CARD_THREAD_WINDOW_SECONDS: i64 = 30 * 60;
const WORKING_STATE_RESTORE_REFRESH_MIN_INTERVAL_MS: u64 = 30_000;
const EXECCTL_LEASE_TTL_MS: u64 = SESSION_GAP_MS;
const EXECCTL_LEASE_HEARTBEAT_MIN_INTERVAL_MS: u64 = 5 * 60 * 1000;
const PROJECT_TASK_TREE_VERSION: &str = "project-task-tree-v1";
const PROJECT_TASK_LEDGER_VERSION: &str = "project-task-ledger-v2";
const WORKSPACE_RESTORE_PACK_VERSION: &str = "workspace-restore-pack-v1";
const WORKSPACE_RESTORE_PACK_ENVELOPE_VERSION: &str = "restore-pack-envelope-v2";
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION: &str =
    "client-budget-blocked-reply-v1";
pub(crate) const CLIENT_REPLY_BUDGET_CONTRACT_VERSION: &str = "client-reply-budget-v1";
pub(crate) const CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND: &str = "rotate_chat_only";
pub(crate) const CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND: &str = "wait_for_budget_only";
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND: &str =
    CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND;
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES: u64 = 1;
pub(crate) const CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE: &str = "Этот чат жжёт внешний лимит клиента: сначала сожми текущий чат; continuity startup используй только если fallback действительно нужен.";
pub(crate) const CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE: &str = "Внешний лимит клиента почти исчерпан во всём клиенте. Не продолжай содержательный ответ, дождись восстановления окна лимита.";
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE: &str =
    CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE;
pub(crate) const GLOBAL_CLIENT_LIMIT_SOURCE_KIND: &str =
    "latest_observed_client_limits_without_current_thread_binding";
pub(crate) const GLOBAL_CLIENT_LIMIT_SOURCE_SUMMARY: &str = "При отсутствии current-thread binding Amai использует только последнее observed значение client limits. Этого достаточно для global warning hint и hard wait при критическом исчерпании, но недостаточно для thread-local rotate pressure.";
pub(crate) const CLIENT_REPLY_BUDGET_MODE_NORMAL: &str = "normal";
pub(crate) const CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL: &str = "compact_high_signal";
pub(crate) const DEFAULT_CLIENT_BUDGET_TARGET_PERCENT: u64 = 90;
pub(crate) const MAX_CLIENT_BUDGET_TARGET_PERCENT: u64 = 90;
pub(crate) const CLIENT_BUDGET_TARGET_STEP_PERCENT: u64 = 10;
pub(crate) const HOST_CURRENT_THREAD_CONTROL_KIND: &str = "thread_overlay_open_current";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_HOST_SURFACE_KIND: &str =
    "codex_webview_internal_command";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_COMMAND_ID: &str = "thread-overlay-open-current";
pub(crate) const HOST_CURRENT_THREAD_COMPACT_WINDOW_KIND: &str = "hotkey_window_open_current";

fn continuity_profile_enabled() -> bool {
    env::var("AMAI_PROFILE_CONTINUITY")
        .ok()
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn continuity_profile_log(stage: &str, elapsed_ms: u128, extra: &str) {
    if continuity_profile_enabled() {
        eprintln!("[amai-continuity-profile] {stage} elapsed_ms={elapsed_ms} {extra}");
    }
}

async fn mirror_handoff_into_commitment_graph(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    event_id: &str,
    snapshot_id: Uuid,
    agent_scope: &str,
    session_id: &str,
    thread_id: Option<&str>,
    headline: &str,
    next_step: &str,
    summary: &str,
    local_path: &str,
    pending_return_queue: &Value,
    recorded_at_epoch_ms: i64,
) -> Result<()> {
    if postgres::find_task_node_by_task_key(
        db,
        project.project_id,
        namespace.namespace_id,
        event_id,
    )
    .await?
    .is_some()
    {
        return Ok(());
    }

    let pending_return_count = pending_return_queue
        .as_array()
        .map(|items| items.len() as i32)
        .unwrap_or(0);
    let execution_state = if pending_return_count > 0 {
        "blocked"
    } else {
        "active"
    };
    let memory_item_metadata = json!({
        "source_kind": "continuity_handoff",
        "source_event_id": event_id,
        "agent_scope": agent_scope,
        "session_id": session_id,
        "thread_id": thread_id,
        "local_path": local_path,
    });
    let source_event_ids = vec![event_id.to_string()];
    let artifact_refs = vec![format!("file://{local_path}")];
    let message_refs = thread_id
        .map(|thread| vec![format!("thread:{thread}")])
        .unwrap_or_default();
    let evidence_span = json!({
        "event_id": event_id,
        "snapshot_id": snapshot_id,
        "thread_id": thread_id,
    });
    let imported_from = json!({});
    let memory_item = postgres::create_memory_item(
        db,
        &project.code,
        &namespace.code,
        &postgres::MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "task",
            identity_key: Some(event_id),
            title: headline,
            summary: Some(summary),
            body: Some(summary),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("proposed"),
            verification_state: Some("proposed"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("operator_write"),
            observed_at_epoch_ms: Some(recorded_at_epoch_ms),
            recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
            valid_from_epoch_ms: Some(recorded_at_epoch_ms),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: None,
            object_version: Some(1),
            causation_id: Some(event_id),
            correlation_id: thread_id,
            utility_score: Some(1.0),
            freshness_score: Some(1.0),
            retention_class: Some("durable"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: Some(&imported_from),
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &memory_item_metadata,
        },
    )
    .await?;
    let task_node_status_payload = json!({
        "source_kind": "continuity_handoff",
        "source_event_id": event_id,
        "source_snapshot_id": snapshot_id,
        "pending_return_queue": pending_return_queue,
    });
    let task_node_source_event_ids = json!([event_id]);
    let task_node_artifact_refs = json!([format!("file://{local_path}")]);
    let task_node_evidence_span = json!({
        "event_id": event_id,
        "snapshot_id": snapshot_id,
        "thread_id": thread_id,
    });
    let task_node_metadata = json!({
        "agent_scope": agent_scope,
        "session_id": session_id,
        "thread_id": thread_id,
        "local_path": local_path,
        "materialized_from": "execctl_task_ledger_entry"
    });
    let task_node = postgres::create_task_node(
        db,
        &project.code,
        &namespace.code,
        &postgres::TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: Some(memory_item.memory_item_id),
            task_key: Some(event_id),
            task_role: Some("historical"),
            headline,
            summary: Some(summary),
            next_step: Some(next_step),
            execution_state: Some(execution_state),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(pending_return_count),
            source_event_ids: Some(&task_node_source_event_ids),
            artifact_refs: Some(&task_node_artifact_refs),
            evidence_span: Some(&task_node_evidence_span),
            derivation_kind: Some("extract"),
            status_payload: &task_node_status_payload,
            metadata: &task_node_metadata,
            opened_at_epoch_ms: Some(recorded_at_epoch_ms),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await?;
    let task_event_payload = json!({
        "source_kind": "continuity_handoff",
        "summary": summary,
        "next_step": next_step,
        "pending_return_queue": pending_return_queue,
    });
    let task_event_artifact_refs = json!([format!("file://{local_path}")]);
    let task_event_message_refs = thread_id
        .map(|thread| json!([format!("thread:{thread}")]))
        .unwrap_or_else(|| json!([]));
    let task_event_evidence_span = json!({
        "event_id": event_id,
        "snapshot_id": snapshot_id,
        "thread_id": thread_id,
        "kind": "continuity_handoff",
    });
    let _ = postgres::create_task_event(
        db,
        &project.code,
        &namespace.code,
        &postgres::TaskEventInsert {
            task_node_id: task_node.task_node_id,
            source_snapshot_id: Some(snapshot_id),
            source_event_id: Some(event_id),
            event_kind: "created",
            prior_execution_state: None,
            next_execution_state: Some(execution_state),
            prior_lifecycle_state: None,
            next_lifecycle_state: Some("hot"),
            source_kind: Some("continuity_handoff"),
            artifact_refs: Some(&task_event_artifact_refs),
            message_refs: Some(&task_event_message_refs),
            evidence_span: Some(&task_event_evidence_span),
            derivation_kind: Some("raw_capture"),
            schema_version: Some("task-event-envelope-v1"),
            event_payload: &task_event_payload,
            recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
        },
    )
    .await?;
    Ok(())
}
pub(crate) const HOST_CURRENT_THREAD_COMPACT_WINDOW_HOST_SURFACE_KIND: &str =
    "codex_webview_compact_window_route";
pub(crate) const HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID: &str = "hotkey-window-open-current";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_EXTERNAL_LAUNCH_KIND: &str =
    "vscode_extension_uri_route";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_URI_SCHEME: &str = "vscode";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_URI_AUTHORITY: &str = "openai.chatgpt";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_ROUTE_PREFIX: &str = "/thread-overlay";
pub(crate) const HOST_CURRENT_THREAD_COMPACT_WINDOW_ROUTE_PREFIX: &str = "/hotkey-window/thread";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_OBSERVE_API_LAUNCH_PATH: &str =
    "/api/client-budget-host-control-launch";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND: &str =
    "host_current_thread_control_feedback";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED: &str = "requested";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED: &str = "opened";
pub(crate) const HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED: &str = "failed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientBudgetBlockingReplyMode {
    Inactive,
    #[allow(dead_code)]
    RotateChatOnly,
    // Kept for contract/test compatibility even though runtime reply blocking is removed.
    #[allow(dead_code)]
    WaitForGlobalBudgetRecovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientReplyBudgetMode {
    Normal,
    CompactHighSignal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostContextCompactionStage {
    Inactive,
    Preserve,
    CriticalRegrowth,
}

impl HostContextCompactionStage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Preserve => "preserve",
            Self::CriticalRegrowth => "critical_regrowth",
        }
    }

    pub(crate) fn preserve_active(self) -> bool {
        !matches!(self, Self::Inactive)
    }

    pub(crate) fn critical_regrowth_active(self) -> bool {
        matches!(self, Self::CriticalRegrowth)
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn shell_join_command(args: &[&str]) -> String {
    args.iter()
        .map(|value| shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ")
}

fn current_workspace_repo_root_string() -> Option<String> {
    discover_repo_root(None).ok().and_then(|path| {
        path.canonicalize()
            .ok()
            .map(|resolved| resolved.to_string_lossy().to_string())
    })
}

fn can_use_workspace_continuity_defaults(
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
) -> bool {
    let Some(repo_root) = repo_root.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    let Some(current_workspace_repo_root) = current_workspace_repo_root_string() else {
        return false;
    };
    current_workspace_repo_root == repo_root
        && namespace_code
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("continuity")
            == "continuity"
}

fn build_workspace_aware_rotate_helper_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
) -> Option<String> {
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        return Some(shell_join_command(&["amai", "continuity", "rotate-chat"]));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code.filter(|value| !value.is_empty())?;
    let repo_root = repo_root.filter(|value| !value.is_empty())?;
    Some(shell_join_command(&[
        "amai",
        "continuity",
        "rotate-chat",
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--repo-root",
        repo_root,
    ]))
}

fn build_workspace_aware_startup_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    token_source_kind: &str,
    runtime_state_json: bool,
) -> Option<String> {
    let namespace_code = namespace_code.filter(|value| !value.is_empty());
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        let mut args = vec!["amai", "continuity", "startup"];
        if runtime_state_json {
            args.push("--runtime-state-json");
        }
        if !token_source_kind.trim().is_empty()
            && token_source_kind != "operator_continuity_startup"
        {
            args.push("--token-source-kind");
            args.push(token_source_kind);
        }
        return Some(shell_join_command(&args));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code?;
    let repo_root = repo_root.filter(|value| !value.is_empty())?;
    let mut args = vec![
        "amai",
        "continuity",
        "startup",
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--repo-root",
        repo_root,
    ];
    if !token_source_kind.trim().is_empty() {
        args.push("--token-source-kind");
        args.push(token_source_kind);
    }
    if runtime_state_json {
        args.push("--runtime-state-json");
    }
    Some(shell_join_command(&args))
}

fn build_workspace_aware_handoff_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    headline: Option<&str>,
    next_step: Option<&str>,
) -> Option<String> {
    let headline = headline.filter(|value| !value.is_empty())?;
    let next_step = next_step.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code.filter(|value| !value.is_empty());
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        return Some(shell_join_command(&[
            "./scripts/continuity_handoff.sh",
            "--project",
            "amai",
            "--namespace",
            "continuity",
            "--headline",
            headline,
            "--next-step",
            next_step,
        ]));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code?;
    Some(shell_join_command(&[
        "amai",
        "continuity",
        "handoff",
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--headline",
        headline,
        "--next-step",
        next_step,
    ]))
}

pub(crate) fn normalize_host_current_thread_control_command_id(
    value: Option<&str>,
) -> &'static str {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID) => {
            HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
        }
        _ => HOST_CURRENT_THREAD_CONTROL_COMMAND_ID,
    }
}

fn host_current_thread_control_kind_for_command_id(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => HOST_CURRENT_THREAD_COMPACT_WINDOW_KIND,
        _ => HOST_CURRENT_THREAD_CONTROL_KIND,
    }
}

fn host_current_thread_control_host_surface_kind_for_command_id(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            HOST_CURRENT_THREAD_COMPACT_WINDOW_HOST_SURFACE_KIND
        }
        _ => HOST_CURRENT_THREAD_CONTROL_HOST_SURFACE_KIND,
    }
}

fn host_current_thread_control_route_prefix_for_command_id(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            HOST_CURRENT_THREAD_COMPACT_WINDOW_ROUTE_PREFIX
        }
        _ => HOST_CURRENT_THREAD_CONTROL_ROUTE_PREFIX,
    }
}

fn host_current_thread_control_button_label(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => "Open compact window",
        _ => "Open thread overlay",
    }
}

fn host_current_thread_control_summary(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Открыть current thread на compact-window route внутри Codex host-клиента."
        }
        _ => "Открыть overlay текущего thread внутри Codex host-клиента.",
    }
}

fn host_current_thread_control_note(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Это same-thread compact-window surface внутри Codex host-клиента. В electron этот route относится к popout/compact-window contour; в VS Code onUri path может открыть тот же compact-window renderer route внутри клиента, а не гарантированный отдельный window. Surface полезен для проверки, режет ли compact-window host overhead лучше overlay, но он всё ещё не равен clean-surface rebase."
        }
        _ => {
            "Это current-thread control surface внутри Codex host-клиента. Он полезен для same-thread operator control, но не равен clean-surface rebase. Public VS Code command palette path пока не доказан, однако при наличии thread_id materialized URI launch и server-side xdg-open path через Amai observe host."
        }
    }
}

fn host_current_thread_control_intro_message(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Открыть same-thread compact-window route текущего giant thread и проверить, снижает ли он host-side overhead лучше overlay."
        }
        _ => "Открыть same-thread overlay текущего giant thread через host surface.",
    }
}

fn host_current_thread_control_requested_message(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Запрошено открытие same-thread compact window текущего giant thread."
        }
        _ => "Запрошено открытие same-thread overlay текущего giant thread.",
    }
}

fn host_current_thread_control_feedback_ack_intro(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "После попытки запуска подтверди, открылся ли compact window. Это попадёт в Amai continuity."
        }
        _ => "После попытки запуска подтверди, открылся ли overlay. Это попадёт в Amai continuity.",
    }
}

fn host_current_thread_control_external_summary(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Попробовать открыть current thread на compact-window route через VS Code URI handler."
        }
        _ => "Попробовать открыть current thread overlay через VS Code URI handler.",
    }
}

fn host_current_thread_control_external_note(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Этот path опирается на openai.chatgpt onUri -> navigateToRoute(path) и webview route /hotkey-window/thread/:conversationId. В electron это compact-window/popout contour; в VS Code расширении route может открыться как compact-window renderer category без гарантии отдельного окна. Локальный Amai observe host умеет запускать xdg-open для этого URI, но end-to-end эффект всё ещё подтверждается отдельным feedback."
        }
        _ => {
            "Этот path опирается на openai.chatgpt onUri -> navigateToRoute(path) и webview route /thread-overlay/:conversationId. Он truthfully best-effort: route и handler materialized; локальный Amai observe host теперь умеет запускать xdg-open для этого URI, но end-to-end open всё ещё подтверждается отдельным feedback."
        }
    }
}

fn host_current_thread_control_observe_api_launch_summary(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Запустить current thread compact-window route через локальный Amai observe host."
        }
        _ => "Запустить current thread overlay через локальный Amai observe host.",
    }
}

fn host_current_thread_control_subject(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => "same-thread compact window",
        _ => "same-thread overlay",
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_host_current_thread_control_surface() -> Value {
    build_host_current_thread_control_surface_for_stage(HostContextCompactionStage::Inactive)
}

fn preferred_host_current_thread_control_command_id_for_stage(
    stage: HostContextCompactionStage,
) -> &'static str {
    if stage.preserve_active() {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
    } else {
        HOST_CURRENT_THREAD_CONTROL_COMMAND_ID
    }
}

fn alternate_host_current_thread_control_command_id_for_stage(
    stage: HostContextCompactionStage,
) -> &'static str {
    if preferred_host_current_thread_control_command_id_for_stage(stage)
        == HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
    {
        HOST_CURRENT_THREAD_CONTROL_COMMAND_ID
    } else {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
    }
}

fn host_current_thread_control_selection_reason(
    stage: HostContextCompactionStage,
    primary_command_id: &str,
) -> &'static str {
    if stage.preserve_active() {
        if normalize_host_current_thread_control_command_id(Some(primary_command_id))
            == HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
        {
            "protect_recent_host_compaction_gain"
        } else if stage.critical_regrowth_active() {
            "critical_regrowth_try_overlay"
        } else {
            "compact_window_failed_try_overlay"
        }
    } else {
        "default_overlay_first"
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_host_current_thread_control_surface_for_stage(
    stage: HostContextCompactionStage,
) -> Value {
    let current_thread = current_thread_id();
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        current_thread.as_deref(),
        stage,
        None,
    )
}

#[allow(dead_code)]
pub(crate) fn build_host_current_thread_control_surface_for_stage_and_primary_command(
    stage: HostContextCompactionStage,
    primary_command_id: Option<&str>,
) -> Value {
    let current_thread = current_thread_id();
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        current_thread.as_deref(),
        stage,
        primary_command_id,
    )
}

fn build_host_current_thread_control_route_path_for_command(
    thread_id: &str,
    command_id: &str,
) -> String {
    format!(
        "{}/{}",
        host_current_thread_control_route_prefix_for_command_id(command_id),
        thread_id
    )
}

fn build_host_current_thread_control_uri(route_path: &str) -> String {
    format!(
        "{HOST_CURRENT_THREAD_CONTROL_URI_SCHEME}://{HOST_CURRENT_THREAD_CONTROL_URI_AUTHORITY}{route_path}"
    )
}

fn build_host_current_thread_control_launch_command(uri: &str) -> Option<String> {
    if cfg!(target_os = "linux") {
        Some(shell_join_command(&["xdg-open", uri]))
    } else {
        None
    }
}

fn build_host_current_thread_control_observe_launch_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    surface: &Value,
) -> Option<String> {
    let thread_id = surface["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let command_id = surface["command_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut base_args = vec!["amai", "observe", "ctl-launch", "--thread-id", thread_id];
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            base_args.push("--compact-window");
        }
        HOST_CURRENT_THREAD_CONTROL_COMMAND_ID => {}
        _ => {
            base_args.push("--command-id");
            base_args.push(command_id);
        }
    }
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        return Some(shell_join_command(&base_args));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code.filter(|value| !value.is_empty())?;
    let repo_root = repo_root.filter(|value| !value.is_empty())?;
    base_args.extend_from_slice(&[
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--repo-root",
        repo_root,
    ]);
    Some(shell_join_command(&base_args))
}

pub(crate) fn build_host_current_thread_control_surface_for_thread(
    thread_id: Option<&str>,
) -> Value {
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        thread_id,
        HostContextCompactionStage::Inactive,
        None,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_host_current_thread_control_surface_for_thread_and_stage(
    thread_id: Option<&str>,
    stage: HostContextCompactionStage,
) -> Value {
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        thread_id, stage, None,
    )
}

pub(crate) fn build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
    thread_id: Option<&str>,
    stage: HostContextCompactionStage,
    primary_command_id: Option<&str>,
) -> Value {
    let primary_command_id = primary_command_id
        .map(|value| normalize_host_current_thread_control_command_id(Some(value)))
        .unwrap_or_else(|| preferred_host_current_thread_control_command_id_for_stage(stage));
    let alternate_command_id = alternate_host_current_thread_control_command_id_for_stage(stage);
    let mut primary = build_host_current_thread_control_surface_for_thread_and_command(
        thread_id,
        Some(primary_command_id),
    );
    if let Some(root) = primary.as_object_mut() {
        root.insert(
            "host_context_compaction_stage".to_string(),
            json!(stage.as_str()),
        );
        root.insert(
            "selection_reason".to_string(),
            json!(host_current_thread_control_selection_reason(
                stage,
                primary_command_id,
            )),
        );
        root.insert(
            "alternate_controls".to_string(),
            Value::Array(vec![
                build_host_current_thread_control_surface_for_thread_and_command(
                    thread_id,
                    Some(if alternate_command_id == primary_command_id {
                        alternate_host_current_thread_control_command_id_for_stage(
                            HostContextCompactionStage::Inactive,
                        )
                    } else {
                        alternate_command_id
                    }),
                ),
            ]),
        );
    }
    primary
}

pub(crate) fn build_host_current_thread_control_surface_for_thread_and_command(
    thread_id: Option<&str>,
    command_id: Option<&str>,
) -> Value {
    let command_id = normalize_host_current_thread_control_command_id(command_id);
    let thread_id = thread_id.map(str::trim).filter(|value| !value.is_empty());
    let route_path = thread_id.map(|thread_id| {
        build_host_current_thread_control_route_path_for_command(thread_id, command_id)
    });
    let uri = route_path
        .as_deref()
        .map(build_host_current_thread_control_uri);
    let launch_command = uri
        .as_deref()
        .and_then(build_host_current_thread_control_launch_command);
    let observe_api_launch_available = launch_command.is_some();
    json!({
        "available": true,
        "control_kind": host_current_thread_control_kind_for_command_id(command_id),
        "host_surface_kind": host_current_thread_control_host_surface_kind_for_command_id(command_id),
        "command_id": command_id,
        "button_label": host_current_thread_control_button_label(command_id),
        "intro_message": host_current_thread_control_intro_message(command_id),
        "requested_message_text": host_current_thread_control_requested_message(command_id),
        "feedback_ack_intro": host_current_thread_control_feedback_ack_intro(command_id),
        "thread_id": thread_id,
        "same_thread_surface": true,
        "automation_ready": observe_api_launch_available,
        "requires_host_bridge": true,
        "snapshot_seeded_before_open": true,
        "resume_if_needed_before_snapshot": true,
        "external_uri_launch": {
            "available": uri.is_some(),
            "launch_surface_kind": HOST_CURRENT_THREAD_CONTROL_EXTERNAL_LAUNCH_KIND,
            "best_effort": true,
            "observe_api_launch_available": observe_api_launch_available,
            "observe_api_launch_path": if observe_api_launch_available {
                Some(HOST_CURRENT_THREAD_CONTROL_OBSERVE_API_LAUNCH_PATH)
            } else {
                None::<&str>
            },
            "observe_api_launch_summary": if observe_api_launch_available {
                Some(host_current_thread_control_observe_api_launch_summary(command_id))
            } else {
                None::<&str>
            },
            "verification_state": if uri.is_some() && observe_api_launch_available {
                "route_resolved_launch_command_available"
            } else if uri.is_some() {
                "route_resolved_not_executed"
            } else {
                "missing_thread_id"
            },
            "uri_scheme": HOST_CURRENT_THREAD_CONTROL_URI_SCHEME,
            "uri_authority": HOST_CURRENT_THREAD_CONTROL_URI_AUTHORITY,
            "route_path": route_path,
            "uri": uri,
            "platform_launch_command": launch_command,
            "summary": host_current_thread_control_external_summary(command_id),
            "note": host_current_thread_control_external_note(command_id)
        },
        "summary": host_current_thread_control_summary(command_id),
        "note": host_current_thread_control_note(command_id)
    })
}

pub(crate) fn compact_host_current_thread_control_surface_for_runtime(surface: &Value) -> Value {
    let Some(node) = surface.as_object() else {
        return Value::Null;
    };
    let surface_exhausted_after_verified_failure = node
        .get("surface_exhausted_after_verified_failure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut compact = serde_json::Map::new();
    for field in [
        "available",
        "automation_ready",
        "button_label",
        "command_id",
        "control_kind",
        "thread_id",
        "host_context_compaction_stage",
        "feedback_pending",
        "measurement_pending",
        "retry_allowed",
        "retry_blocked_reason",
        "effect_verdict",
        "selection_reason",
        "availability_state",
        "surface_exhausted_after_verified_failure",
    ] {
        if let Some(value) = node.get(field).filter(|value| !value.is_null()) {
            compact.insert(field.to_string(), value.clone());
        }
    }
    if !surface_exhausted_after_verified_failure {
        if let Some(value) = node.get("effect_summary").filter(|value| !value.is_null()) {
            compact.insert("effect_summary".to_string(), value.clone());
        }
        if let Some(alternates) = node.get("alternate_controls").and_then(Value::as_array) {
            let alternate_controls = alternates
                .iter()
                .map(|alternate| {
                    json!({
                        "button_label": alternate["button_label"].clone(),
                        "command_id": alternate["command_id"].clone(),
                        "control_kind": alternate["control_kind"].clone(),
                    })
                })
                .collect::<Vec<_>>();
            compact.insert(
                "alternate_controls".to_string(),
                Value::Array(alternate_controls),
            );
        }
    }
    Value::Object(compact)
}

pub(crate) fn normalize_host_current_thread_control_feedback_kind(
    value: &str,
) -> Option<&'static str> {
    match value.trim() {
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED => {
            Some(HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED)
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED => {
            Some(HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED)
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED => {
            Some(HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED)
        }
        _ => None,
    }
}

pub(crate) fn host_current_thread_control_feedback_notice_text_for_command(
    feedback_kind: &str,
    command_id: Option<&str>,
) -> String {
    let subject = host_current_thread_control_subject(
        normalize_host_current_thread_control_command_id(command_id),
    );
    match feedback_kind {
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED => {
            format!("Попытка открыть {subject} зафиксирована. После запуска отметь результат.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED => {
            format!("Подтверждено: {subject} открылся.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED => {
            format!("Зафиксировано: {subject} не открылся.")
        }
        _ => format!("Feedback по {subject} зафиксирован."),
    }
}

fn host_current_thread_control_feedback_summary(
    feedback_kind: &str,
    command_id: Option<&str>,
) -> String {
    let subject = host_current_thread_control_subject(
        normalize_host_current_thread_control_command_id(command_id),
    );
    match feedback_kind {
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED => {
            format!("Requested {subject} launch via host current-thread control.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED => {
            format!("Operator confirmed {subject} opened.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED => {
            format!("Operator reported {subject} did not open.")
        }
        _ => "Recorded host current-thread control feedback.".to_string(),
    }
}

fn build_host_current_thread_control_feedback_snapshot_for_thread(
    thread_id: Option<&str>,
) -> Value {
    let thread_id = thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    if thread_id.is_empty() {
        return Value::Null;
    }
    let client_live_meter =
        codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)
            .ok()
            .flatten();
    let host_context_compaction =
        codex_threads::latest_rollout_context_compaction_observation_for_thread(thread_id)
            .ok()
            .flatten();
    let host_context_compaction = if let Some(observation) = host_context_compaction {
        let current_turn_total_tokens = client_live_meter
            .as_ref()
            .map(|value| value.client_turn_total_tokens)
            .unwrap_or_default();
        let growth_since_compaction_tokens =
            current_turn_total_tokens.saturating_sub(observation.post_compaction_turn_total_tokens);
        let recovered_surface_tokens = observation
            .pre_compaction_turn_total_tokens
            .saturating_sub(observation.post_compaction_turn_total_tokens);
        let regrowth_of_recovered_surface_ratio = if recovered_surface_tokens > 0 {
            growth_since_compaction_tokens as f64 / recovered_surface_tokens as f64
        } else {
            0.0
        };
        json!({
            "compacted_at_epoch_ms": observation.compacted_at_epoch_ms,
            "pre_compaction_turn_total_tokens": observation.pre_compaction_turn_total_tokens,
            "post_compaction_turn_total_tokens": observation.post_compaction_turn_total_tokens,
            "post_compaction_turn_id": observation.post_compaction_turn_id,
            "compaction_count": observation.compaction_count,
            "growth_since_compaction_tokens": growth_since_compaction_tokens,
            "recovered_surface_tokens": recovered_surface_tokens,
            "regrowth_of_recovered_surface_ratio": regrowth_of_recovered_surface_ratio,
            "observation_source": observation.observation_source,
        })
    } else {
        Value::Null
    };
    let client_live_meter = if let Some(observation) = client_live_meter {
        let context_used_percent = if observation.latest_model_context_window == 0 {
            None
        } else {
            Some(
                observation.client_turn_total_tokens as f64 * 100.0
                    / observation.latest_model_context_window as f64,
            )
        };
        json!({
            "thread_id": observation.thread_id,
            "turn_id": observation.turn_id,
            "started_at_epoch_ms": observation.started_at_epoch_ms,
            "ended_at_epoch_ms": observation.ended_at_epoch_ms,
            "client_turn_total_tokens": observation.client_turn_total_tokens,
            "latest_model_context_window": observation.latest_model_context_window,
            "context_used_percent": context_used_percent,
            "primary_limit_used_percent": observation.latest_primary_limit_used_percent,
            "primary_limit_remaining_percent":
                100_u64.saturating_sub(observation.latest_primary_limit_used_percent),
            "secondary_limit_used_percent": observation.latest_secondary_limit_used_percent,
            "secondary_limit_remaining_percent":
                100_u64.saturating_sub(observation.latest_secondary_limit_used_percent),
            "observation_source": observation.observation_source,
        })
    } else {
        Value::Null
    };
    json!({
        "snapshot_version": "host-current-thread-control-effect-snapshot-v1",
        "thread_id": thread_id,
        "client_live_meter": client_live_meter,
        "host_context_compaction": host_context_compaction,
    })
}

pub(crate) fn build_client_budget_blocking_reply_contract(
    mode: ClientBudgetBlockingReplyMode,
) -> Value {
    let (active, response_kind, template) = match mode {
        ClientBudgetBlockingReplyMode::Inactive => {
            (false, CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND, None)
        }
        ClientBudgetBlockingReplyMode::RotateChatOnly => (
            true,
            CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND,
            Some(CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE),
        ),
        ClientBudgetBlockingReplyMode::WaitForGlobalBudgetRecovery => (
            true,
            CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND,
            Some(CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE),
        ),
    };
    json!({
        "contract_version": CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION,
        "active": active,
        "response_kind": response_kind,
        "max_sentences": CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES,
        "must_avoid_substantive_work": true,
        "must_use_action_bundle_operator_flow": true,
        "template": template,
    })
}

pub(crate) fn default_client_budget_target_percent() -> u64 {
    DEFAULT_CLIENT_BUDGET_TARGET_PERCENT
}

pub(crate) fn normalize_client_budget_target_percent(value: u64) -> Option<u64> {
    if value <= MAX_CLIENT_BUDGET_TARGET_PERCENT && value % CLIENT_BUDGET_TARGET_STEP_PERCENT == 0 {
        Some(value)
    } else {
        None
    }
}

pub(crate) fn client_budget_target_percent_from_restore_context(restore_context: &Value) -> u64 {
    restore_context["client_budget_target_percent"]
        .as_u64()
        .and_then(normalize_client_budget_target_percent)
        .unwrap_or(DEFAULT_CLIENT_BUDGET_TARGET_PERCENT)
}

fn compact_reply_budget_summary(target_percent: u64) -> String {
    if target_percent == 0 {
        "Target economy is set to 0%, so compact mode only activates from rotate-pressure, overspend, or other hard client-budget signals. Ответ остаётся содержательным, но должен быть жёстко compact: один абзац или максимум два bullets, сначала прямой результат, затем только изменившиеся факты без повторов.".to_string()
    } else {
        format!(
            "Exact 5ч KPI ниже целевой планки {target_percent}% или rotate-pressure уже materialized. Ответ остаётся содержательным, но должен быть жёстко compact: один абзац или максимум два bullets, сначала прямой результат, затем только изменившиеся факты без повторов."
        )
    }
}

pub(crate) fn build_client_reply_budget_contract_with_target(
    mode: ClientReplyBudgetMode,
    target_percent: u64,
    host_context_compaction_stage: HostContextCompactionStage,
    target_pressure_active: bool,
    current_live_turn_no_amai_activity: bool,
) -> Value {
    let host_context_compaction_preserve_active = host_context_compaction_stage.preserve_active();
    let host_context_compaction_critical_regrowth_active =
        host_context_compaction_stage.critical_regrowth_active();
    let inactive_target_pressure_active = target_pressure_active
        && !host_context_compaction_preserve_active
        && !host_context_compaction_critical_regrowth_active;
    let preserve_stage_strict_active =
        host_context_compaction_preserve_active && target_pressure_active;
    let critical_regrowth_strict_active =
        host_context_compaction_critical_regrowth_active && target_pressure_active;
    let pure_burn_turn_active = target_pressure_active && current_live_turn_no_amai_activity;
    let (
        active,
        mode_label,
        max_paragraphs_soft,
        max_bullets_soft,
        max_sentences_soft,
        max_tool_roundtrips_soft,
        summary,
    ) = {
        let preserve_summary = "Host уже compacted этот thread. Защити новую компактную поверхность: минимум промежуточных апдейтов, никаких широких разведочных проходов без прямого запроса, один плотный batched read вместо серии мелких exploratory tool turns.";
        let inactive_target_pressure_summary = "Даже без host compaction exact 5ч KPI уже ниже целевой планки, поэтому режь расход заранее: короткий ответ без commentary-only апдейтов и без нового tool turn, пока не появится точная material delta-goal.";
        let preserve_target_pressure_summary = "Целевая планка уже не держится даже в preserve-stage, поэтому экономию нужно защищать сразу: не отправляй commentary-only апдейты, не дроби tool-чтение на серию мелких запросов и жди meaningful result перед следующим progress reply.";
        let critical_regrowth_summary = "После host compaction thread уже снова отъел заметную долю восстановленной поверхности. С этого момента каждый лишний tool turn дорог: не отправляй commentary-only апдейты, не дроби чтение на мелкие запросы, отвечай только после meaningful patch/result delta и не гоняй повторные live-diagnostic reread/retry loops без новой дельты.";
        let pure_burn_turn_summary = "Текущий live-turn уже показывает no_amai_activity_in_current_live_turn, значит этот turn пока только сжигает окно клиента. Не отправляй новый progress reply без material patch/result/decision delta и не начинай новый tool turn без точной гипотезы, что именно изменится.";
        match mode {
            ClientReplyBudgetMode::Normal => (
                false,
                CLIENT_REPLY_BUDGET_MODE_NORMAL,
                None,
                None,
                None,
                None,
                "Обычный режим ответа без дополнительного client-budget сжатия.".to_string(),
            ),
            ClientReplyBudgetMode::CompactHighSignal => {
                let max_bullets_soft = if pure_burn_turn_active
                    || host_context_compaction_critical_regrowth_active
                    || preserve_stage_strict_active
                {
                    Some(0)
                } else if inactive_target_pressure_active {
                    Some(1)
                } else if host_context_compaction_preserve_active {
                    Some(1)
                } else {
                    Some(2)
                };
                let max_sentences_soft = if pure_burn_turn_active
                    || host_context_compaction_critical_regrowth_active
                    || preserve_stage_strict_active
                {
                    Some(1)
                } else if inactive_target_pressure_active {
                    Some(2)
                } else if host_context_compaction_preserve_active {
                    Some(2)
                } else {
                    Some(3)
                };
                let mut summary = compact_reply_budget_summary(target_percent);
                if host_context_compaction_critical_regrowth_active {
                    summary.push(' ');
                    summary.push_str(preserve_summary);
                    summary.push(' ');
                    summary.push_str(critical_regrowth_summary);
                    if pure_burn_turn_active {
                        summary.push(' ');
                        summary.push_str(pure_burn_turn_summary);
                    }
                } else if preserve_stage_strict_active {
                    summary.push(' ');
                    summary.push_str(preserve_summary);
                    summary.push(' ');
                    summary.push_str(preserve_target_pressure_summary);
                    if pure_burn_turn_active {
                        summary.push(' ');
                        summary.push_str(pure_burn_turn_summary);
                    }
                } else if inactive_target_pressure_active {
                    summary.push(' ');
                    summary.push_str(inactive_target_pressure_summary);
                    if pure_burn_turn_active {
                        summary.push(' ');
                        summary.push_str(pure_burn_turn_summary);
                    }
                } else if host_context_compaction_preserve_active {
                    summary.push(' ');
                    summary.push_str(preserve_summary);
                }
                (
                    true,
                    CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL,
                    Some(1),
                    max_bullets_soft,
                    max_sentences_soft,
                    Some(
                        if pure_burn_turn_active
                            || critical_regrowth_strict_active
                            || inactive_target_pressure_active
                        {
                            0
                        } else if host_context_compaction_critical_regrowth_active
                            || preserve_stage_strict_active
                        {
                            1
                        } else if host_context_compaction_preserve_active {
                            2
                        } else {
                            3
                        },
                    ),
                    summary,
                )
            }
        }
    };
    json!({
        "contract_version": CLIENT_REPLY_BUDGET_CONTRACT_VERSION,
        "active": active,
        "mode": mode_label,
        "must_preserve_truthfulness": true,
        "must_preserve_technical_accuracy": true,
        "must_disclose_unknowns_instead_of_guessing": true,
        "must_answer_directly_first": active,
        "must_avoid_unrequested_recaps": active,
        "must_avoid_repeating_known_context": active,
        "must_keep_only_changed_facts_when_possible": active,
        "must_prefer_patch_or_result_over_narration_when_coding": active,
        "must_prefer_short_paragraphs": active,
        "host_context_compaction_stage":
            if active { Some(host_context_compaction_stage.as_str()) } else { None },
        "host_context_compaction_target_pressure_active":
            active && target_pressure_active,
        "host_context_compaction_inactive_target_pressure_active":
            active && inactive_target_pressure_active,
        "current_live_turn_no_amai_activity":
            active && current_live_turn_no_amai_activity,
        "same_meter_pure_burn_turn_active":
            active && pure_burn_turn_active,
        "host_context_compaction_preserve_active":
            active && host_context_compaction_preserve_active,
        "host_context_compaction_preserve_strict_active":
            active && preserve_stage_strict_active,
        "host_context_compaction_critical_regrowth_active":
            active && host_context_compaction_critical_regrowth_active,
        "must_protect_recent_host_compaction_gain":
            active && host_context_compaction_preserve_active,
        "must_minimize_nonessential_progress_updates":
            active && host_context_compaction_preserve_active,
        "must_avoid_broad_exploration_without_user_request":
            active && host_context_compaction_preserve_active,
        "must_prefer_single_batched_tool_read_when_exploring":
            active && host_context_compaction_preserve_active,
        "must_avoid_commentary_only_updates":
            active && (
                host_context_compaction_critical_regrowth_active
                    || preserve_stage_strict_active
                    || inactive_target_pressure_active
            ),
        "must_batch_all_tool_reads_before_reply":
            active && (
                host_context_compaction_critical_regrowth_active
                    || preserve_stage_strict_active
                    || inactive_target_pressure_active
            ),
        "must_wait_for_meaningful_result_before_progress_reply":
            active && (
                host_context_compaction_critical_regrowth_active
                    || preserve_stage_strict_active
                    || inactive_target_pressure_active
            ),
        "must_reuse_latest_live_diagnostics_before_reread":
            active && host_context_compaction_critical_regrowth_active,
        "must_avoid_repeated_live_guard_polls_without_new_delta":
            active && host_context_compaction_critical_regrowth_active,
        "must_avoid_serial_same_thread_host_control_retries_without_effect_delta":
            active && host_context_compaction_critical_regrowth_active,
        "must_prefer_single_same_thread_control_then_measure":
            active && host_context_compaction_critical_regrowth_active,
        "must_require_material_delta_before_next_reply":
            active && (
                pure_burn_turn_active
                    || critical_regrowth_strict_active
                    || inactive_target_pressure_active
            ),
        "must_avoid_progress_reply_when_only_guard_changed":
            active && (
                pure_burn_turn_active
                    || critical_regrowth_strict_active
                    || inactive_target_pressure_active
            ),
        "must_avoid_new_tool_turn_without_specific_delta_goal":
            active && (
                pure_burn_turn_active
                    || critical_regrowth_strict_active
                    || inactive_target_pressure_active
            ),
        "max_paragraphs_soft": max_paragraphs_soft,
        "max_bullets_soft": max_bullets_soft,
        "max_sentences_soft": max_sentences_soft,
        "max_tool_roundtrips_soft": max_tool_roundtrips_soft,
        "summary": summary,
    })
}

pub(crate) fn build_global_client_limit_source_contract() -> Value {
    json!({
        "source_kind": GLOBAL_CLIENT_LIMIT_SOURCE_KIND,
        "derived_from_latest_observed_client_limits": true,
        "truly_global_source_materialized": false,
        "authoritative_for": [
            "global_client_limit_hint",
            "wait_for_global_client_budget_recovery_when_critical"
        ],
        "not_authoritative_for": [
            "thread_local_rotate_pressure",
            "live_turn_rows"
        ],
        "summary": GLOBAL_CLIENT_LIMIT_SOURCE_SUMMARY,
    })
}

#[allow(dead_code)]
pub(crate) fn client_budget_guard_requires_rotate_before_reply(guard: &Value) -> bool {
    guard["reply_execution_gate"]["must_rotate_before_reply"].as_bool() == Some(true)
}

pub(crate) fn client_budget_guard_blocks_reply(guard: &Value) -> bool {
    let _ = guard;
    // User-visible reply blocking is removed from the client-budget contour.
    false
}

#[cfg(test)]
pub(crate) fn client_budget_guard_blocks_expensive_tool_turn(guard: &Value) -> bool {
    let _ = guard;
    // Tool-turn blocking is removed from the client-budget contour. Pressure
    // signals such as same_meter_pure_burn_turn_active or max_tool_roundtrips_soft=0
    // remain advisory-only and must not hard-block tools.
    false
}

pub async fn record_handoff_event(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    headline: &str,
    next_step: &str,
    details: &str,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
    local_path: &str,
) -> Result<()> {
    record_handoff_event_with_refresh(
        db,
        project,
        namespace,
        headline,
        next_step,
        details,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
        local_path,
        None,
        true,
    )
    .await
}

pub async fn record_handoff_event_with_previous_restore(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    headline: &str,
    next_step: &str,
    details: &str,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
    local_path: &str,
    previous_restore: Option<&Value>,
) -> Result<()> {
    record_handoff_event_with_refresh(
        db,
        project,
        namespace,
        headline,
        next_step,
        details,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
        local_path,
        previous_restore,
        true,
    )
    .await
}

async fn record_handoff_event_with_refresh(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    headline: &str,
    next_step: &str,
    details: &str,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
    local_path: &str,
    previous_restore: Option<&Value>,
    refresh_restore_snapshot_after_write: bool,
) -> Result<()> {
    let total_started = Instant::now();
    let recorded_at_epoch_ms = now_epoch_ms()?;
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let next_step = normalize_next_step_hint(next_step);
    let previous_restore_started = Instant::now();
    let previous_restore = match previous_restore {
        Some(restore) => Some(restore.clone()),
        None => load_recent_restore_bundle_without_live_guard(db, project, namespace).await?,
    };
    continuity_profile_log(
        "record_handoff.previous_restore",
        previous_restore_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let client_budget_target_percent = previous_restore.as_ref().and_then(|value| {
        value["working_state_restore"]["client_budget_target_percent"]
            .as_u64()
            .and_then(normalize_client_budget_target_percent)
    });
    let pending_return_queue = derive_pending_return_queue(
        previous_restore
            .as_ref()
            .map(|value| &value["working_state_restore"]),
        headline,
        &next_step,
        recorded_at_epoch_ms,
        resolve_current_goal,
        resolved_headlines,
        resolved_task_ids,
    );
    let thread_id = resolve_thread_id_for_project_repo_root(&project.repo_root, None);
    let session_id_started = Instant::now();
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    continuity_profile_log(
        "record_handoff.resolve_session_id",
        session_id_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let event_id = Uuid::new_v4().to_string();
    let active_files = extract_paths_from_text(details);
    let recent_paths = active_files.clone();
    let summary = summarize_details(details, headline, &next_step);
    let open_questions = derive_open_questions(details);
    let materialized_notes = extract_materialized_notes(details);
    let payload = json!({
        "working_state_event": {
            "event_id": event_id,
            "project": project_json(project),
            "namespace": namespace_json(namespace),
            "recorded_at_epoch_ms": recorded_at_epoch_ms,
            "event_kind": "continuity_handoff",
            "session_id": session_id,
            "agent_scope": agent_scope,
            "thread_id": thread_id,
            "source_kind": "continuity_handoff",
            "headline": headline,
            "next_step_hint": next_step,
            "summary": summary,
            "active_files": active_files,
            "recent_paths": recent_paths,
            "visible_projects": vec![project.code.clone()],
            "query": Value::Null,
            "query_type": Value::Null,
            "target_kind": "handoff",
            "current_hypothesis": extract_first_question(details),
            "rejected_hypotheses": Vec::<String>::new(),
            "open_questions": open_questions,
            "materialized_notes": materialized_notes,
            "required_task_set": extract_required_task_set(details),
            "pending_return_queue": pending_return_queue,
            "client_budget_target_percent": client_budget_target_percent,
            "resolve_current_goal": resolve_current_goal,
            "resolved_pending_return_headlines": resolved_headlines,
            "resolved_pending_return_task_ids": resolved_task_ids,
            "last_command": "continuity handoff".to_string(),
            "last_results_summary": format!("Зафиксирован handoff для {} :: {}", project.code, namespace.code),
            "local_path": local_path,
        }
    });
    let insert_snapshot_started = Instant::now();
    let snapshot_id =
        postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
    continuity_profile_log(
        "record_handoff.insert_working_state_event",
        insert_snapshot_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let ledger_started = Instant::now();
    postgres::insert_execctl_task_ledger_entry(
        db,
        &postgres::ExecCtlTaskLedgerEntryInsert {
            project_id: project.project_id,
            namespace_id: namespace.namespace_id,
            agent_scope: &agent_scope,
            session_id: Some(session_id.as_str()),
            thread_id: thread_id.as_deref(),
            source_snapshot_id: Some(snapshot_id),
            source_event_id: event_id.as_str(),
            event_kind: "continuity_handoff",
            source_kind: "continuity_handoff",
            headline,
            next_step: &next_step,
            summary: summary.as_str(),
            active_files: &payload["working_state_event"]["active_files"],
            open_questions: &payload["working_state_event"]["open_questions"],
            materialized_notes: &payload["working_state_event"]["materialized_notes"],
            pending_return_queue: &payload["working_state_event"]["pending_return_queue"],
            local_path: Some(local_path),
            recorded_at_epoch_ms: recorded_at_epoch_ms as i64,
        },
    )
    .await?;
    continuity_profile_log(
        "record_handoff.insert_ledger",
        ledger_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    if let Err(error) = mirror_handoff_into_commitment_graph(
        db,
        project,
        namespace,
        event_id.as_str(),
        snapshot_id,
        &agent_scope,
        session_id.as_str(),
        thread_id.as_deref(),
        headline,
        &next_step,
        summary.as_str(),
        local_path,
        &payload["working_state_event"]["pending_return_queue"],
        recorded_at_epoch_ms as i64,
    )
    .await
    {
        tracing::warn!(
            ?error,
            project = %project.code,
            namespace = %namespace.code,
            source_event_id = %event_id,
            "failed to mirror continuity handoff into commitment graph"
        );
    }
    let lease_expires_at_epoch_ms =
        recorded_at_epoch_ms.saturating_add(EXECCTL_LEASE_TTL_MS) as i64;
    let lease_started = Instant::now();
    postgres::upsert_execctl_task_lease(
        db,
        &postgres::ExecCtlTaskLeaseInsert {
            project_id: project.project_id,
            namespace_id: namespace.namespace_id,
            agent_scope: &agent_scope,
            owner_session_id: Some(session_id.as_str()),
            owner_thread_id: thread_id.as_deref(),
            source_snapshot_id: Some(snapshot_id),
            source_event_id: event_id.as_str(),
            source_kind: "continuity_handoff",
            lease_state: "active",
            headline,
            next_step: &next_step,
            local_path: Some(local_path),
            acquired_at_epoch_ms: recorded_at_epoch_ms as i64,
            heartbeat_at_epoch_ms: recorded_at_epoch_ms as i64,
            expires_at_epoch_ms: lease_expires_at_epoch_ms,
        },
    )
    .await?;
    continuity_profile_log(
        "record_handoff.upsert_lease",
        lease_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    if refresh_restore_snapshot_after_write {
        let refresh_started = Instant::now();
        let _ = if let Some(previous_restore) = previous_restore.as_ref() {
            force_refresh_restore_snapshot_from_previous_handoff(
                db,
                project,
                namespace,
                previous_restore,
                &payload["working_state_event"],
                snapshot_id,
            )
            .await?
        } else {
            force_refresh_restore_snapshot(db, project, namespace).await?
        };
        continuity_profile_log(
            "record_handoff.refresh_restore_snapshot",
            refresh_started.elapsed().as_millis(),
            &format!("project={} namespace={}", project.code, namespace.code),
        );
    }
    continuity_profile_log(
        "record_handoff.total",
        total_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    Ok(())
}

pub async fn record_client_budget_target_event(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    target_percent: u64,
) -> Result<()> {
    record_client_budget_target_event_with_thread_hint(db, project, namespace, target_percent, None)
        .await
}

pub async fn record_client_budget_target_event_with_thread_hint(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    target_percent: u64,
    thread_id_hint: Option<&str>,
) -> Result<()> {
    let total_started = Instant::now();
    let target_percent =
        normalize_client_budget_target_percent(target_percent).ok_or_else(|| {
            anyhow::anyhow!(
                "client budget target must be one of 0, 10, 20, 30, 40, 50, 60, 70, 80, 90"
            )
        })?;
    let previous_restore_started = Instant::now();
    let previous_restore =
        load_recent_restore_bundle_without_live_guard(db, project, namespace).await?;
    continuity_profile_log(
        "client_budget_target.previous_restore",
        previous_restore_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    let current_goal = previous_restore
        .as_ref()
        .and_then(|value| value["working_state_restore"]["current_goal"].as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Продолжить активную рабочую линию");
    let next_step = previous_restore
        .as_ref()
        .and_then(|value| value["working_state_restore"]["next_step"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(normalize_next_step_hint)
        .unwrap_or_else(|| "Продолжить работу с новым target для клиентской экономии.".to_string());
    let recorded_at_epoch_ms = now_epoch_ms()?;
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let thread_id = resolve_thread_id_for_project_repo_root(&project.repo_root, thread_id_hint);
    let session_id_started = Instant::now();
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    continuity_profile_log(
        "client_budget_target.resolve_session_id",
        session_id_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    let event_id = Uuid::new_v4().to_string();
    let summary = format!("Client budget target set to {target_percent}%.");
    let payload = json!({
        "working_state_event": {
            "event_id": event_id,
            "project": project_json(project),
            "namespace": namespace_json(namespace),
            "recorded_at_epoch_ms": recorded_at_epoch_ms,
            "event_kind": "client_budget_target_update",
            "session_id": session_id,
            "agent_scope": agent_scope,
            "thread_id": thread_id,
            "source_kind": "client_budget_target_update",
            "headline": current_goal,
            "next_step_hint": next_step,
            "summary": summary,
            "active_files": Vec::<String>::new(),
            "recent_paths": Vec::<String>::new(),
            "visible_projects": vec![project.code.clone()],
            "query": Value::Null,
            "query_type": Value::Null,
            "target_kind": "client_budget_target",
            "current_hypothesis": Value::Null,
            "rejected_hypotheses": Vec::<String>::new(),
            "open_questions": Vec::<String>::new(),
            "materialized_notes": vec![format!("Client budget target = {target_percent}%")],
            "pending_return_queue": Value::Null,
            "client_budget_target_percent": target_percent,
            "resolve_current_goal": false,
            "resolved_pending_return_headlines": Vec::<String>::new(),
            "last_command": "continuity client-budget-target".to_string(),
            "last_results_summary": summary,
            "local_path": "Amai client budget target".to_string(),
        }
    });
    let insert_snapshot_started = Instant::now();
    let _ = postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
    continuity_profile_log(
        "client_budget_target.insert_working_state_event",
        insert_snapshot_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    let refresh_started = Instant::now();
    refresh_restore_snapshot(db, project, namespace).await?;
    continuity_profile_log(
        "client_budget_target.refresh_restore_snapshot",
        refresh_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    continuity_profile_log(
        "client_budget_target.total",
        total_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} target={target_percent}",
            project.code, namespace.code
        ),
    );
    Ok(())
}

pub async fn record_host_current_thread_control_feedback_with_thread_hint(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    feedback_kind: &str,
    command_id_hint: Option<&str>,
    thread_id_hint: Option<&str>,
) -> Result<()> {
    let feedback_kind = normalize_host_current_thread_control_feedback_kind(feedback_kind)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "host current-thread control feedback must be one of requested, opened, failed"
            )
        })?;
    let previous_restore =
        load_recent_restore_bundle_without_live_guard(db, project, namespace).await?;
    let current_goal = previous_restore
        .as_ref()
        .and_then(|value| value["working_state_restore"]["current_goal"].as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Продолжить активную рабочую линию");
    let next_step = previous_restore
        .as_ref()
        .and_then(|value| value["working_state_restore"]["next_step"].as_str())
        .filter(|value| !value.trim().is_empty())
        .map(normalize_next_step_hint)
        .unwrap_or_else(|| {
            "Продолжить работу по same-thread control для клиентской экономии.".to_string()
        });
    let client_budget_target_percent = previous_restore.as_ref().and_then(|value| {
        value["working_state_restore"]["client_budget_target_percent"]
            .as_u64()
            .and_then(normalize_client_budget_target_percent)
    });
    let command_id = command_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| normalize_host_current_thread_control_command_id(Some(value)))
        .unwrap_or(HOST_CURRENT_THREAD_CONTROL_COMMAND_ID);
    let recorded_at_epoch_ms = now_epoch_ms()?;
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let thread_id = resolve_thread_id_for_project_repo_root(&project.repo_root, thread_id_hint);
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    let event_id = Uuid::new_v4().to_string();
    let summary = host_current_thread_control_feedback_summary(feedback_kind, Some(command_id));
    let feedback_snapshot =
        build_host_current_thread_control_feedback_snapshot_for_thread(thread_id.as_deref());
    let payload = json!({
        "working_state_event": {
            "event_id": event_id,
            "project": project_json(project),
            "namespace": namespace_json(namespace),
            "recorded_at_epoch_ms": recorded_at_epoch_ms,
            "event_kind": HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND,
            "session_id": session_id,
            "agent_scope": agent_scope,
            "thread_id": thread_id,
            "source_kind": HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND,
            "headline": current_goal,
            "next_step_hint": next_step,
            "summary": summary,
            "active_files": Vec::<String>::new(),
            "recent_paths": Vec::<String>::new(),
            "visible_projects": vec![project.code.clone()],
            "query": Value::Null,
            "query_type": Value::Null,
            "target_kind": "host_current_thread_control",
            "current_hypothesis": Value::Null,
            "rejected_hypotheses": Vec::<String>::new(),
            "open_questions": Vec::<String>::new(),
            "materialized_notes": vec![
                format!("Host current-thread control feedback = {feedback_kind}"),
                format!("Host current-thread control command = {command_id}")
            ],
            "pending_return_queue": Value::Null,
            "client_budget_target_percent": client_budget_target_percent,
            "resolve_current_goal": false,
            "resolved_pending_return_headlines": Vec::<String>::new(),
            "last_command": "dashboard client-budget-host-control-feedback".to_string(),
            "last_results_summary": summary,
            "local_path": "Amai host current-thread control feedback".to_string(),
            "host_current_thread_control_feedback": {
                "feedback_version": "host-current-thread-control-feedback-v2",
                "feedback_kind": feedback_kind,
                "command_id": command_id,
                "control_kind": host_current_thread_control_kind_for_command_id(command_id),
                "feedback_snapshot": feedback_snapshot,
            }
        }
    });
    let _ = postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
    refresh_restore_snapshot(db, project, namespace).await?;
    Ok(())
}

pub async fn record_context_pack_event(
    db: &Client,
    payload: &Value,
    token_source_kind: &str,
) -> Result<()> {
    let node = payload
        .as_object()
        .context("context pack payload must be a JSON object")?;
    let project_code = node["project"]["code"].as_str().unwrap_or_default();
    let namespace_code = node["namespace"]["code"].as_str().unwrap_or_default();
    if project_code.is_empty() || namespace_code.is_empty() {
        return Ok(());
    }
    let project = ProjectSummary {
        code: project_code.to_string(),
        display_name: node["project"]["display_name"]
            .as_str()
            .unwrap_or(project_code)
            .to_string(),
        repo_root: node["project"]["repo_root"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    };
    let namespace = NamespaceSummary {
        code: namespace_code.to_string(),
        display_name: node["namespace"]["display_name"]
            .as_str()
            .unwrap_or(namespace_code)
            .to_string(),
    };
    let query = node["query"].as_str().unwrap_or_default().to_string();
    let query_type = token_budget::derive_query_type(&query).to_string();
    let active_files = extract_active_files_from_context_pack(payload);
    let visible_projects = extract_visible_projects(node.get("visible_projects"));
    let exact_documents = node["retrieval"]["exact_documents"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let symbol_hits = node["retrieval"]["symbol_hits"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let lexical_chunks = node["retrieval"]["lexical_chunks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let semantic_chunks = node["retrieval"]["semantic_chunks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let target_kind = if exact_documents > 0 {
        "document"
    } else if symbol_hits > 0 {
        "symbol"
    } else if lexical_chunks > 0 || semantic_chunks > 0 {
        "file"
    } else {
        "unknown"
    };
    let recorded_at_epoch_ms = now_epoch_ms()?;
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let thread_id = if let Some(thread_id) = current_thread_id() {
        Some(thread_id)
    } else if project.repo_root.trim().is_empty() {
        None
    } else {
        token_budget::preferred_dashboard_thread_binding_hint(db, Path::new(&project.repo_root))
            .await?
    };
    let turn_id = thread_id
        .as_deref()
        .and_then(|thread_id| {
            codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)
                .ok()
                .flatten()
        })
        .map(|observation| observation.turn_id)
        .filter(|value| !value.trim().is_empty());
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    let memory_cards = payload["retrieval"]["memory_cards"]
        .as_array()
        .map_or(0, Vec::len);
    let query_summary = format!(
        "Найдено: документов {}, символов {}, lexical chunks {}, semantic chunks {}, memory cards {}.",
        exact_documents, symbol_hits, lexical_chunks, semantic_chunks, memory_cards
    );
    let traffic_class = token_budget::derive_traffic_class(token_source_kind);
    let payload = json!({
        "working_state_event": {
            "event_id": Uuid::new_v4().to_string(),
            "project": project,
            "namespace": namespace,
            "recorded_at_epoch_ms": recorded_at_epoch_ms,
            "event_kind": "retrieval_context_pack",
            "session_id": session_id,
            "agent_scope": agent_scope,
            "thread_id": thread_id,
            "turn_id": turn_id,
            "source_kind": "context_pack",
            "token_source_kind": token_source_kind,
            "traffic_class": traffic_class,
            "headline": format!("Рабочий запрос: {}", query),
            "next_step_hint": derive_retrieval_next_step(&active_files, target_kind),
            "summary": format!("{} {}", query, query_summary),
            "active_files": active_files,
            "recent_paths": extract_active_files_from_context_pack(payload),
            "visible_projects": visible_projects,
            "query": query,
            "query_type": query_type,
            "target_kind": target_kind,
            "current_hypothesis": derive_retrieval_hypothesis(payload),
            "rejected_hypotheses": Vec::<String>::new(),
            "open_questions": derive_open_questions(
                node["query"].as_str().unwrap_or_default()
            ),
            "last_command": format!(
                "context pack --project {} --namespace {} --query {}",
                project.code,
                namespace.code,
                node["query"].as_str().unwrap_or_default()
            ),
            "last_results_summary": query_summary,
            "context_pack_id": node["context_pack_id"].as_str().unwrap_or_default(),
            "retrieval_mode": node["effective_retrieval_mode"].as_str().unwrap_or_default(),
            "latency_ms": node["retrieval_runtime"]["total_ms"].clone(),
            "decision_trace": node.get("decision_trace").cloned().unwrap_or(Value::Null),
            "workspace_graph": node.get("workspace_graph").cloned().unwrap_or(Value::Null),
        }
    });
    postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
    if traffic_class == "live" && !project.repo_root.is_empty() {
        if let Ok(recorded_at_epoch_ms) = i64::try_from(recorded_at_epoch_ms) {
            let _ = token_budget::bump_dashboard_live_turn_retrieval_invalidation(
                Path::new(&project.repo_root),
                recorded_at_epoch_ms,
            );
        }
    }
    let persisted_project = postgres::get_project_by_code(db, &project.code).await?;
    let project_record = ProjectRecord {
        project_id: persisted_project.project_id,
        code: project.code,
        display_name: project.display_name,
        repo_root: project.repo_root,
        visibility_scope: persisted_project.visibility_scope,
        updated_at: String::new(),
    };
    let persisted_namespace =
        postgres::get_namespace_by_code(db, project_record.project_id, &namespace.code).await?;
    let namespace_record = NamespaceRecord {
        namespace_id: persisted_namespace.namespace_id,
        code: namespace.code,
        display_name: namespace.display_name,
        retrieval_mode: persisted_namespace.retrieval_mode,
    };
    refresh_restore_snapshot(db, &project_record, &namespace_record).await?;
    Ok(())
}

async fn build_restore_bundle_without_live_guard(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<Value>> {
    let total_started = Instant::now();
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let step_started = Instant::now();
    let events = load_selected_working_state_events(db, project, namespace).await?;
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.load_selected_events",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} events={}",
            project.code,
            namespace.code,
            events.len()
        ),
    );
    if events.is_empty() {
        continuity_profile_log(
            "build_restore_bundle_without_live_guard.total",
            total_started.elapsed().as_millis(),
            &format!(
                "project={} namespace={} no_events=true",
                project.code, namespace.code
            ),
        );
        return Ok(None);
    }
    let step_started = Instant::now();
    let mut bundle =
        compose_restore_bundle(&project_json(project), &namespace_json(namespace), &events);
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.compose_restore_bundle",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let step_started = Instant::now();
    let durable_entries = postgres::list_execctl_task_ledger_entries(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        Some(MAX_EXECCTL_LEDGER_ENTRIES),
    )
    .await?;
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.list_execctl_task_ledger_entries",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} entries={}",
            project.code,
            namespace.code,
            durable_entries.len()
        ),
    );
    let step_started = Instant::now();
    overlay_durable_project_task_ledger(
        &mut bundle,
        &project_json(project),
        &namespace_json(namespace),
        &durable_entries,
    );
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.overlay_durable_project_task_ledger",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let step_started = Instant::now();
    let active_lease = postgres::get_execctl_task_lease(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        now_epoch_ms()? as i64,
    )
    .await?;
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.get_execctl_task_lease",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} has_lease={}",
            project.code,
            namespace.code,
            active_lease.is_some()
        ),
    );
    let step_started = Instant::now();
    overlay_execctl_active_lease(&mut bundle, active_lease.as_ref());
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.overlay_execctl_active_lease",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let step_started = Instant::now();
    overlay_restore_execution_card(db, project, namespace, &mut bundle).await?;
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.overlay_restore_execution_card",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let step_started = Instant::now();
    overlay_workspace_restore_pack(&mut bundle);
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.overlay_workspace_restore_pack",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    continuity_profile_log(
        "build_restore_bundle_without_live_guard.total",
        total_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    Ok(Some(bundle))
}

#[derive(Debug, Clone)]
struct RestoreExecutionCardBinding {
    thread_id: Option<String>,
    runtime: Option<String>,
    model: Option<String>,
    tool: Option<String>,
    model_provider: Option<String>,
    binding_source: String,
}

fn normalize_lower_token(token: &str) -> Option<String> {
    let trimmed = token.trim().to_lowercase();
    let normalized = trimmed
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() < 3 {
        return None;
    }
    match compact.as_str() {
        "the" | "and" | "for" | "with" | "that" | "this" | "from" | "into" | "или" | "для"
        | "как" | "что" | "при" | "это" | "если" | "его" | "без" => None,
        _ => Some(compact),
    }
}

fn collect_restore_relevance_terms(node: &Value) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for text in [
        node["current_goal"].as_str(),
        node["next_step"].as_str(),
        node["current_focus"].as_str(),
        node["source_summary"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        for token in text.split_whitespace() {
            if let Some(term) = normalize_lower_token(token) {
                terms.insert(term);
            }
        }
    }
    for query in node["recent_queries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        for token in query.split_whitespace() {
            if let Some(term) = normalize_lower_token(token) {
                terms.insert(term);
            }
        }
    }
    terms
}

fn score_restore_execution_card_relevance(card: &Value, terms: &BTreeSet<String>) -> i32 {
    if terms.is_empty() {
        return 0;
    }
    let mut haystacks = Vec::new();
    for text in [
        card["skill_title"].as_str(),
        card["skill_goal"].as_str(),
        card["skill_expected_outcome"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        haystacks.push(text.to_lowercase());
    }
    for key in [
        "skill_trigger_conditions",
        "skill_execution_steps",
        "skill_preconditions",
        "skill_stop_conditions",
        "skill_forbidden_when",
        "skill_context_constraints",
    ] {
        for value in card[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            haystacks.push(value.to_lowercase());
        }
    }

    let mut score = 0;
    for term in terms {
        for haystack in &haystacks {
            if haystack.contains(term) {
                score += if haystack
                    == &card["skill_title"]
                        .as_str()
                        .unwrap_or_default()
                        .to_lowercase()
                {
                    4
                } else {
                    2
                };
                break;
            }
        }
    }
    score
}

fn compact_restore_execution_card(
    card: &Value,
    relevance_score: i32,
    binding: &RestoreExecutionCardBinding,
) -> Value {
    json!({
        "skill_card_id": card["skill_card_id"].clone(),
        "skill_id": card["skill_id"].clone(),
        "skill_version": card["skill_version"].clone(),
        "skill_title": card["skill_title"].clone(),
        "skill_goal": card["skill_goal"].clone(),
        "skill_trigger_conditions": card["skill_trigger_conditions"].clone(),
        "skill_preconditions": card["skill_preconditions"].clone(),
        "skill_execution_steps": card["skill_execution_steps"].clone(),
        "skill_stop_conditions": card["skill_stop_conditions"].clone(),
        "skill_forbidden_when": card["skill_forbidden_when"].clone(),
        "skill_expected_outcome": card["skill_expected_outcome"].clone(),
        "skill_trust_state": card["skill_trust_state"].clone(),
        "skill_verification_state": card["skill_verification_state"].clone(),
        "skill_candidate_class": card["skill_candidate_class"].clone(),
        "skill_scope_type": card["skill_scope_type"].clone(),
        "skill_owner_scope": card["skill_owner_scope"].clone(),
        "skill_context_constraints": card["skill_context_constraints"].clone(),
        "skill_runtime_constraints": card["skill_runtime_constraints"].clone(),
        "skill_model_constraints": card["skill_model_constraints"].clone(),
        "skill_tool_constraints": card["skill_tool_constraints"].clone(),
        "skill_utility_score": card["skill_utility_score"].clone(),
        "skill_success_count": card["skill_success_count"].clone(),
        "skill_failure_count": card["skill_failure_count"].clone(),
        "skill_reuse_count": card["skill_reuse_count"].clone(),
        "skill_patch_parent_id": card["skill_patch_parent_id"].clone(),
        "skill_merge_group_id": card["skill_merge_group_id"].clone(),
        "restore_relevance_score": relevance_score,
        "binding": {
            "thread_id": binding.thread_id,
            "runtime": binding.runtime,
            "model": binding.model,
            "tool": binding.tool,
            "model_provider": binding.model_provider,
            "binding_source": binding.binding_source,
        }
    })
}

fn summarize_restore_execution_card(card: &Value) -> Option<String> {
    let title = card["skill_title"]
        .as_str()
        .filter(|value| !value.is_empty())?;
    let trust = card["skill_trust_state"].as_str().unwrap_or("candidate");
    let first_step = card["skill_execution_steps"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("без шага");
    Some(format!("{title} [{trust}] -> {first_step}"))
}

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

fn build_workspace_restore_pack(restore: &Value) -> Value {
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

fn overlay_workspace_restore_pack(bundle: &mut Value) {
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

fn compact_persisted_project_task_ledger(ledger: &Value) -> Value {
    let Some(entries) = ledger["entries"].as_array() else {
        return ledger.clone();
    };
    let mut kept = Vec::new();
    let mut historical_kept = 0usize;
    for entry in entries {
        let task_role = entry["task_role"].as_str().unwrap_or_default();
        let keep = match task_role {
            "active" | "pending_return" => true,
            "historical_handoff" => {
                if historical_kept < MAX_PERSISTED_PROJECT_TASK_LEDGER_HISTORICAL_ENTRIES {
                    historical_kept += 1;
                    true
                } else {
                    false
                }
            }
            _ => true,
        };
        if keep {
            kept.push(entry.clone());
        }
    }
    let mut compact = ledger.clone();
    if let Some(object) = compact.as_object_mut() {
        object.insert("entries".to_string(), Value::Array(kept));
        object.insert(
            "full_shape_preserved_in_working_state_restore".to_string(),
            Value::Bool(false),
        );
    }
    compact
}

fn persisted_restore_snapshot_payload(bundle: &Value) -> Value {
    let mut payload = json!({
        "working_state_restore": bundle["working_state_restore"].clone()
    });
    if let Some(restore) = payload
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    {
        if restore["project_task_ledger"].is_object() {
            restore.insert(
                "project_task_ledger".to_string(),
                compact_persisted_project_task_ledger(&restore["project_task_ledger"]),
            );
        }
        restore.remove("workspace_restore_pack");
        restore.remove("workspace_restore_pack_summary");
        restore.remove("skill_execution_card");
        restore.remove("skill_execution_card_summary");
        restore.remove("skill_execution_card_binding");
    }
    payload
}

fn resolve_restore_execution_card_binding(
    project: &ProjectRecord,
    node: &Value,
) -> RestoreExecutionCardBinding {
    let explicit_runtime = env::var("AMAI_RESTORE_EXECUTION_CARD_RUNTIME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let explicit_model = env::var("AMAI_RESTORE_EXECUTION_CARD_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let explicit_tool = env::var("AMAI_RESTORE_EXECUTION_CARD_TOOL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let thread_id = node["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| resolve_thread_id_for_project_repo_root(&project.repo_root, None));

    let recent_thread =
        codex_threads::recent_client_thread_records(RESTORE_EXECUTION_CARD_THREAD_WINDOW_SECONDS)
            .ok()
            .and_then(|items| {
                items.into_iter().find(|item| {
                    thread_id.as_deref() == Some(item.thread_id.as_str())
                        || (!project.repo_root.trim().is_empty() && item.cwd == project.repo_root)
                })
            });

    let model =
        explicit_model.or_else(|| recent_thread.as_ref().and_then(|item| item.model.clone()));
    let model_provider = recent_thread
        .as_ref()
        .and_then(|item| item.model_provider.clone());
    let runtime = explicit_runtime.or_else(|| model.as_ref().map(|_| "codex".to_string()));
    let tool = explicit_tool.or_else(|| runtime.as_ref().map(|_| "exec_command".to_string()));
    let binding_source = if env::var("AMAI_RESTORE_EXECUTION_CARD_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some()
    {
        "env_override".to_string()
    } else if recent_thread.is_some() {
        "recent_client_thread".to_string()
    } else if runtime.is_some() || tool.is_some() {
        "runtime_default_without_model".to_string()
    } else {
        "unbound".to_string()
    };

    RestoreExecutionCardBinding {
        thread_id,
        runtime,
        model,
        tool,
        model_provider,
        binding_source,
    }
}

async fn overlay_restore_execution_card(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    bundle: &mut Value,
) -> Result<()> {
    let total_started = Instant::now();
    let Some(node) = bundle
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    let node_snapshot = Value::Object(node.clone());
    let step_started = Instant::now();
    let binding = resolve_restore_execution_card_binding(project, &node_snapshot);
    continuity_profile_log(
        "overlay_restore_execution_card.resolve_binding",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let Some(model) = binding.model.as_deref() else {
        node.insert("skill_execution_card".to_string(), Value::Null);
        node.insert("skill_execution_card_summary".to_string(), Value::Null);
        node.insert(
            "skill_execution_card_binding".to_string(),
            json!({
                "thread_id": binding.thread_id,
                "runtime": binding.runtime,
                "model": binding.model,
                "tool": binding.tool,
                "model_provider": binding.model_provider,
                "binding_source": binding.binding_source,
                "binding_ready": false,
            }),
        );
        continuity_profile_log(
            "overlay_restore_execution_card.total",
            total_started.elapsed().as_millis(),
            &format!(
                "project={} namespace={} binding_ready=false",
                project.code, namespace.code
            ),
        );
        return Ok(());
    };

    let step_started = Instant::now();
    let cards = postgres::build_skill_execution_cards(
        db,
        &project.code,
        &namespace.code,
        None,
        binding.runtime.as_deref(),
        Some(model),
        binding.tool.as_deref(),
        true,
        false,
        false,
    )
    .await?;
    continuity_profile_log(
        "overlay_restore_execution_card.build_skill_execution_cards",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} cards={}",
            project.code,
            namespace.code,
            cards.as_array().map(|items| items.len()).unwrap_or(0)
        ),
    );
    let step_started = Instant::now();
    let terms = collect_restore_relevance_terms(&node_snapshot);
    let selected = cards
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|card| {
            let score = score_restore_execution_card_relevance(card, &terms);
            (score > 0).then_some((score, card))
        })
        .max_by(|(left_score, left_card), (right_score, right_card)| {
            left_score.cmp(right_score).then_with(|| {
                left_card["skill_utility_score"]
                    .as_f64()
                    .partial_cmp(&right_card["skill_utility_score"].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
    continuity_profile_log(
        "overlay_restore_execution_card.select_card",
        step_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} terms={}",
            project.code,
            namespace.code,
            terms.len()
        ),
    );

    if let Some((score, card)) = selected {
        let compact = compact_restore_execution_card(card, score, &binding);
        let summary = summarize_restore_execution_card(&compact);
        node.insert("skill_execution_card".to_string(), compact);
        node.insert(
            "skill_execution_card_summary".to_string(),
            summary.map(Value::String).unwrap_or(Value::Null),
        );
    } else {
        node.insert("skill_execution_card".to_string(), Value::Null);
        node.insert("skill_execution_card_summary".to_string(), Value::Null);
    }
    node.insert(
        "skill_execution_card_binding".to_string(),
        json!({
            "thread_id": binding.thread_id,
            "runtime": binding.runtime,
            "model": binding.model,
            "tool": binding.tool,
            "model_provider": binding.model_provider,
            "binding_source": binding.binding_source,
            "binding_ready": true,
        }),
    );
    continuity_profile_log(
        "overlay_restore_execution_card.total",
        total_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} binding_ready=true selected={}",
            project.code,
            namespace.code,
            selected.is_some()
        ),
    );
    Ok(())
}

pub async fn load_recent_restore_bundle_without_live_guard(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<Value>> {
    let latest_snapshot = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        WORKING_STATE_RESTORE_KIND,
        &project.code,
        &namespace.code,
        Some(1),
    )
    .await?;
    if let Some(snapshot) = latest_snapshot.first() {
        let captured_at_epoch_ms =
            snapshot_payload_captured_at_epoch_ms(snapshot, "working_state_restore");
        if now_epoch_ms()?.saturating_sub(captured_at_epoch_ms)
            <= WORKING_STATE_RESTORE_REFRESH_MIN_INTERVAL_MS
        {
            let events = load_selected_working_state_events(db, project, namespace).await?;
            if !events.is_empty()
                && restore_snapshot_matches_latest_relevant_events(&snapshot.payload, &events)
            {
                let mut bundle = snapshot.payload.clone();
                ensure_runtime_workspace_restore_pack(&mut bundle);
                return Ok(Some(bundle));
            }
        }
    }
    build_restore_bundle_without_live_guard(db, project, namespace).await
}

pub async fn build_restore_bundle(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<Value>> {
    let total_started = Instant::now();
    let Some(mut bundle) = build_restore_bundle_without_live_guard(db, project, namespace).await?
    else {
        continuity_profile_log(
            "build_restore_bundle.total",
            total_started.elapsed().as_millis(),
            &format!(
                "project={} namespace={} no_bundle=true",
                project.code, namespace.code
            ),
        );
        return Ok(None);
    };
    let step_started = Instant::now();
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(db, Some(&bundle))
            .await
            .unwrap_or_else(|error| fallback_client_budget_guard_from_error(&error.to_string()));
    continuity_profile_log(
        "build_restore_bundle.collect_live_current_session_budget_guard",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let step_started = Instant::now();
    overlay_client_budget_guard(&mut bundle, &client_budget_guard);
    continuity_profile_log(
        "build_restore_bundle.overlay_client_budget_guard",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let step_started = Instant::now();
    overlay_workspace_restore_pack(&mut bundle);
    continuity_profile_log(
        "build_restore_bundle.overlay_workspace_restore_pack",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    continuity_profile_log(
        "build_restore_bundle.total",
        total_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    Ok(Some(bundle))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecCtlStartupLeaseRefreshCandidate {
    source_event_id: String,
    source_kind: String,
    headline: String,
    next_step: String,
    local_path: Option<String>,
}

fn execctl_startup_same_thread_lease_refresh_candidate(
    restore_bundle: &Value,
    current_thread_id: Option<&str>,
) -> Option<ExecCtlStartupLeaseRefreshCandidate> {
    let restore = restore_bundle.get("working_state_restore")?;
    let current_thread_id = current_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let _ = current_thread_id;

    let active_task = restore["project_task_tree"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|node| node["task_role"].as_str() == Some("active"));
    let source_event_id = active_task
        .and_then(|node| node["authoritative_event_id"].as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            restore["state_lineage"]["authoritative_event_id"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
        })?
        .trim()
        .to_string();
    let source_kind = active_task
        .and_then(|node| node["source_kind"].as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            restore["state_lineage"]["authoritative_source_kind"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or("working_state_restore")
        .trim()
        .to_string();
    let headline = active_task
        .and_then(|node| node["headline"].as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            restore["current_goal"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
        })?
        .trim()
        .to_string();
    let next_step = active_task
        .and_then(|node| node["next_step"].as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            restore["next_step"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
        })
        .map(normalize_next_step_hint)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let local_path = restore["state_lineage"]["authoritative_local_path"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .or_else(|| {
            restore["project_task_ledger"]["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["task_role"].as_str() == Some("active"))
                .and_then(|entry| entry["local_path"].as_str())
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_string())
        });

    Some(ExecCtlStartupLeaseRefreshCandidate {
        source_event_id,
        source_kind,
        headline,
        next_step,
        local_path,
    })
}

fn execctl_same_thread_lease_refresh_required(
    active_lease: Option<&ExecCtlTaskLeaseRecord>,
    current_thread_id: Option<&str>,
    now_epoch_ms: u64,
    min_refresh_interval_ms: u64,
) -> bool {
    let Some(current_thread_id) = current_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Some(active_lease) = active_lease else {
        return true;
    };
    let owner_thread_id = active_lease
        .owner_thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if owner_thread_id != Some(current_thread_id) {
        return true;
    }
    let heartbeat_at_epoch_ms = active_lease.heartbeat_at_epoch_ms.max(0) as u64;
    let expires_at_epoch_ms = active_lease.expires_at_epoch_ms.max(0) as u64;
    now_epoch_ms.saturating_sub(heartbeat_at_epoch_ms) >= min_refresh_interval_ms
        || expires_at_epoch_ms.saturating_sub(now_epoch_ms) <= min_refresh_interval_ms
}

pub fn print_restore_bundle_human(restore: &Value) {
    let node = &restore["working_state_restore"];
    let next_step = node["next_step"]
        .as_str()
        .map(normalize_next_step_hint)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    println!("Рабочее состояние Amai:");
    println!(
        "- Agent scope: {}",
        node["agent_scope"].as_str().unwrap_or("shared")
    );
    println!(
        "- Активная сессия: {}",
        human_duration_ms(node["session_age_ms"].as_u64().unwrap_or(0))
    );
    println!(
        "- Текущая цель: {}",
        node["current_goal"].as_str().unwrap_or("ещё нет данных")
    );
    println!("- Ближайший следующий шаг: {}", next_step);
    println!(
        "- Целевой client-budget target: {}%",
        client_budget_target_percent_from_restore_context(node)
    );
    if let Some(value) = node["execctl_active_lease_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Активный lease ExecCtl: {value}");
    }
    if let Some(value) = node["restore_confidence"]
        .as_str()
        .filter(|value| *value == "preliminary")
    {
        let _ = value;
        println!("- Статус recovery: предварительно, потому что живая выборка ещё маленькая.");
    }
    if let Some(value) = node["current_hypothesis"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Рабочая гипотеза: {value}");
    }
    print_string_list("- Активные файлы", &node["active_files"], MAX_ACTIVE_FILES);
    print_string_list(
        "- Последние рабочие запросы",
        &node["recent_queries"],
        MAX_RECENT_QUERIES,
    );
    print_string_list(
        "- Открытые вопросы",
        &node["open_questions"],
        MAX_OPEN_QUESTIONS,
    );
    print_string_list(
        "- Materialized решения",
        &node["materialized_notes"],
        MAX_MATERIALIZED_NOTES,
    );
    if let Some(value) = node["pending_return_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Незавершённые линии к возврату: {value}");
    }
    if let Some(value) = node["skill_execution_card_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Исполнимая procedural-карточка: {value}");
    }
    if let Some(value) = node["included_reasons_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Почему вошло: {value}");
    }
    if let Some(value) = node["excluded_reasons_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        println!("- Почему часть не вошла: {value}");
    }
    if let Some(summary) = workspace_graph::human_summary(&node["workspace_graph"]) {
        println!("- Структурный граф рабочей области: {summary}");
    }
    print_recent_actions("- Недавние действия", &node["recent_actions"], 3);
}

async fn refresh_restore_snapshot(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<()> {
    let total_started = Instant::now();
    let now_epoch_ms = now_epoch_ms()?;
    let latest_snapshot_started = Instant::now();
    let latest_snapshot = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        WORKING_STATE_RESTORE_KIND,
        &project.code,
        &namespace.code,
        Some(1),
    )
    .await?;
    continuity_profile_log(
        "refresh_restore_snapshot.list_latest_snapshot",
        latest_snapshot_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    if let Some(snapshot) = latest_snapshot.first() {
        let captured_at_epoch_ms =
            snapshot_payload_captured_at_epoch_ms(snapshot, "working_state_restore");
        if now_epoch_ms.saturating_sub(captured_at_epoch_ms)
            <= WORKING_STATE_RESTORE_REFRESH_MIN_INTERVAL_MS
        {
            let selected_events_started = Instant::now();
            let events = load_selected_working_state_events(db, project, namespace).await?;
            continuity_profile_log(
                "refresh_restore_snapshot.load_selected_events_fast_check",
                selected_events_started.elapsed().as_millis(),
                &format!(
                    "project={} namespace={} events={}",
                    project.code,
                    namespace.code,
                    events.len()
                ),
            );
            if !events.is_empty()
                && restore_snapshot_matches_latest_relevant_events(&snapshot.payload, &events)
            {
                continuity_profile_log(
                    "refresh_restore_snapshot.total",
                    total_started.elapsed().as_millis(),
                    &format!(
                        "project={} namespace={} reused_snapshot=true",
                        project.code, namespace.code
                    ),
                );
                return Ok(());
            }
        }
    }
    let build_bundle_started = Instant::now();
    let Some(bundle) = build_restore_bundle_without_live_guard(db, project, namespace).await?
    else {
        continuity_profile_log(
            "refresh_restore_snapshot.total",
            total_started.elapsed().as_millis(),
            &format!(
                "project={} namespace={} no_bundle=true",
                project.code, namespace.code
            ),
        );
        return Ok(());
    };
    continuity_profile_log(
        "refresh_restore_snapshot.build_bundle",
        build_bundle_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let payload = persisted_restore_snapshot_payload(&bundle);
    let insert_snapshot_started = Instant::now();
    let snapshot_id =
        postgres::insert_observability_snapshot(db, WORKING_STATE_RESTORE_KIND, &payload).await?;
    continuity_profile_log(
        "refresh_restore_snapshot.insert_snapshot",
        insert_snapshot_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let materialize_started = Instant::now();
    materialize_restore_pack(db, project, namespace, &bundle, snapshot_id).await?;
    continuity_profile_log(
        "refresh_restore_snapshot.materialize_restore_pack",
        materialize_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    continuity_profile_log(
        "refresh_restore_snapshot.total",
        total_started.elapsed().as_millis(),
        &format!(
            "project={} namespace={} reused_snapshot=false",
            project.code, namespace.code
        ),
    );
    Ok(())
}

async fn force_refresh_restore_snapshot(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<Value>> {
    let Some(bundle) = build_restore_bundle_without_live_guard(db, project, namespace).await?
    else {
        return Ok(None);
    };
    let payload = persisted_restore_snapshot_payload(&bundle);
    let snapshot_id =
        postgres::insert_observability_snapshot(db, WORKING_STATE_RESTORE_KIND, &payload).await?;
    materialize_restore_pack(db, project, namespace, &bundle, snapshot_id).await?;
    Ok(Some(bundle))
}

async fn force_refresh_restore_snapshot_from_previous_handoff(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    previous_restore: &Value,
    authoritative_event: &Value,
    source_snapshot_id: Uuid,
) -> Result<Option<Value>> {
    let project_value = project_json(project);
    let namespace_value = namespace_json(namespace);
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let durable_entries_started = Instant::now();
    let durable_entries = postgres::list_execctl_task_ledger_entries(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        Some(MAX_EXECCTL_LEDGER_ENTRIES),
    )
    .await?;
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.load_durable_entries",
        durable_entries_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let active_lease_started = Instant::now();
    let active_lease = postgres::get_execctl_task_lease(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        now_epoch_ms()? as i64,
    )
    .await?;
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.load_active_lease",
        active_lease_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let mut bundle = previous_restore.clone();
    let Some(restore) = bundle
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    else {
        return force_refresh_restore_snapshot(db, project, namespace).await;
    };
    let now_epoch_ms = now_epoch_ms().unwrap_or_default();
    let recorded_at_epoch_ms = authoritative_event["recorded_at_epoch_ms"]
        .as_u64()
        .unwrap_or(now_epoch_ms);
    let current_goal = authoritative_event["headline"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("ещё нет данных")
        .to_string();
    let next_step = authoritative_event["next_step_hint"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("ещё нет данных")
        .to_string();
    let pending_return_queue = authoritative_event["pending_return_queue"].clone();
    let pending_return_summary = summarize_pending_return_queue(&pending_return_queue);
    let required_task_set = extract_required_task_set_from_event(authoritative_event);
    let required_task_set_summary = summarize_required_task_set(&required_task_set);

    let mut active_files = Vec::new();
    collect_active_files(&mut active_files, &authoritative_event["active_files"]);
    collect_active_files(&mut active_files, &restore["active_files"]);
    let mut visible_projects = Vec::new();
    collect_unique_strings(
        &mut visible_projects,
        &authoritative_event["visible_projects"],
    );
    collect_unique_strings(&mut visible_projects, &restore["visible_projects"]);
    let mut recent_queries = Vec::new();
    if let Some(query) = authoritative_event["query"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        push_unique(&mut recent_queries, query.to_string());
    }
    collect_unique_strings(&mut recent_queries, &restore["recent_queries"]);
    let mut open_questions = Vec::new();
    collect_open_questions(&mut open_questions, &authoritative_event["open_questions"]);
    collect_open_questions(&mut open_questions, &restore["open_questions"]);
    let mut rejected_hypotheses = Vec::new();
    collect_unique_strings(
        &mut rejected_hypotheses,
        &authoritative_event["rejected_hypotheses"],
    );
    collect_unique_strings(&mut rejected_hypotheses, &restore["rejected_hypotheses"]);
    let mut materialized_notes = Vec::new();
    collect_materialized_notes(
        &mut materialized_notes,
        &authoritative_event["materialized_notes"],
    );
    collect_materialized_notes(&mut materialized_notes, &restore["materialized_notes"]);
    let current_hypothesis = authoritative_event["current_hypothesis"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            restore["current_hypothesis"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let last_command = authoritative_event["last_command"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            restore["last_command"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let last_results_summary = authoritative_event["last_results_summary"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            restore["last_results_summary"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let included_reasons_summary = decision_trace_summary(Some(authoritative_event), "included");
    let excluded_reasons_summary =
        decision_trace_summary(Some(authoritative_event), "not_included");
    let current_focus = derive_restore_current_focus(
        &active_files,
        &recent_queries,
        current_hypothesis.as_deref(),
    );
    let source_summary = derive_restore_source_summary(
        authoritative_event,
        &active_files,
        included_reasons_summary.as_deref(),
    );

    let mut recent_actions = vec![json!({
        "event_id": authoritative_event["event_id"],
        "event_kind": authoritative_event["event_kind"],
        "source_kind": authoritative_event["source_kind"],
        "headline": authoritative_event["headline"],
        "summary": authoritative_event["summary"],
        "recorded_at_epoch_ms": authoritative_event["recorded_at_epoch_ms"],
        "local_path": authoritative_event["local_path"],
        "host_current_thread_control_feedback": if authoritative_event["host_current_thread_control_feedback"].is_object() {
            authoritative_event["host_current_thread_control_feedback"].clone()
        } else {
            Value::Null
        },
        "execution_state": "succeeded",
        "authoritative": true,
    })];
    if let Some(items) = restore["recent_actions"].as_array() {
        for item in items {
            if recent_actions.len() >= MAX_RECENT_ACTIONS {
                break;
            }
            if item["event_id"] == authoritative_event["event_id"] {
                continue;
            }
            let mut updated = item.clone();
            updated["authoritative"] = Value::Bool(false);
            if updated["event_kind"].as_str() == Some("continuity_handoff") {
                updated["execution_state"] = Value::String("superseded".to_string());
            }
            recent_actions.push(updated);
        }
    }
    let action_state_counts = collect_action_state_counts(&recent_actions);
    let lineage_supporting_event_ids = recent_actions
        .iter()
        .filter_map(|item| item["event_id"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let authoritative_event_id = authoritative_event["event_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let lineage_nodes = recent_actions
        .iter()
        .filter_map(|item| {
            let event_id = item["event_id"].as_str()?;
            Some(json!({
                "node_id": event_id,
                "event_kind": item["event_kind"],
                "source_kind": item["source_kind"],
                "headline": item["headline"],
                "execution_state": item["execution_state"],
                "authoritative": item["authoritative"],
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"],
            }))
        })
        .collect::<Vec<_>>();
    let lineage_edges = recent_actions
        .iter()
        .filter_map(|item| {
            let event_id = item["event_id"].as_str()?;
            if event_id == authoritative_event_id {
                return None;
            }
            Some(json!({
                "from_event_id": event_id,
                "to_event_id": authoritative_event_id,
                "relation": lineage_relation(item["execution_state"].as_str().unwrap_or("unknown")),
            }))
        })
        .collect::<Vec<_>>();

    let project_task_tree = build_project_task_tree(
        &project_value,
        &namespace_value,
        authoritative_event,
        &current_goal,
        &next_step,
        &pending_return_queue,
    );
    let project_task_tree_summary = summarize_project_task_tree(&project_task_tree);
    let project_task_ledger = build_durable_project_task_ledger(
        &project_value,
        &namespace_value,
        &durable_entries,
        &authoritative_event_id,
        &pending_return_queue,
    );
    let project_task_ledger_summary = summarize_project_task_ledger(&project_task_ledger);
    let client_budget_guard = default_client_budget_guard();
    let execctl_resume_contract = build_execctl_resume_contract(
        &project_task_tree,
        &pending_return_queue,
        &required_task_set,
    );
    let execctl_resume_contract_summary =
        summarize_execctl_resume_contract(&execctl_resume_contract);
    let startup_next_action = build_startup_next_action(
        &current_goal,
        &next_step,
        &execctl_resume_contract,
        &client_budget_guard,
        Some(project.code.as_str()),
        Some(namespace.code.as_str()),
        Some(project.repo_root.as_str()),
    );
    let startup_next_action_summary = summarize_startup_next_action(&startup_next_action);

    restore.insert("project".to_string(), project_value.clone());
    restore.insert("namespace".to_string(), namespace_value.clone());
    restore.insert("captured_at_epoch_ms".to_string(), json!(now_epoch_ms));
    restore.insert(
        "agent_scope".to_string(),
        authoritative_event["agent_scope"].clone(),
    );
    restore.insert(
        "thread_id".to_string(),
        authoritative_event["thread_id"].clone(),
    );
    restore.insert(
        "session_id".to_string(),
        authoritative_event["session_id"].clone(),
    );
    restore.insert(
        "session_age_ms".to_string(),
        json!(now_epoch_ms.saturating_sub(recorded_at_epoch_ms)),
    );
    restore.insert(
        "events_count".to_string(),
        Value::from(
            restore["events_count"]
                .as_u64()
                .unwrap_or_default()
                .saturating_add(1),
        ),
    );
    restore.insert("current_goal".to_string(), json!(current_goal));
    restore.insert("next_step".to_string(), json!(next_step));
    restore.insert("next_step_state".to_string(), json!("planned"));
    restore.insert(
        "current_hypothesis".to_string(),
        current_hypothesis.map(Value::String).unwrap_or(Value::Null),
    );
    restore.insert(
        "current_focus".to_string(),
        current_focus.map(Value::String).unwrap_or(Value::Null),
    );
    restore.insert("open_questions".to_string(), json!(open_questions));
    restore.insert(
        "rejected_hypotheses".to_string(),
        json!(rejected_hypotheses),
    );
    restore.insert("materialized_notes".to_string(), json!(materialized_notes));
    restore.insert("required_task_set".to_string(), json!(required_task_set));
    restore.insert(
        "required_task_set_summary".to_string(),
        required_task_set_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert("active_files".to_string(), json!(active_files));
    restore.insert("visible_projects".to_string(), json!(visible_projects));
    restore.insert("recent_queries".to_string(), json!(recent_queries));
    restore.insert("recent_actions".to_string(), json!(recent_actions));
    restore.insert(
        "pending_return_queue".to_string(),
        pending_return_queue.clone(),
    );
    restore.insert(
        "pending_return_summary".to_string(),
        pending_return_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert(
        "execctl_resume_state".to_string(),
        if pending_return_queue
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            json!("pending_return_queue_present")
        } else {
            json!("clear")
        },
    );
    restore.insert(
        "execctl_resume_contract".to_string(),
        execctl_resume_contract.clone(),
    );
    restore.insert(
        "execctl_resume_contract_summary".to_string(),
        execctl_resume_contract_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert(
        "required_return_task".to_string(),
        execctl_resume_contract["required_return_task"].clone(),
    );
    restore.insert(
        "client_budget_guard".to_string(),
        client_budget_guard.clone(),
    );
    restore.insert("startup_next_action".to_string(), startup_next_action);
    restore.insert(
        "startup_next_action_summary".to_string(),
        startup_next_action_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert("project_task_tree".to_string(), project_task_tree);
    restore.insert(
        "project_task_tree_summary".to_string(),
        project_task_tree_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert("project_task_ledger".to_string(), project_task_ledger);
    restore.insert(
        "project_task_ledger_summary".to_string(),
        project_task_ledger_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert(
        "last_command".to_string(),
        last_command.map(Value::String).unwrap_or(Value::Null),
    );
    restore.insert(
        "last_results_summary".to_string(),
        last_results_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert(
        "source_summary".to_string(),
        source_summary.map(Value::String).unwrap_or(Value::Null),
    );
    restore.insert("latest_decision_trace".to_string(), Value::Null);
    restore.insert(
        "included_reasons_summary".to_string(),
        included_reasons_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert(
        "excluded_reasons_summary".to_string(),
        excluded_reasons_summary
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    restore.insert("action_state_counts".to_string(), action_state_counts);
    restore.insert(
        "restore_confidence".to_string(),
        restore
            .get("restore_confidence")
            .cloned()
            .unwrap_or_else(|| json!("high")),
    );
    restore.insert("restore_freshness_state".to_string(), json!("fresh"));
    restore.insert(
        "state_lineage".to_string(),
        json!({
            "lineage_model_version": restore["execution_catalog"]["lineage_model_version"].clone(),
            "authoritative_event_id": authoritative_event["event_id"],
            "authoritative_event_kind": authoritative_event["event_kind"],
            "authoritative_source_kind": authoritative_event["source_kind"],
            "authoritative_local_path": authoritative_event["local_path"],
            "source_snapshot_id": source_snapshot_id.to_string(),
            "supporting_event_ids": lineage_supporting_event_ids,
            "truth_ranking": restore["execution_catalog"]["truth_ranking"].clone(),
            "nodes": lineage_nodes,
            "edges": lineage_edges,
        }),
    );
    restore.insert("is_preliminary".to_string(), json!(false));

    let overlay_lease_started = Instant::now();
    overlay_execctl_active_lease(&mut bundle, active_lease.as_ref());
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.overlay_active_lease",
        overlay_lease_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let overlay_execution_card_started = Instant::now();
    overlay_restore_execution_card(db, project, namespace, &mut bundle).await?;
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.overlay_execution_card",
        overlay_execution_card_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let overlay_restore_pack_started = Instant::now();
    overlay_workspace_restore_pack(&mut bundle);
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.overlay_workspace_restore_pack",
        overlay_restore_pack_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );

    let payload = persisted_restore_snapshot_payload(&bundle);
    let insert_snapshot_started = Instant::now();
    let snapshot_id =
        postgres::insert_observability_snapshot(db, WORKING_STATE_RESTORE_KIND, &payload).await?;
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.insert_snapshot",
        insert_snapshot_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    let materialize_started = Instant::now();
    materialize_restore_pack(db, project, namespace, &bundle, snapshot_id).await?;
    continuity_profile_log(
        "force_refresh_restore_snapshot_from_previous_handoff.materialize_restore_pack",
        materialize_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project.code, namespace.code),
    );
    Ok(Some(bundle))
}

pub(crate) async fn refresh_same_thread_execctl_active_lease_for_startup(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    restore: Option<&Value>,
) -> Result<Option<Value>> {
    let Some(restore) = restore else {
        return Ok(None);
    };
    let current_thread_id = resolve_thread_id_for_project_repo_root(&project.repo_root, None);
    let Some(candidate) =
        execctl_startup_same_thread_lease_refresh_candidate(restore, current_thread_id.as_deref())
    else {
        return Ok(Some(restore.clone()));
    };
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let live_active_lease = postgres::get_execctl_task_lease(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        now_epoch_ms()? as i64,
    )
    .await?;
    let now_epoch_ms = now_epoch_ms()?;
    if !execctl_same_thread_lease_refresh_required(
        live_active_lease.as_ref(),
        current_thread_id.as_deref(),
        now_epoch_ms,
        0,
    ) {
        return Ok(Some(restore.clone()));
    }
    let recorded_at_epoch_ms = now_epoch_ms;
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    let lease_expires_at_epoch_ms =
        recorded_at_epoch_ms.saturating_add(EXECCTL_LEASE_TTL_MS) as i64;
    postgres::upsert_execctl_task_lease(
        db,
        &postgres::ExecCtlTaskLeaseInsert {
            project_id: project.project_id,
            namespace_id: namespace.namespace_id,
            agent_scope: &agent_scope,
            owner_session_id: Some(session_id.as_str()),
            owner_thread_id: current_thread_id.as_deref(),
            source_snapshot_id: None,
            source_event_id: candidate.source_event_id.as_str(),
            source_kind: candidate.source_kind.as_str(),
            lease_state: "active",
            headline: candidate.headline.as_str(),
            next_step: candidate.next_step.as_str(),
            local_path: candidate.local_path.as_deref(),
            acquired_at_epoch_ms: recorded_at_epoch_ms as i64,
            heartbeat_at_epoch_ms: recorded_at_epoch_ms as i64,
            expires_at_epoch_ms: lease_expires_at_epoch_ms,
        },
    )
    .await?;
    let refreshed_lease = postgres::get_execctl_task_lease(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        now_epoch_ms as i64,
    )
    .await?;
    let mut refreshed_bundle = restore.clone();
    overlay_execctl_active_lease(&mut refreshed_bundle, refreshed_lease.as_ref());
    overlay_workspace_restore_pack(&mut refreshed_bundle);
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(db, Some(&refreshed_bundle))
            .await
            .unwrap_or_else(|error| fallback_client_budget_guard_from_error(&error.to_string()));
    overlay_client_budget_guard(&mut refreshed_bundle, &client_budget_guard);
    overlay_workspace_restore_pack(&mut refreshed_bundle);
    Ok(Some(refreshed_bundle))
}

pub(crate) async fn maintain_same_thread_execctl_active_lease_for_guard(
    db: &Client,
    restore: Option<&Value>,
    explicit_thread_id_hint: Option<&str>,
) -> Result<()> {
    let Some(restore) = restore else {
        return Ok(());
    };
    let project_code = restore["working_state_restore"]["project"]["code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let namespace_code = restore["working_state_restore"]["namespace"]["code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(project_code) = project_code else {
        return Ok(());
    };
    let Some(namespace_code) = namespace_code else {
        return Ok(());
    };
    let project = postgres::get_project_by_code(db, project_code).await?;
    let Some(namespace) =
        postgres::find_namespace_by_code(db, project.project_id, namespace_code).await?
    else {
        return Ok(());
    };
    let current_thread_id =
        resolve_thread_id_for_project_repo_root(&project.repo_root, explicit_thread_id_hint);
    let Some(candidate) =
        execctl_startup_same_thread_lease_refresh_candidate(restore, current_thread_id.as_deref())
    else {
        return Ok(());
    };
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let now_epoch_ms = now_epoch_ms()?;
    let live_active_lease = postgres::get_execctl_task_lease(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        now_epoch_ms as i64,
    )
    .await?;
    if !execctl_same_thread_lease_refresh_required(
        live_active_lease.as_ref(),
        current_thread_id.as_deref(),
        now_epoch_ms,
        EXECCTL_LEASE_HEARTBEAT_MIN_INTERVAL_MS,
    ) {
        return Ok(());
    }
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        now_epoch_ms,
    )
    .await?;
    let lease_expires_at_epoch_ms = now_epoch_ms.saturating_add(EXECCTL_LEASE_TTL_MS) as i64;
    postgres::upsert_execctl_task_lease(
        db,
        &postgres::ExecCtlTaskLeaseInsert {
            project_id: project.project_id,
            namespace_id: namespace.namespace_id,
            agent_scope: &agent_scope,
            owner_session_id: Some(session_id.as_str()),
            owner_thread_id: current_thread_id.as_deref(),
            source_snapshot_id: None,
            source_event_id: candidate.source_event_id.as_str(),
            source_kind: candidate.source_kind.as_str(),
            lease_state: "active",
            headline: candidate.headline.as_str(),
            next_step: candidate.next_step.as_str(),
            local_path: candidate.local_path.as_deref(),
            acquired_at_epoch_ms: now_epoch_ms as i64,
            heartbeat_at_epoch_ms: now_epoch_ms as i64,
            expires_at_epoch_ms: lease_expires_at_epoch_ms,
        },
    )
    .await?;
    let _ = force_refresh_restore_snapshot(db, &project, &namespace).await?;
    Ok(())
}

async fn load_selected_working_state_events(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        WORKING_STATE_EVENT_KIND,
        &project.code,
        &namespace.code,
        Some(MAX_RESTORE_EVENTS),
    )
    .await?;
    Ok(select_relevant_events(
        snapshots,
        &project.code,
        &namespace.code,
        &agent_scope,
    ))
}

fn snapshot_payload_captured_at_epoch_ms(
    snapshot: &ObservabilitySnapshotRecord,
    payload_root: &str,
) -> u64 {
    snapshot.payload[payload_root]["captured_at_epoch_ms"]
        .as_u64()
        .or_else(|| snapshot.payload[payload_root]["recorded_at_epoch_ms"].as_u64())
        .unwrap_or(snapshot.created_at_epoch_ms.max(0) as u64)
}

fn inferred_restore_thread_id(events: &[ObservabilitySnapshotRecord]) -> Option<String> {
    let mut inferred = None::<String>;
    for snapshot in events.iter().take(MAX_RECENT_ACTIONS) {
        let thread_id = snapshot.payload["working_state_event"]["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(thread_id) = thread_id else {
            continue;
        };
        if let Some(existing) = inferred.as_deref() {
            if existing != thread_id {
                return None;
            }
        } else {
            inferred = Some(thread_id.to_string());
        }
    }
    inferred
}

fn restore_pack_source_event_ids(restore: &Value) -> Value {
    let mut ids = BTreeSet::new();
    if let Some(value) = restore["state_lineage"]["authoritative_event_id"].as_str() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            ids.insert(trimmed.to_string());
        }
    }
    if let Some(actions) = restore["recent_actions"].as_array() {
        for action in actions {
            if let Some(value) = action["event_id"].as_str() {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    ids.insert(trimmed.to_string());
                }
            }
        }
    }
    Value::Array(ids.into_iter().map(Value::String).collect())
}

fn restore_pack_artifact_refs(restore: &Value) -> Value {
    let mut refs = BTreeSet::new();
    if let Some(value) = restore["state_lineage"]["authoritative_local_path"].as_str() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            refs.insert(format!("file://{trimmed}"));
        }
    }
    if let Some(actions) = restore["recent_actions"].as_array() {
        for action in actions {
            if let Some(value) = action["local_path"].as_str() {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    refs.insert(format!("file://{trimmed}"));
                }
            }
        }
    }
    if let Some(files) = restore["active_files"].as_array() {
        for file in files {
            if let Some(value) = file.as_str() {
                let trimmed = value.trim();
                if !trimmed.is_empty() && trimmed.starts_with('/') {
                    refs.insert(format!("file://{trimmed}"));
                }
            }
        }
    }
    Value::Array(refs.into_iter().map(Value::String).collect())
}

fn restore_pack_message_refs(restore: &Value) -> Value {
    let mut refs = BTreeSet::new();
    if let Some(value) = restore["thread_id"].as_str() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            refs.insert(format!("thread:{trimmed}"));
        }
    }
    Value::Array(refs.into_iter().map(Value::String).collect())
}

async fn materialize_restore_pack(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    bundle: &Value,
    source_snapshot_id: Uuid,
) -> Result<()> {
    let restore = &bundle["working_state_restore"];
    let workspace_restore_pack = bundle
        .get("workspace_restore_pack")
        .cloned()
        .unwrap_or_else(|| build_workspace_restore_pack(restore));
    let source_event_ids = restore_pack_source_event_ids(restore);
    let artifact_refs = restore_pack_artifact_refs(restore);
    let message_refs = restore_pack_message_refs(restore);
    let evidence_span = json!({
        "kind": "working_state_restore",
        "authoritative_event_id": restore["state_lineage"]["authoritative_event_id"].clone(),
        "authoritative_event_kind": restore["state_lineage"]["authoritative_event_kind"].clone(),
        "restore_confidence": restore["restore_confidence"].clone(),
        "restore_freshness_state": restore["restore_freshness_state"].clone(),
        "recent_actions_count": restore["recent_actions"].as_array().map(|items| items.len()).unwrap_or(0),
        "pending_return_count": restore["pending_return_queue"].as_array().map(|items| items.len()).unwrap_or(0),
        "source_snapshot_id": source_snapshot_id,
    });
    let headline = workspace_restore_pack["current_goal"]
        .as_str()
        .filter(|value| !value.trim().is_empty());
    let summary = workspace_restore_pack["summary"]
        .as_str()
        .filter(|value| !value.trim().is_empty());
    let agent_scope = restore["agent_scope"]
        .as_str()
        .filter(|value| !value.trim().is_empty());
    let session_id = restore["session_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty());
    let thread_id = restore["thread_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty());
    let captured_at_epoch_ms = restore["captured_at_epoch_ms"].as_i64();
    let payload = workspace_restore_pack.clone();
    let _ = postgres::create_restore_pack(
        db,
        &project.code,
        &namespace.code,
        &postgres::RestorePackInsert {
            agent_scope,
            session_id,
            thread_id,
            source_snapshot_id: Some(source_snapshot_id),
            source_snapshot_hint: Some(postgres::RestorePackSourceSnapshotHint {
                snapshot_kind: WORKING_STATE_RESTORE_KIND,
                scope_project_code: Some(project.code.as_str()),
                scope_namespace_code: Some(namespace.code.as_str()),
                verified_exists: true,
            }),
            pack_kind: "workspace_restore_pack",
            source_kind: Some("working_state_restore_runtime"),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("summary"),
            schema_version: Some(WORKSPACE_RESTORE_PACK_ENVELOPE_VERSION),
            headline,
            summary,
            payload: &payload,
            captured_at_epoch_ms,
        },
    )
    .await?;
    Ok(())
}

fn restore_snapshot_matches_latest_relevant_events(
    snapshot_payload: &Value,
    events: &[ObservabilitySnapshotRecord],
) -> bool {
    if events.is_empty() {
        return false;
    }
    let restore = &snapshot_payload["working_state_restore"];
    let latest_event = &events[0].payload["working_state_event"];
    let latest_event_id = latest_event["event_id"].as_str().unwrap_or_default();
    let restore_latest_event_id = restore["recent_actions"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["event_id"].as_str())
        .unwrap_or_default();
    if !latest_event_id.is_empty() && latest_event_id != restore_latest_event_id {
        return false;
    }
    let authoritative_event = events
        .iter()
        .map(|snapshot| &snapshot.payload["working_state_event"])
        .find(|event| {
            event["event_kind"].as_str() == Some("continuity_handoff")
                && !is_meta_continuity_event(event)
        })
        .unwrap_or(latest_event);
    let authoritative_event_id = authoritative_event["event_id"].as_str().unwrap_or_default();
    let restore_authoritative_event_id = restore["state_lineage"]["authoritative_event_id"]
        .as_str()
        .unwrap_or_default();
    if !authoritative_event_id.is_empty()
        && authoritative_event_id != restore_authoritative_event_id
    {
        return false;
    }
    let latest_agent_scope = latest_event["agent_scope"].as_str().unwrap_or("shared");
    let restore_agent_scope = restore["agent_scope"].as_str().unwrap_or("shared");
    if !latest_agent_scope.is_empty() && latest_agent_scope != restore_agent_scope {
        return false;
    }
    let latest_session_id = latest_event["session_id"].as_str().unwrap_or_default();
    let restore_session_id = restore["session_id"].as_str().unwrap_or_default();
    if !latest_session_id.is_empty() && latest_session_id != restore_session_id {
        return false;
    }

    let mut active_files = Vec::new();
    let mut recent_queries = Vec::new();
    collect_active_files(&mut active_files, &latest_event["active_files"]);
    if let Some(query) = latest_event["query"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        recent_queries.push(query.to_string());
    }
    let expected_focus = derive_restore_current_focus(
        &active_files,
        &recent_queries,
        restore["current_hypothesis"].as_str(),
    );
    if let Some(expected_focus) = expected_focus {
        let restore_focus = restore["current_focus"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if restore_focus != Some(expected_focus.as_str()) {
            return false;
        }
    }
    let expected_source_summary =
        derive_restore_source_summary(authoritative_event, &active_files, None);
    if let Some(expected_source_summary) = expected_source_summary {
        let restore_source_summary = restore["source_summary"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if restore_source_summary != Some(expected_source_summary.as_str()) {
            return false;
        }
    }
    true
}

fn compose_restore_bundle(
    project: &Value,
    namespace: &Value,
    events: &[ObservabilitySnapshotRecord],
) -> Value {
    let latest_event = events
        .iter()
        .map(|snapshot| &snapshot.payload["working_state_event"])
        .find(|event| !is_meta_continuity_event(event))
        .unwrap_or(&events[0].payload["working_state_event"]);
    let latest = latest_event;
    let authoritative_event = events
        .iter()
        .map(|snapshot| &snapshot.payload["working_state_event"])
        .find(|event| {
            event["event_kind"].as_str() == Some("continuity_handoff")
                && !is_meta_continuity_event(event)
        })
        .unwrap_or(latest_event);
    let authoritative_event_id = authoritative_event["event_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let session_id = latest["session_id"].as_str().unwrap_or_default();
    let thread_id = latest["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| inferred_restore_thread_id(events));
    let latest_recorded_at = latest["recorded_at_epoch_ms"]
        .as_u64()
        .unwrap_or(events[0].created_at_epoch_ms.max(0) as u64);
    let mut current_goal = latest["headline"]
        .as_str()
        .unwrap_or("ещё нет данных")
        .to_string();
    let mut next_step = latest["next_step_hint"]
        .as_str()
        .unwrap_or("ещё нет данных")
        .to_string();
    if authoritative_event["event_kind"].as_str() == Some("continuity_handoff") {
        let handoff = authoritative_event;
        if let Some(value) = handoff["headline"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            current_goal = value.to_string();
        }
        if let Some(value) = handoff["next_step_hint"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            next_step = value.to_string();
        }
    }

    let mut active_files = Vec::new();
    let mut visible_projects = Vec::new();
    let mut recent_queries = Vec::new();
    let mut open_questions = Vec::new();
    let mut rejected_hypotheses = Vec::new();
    let mut materialized_notes = Vec::new();
    let mut current_hypothesis = None::<String>;
    let mut last_command = None::<String>;
    let mut last_results_summary = None::<String>;
    let mut recent_actions = Vec::new();
    let mut recent_decision_traces = Vec::new();
    let mut workspace_graph_inputs = Vec::new();
    let now_epoch_ms = now_epoch_ms().unwrap_or(latest_recorded_at);

    for snapshot in events.iter().take(MAX_RECENT_ACTIONS) {
        let event = &snapshot.payload["working_state_event"];
        if is_meta_continuity_event(event) {
            continue;
        }
        collect_active_files(&mut active_files, &event["active_files"]);
        collect_unique_strings(&mut visible_projects, &event["visible_projects"]);
        if let Some(query) = event["query"].as_str().filter(|value| !value.is_empty()) {
            push_unique(&mut recent_queries, query.to_string());
        }
        collect_open_questions(&mut open_questions, &event["open_questions"]);
        collect_unique_strings(&mut rejected_hypotheses, &event["rejected_hypotheses"]);
        collect_materialized_notes(&mut materialized_notes, &event["materialized_notes"]);
        if current_hypothesis.is_none() {
            current_hypothesis = event["current_hypothesis"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
        }
        if last_command.is_none() {
            last_command = event["last_command"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
        }
        if last_results_summary.is_none() {
            last_results_summary = event["last_results_summary"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
        }
        if recent_decision_traces.len() < MAX_RECENT_DECISION_TRACES
            && let Some(trace) = summarize_decision_trace(event)
        {
            recent_decision_traces.push(trace);
        }
        let action_state = classify_action_state(
            event,
            &authoritative_event_id,
            latest_recorded_at,
            now_epoch_ms,
        );
        recent_actions.push(json!({
            "event_id": event["event_id"],
            "event_kind": event["event_kind"],
            "source_kind": event["source_kind"],
            "headline": event["headline"],
            "summary": event["summary"],
            "recorded_at_epoch_ms": event["recorded_at_epoch_ms"],
            "local_path": event["local_path"],
            "host_current_thread_control_feedback": if event["host_current_thread_control_feedback"].is_object() {
                event["host_current_thread_control_feedback"].clone()
            } else {
                Value::Null
            },
            "execution_state": action_state,
            "authoritative": event["event_id"].as_str() == Some(authoritative_event_id.as_str()),
        }));
        if !event["workspace_graph"].is_null() {
            workspace_graph_inputs.push(event["workspace_graph"].clone());
        }
    }
    let required_task_set = extract_required_task_set_from_event(authoritative_event);
    let required_task_set_summary = summarize_required_task_set(&required_task_set);

    let restore_confidence = if events.len() >= 4 && latest_recorded_at > 0 {
        if now_epoch_ms - latest_recorded_at <= 15 * 60 * 1000 {
            "high"
        } else {
            "medium"
        }
    } else {
        "preliminary"
    };
    let execution_catalog = retrieval_science::execution_state_catalog_json().unwrap_or_else(|_| {
        json!({
            "execution_state_model_version": "execution-state-v1",
            "lineage_model_version": "lineage-v2",
            "states": ["planned", "attempted", "succeeded", "superseded", "stale"],
            "truth_ranking": ["continuity_handoff", "working_state_restore", "live_context_pack"]
        })
    });
    let lineage_supporting_event_ids = recent_actions
        .iter()
        .filter_map(|item| item["event_id"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let lineage_nodes = recent_actions
        .iter()
        .filter_map(|item| {
            let event_id = item["event_id"].as_str()?;
            Some(json!({
                "node_id": event_id,
                "event_kind": item["event_kind"],
                "source_kind": item["source_kind"],
                "headline": item["headline"],
                "execution_state": item["execution_state"],
                "authoritative": item["authoritative"],
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"],
            }))
        })
        .collect::<Vec<_>>();
    let lineage_edges = recent_actions
        .iter()
        .filter_map(|item| {
            let event_id = item["event_id"].as_str()?;
            if event_id == authoritative_event_id {
                return None;
            }
            Some(json!({
                "from_event_id": event_id,
                "to_event_id": authoritative_event_id,
                "relation": lineage_relation(item["execution_state"].as_str().unwrap_or("unknown")),
            }))
        })
        .collect::<Vec<_>>();
    let action_state_counts = collect_action_state_counts(&recent_actions);
    let pending_return_queue = extract_pending_return_queue(
        authoritative_event,
        latest_recorded_at,
        &current_goal,
        &next_step,
    );
    let client_budget_target_percent = events
        .iter()
        .find_map(|snapshot| {
            snapshot.payload["working_state_event"]["client_budget_target_percent"]
                .as_u64()
                .and_then(normalize_client_budget_target_percent)
        })
        .unwrap_or(DEFAULT_CLIENT_BUDGET_TARGET_PERCENT);
    let pending_return_summary = summarize_pending_return_queue(&pending_return_queue);
    let has_pending_return_queue = pending_return_queue
        .as_array()
        .is_some_and(|items| !items.is_empty());
    let execctl_resume_state = if has_pending_return_queue {
        "pending_return_queue_present"
    } else {
        "clear"
    };
    let restore_freshness_state =
        if now_epoch_ms.saturating_sub(latest_recorded_at) > 15 * 60 * 1000 {
            "stale"
        } else {
            "fresh"
        };
    let merged_workspace_graph = workspace_graph::merge_workspace_graphs(&workspace_graph_inputs);
    let latest_decision_trace = recent_decision_traces
        .first()
        .cloned()
        .unwrap_or(Value::Null);
    let included_reasons_summary = decision_trace_summary(Some(&latest_decision_trace), "included");
    let excluded_reasons_summary =
        decision_trace_summary(Some(&latest_decision_trace), "not_included");
    let current_focus = derive_restore_current_focus(
        &active_files,
        &recent_queries,
        current_hypothesis.as_deref(),
    );
    let source_summary = derive_restore_source_summary(
        authoritative_event,
        &active_files,
        included_reasons_summary.as_deref(),
    );
    let project_task_tree = build_project_task_tree(
        project,
        namespace,
        authoritative_event,
        &current_goal,
        &next_step,
        &pending_return_queue,
    );
    let project_task_tree_summary = summarize_project_task_tree(&project_task_tree);
    let project_task_ledger = build_project_task_ledger(
        project,
        namespace,
        events,
        &authoritative_event_id,
        &pending_return_queue,
    );
    let project_task_ledger_summary = summarize_project_task_ledger(&project_task_ledger);
    let execctl_resume_contract = build_execctl_resume_contract(
        &project_task_tree,
        &pending_return_queue,
        &required_task_set,
    );
    let execctl_resume_contract_summary =
        summarize_execctl_resume_contract(&execctl_resume_contract);
    let client_budget_guard = default_client_budget_guard();
    let startup_next_action = build_startup_next_action(
        &current_goal,
        &next_step,
        &execctl_resume_contract,
        &client_budget_guard,
        project["code"].as_str(),
        namespace["code"].as_str(),
        project["repo_root"].as_str(),
    );
    let startup_next_action_summary = summarize_startup_next_action(&startup_next_action);

    json!({
        "working_state_restore": {
            "project": project,
            "namespace": namespace,
            "captured_at_epoch_ms": now_epoch_ms,
            "agent_scope": latest["agent_scope"].as_str().unwrap_or("shared"),
            "thread_id": thread_id,
            "session_id": session_id,
            "session_age_ms": now_epoch_ms.saturating_sub(latest_recorded_at),
            "events_count": events.len(),
            "current_goal": current_goal,
            "next_step": next_step,
            "next_step_state": "planned",
            "current_hypothesis": current_hypothesis,
            "current_focus": current_focus,
            "open_questions": open_questions,
            "rejected_hypotheses": rejected_hypotheses,
            "materialized_notes": materialized_notes,
            "required_task_set": required_task_set,
            "required_task_set_summary": required_task_set_summary,
            "active_files": active_files,
            "visible_projects": visible_projects,
            "recent_queries": recent_queries,
            "recent_actions": recent_actions,
            "pending_return_queue": pending_return_queue,
            "pending_return_summary": pending_return_summary,
            "client_budget_target_percent": client_budget_target_percent,
            "execctl_resume_state": execctl_resume_state,
            "execctl_resume_contract": execctl_resume_contract,
            "execctl_resume_contract_summary": execctl_resume_contract_summary,
            "required_return_task": execctl_resume_contract["required_return_task"].clone(),
            "client_budget_guard": client_budget_guard,
            "startup_next_action": startup_next_action,
            "startup_next_action_summary": startup_next_action_summary,
            "project_task_tree": project_task_tree,
            "project_task_tree_summary": project_task_tree_summary,
            "project_task_ledger": project_task_ledger,
            "project_task_ledger_summary": project_task_ledger_summary,
            "last_command": last_command,
            "last_results_summary": last_results_summary,
            "source_summary": source_summary,
            "latest_decision_trace": latest_decision_trace,
            "included_reasons_summary": included_reasons_summary,
            "excluded_reasons_summary": excluded_reasons_summary,
            "recent_decision_traces": recent_decision_traces,
            "restore_confidence": restore_confidence,
            "restore_freshness_state": restore_freshness_state,
            "execution_catalog": execution_catalog,
            "action_state_counts": action_state_counts,
            "workspace_graph": merged_workspace_graph,
            "state_lineage": {
                "lineage_model_version": execution_catalog["lineage_model_version"].clone(),
                "authoritative_event_id": authoritative_event["event_id"],
                "authoritative_event_kind": authoritative_event["event_kind"],
                "authoritative_source_kind": authoritative_event["source_kind"],
                "authoritative_local_path": authoritative_event["local_path"],
                "supporting_event_ids": lineage_supporting_event_ids,
                "truth_ranking": execution_catalog["truth_ranking"].clone(),
                "nodes": lineage_nodes,
                "edges": lineage_edges,
            },
            "is_preliminary": events.len() < 3,
        }
    })
}

fn derive_restore_current_focus(
    active_files: &[String],
    recent_queries: &[String],
    current_hypothesis: Option<&str>,
) -> Option<String> {
    let primary_file = active_files
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty() && *value != "ещё нет данных");
    let recent_query = recent_queries
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty() && *value != "ещё нет данных");

    match (primary_file, recent_query) {
        (Some(path), Some(query)) => Some(format!(
            "Сейчас основной фокус: {path} по запросу «{query}»."
        )),
        (Some(path), None) => Some(format!("Сейчас основной фокус: {path}.")),
        (None, Some(query)) => Some(format!("Сейчас основной фокус: запрос «{query}».")),
        (None, None) => current_hypothesis
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "ещё нет данных")
            .map(ToOwned::to_owned),
    }
}

fn derive_restore_source_summary(
    authoritative_event: &Value,
    active_files: &[String],
    included_reasons_summary: Option<&str>,
) -> Option<String> {
    let source_kind = authoritative_event["source_kind"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "ещё нет данных")?;
    let local_path = authoritative_event["local_path"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "ещё нет данных");
    let primary_file = active_files
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty() && *value != "ещё нет данных");
    let retrieval_summary = included_reasons_summary
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut parts = Vec::new();
    match local_path {
        Some(path) => parts.push(format!("Источник истины: {source_kind} ({path}).")),
        None => parts.push(format!("Источник истины: {source_kind}.")),
    }
    if let Some(path) = primary_file {
        parts.push(format!("Главный подтверждающий артефакт: {path}."));
    }
    if let Some(summary) = retrieval_summary {
        parts.push(format!("Почему это вошло: {summary}"));
    }
    Some(parts.join(" "))
}

pub fn degradation_proof_scenarios(captured_at_epoch_ms: u64) -> Result<Vec<Value>> {
    let base = captured_at_epoch_ms as i64;
    let exact = synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
        project_code: "art",
        namespace_code: "continuity",
        agent_scope: "art::primary",
        session_id: "session-a",
        event_kind: "retrieval_context_pack",
        headline: "exact-scope",
        next_step_hint: "",
        summary: "",
        offset: base,
    });
    let foreign = synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
        project_code: "art",
        namespace_code: "continuity",
        agent_scope: "art::secondary",
        session_id: "session-b",
        event_kind: "retrieval_context_pack",
        headline: "foreign-scope",
        next_step_hint: "",
        summary: "",
        offset: base - 1,
    });
    let exact_selected = select_relevant_events(
        vec![exact.clone(), foreign.clone()],
        "art",
        "continuity",
        "art::primary",
    );
    let foreign_only_selected =
        select_relevant_events(vec![foreign], "art", "continuity", "art::primary");
    let cross_agent_pass = exact_selected.len() == 1
        && exact_selected[0].payload["working_state_event"]["headline"] == json!("exact-scope")
        && foreign_only_selected.is_empty();

    let corrupt_project_selected = select_relevant_events(
        vec![synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art-corrupt",
            namespace_code: "continuity",
            agent_scope: "art::primary",
            session_id: "session-corrupt-project",
            event_kind: "retrieval_context_pack",
            headline: "corrupt-project",
            next_step_hint: "",
            summary: "",
            offset: base - 4,
        })],
        "art",
        "continuity",
        "art::primary",
    );
    let corrupt_namespace_selected = select_relevant_events(
        vec![synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity-corrupt",
            agent_scope: "art::primary",
            session_id: "session-corrupt-namespace",
            event_kind: "retrieval_context_pack",
            headline: "corrupt-namespace",
            next_step_hint: "",
            summary: "",
            offset: base - 5,
        })],
        "art",
        "continuity",
        "art::primary",
    );
    let corrupt_scope_selected = select_relevant_events(
        vec![synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::pr1mary?",
            session_id: "session-corrupt-scope",
            event_kind: "retrieval_context_pack",
            headline: "corrupt-scope",
            next_step_hint: "",
            summary: "",
            offset: base - 6,
        })],
        "art",
        "continuity",
        "art::primary",
    );
    let corrupt_scope_metadata_pass = corrupt_project_selected.is_empty()
        && corrupt_namespace_selected.is_empty()
        && corrupt_scope_selected.is_empty();

    let partial_refresh_events = vec![
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-partial-refresh",
            event_kind: "continuity_handoff",
            headline: "Partial refresh handoff",
            next_step_hint: "Finish continuity refresh.",
            summary: "Only the newest handoff landed so far.",
            offset: base - 60_000,
        }),
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-partial-refresh",
            event_kind: "retrieval_context_pack",
            headline: "Partial refresh retrieval",
            next_step_hint: "Inspect refresh gap.",
            summary: "Only one supporting retrieval event is available.",
            offset: base - 60_001,
        }),
    ];
    let partial_refresh_bundle = compose_restore_bundle(
        &json!({"code":"art"}),
        &json!({"code":"continuity"}),
        &partial_refresh_events,
    );
    let partial_refresh_restore = &partial_refresh_bundle["working_state_restore"];
    let partial_refresh_pass = partial_refresh_restore["restore_confidence"]
        == json!("preliminary")
        && partial_refresh_restore["is_preliminary"] == json!(true)
        && partial_refresh_restore["current_goal"] == json!("Partial refresh handoff")
        && partial_refresh_restore["state_lineage"]["authoritative_event_kind"]
            == json!("continuity_handoff");

    let stale_events = vec![
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-stale",
            event_kind: "continuity_handoff",
            headline: "Stale authoritative handoff",
            next_step_hint: "Refresh continuity.",
            summary: "Old but authoritative handoff.",
            offset: base - 16 * 60 * 1000,
        }),
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-stale",
            event_kind: "continuity_handoff",
            headline: "Older stale handoff",
            next_step_hint: "Do older stale thing.",
            summary: "Older stale handoff.",
            offset: base - 16 * 60 * 1000 - 1,
        }),
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-stale",
            event_kind: "retrieval_context_pack",
            headline: "Stale retrieval",
            next_step_hint: "Inspect stale state.",
            summary: "Stale retrieval.",
            offset: base - 16 * 60 * 1000 - 2,
        }),
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-stale",
            event_kind: "retrieval_context_pack",
            headline: "Older retrieval",
            next_step_hint: "Inspect older state.",
            summary: "Older retrieval.",
            offset: base - 16 * 60 * 1000 - 3,
        }),
    ];
    let stale_bundle = compose_restore_bundle(
        &json!({"code":"art"}),
        &json!({"code":"continuity"}),
        &stale_events,
    );
    let stale_restore = &stale_bundle["working_state_restore"];
    let stale_handoff_pass = stale_restore["restore_freshness_state"] == json!("stale")
        && stale_restore["current_goal"] == json!("Stale authoritative handoff");

    let conflict_events = vec![
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-conflict",
            event_kind: "continuity_handoff",
            headline: "Authoritative handoff",
            next_step_hint: "Ship the next change.",
            summary: "Materialized authoritative result.",
            offset: base,
        }),
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-conflict",
            event_kind: "continuity_handoff",
            headline: "Older handoff",
            next_step_hint: "Do older thing.",
            summary: "Superseded result.",
            offset: base - 1,
        }),
        synthetic_snapshot_with_kind(SyntheticSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-conflict",
            event_kind: "retrieval_context_pack",
            headline: "Рабочий запрос: current context",
            next_step_hint: "Inspect file.",
            summary: "Attempted retrieval.",
            offset: base - 2,
        }),
    ];
    let conflict_bundle = compose_restore_bundle(
        &json!({"code":"art"}),
        &json!({"code":"continuity"}),
        &conflict_events,
    );
    let conflict_restore = &conflict_bundle["working_state_restore"];
    let working_state_conflict_pass = conflict_restore["state_lineage"]["authoritative_event_kind"]
        == json!("continuity_handoff")
        && conflict_restore["action_state_counts"]["succeeded"] == json!(1)
        && conflict_restore["action_state_counts"]["superseded"] == json!(1)
        && conflict_restore["action_state_counts"]["attempted"] == json!(1);

    Ok(vec![
        json!({
            "class_key": "cross_agent_scope",
            "title": "Чужой рабочий контур агента",
            "status": if cross_agent_pass { "pass" } else { "critical" },
            "reason": if cross_agent_pass {
                "select_relevant_events выбирает exact agent_scope и fail-closed отбрасывает чужой scope без shared fallback."
            } else {
                "working-state selection смешал чужой agent_scope или не отфильтровал foreign-only scope."
            },
            "details": {
                "exact_scope_selected_count": exact_selected.len(),
                "foreign_only_selected_count": foreign_only_selected.len(),
            }
        }),
        json!({
            "class_key": "corrupt_scope_metadata",
            "title": "Битые scope-метаданные",
            "status": if corrupt_scope_metadata_pass { "pass" } else { "critical" },
            "reason": if corrupt_scope_metadata_pass {
                "working-state selection держит exact project/namespace/agent scope и fail-closed отбрасывает битые scope-метаданные без nearest-scope угадывания."
            } else {
                "working-state selection принял битые project/namespace/agent scope-метаданные вместо безопасного пустого результата."
            },
            "details": {
                "corrupt_project_selected_count": corrupt_project_selected.len(),
                "corrupt_namespace_selected_count": corrupt_namespace_selected.len(),
                "corrupt_agent_scope_selected_count": corrupt_scope_selected.len(),
            }
        }),
        json!({
            "class_key": "partial_refresh",
            "title": "Неполный refresh",
            "status": if partial_refresh_pass { "pass" } else { "critical" },
            "reason": if partial_refresh_pass {
                "build_restore_bundle не маскирует неполный refresh под свежий: оставляет restore_confidence = preliminary и явный authoritative lineage."
            } else {
                "restore bundle замаскировал неполный refresh под полноценный свежий restore."
            },
            "details": {
                "events_count": partial_refresh_restore["events_count"].clone(),
                "restore_confidence": partial_refresh_restore["restore_confidence"].clone(),
                "is_preliminary": partial_refresh_restore["is_preliminary"].clone(),
                "current_goal": partial_refresh_restore["current_goal"].clone(),
            }
        }),
        json!({
            "class_key": "stale_handoff",
            "title": "Устаревший handoff",
            "status": if stale_handoff_pass { "pass" } else { "critical" },
            "reason": if stale_handoff_pass {
                "compose_restore_bundle честно помечает устаревший handoff как stale и сохраняет authoritative lineage."
            } else {
                "restore bundle не пометил старый handoff как stale."
            },
            "details": {
                "restore_freshness_state": stale_restore["restore_freshness_state"].clone(),
                "current_goal": stale_restore["current_goal"].clone(),
            }
        }),
        json!({
            "class_key": "working_state_conflict",
            "title": "Конфликт рабочего состояния",
            "status": if working_state_conflict_pass { "pass" } else { "critical" },
            "reason": if working_state_conflict_pass {
                "restore bundle не скрывает конфликт: сохраняет authoritative lineage и явные execution states succeeded/superseded/attempted."
            } else {
                "restore bundle потерял lineage или скрыл conflict execution states."
            },
            "details": {
                "action_state_counts": conflict_restore["action_state_counts"].clone(),
                "state_lineage": conflict_restore["state_lineage"].clone(),
            }
        }),
    ])
}

#[cfg(test)]
pub fn degradation_proof_report(captured_at_epoch_ms: u64) -> Result<Value> {
    let scenarios = degradation_proof_scenarios(captured_at_epoch_ms)?;
    Ok(json!({
        "degradation_verification": {
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "scenarios": scenarios,
        }
    }))
}

fn is_meta_continuity_event(event: &Value) -> bool {
    if event["event_kind"].as_str() != Some("continuity_handoff") {
        return false;
    }
    let headline = event["headline"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let next_step = event["next_step_hint"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let summary = event["summary"].as_str().unwrap_or_default().to_lowercase();
    headline.contains("continuity restored")
        || headline.contains("continuity reported")
        || headline.contains("restored and reported for new chat")
        || next_step.contains("ждать указание пользователя")
        || summary.contains("пользователь спросил, на чем остановились")
        || summary.contains("пользователь спросил, на чём остановились")
        || summary.contains("обязательный startup-path")
}

fn select_relevant_events(
    snapshots: Vec<ObservabilitySnapshotRecord>,
    project_code: &str,
    namespace_code: &str,
    agent_scope: &str,
) -> Vec<ObservabilitySnapshotRecord> {
    let project_events = snapshots
        .into_iter()
        .filter(|snapshot| {
            let node = &snapshot.payload["working_state_event"];
            node["project"]["code"].as_str() == Some(project_code)
                && node["namespace"]["code"].as_str() == Some(namespace_code)
        })
        .collect::<Vec<_>>();
    if project_events.is_empty() {
        return Vec::new();
    }

    let exact_scope = project_events.iter().any(|snapshot| {
        snapshot.payload["working_state_event"]["agent_scope"].as_str() == Some(agent_scope)
    });
    let shared_scope = project_events.iter().any(|snapshot| {
        matches!(
            snapshot.payload["working_state_event"]["agent_scope"].as_str(),
            Some("shared") | None | Some("")
        )
    });

    let scoped = if exact_scope {
        project_events
            .into_iter()
            .filter(|snapshot| {
                snapshot.payload["working_state_event"]["agent_scope"].as_str() == Some(agent_scope)
            })
            .collect::<Vec<_>>()
    } else if shared_scope {
        project_events
            .into_iter()
            .filter(|snapshot| {
                matches!(
                    snapshot.payload["working_state_event"]["agent_scope"].as_str(),
                    Some("shared") | None | Some("")
                )
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if scoped.is_empty() {
        return scoped;
    }
    let latest_session_id = scoped[0].payload["working_state_event"]["session_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if latest_session_id.is_empty() {
        return scoped.into_iter().take(1).collect();
    }
    scoped
        .into_iter()
        .filter(|snapshot| {
            snapshot.payload["working_state_event"]["session_id"].as_str()
                == Some(latest_session_id.as_str())
        })
        .collect()
}

fn classify_action_state(
    event: &Value,
    authoritative_event_id: &str,
    latest_recorded_at: u64,
    now_epoch_ms: u64,
) -> &'static str {
    let recorded_at = event["recorded_at_epoch_ms"].as_u64().unwrap_or_default();
    if !authoritative_event_id.is_empty()
        && event["event_id"].as_str() == Some(authoritative_event_id)
        && event["event_kind"].as_str() == Some("continuity_handoff")
    {
        "succeeded"
    } else if event["event_kind"].as_str() == Some("continuity_handoff") {
        "superseded"
    } else if now_epoch_ms.saturating_sub(recorded_at.max(latest_recorded_at)) > 15 * 60 * 1000 {
        "stale"
    } else {
        "attempted"
    }
}

fn collect_action_state_counts(actions: &[Value]) -> Value {
    let mut counts = serde_json::Map::new();
    for action in actions {
        let state = action["execution_state"].as_str().unwrap_or("unknown");
        let next = counts
            .get(state)
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .saturating_add(1);
        counts.insert(state.to_string(), json!(next));
    }
    Value::Object(counts)
}

fn lineage_relation(execution_state: &str) -> &'static str {
    match execution_state {
        "superseded" => "superseded_by",
        "stale" => "stale_support_for",
        _ => "supports",
    }
}

fn summarize_decision_trace(event: &Value) -> Option<Value> {
    let trace = event["decision_trace"].as_object()?;
    let included = trace
        .get("included")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(4)
                .map(|item| {
                    json!({
                        "strategy": item["strategy"],
                        "count": item["count"],
                        "reason": item["reason"],
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let not_included = trace
        .get("not_included")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(4)
                .map(|item| {
                    json!({
                        "strategy": item["strategy"],
                        "reason": item["reason"],
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(json!({
        "event_id": event["event_id"],
        "headline": event["headline"],
        "query": event["query"],
        "recorded_at_epoch_ms": event["recorded_at_epoch_ms"],
        "scope": trace.get("scope").cloned().unwrap_or(Value::Null),
        "selection_priority": trace.get("selection_priority").cloned().unwrap_or(Value::Null),
        "included": included,
        "not_included": not_included,
        "semantic_guard": trace.get("semantic_guard").cloned().unwrap_or(Value::Null),
    }))
}

fn decision_trace_strategy_label(strategy: &str) -> &str {
    match strategy {
        "exact_documents" => "точные совпадения",
        "symbol_hits" => "совпадения по символам",
        "lexical_chunks" => "текстовые фрагменты",
        "semantic_chunks" => "смысловые фрагменты",
        _ => strategy,
    }
}

fn decision_trace_summary(trace: Option<&Value>, key: &str) -> Option<String> {
    let items = trace?.get(key)?.as_array()?;
    let parts = items
        .iter()
        .take(3)
        .filter_map(|item| {
            let reason = item["reason"].as_str()?.trim();
            if reason.is_empty() {
                return None;
            }
            let strategy =
                decision_trace_strategy_label(item["strategy"].as_str().unwrap_or_default());
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
        Some(parts.join(" • "))
    }
}

async fn resolve_session_id(
    db: &Client,
    project_code: &str,
    namespace_code: &str,
    agent_scope: &str,
    recorded_at_epoch_ms: u64,
) -> Result<String> {
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        WORKING_STATE_EVENT_KIND,
        project_code,
        namespace_code,
        Some(60),
    )
    .await?;
    let events = select_relevant_events(snapshots, project_code, namespace_code, agent_scope);
    if let Some(latest) = events.first() {
        let node = &latest.payload["working_state_event"];
        let latest_recorded = node["recorded_at_epoch_ms"]
            .as_u64()
            .unwrap_or(latest.created_at_epoch_ms.max(0) as u64);
        if recorded_at_epoch_ms.saturating_sub(latest_recorded) <= SESSION_GAP_MS
            && let Some(session_id) = node["session_id"]
                .as_str()
                .filter(|value| !value.is_empty())
        {
            return Ok(session_id.to_string());
        }
    }
    Ok(format!(
        "{}::{}::{}",
        project_code, agent_scope, recorded_at_epoch_ms
    ))
}

pub(crate) fn current_agent_scope_for(project_code: &str, namespace_code: &str) -> String {
    for key in [
        "AMAI_AGENT_SCOPE",
        "CODEX_AGENT_SCOPE",
        "AMAI_CLIENT_SCOPE",
        "AMAI_CLIENT_KEY",
    ] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    format!("{project_code}::{namespace_code}::default")
}

struct SyntheticSnapshotSpec<'a> {
    project_code: &'a str,
    namespace_code: &'a str,
    agent_scope: &'a str,
    session_id: &'a str,
    event_kind: &'a str,
    headline: &'a str,
    next_step_hint: &'a str,
    summary: &'a str,
    offset: i64,
}

fn synthetic_snapshot_with_kind(spec: SyntheticSnapshotSpec<'_>) -> ObservabilitySnapshotRecord {
    ObservabilitySnapshotRecord {
        snapshot_id: Uuid::new_v4(),
        snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
        payload: json!({
            "working_state_event": {
                "event_id": format!("{}-{}", spec.headline, spec.offset),
                "project": { "code": spec.project_code },
                "namespace": { "code": spec.namespace_code },
                "agent_scope": spec.agent_scope,
                "session_id": spec.session_id,
                "event_kind": spec.event_kind,
                "source_kind": "synthetic_degradation_proof",
                "headline": spec.headline,
                "next_step_hint": spec.next_step_hint,
                "summary": spec.summary,
                "local_path": "/tmp/degradation-proof",
                "recorded_at_epoch_ms": spec.offset,
            }
        }),
        created_at_epoch_ms: spec.offset,
    }
}

fn current_thread_id() -> Option<String> {
    env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolved_thread_id_with_candidates(
    explicit_thread_id_hint: Option<&str>,
    env_thread_id: Option<&str>,
    repo_preferred_thread_id: Option<&str>,
) -> Option<String> {
    explicit_thread_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            env_thread_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            repo_preferred_thread_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

fn resolve_thread_id_for_project_repo_root(
    repo_root: &str,
    explicit_thread_id_hint: Option<&str>,
) -> Option<String> {
    let env_thread_id = current_thread_id();
    let repo_preferred_thread_id = if repo_root.trim().is_empty() {
        None
    } else {
        codex_threads::preferred_thread_id_for_repo(repo_root)
            .ok()
            .flatten()
    };
    resolved_thread_id_with_candidates(
        explicit_thread_id_hint,
        env_thread_id.as_deref(),
        repo_preferred_thread_id.as_deref(),
    )
}

fn project_json(project: &ProjectRecord) -> Value {
    json!({
        "code": project.code,
        "display_name": project.display_name,
        "repo_root": project.repo_root,
        "visibility_scope": project.visibility_scope,
    })
}

fn namespace_json(namespace: &NamespaceRecord) -> Value {
    json!({
        "code": namespace.code,
        "display_name": namespace.display_name,
        "retrieval_mode": namespace.retrieval_mode,
    })
}

#[derive(Debug, Clone, Serialize)]
struct ProjectSummary {
    code: String,
    display_name: String,
    repo_root: String,
}

#[derive(Debug, Clone, Serialize)]
struct NamespaceSummary {
    code: String,
    display_name: String,
}

fn extract_active_files_from_context_pack(payload: &Value) -> Vec<String> {
    let retrieval = &payload["retrieval"];
    let mut active_files = Vec::new();
    for key in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
    ] {
        if let Some(items) = retrieval[key].as_array() {
            for item in items {
                if let Some(path) = item["relative_path"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                {
                    push_unique(&mut active_files, path.to_string());
                } else if let Some(path) = item["provenance"]["path"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                {
                    push_unique(&mut active_files, path.to_string());
                }
                if active_files.len() >= MAX_ACTIVE_FILES {
                    return active_files;
                }
            }
        }
    }
    if active_files.is_empty() {
        for path in payload["cache_reuse_reference"]["active_files"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(path) = path.as_str().filter(|value| !value.is_empty()) {
                push_unique(&mut active_files, path.to_string());
            }
            if active_files.len() >= MAX_ACTIVE_FILES {
                return active_files;
            }
        }
    }
    active_files
}

fn extract_visible_projects(value: Option<&Value>) -> Vec<String> {
    let mut visible = Vec::new();
    let Some(items) = value.and_then(Value::as_array) else {
        return visible;
    };
    for item in items {
        if let Some(project_code) = item["project_code"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            push_unique(&mut visible, project_code.to_string());
        }
    }
    visible
}

fn derive_retrieval_hypothesis(payload: &Value) -> Option<String> {
    let active_files = extract_active_files_from_context_pack(payload);
    if active_files.is_empty() {
        None
    } else {
        Some(format!(
            "Вероятный рабочий контекст сейчас лежит в: {}",
            active_files
                .into_iter()
                .take(3)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn derive_retrieval_next_step(active_files: &[String], target_kind: &str) -> String {
    if let Some(path) = active_files.first() {
        format!("Откройте {} и продолжайте работу от этого артефакта.", path)
    } else {
        format!(
            "Уточните запрос или задайте follow-up, если текущий {} ещё не дал нужный контекст.",
            target_kind
        )
    }
}

fn derive_pending_return_queue(
    restore_node: Option<&Value>,
    new_headline: &str,
    new_next_step: &str,
    queued_at_epoch_ms: u64,
    resolve_current_goal: bool,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
) -> Vec<Value> {
    let mut queue = restore_node
        .and_then(|node| node["pending_return_queue"].as_array())
        .cloned()
        .unwrap_or_default();
    prune_resolved_pending_return_items(&mut queue, resolved_headlines, resolved_task_ids);
    let Some(node) = restore_node else {
        return queue;
    };
    let previous_goal = node["current_goal"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let previous_next_step = node["next_step"]
        .as_str()
        .map(normalize_next_step_hint)
        .unwrap_or_default();
    let previous_authoritative_event_id = node["state_lineage"]["authoritative_event_id"]
        .as_str()
        .unwrap_or_default();
    let previous_task_id = task_id_for_authoritative_event(previous_authoritative_event_id);
    let normalized_new_next_step = normalize_next_step_hint(new_next_step);
    if !is_meaningful_pending_return_headline(previous_goal)
        || previous_goal == new_headline
        || resolve_current_goal
        || resolved_pending_return_headline_matches(previous_goal, resolved_headlines)
        || resolved_pending_return_task_id_matches(
            previous_task_id.as_deref(),
            previous_authoritative_event_id,
            resolved_task_ids,
        )
        || (!previous_next_step.is_empty() && previous_next_step == normalized_new_next_step)
    {
        return queue;
    }
    let candidate = json!({
        "task_id": previous_task_id,
        "headline": previous_goal,
        "next_step": previous_next_step,
        "queued_at_epoch_ms": queued_at_epoch_ms,
        "queued_reason": "interrupted_by_new_handoff",
        "resume_state": "pending_return",
        "authoritative_event_id": node["state_lineage"]["authoritative_event_id"],
        "authoritative_event_kind": node["state_lineage"]["authoritative_event_kind"],
        "authoritative_local_path": node["state_lineage"]["authoritative_local_path"],
    });
    prepend_pending_return_item(&mut queue, candidate);
    queue.truncate(MAX_PENDING_RETURN_QUEUE);
    queue
}

fn resolved_pending_return_headline_matches(value: &str, resolved_headlines: &[String]) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && resolved_headlines
            .iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .any(|item| item == trimmed)
}

fn normalize_resolved_task_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(stripped) = trimmed.strip_prefix("task::") {
        let stripped = stripped.trim();
        if stripped.is_empty() {
            None
        } else {
            Some(format!("task::{stripped}"))
        }
    } else {
        Some(format!("task::{trimmed}"))
    }
}

fn task_id_for_authoritative_event(event_id: &str) -> Option<String> {
    let trimmed = event_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(format!("task::{trimmed}"))
    }
}

fn resolved_pending_return_task_id_matches(
    task_id: Option<&str>,
    authoritative_event_id: &str,
    resolved_task_ids: &[String],
) -> bool {
    let Some(candidate_task_id) = task_id
        .and_then(normalize_resolved_task_id)
        .or_else(|| task_id_for_authoritative_event(authoritative_event_id))
    else {
        return false;
    };
    resolved_task_ids.iter().any(|item| {
        normalize_resolved_task_id(item)
            .as_deref()
            .is_some_and(|value| value == candidate_task_id)
    })
}

fn prune_resolved_pending_return_items(
    queue: &mut Vec<Value>,
    resolved_headlines: &[String],
    resolved_task_ids: &[String],
) {
    if resolved_headlines.is_empty() && resolved_task_ids.is_empty() {
        return;
    }
    queue.retain(|item| {
        !resolved_pending_return_headline_matches(
            item["headline"].as_str().unwrap_or_default(),
            resolved_headlines,
        ) && !resolved_pending_return_task_id_matches(
            item["task_id"].as_str(),
            item["authoritative_event_id"].as_str().unwrap_or_default(),
            resolved_task_ids,
        )
    });
}

fn extract_pending_return_queue(
    authoritative_event: &Value,
    fallback_epoch_ms: u64,
    current_goal: &str,
    current_next_step: &str,
) -> Value {
    let mut queue = authoritative_event["pending_return_queue"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    queue.retain(|item| {
        let headline = item["headline"].as_str().unwrap_or_default();
        let next_step = item["next_step"].as_str().unwrap_or_default();
        is_meaningful_pending_return_headline(headline)
            && !(headline == current_goal
                && normalize_next_step_hint(next_step)
                    == normalize_next_step_hint(current_next_step))
    });
    for item in &mut queue {
        if item["queued_at_epoch_ms"].is_null() {
            item["queued_at_epoch_ms"] = json!(fallback_epoch_ms);
        }
        if item["task_id"].is_null() {
            if let Some(task_id) = task_id_for_authoritative_event(
                item["authoritative_event_id"].as_str().unwrap_or_default(),
            ) {
                item["task_id"] = json!(task_id);
            }
        }
        if item["resume_state"].is_null() {
            item["resume_state"] = json!("pending_return");
        }
        if item["queued_reason"].is_null() {
            item["queued_reason"] = json!("interrupted_by_new_handoff");
        }
    }
    queue.truncate(MAX_PENDING_RETURN_QUEUE);
    Value::Array(queue)
}

fn prepend_pending_return_item(queue: &mut Vec<Value>, candidate: Value) {
    let candidate_event_id = candidate["authoritative_event_id"]
        .as_str()
        .unwrap_or_default();
    let candidate_headline = candidate["headline"].as_str().unwrap_or_default();
    let candidate_next_step = candidate["next_step"].as_str().unwrap_or_default();
    queue.retain(|item| {
        let item_event_id = item["authoritative_event_id"].as_str().unwrap_or_default();
        let item_headline = item["headline"].as_str().unwrap_or_default();
        let item_next_step = item["next_step"].as_str().unwrap_or_default();
        if !candidate_event_id.is_empty() && item_event_id == candidate_event_id {
            return false;
        }
        !(item_headline == candidate_headline && item_next_step == candidate_next_step)
    });
    queue.insert(0, candidate);
}

fn is_meaningful_pending_return_headline(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed != "ещё нет данных"
        && trimmed != "Продолжить активную рабочую линию"
}

fn summarize_pending_return_queue(value: &Value) -> Option<String> {
    let items = value.as_array()?;
    let rendered = items
        .iter()
        .filter_map(|item| {
            let headline = item["headline"].as_str().unwrap_or_default();
            if !is_meaningful_pending_return_headline(headline) {
                None
            } else {
                Some(collapse_human_text(headline, 72))
            }
        })
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        let more = rendered.len().saturating_sub(1);
        let mut summary = format!("pending_return({}): {}", rendered.len(), rendered[0]);
        if more > 0 {
            summary.push_str(&format!("; +{more} more"));
        }
        Some(summary)
    }
}

fn build_project_task_tree(
    project: &Value,
    namespace: &Value,
    authoritative_event: &Value,
    current_goal: &str,
    current_next_step: &str,
    pending_return_queue: &Value,
) -> Value {
    let project_code = project["code"].as_str().unwrap_or_default();
    let namespace_code = namespace["code"].as_str().unwrap_or_default();
    let root_task_id = format!("{project_code}::{namespace_code}::open-task-root");
    let active_event_id = authoritative_event["event_id"].as_str().unwrap_or_default();
    let active_task_id = if active_event_id.is_empty() {
        format!("{root_task_id}::active")
    } else {
        format!("task::{active_event_id}")
    };
    let active_recorded_at = authoritative_event["recorded_at_epoch_ms"].as_u64();
    let active_source_kind = authoritative_event["source_kind"]
        .as_str()
        .unwrap_or("working_state_restore");
    let mut nodes = vec![json!({
        "task_id": active_task_id,
        "parent_task_id": root_task_id,
        "task_role": "active",
        "task_state": "active",
        "resume_state": "active",
        "headline": current_goal,
        "next_step": current_next_step,
        "authoritative_event_id": active_event_id,
        "recorded_at_epoch_ms": active_recorded_at,
        "source_kind": active_source_kind,
    })];
    let mut edges = vec![json!({
        "from_task_id": root_task_id,
        "to_task_id": nodes[0]["task_id"].clone(),
        "relation": "tracks_open_task",
        "priority_rank": 0,
    })];

    if let Some(items) = pending_return_queue.as_array() {
        for (index, item) in items.iter().enumerate() {
            let pending_event_id = item["authoritative_event_id"].as_str().unwrap_or_default();
            let task_id = if pending_event_id.is_empty() {
                format!("{root_task_id}::pending-return-{}", index + 1)
            } else {
                format!("task::{pending_event_id}")
            };
            let priority_rank = (index + 1) as u64;
            nodes.push(json!({
                "task_id": task_id,
                "parent_task_id": root_task_id,
                "task_role": "pending_return",
                "task_state": "suspended",
                "resume_state": item["resume_state"].as_str().unwrap_or("pending_return"),
                "headline": item["headline"].as_str().unwrap_or_default(),
                "next_step": item["next_step"].as_str().unwrap_or_default(),
                "authoritative_event_id": pending_event_id,
                "queued_at_epoch_ms": item["queued_at_epoch_ms"].as_u64(),
                "queued_reason": item["queued_reason"].as_str().unwrap_or("interrupted_by_new_handoff"),
                "source_kind": "pending_return_queue",
            }));
            edges.push(json!({
                "from_task_id": root_task_id,
                "to_task_id": nodes.last().and_then(|node| node.get("task_id")).cloned().unwrap_or(Value::Null),
                "relation": "tracks_open_task",
                "priority_rank": priority_rank,
            }));
        }
    }

    json!({
        "tree_version": PROJECT_TASK_TREE_VERSION,
        "project_code": project_code,
        "namespace_code": namespace_code,
        "root_task_id": root_task_id,
        "open_tasks_count": nodes.len(),
        "pending_return_count": nodes.len().saturating_sub(1),
        "nodes": nodes,
        "edges": edges,
    })
}

fn summarize_project_task_tree(value: &Value) -> Option<String> {
    let nodes = value["nodes"].as_array()?;
    if nodes.is_empty() {
        return None;
    }
    let active = nodes
        .iter()
        .find(|item| item["task_role"].as_str() == Some("active"));
    let active_headline = active
        .and_then(|item| item["headline"].as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("ещё нет данных");
    let pending = nodes
        .iter()
        .filter(|item| item["task_role"].as_str() == Some("pending_return"))
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Some(format!(
            "active: {}",
            collapse_human_text(active_headline, 72)
        ));
    }
    let pending_headline = pending
        .iter()
        .filter_map(|item| item["headline"].as_str())
        .find(|headline| !headline.is_empty())
        .map(|headline| collapse_human_text(headline, 72))
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let more = pending.len().saturating_sub(1);
    let mut pending_summary = format!("pending_return({}): {pending_headline}", pending.len());
    if more > 0 {
        pending_summary.push_str(&format!("; +{more} more"));
    }
    Some(format!(
        "active: {}; {pending_summary}",
        collapse_human_text(active_headline, 72)
    ))
}

fn build_project_task_ledger(
    project: &Value,
    namespace: &Value,
    events: &[ObservabilitySnapshotRecord],
    authoritative_event_id: &str,
    pending_return_queue: &Value,
) -> Value {
    let project_code = project["code"].as_str().unwrap_or_default();
    let namespace_code = namespace["code"].as_str().unwrap_or_default();
    let pending_event_ids = pending_return_queue
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["authoritative_event_id"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut seen_event_ids = Vec::<String>::new();

    for snapshot in events {
        let event = &snapshot.payload["working_state_event"];
        if event["event_kind"].as_str() != Some("continuity_handoff")
            || is_meta_continuity_event(event)
        {
            continue;
        }
        let event_id = event["event_id"].as_str().unwrap_or_default();
        if !event_id.is_empty() {
            if seen_event_ids.iter().any(|value| value == event_id) {
                continue;
            }
            seen_event_ids.push(event_id.to_string());
        }
        let task_role = if !authoritative_event_id.is_empty() && event_id == authoritative_event_id
        {
            "active"
        } else if pending_event_ids.iter().any(|value| value == event_id) {
            "pending_return"
        } else {
            "historical_handoff"
        };
        let task_state = match task_role {
            "active" => "active",
            "pending_return" => "suspended",
            _ => "superseded",
        };
        let resume_state = match task_role {
            "active" => "active",
            "pending_return" => "pending_return",
            _ => "historical_only",
        };
        let task_id = if event_id.is_empty() {
            format!(
                "task::{project_code}::{namespace_code}::historical-{}",
                entries.len() + 1
            )
        } else {
            format!("task::{event_id}")
        };
        entries.push(json!({
            "task_id": task_id,
            "headline": event["headline"].as_str().unwrap_or_default(),
            "next_step": event["next_step_hint"].as_str().unwrap_or_default(),
            "task_role": task_role,
            "task_state": task_state,
            "resume_state": resume_state,
            "authoritative_event_id": event_id,
            "recorded_at_epoch_ms": event["recorded_at_epoch_ms"].as_u64(),
            "source_kind": event["source_kind"].as_str().unwrap_or("continuity_handoff"),
            "local_path": event["local_path"].as_str().unwrap_or_default(),
        }));
    }

    if let Some(items) = pending_return_queue.as_array() {
        for (index, item) in items.iter().enumerate() {
            let pending_event_id = item["authoritative_event_id"].as_str().unwrap_or_default();
            if !pending_event_id.is_empty()
                && entries
                    .iter()
                    .any(|entry| entry["authoritative_event_id"].as_str() == Some(pending_event_id))
            {
                continue;
            }
            let task_id = if pending_event_id.is_empty() {
                format!(
                    "task::{project_code}::{namespace_code}::pending-return-history-{}",
                    index + 1
                )
            } else {
                format!("task::{pending_event_id}")
            };
            entries.push(json!({
                "task_id": task_id,
                "headline": item["headline"].as_str().unwrap_or_default(),
                "next_step": item["next_step"].as_str().unwrap_or_default(),
                "task_role": "pending_return",
                "task_state": "suspended",
                "resume_state": item["resume_state"].as_str().unwrap_or("pending_return"),
                "authoritative_event_id": pending_event_id,
                "recorded_at_epoch_ms": item["queued_at_epoch_ms"].as_u64(),
                "source_kind": "pending_return_queue",
                "queued_reason": item["queued_reason"].as_str().unwrap_or("interrupted_by_new_handoff"),
            }));
        }
    }

    let open_tasks_count = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry["task_role"].as_str().unwrap_or_default(),
                "active" | "pending_return"
            )
        })
        .count();
    let historical_handoffs_count = entries
        .iter()
        .filter(|entry| entry["task_role"].as_str() == Some("historical_handoff"))
        .count();

    json!({
        "ledger_version": PROJECT_TASK_LEDGER_VERSION,
        "project_code": project_code,
        "namespace_code": namespace_code,
        "entries_count": entries.len(),
        "open_tasks_count": open_tasks_count,
        "historical_handoffs_count": historical_handoffs_count,
        "persistence_state": "restore_side_only",
        "storage_lane": "working_state_restore_window",
        "entries": entries,
    })
}

fn summarize_project_task_ledger(value: &Value) -> Option<String> {
    let entries = value["entries"].as_array()?;
    if entries.is_empty() {
        return None;
    }
    let active = entries
        .iter()
        .find(|item| item["task_role"].as_str() == Some("active"));
    let active_headline = active
        .and_then(|item| item["headline"].as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("ещё нет данных");
    let pending = entries
        .iter()
        .filter(|item| item["task_role"].as_str() == Some("pending_return"))
        .count();
    let historical = entries
        .iter()
        .filter(|item| item["task_role"].as_str() == Some("historical_handoff"))
        .count();
    Some(format!(
        "active: {}; pending_return({pending}); historical_handoffs({historical})",
        collapse_human_text(active_headline, 72)
    ))
}

fn summarize_execctl_active_lease(value: &Value) -> Option<String> {
    let owner_state = value["lease_owner_state"]
        .as_str()
        .filter(|item| !item.is_empty())
        .unwrap_or("unknown_owner");
    let headline = value["headline"].as_str().filter(|item| !item.is_empty())?;
    Some(format!(
        "{owner_state}: {}",
        collapse_human_text(headline, 72)
    ))
}

fn overlay_durable_project_task_ledger(
    bundle: &mut Value,
    project: &Value,
    namespace: &Value,
    durable_entries: &[ExecCtlTaskLedgerEntryRecord],
) {
    let Some(restore) = bundle
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if durable_entries.is_empty() {
        if let Some(ledger) = restore
            .get_mut("project_task_ledger")
            .and_then(Value::as_object_mut)
        {
            ledger.insert("persistence_state".to_string(), json!("restore_side_only"));
            ledger.insert(
                "storage_lane".to_string(),
                json!("working_state_restore_window"),
            );
        }
        return;
    }

    let pending_return_queue = restore
        .get("pending_return_queue")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let authoritative_event_id = restore
        .get("state_lineage")
        .and_then(|value| value["authoritative_event_id"].as_str())
        .unwrap_or_default()
        .to_string();
    let ledger = build_durable_project_task_ledger(
        project,
        namespace,
        durable_entries,
        &authoritative_event_id,
        &pending_return_queue,
    );
    let summary = summarize_project_task_ledger(&ledger);
    restore.insert("project_task_ledger".to_string(), ledger);
    if let Some(summary) = summary {
        restore.insert("project_task_ledger_summary".to_string(), json!(summary));
    }
}

fn overlay_execctl_active_lease(bundle: &mut Value, active_lease: Option<&ExecCtlTaskLeaseRecord>) {
    let Some(restore) = bundle
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(lease) = active_lease else {
        return;
    };
    let session_id = restore
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let owner_state = if lease.owner_session_id.as_deref() == Some(session_id) {
        "same_session_owner"
    } else {
        "previous_session_owner"
    };
    let lease_value = json!({
        "lease_version": "execctl-active-lease-v1",
        "lease_id": lease.lease_id.to_string(),
        "agent_scope": lease.agent_scope,
        "lease_state": lease.lease_state,
        "owner_session_id": lease.owner_session_id,
        "owner_thread_id": lease.owner_thread_id,
        "lease_owner_state": owner_state,
        "source_snapshot_id": lease.source_snapshot_id.map(|value| value.to_string()),
        "source_event_id": lease.source_event_id,
        "source_kind": lease.source_kind,
        "headline": lease.headline,
        "next_step": lease.next_step,
        "local_path": lease.local_path,
        "acquired_at_epoch_ms": lease.acquired_at_epoch_ms,
        "heartbeat_at_epoch_ms": lease.heartbeat_at_epoch_ms,
        "expires_at_epoch_ms": lease.expires_at_epoch_ms,
        "created_at_epoch_ms": lease.created_at_epoch_ms,
        "updated_at_epoch_ms": lease.updated_at_epoch_ms,
        "storage_lane": "ami.execctl_task_leases",
    });
    restore.insert("execctl_active_lease".to_string(), lease_value.clone());
    if let Some(owner_thread_id) = lease
        .owner_thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        restore.insert("thread_id".to_string(), json!(owner_thread_id));
    }
    if let Some(summary) = summarize_execctl_active_lease(&lease_value) {
        restore.insert("execctl_active_lease_summary".to_string(), json!(summary));
    }
}

fn overlay_client_budget_guard(bundle: &mut Value, client_budget_guard: &Value) {
    let Some(restore) = bundle
        .get_mut("working_state_restore")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    restore.insert(
        "client_budget_guard".to_string(),
        client_budget_guard.clone(),
    );
    let current_goal = restore["current_goal"]
        .as_str()
        .unwrap_or("ещё нет данных")
        .to_string();
    let next_step = restore["next_step"]
        .as_str()
        .unwrap_or("ещё нет данных")
        .to_string();
    let contract = restore
        .get("execctl_resume_contract")
        .cloned()
        .unwrap_or(Value::Null);
    let startup_next_action = build_startup_next_action(
        &current_goal,
        &next_step,
        &contract,
        client_budget_guard,
        restore["project"]["code"].as_str(),
        restore["namespace"]["code"].as_str(),
        restore["project"]["repo_root"].as_str(),
    );
    let startup_next_action_summary = summarize_startup_next_action(&startup_next_action);
    restore.insert("startup_next_action".to_string(), startup_next_action);
    if let Some(summary) = startup_next_action_summary {
        restore.insert("startup_next_action_summary".to_string(), json!(summary));
    }
}

fn default_client_budget_guard() -> Value {
    json!({
        "source": "dashboard_current_session_budget_guard_v2",
        "status": "unknown",
        "status_label": "нет данных",
        "should_rotate_chat_now": false,
        "should_rotate_chat_soon": false,
        "full_turn_savings_proven": false,
        "note": "client-budget guard ещё не materialized"
    })
}

fn fallback_client_budget_guard_from_error(error: &str) -> Value {
    let mut guard = default_client_budget_guard();
    if let Some(node) = guard.as_object_mut() {
        node.insert("status".to_string(), json!("unknown"));
        node.insert(
            "note".to_string(),
            json!(format!("client-budget guard не materialized: {error}")),
        );
    }
    guard
}

fn build_durable_project_task_ledger(
    project: &Value,
    namespace: &Value,
    entries: &[ExecCtlTaskLedgerEntryRecord],
    authoritative_event_id: &str,
    pending_return_queue: &Value,
) -> Value {
    let project_code = project["code"].as_str().unwrap_or_default();
    let namespace_code = namespace["code"].as_str().unwrap_or_default();
    let pending_event_ids = pending_return_queue
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["authoritative_event_id"].as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut serialized_entries = Vec::new();

    for entry in entries {
        let task_role = if !authoritative_event_id.is_empty()
            && entry.source_event_id == authoritative_event_id
        {
            "active"
        } else if pending_event_ids
            .iter()
            .any(|value| value == &entry.source_event_id)
        {
            "pending_return"
        } else {
            "historical_handoff"
        };
        let task_state = match task_role {
            "active" => "active",
            "pending_return" => "suspended",
            _ => "superseded",
        };
        let resume_state = match task_role {
            "active" => "active",
            "pending_return" => "pending_return",
            _ => "historical_only",
        };
        serialized_entries.push(json!({
            "ledger_entry_id": entry.ledger_entry_id.to_string(),
            "task_id": format!("task::{}", entry.source_event_id),
            "headline": entry.headline,
            "next_step": entry.next_step,
            "summary": entry.summary,
            "task_role": task_role,
            "task_state": task_state,
            "resume_state": resume_state,
            "authoritative_event_id": entry.source_event_id,
            "recorded_at_epoch_ms": entry.recorded_at_epoch_ms,
            "created_at_epoch_ms": entry.created_at_epoch_ms,
            "event_kind": entry.event_kind,
            "source_kind": entry.source_kind,
            "source_snapshot_id": entry.source_snapshot_id.map(|value| value.to_string()),
            "agent_scope": entry.agent_scope,
            "session_id": entry.session_id,
            "thread_id": entry.thread_id,
            "active_files": entry.active_files,
            "open_questions": entry.open_questions,
            "materialized_notes": entry.materialized_notes,
            "pending_return_queue": entry.pending_return_queue,
            "local_path": entry.local_path,
        }));
    }

    let open_tasks_count = serialized_entries
        .iter()
        .filter(|entry| {
            matches!(
                entry["task_role"].as_str().unwrap_or_default(),
                "active" | "pending_return"
            )
        })
        .count();
    let historical_handoffs_count = serialized_entries
        .iter()
        .filter(|entry| entry["task_role"].as_str() == Some("historical_handoff"))
        .count();

    json!({
        "ledger_version": PROJECT_TASK_LEDGER_VERSION,
        "project_code": project_code,
        "namespace_code": namespace_code,
        "entries_count": serialized_entries.len(),
        "open_tasks_count": open_tasks_count,
        "historical_handoffs_count": historical_handoffs_count,
        "persistence_state": "durable_postgres",
        "storage_lane": "ami.execctl_task_ledger_entries",
        "entries": serialized_entries,
    })
}

fn build_execctl_resume_contract(
    project_task_tree: &Value,
    pending_return_queue: &Value,
    required_task_set: &[String],
) -> Value {
    let nodes = project_task_tree["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let active_task = nodes
        .iter()
        .find(|item| item["task_role"].as_str() == Some("active"))
        .cloned()
        .unwrap_or(Value::Null);
    let required_return_task = nodes
        .iter()
        .find(|item| {
            item["task_role"].as_str() == Some("pending_return")
                && is_meaningful_pending_return_headline(
                    item["headline"].as_str().unwrap_or_default(),
                )
        })
        .cloned()
        .or_else(|| {
            pending_return_queue
                .as_array()
                .and_then(|items| {
                    items.iter().find(|item| {
                        is_meaningful_pending_return_headline(
                            item["headline"].as_str().unwrap_or_default(),
                        )
                    })
                })
                .cloned()
        })
        .unwrap_or(Value::Null);
    let pending_return_count = nodes
        .iter()
        .filter(|item| item["task_role"].as_str() == Some("pending_return"))
        .count();
    let required_task_set_count = required_task_set.len();
    let resume_state = if required_return_task.is_null() {
        "clear"
    } else {
        "return_required"
    };
    json!({
        "contract_version": "execctl-resume-contract-v1",
        "resume_state": resume_state,
        "no_silent_drop": true,
        "pending_return_count": pending_return_count,
        "active_task": active_task,
        "required_return_task": required_return_task,
        "required_task_set_count": required_task_set_count,
        "required_task_set": required_task_set,
    })
}

fn summarize_execctl_resume_contract(value: &Value) -> Option<String> {
    if value["resume_state"].as_str() == Some("clear") {
        return Some("clear".to_string());
    }
    let required = &value["required_return_task"];
    let headline = required["headline"]
        .as_str()
        .filter(|item| !item.is_empty())
        .unwrap_or("ещё нет данных");
    let count = value["pending_return_count"].as_u64().unwrap_or(0);
    let required_task_set_count = value["required_task_set_count"].as_u64().unwrap_or(0);
    let mut summary = format!(
        "return_required({count}): {}",
        collapse_human_text(headline, 72)
    );
    if required_task_set_count > 0 {
        summary.push_str(&format!(" + required_task_set({required_task_set_count})"));
    }
    Some(summary)
}

pub(crate) fn build_rotate_chat_action_bundle(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    preserves_return_obligation: bool,
    recommended_headline: Option<&str>,
    recommended_next_step: Option<&str>,
) -> Value {
    build_rotate_chat_action_bundle_for_stage(
        project_code,
        namespace_code,
        repo_root,
        preserves_return_obligation,
        recommended_headline,
        recommended_next_step,
        HostContextCompactionStage::Inactive,
    )
}

pub(crate) fn build_rotate_chat_action_bundle_for_stage(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    preserves_return_obligation: bool,
    recommended_headline: Option<&str>,
    recommended_next_step: Option<&str>,
    host_context_compaction_stage: HostContextCompactionStage,
) -> Value {
    build_rotate_chat_action_bundle_for_stage_with_preference(
        project_code,
        namespace_code,
        repo_root,
        preserves_return_obligation,
        recommended_headline,
        recommended_next_step,
        host_context_compaction_stage,
        true,
    )
}

pub(crate) fn build_rotate_chat_action_bundle_for_stage_with_preference(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    preserves_return_obligation: bool,
    recommended_headline: Option<&str>,
    recommended_next_step: Option<&str>,
    host_context_compaction_stage: HostContextCompactionStage,
    prefer_same_thread_host_control_primary: bool,
) -> Value {
    build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
        project_code,
        namespace_code,
        repo_root,
        preserves_return_obligation,
        recommended_headline,
        recommended_next_step,
        host_context_compaction_stage,
        prefer_same_thread_host_control_primary,
        current_thread_id().as_deref(),
        None,
    )
}

pub(crate) fn build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    preserves_return_obligation: bool,
    recommended_headline: Option<&str>,
    recommended_next_step: Option<&str>,
    host_context_compaction_stage: HostContextCompactionStage,
    prefer_same_thread_host_control_primary: bool,
    thread_id: Option<&str>,
    primary_command_id: Option<&str>,
) -> Value {
    let project_code = project_code.filter(|value| !value.is_empty());
    let namespace_code = namespace_code.filter(|value| !value.is_empty());
    let repo_root = repo_root.filter(|value| !value.is_empty());
    let recommended_headline = recommended_headline
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let recommended_next_step = recommended_next_step
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut missing_inputs = Vec::new();
    if project_code.is_none() {
        missing_inputs.push("project_code");
    }
    if namespace_code.is_none() {
        missing_inputs.push("namespace_code");
    }
    if repo_root.is_none() {
        missing_inputs.push("repo_root");
    }
    let project_arg = project_code.unwrap_or("<project_code_required>");
    let namespace_arg = namespace_code.unwrap_or("<namespace_code_required>");
    let repo_root_arg = repo_root.unwrap_or("<repo_root_required>");
    let handoff_command = build_workspace_aware_handoff_command(
        project_code,
        namespace_code,
        repo_root,
        recommended_headline,
        recommended_next_step,
    );
    let startup_command = build_workspace_aware_startup_command(
        project_code,
        namespace_code,
        repo_root,
        "live_continuity_startup",
        true,
    );
    let rotate_helper_command =
        build_workspace_aware_rotate_helper_command(project_code, namespace_code, repo_root);
    let host_current_thread_control =
        build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
            thread_id,
            if host_context_compaction_stage == HostContextCompactionStage::Inactive {
                HostContextCompactionStage::Inactive
            } else {
                host_context_compaction_stage
            },
            primary_command_id,
        );
    let host_current_thread_control_launch_command = if prefer_same_thread_host_control_primary {
        build_host_current_thread_control_observe_launch_command(
            project_code,
            namespace_code,
            repo_root,
            &host_current_thread_control,
        )
    } else {
        None
    };
    let same_thread_host_control_primary = host_current_thread_control_launch_command.is_some();
    let (primary_command_kind, primary_command) =
        if let Some(command) = host_current_thread_control_launch_command.clone() {
            (
                Some("same_thread_host_control_launch_command"),
                Some(command),
            )
        } else if let Some(command) = rotate_helper_command.clone() {
            (Some("rotate_helper_command"), Some(command))
        } else {
            (None, None)
        };
    let copy_paste_ready =
        primary_command.is_some() || (rotate_helper_command.is_some() && startup_command.is_some());
    let order = if same_thread_host_control_primary {
        json!([
            "run_same_thread_host_control",
            "confirm_surface_effect",
            "fallback_rotate_chat"
        ])
    } else {
        json!([
            "run_rotate_helper",
            "open_fresh_chat",
            "run_continuity_startup"
        ])
    };
    json!({
        "bundle_version": "rotate-chat-action-bundle-v1",
        "ready_for_automation": missing_inputs.is_empty(),
        "missing_inputs": missing_inputs,
        "preserves_return_obligation": preserves_return_obligation,
        "host_current_thread_control": host_current_thread_control,
        "recommended_handoff": {
            "available": recommended_headline.is_some() && recommended_next_step.is_some(),
            "headline": recommended_headline,
            "next_step": recommended_next_step,
        },
        "operator_flow": {
            "copy_paste_ready": copy_paste_ready,
            "primary_command_kind": primary_command_kind,
            "primary_command": primary_command,
            "host_current_thread_control_launch_command":
                host_current_thread_control_launch_command,
            "rotate_helper_command": rotate_helper_command,
            "handoff_command": handoff_command,
            "open_fresh_chat_summary": if same_thread_host_control_primary {
                "если same-thread compact control не снизил burn, открой свежий чат клиента вручную"
            } else {
                "после rotate helper открой свежий чат клиента вручную"
            },
            "startup_command": startup_command,
        },
        "order": order,
        "run_same_thread_host_control": {
            "subcommand": "observe client-budget-host-control-launch",
            "argv_template": if let Some(thread_id) =
                host_current_thread_control["thread_id"].as_str()
            {
                json!([
                    "amai",
                    "observe",
                    "client-budget-host-control-launch",
                    "--thread-id",
                    thread_id,
                    "--command-id",
                    host_current_thread_control["command_id"].as_str().unwrap_or("<command_id_required>"),
                    "--project",
                    project_arg,
                    "--namespace",
                    namespace_arg,
                    "--repo-root",
                    repo_root_arg
                ])
            } else {
                Value::Null
            },
            "project": project_code,
            "namespace": namespace_code,
            "repo_root": repo_root,
            "thread_id": host_current_thread_control["thread_id"].as_str(),
            "command_id": host_current_thread_control["command_id"].as_str(),
            "preferred_before_rotate": same_thread_host_control_primary
        },
        "confirm_surface_effect": {
            "action_kind": "confirm_host_current_thread_control_feedback",
            "required": same_thread_host_control_primary,
            "summary": if same_thread_host_control_primary {
                "после запуска compact surface отметь, открылся ли он и помог ли уменьшить regrowth/burn"
            } else {
                "same-thread compact confirmation не требуется"
            }
        },
        "run_rotate_helper": {
            "subcommand": "continuity rotate-chat",
            "argv_template": [
                "amai",
                "continuity",
                "rotate-chat",
                "--project",
                project_arg,
                "--namespace",
                namespace_arg,
                "--repo-root",
                repo_root_arg
            ],
            "project": project_code,
            "namespace": namespace_code,
            "repo_root": repo_root,
            "captures_handoff": true,
            "prints_startup_command": true
        },
        "fallback_rotate_chat": {
            "available": rotate_helper_command.is_some(),
            "summary": "если same-thread compact window не помог, fallback — continuity rotate-chat и fresh continuity startup",
            "rotate_helper_command": rotate_helper_command,
            "startup_command": startup_command
        },
        "capture_continuity_handoff": {
            "subcommand": "continuity handoff",
            "argv_template": [
                "amai",
                "continuity",
                "handoff",
                "--project",
                project_arg,
                "--namespace",
                namespace_arg,
                "--headline",
                "<headline_required>",
                "--next-step",
                "<next_step_required>"
            ],
            "project": project_code,
            "namespace": namespace_code,
            "requires_caller_supplied": ["headline", "next_step"],
            "details_file_optional": true
        },
        "open_fresh_chat": {
            "action_kind": "open_fresh_client_chat",
            "required": true
        },
        "run_continuity_startup": {
            "subcommand": "continuity startup",
            "argv_template": [
                "amai",
                "continuity",
                "startup",
                "--project",
                project_arg,
                "--namespace",
                namespace_arg,
                "--repo-root",
                repo_root_arg,
                "--token-source-kind",
                "live_continuity_startup",
                "--json"
            ],
            "project": project_code,
            "namespace": namespace_code,
            "repo_root": repo_root,
            "token_source_kind": "live_continuity_startup"
        }
    })
}

pub(crate) fn build_wait_for_global_client_budget_action_bundle(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    preserves_return_obligation: bool,
    recommended_headline: Option<&str>,
    recommended_next_step: Option<&str>,
) -> Value {
    let project_code = project_code.filter(|value| !value.is_empty());
    let namespace_code = namespace_code.filter(|value| !value.is_empty());
    let repo_root = repo_root.filter(|value| !value.is_empty());
    let recommended_headline = recommended_headline
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let recommended_next_step = recommended_next_step
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut missing_inputs = Vec::new();
    if project_code.is_none() {
        missing_inputs.push("project_code");
    }
    if namespace_code.is_none() {
        missing_inputs.push("namespace_code");
    }
    if repo_root.is_none() {
        missing_inputs.push("repo_root");
    }
    let project_arg = project_code.unwrap_or("<project_code_required>");
    let namespace_arg = namespace_code.unwrap_or("<namespace_code_required>");
    let repo_root_arg = repo_root.unwrap_or("<repo_root_required>");
    let handoff_command = build_workspace_aware_handoff_command(
        project_code,
        namespace_code,
        repo_root,
        recommended_headline,
        recommended_next_step,
    );
    let startup_command = match (project_code, namespace_code, repo_root) {
        (Some(project), Some(namespace), Some(root)) => Some(shell_join_command(&[
            "amai",
            "continuity",
            "startup",
            "--project",
            project,
            "--namespace",
            namespace,
            "--repo-root",
            root,
            "--token-source-kind",
            "live_continuity_startup",
            "--json",
        ])),
        _ => None,
    };
    json!({
        "bundle_version": "wait-client-budget-action-bundle-v1",
        "ready_for_automation": missing_inputs.is_empty(),
        "missing_inputs": missing_inputs,
        "preserves_return_obligation": preserves_return_obligation,
        "budget_source": build_global_client_limit_source_contract(),
        "recommended_handoff": {
            "available": recommended_headline.is_some() && recommended_next_step.is_some(),
            "headline": recommended_headline,
            "next_step": recommended_next_step,
        },
        "operator_flow": {
            "copy_paste_ready": handoff_command.is_some() && startup_command.is_some(),
            "handoff_command": handoff_command,
            "wait_summary": "не отвечай содержательно, пока не восстановится окно клиентского лимита",
            "resume_after_recovery_summary": "после восстановления лимита снова проверь continuity startup или client-budget guard перед следующим substantive reply",
            "startup_after_recovery_command": startup_command,
        },
        "order": [
            "capture_continuity_handoff",
            "wait_for_budget_recovery",
            "recheck_after_recovery"
        ],
        "capture_continuity_handoff": {
            "subcommand": "continuity handoff",
            "argv_template": [
                "amai",
                "continuity",
                "handoff",
                "--project",
                project_arg,
                "--namespace",
                namespace_arg,
                "--headline",
                "<headline_required>",
                "--next-step",
                "<next_step_required>"
            ],
            "project": project_code,
            "namespace": namespace_code,
            "requires_caller_supplied": ["headline", "next_step"],
            "details_file_optional": true
        },
        "wait_for_budget_recovery": {
            "action_kind": "wait_for_global_client_budget_recovery",
            "required": true,
            "summary": "дождись нового окна клиентского лимита или снижения внешнего расхода"
        },
        "recheck_after_recovery": {
            "subcommand": "continuity startup",
            "argv_template": [
                "amai",
                "continuity",
                "startup",
                "--project",
                project_arg,
                "--namespace",
                namespace_arg,
                "--repo-root",
                repo_root_arg,
                "--token-source-kind",
                "live_continuity_startup",
                "--json"
            ],
            "project": project_code,
            "namespace": namespace_code,
            "repo_root": repo_root,
            "token_source_kind": "live_continuity_startup",
            "summary": "после восстановления лимита заново проверь continuity startup и только потом продолжай substantive reply"
        }
    })
}

fn build_startup_next_action(
    current_goal: &str,
    next_step: &str,
    contract: &Value,
    client_budget_guard: &Value,
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
) -> Value {
    let resume_state = contract["resume_state"].as_str().unwrap_or("clear");
    let no_silent_drop = contract["no_silent_drop"].as_bool().unwrap_or(true);
    let active_task = &contract["active_task"];
    let required_return_task = &contract["required_return_task"];
    let active_headline = active_task["headline"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(current_goal);
    let active_next_step = active_task["next_step"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(next_step);
    let required_headline = required_return_task["headline"]
        .as_str()
        .filter(|value| !value.is_empty());
    let required_next_step = required_return_task["next_step"]
        .as_str()
        .filter(|value| !value.is_empty());
    let required_task_set = extract_required_task_set_from_value(&contract["required_task_set"]);
    let required_task_set_summary = summarize_required_task_set(&required_task_set);
    let required_task_set_next_step = required_task_set.first().map(String::as_str);
    let has_required_task_set = !required_task_set.is_empty();
    let _ = client_budget_guard;
    let _ = project_code;
    let _ = namespace_code;
    let _ = repo_root;
    if resume_state != "clear" && required_headline.is_some() {
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
    } else if has_required_task_set {
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "honor_required_task_set",
            "blocking": true,
            "reason": "required_task_set_present",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": active_headline,
            "next_step": required_task_set_next_step.unwrap_or(active_next_step),
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
            "next_step": active_next_step,
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
    let mut summary = format!("{action_kind}: {}", collapse_human_text(headline, 72));
    if action_kind == "honor_required_task_set" {
        if let Some(task_summary) = value["required_task_set_summary"]
            .as_str()
            .filter(|item| !item.is_empty())
        {
            summary.push_str(&format!(" ({})", collapse_human_text(task_summary, 72)));
        }
    }
    Some(summary)
}

fn normalize_next_step_hint(value: &str) -> String {
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
    normalized
        .trim_end_matches(['`', '"', '\'', '«', '»', '|'])
        .trim()
        .to_string()
}

fn summarize_details(details: &str, headline: &str, next_step: &str) -> String {
    let trimmed = details.trim();
    if trimmed.is_empty() {
        format!("{headline}. Дальше: {next_step}.")
    } else {
        let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.chars().count() > 260 {
            format!("{}...", collapsed.chars().take(260).collect::<String>())
        } else {
            collapsed
        }
    }
}

fn extract_paths_from_text(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in text.split_whitespace() {
        let cleaned = token
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '(' | ')' | '[' | ']' | '"' | '\'' | ',' | ';' | '`' | '|'
                )
            })
            .trim_end_matches(['.', ':', '`', '|']);
        if cleaned.starts_with("/home/") {
            push_unique(&mut paths, cleaned.to_string());
        } else if let Some(start) = cleaned.find("/home/") {
            push_unique(&mut paths, cleaned[start..].to_string());
        }
        if paths.len() >= MAX_ACTIVE_FILES {
            break;
        }
    }
    paths
}

fn extract_first_question(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| line.ends_with('?'))
        .map(ToOwned::to_owned)
}

fn derive_open_questions(text: &str) -> Vec<String> {
    let mut questions = Vec::new();
    let trimmed = text.trim();
    if looks_like_question(trimmed) {
        push_unique(&mut questions, trimmed.to_string());
    }
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if looks_like_question(line) {
            push_unique(&mut questions, line.to_string());
        }
        if questions.len() >= MAX_OPEN_QUESTIONS {
            break;
        }
    }
    questions
}

fn extract_materialized_notes(text: &str) -> Vec<String> {
    let mut notes = Vec::new();
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let mut saw_bullets = false;
    for line in &lines {
        if let Some(rest) = line.strip_prefix("- ") {
            push_unique(&mut notes, rest.trim().to_string());
            saw_bullets = true;
        }
        if notes.len() >= MAX_MATERIALIZED_NOTES {
            break;
        }
    }
    if saw_bullets {
        return notes;
    }
    for line in lines {
        if is_section_heading(line) || looks_like_question(line) {
            continue;
        }
        let chars = line.chars().count();
        if (16..=220).contains(&chars) {
            push_unique(&mut notes, line.to_string());
        }
        if notes.len() >= MAX_MATERIALIZED_NOTES {
            break;
        }
    }
    notes
}

fn extract_required_task_set(text: &str) -> Vec<String> {
    let mut tasks = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_section {
                break;
            }
            continue;
        }
        let lower = trimmed.to_lowercase();
        let is_header = trimmed.ends_with(':')
            && [
                "задач",
                "обязател",
                "нужно",
                "требует",
                "todo",
                "tasks",
                "must",
                "pending",
                "не выполнен",
                "не закрыт",
            ]
            .iter()
            .any(|needle| lower.contains(needle));
        if is_header {
            in_section = true;
            continue;
        }
        if trimmed.starts_with("- [x]")
            || trimmed.starts_with("* [x]")
            || trimmed.starts_with("• [x]")
        {
            continue;
        }
        if trimmed.starts_with("- [ ]")
            || trimmed.starts_with("* [ ]")
            || trimmed.starts_with("• [ ]")
        {
            push_unique(&mut tasks, clean_task_line(trimmed));
            continue;
        }
        if in_section
            && (trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• "))
        {
            push_unique(&mut tasks, clean_task_line(trimmed));
        }
    }
    tasks.truncate(MAX_REQUIRED_TASK_SET);
    tasks
}

fn extract_required_task_set_from_event(event: &Value) -> Vec<String> {
    let mut tasks = Vec::new();
    if let Some(items) = event["required_task_set"].as_array() {
        for item in items {
            if let Some(value) = item
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push_unique(&mut tasks, value.to_string());
            }
        }
    }
    tasks.truncate(MAX_REQUIRED_TASK_SET);
    tasks
}

fn extract_required_task_set_from_value(value: &Value) -> Vec<String> {
    let mut tasks = Vec::new();
    if let Some(items) = value.as_array() {
        for item in items {
            if let Some(value) = item
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push_unique(&mut tasks, value.to_string());
            }
        }
    }
    tasks.truncate(MAX_REQUIRED_TASK_SET);
    tasks
}

fn summarize_required_task_set(tasks: &[String]) -> Option<String> {
    if tasks.is_empty() {
        return None;
    }
    let first = collapse_human_text(&tasks[0], 64);
    if tasks.len() == 1 {
        Some(first)
    } else {
        Some(format!("{} задач(и): {first}", tasks.len()))
    }
}

fn clean_task_line(line: &str) -> String {
    let trimmed = line
        .trim_start_matches("- [ ]")
        .trim_start_matches("* [ ]")
        .trim_start_matches("• [ ]")
        .trim_start_matches("- ")
        .trim_start_matches("* ")
        .trim_start_matches("• ")
        .trim();
    trimmed.trim_end_matches(['.', ';', ':']).trim().to_string()
}

fn looks_like_question(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with('?') {
        return true;
    }
    if is_section_heading(trimmed) || trimmed.chars().count() > 180 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    [
        "почему ",
        "зачем ",
        "как ",
        "где ",
        "когда ",
        "что ",
        "можно ли ",
        "нужно ли ",
        "what ",
        "why ",
        "how ",
        "where ",
        "when ",
        "can ",
    ]
    .iter()
    .any(|needle| lower.starts_with(needle))
}

fn is_section_heading(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.ends_with(':') && !trimmed.ends_with("?:")
}

fn collect_unique_strings(target: &mut Vec<String>, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        if let Some(text) = item.as_str().filter(|text| !text.is_empty()) {
            push_unique(target, text.to_string());
        }
    }
}

fn collect_active_files(target: &mut Vec<String>, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        if let Some(text) = item
            .as_str()
            .map(normalize_recorded_path)
            .filter(|text| !text.is_empty())
        {
            push_unique(target, text);
        }
    }
}

fn collect_open_questions(target: &mut Vec<String>, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        if let Some(text) = item.as_str().map(str::trim).filter(|text| !text.is_empty())
            && looks_like_question(text)
            && !text.contains('\n')
            && text.chars().count() <= 180
        {
            push_unique(target, text.to_string());
        }
    }
}

fn normalize_recorded_path(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '(' | ')' | '[' | ']' | '"' | '\'' | ',' | ';' | '`' | '|'
            )
        })
        .trim_end_matches(['.', ':', '`', '|'])
        .trim()
        .to_string()
}

fn collect_materialized_notes(target: &mut Vec<String>, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        if let Some(text) = item.as_str().map(str::trim).filter(|text| !text.is_empty())
            && !is_section_heading(text)
            && !looks_like_question(text)
            && !text.contains('\n')
            && (16..=220).contains(&text.chars().count())
        {
            push_unique(target, text.to_string());
        }
    }
}

fn push_unique(target: &mut Vec<String>, value: String) {
    if !target.iter().any(|existing| existing == &value) {
        target.push(value);
    }
}

fn print_string_list(label: &str, value: &Value, limit: usize) {
    let Some(items) = value.as_array() else {
        return;
    };
    if items.is_empty() {
        return;
    }
    let rendered = items
        .iter()
        .filter_map(Value::as_str)
        .take(limit)
        .collect::<Vec<_>>()
        .join(" | ");
    if !rendered.is_empty() {
        println!("{label}: {rendered}");
    }
}

fn print_recent_actions(label: &str, value: &Value, limit: usize) {
    let Some(items) = value.as_array() else {
        return;
    };
    let rendered = items
        .iter()
        .take(limit)
        .filter_map(|item| {
            let headline = item["headline"].as_str().unwrap_or_default();
            let summary = item["summary"].as_str().unwrap_or_default();
            if headline.is_empty() && summary.is_empty() {
                None
            } else if !headline.is_empty() {
                Some(headline.to_string())
            } else {
                Some(collapse_human_text(summary, 120))
            }
        })
        .collect::<Vec<_>>()
        .join(" || ");
    if !rendered.is_empty() {
        println!("{label}: {rendered}");
    }
}

fn collapse_human_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        collapsed.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn human_duration_ms(duration_ms: u64) -> String {
    let duration_secs = duration_ms / 1000;
    let hours = duration_secs / 3600;
    let minutes = (duration_secs % 3600) / 60;
    if hours > 0 {
        format!("{hours} ч. {minutes} мин.")
    } else if minutes > 0 {
        format!("{minutes} мин.")
    } else {
        format!("{} сек.", duration_secs)
    }
}

fn now_epoch_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::postgres;
    use proptest::prelude::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    struct FakeSnapshotSpec<'a> {
        project_code: &'a str,
        namespace_code: &'a str,
        agent_scope: &'a str,
        session_id: &'a str,
        event_kind: &'a str,
        headline: &'a str,
        next_step_hint: &'a str,
        summary: &'a str,
        offset: i64,
    }

    #[test]
    fn derive_open_questions_marks_human_questions() {
        let questions = derive_open_questions("Почему dashboard не показывает нужный файл?");
        assert_eq!(questions.len(), 1);
        assert!(questions[0].contains("Почему"));
    }

    #[test]
    fn derive_open_questions_ignores_section_headers() {
        let questions = derive_open_questions("Почему это важно:");
        assert!(questions.is_empty());
    }

    #[test]
    fn extract_paths_from_text_collects_local_files() {
        let paths = extract_paths_from_text(
            "Смотрели /home/art/Art/README.md и [token]( /home/art/agent-memory-index/src/token_budget.rs ).",
        );
        assert!(
            paths
                .iter()
                .any(|path| path.contains("/home/art/Art/README.md"))
        );
        assert!(
            paths
                .iter()
                .any(|path| path.contains("/home/art/agent-memory-index/src/token_budget.rs"))
        );
    }

    #[test]
    fn extract_active_files_from_context_pack_falls_back_to_cache_reuse_reference() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            },
            "cache_reuse_reference": {
                "state": "same_thread_context_pack_replay",
                "active_files": [
                    "docs/continuity.md",
                    "src/lib.rs"
                ]
            }
        });

        let active_files = extract_active_files_from_context_pack(&payload);

        assert_eq!(
            active_files,
            vec!["docs/continuity.md".to_string(), "src/lib.rs".to_string()]
        );
    }

    #[test]
    fn compact_reply_budget_contract_tightens_after_host_compaction() {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::Preserve,
            false,
            false,
        );
        assert_eq!(
            contract["host_context_compaction_preserve_active"],
            json!(true)
        );
        assert_eq!(contract["host_context_compaction_stage"], json!("preserve"));
        assert_eq!(
            contract["must_protect_recent_host_compaction_gain"],
            json!(true)
        );
        assert_eq!(
            contract["must_minimize_nonessential_progress_updates"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_broad_exploration_without_user_request"],
            json!(true)
        );
        assert_eq!(
            contract["must_prefer_single_batched_tool_read_when_exploring"],
            json!(true)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(2));
        assert_eq!(contract["max_bullets_soft"], json!(1));
        assert_eq!(contract["max_sentences_soft"], json!(2));
        assert_eq!(
            contract["host_context_compaction_preserve_strict_active"],
            json!(false)
        );
        assert_eq!(contract["must_avoid_commentary_only_updates"], json!(false));
    }

    #[test]
    fn compact_reply_budget_contract_tightens_before_host_compaction_under_target_pressure() {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::Inactive,
            true,
            false,
        );
        assert_eq!(
            contract["host_context_compaction_target_pressure_active"],
            json!(true)
        );
        assert_eq!(
            contract["host_context_compaction_inactive_target_pressure_active"],
            json!(true)
        );
        assert_eq!(contract["must_avoid_commentary_only_updates"], json!(true));
        assert_eq!(
            contract["must_batch_all_tool_reads_before_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_wait_for_meaningful_result_before_progress_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_progress_reply_when_only_guard_changed"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_new_tool_turn_without_specific_delta_goal"],
            json!(true)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(0));
        assert_eq!(contract["max_bullets_soft"], json!(1));
        assert_eq!(contract["max_sentences_soft"], json!(2));
    }

    #[test]
    fn compact_reply_budget_contract_marks_pure_burn_before_host_compaction_under_target_pressure()
    {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::Inactive,
            true,
            true,
        );
        assert_eq!(contract["current_live_turn_no_amai_activity"], json!(true));
        assert_eq!(contract["same_meter_pure_burn_turn_active"], json!(true));
        assert_eq!(
            contract["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_progress_reply_when_only_guard_changed"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_new_tool_turn_without_specific_delta_goal"],
            json!(true)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(0));
        assert_eq!(contract["max_bullets_soft"], json!(0));
        assert_eq!(contract["max_sentences_soft"], json!(1));
        assert!(
            contract["summary"]
                .as_str()
                .expect("summary")
                .contains("no_amai_activity_in_current_live_turn")
        );
    }

    #[test]
    fn compact_reply_budget_contract_tightens_preserve_stage_under_target_pressure() {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::Preserve,
            true,
            false,
        );
        assert_eq!(
            contract["host_context_compaction_target_pressure_active"],
            json!(true)
        );
        assert_eq!(
            contract["host_context_compaction_preserve_strict_active"],
            json!(true)
        );
        assert_eq!(contract["must_avoid_commentary_only_updates"], json!(true));
        assert_eq!(
            contract["must_batch_all_tool_reads_before_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_wait_for_meaningful_result_before_progress_reply"],
            json!(true)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(1));
        assert_eq!(contract["max_bullets_soft"], json!(0));
        assert_eq!(contract["max_sentences_soft"], json!(1));
    }

    #[test]
    fn compact_reply_budget_contract_enters_critical_regrowth_stage_after_host_rebound() {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::CriticalRegrowth,
            true,
            false,
        );
        assert_eq!(
            contract["host_context_compaction_stage"],
            json!("critical_regrowth")
        );
        assert_eq!(
            contract["host_context_compaction_critical_regrowth_active"],
            json!(true)
        );
        assert_eq!(contract["must_avoid_commentary_only_updates"], json!(true));
        assert_eq!(
            contract["must_batch_all_tool_reads_before_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_reuse_latest_live_diagnostics_before_reread"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_repeated_live_guard_polls_without_new_delta"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_serial_same_thread_host_control_retries_without_effect_delta"],
            json!(true)
        );
        assert_eq!(
            contract["must_prefer_single_same_thread_control_then_measure"],
            json!(true)
        );
        assert_eq!(
            contract["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_progress_reply_when_only_guard_changed"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_new_tool_turn_without_specific_delta_goal"],
            json!(true)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(0));
        assert_eq!(contract["max_bullets_soft"], json!(0));
        assert_eq!(contract["max_sentences_soft"], json!(1));
    }

    #[test]
    fn compact_reply_budget_contract_keeps_one_roundtrip_for_critical_regrowth_without_target_pressure()
     {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::CriticalRegrowth,
            false,
            false,
        );
        assert_eq!(
            contract["must_require_material_delta_before_next_reply"],
            json!(false)
        );
        assert_eq!(
            contract["must_avoid_progress_reply_when_only_guard_changed"],
            json!(false)
        );
        assert_eq!(
            contract["must_avoid_new_tool_turn_without_specific_delta_goal"],
            json!(false)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(1));
    }

    #[test]
    fn compact_reply_budget_contract_marks_pure_burn_turn_in_critical_regrowth() {
        let contract = build_client_reply_budget_contract_with_target(
            ClientReplyBudgetMode::CompactHighSignal,
            60,
            HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
        );
        assert_eq!(contract["current_live_turn_no_amai_activity"], json!(true));
        assert_eq!(contract["same_meter_pure_burn_turn_active"], json!(true));
        assert_eq!(
            contract["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_progress_reply_when_only_guard_changed"],
            json!(true)
        );
        assert_eq!(
            contract["must_avoid_new_tool_turn_without_specific_delta_goal"],
            json!(true)
        );
        assert_eq!(contract["max_tool_roundtrips_soft"], json!(0));
        assert!(
            contract["summary"]
                .as_str()
                .expect("summary")
                .contains("no_amai_activity_in_current_live_turn")
        );
    }

    #[test]
    fn normalize_recorded_path_trims_trailing_markup() {
        assert_eq!(
            normalize_recorded_path("/home/art/Art/scripts/tools/amai_art_continuity_refresh.sh`"),
            "/home/art/Art/scripts/tools/amai_art_continuity_refresh.sh"
        );
    }

    #[test]
    fn extract_materialized_notes_prefers_bullets() {
        let notes = extract_materialized_notes(
            "Сделан слой.\n- Первый важный результат.\n- Второй важный результат.\nПочему так?\n",
        );
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], "Первый важный результат.");
    }

    #[test]
    fn extract_materialized_notes_ignores_headings_without_bullets() {
        let notes = extract_materialized_notes(
            "Что сделано:\nTemporal import теперь получает compact summary upstream.\nПочему это важно:\nОтветы стали короче.\n",
        );
        assert_eq!(notes.len(), 2);
        assert_eq!(
            notes[0],
            "Temporal import теперь получает compact summary upstream."
        );
    }

    #[test]
    fn extract_required_task_set_prefers_checkbox_tasks() {
        let tasks = extract_required_task_set(
            "Задачи:\n- [ ] Починить забывание задач\n- [x] Не трогать готовые контуры\n- [ ] Добавить обязательный список\n",
        );
        assert_eq!(
            tasks,
            vec![
                "Починить забывание задач".to_string(),
                "Добавить обязательный список".to_string()
            ]
        );
    }

    #[test]
    fn compose_restore_bundle_filters_noisy_multiline_open_questions() {
        let noisy = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Amai upstream thread-index enrich materialized",
            next_step_hint: "Сделать auto-injection restore pack прямо в chat-start prompt.",
            summary: "Materialized upstream temporal continuity enrich-path.",
            offset: 3,
        });
        let clean = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "retrieval_context_pack",
                    "headline": "Рабочий запрос: startup restore pack",
                    "next_step_hint": "Проверить новый чат.",
                    "summary": "Проверили startup restore pack.",
                    "recorded_at_epoch_ms": 4,
                    "open_questions": [
                        "Как сделать auto-injection без дополнительного helper-обхода?",
                        "Materialized upstream temporal continuity enrich-path.\n\nЧто сделано:\n- шумный блок"
                    ],
                    "materialized_notes": [
                        "Materialized upstream temporal continuity enrich-path.",
                        "Что сделано:"
                    ]
                }
            }),
            created_at_epoch_ms: 4,
        };

        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[clean, noisy],
        );
        let open_questions = bundle["working_state_restore"]["open_questions"]
            .as_array()
            .expect("open questions array");
        assert_eq!(open_questions.len(), 1);
        assert_eq!(
            open_questions[0],
            json!("Как сделать auto-injection без дополнительного helper-обхода?")
        );
    }

    #[test]
    fn select_relevant_events_prefers_exact_agent_scope() {
        let exact = fake_snapshot("art", "continuity", "art::primary", "session-a", "exact", 2);
        let shared = fake_snapshot("art", "continuity", "shared", "session-b", "shared", 1);
        let selected = select_relevant_events(
            vec![exact.clone(), shared],
            "art",
            "continuity",
            "art::primary",
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(
            selected[0].payload["working_state_event"]["headline"],
            json!("exact")
        );
    }

    #[test]
    fn select_relevant_events_does_not_mix_other_agent_scope_when_shared_missing() {
        let foreign = fake_snapshot(
            "art",
            "continuity",
            "art::secondary",
            "session-b",
            "foreign",
            1,
        );
        let selected = select_relevant_events(vec![foreign], "art", "continuity", "art::primary");
        assert!(selected.is_empty());
    }

    #[test]
    fn select_relevant_events_fail_closed_when_latest_session_id_missing() {
        let missing = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::primary",
            session_id: "",
            event_kind: "retrieval_context_pack",
            headline: "latest-without-session",
            next_step_hint: "",
            summary: "",
            offset: 5,
        });
        let older = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::primary",
            session_id: "",
            event_kind: "retrieval_context_pack",
            headline: "older-without-session",
            next_step_hint: "",
            summary: "",
            offset: 4,
        });
        let selected = select_relevant_events(
            vec![missing.clone(), older],
            "art",
            "continuity",
            "art::primary",
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(
            selected[0].payload["working_state_event"]["headline"],
            json!("latest-without-session")
        );
    }

    #[test]
    fn compose_restore_bundle_ignores_meta_continuity_handoff() {
        let meta = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Continuity restored and reported for new chat",
            next_step_hint: "Ждать указание пользователя",
            summary: "Пользователь спросил, на чём остановились",
            offset: 3,
        });
        let real = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Amai startup restore pack enriched and committed",
            next_step_hint: "Сделать auto-injection restore pack прямо в chat-start prompt.",
            summary: "Materialized working-state recovery contour.",
            offset: 2,
        });
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[meta, real],
        );
        assert_eq!(
            bundle["working_state_restore"]["current_goal"],
            json!("Amai startup restore pack enriched and committed")
        );
    }

    #[test]
    fn compose_restore_bundle_tracks_execution_states_and_lineage() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Authoritative handoff",
            next_step_hint: "Ship the next change.",
            summary: "Materialized authoritative result.",
            offset: base,
        });
        let older_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Older handoff",
            next_step_hint: "Do older thing.",
            summary: "Superseded result.",
            offset: base - 1,
        });
        let retrieval = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "retrieval_context_pack",
            headline: "Рабочий запрос: current context",
            next_step_hint: "Inspect file.",
            summary: "Attempted retrieval.",
            offset: base - 2,
        });
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff, older_handoff, retrieval],
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(restore["next_step_state"], json!("planned"));
        assert_eq!(
            restore["state_lineage"]["authoritative_event_kind"],
            json!("continuity_handoff")
        );
        assert_eq!(
            restore["state_lineage"]["lineage_model_version"],
            json!("lineage-v2")
        );
        assert_eq!(restore["action_state_counts"]["succeeded"], json!(1));
        assert_eq!(restore["action_state_counts"]["superseded"], json!(1));
        assert_eq!(
            restore["recent_actions"][0]["execution_state"],
            json!("succeeded")
        );
        assert_eq!(
            restore["recent_actions"][1]["execution_state"],
            json!("superseded")
        );
        assert_eq!(
            restore["recent_actions"][2]["execution_state"],
            json!("attempted")
        );
        let edges = restore["state_lineage"]["edges"].as_array().expect("edges");
        assert_eq!(edges.len(), 2);
        assert!(edges.iter().any(|edge| {
            edge["from_event_id"]
                .as_str()
                .is_some_and(|value| value.starts_with("Older handoff-"))
                && edge["relation"] == json!("superseded_by")
        }));
        assert!(edges.iter().any(|edge| {
            edge["from_event_id"]
                .as_str()
                .is_some_and(|value| value.starts_with("Рабочий запрос: current context-"))
                && edge["relation"] == json!("supports")
        }));
    }

    #[test]
    fn restore_snapshot_matches_latest_relevant_events_accepts_current_snapshot() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Project relocation contour",
            next_step_hint: "Dovetail runtime auto-start guarantees.",
            summary: "Relocation contour materialized.",
            offset: base,
        });
        let older_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Same-meter spend control",
            next_step_hint: "Materialize live assistant generation source.",
            summary: "Older handoff.",
            offset: base - 1,
        });
        let events = vec![latest_handoff, older_handoff];
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &events,
        );

        assert!(restore_snapshot_matches_latest_relevant_events(
            &bundle, &events
        ));
    }

    #[test]
    fn restore_snapshot_matches_latest_relevant_events_rejects_latest_event_drift() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let older_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Same-meter spend control",
            next_step_hint: "Materialize live assistant generation source.",
            summary: "Older handoff.",
            offset: base - 1,
        });
        let stale_bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            std::slice::from_ref(&older_handoff),
        );
        let latest_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Project relocation contour",
            next_step_hint: "Dovetail runtime auto-start guarantees.",
            summary: "Relocation contour materialized.",
            offset: base,
        });
        let events = vec![latest_handoff, older_handoff];

        assert!(!restore_snapshot_matches_latest_relevant_events(
            &stale_bundle,
            &events
        ));
    }

    #[test]
    fn restore_snapshot_matches_latest_relevant_events_rejects_authoritative_drift() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let retrieval = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "retrieval_context_pack",
            headline: "Inspect startup state",
            next_step_hint: "Check restore bundle.",
            summary: "Retrieved current state.",
            offset: base,
        });
        let older_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Same-meter spend control",
            next_step_hint: "Materialize live assistant generation source.",
            summary: "Older handoff.",
            offset: base - 1,
        });
        let stale_bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[retrieval.clone(), older_handoff.clone()],
        );
        let newer_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Project relocation contour",
            next_step_hint: "Dovetail runtime auto-start guarantees.",
            summary: "Relocation contour materialized.",
            offset: base - 0,
        });
        let events = vec![retrieval, newer_handoff, older_handoff];

        assert!(!restore_snapshot_matches_latest_relevant_events(
            &stale_bundle,
            &events
        ));
    }

    #[test]
    fn restore_snapshot_matches_latest_relevant_events_rejects_empty_event_set() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Project relocation contour",
            next_step_hint: "Dovetail runtime auto-start guarantees.",
            summary: "Relocation contour materialized.",
            offset: base,
        });
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff],
        );

        assert!(!restore_snapshot_matches_latest_relevant_events(
            &bundle,
            &[]
        ));
    }

    #[test]
    fn derive_pending_return_queue_captures_interrupted_previous_line() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Same-meter spend control",
                "next_step": "Materialize live assistant generation source.",
                "pending_return_queue": [
                    {
                        "headline": "Older suspended line",
                        "next_step": "Return there later.",
                        "queued_at_epoch_ms": 5,
                        "resume_state": "pending_return"
                    }
                ],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "Project relocation contour",
            "Document automatic startup behavior.",
            42,
            false,
            &[],
            &[],
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0]["headline"], json!("Same-meter spend control"));
        assert_eq!(
            queue[0]["next_step"],
            json!("Materialize live assistant generation source.")
        );
        assert_eq!(
            queue[0]["queued_reason"],
            json!("interrupted_by_new_handoff")
        );
        assert_eq!(queue[0]["resume_state"], json!("pending_return"));
        assert_eq!(queue[0]["authoritative_event_id"], json!("event-123"));
        assert_eq!(queue[1]["headline"], json!("Older suspended line"));
    }

    #[test]
    fn restore_snapshot_matches_latest_relevant_events_rejects_focus_or_source_drift() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let retrieval = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "retrieval-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Рабочий запрос: Continuity snapshot",
                    "summary": "Continuity snapshot Найдено: документов 1, символов 0, lexical chunks 0, semantic chunks 0.",
                    "recorded_at_epoch_ms": base,
                    "active_files": [".amai-continuity/bootstrap/continuity-snapshot.md"],
                    "visible_projects": ["art"],
                    "query": "Continuity snapshot",
                    "current_hypothesis": "Вероятный рабочий контекст сейчас лежит в: .amai-continuity/bootstrap/continuity-snapshot.md",
                    "last_results_summary": "Найдено: документов 1, символов 0, lexical chunks 0, semantic chunks 0."
                }
            }),
            created_at_epoch_ms: base,
        };
        let handoff = fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code: "art",
            namespace_code: "continuity",
            agent_scope: "art::continuity::default",
            session_id: "session-a",
            event_kind: "continuity_handoff",
            headline: "Amai continuity migration proof",
            next_step_hint: "Убедиться, что startup summary и retrieval уже живут без project .codex",
            summary: "Relocation contour materialized.",
            offset: base + 1,
        });
        let events = vec![handoff.clone(), retrieval.clone()];
        let mut bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &events,
        );
        bundle["working_state_restore"]["current_focus"] = Value::Null;
        bundle["working_state_restore"]["source_summary"] = Value::Null;

        assert!(!restore_snapshot_matches_latest_relevant_events(
            &bundle, &events
        ));
    }

    #[test]
    fn derive_pending_return_queue_skips_placeholder_previous_line() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "ещё нет данных",
                "next_step": "ещё нет данных",
                "pending_return_queue": [],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "Project relocation contour",
            "Document automatic startup behavior.",
            42,
            false,
            &[],
            &[],
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn derive_pending_return_queue_skips_generic_continue_workline_placeholder() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Продолжить активную рабочую линию",
                "next_step": "продолжить работу в свежем чате через continuity startup",
                "pending_return_queue": [],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "Project relocation contour",
            "Document automatic startup behavior.",
            42,
            false,
            &[],
            &[],
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn derive_pending_return_queue_can_resolve_current_goal_without_requeue() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Amai continuity migration proof",
                "next_step": "Убедиться, что startup summary и retrieval уже живут без project .codex",
                "pending_return_queue": [
                    {
                        "headline": "Older suspended line",
                        "next_step": "Return there later.",
                        "queued_at_epoch_ms": 5,
                        "resume_state": "pending_return"
                    }
                ],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "Art continuity proof contour green on source-current startup gate",
            "Resume the next pending-return contour now that Art startup/migration proofs and client-budget gate semantics are green at commit 30feca3.",
            42,
            true,
            &[],
            &[],
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["headline"], json!("Older suspended line"));
    }

    #[test]
    fn derive_pending_return_queue_prunes_explicitly_resolved_headlines() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Current active line",
                "next_step": "Continue active work.",
                "pending_return_queue": [
                    {
                        "headline": "Amai continuity migration proof",
                        "next_step": "Убедиться, что startup summary и retrieval уже живут без project .codex",
                        "queued_at_epoch_ms": 5,
                        "resume_state": "pending_return"
                    },
                    {
                        "headline": "Soft rotate recommendation no longer hard-blocks replies",
                        "next_step": "Verify startup summary and retrieval keep advisory rotate as note-only without falling back to project .codex",
                        "queued_at_epoch_ms": 4,
                        "resume_state": "pending_return"
                    }
                ],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let resolved = vec![
            "Amai continuity migration proof".to_string(),
            "Soft rotate recommendation no longer hard-blocks replies".to_string(),
        ];
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "ExecCtl stale pending-return closure semantics materialized",
            "Recheck Art startup queue after explicit resolve path.",
            42,
            false,
            &resolved,
            &[],
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["headline"], json!("Current active line"));
    }

    #[test]
    fn derive_pending_return_queue_prunes_explicitly_resolved_task_ids() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Current active line",
                "next_step": "Continue active work.",
                "pending_return_queue": [
                    {
                        "task_id": "task::event-111",
                        "headline": "Amai continuity migration proof",
                        "next_step": "Убедиться, что startup summary и retrieval уже живут без project .codex",
                        "queued_at_epoch_ms": 5,
                        "resume_state": "pending_return",
                        "authoritative_event_id": "event-111"
                    }
                ],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let resolved_task_ids = vec!["task::event-111".to_string()];
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "ExecCtl stale pending-return closure semantics materialized",
            "Recheck Art startup queue after explicit resolve path.",
            42,
            false,
            &[],
            &resolved_task_ids,
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["task_id"], json!("task::event-123"));
        assert_eq!(queue[0]["headline"], json!("Current active line"));
    }

    #[test]
    fn derive_pending_return_queue_prunes_only_matching_duplicate_headline_task_id() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Current active line",
                "next_step": "Continue active work.",
                "pending_return_queue": [
                    {
                        "task_id": "task::event-111",
                        "headline": "Shared headline",
                        "next_step": "First incarnation.",
                        "queued_at_epoch_ms": 7,
                        "resume_state": "pending_return",
                        "authoritative_event_id": "event-111"
                    },
                    {
                        "task_id": "task::event-222",
                        "headline": "Shared headline",
                        "next_step": "Second incarnation.",
                        "queued_at_epoch_ms": 6,
                        "resume_state": "pending_return",
                        "authoritative_event_id": "event-222"
                    },
                    {
                        "task_id": "task::event-333",
                        "headline": "Different suspended line",
                        "next_step": "Keep this too.",
                        "queued_at_epoch_ms": 5,
                        "resume_state": "pending_return",
                        "authoritative_event_id": "event-333"
                    }
                ],
                "state_lineage": {
                    "authoritative_event_id": "event-999",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let resolved_task_ids = vec!["event-222".to_string()];
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "New active line",
            "Verify identity-based cleanup.",
            42,
            false,
            &[],
            &resolved_task_ids,
        );
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0]["task_id"], json!("task::event-999"));
        let shared = queue
            .iter()
            .filter(|item| item["headline"] == json!("Shared headline"))
            .collect::<Vec<_>>();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0]["task_id"], json!("task::event-111"));
        assert!(
            queue
                .iter()
                .all(|item| item["task_id"] != json!("task::event-222"))
        );
    }

    #[test]
    fn derive_pending_return_queue_skips_requeue_when_current_goal_resolved_by_task_id() {
        let restore = json!({
            "working_state_restore": {
                "current_goal": "Identity-based resolved current line",
                "next_step": "Close this line before switching.",
                "pending_return_queue": [
                    {
                        "task_id": "task::event-111",
                        "headline": "Older suspended line",
                        "next_step": "Return there later.",
                        "queued_at_epoch_ms": 5,
                        "resume_state": "pending_return",
                        "authoritative_event_id": "event-111"
                    }
                ],
                "state_lineage": {
                    "authoritative_event_id": "event-123",
                    "authoritative_event_kind": "continuity_handoff",
                    "authoritative_local_path": "/home/art/agent-memory-index"
                }
            }
        });
        let resolved_task_ids = vec!["task::event-123".to_string()];
        let queue = derive_pending_return_queue(
            Some(&restore["working_state_restore"]),
            "New active line",
            "Do not requeue the resolved current line.",
            42,
            false,
            &[],
            &resolved_task_ids,
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["task_id"], json!("task::event-111"));
        assert!(
            queue
                .iter()
                .all(|item| item["headline"] != json!("Identity-based resolved current line"))
        );
    }

    #[test]
    fn compose_restore_bundle_surfaces_pending_return_queue() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "continuity_handoff",
                    "headline": "Project relocation contour",
                    "next_step_hint": "Dovetail runtime auto-start guarantees.",
                    "summary": "Relocation contour materialized.",
                    "recorded_at_epoch_ms": base,
                    "pending_return_queue": [
                        {
                            "headline": "Same-meter spend control",
                            "next_step": "Materialize live assistant generation source.",
                            "queued_at_epoch_ms": base - 1,
                            "resume_state": "pending_return",
                            "queued_reason": "interrupted_by_new_handoff"
                        }
                    ]
                }
            }),
            created_at_epoch_ms: base,
        };
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff],
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(
            restore["execctl_resume_state"],
            json!("pending_return_queue_present")
        );
        assert_eq!(
            restore["execctl_resume_contract"]["resume_state"],
            json!("return_required")
        );
        assert_eq!(
            restore["pending_return_queue"][0]["headline"],
            json!("Same-meter spend control")
        );
        assert!(
            restore["pending_return_summary"]
                .as_str()
                .is_some_and(|value| value.contains("Same-meter spend control"))
        );
        assert!(
            restore["execctl_resume_contract_summary"]
                .as_str()
                .is_some_and(|value| value.contains("return_required(1)"))
        );
        assert_eq!(
            restore["startup_next_action"]["action_kind"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            restore["startup_next_action"]["headline"],
            json!("Same-meter spend control")
        );
        assert!(
            restore["startup_next_action_summary"]
                .as_str()
                .is_some_and(|value| value.contains("resume_required_return_task"))
        );
        assert_eq!(
            restore["project_task_tree"]["tree_version"],
            json!("project-task-tree-v1")
        );
        assert_eq!(restore["project_task_tree"]["open_tasks_count"], json!(2));
        assert_eq!(
            restore["project_task_tree"]["nodes"][0]["task_role"],
            json!("active")
        );
        assert_eq!(
            restore["project_task_tree"]["nodes"][1]["task_role"],
            json!("pending_return")
        );
        assert!(
            restore["project_task_tree_summary"]
                .as_str()
                .is_some_and(|value| value.contains("pending_return(1)"))
        );
        assert_eq!(
            restore["project_task_ledger"]["ledger_version"],
            json!("project-task-ledger-v2")
        );
        assert_eq!(restore["project_task_ledger"]["open_tasks_count"], json!(2));
        assert_eq!(
            restore["project_task_ledger"]["historical_handoffs_count"],
            json!(0)
        );
        assert_eq!(
            restore["project_task_ledger"]["persistence_state"],
            json!("restore_side_only")
        );
        assert_eq!(
            restore["project_task_ledger"]["entries"][0]["task_role"],
            json!("active")
        );
        assert!(
            restore["project_task_ledger_summary"]
                .as_str()
                .is_some_and(|value| value.contains("historical_handoffs(0)"))
        );
    }

    #[test]
    fn compose_restore_bundle_surfaces_required_task_set_as_startup_action() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-2",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "continuity_handoff",
                    "headline": "UI cleanup required",
                    "next_step_hint": "Close all pending UI gaps.",
                    "summary": "User asked for multi-task UI fixes.",
                    "recorded_at_epoch_ms": base,
                    "required_task_set": [
                        "Fix card A",
                        "Fix card B"
                    ],
                    "pending_return_queue": []
                }
            }),
            created_at_epoch_ms: base,
        };
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff],
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(
            restore["required_task_set"],
            json!(["Fix card A", "Fix card B"])
        );
        assert_eq!(
            restore["startup_next_action"]["action_kind"],
            json!("honor_required_task_set")
        );
        assert!(
            restore["startup_next_action"]["blocking"]
                .as_bool()
                .unwrap_or(false)
        );
    }

    #[test]
    fn compose_restore_bundle_drops_placeholder_pending_return_queue() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "continuity_handoff",
                    "headline": "Project relocation contour",
                    "next_step_hint": "Dovetail runtime auto-start guarantees.",
                    "summary": "Relocation contour materialized.",
                    "recorded_at_epoch_ms": base,
                    "pending_return_queue": [
                        {
                            "headline": "ещё нет данных",
                            "next_step": "ещё нет данных",
                            "queued_at_epoch_ms": base - 1,
                            "resume_state": "pending_return",
                            "queued_reason": "interrupted_by_new_handoff"
                        }
                    ]
                }
            }),
            created_at_epoch_ms: base,
        };
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff],
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(restore["execctl_resume_state"], json!("clear"));
        assert_eq!(
            restore["execctl_resume_contract"]["resume_state"],
            json!("clear")
        );
        assert_eq!(restore["pending_return_queue"], json!([]));
        assert_eq!(restore["pending_return_summary"], Value::Null);
        assert_eq!(restore["execctl_resume_contract_summary"], json!("clear"));
        assert_eq!(
            restore["startup_next_action"]["action_kind"],
            json!("continue_active_workline")
        );
    }

    #[test]
    fn compose_restore_bundle_derives_current_focus_and_source_summary() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let retrieval_event = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "retrieval-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Рабочий запрос: Continuity snapshot",
                    "summary": "Continuity snapshot Найдено: документов 1, символов 0, lexical chunks 0, semantic chunks 0.",
                    "recorded_at_epoch_ms": base - 100,
                    "active_files": [".amai-continuity/bootstrap/continuity-snapshot.md"],
                    "visible_projects": ["art"],
                    "query": "Continuity snapshot",
                    "current_hypothesis": "Вероятный рабочий контекст сейчас лежит в: .amai-continuity/bootstrap/continuity-snapshot.md",
                    "last_results_summary": "Найдено: документов 1, символов 0, lexical chunks 0, semantic chunks 0.",
                    "decision_trace": {
                        "included": [
                            {
                                "label": "точные совпадения",
                                "summary": "Нашлись точные document/path совпадения внутри видимого контура."
                            }
                        ]
                    }
                }
            }),
            created_at_epoch_ms: base - 100,
        };
        let latest_handoff = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "continuity_handoff",
                    "source_kind": "continuity_handoff",
                    "headline": "Amai continuity migration proof",
                    "next_step_hint": "Убедиться, что startup summary и retrieval уже живут без project .codex",
                    "summary": "Relocation contour materialized.",
                    "recorded_at_epoch_ms": base,
                    "local_path": "/tmp/live-handoff.md",
                    "pending_return_queue": []
                }
            }),
            created_at_epoch_ms: base,
        };
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff, retrieval_event],
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(
            restore["current_focus"],
            json!(
                "Сейчас основной фокус: .amai-continuity/bootstrap/continuity-snapshot.md по запросу «Continuity snapshot»."
            )
        );
        assert!(restore["source_summary"].as_str().is_some_and(|value| {
            value.contains("Источник истины: continuity_handoff (/tmp/live-handoff.md).")
        }));
        assert!(
            restore["source_summary"]
                .as_str()
                .is_some_and(|value| value.contains("Главный подтверждающий артефакт: .amai-continuity/bootstrap/continuity-snapshot.md."))
        );
    }

    #[test]
    fn overlay_durable_project_task_ledger_prefers_postgres_entries() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "continuity_handoff",
                    "headline": "Project relocation contour",
                    "next_step_hint": "Dovetail runtime auto-start guarantees.",
                    "summary": "Relocation contour materialized.",
                    "recorded_at_epoch_ms": base,
                    "pending_return_queue": [
                        {
                            "headline": "Same-meter spend control",
                            "next_step": "Materialize live assistant generation source.",
                            "queued_at_epoch_ms": base - 1,
                            "resume_state": "pending_return",
                            "queued_reason": "interrupted_by_new_handoff"
                        }
                    ]
                }
            }),
            created_at_epoch_ms: base,
        };
        let mut bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff],
        );
        let durable_entries = vec![ExecCtlTaskLedgerEntryRecord {
            ledger_entry_id: Uuid::new_v4(),
            source_snapshot_id: Some(Uuid::new_v4()),
            source_event_id: "handoff-1".to_string(),
            event_kind: "continuity_handoff".to_string(),
            source_kind: "continuity_handoff".to_string(),
            agent_scope: "art::continuity::default".to_string(),
            session_id: Some("session-a".to_string()),
            thread_id: None,
            headline: "Project relocation contour".to_string(),
            next_step: "Dovetail runtime auto-start guarantees.".to_string(),
            summary: "Relocation contour materialized.".to_string(),
            active_files: json!(["/home/art/agent-memory-index/src/continuity.rs"]),
            open_questions: json!(["How to enforce auto-start?"]),
            materialized_notes: json!(["Relocation contour materialized."]),
            pending_return_queue: json!([
                {
                    "headline": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source.",
                    "queued_at_epoch_ms": base - 1,
                    "resume_state": "pending_return",
                    "queued_reason": "interrupted_by_new_handoff"
                }
            ]),
            local_path: Some(
                "/home/art/agent-memory-index/.amai-continuity/live-handoff/HANDOFF.md".to_string(),
            ),
            recorded_at_epoch_ms: base,
            created_at_epoch_ms: base,
        }];

        overlay_durable_project_task_ledger(
            &mut bundle,
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &durable_entries,
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(
            restore["project_task_ledger"]["ledger_version"],
            json!("project-task-ledger-v2")
        );
        assert_eq!(
            restore["project_task_ledger"]["persistence_state"],
            json!("durable_postgres")
        );
        assert_eq!(
            restore["project_task_ledger"]["storage_lane"],
            json!("ami.execctl_task_ledger_entries")
        );
        assert_eq!(
            restore["project_task_ledger"]["entries"][0]["source_snapshot_id"]
                .as_str()
                .is_some(),
            true
        );
        assert_eq!(
            restore["project_task_ledger"]["entries"][0]["task_role"],
            json!("active")
        );
    }

    #[test]
    fn overlay_execctl_active_lease_surfaces_current_owner() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_handoff = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-1",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "continuity_handoff",
                    "headline": "Project relocation contour",
                    "next_step_hint": "Dovetail runtime auto-start guarantees.",
                    "summary": "Relocation contour materialized.",
                    "recorded_at_epoch_ms": base
                }
            }),
            created_at_epoch_ms: base,
        };
        let mut bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_handoff],
        );
        let lease = ExecCtlTaskLeaseRecord {
            lease_id: Uuid::new_v4(),
            source_snapshot_id: Some(Uuid::new_v4()),
            source_event_id: "handoff-1".to_string(),
            source_kind: "continuity_handoff".to_string(),
            agent_scope: "art::continuity::default".to_string(),
            owner_session_id: Some("session-a".to_string()),
            owner_thread_id: Some("thread-a".to_string()),
            lease_state: "active".to_string(),
            headline: "Project relocation contour".to_string(),
            next_step: "Dovetail runtime auto-start guarantees.".to_string(),
            local_path: Some("/tmp/HANDOFF.md".to_string()),
            acquired_at_epoch_ms: base,
            heartbeat_at_epoch_ms: base,
            expires_at_epoch_ms: base + 30_000,
            created_at_epoch_ms: base,
            updated_at_epoch_ms: base,
        };

        overlay_execctl_active_lease(&mut bundle, Some(&lease));
        let restore = &bundle["working_state_restore"];
        assert_eq!(
            restore["execctl_active_lease"]["lease_owner_state"],
            json!("same_session_owner")
        );
        assert_eq!(
            restore["execctl_active_lease"]["headline"],
            json!("Project relocation contour")
        );
        assert_eq!(
            restore["execctl_active_lease"]["storage_lane"],
            json!("ami.execctl_task_leases")
        );
        assert_eq!(restore["thread_id"], json!("thread-a"));
        assert!(
            restore["execctl_active_lease_summary"]
                .as_str()
                .is_some_and(|value| value.contains("same_session_owner"))
        );
    }

    #[test]
    fn startup_same_thread_lease_refresh_candidate_prefers_active_task_identity() {
        let restore = json!({
            "working_state_restore": {
                "thread_id": "thread-a",
                "current_goal": "Less precise fallback goal",
                "next_step": "less precise fallback next step",
                "state_lineage": {
                    "authoritative_event_id": "handoff-1",
                    "authoritative_source_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/HANDOFF.md"
                },
                "project_task_tree": {
                    "nodes": [
                        {
                            "task_role": "active",
                            "authoritative_event_id": "handoff-1",
                            "source_kind": "continuity_handoff",
                            "headline": "ExecCtl foundation hardening",
                            "next_step": "Stress interrupted handoff transitions."
                        }
                    ]
                },
                "project_task_ledger": {
                    "entries": []
                }
            }
        });

        let candidate =
            execctl_startup_same_thread_lease_refresh_candidate(&restore, Some("thread-a"))
                .expect("candidate");
        assert_eq!(candidate.source_event_id, "handoff-1");
        assert_eq!(candidate.source_kind, "continuity_handoff");
        assert_eq!(candidate.headline, "ExecCtl foundation hardening");
        assert_eq!(
            candidate.next_step,
            "Stress interrupted handoff transitions."
        );
        assert_eq!(candidate.local_path.as_deref(), Some("/tmp/HANDOFF.md"));
    }

    #[test]
    fn startup_same_thread_lease_refresh_candidate_accepts_rebind_to_new_thread() {
        let restore = json!({
            "working_state_restore": {
                "thread_id": "thread-b",
                "current_goal": "ExecCtl foundation hardening",
                "next_step": "Stress interrupted handoff transitions.",
                "state_lineage": {
                    "authoritative_event_id": "handoff-1",
                    "authoritative_source_kind": "continuity_handoff"
                },
                "project_task_tree": {
                    "nodes": [
                        {
                            "task_role": "active",
                            "authoritative_event_id": "handoff-1",
                            "headline": "ExecCtl foundation hardening",
                            "next_step": "Stress interrupted handoff transitions."
                        }
                    ]
                },
                "project_task_ledger": {
                    "entries": []
                }
            }
        });

        let candidate =
            execctl_startup_same_thread_lease_refresh_candidate(&restore, Some("thread-a"))
                .expect("candidate");
        assert_eq!(candidate.source_event_id, "handoff-1");
        assert_eq!(candidate.headline, "ExecCtl foundation hardening");
        assert_eq!(
            candidate.next_step,
            "Stress interrupted handoff transitions."
        );
    }

    #[test]
    fn startup_same_thread_lease_refresh_candidate_falls_back_to_state_lineage() {
        let restore = json!({
            "working_state_restore": {
                "thread_id": "thread-a",
                "current_goal": "ExecCtl foundation hardening",
                "next_step": "Stress interrupted handoff transitions.",
                "state_lineage": {
                    "authoritative_event_id": "handoff-2",
                    "authoritative_source_kind": "continuity_handoff",
                    "authoritative_local_path": "/tmp/FALLBACK.md"
                },
                "project_task_tree": {
                    "nodes": [
                        {
                            "task_role": "active",
                            "headline": "ExecCtl foundation hardening",
                            "next_step": "Stress interrupted handoff transitions."
                        }
                    ]
                },
                "project_task_ledger": {
                    "entries": [
                        {
                            "task_role": "active",
                            "local_path": "/tmp/LEDGER.md"
                        }
                    ]
                }
            }
        });

        let candidate =
            execctl_startup_same_thread_lease_refresh_candidate(&restore, Some("thread-a"))
                .expect("candidate");
        assert_eq!(candidate.source_event_id, "handoff-2");
        assert_eq!(candidate.source_kind, "continuity_handoff");
        assert_eq!(candidate.local_path.as_deref(), Some("/tmp/FALLBACK.md"));
    }

    #[test]
    fn compose_restore_bundle_infers_recent_thread_id_when_latest_events_are_blank() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_blank = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-blank",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "thread_id": "",
                    "event_kind": "continuity_handoff",
                    "source_kind": "continuity_handoff",
                    "headline": "Latest handoff without thread binding",
                    "next_step_hint": "Keep the same line alive.",
                    "summary": "Blank thread id.",
                    "recorded_at_epoch_ms": base + 2
                }
            }),
            created_at_epoch_ms: base + 2,
        };
        let previous_blank = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-blank-2",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "thread_id": "",
                    "event_kind": "continuity_handoff",
                    "source_kind": "continuity_handoff",
                    "headline": "Still blank",
                    "next_step_hint": "Still the same line.",
                    "summary": "Blank thread id again.",
                    "recorded_at_epoch_ms": base + 1
                }
            }),
            created_at_epoch_ms: base + 1,
        };
        let earlier_bound = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "ctx-thread",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "thread_id": "thread-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Thread-bound context pack",
                    "next_step_hint": "Keep the same line alive.",
                    "summary": "Thread-bound event.",
                    "recorded_at_epoch_ms": base
                }
            }),
            created_at_epoch_ms: base,
        };

        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_blank, previous_blank, earlier_bound],
        );

        assert_eq!(
            bundle["working_state_restore"]["thread_id"],
            json!("thread-a")
        );
    }

    #[test]
    fn compose_restore_bundle_keeps_thread_id_empty_when_recent_history_is_ambiguous() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let latest_blank = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "handoff-blank",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "thread_id": "",
                    "event_kind": "continuity_handoff",
                    "source_kind": "continuity_handoff",
                    "headline": "Latest handoff without thread binding",
                    "next_step_hint": "Keep the same line alive.",
                    "summary": "Blank thread id.",
                    "recorded_at_epoch_ms": base + 2
                }
            }),
            created_at_epoch_ms: base + 2,
        };
        let bound_a = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "ctx-thread-a",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "thread_id": "thread-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Thread A",
                    "next_step_hint": "Keep the same line alive.",
                    "summary": "Thread-bound event A.",
                    "recorded_at_epoch_ms": base + 1
                }
            }),
            created_at_epoch_ms: base + 1,
        };
        let bound_b = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "ctx-thread-b",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "thread_id": "thread-b",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Thread B",
                    "next_step_hint": "Keep the same line alive.",
                    "summary": "Thread-bound event B.",
                    "recorded_at_epoch_ms": base
                }
            }),
            created_at_epoch_ms: base,
        };

        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[latest_blank, bound_a, bound_b],
        );

        assert_eq!(bundle["working_state_restore"]["thread_id"], Value::Null);
    }

    #[test]
    fn execctl_same_thread_lease_refresh_required_for_missing_lease() {
        assert!(execctl_same_thread_lease_refresh_required(
            None,
            Some("thread-a"),
            10_000,
            5_000
        ));
    }

    #[test]
    fn execctl_same_thread_lease_refresh_required_rebinds_foreign_owner() {
        let lease = ExecCtlTaskLeaseRecord {
            lease_id: Uuid::new_v4(),
            source_snapshot_id: None,
            source_event_id: "handoff-1".to_string(),
            source_kind: "continuity_handoff".to_string(),
            agent_scope: "art::continuity::default".to_string(),
            owner_session_id: Some("session-a".to_string()),
            owner_thread_id: Some("thread-b".to_string()),
            lease_state: "active".to_string(),
            headline: "Live".to_string(),
            next_step: "Keep going.".to_string(),
            local_path: None,
            acquired_at_epoch_ms: 1_000,
            heartbeat_at_epoch_ms: 9_500,
            expires_at_epoch_ms: 20_000,
            created_at_epoch_ms: 1_000,
            updated_at_epoch_ms: 9_500,
        };

        assert!(execctl_same_thread_lease_refresh_required(
            Some(&lease),
            Some("thread-a"),
            10_000,
            5_000
        ));
    }

    #[test]
    fn execctl_same_thread_lease_refresh_required_waits_while_heartbeat_is_fresh() {
        let lease = ExecCtlTaskLeaseRecord {
            lease_id: Uuid::new_v4(),
            source_snapshot_id: None,
            source_event_id: "handoff-1".to_string(),
            source_kind: "continuity_handoff".to_string(),
            agent_scope: "art::continuity::default".to_string(),
            owner_session_id: Some("session-a".to_string()),
            owner_thread_id: Some("thread-a".to_string()),
            lease_state: "active".to_string(),
            headline: "Live".to_string(),
            next_step: "Keep going.".to_string(),
            local_path: None,
            acquired_at_epoch_ms: 1_000,
            heartbeat_at_epoch_ms: 9_500,
            expires_at_epoch_ms: 20_000,
            created_at_epoch_ms: 1_000,
            updated_at_epoch_ms: 9_500,
        };

        assert!(!execctl_same_thread_lease_refresh_required(
            Some(&lease),
            Some("thread-a"),
            10_000,
            5_000
        ));
    }

    #[test]
    fn execctl_same_thread_lease_refresh_required_triggers_on_stale_heartbeat() {
        let lease = ExecCtlTaskLeaseRecord {
            lease_id: Uuid::new_v4(),
            source_snapshot_id: None,
            source_event_id: "handoff-1".to_string(),
            source_kind: "continuity_handoff".to_string(),
            agent_scope: "art::continuity::default".to_string(),
            owner_session_id: Some("session-a".to_string()),
            owner_thread_id: Some("thread-a".to_string()),
            lease_state: "active".to_string(),
            headline: "Live".to_string(),
            next_step: "Keep going.".to_string(),
            local_path: None,
            acquired_at_epoch_ms: 1_000,
            heartbeat_at_epoch_ms: 4_000,
            expires_at_epoch_ms: 20_000,
            created_at_epoch_ms: 1_000,
            updated_at_epoch_ms: 4_000,
        };

        assert!(execctl_same_thread_lease_refresh_required(
            Some(&lease),
            Some("thread-a"),
            10_000,
            5_000
        ));
    }

    #[test]
    fn execctl_same_thread_lease_refresh_required_triggers_when_expiry_is_near() {
        let lease = ExecCtlTaskLeaseRecord {
            lease_id: Uuid::new_v4(),
            source_snapshot_id: None,
            source_event_id: "handoff-1".to_string(),
            source_kind: "continuity_handoff".to_string(),
            agent_scope: "art::continuity::default".to_string(),
            owner_session_id: Some("session-a".to_string()),
            owner_thread_id: Some("thread-a".to_string()),
            lease_state: "active".to_string(),
            headline: "Live".to_string(),
            next_step: "Keep going.".to_string(),
            local_path: None,
            acquired_at_epoch_ms: 1_000,
            heartbeat_at_epoch_ms: 9_500,
            expires_at_epoch_ms: 12_000,
            created_at_epoch_ms: 1_000,
            updated_at_epoch_ms: 9_500,
        };

        assert!(execctl_same_thread_lease_refresh_required(
            Some(&lease),
            Some("thread-a"),
            10_000,
            5_000
        ));
    }

    #[test]
    fn compose_restore_bundle_merges_workspace_graphs_from_recent_actions() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let retrieval_a = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "retrieval-a",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Graph A",
                    "summary": "Graph A",
                    "recorded_at_epoch_ms": base,
                    "workspace_graph": {
                        "workspace_graph_model_version": "workspace-graph-v10",
                        "artifact_lineage_model_version": "artifact-lineage-v1",
                        "lineage_model_version": "lineage-v2",
                        "truth_ranking": ["continuity_handoff"],
                        "scope_signature": "scope-a",
                        "visible_projects": [{"project_code":"art","namespace_code":"continuity"}],
                        "source_context_pack_ids": ["ctx-a"],
                        "nodes": [
                            {"node_id":"file:art:continuity:src/lib.rs","node_type":"file"}
                        ],
                        "edges": [
                            {"from_node_id":"context_pack:ctx-a","to_node_id":"file:art:continuity:src/lib.rs","relation":"retrieved_exact_document"}
                        ]
                    }
                }
            }),
            created_at_epoch_ms: base,
        };
        let retrieval_b = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "retrieval-b",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Graph B",
                    "summary": "Graph B",
                    "recorded_at_epoch_ms": base - 1,
                    "workspace_graph": {
                        "workspace_graph_model_version": "workspace-graph-v10",
                        "artifact_lineage_model_version": "artifact-lineage-v1",
                        "lineage_model_version": "lineage-v2",
                        "truth_ranking": ["continuity_handoff"],
                        "scope_signature": "scope-b",
                        "visible_projects": [{"project_code":"art","namespace_code":"continuity"}],
                        "source_context_pack_ids": ["ctx-b"],
                        "nodes": [
                            {"node_id":"file:art:continuity:src/lib.rs","node_type":"file"},
                            {"node_id":"symbol:art:continuity:src/lib.rs:alpha:1","node_type":"symbol"}
                        ],
                        "edges": [
                            {"from_node_id":"file:art:continuity:src/lib.rs","to_node_id":"symbol:art:continuity:src/lib.rs:alpha:1","relation":"contains_symbol"}
                        ]
                    }
                }
            }),
            created_at_epoch_ms: base - 1,
        };
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[retrieval_a, retrieval_b],
        );
        let graph = &bundle["working_state_restore"]["workspace_graph"];
        assert_eq!(
            graph["source_context_pack_ids"].as_array().unwrap().len(),
            2
        );
        assert_eq!(graph["scope_signatures"].as_array().unwrap().len(), 2);
        assert_eq!(graph["summary"]["node_counts"]["file"], json!(1));
        assert_eq!(graph["summary"]["node_counts"]["symbol"], json!(1));
        assert_eq!(graph["summary"]["edge_count"], json!(2));
    }

    #[test]
    fn compose_restore_bundle_carries_recent_decision_trace_summary() {
        let base = now_epoch_ms().unwrap_or(1_000_000) as i64;
        let retrieval = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": "retrieval-decision",
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_id": "session-a",
                    "event_kind": "retrieval_context_pack",
                    "source_kind": "context_pack",
                    "headline": "Рабочий запрос: current context",
                    "summary": "Attempted retrieval.",
                    "query": "shared_runtime_marker",
                    "recorded_at_epoch_ms": base,
                    "decision_trace": {
                        "scope": {
                            "project_code": "art",
                            "namespace_code": "continuity",
                            "effective_retrieval_mode": "local_strict"
                        },
                        "selection_priority": ["exact_documents", "lexical_chunks"],
                        "included": [
                            {"strategy":"exact_documents","count":1,"reason":"Exact hit"}
                        ],
                        "not_included": [
                            {"strategy":"semantic_chunks","reason":"abstained"}
                        ],
                        "semantic_guard": {"abstained": true}
                    }
                }
            }),
            created_at_epoch_ms: base,
        };
        let bundle = compose_restore_bundle(
            &json!({"code":"art"}),
            &json!({"code":"continuity"}),
            &[retrieval],
        );
        let restore = &bundle["working_state_restore"];
        assert_eq!(
            restore["latest_decision_trace"]["scope"]["effective_retrieval_mode"],
            json!("local_strict")
        );
        assert_eq!(
            restore["latest_decision_trace"]["included"][0]["strategy"],
            json!("exact_documents")
        );
        assert_eq!(
            restore["included_reasons_summary"],
            json!("точные совпадения (1) — Exact hit")
        );
        assert_eq!(
            restore["excluded_reasons_summary"],
            json!("смысловые фрагменты — abstained")
        );
        assert_eq!(
            restore["recent_decision_traces"].as_array().map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn degradation_proof_report_marks_core_working_state_scenarios_pass() {
        let report = degradation_proof_report(now_epoch_ms().unwrap_or(2_000_000)).expect("report");
        let scenarios = report["degradation_verification"]["scenarios"]
            .as_array()
            .expect("scenarios");
        assert_eq!(scenarios.len(), 5);
        assert!(
            scenarios
                .iter()
                .all(|scenario| scenario["status"].as_str() == Some("pass"))
        );
        assert!(scenarios.iter().any(|scenario| {
            scenario["class_key"].as_str() == Some("cross_agent_scope")
                && scenario["details"]["foreign_only_selected_count"] == json!(0)
        }));
        assert!(scenarios.iter().any(|scenario| {
            scenario["class_key"].as_str() == Some("corrupt_scope_metadata")
                && scenario["details"]["corrupt_project_selected_count"] == json!(0)
        }));
        assert!(scenarios.iter().any(|scenario| {
            scenario["class_key"].as_str() == Some("partial_refresh")
                && scenario["details"]["restore_confidence"] == json!("preliminary")
        }));
        assert!(scenarios.iter().any(|scenario| {
            scenario["class_key"].as_str() == Some("stale_handoff")
                && scenario["details"]["restore_freshness_state"] == json!("stale")
        }));
    }

    #[test]
    fn rotate_chat_action_bundle_exposes_canonical_handoff_and_startup_commands() {
        let bundle = super::build_rotate_chat_action_bundle(
            Some("amai"),
            Some("continuity"),
            Some("/tmp/amai"),
            true,
            Some("Same-meter spend control"),
            Some("Materialize live assistant generation source."),
        );
        assert_eq!(
            bundle["bundle_version"],
            json!("rotate-chat-action-bundle-v1")
        );
        assert_eq!(bundle["ready_for_automation"], json!(true));
        assert_eq!(bundle["preserves_return_obligation"], json!(true));
        assert_eq!(
            bundle["host_current_thread_control"]["control_kind"],
            json!("thread_overlay_open_current")
        );
        assert_eq!(
            bundle["host_current_thread_control"]["command_id"],
            json!("thread-overlay-open-current")
        );
        assert_eq!(
            bundle["host_current_thread_control"]["automation_ready"],
            json!(cfg!(target_os = "linux"))
        );
        assert_eq!(
            bundle["capture_continuity_handoff"]["argv_template"][0],
            json!("amai")
        );
        assert_eq!(
            bundle["run_rotate_helper"]["argv_template"][2],
            json!("rotate-chat")
        );
        assert_eq!(
            bundle["capture_continuity_handoff"]["argv_template"][2],
            json!("handoff")
        );
        assert_eq!(
            bundle["run_continuity_startup"]["argv_template"][2],
            json!("startup")
        );
        assert_eq!(
            bundle["run_continuity_startup"]["token_source_kind"],
            json!("live_continuity_startup")
        );
        assert!(
            bundle["operator_flow"]["startup_command"]
                .as_str()
                .unwrap_or_default()
                .contains("--runtime-state-json")
        );
        assert_eq!(
            bundle["recommended_handoff"]["headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(
            bundle["recommended_handoff"]["next_step"],
            json!("Materialize live assistant generation source.")
        );
        assert_eq!(bundle["operator_flow"]["copy_paste_ready"], json!(true));
        assert_eq!(
            bundle["operator_flow"]["primary_command_kind"],
            json!("same_thread_host_control_launch_command")
        );
        assert!(
            bundle["operator_flow"]["primary_command"]
                .as_str()
                .unwrap_or_default()
                .contains("ctl-launch")
        );
        assert!(
            bundle["operator_flow"]["rotate_helper_command"]
                .as_str()
                .unwrap_or_default()
                .contains("rotate-chat")
        );
        assert!(
            bundle["operator_flow"]["handoff_command"]
                .as_str()
                .unwrap_or_default()
                .contains("--headline")
        );
    }

    #[test]
    fn host_current_thread_control_surface_for_thread_exposes_vscode_uri_launch() {
        let surface =
            super::build_host_current_thread_control_surface_for_thread(Some("thread-current"));
        assert_eq!(surface["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(surface["thread_id"], json!("thread-current"));
        assert_eq!(surface["button_label"], json!("Open thread overlay"));
        assert_eq!(
            surface["external_uri_launch"]["launch_surface_kind"],
            json!("vscode_extension_uri_route")
        );
        assert_eq!(
            surface["external_uri_launch"]["uri"],
            json!("vscode://openai.chatgpt/thread-overlay/thread-current")
        );
        if cfg!(target_os = "linux") {
            assert_eq!(surface["automation_ready"], json!(true));
            assert_eq!(
                surface["external_uri_launch"]["observe_api_launch_available"],
                json!(true)
            );
            assert_eq!(
                surface["external_uri_launch"]["observe_api_launch_path"],
                json!("/api/client-budget-host-control-launch")
            );
            assert_eq!(
                surface["external_uri_launch"]["verification_state"],
                json!("route_resolved_launch_command_available")
            );
            assert!(
                surface["external_uri_launch"]["platform_launch_command"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("xdg-open")
            );
        }
        let alternates = surface["alternate_controls"]
            .as_array()
            .expect("alternate controls");
        assert_eq!(alternates.len(), 1);
        assert_eq!(
            alternates[0]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(alternates[0]["button_label"], json!("Open compact window"));
        assert_eq!(
            alternates[0]["external_uri_launch"]["uri"],
            json!("vscode://openai.chatgpt/hotkey-window/thread/thread-current")
        );
    }

    #[test]
    fn host_current_thread_control_surface_for_preserve_stage_prefers_compact_window() {
        let surface = super::build_host_current_thread_control_surface_for_thread_and_stage(
            Some("thread-current"),
            super::HostContextCompactionStage::Preserve,
        );
        assert_eq!(surface["command_id"], json!("hotkey-window-open-current"));
        assert_eq!(surface["button_label"], json!("Open compact window"));
        assert_eq!(surface["host_context_compaction_stage"], json!("preserve"));
        assert_eq!(
            surface["selection_reason"],
            json!("protect_recent_host_compaction_gain")
        );
        let alternates = surface["alternate_controls"]
            .as_array()
            .expect("alternate controls");
        assert_eq!(alternates.len(), 1);
        assert_eq!(
            alternates[0]["command_id"],
            json!("thread-overlay-open-current")
        );
        assert_eq!(alternates[0]["button_label"], json!("Open thread overlay"));
    }

    #[test]
    fn host_current_thread_control_surface_for_critical_regrowth_can_try_overlay_first() {
        let surface =
            super::build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
                Some("thread-current"),
                super::HostContextCompactionStage::CriticalRegrowth,
                Some("thread-overlay-open-current"),
            );
        assert_eq!(surface["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(surface["button_label"], json!("Open thread overlay"));
        assert_eq!(
            surface["selection_reason"],
            json!("critical_regrowth_try_overlay")
        );
        let alternates = surface["alternate_controls"]
            .as_array()
            .expect("alternate controls");
        assert_eq!(alternates.len(), 1);
        assert_eq!(
            alternates[0]["command_id"],
            json!("hotkey-window-open-current")
        );
    }

    #[test]
    fn rotate_chat_action_bundle_for_preserve_stage_prefers_compact_window_host_control() {
        let bundle = super::build_rotate_chat_action_bundle_for_stage(
            Some("amai"),
            Some("continuity"),
            Some("/tmp/amai"),
            true,
            Some("Same-meter spend control"),
            Some("Protect compacted host surface first."),
            super::HostContextCompactionStage::Preserve,
        );
        assert_eq!(
            bundle["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            bundle["host_current_thread_control"]["button_label"],
            json!("Open compact window")
        );
        let alternates = bundle["host_current_thread_control"]["alternate_controls"]
            .as_array()
            .expect("alternate controls");
        assert_eq!(alternates.len(), 1);
        assert_eq!(
            alternates[0]["command_id"],
            json!("thread-overlay-open-current")
        );
    }

    #[test]
    fn rotate_chat_action_bundle_with_explicit_thread_prefers_same_thread_primary_command() {
        let bundle =
            super::build_rotate_chat_action_bundle_for_stage_with_preference_and_primary_command(
                Some("amai"),
                Some("continuity"),
                Some("/tmp/amai"),
                true,
                Some("Same-meter spend control"),
                Some("Protect compacted host surface first."),
                super::HostContextCompactionStage::Preserve,
                true,
                Some("thread-current"),
                Some(super::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID),
            );
        assert_eq!(
            bundle["host_current_thread_control"]["thread_id"],
            json!("thread-current")
        );
        assert_eq!(
            bundle["operator_flow"]["primary_command_kind"],
            json!("same_thread_host_control_launch_command")
        );
        assert!(
            bundle["operator_flow"]["host_current_thread_control_launch_command"]
                .as_str()
                .unwrap_or_default()
                .contains("--thread-id")
        );
        assert!(
            bundle["operator_flow"]["host_current_thread_control_launch_command"]
                .as_str()
                .unwrap_or_default()
                .contains("thread-current")
        );
    }

    #[test]
    fn host_current_thread_control_observe_launch_command_is_copy_pasteable() {
        let surface = super::build_host_current_thread_control_surface_for_thread_and_stage(
            Some("thread-current"),
            super::HostContextCompactionStage::Preserve,
        );
        let command = super::build_host_current_thread_control_observe_launch_command(
            Some("amai"),
            Some("continuity"),
            Some("/tmp/amai"),
            &surface,
        )
        .expect("observe launch command");
        assert!(command.contains("observe"));
        assert!(command.contains("ctl-launch"));
        assert!(command.contains("--thread-id"));
        assert!(command.contains("thread-current"));
        assert!(command.contains("--compact-window"));
    }

    #[test]
    fn host_current_thread_control_feedback_kind_normalization_is_strict() {
        assert_eq!(
            super::normalize_host_current_thread_control_feedback_kind("requested"),
            Some("requested")
        );
        assert_eq!(
            super::normalize_host_current_thread_control_feedback_kind("opened"),
            Some("opened")
        );
        assert_eq!(
            super::normalize_host_current_thread_control_feedback_kind("failed"),
            Some("failed")
        );
        assert_eq!(
            super::normalize_host_current_thread_control_feedback_kind("launched"),
            None
        );
    }

    #[test]
    fn host_current_thread_control_feedback_notice_text_is_human_readable() {
        assert!(
            super::host_current_thread_control_feedback_notice_text_for_command("requested", None)
                .contains("отметь")
        );
        assert!(
            super::host_current_thread_control_feedback_notice_text_for_command("opened", None)
                .contains("открылся")
        );
        assert!(
            super::host_current_thread_control_feedback_notice_text_for_command("failed", None)
                .contains("не открылся")
        );
        assert!(
            super::host_current_thread_control_feedback_notice_text_for_command(
                "opened",
                Some("hotkey-window-open-current"),
            )
            .contains("compact window")
        );
    }

    #[test]
    fn wait_for_global_client_budget_action_bundle_exposes_recovery_guidance() {
        let bundle = super::build_wait_for_global_client_budget_action_bundle(
            Some("amai"),
            Some("continuity"),
            Some("/tmp/amai"),
            true,
            Some("Same-meter spend control"),
            Some("Materialize live assistant generation source."),
        );
        assert_eq!(
            bundle["bundle_version"],
            json!("wait-client-budget-action-bundle-v1")
        );
        assert_eq!(bundle["preserves_return_obligation"], json!(true));
        assert_eq!(
            bundle["wait_for_budget_recovery"]["action_kind"],
            json!("wait_for_global_client_budget_recovery")
        );
        assert_eq!(
            bundle["budget_source"]["source_kind"],
            json!(super::GLOBAL_CLIENT_LIMIT_SOURCE_KIND)
        );
        assert_eq!(
            bundle["budget_source"]["truly_global_source_materialized"],
            json!(false)
        );
        assert!(
            bundle["budget_source"]["summary"]
                .as_str()
                .unwrap_or_default()
                .contains("последнее observed значение client limits")
        );
        assert!(
            bundle["operator_flow"]["wait_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("не отвечай содержательно")
        );
        assert!(
            bundle["operator_flow"]["startup_after_recovery_command"]
                .as_str()
                .unwrap_or_default()
                .contains("continuity")
        );
    }

    #[test]
    fn build_client_budget_blocking_reply_contract_supports_wait_mode() {
        let contract = super::build_client_budget_blocking_reply_contract(
            super::ClientBudgetBlockingReplyMode::WaitForGlobalBudgetRecovery,
        );
        assert_eq!(
            contract["response_kind"],
            json!(super::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND)
        );
        assert_eq!(contract["active"], json!(true));
        assert_eq!(
            contract["template"],
            json!(super::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE)
        );
    }

    #[test]
    fn client_budget_guard_blocks_reply_ignores_rotate_soon_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true
        });
        assert!(!super::client_budget_guard_requires_rotate_before_reply(
            &guard
        ));
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_blocks_reply_ignores_rotate_now_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false
            },
            "should_rotate_chat_now": true,
            "should_rotate_chat_soon": true
        });
        assert!(!super::client_budget_guard_requires_rotate_before_reply(
            &guard
        ));
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_keeps_same_meter_pure_burn_tool_pressure_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "same_meter_pure_burn_turn_active": true,
                "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                "max_tool_roundtrips_soft": 0
            }
        });
        assert!(!super::client_budget_guard_blocks_expensive_tool_turn(
            &guard
        ));
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_keeps_zero_roundtrip_tool_pressure_advisory_without_pure_burn() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "same_meter_pure_burn_turn_active": false,
                "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                "max_tool_roundtrips_soft": 0
            }
        });
        assert!(!super::client_budget_guard_blocks_expensive_tool_turn(
            &guard
        ));
    }

    #[test]
    fn client_budget_guard_keeps_non_zero_roundtrip_tool_pressure_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "same_meter_pure_burn_turn_active": false,
                "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                "max_tool_roundtrips_soft": 1
            }
        });
        assert!(!super::client_budget_guard_blocks_expensive_tool_turn(
            &guard
        ));
    }

    #[test]
    fn build_startup_next_action_keeps_global_budget_wait_as_advisory() {
        let contract = json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "active_task": {
                "headline": "Same-meter spend control",
                "next_step": "Materialize live assistant generation source."
            },
            "required_return_task": {
                "headline": "",
                "next_step": ""
            }
        });
        let client_budget_guard = json!({
            "status_label": "глобальный лимит клиента почти исчерпан",
            "note": "global client budget is almost exhausted",
            "reply_execution_gate": {
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true
            }
        });

        let action = super::build_startup_next_action(
            "Same-meter spend control",
            "Materialize live assistant generation source.",
            &contract,
            &client_budget_guard,
            Some("amai"),
            Some("continuity"),
            Some("/tmp/amai"),
        );
        assert_eq!(action["action_kind"], json!("continue_active_workline"));
        assert_eq!(action["blocking"], json!(false));
    }

    #[test]
    fn build_execctl_resume_contract_ignores_generic_continue_workline_placeholder() {
        let project_task_tree = json!({
            "nodes": [
                {
                    "task_role": "active",
                    "headline": "Current active line",
                    "next_step": "Do real work."
                },
                {
                    "task_role": "pending_return",
                    "headline": "Продолжить активную рабочую линию",
                    "next_step": "продолжить работу в свежем чате через continuity startup",
                    "resume_state": "pending_return"
                }
            ]
        });
        let pending_return_queue = json!([
            {
                "headline": "Продолжить активную рабочую линию",
                "next_step": "продолжить работу в свежем чате через continuity startup",
                "resume_state": "pending_return"
            }
        ]);
        let contract =
            super::build_execctl_resume_contract(&project_task_tree, &pending_return_queue, &[]);
        assert_eq!(contract["resume_state"], json!("clear"));
        assert_eq!(contract["required_return_task"], Value::Null);
    }

    #[test]
    fn build_startup_next_action_does_not_block_on_rotate_soon_advisory() {
        let contract = json!({
            "resume_state": "return_required",
            "no_silent_drop": true,
            "active_task": {
                "headline": "Same-meter spend control",
                "next_step": "Materialize live assistant generation source."
            },
            "required_return_task": {
                "headline": "Same-meter spend control",
                "next_step": "Materialize live assistant generation source."
            }
        });
        let client_budget_guard = json!({
            "status_label": "новый чат рекомендован",
            "note": "soft rotate recommendation only",
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true
        });

        let action = super::build_startup_next_action(
            "Same-meter spend control",
            "Materialize live assistant generation source.",
            &contract,
            &client_budget_guard,
            Some("amai"),
            Some("continuity"),
            Some("/tmp/amai"),
        );

        assert_eq!(action["action_kind"], json!("resume_required_return_task"));
        assert_eq!(action["blocking"], json!(true));
    }

    #[test]
    fn fallback_client_budget_guard_uses_dashboard_live_source() {
        let guard = fallback_client_budget_guard_from_error("test drift");
        assert_eq!(
            guard["source"],
            json!("dashboard_current_session_budget_guard_v2")
        );
        assert_eq!(guard["status"], json!("unknown"));
        assert!(
            guard["note"]
                .as_str()
                .is_some_and(|value| value.contains("test drift"))
        );
    }

    proptest! {
        #[test]
        fn select_relevant_events_keeps_only_latest_exact_scope_session(
            shared_count in 0usize..6,
            foreign_count in 0usize..6,
            older_exact_same_session in 0usize..6,
            older_exact_other_session in 0usize..6,
        ) {
            let mut snapshots = Vec::new();
            let mut offset = 10_000_i64;
            snapshots.push(fake_snapshot("art", "continuity", "art::primary", "session-a", "latest-exact", offset));
            offset -= 1;
            for index in 0..older_exact_same_session {
                snapshots.push(fake_snapshot("art", "continuity", "art::primary", "session-a", &format!("exact-same-{index}"), offset));
                offset -= 1;
            }
            for index in 0..older_exact_other_session {
                snapshots.push(fake_snapshot("art", "continuity", "art::primary", "session-b", &format!("exact-other-{index}"), offset));
                offset -= 1;
            }
            for index in 0..shared_count {
                snapshots.push(fake_snapshot("art", "continuity", "shared", "session-shared", &format!("shared-{index}"), offset));
                offset -= 1;
            }
            for index in 0..foreign_count {
                snapshots.push(fake_snapshot("art", "continuity", "art::secondary", "session-foreign", &format!("foreign-{index}"), offset));
                offset -= 1;
            }

            let selected = select_relevant_events(snapshots, "art", "continuity", "art::primary");
            prop_assert!(!selected.is_empty());
            let all_exact_scope = selected.iter().all(|snapshot| {
                let event = &snapshot.payload["working_state_event"];
                event["project"]["code"].as_str() == Some("art")
                    && event["namespace"]["code"].as_str() == Some("continuity")
                    && event["agent_scope"].as_str() == Some("art::primary")
                    && event["session_id"].as_str() == Some("session-a")
            });
            prop_assert!(all_exact_scope);
        }

        #[test]
        fn select_relevant_events_falls_back_to_shared_scope_without_mixing_foreign(
            shared_count in 0usize..8,
            foreign_count in 0usize..8,
        ) {
            let mut snapshots = Vec::new();
            let mut offset = 20_000_i64;
            snapshots.push(fake_snapshot("art", "continuity", "shared", "session-shared", "latest-shared", offset));
            offset -= 1;
            for index in 0..shared_count {
                snapshots.push(fake_snapshot("art", "continuity", "shared", "session-shared", &format!("shared-{index}"), offset));
                offset -= 1;
            }
            for index in 0..foreign_count {
                snapshots.push(fake_snapshot("art", "continuity", "art::secondary", "session-foreign", &format!("foreign-{index}"), offset));
                offset -= 1;
            }

            let selected = select_relevant_events(snapshots, "art", "continuity", "art::primary");
            prop_assert!(!selected.is_empty());
            let all_shared_scope = selected.iter().all(|snapshot| {
                let event = &snapshot.payload["working_state_event"];
                matches!(event["agent_scope"].as_str(), Some("shared") | None | Some(""))
                    && event["session_id"].as_str() == Some("session-shared")
            });
            prop_assert!(all_shared_scope);
        }

        #[test]
        fn select_relevant_events_fail_closes_for_foreign_or_corrupt_scope_only(
            foreign_count in 1usize..10,
            corrupt_project_count in 0usize..6,
            corrupt_namespace_count in 0usize..6,
        ) {
            let mut snapshots = Vec::new();
            let mut offset = 30_000_i64;
            for index in 0..foreign_count {
                snapshots.push(fake_snapshot("art", "continuity", "art::secondary", "session-foreign", &format!("foreign-{index}"), offset));
                offset -= 1;
            }
            for index in 0..corrupt_project_count {
                snapshots.push(fake_snapshot("art-corrupt", "continuity", "art::primary", "session-corrupt-project", &format!("corrupt-project-{index}"), offset));
                offset -= 1;
            }
            for index in 0..corrupt_namespace_count {
                snapshots.push(fake_snapshot("art", "continuity-corrupt", "art::primary", "session-corrupt-namespace", &format!("corrupt-namespace-{index}"), offset));
                offset -= 1;
            }

            let selected = select_relevant_events(snapshots, "art", "continuity", "art::primary");
            prop_assert!(selected.is_empty());
        }

        #[test]
        fn select_relevant_events_with_empty_latest_session_returns_only_latest_exact(
            older_exact_count in 0usize..8,
            shared_count in 0usize..8,
        ) {
            let mut snapshots = Vec::new();
            let mut offset = 40_000_i64;
            snapshots.push(fake_snapshot("art", "continuity", "art::primary", "", "latest-empty-session", offset));
            offset -= 1;
            for index in 0..older_exact_count {
                snapshots.push(fake_snapshot("art", "continuity", "art::primary", "session-older", &format!("exact-{index}"), offset));
                offset -= 1;
            }
            for index in 0..shared_count {
                snapshots.push(fake_snapshot("art", "continuity", "shared", "session-shared", &format!("shared-{index}"), offset));
                offset -= 1;
            }

            let selected = select_relevant_events(snapshots, "art", "continuity", "art::primary");
            prop_assert_eq!(selected.len(), 1);
            prop_assert_eq!(
                selected[0].payload["working_state_event"]["headline"].as_str(),
                Some("latest-empty-session")
            );
        }
    }

    fn fake_snapshot(
        project_code: &str,
        namespace_code: &str,
        agent_scope: &str,
        session_id: &str,
        headline: &str,
        offset: i64,
    ) -> ObservabilitySnapshotRecord {
        fake_snapshot_with_kind(FakeSnapshotSpec {
            project_code,
            namespace_code,
            agent_scope,
            session_id,
            event_kind: "retrieval_context_pack",
            headline,
            next_step_hint: "",
            summary: "",
            offset,
        })
    }

    fn fake_snapshot_with_kind(spec: FakeSnapshotSpec<'_>) -> ObservabilitySnapshotRecord {
        ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: WORKING_STATE_EVENT_KIND.to_string(),
            payload: json!({
                "working_state_event": {
                    "event_id": format!("{}-{}", spec.headline, spec.offset),
                    "project": {
                        "code": spec.project_code,
                    },
                    "namespace": {
                        "code": spec.namespace_code,
                    },
                    "agent_scope": spec.agent_scope,
                    "session_id": spec.session_id,
                    "event_kind": spec.event_kind,
                    "source_kind": "test",
                    "headline": spec.headline,
                    "next_step_hint": spec.next_step_hint,
                    "summary": spec.summary,
                    "local_path": "/tmp/test",
                    "recorded_at_epoch_ms": spec.offset,
                }
            }),
            created_at_epoch_ms: spec.offset,
        }
    }

    #[test]
    fn resolved_thread_id_with_candidates_prefers_explicit_hint() {
        assert_eq!(
            resolved_thread_id_with_candidates(
                Some("thread-explicit"),
                Some("thread-env"),
                Some("thread-repo"),
            )
            .as_deref(),
            Some("thread-explicit")
        );
    }

    #[test]
    fn resolved_thread_id_with_candidates_falls_back_to_env_before_repo() {
        assert_eq!(
            resolved_thread_id_with_candidates(None, Some("thread-env"), Some("thread-repo"))
                .as_deref(),
            Some("thread-env")
        );
    }

    #[test]
    fn resolved_thread_id_with_candidates_uses_repo_hint_when_other_sources_missing() {
        assert_eq!(
            resolved_thread_id_with_candidates(None, None, Some("thread-repo")).as_deref(),
            Some("thread-repo")
        );
    }

    #[tokio::test]
    async fn record_handoff_event_materializes_workspace_restore_pack() {
        if let Ok(env_text) =
            std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
        {
            for line in env_text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = trimmed.split_once('=') else {
                    continue;
                };
                if std::env::var_os(key).is_none() {
                    unsafe {
                        std::env::set_var(key.trim(), value.trim_matches('\"'));
                    }
                }
            }
        }

        let cfg = AppConfig::from_env().expect("config");
        let client = postgres::connect_admin(&cfg).await.expect("postgres");
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos();
        let workspace_code = format!("restore_pack_ws_{suffix}");
        let project_code = format!("restore_pack_proj_{suffix}");
        let repo_root = format!("/tmp/{project_code}");
        std::fs::create_dir_all(&repo_root).expect("repo root");
        let handoff_path = format!("{repo_root}/HANDOFF.md");
        std::fs::write(&handoff_path, "restore pack test handoff").expect("handoff file");

        postgres::ensure_workspace(&client, &workspace_code, "Restore Pack Workspace", "active")
            .await
            .expect("workspace");
        let project = postgres::upsert_project(
            &client,
            &project_code,
            "Restore Pack Project",
            &repo_root,
            Some("main"),
            &workspace_code,
            "project_shared",
            "local_strict",
        )
        .await
        .expect("project");
        let namespace = postgres::get_namespace_by_code(&client, project.project_id, "default")
            .await
            .expect("default namespace");

        record_handoff_event(
            &client,
            &project,
            &namespace,
            "Restore pack runtime proof",
            "Verify workspace restore pack materialization.",
            "Reviewed /tmp/runtime-proof.md and confirmed restore pack should be durable.",
            false,
            &[],
            &[],
            &handoff_path,
        )
        .await
        .expect("record handoff");

        let row = client
            .query_one(
                r#"
                SELECT
                    pack_kind,
                    source_kind,
                    payload,
                    source_event_ids,
                    artifact_refs,
                    message_refs,
                    evidence_span
                FROM ami.restore_packs
                WHERE project_id = $1
                  AND namespace_id = $2
                ORDER BY created_at DESC, restore_pack_id DESC
                LIMIT 1
                "#,
                &[&project.project_id, &namespace.namespace_id],
            )
            .await
            .expect("restore pack row");
        assert_eq!(row.get::<_, String>(0), "workspace_restore_pack");
        assert_eq!(
            row.get::<_, Option<String>>(1).as_deref(),
            Some("working_state_restore_runtime")
        );
        let payload: Value = row.get(2);
        assert_eq!(
            payload["pack_version"],
            json!(WORKSPACE_RESTORE_PACK_VERSION)
        );
        assert_eq!(payload["current_goal"], json!("Restore pack runtime proof"));
        assert!(payload["active_commitments"].is_array());
        assert!(payload["active_constraints"].is_array());
        assert!(payload["important_artifacts"].is_array());
        assert_eq!(
            payload["procedural_restore_policy"]["raw_procedural_archive_forbidden"],
            json!(true)
        );
        let source_event_ids: Value = row.get(3);
        let artifact_refs: Value = row.get(4);
        let message_refs: Value = row.get(5);
        let evidence_span: Value = row.get(6);
        assert!(
            source_event_ids
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        assert!(artifact_refs.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(format!("file://{handoff_path}").as_str()))
        }));
        assert!(
            message_refs
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        assert_eq!(evidence_span["kind"], json!("working_state_restore"));
    }

    #[tokio::test]
    async fn record_handoff_event_surfaces_compact_restore_execution_card() {
        if let Ok(env_text) =
            std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
        {
            for line in env_text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = trimmed.split_once('=') else {
                    continue;
                };
                if std::env::var_os(key).is_none() {
                    unsafe {
                        std::env::set_var(key.trim(), value.trim_matches('\"'));
                    }
                }
            }
        }

        unsafe {
            std::env::set_var("AMAI_RESTORE_EXECUTION_CARD_RUNTIME", "codex");
            std::env::set_var("AMAI_RESTORE_EXECUTION_CARD_MODEL", "gpt-5");
            std::env::set_var("AMAI_RESTORE_EXECUTION_CARD_TOOL", "exec_command");
        }

        let cfg = AppConfig::from_env().expect("config");
        let client = postgres::connect_admin(&cfg).await.expect("postgres");
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos();
        let workspace_code = format!("restore_card_ws_{suffix}");
        let project_code = format!("restore_card_proj_{suffix}");
        let repo_root = format!("/tmp/{project_code}");
        std::fs::create_dir_all(&repo_root).expect("repo root");
        let handoff_path = format!("{repo_root}/HANDOFF.md");
        std::fs::write(&handoff_path, "restore execution card test handoff").expect("handoff file");

        postgres::ensure_workspace(&client, &workspace_code, "Restore Card Workspace", "active")
            .await
            .expect("workspace");
        let project = postgres::upsert_project(
            &client,
            &project_code,
            "Restore Card Project",
            &repo_root,
            Some("main"),
            &workspace_code,
            "project_shared",
            "local_strict",
        )
        .await
        .expect("project");
        let namespace = postgres::get_namespace_by_code(&client, project.project_id, "default")
            .await
            .expect("default namespace");

        let skill = postgres::create_skill_card_candidate(
            &client,
            &project.code,
            &namespace.code,
            &format!("restore_execution_skill_{suffix}"),
            1,
            "Restore Continuity Card",
            "Restore continuity for current step",
            &["restore continuity".to_string()],
            &["continuity fresh".to_string()],
            &[
                "inspect startup gate".to_string(),
                "confirm required return task".to_string(),
            ],
            &["required return cleared".to_string()],
            &["continuity stale".to_string()],
            Some("safe resume"),
            "project_private",
            "project",
            &["codex".to_string()],
            &["gpt-5".to_string()],
            &["exec_command".to_string()],
            &["continuity".to_string(), "restore".to_string()],
            &[format!("event:restore-card:{suffix}")],
            &[format!("artifact://restore-card/{suffix}")],
            &json!({
                "skill_change_summary": {
                    "changed_by": "test",
                    "change_reason": "seed restore execution card"
                }
            }),
            None,
            Some("extract"),
        )
        .await
        .expect("skill");

        let message_refs = json!([format!("thread:restore-card:{suffix}")]);
        postgres::create_skill_evidence_bundle(
            &client,
            skill.skill_card_id,
            "trace",
            Some("restore card evidence"),
            &[format!("event:restore-card:evidence:{suffix}")],
            &[format!("artifact://restore-card/evidence/{suffix}")],
            Some("manual_proof"),
            Some(&message_refs),
            Some(&json!({"kind":"bundle","context":"continuity"})),
            Some("extract"),
            Some("skill-evidence-bundle-envelope-v1"),
        )
        .await
        .expect("evidence");
        postgres::record_skill_trigger_match(
            &client,
            skill.skill_card_id,
            "project_task",
            "restore continuity",
            true,
            Some("trigger matched"),
            Some("manual_trigger"),
            Some(&json!([format!("event:restore-card:trigger:{suffix}")])),
            Some(&json!([format!(
                "artifact://restore-card/trigger/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"trigger","context":"continuity"})),
            Some("extract"),
            Some("skill-trigger-match-envelope-v1"),
        )
        .await
        .expect("trigger");
        postgres::record_skill_trial_run(
            &client,
            skill.skill_card_id,
            "shadow",
            Some("restore card shadow"),
            Some("codex"),
            Some("gpt-5"),
            Some("exec_command"),
            true,
            false,
            "success",
            Some("shadow success"),
            Some("manual_shadow"),
            Some(&json!([format!("event:restore-card:shadow:{suffix}")])),
            Some(&json!([format!("artifact://restore-card/shadow/{suffix}")])),
            Some(&message_refs),
            Some(&json!({"kind":"shadow","context":"continuity"})),
            Some("extract"),
            Some("skill-trial-run-envelope-v1"),
        )
        .await
        .expect("shadow run");
        postgres::record_skill_eval(
            &client,
            skill.skill_card_id,
            "promote_shadow",
            "manual_eval",
            true,
            true,
            true,
            0.0,
            Some("promote to shadow"),
            Some("manual_eval"),
            Some(&json!([format!("event:restore-card:eval-shadow:{suffix}")])),
            Some(&json!([format!(
                "artifact://restore-card/eval-shadow/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"shadow"})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote shadow");
        postgres::record_skill_trial_run(
            &client,
            skill.skill_card_id,
            "trial",
            Some("restore card trial"),
            Some("codex"),
            Some("gpt-5"),
            Some("exec_command"),
            true,
            true,
            "success",
            Some("trial success"),
            Some("manual_trial"),
            Some(&json!([format!("event:restore-card:trial:{suffix}")])),
            Some(&json!([format!("artifact://restore-card/trial/{suffix}")])),
            Some(&message_refs),
            Some(&json!({"kind":"trial","context":"continuity"})),
            Some("extract"),
            Some("skill-trial-run-envelope-v1"),
        )
        .await
        .expect("trial run");
        postgres::record_skill_eval(
            &client,
            skill.skill_card_id,
            "promote_trial",
            "manual_eval",
            true,
            true,
            true,
            0.8,
            Some("promote to trial"),
            Some("manual_eval"),
            Some(&json!([format!("event:restore-card:eval:{suffix}")])),
            Some(&json!([format!("artifact://restore-card/eval/{suffix}")])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"trial"})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote trial");

        record_handoff_event(
            &client,
            &project,
            &namespace,
            "Restore continuity for current step",
            "Restore continuity safely via compact execution card.",
            "Reviewed restore path and need compact execution card.",
            false,
            &[],
            &[],
            &handoff_path,
        )
        .await
        .expect("record handoff");

        let row = client
            .query_one(
                r#"
                SELECT payload
                FROM ami.restore_packs
                WHERE project_id = $1
                  AND namespace_id = $2
                ORDER BY created_at DESC, restore_pack_id DESC
                LIMIT 1
                "#,
                &[&project.project_id, &namespace.namespace_id],
            )
            .await
            .expect("restore pack row");
        let payload: Value = row.get(0);
        assert_eq!(
            payload["skill_execution_card"]["skill_title"],
            json!("Restore Continuity Card")
        );
        assert_eq!(
            payload["skill_execution_card"]["skill_execution_steps"][0],
            json!("inspect startup gate")
        );
        assert_eq!(
            payload["skill_execution_card"]["binding"]["model"],
            json!("gpt-5")
        );
        assert_eq!(
            payload["skill_execution_card"]["binding"]["runtime"],
            json!("codex")
        );
        assert_eq!(
            payload["skill_execution_card"]["binding"]["tool"],
            json!("exec_command")
        );
        assert_eq!(
            payload["skill_execution_card"]["skill_trust_state"],
            json!("trial")
        );
        assert!(
            payload["skill_execution_card_summary"]
                .as_str()
                .is_some_and(|value| value.contains("Restore Continuity Card"))
        );
    }

    #[test]
    fn build_workspace_restore_pack_surfaces_full_acceptance_buckets() {
        let restore = json!({
            "project": {
                "code": "amai",
                "visibility_scope": "project_shared"
            },
            "namespace": {
                "code": "continuity",
                "retrieval_mode": "local_strict"
            },
            "agent_scope": "amai::continuity::default",
            "thread_id": "thread-acceptance",
            "visible_projects": ["amai"],
            "latest_decision_trace": {
                "scope": {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "effective_retrieval_mode": "local_strict",
                    "visible_projects_total": 1
                }
            },
            "state_lineage": {
                "authoritative_source_kind": "continuity_handoff",
                "authoritative_local_path": "/tmp/acceptance/HANDOFF.md"
            },
            "project_task_tree": {
                "nodes": [
                    {
                        "task_id": "task::active",
                        "task_role": "active",
                        "task_state": "active",
                        "resume_state": "active",
                        "headline": "Workspace restore acceptance line",
                        "next_step": "Verify the full restore pack."
                    },
                    {
                        "task_id": "task::waiting",
                        "task_role": "active",
                        "task_state": "waiting_external",
                        "resume_state": "waiting_external",
                        "headline": "Waiting on evaluator approval",
                        "next_step": "Resume after approval arrives."
                    },
                    {
                        "task_id": "task::review",
                        "task_role": "active",
                        "task_state": "in_review",
                        "resume_state": "in_review",
                        "headline": "Review restore wording",
                        "next_step": "Approve prompt summary wording."
                    }
                ]
            },
            "project_task_ledger": {
                "entries": [
                    {
                        "task_id": "task::active",
                        "task_role": "active",
                        "task_state": "active",
                        "resume_state": "active",
                        "headline": "Workspace restore acceptance line",
                        "next_step": "Verify the full restore pack.",
                        "recorded_at_epoch_ms": 100
                    },
                    {
                        "task_id": "task::waiting",
                        "task_role": "active",
                        "task_state": "waiting_external",
                        "resume_state": "waiting_external",
                        "headline": "Waiting on evaluator approval",
                        "next_step": "Resume after approval arrives.",
                        "recorded_at_epoch_ms": 90
                    },
                    {
                        "task_id": "task::closed",
                        "task_role": "historical_handoff",
                        "task_state": "resolved",
                        "resume_state": "historical_only",
                        "headline": "Closed restore rehearsal",
                        "next_step": "None",
                        "recorded_at_epoch_ms": 80,
                        "source_kind": "continuity_handoff"
                    }
                ]
            },
            "pending_return_queue": [
                {
                    "task_id": "task::paused",
                    "headline": "Paused compare branch",
                    "next_step": "Return after acceptance proof.",
                    "queued_reason": "interrupted_by_new_handoff",
                    "resume_state": "pending_return",
                    "queued_at_epoch_ms": 95
                }
            ],
            "source_summary": "Workspace restore pack now combines task truth and restore truth.",
            "current_hypothesis": "One pack should surface the current working picture.",
            "materialized_notes": [
                "Acceptance note one",
                "Acceptance note two"
            ],
            "recent_actions": [
                {
                    "headline": "Recorded live handoff",
                    "summary": "Captured latest execution line.",
                    "event_kind": "continuity_handoff",
                    "execution_state": "succeeded",
                    "recorded_at_epoch_ms": 110,
                    "authoritative": true,
                    "local_path": "/tmp/acceptance/action-1.md"
                },
                {
                    "headline": "Ran restore check",
                    "summary": "Verified startup and restore surfaces.",
                    "event_kind": "restore_check",
                    "execution_state": "succeeded",
                    "recorded_at_epoch_ms": 111,
                    "authoritative": false
                }
            ],
            "recent_decision_traces": [
                {
                    "decision": "keep_compact_execution_card",
                    "recorded_at_epoch_ms": 112
                }
            ],
            "execctl_resume_contract": {
                "resume_state": "return_required"
            },
            "execctl_resume_contract_summary": "return_required(1): Paused compare branch",
            "required_return_task": {
                "headline": "Paused compare branch",
                "next_step": "Return after acceptance proof."
            },
            "startup_next_action": {
                "action_kind": "resume_required_return_task",
                "blocking": true
            },
            "startup_next_action_summary": "resume_required_return_task: Paused compare branch",
            "client_budget_guard": {
                "status": "critical",
                "status_label": "compact_now",
                "reply_execution_gate": {
                    "action_kind": "compact_current_thread_for_client_budget"
                }
            },
            "skill_execution_card_summary": "Restore Continuity Card [trial] -> inspect startup gate",
            "skill_execution_card": {
                "skill_title": "Restore Continuity Card",
                "skill_trust_state": "trial",
                "skill_execution_steps": [
                    "inspect startup gate",
                    "confirm startup next action"
                ],
                "skill_runtime_constraints": ["codex"],
                "skill_model_constraints": ["gpt-5"],
                "skill_tool_constraints": ["exec_command"],
                "skill_context_constraints": ["continuity", "restore"]
            },
            "skill_execution_card_binding": {
                "runtime": "codex",
                "model": "gpt-5",
                "tool": "exec_command"
            },
            "active_files": [
                "/tmp/acceptance/current.rs"
            ],
            "open_questions": [
                "Should startup pack show more than one paused branch?"
            ],
            "rejected_hypotheses": [
                "Do not emit raw procedural archive."
            ],
            "excluded_reasons_summary": "memory_cards — no extra factual summary was relevant."
        });

        let pack = super::build_workspace_restore_pack(&restore);

        assert_eq!(
            pack["active_commitments"][0]["headline"],
            json!("Workspace restore acceptance line")
        );
        assert_eq!(
            pack["blocked_waiting_items"].as_array().map(|v| v.len()),
            Some(2)
        );
        assert_eq!(
            pack["blocked_waiting_items"][0]["headline"],
            json!("Waiting on evaluator approval")
        );
        assert_eq!(
            pack["paused_branches"][0]["headline"],
            json!("Paused compare branch")
        );
        assert_eq!(
            pack["recently_closed"][0]["headline"],
            json!("Closed restore rehearsal")
        );
        assert_eq!(
            pack["relevant_semantic_facts"].as_array().map(|v| v.len()),
            Some(4)
        );
        assert_eq!(
            pack["recent_episodic_traces"].as_array().map(|v| v.len()),
            Some(3)
        );
        assert_eq!(
            pack["active_constraints"].as_array().map(|v| v.len()),
            Some(4)
        );
        assert_eq!(
            pack["active_constraints"][3]["constraint_kind"],
            json!("procedural_binding")
        );
        assert_eq!(
            pack["permission_summary"]["namespace_retrieval_mode"],
            json!("local_strict")
        );
        assert_eq!(
            pack["important_artifacts"].as_array().map(|v| v.len()),
            Some(3)
        );
        assert_eq!(
            pack["unresolved_conflicts"].as_array().map(|v| v.len()),
            Some(3)
        );
        assert_eq!(
            pack["relevant_procedures"][0]["procedure_kind"],
            json!("compact_execution_card")
        );
        assert_eq!(
            pack["relevant_procedures"][0]["raw_procedural_archive_included"],
            json!(false)
        );
        assert_eq!(
            pack["relevant_procedures"][0]["binding"]["tool"],
            json!("exec_command")
        );
        assert_eq!(
            pack["procedural_restore_policy"]["materialized_surface"],
            json!("compact_execution_card")
        );
        assert_eq!(
            pack["procedural_restore_policy"]["raw_procedural_archive_forbidden"],
            json!(true)
        );
        assert_eq!(
            pack["summary"],
            json!(
                "active(1); blocked(2); paused(1); facts(4); constraints(4); artifacts(3); procedures(1)"
            )
        );
    }

    #[test]
    fn build_workspace_restore_pack_handles_malformed_surface_fail_closed() {
        let restore = json!({
            "project": "broken-project",
            "namespace": 7,
            "visible_projects": "amai",
            "latest_decision_trace": {"scope": "broken"},
            "state_lineage": {"authoritative_source_kind": 9},
            "project_task_tree": {"nodes": "broken"},
            "project_task_ledger": {"entries": "broken"},
            "pending_return_queue": "broken",
            "recent_actions": "broken",
            "recent_decision_traces": "broken",
            "execctl_resume_contract": "broken",
            "startup_next_action": "broken",
            "client_budget_guard": "broken",
            "skill_execution_card": "broken",
            "skill_execution_card_summary": "raw note must not be treated as a card",
            "active_files": "broken",
            "open_questions": "broken",
            "rejected_hypotheses": "broken",
            "excluded_reasons_summary": "",
            "materialized_notes": "broken"
        });

        let pack = super::build_workspace_restore_pack(&restore);

        assert_eq!(pack["active_commitments"], json!([]));
        assert_eq!(pack["blocked_waiting_items"], json!([]));
        assert_eq!(pack["paused_branches"], json!([]));
        assert_eq!(pack["recent_episodic_traces"], json!([]));
        assert_eq!(pack["active_constraints"], json!([]));
        assert_eq!(pack["relevant_procedures"], json!([]));
        assert_eq!(
            pack["procedural_restore_policy"]["materialized_surface"],
            json!("none")
        );
        assert_eq!(
            pack["procedural_restore_policy"]["raw_procedural_archive_forbidden"],
            json!(true)
        );
        assert_eq!(
            pack["permission_summary"]["visible_projects_count"],
            json!(0)
        );
        assert!(pack["summary"].is_null());
    }

    #[test]
    fn build_workspace_restore_pack_does_not_surface_raw_procedural_note_without_card() {
        let restore = json!({
            "project": {"code": "amai", "visibility_scope": "project_shared"},
            "namespace": {"code": "continuity", "retrieval_mode": "local_strict"},
            "visible_projects": ["amai"],
            "latest_decision_trace": {"scope": {"project_code": "amai"}},
            "state_lineage": {"authoritative_source_kind": "continuity_handoff"},
            "skill_execution_card_summary": "Long procedural note masquerading as summary",
            "skill_execution_card": Value::Null
        });

        let pack = super::build_workspace_restore_pack(&restore);

        assert_eq!(pack["relevant_procedures"], json!([]));
        assert_eq!(
            pack["procedural_restore_policy"]["materialized_surface"],
            json!("none")
        );
    }

    #[test]
    fn persisted_restore_snapshot_payload_compacts_project_task_ledger_and_strips_runtime_derived_fields()
     {
        let mut entries = vec![
            json!({
                "task_role": "active",
                "headline": "Current active line"
            }),
            json!({
                "task_role": "pending_return",
                "headline": "Pending line"
            }),
        ];
        for idx in 0..7 {
            entries.push(json!({
                "task_role": "historical_handoff",
                "headline": format!("Historical line {idx}")
            }));
        }
        let bundle = json!({
            "working_state_restore": {
                "project_task_ledger": {
                    "ledger_version": "project-task-ledger-v2",
                    "open_tasks_count": 2,
                    "historical_handoffs_count": 7,
                    "entries": entries
                },
                "workspace_restore_pack": {
                    "pack_version": WORKSPACE_RESTORE_PACK_VERSION
                },
                "workspace_restore_pack_summary": "runtime-only pack",
                "skill_execution_card": {
                    "card_version": "restore-execution-card-v1"
                },
                "skill_execution_card_summary": "runtime-only card",
                "skill_execution_card_binding": {
                    "tool": "exec_command"
                }
            }
        });

        let payload = super::persisted_restore_snapshot_payload(&bundle);
        let restore = &payload["working_state_restore"];
        let entries = restore["project_task_ledger"]["entries"]
            .as_array()
            .expect("compacted ledger entries");

        assert_eq!(
            restore["project_task_ledger"]["historical_handoffs_count"],
            json!(7)
        );
        assert_eq!(
            restore["project_task_ledger"]["full_shape_preserved_in_working_state_restore"],
            json!(false)
        );
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0]["task_role"], json!("active"));
        assert_eq!(entries[1]["task_role"], json!("pending_return"));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry["task_role"] == json!("historical_handoff"))
                .count(),
            MAX_PERSISTED_PROJECT_TASK_LEDGER_HISTORICAL_ENTRIES
        );
        assert!(restore.get("workspace_restore_pack").is_none());
        assert!(restore.get("workspace_restore_pack_summary").is_none());
        assert!(restore.get("skill_execution_card").is_none());
        assert!(restore.get("skill_execution_card_summary").is_none());
        assert!(restore.get("skill_execution_card_binding").is_none());
    }
}
