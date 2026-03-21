use crate::postgres::ObservabilitySnapshotRecord;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ChatTail {
    pub thread_id: String,
    pub title: String,
    pub summary_headline: Option<String>,
    pub summary_next_step: Option<String>,
    pub messages: Vec<TranscriptMessage>,
}

#[derive(Debug, Clone)]
struct ThreadRecord {
    thread_id: String,
    title: String,
    cwd: String,
    first_user_message: String,
    rollout_path: String,
    created_at_epoch_s: i64,
    updated_at_epoch_s: i64,
}

#[derive(Debug, Clone)]
struct RolloutMessage {
    timestamp: String,
    role: String,
    phase: Option<String>,
    text: String,
}

#[derive(Debug, Clone)]
struct RolloutSummary {
    started_at: String,
    ended_at: String,
    messages_count: usize,
    last_user_message: String,
    last_assistant_message: String,
    summary_headline: Option<String>,
    summary_next_step: Option<String>,
    tail_messages: Vec<TranscriptMessage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThreadIndexSummary {
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub messages_count: usize,
    #[serde(default)]
    pub last_user_message: String,
    #[serde(default)]
    pub last_assistant_message: String,
    #[serde(default)]
    pub summary_headline: String,
    #[serde(default)]
    pub summary_next_step: String,
    #[serde(default)]
    pub created_at_epoch_s: i64,
    #[serde(default)]
    pub updated_at_epoch_s: i64,
}

#[derive(Debug, Deserialize)]
struct ThreadIndexFile {
    #[serde(default)]
    threads: Vec<ThreadIndexEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ThreadIndexEntry {
    #[serde(default)]
    thread_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    source_rollout: String,
    #[serde(default)]
    raw_mirror: String,
    #[serde(default)]
    rendered_transcript: String,
    #[serde(default)]
    summary_headline: String,
    #[serde(default)]
    summary_next_step: String,
}

const SYNTHETIC_AGENTS_PREFIX: &str = "# AGENTS.md instructions for ";
const SYNTHETIC_INSTRUCTIONS_MARKER: &str = "<INSTRUCTIONS>";

pub fn current_thread_id() -> Option<String> {
    env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn derive_thread_index_summary(
    cwd: Option<&str>,
    rendered_transcript: Option<&Path>,
    source_rollout: Option<&Path>,
    raw_mirror: Option<&Path>,
) -> Result<Option<ThreadIndexSummary>> {
    if let Some(path) = rendered_transcript.filter(|path| path.exists()) {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if let Some(summary) =
            rendered_transcript_summary(&content, &path.display().to_string(), cwd)
        {
            return Ok(Some(thread_index_summary_from_value(&summary)));
        }
    }

    let rollout_path = source_rollout
        .filter(|path| path.exists())
        .or_else(|| raw_mirror.filter(|path| path.exists()));
    let Some(path) = rollout_path else {
        return Ok(None);
    };
    let summary = rollout_summary_from_path(path, 2)?;
    Ok(Some(ThreadIndexSummary {
        started_at: summary.started_at.clone(),
        ended_at: summary.ended_at.clone(),
        messages_count: summary.messages_count,
        last_user_message: summary.last_user_message,
        last_assistant_message: summary.last_assistant_message,
        summary_headline: summary.summary_headline.unwrap_or_default(),
        summary_next_step: summary.summary_next_step.unwrap_or_default(),
        created_at_epoch_s: parse_rfc3339_epoch_s(&summary.started_at).unwrap_or_default(),
        updated_at_epoch_s: parse_rfc3339_epoch_s(&summary.ended_at).unwrap_or_default(),
    }))
}

pub fn nth_previous_chat_tail(
    repo_root: &str,
    offset: usize,
    count: usize,
) -> Result<Option<ChatTail>> {
    let offset = offset.max(1);
    if let Some(record) = previous_thread_record(repo_root, current_thread_id().as_deref(), offset)?
    {
        return build_previous_chat_tail(
            &record.thread_id,
            &record.title,
            &record.rollout_path,
            count,
        )
        .map(Some);
    }

    let index_path = thread_index_path()?;
    if !index_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&index_path)
        .with_context(|| format!("failed to read {}", index_path.display()))?;
    let mut index: ThreadIndexFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", index_path.display()))?;
    index
        .threads
        .sort_by(|left, right| left.thread_id.cmp(&right.thread_id));
    let current_thread = current_thread_id();
    let Some(entry) = index
        .threads
        .into_iter()
        .rev()
        .filter(|item| {
            item.cwd.starts_with(repo_root)
                && Some(item.thread_id.as_str()) != current_thread.as_deref()
        })
        .nth(offset.saturating_sub(1))
    else {
        return Ok(None);
    };

    let rollout_path = if !entry.raw_mirror.is_empty() {
        entry.raw_mirror
    } else {
        entry.source_rollout
    };
    if !rollout_path.is_empty() {
        return build_previous_chat_tail(&entry.thread_id, &entry.title, &rollout_path, count)
            .map(Some);
    }
    if entry.rendered_transcript.is_empty() {
        return Ok(None);
    }
    let rendered_path = PathBuf::from(&entry.rendered_transcript);
    let messages = extract_last_messages(&rendered_path, count)?;
    Ok(Some(ChatTail {
        thread_id: entry.thread_id,
        title: sanitize_chat_title(&entry.title, &messages),
        summary_headline: if entry.summary_headline.is_empty() {
            messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .and_then(|message| compact_headline_from_text(&message.text, 220))
        } else {
            Some(entry.summary_headline)
        },
        summary_next_step: if entry.summary_next_step.is_empty() {
            messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .and_then(|message| compact_next_step_from_text(&message.text))
        } else {
            Some(entry.summary_next_step)
        },
        messages,
    }))
}

pub fn current_chat_tail(repo_root: &str, count: usize) -> Result<Option<ChatTail>> {
    if let Some(record) = current_thread_record(repo_root, current_thread_id().as_deref())? {
        return build_previous_chat_tail(
            &record.thread_id,
            &record.title,
            &record.rollout_path,
            count,
        )
        .map(Some);
    }

    let index_path = thread_index_path()?;
    if !index_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&index_path)
        .with_context(|| format!("failed to read {}", index_path.display()))?;
    let mut index: ThreadIndexFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", index_path.display()))?;
    index
        .threads
        .sort_by(|left, right| left.thread_id.cmp(&right.thread_id));
    let current_thread = current_thread_id();
    let entry = if let Some(current_thread) = current_thread.as_deref() {
        index
            .threads
            .into_iter()
            .find(|item| item.cwd.starts_with(repo_root) && item.thread_id == current_thread)
    } else {
        index
            .threads
            .into_iter()
            .rev()
            .find(|item| item.cwd.starts_with(repo_root))
    };
    let Some(entry) = entry else {
        return Ok(None);
    };

