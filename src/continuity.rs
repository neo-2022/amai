use crate::cli::{
    ContinuityAnswerArgs, ContinuityHandoffArgs, ContinuityImportArgs, ContinuityStartupArgs,
};
use crate::codex_threads;
use crate::config::AppConfig;
use crate::postgres::{self, ChunkRecord, DocumentRecord, NamespaceRecord, ProjectRecord};
use crate::s3;
use crate::working_state;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
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

#[derive(Debug, Clone, Deserialize)]
struct ContinuityThreadIndexFile {
    #[serde(default)]
    threads: Vec<ContinuityThreadIndexEntry>,
}

#[derive(Debug, Clone, Deserialize)]
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
}

const MAX_SEARCHABLE_CONTINUITY_BYTES: usize = 12_000;

pub async fn import_sources(cfg: &AppConfig, args: &ContinuityImportArgs) -> Result<()> {
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

        let summary = if entry.rendered_transcript.is_empty() {
            None
        } else {
            fs::read_to_string(&entry.rendered_transcript)
                .ok()
                .and_then(|content| {
                    codex_threads::rendered_transcript_summary(
                        &content,
                        &entry.rendered_transcript,
                        Some(&entry.cwd),
                    )
                })
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
                "title": summary.as_ref().map(|value| value["title"].clone()).unwrap_or_else(|| json!(entry.title)),
                "cwd": summary.as_ref().map(|value| value["cwd"].clone()).unwrap_or_else(|| json!(entry.cwd)),
                "first_user_message": summary.as_ref().map(|value| value["first_user_message"].clone()).unwrap_or_else(|| json!(entry.first_user_message)),
                "started_at": summary.as_ref().map(|value| value["started_at"].clone()).unwrap_or_else(|| json!("")),
                "ended_at": summary.as_ref().map(|value| value["ended_at"].clone()).unwrap_or_else(|| json!("")),
                "messages_count": summary.as_ref().map(|value| value["messages_count"].clone()).unwrap_or_else(|| json!(0)),
                "last_user_message": summary.as_ref().map(|value| value["last_user_message"].clone()).unwrap_or_else(|| json!("")),
                "last_assistant_message": summary.as_ref().map(|value| value["last_assistant_message"].clone()).unwrap_or_else(|| json!("")),
                "rendered_transcript": if entry.rendered_transcript.is_empty() { json!("") } else { json!(entry.rendered_transcript) },
                "source_rollout": if entry.source_rollout.is_empty() { json!("") } else { json!(entry.source_rollout) },
                "raw_rollout": if entry.raw_mirror.is_empty() { json!("") } else { json!(entry.raw_mirror) },
                "created_at_epoch_s": summary.as_ref().map(|value| value["created_at_epoch_s"].clone()).unwrap_or_else(|| json!(0)),
                "updated_at_epoch_s": summary.as_ref().map(|value| value["updated_at_epoch_s"].clone()).unwrap_or_else(|| json!(0)),
            }
        });
        let _ = postgres::insert_observability_snapshot(db, "continuity_thread_index", &payload)
            .await?;
    }
    Ok(())
}

