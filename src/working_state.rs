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
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
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
const MAX_RECENT_DECISION_TRACES: usize = 3;
const MAX_PENDING_RETURN_QUEUE: usize = 6;
const MAX_EXECCTL_LEDGER_ENTRIES: i64 = 256;
const EXECCTL_LEASE_TTL_MS: u64 = SESSION_GAP_MS;
const PROJECT_TASK_TREE_VERSION: &str = "project-task-tree-v1";
const PROJECT_TASK_LEDGER_VERSION: &str = "project-task-ledger-v2";
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION: &str =
    "client-budget-blocked-reply-v1";
pub(crate) const CLIENT_REPLY_BUDGET_CONTRACT_VERSION: &str = "client-reply-budget-v1";
pub(crate) const CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND: &str = "rotate_chat_only";
pub(crate) const CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND: &str = "wait_for_budget_only";
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND: &str =
    CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND;
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES: u64 = 1;
pub(crate) const CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE: &str = "Этот чат жжёт внешний лимит клиента: сохрани handoff, открой новый чат и запусти continuity startup.";
pub(crate) const CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE: &str = "Внешний лимит клиента почти исчерпан во всём клиенте. Не продолжай содержательный ответ, дождись восстановления окна лимита.";
pub(crate) const CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE: &str =
    CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE;
pub(crate) const GLOBAL_CLIENT_LIMIT_SOURCE_KIND: &str =
    "latest_observed_client_limits_without_current_thread_binding";
pub(crate) const GLOBAL_CLIENT_LIMIT_SOURCE_SUMMARY: &str = "При отсутствии current-thread binding Amai использует только последнее observed значение client limits. Этого достаточно для global warning hint и hard wait при критическом исчерпании, но недостаточно для thread-local rotate pressure.";
pub(crate) const CLIENT_REPLY_BUDGET_MODE_NORMAL: &str = "normal";
pub(crate) const CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL: &str = "compact_high_signal";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientBudgetBlockingReplyMode {
    Inactive,
    RotateChatOnly,
    WaitForGlobalBudgetRecovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientReplyBudgetMode {
    Normal,
    CompactHighSignal,
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

pub(crate) fn build_client_reply_budget_contract(mode: ClientReplyBudgetMode) -> Value {
    let (active, mode_label, max_paragraphs_soft, max_bullets_soft, max_sentences_soft, summary) =
        match mode {
            ClientReplyBudgetMode::Normal => (
                false,
                CLIENT_REPLY_BUDGET_MODE_NORMAL,
                None,
                None,
                None,
                "Обычный режим ответа без дополнительного client-budget сжатия.",
            ),
            ClientReplyBudgetMode::CompactHighSignal => (
                true,
                CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL,
                Some(1),
                Some(3),
                Some(4),
                "Exact 5ч KPI ниже целевого >90% или advisory rotate уже materialized. Ответ остаётся содержательным, но должен быть коротким: один абзац или короткий список, сначала прямой результат, затем только изменившиеся факты без повторов.",
            ),
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
        "max_paragraphs_soft": max_paragraphs_soft,
        "max_bullets_soft": max_bullets_soft,
        "max_sentences_soft": max_sentences_soft,
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

pub(crate) fn client_budget_guard_requires_rotate_before_reply(guard: &Value) -> bool {
    let reply_execution_gate = &guard["reply_execution_gate"];
    reply_execution_gate["must_rotate_before_reply"].as_bool() == Some(true)
        || (reply_execution_gate["action_kind"].as_str() == Some("rotate_chat_for_client_budget")
            && reply_execution_gate["blocking"].as_bool() != Some(false))
        || guard["should_rotate_chat_now"].as_bool() == Some(true)
}

pub(crate) fn client_budget_guard_blocks_reply(guard: &Value) -> bool {
    let reply_execution_gate = &guard["reply_execution_gate"];
    reply_execution_gate["blocking"].as_bool() == Some(true)
        || reply_execution_gate["must_wait_for_budget_recovery_before_reply"].as_bool()
            == Some(true)
        || guard["requires_global_budget_recovery_before_reply"].as_bool() == Some(true)
        || client_budget_guard_requires_rotate_before_reply(guard)
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
    local_path: &str,
) -> Result<()> {
    let recorded_at_epoch_ms = now_epoch_ms()?;
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let next_step = normalize_next_step_hint(next_step);
    let previous_restore = build_restore_bundle(db, project, namespace).await?;
    let pending_return_queue = derive_pending_return_queue(
        previous_restore
            .as_ref()
            .map(|value| &value["working_state_restore"]),
        headline,
        &next_step,
        recorded_at_epoch_ms,
        resolve_current_goal,
        resolved_headlines,
    );
    let thread_id = current_thread_id();
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
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
            "pending_return_queue": pending_return_queue,
            "resolve_current_goal": resolve_current_goal,
            "resolved_pending_return_headlines": resolved_headlines,
            "last_command": "continuity handoff".to_string(),
            "last_results_summary": format!("Зафиксирован handoff для {} :: {}", project.code, namespace.code),
            "local_path": local_path,
        }
    });
    let snapshot_id =
        postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
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
    let lease_expires_at_epoch_ms =
        recorded_at_epoch_ms.saturating_add(EXECCTL_LEASE_TTL_MS) as i64;
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
    refresh_restore_snapshot(db, project, namespace).await?;
    Ok(())
}

pub async fn record_context_pack_event(db: &Client, payload: &Value) -> Result<()> {
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
    let thread_id = current_thread_id();
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    let query_summary = format!(
        "Найдено: документов {}, символов {}, lexical chunks {}, semantic chunks {}.",
        exact_documents, symbol_hits, lexical_chunks, semantic_chunks
    );
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
            "source_kind": "context_pack",
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
            "decision_trace": node["decision_trace"].clone(),
            "workspace_graph": node["workspace_graph"].clone(),
        }
    });
    postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
    let project_record = ProjectRecord {
        project_id: postgres::get_project_by_code(db, &project.code)
            .await?
            .project_id,
        code: project.code,
        display_name: project.display_name,
        repo_root: project.repo_root,
        updated_at: String::new(),
    };
    let namespace_record = NamespaceRecord {
        namespace_id: postgres::get_namespace_by_code(
            db,
            project_record.project_id,
            &namespace.code,
        )
        .await?
        .namespace_id,
        code: namespace.code,
        display_name: namespace.display_name,
        retrieval_mode: String::new(),
    };
    refresh_restore_snapshot(db, &project_record, &namespace_record).await?;
    Ok(())
}