    let rollout_path = if !entry.raw_mirror.is_empty() {
        entry.raw_mirror
    } else {
        entry.source_rollout
    };
    if !rollout_path.is_empty() {
        return build_previous_chat_tail(&entry.thread_id, &entry.title, &rollout_path, count)
            .map(Some);
    }
    if entry.rendered_transcript.is_empty() {
        return Ok(None);
    }
    let rendered_path = PathBuf::from(&entry.rendered_transcript);
    let messages = extract_last_messages(&rendered_path, count)?;
    Ok(Some(ChatTail {
        thread_id: entry.thread_id,
        title: sanitize_chat_title(&entry.title, &messages),
        summary_headline: if entry.summary_headline.is_empty() {
            messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .and_then(|message| compact_headline_from_text(&message.text, 220))
        } else {
            Some(entry.summary_headline)
        },
        summary_next_step: if entry.summary_next_step.is_empty() {
            messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .and_then(|message| compact_next_step_from_text(&message.text))
        } else {
            Some(entry.summary_next_step)
        },
        messages,
    }))
}

pub fn nth_previous_chat_tail_from_snapshots(
    snapshots: &[ObservabilitySnapshotRecord],
    project_code: &str,
    namespace_code: &str,
    current_thread_id: Option<&str>,
    offset: usize,
    count: usize,
) -> Option<ChatTail> {
    let offset = offset.max(1);
    let snapshot = snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.payload["continuity_thread_index"]["project"]["code"].as_str()
                == Some(project_code)
                && snapshot.payload["continuity_thread_index"]["namespace"]["code"].as_str()
                    == Some(namespace_code)
                && snapshot.payload["continuity_thread_index"]["thread_id"].as_str()
                    != current_thread_id
        })
        .collect::<Vec<_>>();
    let mut scoped = snapshot
        .into_iter()
        .map(|snapshot| {
            let key = (
                snapshot.payload["continuity_thread_index"]["updated_at_epoch_s"]
                    .as_i64()
                    .unwrap_or_default(),
                snapshot.created_at_epoch_ms,
            );
            (key, snapshot)
        })
        .collect::<Vec<_>>();
    scoped.sort_by(|left, right| right.0.cmp(&left.0));
    let snapshot = scoped
        .into_iter()
        .nth(offset.saturating_sub(1))
        .map(|(_, snapshot)| snapshot)?;
    let node = &snapshot.payload["continuity_thread_index"];
    let messages = snapshot_messages(node, count)
        .or_else(|| snapshot_rollout_messages(node, count).ok().flatten())
        .unwrap_or_default();
    Some(ChatTail {
        thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
        title: sanitize_chat_title(node["title"].as_str().unwrap_or_default(), &messages),
        summary_headline: node["summary_headline"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        summary_next_step: node["summary_next_step"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        messages,
    })
}

pub fn current_chat_tail_from_snapshots(
    snapshots: &[ObservabilitySnapshotRecord],
    project_code: &str,
    namespace_code: &str,
    current_thread_id: Option<&str>,
    count: usize,
) -> Option<ChatTail> {
    let snapshot = if let Some(current_thread_id) = current_thread_id {
        snapshots.iter().find(|snapshot| {
            snapshot.payload["continuity_thread_index"]["project"]["code"].as_str()
                == Some(project_code)
                && snapshot.payload["continuity_thread_index"]["namespace"]["code"].as_str()
                    == Some(namespace_code)
                && snapshot.payload["continuity_thread_index"]["thread_id"].as_str()
                    == Some(current_thread_id)
        })
    } else {
        snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.payload["continuity_thread_index"]["project"]["code"].as_str()
                    == Some(project_code)
                    && snapshot.payload["continuity_thread_index"]["namespace"]["code"].as_str()
                        == Some(namespace_code)
            })
            .max_by_key(|snapshot| {
                (
                    snapshot.payload["continuity_thread_index"]["updated_at_epoch_s"]
                        .as_i64()
                        .unwrap_or_default(),
                    snapshot.created_at_epoch_ms,
                )
            })
    }?;
    let node = &snapshot.payload["continuity_thread_index"];
    let messages = snapshot_messages(node, count)
        .or_else(|| snapshot_rollout_messages(node, count).ok().flatten())
        .unwrap_or_default();
    Some(ChatTail {
        thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
        title: sanitize_chat_title(node["title"].as_str().unwrap_or_default(), &messages),
        summary_headline: node["summary_headline"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        summary_next_step: node["summary_next_step"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        messages,
    })
}

pub fn chat_tail_at_time(
    repo_root: &str,
    at_time_rfc3339: &str,
    count: usize,
) -> Result<Option<ChatTail>> {
    let target_epoch_s = parse_rfc3339_epoch_s(at_time_rfc3339)?;
    let Some(record) = thread_record_at_time(repo_root, target_epoch_s)? else {
        return Ok(None);
    };
    build_chat_tail_at_time(&record, target_epoch_s, count).map(Some)
}

pub fn chat_tail_at_time_from_snapshots(
    snapshots: &[ObservabilitySnapshotRecord],
    project_code: &str,
    namespace_code: &str,
    at_time_rfc3339: &str,
    count: usize,
) -> Result<Option<ChatTail>> {
    let target_epoch_s = parse_rfc3339_epoch_s(at_time_rfc3339)?;
    let scoped_snapshots = snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.payload["continuity_thread_index"]["project"]["code"].as_str()
                == Some(project_code)
                && snapshot.payload["continuity_thread_index"]["namespace"]["code"].as_str()
                    == Some(namespace_code)
        })
        .collect::<Vec<_>>();
    if !target_is_within_snapshot_bounds(&scoped_snapshots, target_epoch_s) {
        return Ok(None);
    }
    let snapshot = scoped_snapshots
        .into_iter()
        .filter_map(|snapshot| {
            let node = &snapshot.payload["continuity_thread_index"];
            let (started_at_epoch_s, ended_at_epoch_s) = snapshot_window_epoch_s(node);
            let width = if started_at_epoch_s > 0
                && ended_at_epoch_s > 0
                && ended_at_epoch_s >= started_at_epoch_s
            {
                ended_at_epoch_s - started_at_epoch_s
            } else {
                i64::MAX
            };
            let contains = started_at_epoch_s > 0
                && ended_at_epoch_s > 0
                && started_at_epoch_s <= target_epoch_s
                && target_epoch_s <= ended_at_epoch_s;
            let before = ended_at_epoch_s > 0 && ended_at_epoch_s <= target_epoch_s;
            let after = started_at_epoch_s > 0 && started_at_epoch_s >= target_epoch_s;
            let rank = if contains {
                (
                    0_i32,
                    width,
                    target_epoch_s - ended_at_epoch_s,
                    started_at_epoch_s,
                )
            } else if before {
                (
                    1_i32,
                    target_epoch_s - ended_at_epoch_s,
                    width,
                    started_at_epoch_s,
                )
            } else if after {
                (
                    2_i32,
                    started_at_epoch_s - target_epoch_s,
                    width,
                    started_at_epoch_s,
                )
            } else {
                return None;
            };
            Some((rank, snapshot))
        })
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, snapshot)| snapshot);
    let Some(snapshot) = snapshot else {
        return Ok(None);
    };
    let node = &snapshot.payload["continuity_thread_index"];
    if let Some(messages) = snapshot_rollout_messages_at_time(node, target_epoch_s, count)? {
        return Ok(Some(ChatTail {
            thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
            title: sanitize_chat_title(node["title"].as_str().unwrap_or_default(), &messages),
            summary_headline: node["summary_headline"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            summary_next_step: node["summary_next_step"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            messages,
        }));
    }
    let messages = snapshot_messages(node, count).unwrap_or_default();
    Ok(Some(ChatTail {
        thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
        title: sanitize_chat_title(node["title"].as_str().unwrap_or_default(), &messages),
        summary_headline: node["summary_headline"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        summary_next_step: node["summary_next_step"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        messages,
    }))
}

pub fn rendered_transcript_summary(
    content: &str,
    transcript_path: &str,
    thread_cwd: Option<&str>,
) -> Option<Value> {
    let parsed_title = content
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .unwrap_or_default()
        .to_string();
    let thread_id = extract_field(content, "- `thread_id`: `")?;
    let record = thread_record_by_id(&thread_id).ok().flatten();
    let rendered_messages = extract_messages_from_rendered_text(content);
    let source_rollout = record
        .as_ref()
        .map(|item| item.rollout_path.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| extract_field(content, "- `source_rollout`: `"))
        .unwrap_or_default();
    let rollout_summary = if source_rollout.is_empty() {
        None
    } else {
        rollout_summary_from_path(Path::new(&source_rollout), 2).ok()
    };
    let title = record
        .as_ref()
        .map(|item| item.title.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or(parsed_title);
    let cwd = record
        .as_ref()
        .map(|item| item.cwd.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| extract_field(content, "- `cwd`: `"))
        .or(thread_cwd.map(ToOwned::to_owned))
        .unwrap_or_default();
    let first_user_message = record
        .as_ref()
        .map(|item| item.first_user_message.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| extract_field(content, "- `first_user_message`: `"))
        .unwrap_or_default();
    let started_at = rollout_summary
        .as_ref()
        .map(|summary| summary.started_at.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            rendered_messages
                .first()
                .map(|message| message.timestamp.clone())
        })
        .unwrap_or_default();
    let ended_at = rollout_summary
        .as_ref()
        .map(|summary| summary.ended_at.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            rendered_messages
                .last()
                .map(|message| message.timestamp.clone())
        })
        .unwrap_or_default();
    let messages_count = rollout_summary
        .as_ref()
        .map(|summary| summary.messages_count)
        .unwrap_or(rendered_messages.len());
    let last_user_message = rollout_summary
        .as_ref()
        .map(|summary| summary.last_user_message.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            rendered_messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| message.text.clone())
        })
        .unwrap_or_default();
    let last_assistant_message = rollout_summary
        .as_ref()
        .map(|summary| summary.last_assistant_message.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            rendered_messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .map(|message| message.text.clone())
        })
        .unwrap_or_default();
    let summary_headline = rollout_summary
        .as_ref()
        .and_then(|summary| summary.summary_headline.clone())
        .or_else(|| compact_headline_from_text(&last_assistant_message, 220));
    let summary_next_step = rollout_summary
        .as_ref()
        .and_then(|summary| summary.summary_next_step.clone())
        .or_else(|| compact_next_step_from_text(&last_assistant_message));

    Some(json!({
        "thread_id": thread_id,
        "title": title,
        "cwd": cwd,
        "first_user_message": first_user_message,
        "started_at": started_at,
        "ended_at": ended_at,
        "messages_count": messages_count,
        "last_user_message": last_user_message,
        "last_assistant_message": last_assistant_message,
        "summary_headline": summary_headline,
        "summary_next_step": summary_next_step,
        "rendered_transcript": transcript_path,
        "source_rollout": source_rollout,
        "created_at_epoch_s": record.as_ref().map(|item| item.created_at_epoch_s).unwrap_or_default(),
        "updated_at_epoch_s": record.as_ref().map(|item| item.updated_at_epoch_s).unwrap_or_default(),
    }))
}