pub async fn print_startup(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    let project = resolve_project(&db, args).await?;
    let namespace = postgres::find_namespace_by_code(&db, project.project_id, &args.namespace)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.namespace))?;
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
    println!("Amai continuity startup");
    println!();
    println!("Проект: {} ({})", project.display_name, project.code);
    println!("Корень проекта: {}", project.repo_root);
    println!("Namespace continuity: {}", namespace.code);
    println!(
        "Последний импорт continuity: {}",
        human_epoch_ms(continuity["imported_at_epoch_ms"].as_u64())
    );
    println!(
        "Импортировано документов: {}",
        continuity["documents_imported"].as_u64().unwrap_or(0)
    );
    println!(
        "Continuity snapshot: {}",
        continuity["bootstrap_summary"]["bootstrap_file"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    let bridge_files = continuity["session_memory_files"].as_u64().unwrap_or(0);
    if bridge_files > 0 {
        println!("Дополнительные bridge-notes: {}", bridge_files);
    }
    println!(
        "Rendered transcripts: {}",
        continuity["rendered_transcript_files"]
            .as_u64()
            .unwrap_or(0)
    );
    println!();
    println!("Текущая активная линия:");
    println!(
        "- {}",
        handoff_summary["headline"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    println!(
        "- Ближайший обязательный следующий шаг: {}",
        handoff_summary["next_step"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    if let Some(restore) = working_state::build_restore_bundle(&db, &project, &namespace).await? {
        println!();
        working_state::print_restore_bundle_human(&restore);
    }
    println!();
    println!("Bootstrap continuity:");
    println!(
        "- Thread count: {}",
        continuity["bootstrap_summary"]["details"]["thread_count"]
            .as_u64()
            .unwrap_or(0)
    );
    println!(
        "- Последний rendered transcript: {}",
        continuity["bootstrap_summary"]["details"]["latest_rendered_transcript"]
            .as_str()
            .unwrap_or("ещё нет данных")
    );
    println!();
    let mut import_command = format!(
        "cargo run -- continuity import --project {} --display-name '{}' --repo-root {} --namespace {} --bootstrap-file {}",
        project.code,
        project.display_name.replace('\'', "\\'"),
        shell_quote(&project.repo_root),
        namespace.code,
        shell_quote(
            continuity["bootstrap_summary"]["bootstrap_file"]
                .as_str()
                .unwrap_or_default()
        ),
    );
    let active_workline_arg = continuity["active_workline_summary"]["active_workline_file"]
        .as_str()
        .unwrap_or_default();
    if !active_workline_arg.is_empty() {
        import_command.push_str(" --active-workline-file ");
        import_command.push_str(&shell_quote(active_workline_arg));
    }
    println!("Как использовать дальше:");
    println!(
        "- Для project-scoped retrieval: cargo run -- context pack --project {} --namespace {} --query 'ваш вопрос'",
        project.code, namespace.code
    );
    println!("- Для обновления continuity после новых изменений: {import_command}");
    Ok(())
}

pub async fn print_restore(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    let project = resolve_project(&db, args).await?;
    let namespace = postgres::find_namespace_by_code(&db, project.project_id, &args.namespace)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.namespace))?;
    let restore = working_state::build_restore_bundle(&db, &project, &namespace)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "no working-state restore bundle found for {}::{}",
                project.code,
                namespace.code
            )
        })?;
    println!("{}", serde_json::to_string_pretty(&restore)?);
    Ok(())
}

pub async fn print_answer(cfg: &AppConfig, args: &ContinuityAnswerArgs) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
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
    let wants_chat_lookup = args.include_chat_messages
        || args.at_time_rfc3339.is_some()
        || args.chat_reference.is_some()
        || args.intent == "previous_chat"
        || args.intent == "chat_at_time";
    let chat_tail =
        if wants_chat_lookup {
            let thread_index_snapshots = postgres::list_observability_snapshots_by_kinds(
                &db,
                &["continuity_thread_index"],
                Some(200),
            )
            .await?;
            if let Some(at_time_rfc3339) = &args.at_time_rfc3339 {
                codex_threads::chat_tail_at_time(
                    &project.repo_root,
                    at_time_rfc3339,
                    args.messages_count,
                )?
                .or(codex_threads::chat_tail_at_time_from_snapshots(
                    &thread_index_snapshots,
                    &project.code,
                    &namespace.code,
                    at_time_rfc3339,
                    args.messages_count,
                )?)
            } else {
                let chat_reference =
                    args.chat_reference
                        .as_deref()
                        .unwrap_or(if args.intent == "previous_chat" {
                            "previous"
                        } else {
                            "current"
                        });
                match chat_reference {
                    "previous" => {
                        codex_threads::previous_chat_tail(&project.repo_root, args.messages_count)?
                            .or(codex_threads::previous_chat_tail_from_snapshots(
                                &thread_index_snapshots,
                                &project.code,
                                &namespace.code,
                                current_thread_id.as_deref(),
                                args.messages_count,
                            ))
                    }
                    "current" => {
                        codex_threads::current_chat_tail(&project.repo_root, args.messages_count)?
                            .or(codex_threads::current_chat_tail_from_snapshots(
                                &thread_index_snapshots,
                                &project.code,
                                &namespace.code,
                                current_thread_id.as_deref(),
                                args.messages_count,
                            ))
                    }
                    _ => None,
                }
            }
        } else {
            None
        };
    println!(
        "{}",
        render_direct_answer(
            &handoff_summary,
            restore.as_ref(),
            chat_tail.as_ref(),
            &args.intent,
            args.at_time_rfc3339.as_deref(),
        )
    );
    Ok(())
}