pub async fn build_restore_bundle(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
) -> Result<Option<Value>> {
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let snapshots = postgres::list_observability_snapshots_by_kinds(
        db,
        &[WORKING_STATE_EVENT_KIND],
        Some(MAX_RESTORE_EVENTS),
    )
    .await?;
    let events = select_relevant_events(snapshots, &project.code, &namespace.code, &agent_scope);
    if events.is_empty() {
        return Ok(None);
    }
    let mut bundle =
        compose_restore_bundle(&project_json(project), &namespace_json(namespace), &events);
    let durable_entries = postgres::list_execctl_task_ledger_entries(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        Some(MAX_EXECCTL_LEDGER_ENTRIES),
    )
    .await?;
    overlay_durable_project_task_ledger(
        &mut bundle,
        &project_json(project),
        &namespace_json(namespace),
        &durable_entries,
    );
    let active_lease = postgres::get_execctl_task_lease(
        db,
        project.project_id,
        namespace.namespace_id,
        &agent_scope,
        now_epoch_ms()? as i64,
    )
    .await?;
    overlay_execctl_active_lease(&mut bundle, active_lease.as_ref());
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(db, Some(&bundle))
            .await
            .unwrap_or_else(|error| fallback_client_budget_guard_from_error(&error.to_string()));
    overlay_client_budget_guard(&mut bundle, &client_budget_guard);
    Ok(Some(bundle))
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
    let Some(bundle) = build_restore_bundle(db, project, namespace).await? else {
        return Ok(());
    };
    let payload = json!({
        "working_state_restore": bundle["working_state_restore"].clone()
    });
    postgres::insert_observability_snapshot(db, WORKING_STATE_RESTORE_KIND, &payload).await?;
    Ok(())
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
            "execution_state": action_state,
            "authoritative": event["event_id"].as_str() == Some(authoritative_event_id.as_str()),
        }));
        if !event["workspace_graph"].is_null() {
            workspace_graph_inputs.push(event["workspace_graph"].clone());
        }
    }

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
    let execctl_resume_contract =
        build_execctl_resume_contract(&project_task_tree, &pending_return_queue);
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
            "thread_id": latest["thread_id"].as_str().unwrap_or_default(),
            "session_id": session_id,
            "session_age_ms": now_epoch_ms.saturating_sub(latest_recorded_at),
            "events_count": events.len(),
            "current_goal": current_goal,
            "next_step": next_step,
            "next_step_state": "planned",
            "current_hypothesis": current_hypothesis,
            "open_questions": open_questions,
            "rejected_hypotheses": rejected_hypotheses,
            "materialized_notes": materialized_notes,
            "active_files": active_files,
            "visible_projects": visible_projects,
            "recent_queries": recent_queries,
            "recent_actions": recent_actions,
            "pending_return_queue": pending_return_queue,
            "pending_return_summary": pending_return_summary,
            "execctl_resume_state": execctl_resume_state,
            "execctl_resume_contract": execctl_resume_contract,
            "execctl_resume_contract_summary": execctl_resume_contract_summary,
            "client_budget_guard": client_budget_guard,
            "startup_next_action": startup_next_action,
            "startup_next_action_summary": startup_next_action_summary,
            "project_task_tree": project_task_tree,
            "project_task_tree_summary": project_task_tree_summary,
            "project_task_ledger": project_task_ledger,
            "project_task_ledger_summary": project_task_ledger_summary,
            "last_command": last_command,
            "last_results_summary": last_results_summary,
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
    let snapshots =
        postgres::list_observability_snapshots_by_kinds(db, &[WORKING_STATE_EVENT_KIND], Some(60))
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