fn thread_index_summary_from_value(value: &Value) -> ThreadIndexSummary {
    ThreadIndexSummary {
        started_at: value["started_at"].as_str().unwrap_or_default().to_string(),
        ended_at: value["ended_at"].as_str().unwrap_or_default().to_string(),
        messages_count: value["messages_count"].as_u64().unwrap_or_default() as usize,
        last_user_message: value["last_user_message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        last_assistant_message: value["last_assistant_message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        summary_headline: value["summary_headline"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        summary_next_step: value["summary_next_step"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        created_at_epoch_s: value["created_at_epoch_s"].as_i64().unwrap_or_default(),
        updated_at_epoch_s: value["updated_at_epoch_s"].as_i64().unwrap_or_default(),
    }
}

fn previous_thread_record(
    repo_root: &str,
    current_thread_id: Option<&str>,
    offset: usize,
) -> Result<Option<ThreadRecord>> {
    let Some(db_path) = codex_db_path() else {
        return Ok(None);
    };
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    let current = current_thread_id
        .map(|thread_id| load_thread_record(&conn, thread_id))
        .transpose()?
        .flatten();
    let repo_prefix = format!("{repo_root}/%");

    let query = if current.is_some() {
        r#"
        SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
        FROM threads
        WHERE (cwd = ?1 OR cwd LIKE ?2)
          AND (
            updated_at < ?3
            OR (updated_at = ?3 AND id < ?4)
          )
        ORDER BY updated_at DESC, id DESC
        LIMIT 1 OFFSET ?5
        "#
    } else {
        r#"
        SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
        FROM threads
        WHERE (cwd = ?1 OR cwd LIKE ?2)
          AND (?3 IS NULL OR id != ?3)
        ORDER BY updated_at DESC, id DESC
        LIMIT 1 OFFSET ?4
        "#
    };

    let record = if let Some(current) = current {
        conn.query_row(
            query,
            params![
                repo_root,
                repo_prefix,
                current.updated_at_epoch_s,
                current.thread_id,
                offset.saturating_sub(1) as i64
            ],
            map_thread_record,
        )
        .optional()?
    } else {
        conn.query_row(
            query,
            params![
                repo_root,
                repo_prefix,
                current_thread_id,
                offset.saturating_sub(1) as i64
            ],
            map_thread_record,
        )
        .optional()?
    };
    Ok(record)
}

fn current_thread_record(
    repo_root: &str,
    current_thread_id: Option<&str>,
) -> Result<Option<ThreadRecord>> {
    let Some(db_path) = codex_db_path() else {
        return Ok(None);
    };
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    if let Some(current_thread_id) = current_thread_id {
        return load_thread_record(&conn, current_thread_id);
    }

    let repo_prefix = format!("{repo_root}/%");
    let record = conn
        .query_row(
            r#"
            SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
            FROM threads
            WHERE (cwd = ?1 OR cwd LIKE ?2)
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
            params![repo_root, repo_prefix],
            map_thread_record,
        )
        .optional()?;
    Ok(record)
}

fn thread_record_by_id(thread_id: &str) -> Result<Option<ThreadRecord>> {
    let Some(db_path) = codex_db_path() else {
        return Ok(None);
    };
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    load_thread_record(&conn, thread_id)
}

fn load_thread_record(conn: &Connection, thread_id: &str) -> Result<Option<ThreadRecord>> {
    let record = conn
        .query_row(
            r#"
            SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
            FROM threads
            WHERE id = ?1
            "#,
            params![thread_id],
            map_thread_record,
        )
        .optional()
        .context("failed to read thread metadata from sqlite")?;
    Ok(record)
}

fn map_thread_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadRecord> {
    Ok(ThreadRecord {
        thread_id: row.get(0)?,
        title: row.get(1)?,
        cwd: row.get(2)?,
        first_user_message: row.get(3)?,
        rollout_path: row.get(4)?,
        created_at_epoch_s: row.get(5)?,
        updated_at_epoch_s: row.get(6)?,
    })
}

fn build_previous_chat_tail(
    thread_id: &str,
    title: &str,
    rollout_path: &str,
    count: usize,
) -> Result<ChatTail> {
    let summary = rollout_summary_from_path(Path::new(rollout_path), count)?;
    Ok(ChatTail {
        thread_id: thread_id.to_string(),
        title: sanitize_chat_title(title, &summary.tail_messages),
        summary_headline: summary.summary_headline,
        summary_next_step: summary.summary_next_step,
        messages: summary.tail_messages,
    })
}

fn build_chat_tail_at_time(
    record: &ThreadRecord,
    target_epoch_s: i64,
    count: usize,
) -> Result<ChatTail> {
    let summary =
        rollout_summary_from_path_at_time(Path::new(&record.rollout_path), target_epoch_s, count)?;
    Ok(ChatTail {
        thread_id: record.thread_id.clone(),
        title: sanitize_chat_title(&record.title, &summary.tail_messages),
        summary_headline: summary.summary_headline,
        summary_next_step: summary.summary_next_step,
        messages: summary.tail_messages,
    })
}

fn sanitize_chat_title(title: &str, messages: &[TranscriptMessage]) -> String {
    let collapsed_title = collapse_text(title, 160);
    let first_user_message = messages
        .iter()
        .find(|message| message.role == "user" && !message.text.trim().is_empty())
        .map(|message| collapse_text(&message.text, 160))
        .unwrap_or_default();
    let title_is_just_first_question =
        !first_user_message.is_empty() && collapsed_title == first_user_message;
    let title_needs_summary_fallback = looks_like_noisy_title(&collapsed_title)
        || title_is_just_first_question
        || looks_like_weak_question_title(&collapsed_title)
        || collapsed_title.chars().count() < 4;
    if !title_needs_summary_fallback {
        return collapsed_title;
    }
    if let Some(assistant_summary) = messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant" && !message.text.trim().is_empty())
        .and_then(|message| compact_headline_from_text(&message.text, 160))
        .filter(|value| !looks_like_noisy_title(value))
    {
        return assistant_summary;
    }
    let fallback = first_user_message;
    if fallback.is_empty() {
        collapsed_title
    } else {
        fallback
    }
}

fn looks_like_noisy_title(title: &str) -> bool {
    let normalized = title.to_lowercase();
    title.contains('\n')
        || title.len() > 120
        || normalized.starts_with("agents.md прочитан")
        || normalized.contains("agents.md прочитан")
        || normalized.starts_with("продолжай строго")
        || normalized.contains("продолжай строго")
        || normalized.starts_with("continue strictly")
        || normalized.starts_with("# context from my ide setup")
        || normalized.contains("## active file:")
        || normalized.contains("## open tabs:")
        || normalized.contains("перед любой содержательной работой")
        || normalized.contains("<instructions>")
}

fn looks_like_weak_question_title(title: &str) -> bool {
    let trimmed = title.trim();
    trimmed.ends_with('?') || trimmed.ends_with('؟')
}

fn compact_headline_from_text(text: &str, max_chars: usize) -> Option<String> {
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
    let collapsed = collapsed
        .trim_matches(['`', '"', '\'', '«', '»'])
        .trim()
        .to_string();
    if collapsed.is_empty() {
        return None;
    }
    for label in [
        "На чём закончился прошлый чат:",
        "На чём остановились:",
        "Продолжаем с этой линии:",
        "Текущий handoff в Amai:",
        "Последний зафиксированный handoff:",
        "активная линия тогда была",
        "активная линия была",
        "active line was",
        "headline:",
    ] {
        if let Some((_, rest)) = collapsed.split_once(label) {
            if let Some(value) = extract_backticked_value(rest) {
                return Some(truncate_compact_value(&value, max_chars));
            }
            if let Some(value) = compact_sentence(rest, max_chars) {
                return Some(value);
            }
        }
    }
    compact_sentence(&collapsed, max_chars)
}

fn extract_backticked_value(value: &str) -> Option<String> {
    let (_, rest) = value.split_once('`')?;
    let (candidate, _) = rest.split_once('`')?;
    let candidate = candidate.trim();
    (!candidate.is_empty()).then_some(candidate.to_string())
}

fn truncate_compact_value(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn compact_sentence(value: &str, max_chars: usize) -> Option<String> {
    let value = value
        .trim_start_matches(|ch: char| ch == ':' || ch == '-' || ch.is_whitespace())
        .trim_matches(['`', '"', '\'', '«', '»'])
        .trim();
    if value.is_empty() {
        return None;
    }
    let first_sentence = find_sentence_boundary(value)
        .map(|index| value[..=index].trim())
        .unwrap_or(value);
    Some(truncate_compact_value(first_sentence, max_chars))
}

fn find_sentence_boundary(value: &str) -> Option<usize> {
    for (index, ch) in value.char_indices() {
        if !matches!(ch, '.' | '!' | '?') {
            continue;
        }
        let mut tail = value[index + ch.len_utf8()..].chars().peekable();
        while let Some(next) = tail.peek() {
            if matches!(*next, '`' | '"' | '\'' | '«' | '»' | ')' | ']') {
                tail.next();
                continue;
            }
            break;
        }
        match tail.peek() {
            None => return Some(index),
            Some(next) if next.is_whitespace() => return Some(index),
            _ => {}
        }
    }
    None
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

fn compact_next_step_from_text(text: &str) -> Option<String> {
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

fn snapshot_messages(node: &Value, count: usize) -> Option<Vec<TranscriptMessage>> {
    let mut messages = Vec::new();
    if count >= 2
        && let Some(text) = node["last_user_message"]
            .as_str()
            .filter(|value| !value.is_empty())
    {
        messages.push(TranscriptMessage {
            role: "user".to_string(),
            text: text.to_string(),
        });
    }
    if let Some(text) = node["last_assistant_message"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        messages.push(TranscriptMessage {
            role: "assistant".to_string(),
            text: text.to_string(),
        });
    }
    (!messages.is_empty()).then_some(messages)
}

fn snapshot_rollout_messages(node: &Value, count: usize) -> Result<Option<Vec<TranscriptMessage>>> {
    let path = node["source_rollout"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            node["raw_rollout"]
                .as_str()
                .filter(|value| !value.is_empty())
        });
    let Some(path) = path else {
        return Ok(None);
    };
    let summary = rollout_summary_from_path(Path::new(path), count)?;
    if summary.tail_messages.is_empty() {
        Ok(None)
    } else {
        Ok(Some(summary.tail_messages))
    }
}

fn snapshot_rollout_messages_at_time(
    node: &Value,
    target_epoch_s: i64,
    count: usize,
) -> Result<Option<Vec<TranscriptMessage>>> {
    let path = node["source_rollout"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            node["raw_rollout"]
                .as_str()
                .filter(|value| !value.is_empty())
        });
    let Some(path) = path else {
        return Ok(None);
    };
    let summary = rollout_summary_from_path_at_time(Path::new(path), target_epoch_s, count)?;
    if summary.tail_messages.is_empty() {
        Ok(None)
    } else {
        Ok(Some(summary.tail_messages))
    }
}

fn rollout_summary_from_path(path: &Path, count: usize) -> Result<RolloutSummary> {
    if !path.exists() {
        return Ok(RolloutSummary {
            started_at: String::new(),
            ended_at: String::new(),
            messages_count: 0,
            last_user_message: String::new(),
            last_assistant_message: String::new(),
            summary_headline: None,
            summary_next_step: None,
            tail_messages: Vec::new(),
        });
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let messages = extract_chat_messages_from_rollout_text(&text)?;
    let started_at = messages
        .first()
        .map(|message| message.timestamp.clone())
        .unwrap_or_default();
    let ended_at = messages
        .last()
        .map(|message| message.timestamp.clone())
        .unwrap_or_default();
    let last_user_message = messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.text.clone())
        .unwrap_or_default();
    let last_assistant_message = messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .map(|message| message.text.clone())
        .unwrap_or_default();
    let summary_headline = compact_headline_from_text(&last_assistant_message, 220);
    let summary_next_step = compact_next_step_from_text(&last_assistant_message);
    Ok(RolloutSummary {
        started_at,
        ended_at,
        messages_count: messages.len(),
        last_user_message,
        last_assistant_message,
        summary_headline,
        summary_next_step,
        tail_messages: select_tail_messages(&messages, count),
    })
}

fn rollout_summary_from_path_at_time(
    path: &Path,
    target_epoch_s: i64,
    count: usize,
) -> Result<RolloutSummary> {
    if !path.exists() {
        return Ok(RolloutSummary {
            started_at: String::new(),
            ended_at: String::new(),
            messages_count: 0,
            last_user_message: String::new(),
            last_assistant_message: String::new(),
            summary_headline: None,
            summary_next_step: None,
            tail_messages: Vec::new(),
        });
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let messages = extract_chat_messages_from_rollout_text(&text)?;
    let started_at = messages
        .first()
        .map(|message| message.timestamp.clone())
        .unwrap_or_default();
    let ended_at = messages
        .last()
        .map(|message| message.timestamp.clone())
        .unwrap_or_default();
    let last_user_message = messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.text.clone())
        .unwrap_or_default();
    let last_assistant_message = messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .map(|message| message.text.clone())
        .unwrap_or_default();
    let selected_tail_messages = select_messages_for_time(&messages, target_epoch_s, count);
    let summary_headline = selected_tail_messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .and_then(|message| compact_headline_from_text(&message.text, 220))
        .or_else(|| compact_headline_from_text(&last_assistant_message, 220));
    let summary_next_step = selected_tail_messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .and_then(|message| compact_next_step_from_text(&message.text))
        .or_else(|| compact_next_step_from_text(&last_assistant_message));
    Ok(RolloutSummary {
        started_at,
        ended_at,
        messages_count: messages.len(),
        last_user_message,
        last_assistant_message,
        summary_headline,
        summary_next_step,
        tail_messages: selected_tail_messages,
    })
}

fn extract_chat_messages_from_rollout_text(text: &str) -> Result<Vec<RolloutMessage>> {
    let mut messages = Vec::new();
    for line in text.lines() {
        let row: Value =
            serde_json::from_str(line).context("failed to parse rollout jsonl line")?;
        if row["type"].as_str() != Some("response_item") {
            continue;
        }
        let payload = &row["payload"];
        if payload["type"].as_str() != Some("message") {
            continue;
        }
        let role = payload["role"]
            .as_str()
            .filter(|role| matches!(*role, "user" | "assistant"));
        let Some(role) = role else {
            continue;
        };
        let text = extract_message_text(payload);
        if text.is_empty() || is_synthetic_bootstrap_message(role, &text) {
            continue;
        }
        messages.push(RolloutMessage {
            timestamp: row["timestamp"].as_str().unwrap_or_default().to_string(),
            role: role.to_string(),
            phase: payload["phase"].as_str().map(ToOwned::to_owned),
            text,
        });
    }
    Ok(messages)
}

fn extract_message_text(payload: &Value) -> String {
    payload["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let item_type = item["type"].as_str()?;
            if !matches!(item_type, "input_text" | "output_text") {
                return None;
            }
            item["text"]
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_synthetic_bootstrap_message(role: &str, text: &str) -> bool {
    role == "user"
        && (text.starts_with(SYNTHETIC_AGENTS_PREFIX)
            || text.contains(SYNTHETIC_INSTRUCTIONS_MARKER))
}

fn select_tail_messages(messages: &[RolloutMessage], count: usize) -> Vec<TranscriptMessage> {
    if messages.is_empty() || count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return messages
            .last()
            .map(|message| TranscriptMessage {
                role: message.role.clone(),
                text: message.text.clone(),
            })
            .into_iter()
            .collect();
    }
    if count == 2
        && let Some(assistant_index) = messages.iter().rposition(|message| {
            message.role == "assistant"
                && matches!(
                    message.phase.as_deref(),
                    Some("final_answer") | Some("final") | None
                )
        })
    {
        let mut selected = Vec::new();
        if let Some(user_index) = messages[..assistant_index]
            .iter()
            .rposition(|message| message.role == "user")
        {
            selected.push(TranscriptMessage {
                role: "user".to_string(),
                text: messages[user_index].text.clone(),
            });
        }
        selected.push(TranscriptMessage {
            role: "assistant".to_string(),
            text: messages[assistant_index].text.clone(),
        });
        return selected;
    }
    messages
        .iter()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| TranscriptMessage {
            role: message.role.clone(),
            text: message.text.clone(),
        })
        .collect()
}

fn select_messages_for_time(
    messages: &[RolloutMessage],
    target_epoch_s: i64,
    count: usize,
) -> Vec<TranscriptMessage> {
    if messages.is_empty() || count == 0 {
        return Vec::new();
    }

    let last_completed_before = messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, message)| {
            if message.role != "assistant" {
                return None;
            }
            let timestamp = parse_rfc3339_epoch_s(&message.timestamp).ok()?;
            if timestamp > target_epoch_s {
                return None;
            }
            let user_index = messages[..index]
                .iter()
                .rposition(|candidate| candidate.role == "user")?;
            Some((user_index, index))
        });

    if let Some((user_index, assistant_index)) = last_completed_before {
        let mut selected = Vec::new();
        if count >= 2 {
            selected.push(TranscriptMessage {
                role: "user".to_string(),
                text: messages[user_index].text.clone(),
            });
        }
        selected.push(TranscriptMessage {
            role: "assistant".to_string(),
            text: messages[assistant_index].text.clone(),
        });
        return selected;
    }

    let user_before = messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, message)| {
            if message.role != "user" {
                return None;
            }
            let timestamp = parse_rfc3339_epoch_s(&message.timestamp).ok()?;
            (timestamp <= target_epoch_s).then_some(index)
        });
    if let Some(user_index) = user_before {
        let mut selected = vec![TranscriptMessage {
            role: "user".to_string(),
            text: messages[user_index].text.clone(),
        }];
        if count >= 2
            && let Some(assistant_index) = messages[user_index + 1..]
                .iter()
                .position(|candidate| candidate.role == "assistant")
                .map(|offset| user_index + 1 + offset)
        {
            selected.push(TranscriptMessage {
                role: "assistant".to_string(),
                text: messages[assistant_index].text.clone(),
            });
        }
        return selected;
    }

    let nearest_index = messages
        .iter()
        .enumerate()
        .min_by_key(|(_, message)| {
            parse_rfc3339_epoch_s(&message.timestamp)
                .map(|timestamp| (timestamp - target_epoch_s).abs())
                .unwrap_or(i64::MAX)
        })
        .map(|(index, _)| index)
        .unwrap_or(messages.len() - 1);
    let start = nearest_index.saturating_sub(count.saturating_sub(1));
    messages[start..=nearest_index]
        .iter()
        .map(|message| TranscriptMessage {
            role: message.role.clone(),
            text: message.text.clone(),
        })
        .collect()
}

