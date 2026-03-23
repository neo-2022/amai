use crate::chat_question;
use crate::cli::{
    ContinuityAnswerArgs, ContinuityHandoffArgs, ContinuityImportArgs, ContinuityStartupArgs,
    ContinuityThreadIndexEnrichArgs,
};
use crate::codex_threads;
use crate::config::AppConfig;
use crate::postgres::{self, ChunkRecord, DocumentRecord, NamespaceRecord, ProjectRecord};
use crate::s3;
use crate::working_state;
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
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
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
    let db = postgres::connect_admin(cfg).await?;
    let context = load_startup_context(&db, args).await?;
    let chat_start_restore = build_chat_start_restore(
        &context.project,
        &context.namespace,
        &context.continuity,
        &context.handoff_summary,
        context.restore.as_ref(),
    );
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

pub async fn print_restore(cfg: &AppConfig, args: &ContinuityStartupArgs) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
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
    let mut output = serde_json::Map::new();
    output.insert(
        "chat_start_restore".to_string(),
        chat_start_restore["chat_start_restore"].clone(),
    );
    if let Some(node) = restore.get("working_state_restore") {
        output.insert("working_state_restore".to_string(), node.clone());
    }
    println!("{}", serde_json::to_string_pretty(&Value::Object(output))?);
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
    let parsed_question = args.question.as_deref().and_then(|question| {
        chat_question::interpret(question, chat_question::current_local_now())
    });
    let mut intent = if args.intent != "last_chat" {
        args.intent.clone()
    } else {
        parsed_question
            .as_ref()
            .map(|value| value.intent.clone())
            .unwrap_or_else(|| args.intent.clone())
    };
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
    if intent == "last_chat" && parsed_chat_reference.0 == "previous" {
        intent = "previous_chat".to_string();
    }
    let at_time_rfc3339 = args.at_time_rfc3339.clone().or_else(|| {
        parsed_question
            .as_ref()
            .and_then(|value| value.at_time_rfc3339.clone())
    });
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
    println!(
        "{}",
        render_direct_answer(
            &handoff_summary,
            restore.as_ref(),
            chat_tail.as_ref(),
            &intent,
            at_time_rfc3339.as_deref(),
            if wants_chat_lookup && parsed_chat_reference.0 == "previous" {
                parsed_chat_reference.1
            } else {
                1
            },
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
        restore_node.and_then(|value| summarize_materialized_notes(&value["materialized_notes"]));
    let recent_actions_summary =
        restore_node.and_then(|value| summarize_recent_actions(&value["recent_actions"]));
    let active_files_summary =
        restore_node.and_then(|value| summarize_string_list(&value["active_files"], 4));
    let open_questions_summary =
        restore_node.and_then(|value| summarize_string_list(&value["open_questions"], 3));
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

fn render_chat_start_prompt(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    handoff_summary: &Value,
    restore_node: Option<&Value>,
    thread_count: u64,
) -> String {
    let headline = handoff_summary["headline"]
        .as_str()
        .unwrap_or("ещё нет данных");
    let next_step = handoff_summary["next_step"]
        .as_str()
        .and_then(normalize_next_step_value)
        .unwrap_or_else(|| "ещё нет данных".to_string());
    let current_goal = restore_node
        .and_then(|value| value["current_goal"].as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(headline);
    let materialized_summary =
        restore_node.and_then(|value| summarize_materialized_notes(&value["materialized_notes"]));
    let recent_actions_summary =
        restore_node.and_then(|value| summarize_recent_actions(&value["recent_actions"]));
    let active_files_summary =
        restore_node.and_then(|value| summarize_string_list(&value["active_files"], 4));
    let open_questions_summary =
        restore_node.and_then(|value| summarize_string_list(&value["open_questions"], 3));
    let restore_confidence = restore_node
        .and_then(|value| value["restore_confidence"].as_str())
        .unwrap_or("preliminary");
    let mut lines = vec![
        "CHAT_START_RESTORE".to_string(),
        format!("Project: {} ({})", project.display_name, project.code),
        format!("Namespace: {}", namespace.code),
        format!("Продолжаем с линии: {headline}"),
        format!("Обязательный следующий шаг: {next_step}"),
    ];
    if current_goal != headline {
        lines.push(format!("Текущая цель: {current_goal}"));
    }
    if let Some(value) = materialized_summary {
        lines.push(format!("Что уже materialized: {value}"));
    }
    if let Some(value) = recent_actions_summary {
        lines.push(format!("Недавние действия: {value}"));
    }
    if let Some(value) = active_files_summary {
        lines.push(format!("Активные файлы: {value}"));
    }
    if let Some(value) = open_questions_summary {
        lines.push(format!("Открытые вопросы: {value}"));
    }
    lines.push(format!("Thread count in continuity index: {thread_count}"));
    if restore_confidence == "preliminary" {
        lines.push(
            "Статус recovery: preliminary; first substantive reply should still continue from this pack, not restart continuity from zero.".to_string(),
        );
    }
    lines.push(
        "Используй этот блок как восстановленный рабочий контекст для первого содержательного ответа нового чата и не трать первый ответ на повторное восстановление continuity, если пользователь не попросил этого явно.".to_string(),
    );
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

fn resolve_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    Ok(cwd.join(path))
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
        build_chat_start_restore, degradation_proof_scenarios, enrich_thread_index_file,
        extract_next_step_from_text, is_meta_continuity_handoff, parse_chat_reference_spec,
        render_direct_answer,
    };
    use crate::cli::ContinuityThreadIndexEnrichArgs;
    use crate::codex_threads::{ChatTail, ThreadTimeSliceSummary, TranscriptMessage};
    use crate::postgres::{NamespaceRecord, ProjectRecord};
    use serde_json::json;
    use std::fs;

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
        assert!(
            prompt.contains("Продолжаем с линии: Amai upstream thread-index enrich materialized")
        );
        assert!(prompt.contains("Обязательный следующий шаг: Сделать auto-injection restore pack прямо в chat-start prompt."));
        assert!(prompt.contains(
            "Недавние действия: Проверили previous chat lookup; Проверили exact-time lookup"
        ));
        assert_eq!(node["thread_count"], json!(16));
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