fn current_agent_scope_for(project_code: &str, namespace_code: &str) -> String {
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

fn project_json(project: &ProjectRecord) -> Value {
    json!({
        "code": project.code,
        "display_name": project.display_name,
        "repo_root": project.repo_root,
    })
}

fn namespace_json(namespace: &NamespaceRecord) -> Value {
    json!({
        "code": namespace.code,
        "display_name": namespace.display_name,
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
) -> Vec<Value> {
    let mut queue = restore_node
        .and_then(|node| node["pending_return_queue"].as_array())
        .cloned()
        .unwrap_or_default();
    prune_resolved_pending_return_items(&mut queue, resolved_headlines);
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
    let normalized_new_next_step = normalize_next_step_hint(new_next_step);
    if !is_meaningful_pending_return_headline(previous_goal)
        || previous_goal == new_headline
        || resolve_current_goal
        || resolved_pending_return_headline_matches(previous_goal, resolved_headlines)
        || (!previous_next_step.is_empty() && previous_next_step == normalized_new_next_step)
    {
        return queue;
    }
    let candidate = json!({
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

fn prune_resolved_pending_return_items(queue: &mut Vec<Value>, resolved_headlines: &[String]) {
    if resolved_headlines.is_empty() {
        return;
    }
    queue.retain(|item| {
        !resolved_pending_return_headline_matches(
            item["headline"].as_str().unwrap_or_default(),
            resolved_headlines,
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
        return Some(format!("active: {}", collapse_human_text(active_headline, 72)));
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

fn build_execctl_resume_contract(project_task_tree: &Value, pending_return_queue: &Value) -> Value {
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
    Some(format!(
        "return_required({count}): {}",
        collapse_human_text(headline, 72)
    ))
}

pub(crate) fn build_rotate_chat_action_bundle(
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
    let handoff_command = match (
        project_code,
        namespace_code,
        recommended_headline,
        recommended_next_step,
    ) {
        (Some(project), Some(namespace), Some(headline), Some(next_step)) => {
            Some(shell_join_command(&[
                "amai",
                "continuity",
                "handoff",
                "--project",
                project,
                "--namespace",
                namespace,
                "--headline",
                headline,
                "--next-step",
                next_step,
            ]))
        }
        _ => None,
    };
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
    let rotate_helper_command = match (project_code, namespace_code, repo_root) {
        (Some(project), Some(namespace), Some(root)) => Some(shell_join_command(&[
            "amai",
            "continuity",
            "rotate-chat",
            "--project",
            project,
            "--namespace",
            namespace,
            "--repo-root",
            root,
        ])),
        _ => None,
    };
    json!({
        "bundle_version": "rotate-chat-action-bundle-v1",
        "ready_for_automation": missing_inputs.is_empty(),
        "missing_inputs": missing_inputs,
        "preserves_return_obligation": preserves_return_obligation,
        "recommended_handoff": {
            "available": recommended_headline.is_some() && recommended_next_step.is_some(),
            "headline": recommended_headline,
            "next_step": recommended_next_step,
        },
        "operator_flow": {
            "copy_paste_ready": handoff_command.is_some() && startup_command.is_some(),
            "rotate_helper_command": rotate_helper_command,
            "handoff_command": handoff_command,
            "open_fresh_chat_summary": "открой свежий чат клиента вручную",
            "startup_command": startup_command,
        },
        "order": [
            "capture_continuity_handoff",
            "open_fresh_chat",
            "run_continuity_startup"
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
    let handoff_command = match (
        project_code,
        namespace_code,
        recommended_headline,
        recommended_next_step,
    ) {
        (Some(project), Some(namespace), Some(headline), Some(next_step)) => {
            Some(shell_join_command(&[
                "amai",
                "continuity",
                "handoff",
                "--project",
                project,
                "--namespace",
                namespace,
                "--headline",
                headline,
                "--next-step",
                next_step,
            ]))
        }
        _ => None,
    };
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
    let reply_execution_gate = &client_budget_guard["reply_execution_gate"];
    let should_rotate_chat = client_budget_guard_requires_rotate_before_reply(client_budget_guard);
    let wait_for_global_budget_recovery = reply_execution_gate["action_kind"].as_str()
        == Some("wait_for_global_client_budget_recovery")
        && reply_execution_gate["blocking"].as_bool() == Some(true);
    let client_budget_status = client_budget_guard["status_label"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("новый чат рекомендован");
    let client_budget_note = client_budget_guard["note"]
        .as_str()
        .filter(|value| !value.is_empty());
    if wait_for_global_budget_recovery {
        let preserves_return_obligation = resume_state != "clear";
        json!({
            "action_version": "startup-next-action-v1",
            "action_kind": "wait_for_global_client_budget_recovery",
            "blocking": true,
            "reason": "client_budget_guard_global_exhaustion",
            "resume_state": resume_state,
            "no_silent_drop": no_silent_drop,
            "headline": format!("Клиентский лимит: {client_budget_status}"),
            "next_step": "не продолжай содержательный reply, дождись восстановления внешнего клиентского лимита и только потом снова проверь continuity startup",
            "client_budget_status_label": client_budget_status,
            "client_budget_note": client_budget_note,
            "preserves_return_obligation": preserves_return_obligation,
            "action_bundle": build_wait_for_global_client_budget_action_bundle(
                project_code,
                namespace_code,
                repo_root,
                preserves_return_obligation,
                Some(active_headline),
                Some(active_next_step),
            ),
        })
    } else if should_rotate_chat {
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
            "action_bundle": build_rotate_chat_action_bundle(
                project_code,
                namespace_code,
                repo_root,
                preserves_return_obligation,
                Some(active_headline),
                Some(active_next_step),
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
    Some(format!(
        "{action_kind}: {}",
        collapse_human_text(headline, 72)
    ))
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
    use proptest::prelude::*;
    use serde_json::json;
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
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["headline"], json!("Current active line"));
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
        assert!(
            restore["execctl_active_lease_summary"]
                .as_str()
                .is_some_and(|value| value.contains("same_session_owner"))
        );
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
            bundle["capture_continuity_handoff"]["argv_template"][0],
            json!("amai")
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
        assert_eq!(
            bundle["recommended_handoff"]["headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(
            bundle["recommended_handoff"]["next_step"],
            json!("Materialize live assistant generation source.")
        );
        assert_eq!(bundle["operator_flow"]["copy_paste_ready"], json!(true));
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
                "action_kind": "continue_current_chat",
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
    fn build_startup_next_action_waits_for_global_budget_recovery() {
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
        assert_eq!(
            action["action_kind"],
            json!("wait_for_global_client_budget_recovery")
        );
        assert_eq!(action["blocking"], json!(true));
        assert_eq!(
            action["action_bundle"]["bundle_version"],
            json!("wait-client-budget-action-bundle-v1")
        );
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
            super::build_execctl_resume_contract(&project_task_tree, &pending_return_queue);
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
                "action_kind": "continue_current_chat",
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
}