fn thread_record_at_time(repo_root: &str, target_epoch_s: i64) -> Result<Option<ThreadRecord>> {
    let Some(db_path) = codex_db_path() else {
        return Ok(None);
    };
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    let candidate_records = thread_records_around_time(&conn, repo_root, target_epoch_s, 12)?;
    if !target_is_within_thread_bounds(&candidate_records, target_epoch_s) {
        return Ok(None);
    }
    let ranked = candidate_records
        .into_iter()
        .filter_map(|record| {
            let (started_at_epoch_s, ended_at_epoch_s) =
                rollout_window_epoch_s(Path::new(&record.rollout_path)).ok()??;
            let width = if ended_at_epoch_s >= started_at_epoch_s {
                ended_at_epoch_s - started_at_epoch_s
            } else {
                i64::MAX
            };
            let contains =
                started_at_epoch_s <= target_epoch_s && target_epoch_s <= ended_at_epoch_s;
            let before = ended_at_epoch_s <= target_epoch_s;
            let after = started_at_epoch_s >= target_epoch_s;
            let rank = if contains {
                (
                    0_i32,
                    width,
                    target_epoch_s - ended_at_epoch_s,
                    started_at_epoch_s,
                )
            } else if before {
                (
                    1_i32,
                    target_epoch_s - ended_at_epoch_s,
                    width,
                    started_at_epoch_s,
                )
            } else if after {
                (
                    2_i32,
                    started_at_epoch_s - target_epoch_s,
                    width,
                    started_at_epoch_s,
                )
            } else {
                return None;
            };
            Some((rank, record))
        })
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, record)| record);
    Ok(ranked)
}

