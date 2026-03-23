use crate::postgres::{self, NamespaceRecord, ObservabilitySnapshotRecord, ProjectRecord};
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

pub async fn record_handoff_event(
    db: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    headline: &str,
    next_step: &str,
    details: &str,
    local_path: &str,
) -> Result<()> {
    let recorded_at_epoch_ms = now_epoch_ms()?;
    let agent_scope = current_agent_scope_for(&project.code, &namespace.code);
    let next_step = normalize_next_step_hint(next_step);
    let thread_id = current_thread_id();
    let session_id = resolve_session_id(
        db,
        &project.code,
        &namespace.code,
        &agent_scope,
        recorded_at_epoch_ms,
    )
    .await?;
    let active_files = extract_paths_from_text(details);
    let payload = json!({
        "working_state_event": {
            "event_id": Uuid::new_v4().to_string(),
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
            "summary": summarize_details(details, headline, &next_step),
            "active_files": active_files,
            "recent_paths": extract_paths_from_text(details),
            "visible_projects": vec![project.code.clone()],
            "query": Value::Null,
            "query_type": Value::Null,
            "target_kind": "handoff",
            "current_hypothesis": extract_first_question(details),
            "rejected_hypotheses": Vec::<String>::new(),
            "open_questions": derive_open_questions(details),
            "materialized_notes": extract_materialized_notes(details),
            "last_command": "continuity handoff".to_string(),
            "last_results_summary": format!("Зафиксирован handoff для {} :: {}", project.code, namespace.code),
            "local_path": local_path,
        }
    });
    postgres::insert_observability_snapshot(db, WORKING_STATE_EVENT_KIND, &payload).await?;
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
    let snapshots = postgres::list_observability_snapshots_by_kinds(
        db,
        &[WORKING_STATE_EVENT_KIND],
        Some(MAX_RESTORE_EVENTS),
    )
    .await?;
    let events = select_relevant_events(
        snapshots,
        &project.code,
        &namespace.code,
        &current_agent_scope_for(&project.code, &namespace.code),
    );
    if events.is_empty() {
        return Ok(None);
    }
    Ok(Some(compose_restore_bundle(
        &project_json(project),
        &namespace_json(namespace),
        &events,
    )))
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
    let restore_freshness_state =
        if now_epoch_ms.saturating_sub(latest_recorded_at) > 15 * 60 * 1000 {
            "stale"
        } else {
            "fresh"
        };
    let merged_workspace_graph = workspace_graph::merge_workspace_graphs(&workspace_graph_inputs);

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
            "last_command": last_command,
            "last_results_summary": last_results_summary,
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
                        "workspace_graph_model_version": "workspace-graph-v3",
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
                        "workspace_graph_model_version": "workspace-graph-v3",
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