pub async fn capture_handoff(cfg: &AppConfig, args: &ContinuityHandoffArgs) -> Result<()> {
    let mut db = postgres::connect_admin(cfg).await?;
    let project = postgres::get_project_by_code(&db, &args.project).await?;
    let namespace = postgres::find_namespace_by_code(&db, project.project_id, &args.namespace)
        .await?
        .ok_or_else(|| anyhow!("continuity namespace not found: {}", args.namespace))?;
    let details = if let Some(details_file) = &args.details_file {
        fs::read_to_string(details_file)
            .with_context(|| format!("failed to read {}", details_file.display()))?
    } else {
        String::new()
    };
    let captured_at_epoch_ms = now_epoch_ms()?;
    let body = render_handoff_markdown(&args.headline, &args.next_step, &details);
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
    postgres::replace_document_index(&mut db, &document, &[], &chunks).await?;
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
            "headline": args.headline,
            "next_step": args.next_step,
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
        &args.headline,
        &args.next_step,
        &details,
        &local_handoff_path.display().to_string(),
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn render_direct_answer(
    handoff_summary: &Value,
    restore: Option<&Value>,
    chat_tail: Option<&codex_threads::ChatTail>,
    intent: &str,
    at_time_rfc3339: Option<&str>,
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
    let next_step = handoff_summary["next_step"]
        .as_str()
        .unwrap_or("ещё нет данных");

    let mut lines = vec![format!("{heading} {headline}")];
    if intent != "previous_chat"
        && intent != "chat_at_time"
        && let Some(restore_node) = restore.map(|value| &value["working_state_restore"])
    {
        if let Some(summary) = summarize_materialized_notes(&restore_node["materialized_notes"]) {
            lines.push(format!("Что уже materialized: {summary}"));
        }
        if let Some(summary) = summarize_recent_actions(&restore_node["recent_actions"]) {
            lines.push(format!("Последние действия: {summary}"));
        }
        if let Some(summary) = summarize_string_list(&restore_node["active_files"], 3) {
            lines.push(format!("Активные файлы: {summary}"));
        }
        if restore_node["restore_confidence"].as_str() == Some("preliminary") {
            lines.push(
                "Статус continuity: предварительно, потому что живая выборка ещё маленькая."
                    .to_string(),
            );
        }
    }
    if let Some(chat_tail) = chat_tail {
        let title = if chat_tail.title.trim().is_empty() {
            chat_tail.thread_id.as_str()
        } else {
            chat_tail.title.as_str()
        };
        if let Some(at_time_rfc3339) = at_time_rfc3339 {
            lines.push(format!("Целевой момент времени: {at_time_rfc3339}"));
            lines.push(format!("Подходящий chat thread: {title}"));
        } else if intent == "previous_chat" {
            lines.push(format!("Предыдущий чат по времени: {title}"));
        } else {
            lines.push(format!("Найденный chat thread: {title}"));
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
                lines.push(format!("- {role}: {}", message.text));
            }
        }
    }
    lines.push(format!("Ближайший обязательный следующий шаг: {next_step}"));
    lines.join("\n")
}

fn summarize_materialized_notes(value: &Value) -> Option<String> {
    summarize_string_list(value, 2)
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
    Ok(snapshots
        .into_iter()
        .find(|snapshot| {
            snapshot.payload["continuity_handoff"]["project"]["code"].as_str()
                == Some(project.code.as_str())
                && snapshot.payload["continuity_handoff"]["namespace"]["code"].as_str()
                    == Some(namespace.code.as_str())
                && !is_meta_continuity_handoff(
                    snapshot.payload["continuity_handoff"]["headline"]
                        .as_str()
                        .unwrap_or_default(),
                    snapshot.payload["continuity_handoff"]["next_step"]
                        .as_str()
                        .unwrap_or_default(),
                    snapshot.payload["continuity_handoff"]["details"]
                        .as_str()
                        .unwrap_or_default(),
                )
        })
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
        return postgres::get_project_by_code(db, project).await;
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
    use super::{is_meta_continuity_handoff, render_direct_answer};
    use crate::codex_threads::{ChatTail, TranscriptMessage};
    use serde_json::json;

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

        let answer = render_direct_answer(&handoff, Some(&restore), None, "last_chat", None);

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
        );

        assert!(answer.contains("Что было в чате на этот момент: Temporal lookup materialized"));
        assert!(answer.contains("Целевой момент времени: 2026-03-19T12:00:00+03:00"));
        assert!(answer.contains("Подходящий chat thread: чат про continuity"));
        assert!(answer.contains("Ближайшие сообщения к этому моменту:"));
        assert!(answer.contains("- Ваше: о чём говорили?"));
        assert!(answer.contains("- Моё: про temporal lookup"));
    }
}