fn target_is_within_thread_bounds(records: &[ThreadRecord], target_epoch_s: i64) -> bool {
    let mut earliest_started_at = i64::MAX;
    let mut latest_ended_at = i64::MIN;
    let mut found_window = false;

    for record in records {
        let Ok(Some((started_at_epoch_s, ended_at_epoch_s))) =
            rollout_window_epoch_s(Path::new(&record.rollout_path))
        else {
            continue;
        };
        earliest_started_at = earliest_started_at.min(started_at_epoch_s);
        latest_ended_at = latest_ended_at.max(ended_at_epoch_s);
        found_window = true;
    }

    found_window && earliest_started_at <= target_epoch_s && target_epoch_s <= latest_ended_at
}

fn target_is_within_snapshot_bounds(
    snapshots: &[&ObservabilitySnapshotRecord],
    target_epoch_s: i64,
) -> bool {
    let mut earliest_started_at = i64::MAX;
    let mut latest_ended_at = i64::MIN;
    let mut found_window = false;

    for snapshot in snapshots {
        let node = &snapshot.payload["continuity_thread_index"];
        let (started_at_epoch_s, ended_at_epoch_s) = snapshot_window_epoch_s(node);
        if started_at_epoch_s <= 0 || ended_at_epoch_s <= 0 {
            continue;
        }
        earliest_started_at = earliest_started_at.min(started_at_epoch_s);
        latest_ended_at = latest_ended_at.max(ended_at_epoch_s);
        found_window = true;
    }

    found_window && earliest_started_at <= target_epoch_s && target_epoch_s <= latest_ended_at
}

fn thread_records_around_time(
    conn: &Connection,
    repo_root: &str,
    target_epoch_s: i64,
    limit: usize,
) -> Result<Vec<ThreadRecord>> {
    let repo_prefix = format!("{repo_root}/%");
    let mut records = Vec::new();

    let mut containing = conn.prepare(
        r#"
        SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
        FROM threads
        WHERE (cwd = ?1 OR cwd LIKE ?2)
          AND created_at <= ?3
          AND updated_at >= ?3
        ORDER BY updated_at DESC, id DESC
        LIMIT ?4
        "#,
    )?;
    let containing_rows = containing.query_map(
        params![repo_root, repo_prefix, target_epoch_s, limit as i64],
        map_thread_record,
    )?;
    for row in containing_rows {
        let record = row?;
        if !records
            .iter()
            .any(|candidate: &ThreadRecord| candidate.thread_id == record.thread_id)
        {
            records.push(record);
        }
    }

    let side_limit = (limit / 2).max(2);
    let mut previous = conn.prepare(
        r#"
        SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
        FROM threads
        WHERE (cwd = ?1 OR cwd LIKE ?2)
          AND updated_at <= ?3
        ORDER BY updated_at DESC, id DESC
        LIMIT ?4
        "#,
    )?;
    let previous_rows = previous.query_map(
        params![repo_root, repo_prefix, target_epoch_s, side_limit as i64],
        map_thread_record,
    )?;
    for row in previous_rows {
        let record = row?;
        if !records
            .iter()
            .any(|candidate: &ThreadRecord| candidate.thread_id == record.thread_id)
        {
            records.push(record);
        }
    }

    let mut next = conn.prepare(
        r#"
        SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
        FROM threads
        WHERE (cwd = ?1 OR cwd LIKE ?2)
          AND created_at >= ?3
        ORDER BY created_at ASC, id ASC
        LIMIT ?4
        "#,
    )?;
    let next_rows = next.query_map(
        params![repo_root, repo_prefix, target_epoch_s, side_limit as i64],
        map_thread_record,
    )?;
    for row in next_rows {
        let record = row?;
        if !records
            .iter()
            .any(|candidate: &ThreadRecord| candidate.thread_id == record.thread_id)
        {
            records.push(record);
        }
    }

    Ok(records)
}

fn parse_rfc3339_epoch_s(value: &str) -> Result<i64> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .with_context(|| format!("failed to parse RFC3339 time: {value}"))?;
    Ok(parsed.unix_timestamp())
}

fn rollout_window_epoch_s(path: &Path) -> Result<Option<(i64, i64)>> {
    if !path.exists() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let messages = extract_chat_messages_from_rollout_text(&text)?;
    let started_at = messages
        .first()
        .map(|message| parse_rfc3339_epoch_s(&message.timestamp))
        .transpose()?;
    let ended_at = messages
        .last()
        .map(|message| parse_rfc3339_epoch_s(&message.timestamp))
        .transpose()?;
    match (started_at, ended_at) {
        (Some(started_at), Some(ended_at)) => Ok(Some((started_at, ended_at))),
        _ => Ok(None),
    }
}

fn snapshot_window_epoch_s(node: &Value) -> (i64, i64) {
    let started_at = node["started_at"]
        .as_str()
        .filter(|value| !value.is_empty())
        .and_then(|value| parse_rfc3339_epoch_s(value).ok());
    let ended_at = node["ended_at"]
        .as_str()
        .filter(|value| !value.is_empty())
        .and_then(|value| parse_rfc3339_epoch_s(value).ok());
    (
        started_at.unwrap_or_else(|| node["created_at_epoch_s"].as_i64().unwrap_or_default()),
        ended_at.unwrap_or_else(|| node["updated_at_epoch_s"].as_i64().unwrap_or_default()),
    )
}

fn codex_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex").join("state_5.sqlite"))
}

fn thread_index_path() -> Result<PathBuf> {
    let memory_home = env::var("MEMORY_HOME")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(".memory")
        });
    Ok(memory_home
        .join("transcripts")
        .join("codex")
        .join("thread_index.json"))
}

fn extract_last_messages(rendered_path: &Path, count: usize) -> Result<Vec<TranscriptMessage>> {
    if !rendered_path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(rendered_path)
        .with_context(|| format!("failed to read {}", rendered_path.display()))?;
    let mut messages = Vec::new();
    let mut current_role = None::<String>;
    let mut current_lines = Vec::new();

    for line in text.lines() {
        if let Some((_, role)) = parse_role_heading(line) {
            flush_message(&mut messages, current_role.take(), &mut current_lines);
            current_role = Some(role);
            continue;
        }
        if line.starts_with("### ") {
            flush_message(&mut messages, current_role.take(), &mut current_lines);
            continue;
        }
        if current_role.is_some() {
            current_lines.push(line);
        }
    }
    flush_message(&mut messages, current_role.take(), &mut current_lines);

    if messages.len() > count {
        Ok(messages.split_off(messages.len() - count))
    } else {
        Ok(messages)
    }
}

fn flush_message(target: &mut Vec<TranscriptMessage>, role: Option<String>, lines: &mut Vec<&str>) {
    let Some(role) = role else {
        lines.clear();
        return;
    };
    let text = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    lines.clear();
    if text.is_empty() {
        return;
    }
    target.push(TranscriptMessage {
        role,
        text: collapse_text(&text, 280),
    });
}

#[derive(Debug, Clone)]
struct RenderedTranscriptMessage {
    timestamp: String,
    role: String,
    text: String,
}

fn parse_role_heading(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with("### ") {
        return None;
    }
    let mut parts = trimmed.split_whitespace().skip(1).collect::<Vec<_>>();
    let role = parts.pop()?;
    let timestamp = parts.join(" ");
    match role {
        "user" | "assistant" => Some((timestamp, role.to_string())),
        _ => None,
    }
}

fn extract_messages_from_rendered_text(text: &str) -> Vec<RenderedTranscriptMessage> {
    let mut messages = Vec::new();
    let mut current_role = None::<String>;
    let mut current_timestamp = None::<String>;
    let mut current_lines = Vec::new();

    for line in text.lines() {
        if let Some((timestamp, role)) = parse_role_heading(line) {
            flush_rendered_message(
                &mut messages,
                current_timestamp.take(),
                current_role.take(),
                &mut current_lines,
            );
            current_timestamp = Some(timestamp);
            current_role = Some(role);
            continue;
        }
        if line.starts_with("### ") {
            flush_rendered_message(
                &mut messages,
                current_timestamp.take(),
                current_role.take(),
                &mut current_lines,
            );
            continue;
        }
        if current_role.is_some() {
            current_lines.push(line);
        }
    }

    flush_rendered_message(
        &mut messages,
        current_timestamp.take(),
        current_role.take(),
        &mut current_lines,
    );
    messages
}

fn flush_rendered_message(
    target: &mut Vec<RenderedTranscriptMessage>,
    timestamp: Option<String>,
    role: Option<String>,
    lines: &mut Vec<&str>,
) {
    let Some(role) = role else {
        lines.clear();
        return;
    };
    let text = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    lines.clear();
    if text.is_empty() {
        return;
    }
    target.push(RenderedTranscriptMessage {
        timestamp: timestamp.unwrap_or_default(),
        role,
        text: collapse_text(&text, 280),
    });
}

fn extract_field(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix(prefix))
        .and_then(|rest| rest.strip_suffix('`'))
        .map(ToOwned::to_owned)
}

fn collapse_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        format!(
            "{}...",
            collapsed.chars().take(max_chars).collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        chat_tail_at_time_from_snapshots, collapse_text, compact_headline_from_text,
        compact_next_step_from_text, extract_chat_messages_from_rollout_text,
        extract_last_messages, nth_previous_chat_tail_from_snapshots, parse_rfc3339_epoch_s,
        parse_role_heading, rendered_transcript_summary, rollout_summary_from_path,
        select_messages_for_time, select_tail_messages,
    };
    use crate::postgres::ObservabilitySnapshotRecord;
    use serde_json::json;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn parse_role_heading_accepts_only_user_and_assistant() {
        assert_eq!(
            parse_role_heading("### 2026-03-21 user"),
            Some(("2026-03-21".to_string(), "user".to_string()))
        );
        assert_eq!(
            parse_role_heading("### 2026-03-21 assistant"),
            Some(("2026-03-21".to_string(), "assistant".to_string()))
        );
        assert_eq!(parse_role_heading("### 2026-03-21 tool_call"), None);
    }

    #[test]
    fn collapse_text_truncates_and_compacts() {
        let collapsed = collapse_text("one   two\nthree", 10);
        assert_eq!(collapsed, "one two th...");
    }

    #[test]
    fn extract_last_messages_reads_tail_of_rendered_transcript() {
        let transcript =
            std::env::temp_dir().join(format!("amai-thread-{}.md", std::process::id()));
        fs::write(
            &transcript,
            "# thread\n\n### 2026 user\n\nfirst question\n\n### 2026 assistant\n\nfirst answer\n\n### 2027 user\n\nsecond question\n\n### 2027 assistant\n\nsecond answer\n",
        )
        .expect("write transcript");

        let tail = extract_last_messages(&transcript, 2).expect("tail");
        let _ = fs::remove_file(&transcript);

        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].role, "user");
        assert!(tail[0].text.contains("second question"));
        assert_eq!(tail[1].role, "assistant");
        assert!(tail[1].text.contains("second answer"));
    }

    #[test]
    fn rendered_transcript_summary_extracts_tail_and_thread_id() {
        let content = "# test\n\n- `thread_id`: `thread-1`\n- `cwd`: `/home/art/Art`\n- `first_user_message`: `hello`\n\n## Transcript\n\n### 2026-03-21T12:00:00Z user\n\nfirst\n\n### 2026-03-21T12:01:00Z assistant\n\nsecond\n";
        let summary =
            rendered_transcript_summary(content, "/tmp/thread.md", None).expect("summary");
        assert_eq!(summary["thread_id"], json!("thread-1"));
        assert_eq!(summary["started_at"], json!("2026-03-21T12:00:00Z"));
        assert_eq!(summary["last_assistant_message"], json!("second"));
    }

    #[test]
    fn rollout_parser_skips_synthetic_agents_wrapper() {
        let rollout = r##"{"timestamp":"2026-03-21T12:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /home/art/Art\n<INSTRUCTIONS>"}]}}
{"timestamp":"2026-03-21T12:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"реальный вопрос"}]}}
{"timestamp":"2026-03-21T12:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"реальный ответ"}]}}
"##;
        let messages = extract_chat_messages_from_rollout_text(rollout).expect("messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, "реальный вопрос");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].text, "реальный ответ");
    }

    #[test]
    fn select_tail_messages_prefers_last_real_user_and_final_answer() {
        let rollout = r##"{"timestamp":"2026-03-21T12:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"вопрос"}]}}
{"timestamp":"2026-03-21T12:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"иду смотреть"}]}}
{"timestamp":"2026-03-21T12:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"готовый ответ"}]}}
"##;
        let messages = extract_chat_messages_from_rollout_text(rollout).expect("messages");
        let tail = select_tail_messages(&messages, 2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].role, "user");
        assert_eq!(tail[0].text, "вопрос");
        assert_eq!(tail[1].role, "assistant");
        assert_eq!(tail[1].text, "готовый ответ");
    }

    #[test]
    fn rollout_summary_uses_raw_rollout_messages() {
        let path = std::env::temp_dir().join(format!("amai-rollout-{}.jsonl", std::process::id()));
        fs::write(
            &path,
            r#"{"timestamp":"2026-03-21T12:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"вопрос"}]}}
{"timestamp":"2026-03-21T12:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"ответ"}]}}
"#,
        )
        .expect("write rollout");
        let summary = rollout_summary_from_path(&path, 2).expect("summary");
        let _ = fs::remove_file(&path);
        assert_eq!(summary.messages_count, 2);
        assert_eq!(summary.last_user_message, "вопрос");
        assert_eq!(summary.last_assistant_message, "ответ");
        assert_eq!(summary.tail_messages.len(), 2);
    }

    #[test]
    fn compact_next_step_strips_nested_labels_and_markdown_noise() {
        let text = "Ближайший обязательный следующий шаг: Следующий обязательный шаг: проверить новый чат ещё раз.`|";
        let next_step = compact_next_step_from_text(text).expect("next step");
        assert_eq!(next_step, "проверить новый чат ещё раз.");
    }

    #[test]
    fn compact_headline_prefers_backticked_active_line_value() {
        let text = "В предыдущем чате мы закончили на continuity-контуре: по `Amai` активная линия тогда была `Amai startup restore pack enriched and committed`.";
        let headline = compact_headline_from_text(text, 220).expect("headline");
        assert_eq!(headline, "Amai startup restore pack enriched and committed");
    }

    #[test]
    fn compact_headline_does_not_cut_on_filename_dot() {
        let text = "В этом `providers`-каталоге ещё есть более слабые фасады, чем образец: `auth.rs` и `process.rs`. Дотягиваю их до того же уровня формулировки.";
        let headline = compact_headline_from_text(text, 220).expect("headline");
        assert_eq!(
            headline,
            "В этом `providers`-каталоге ещё есть более слабые фасады, чем образец: `auth.rs` и `process.rs`."
        );
    }

    #[test]
    fn select_messages_for_time_prefers_completed_exchange_before_target() {
        let rollout = r##"{"timestamp":"2026-03-21T11:59:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"старый вопрос"}]}}
{"timestamp":"2026-03-21T11:59:10Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"старый ответ"}]}}
{"timestamp":"2026-03-21T12:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"новый вопрос"}]}}
{"timestamp":"2026-03-21T12:00:30Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"новый ответ"}]}}
"##;
        let messages = extract_chat_messages_from_rollout_text(rollout).expect("messages");
        let selected = select_messages_for_time(
            &messages,
            parse_rfc3339_epoch_s("2026-03-21T11:59:55Z").expect("time"),
            2,
        );
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].role, "user");
        assert_eq!(selected[0].text, "старый вопрос");
        assert_eq!(selected[1].role, "assistant");
        assert_eq!(selected[1].text, "старый ответ");
    }

    #[test]
    fn select_messages_for_time_returns_open_pair_around_target() {
        let rollout = r##"{"timestamp":"2026-03-21T12:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"вопрос"}]}}
{"timestamp":"2026-03-21T12:00:30Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"ответ"}]}}
"##;
        let messages = extract_chat_messages_from_rollout_text(rollout).expect("messages");
        let selected = select_messages_for_time(
            &messages,
            parse_rfc3339_epoch_s("2026-03-21T12:00:10Z").expect("time"),
            2,
        );
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].role, "user");
        assert_eq!(selected[0].text, "вопрос");
        assert_eq!(selected[1].role, "assistant");
        assert_eq!(selected[1].text, "ответ");
    }

    #[test]
    fn chat_tail_at_time_from_snapshots_works_with_thread_index_only() {
        let snapshots = vec![
            ObservabilitySnapshotRecord {
                snapshot_id: Uuid::nil(),
                snapshot_kind: "continuity_thread_index".to_string(),
                created_at_epoch_ms: 1_744_087_814_000,
                payload: json!({
                    "continuity_thread_index": {
                        "project": {"code": "art"},
                        "namespace": {"code": "continuity"},
                        "thread_id": "older-thread",
                        "title": "старый чат",
                        "created_at_epoch_s": 1_742_553_600,
                        "updated_at_epoch_s": 1_742_553_660,
                        "last_user_message": "старый вопрос",
                        "last_assistant_message": "старый ответ"
                    }
                }),
            },
            ObservabilitySnapshotRecord {
                snapshot_id: Uuid::new_v4(),
                snapshot_kind: "continuity_thread_index".to_string(),
                created_at_epoch_ms: 1_744_087_815_000,
                payload: json!({
                    "continuity_thread_index": {
                        "project": {"code": "art"},
                        "namespace": {"code": "continuity"},
                        "thread_id": "newer-thread",
                        "title": "новый чат",
                        "created_at_epoch_s": 1_742_554_000,
                        "updated_at_epoch_s": 1_742_554_060,
                        "last_user_message": "новый вопрос",
                        "last_assistant_message": "новый ответ"
                    }
                }),
            },
        ];

        let tail = chat_tail_at_time_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            "2025-03-21T10:41:00Z",
            2,
        )
        .expect("tail result")
        .expect("tail");

        assert_eq!(tail.thread_id, "older-thread");
        assert_eq!(tail.title, "старый чат");
        assert_eq!(tail.messages.len(), 2);
        assert_eq!(tail.messages[0].role, "user");
        assert_eq!(tail.messages[0].text, "старый вопрос");
        assert_eq!(tail.messages[1].role, "assistant");
        assert_eq!(tail.messages[1].text, "старый ответ");
    }

    #[test]
    fn chat_tail_at_time_from_snapshots_returns_none_for_future_time() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1_744_087_814_000,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "older-thread",
                    "title": "старый чат",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "старый вопрос",
                    "last_assistant_message": "старый ответ"
                }
            }),
        }];

        let tail = chat_tail_at_time_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            "2099-01-01T12:00:00Z",
            2,
        )
        .expect("tail result");

        assert!(tail.is_none());
    }

    #[test]
    fn chat_tail_at_time_from_snapshots_returns_none_before_first_chat() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1_744_087_814_000,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "older-thread",
                    "title": "старый чат",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "старый вопрос",
                    "last_assistant_message": "старый ответ"
                }
            }),
        }];

        let tail = chat_tail_at_time_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            "2020-01-01T12:00:00Z",
            2,
        )
        .expect("tail result");

        assert!(tail.is_none());
    }

    #[test]
    fn noisy_title_prefers_assistant_summary_over_raw_question_noise() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-1",
                    "title": "AGENTS.md прочитан.\nПродолжай строго из /home/art/Art",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "о чем мы говорили?",
                    "last_assistant_message": "про temporal lookup"
                }
            }),
        }];

        let tail = chat_tail_at_time_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            "2025-03-21T10:41:00Z",
            2,
        )
        .expect("tail result")
        .expect("tail");

        assert_eq!(tail.title, "про temporal lookup");
    }

    #[test]
    fn dotted_agents_title_is_still_treated_as_noise() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-dot",
                    "title": "AGENTS.md прочитан. Продолжай строго из `/home/art/Art`.",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "что делать дальше?",
                    "last_assistant_message": "Compact continuity label"
                }
            }),
        }];

        let tail = nth_previous_chat_tail_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            Some("current-thread"),
            1,
            1,
        )
        .expect("tail");

        assert_eq!(tail.title, "Compact continuity label");
    }

    #[test]
    fn question_like_title_prefers_assistant_summary_over_first_user_message() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-2",
                    "title": "на чем закончили в прошлом чате, какие последние два сообщения?",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "на чем закончили в прошлом чате, какие последние два сообщения?",
                    "last_assistant_message": "Amai startup restore pack enriched and committed"
                }
            }),
        }];

        let tail = nth_previous_chat_tail_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            Some("current-thread"),
            1,
            2,
        )
        .expect("tail");

        assert_eq!(
            tail.title,
            "Amai startup restore pack enriched and committed"
        );
    }

    #[test]
    fn short_or_question_only_title_prefers_assistant_summary() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-short",
                    "title": "работает?",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "работает?",
                    "last_assistant_message": "Контур exact-time lookup materialized"
                }
            }),
        }];

        let tail = nth_previous_chat_tail_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            Some("current-thread"),
            1,
            2,
        )
        .expect("tail");

        assert_eq!(tail.title, "Контур exact-time lookup materialized");
    }

    #[test]
    fn continue_strictly_style_title_is_treated_as_noise() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-strict",
                    "title": "Продолжай строго из `/home/art/Art` и строго по `/home/art/Art/AGENTS.md`.",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "что дальше?",
                    "last_assistant_message": "Human continuity label"
                }
            }),
        }];

        let tail = nth_previous_chat_tail_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            Some("current-thread"),
            1,
            1,
        )
        .expect("tail");

        assert_eq!(tail.title, "Human continuity label");
    }

    #[test]
    fn ide_setup_title_is_treated_as_noise() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-ide",
                    "title": "# Context from my IDE setup: ## Active file: core/src/lib.rs ## Open tabs: - lib.rs",
                    "created_at_epoch_s": 1_742_553_600,
                    "updated_at_epoch_s": 1_742_553_660,
                    "last_user_message": "что там было?",
                    "last_assistant_message": "Human exact-time summary"
                }
            }),
        }];

        let tail = nth_previous_chat_tail_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            Some("current-thread"),
            1,
            1,
        )
        .expect("tail");

        assert_eq!(tail.title, "Human exact-time summary");
    }

    #[test]
    fn nth_previous_chat_tail_from_snapshots_picks_second_previous_thread() {
        let snapshots = vec![
            ObservabilitySnapshotRecord {
                snapshot_id: Uuid::new_v4(),
                snapshot_kind: "continuity_thread_index".to_string(),
                created_at_epoch_ms: 30,
                payload: json!({
                    "continuity_thread_index": {
                        "project": {"code": "art"},
                        "namespace": {"code": "continuity"},
                        "thread_id": "thread-3",
                        "title": "current",
                        "updated_at_epoch_s": 30,
                        "last_user_message": "current user",
                        "last_assistant_message": "current assistant"
                    }
                }),
            },
            ObservabilitySnapshotRecord {
                snapshot_id: Uuid::new_v4(),
                snapshot_kind: "continuity_thread_index".to_string(),
                created_at_epoch_ms: 20,
                payload: json!({
                    "continuity_thread_index": {
                        "project": {"code": "art"},
                        "namespace": {"code": "continuity"},
                        "thread_id": "thread-2",
                        "title": "previous",
                        "updated_at_epoch_s": 20,
                        "last_user_message": "previous user",
                        "last_assistant_message": "previous assistant"
                    }
                }),
            },
            ObservabilitySnapshotRecord {
                snapshot_id: Uuid::new_v4(),
                snapshot_kind: "continuity_thread_index".to_string(),
                created_at_epoch_ms: 10,
                payload: json!({
                    "continuity_thread_index": {
                        "project": {"code": "art"},
                        "namespace": {"code": "continuity"},
                        "thread_id": "thread-1",
                        "title": "second previous",
                        "updated_at_epoch_s": 10,
                        "last_user_message": "second previous user",
                        "last_assistant_message": "second previous assistant"
                    }
                }),
            },
        ];

        let tail = nth_previous_chat_tail_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            Some("thread-3"),
            2,
            2,
        )
        .expect("tail");

        assert_eq!(tail.thread_id, "thread-1");
        assert_eq!(tail.messages[0].text, "second previous user");
        assert_eq!(tail.messages[1].text, "second previous assistant");
    }
}
