use crate::postgres::ObservabilitySnapshotRecord;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PreviousChatTail {
    pub thread_id: String,
    pub title: String,
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
    tail_messages: Vec<TranscriptMessage>,
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
}

const SYNTHETIC_AGENTS_PREFIX: &str = "# AGENTS.md instructions for ";
const SYNTHETIC_INSTRUCTIONS_MARKER: &str = "<INSTRUCTIONS>";

pub fn current_thread_id() -> Option<String> {
    env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn previous_chat_tail(repo_root: &str, count: usize) -> Result<Option<PreviousChatTail>> {
    if let Some(record) = previous_thread_record(repo_root, current_thread_id().as_deref())? {
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
    let Some(entry) = index.threads.into_iter().rev().find(|item| {
        item.cwd.starts_with(repo_root)
            && Some(item.thread_id.as_str()) != current_thread.as_deref()
    }) else {
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
    Ok(Some(PreviousChatTail {
        thread_id: entry.thread_id,
        title: entry.title,
        messages,
    }))
}

pub fn previous_chat_tail_from_snapshots(
    snapshots: &[ObservabilitySnapshotRecord],
    project_code: &str,
    namespace_code: &str,
    current_thread_id: Option<&str>,
    count: usize,
) -> Option<PreviousChatTail> {
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
        .max_by_key(|snapshot| {
            (
                snapshot.payload["continuity_thread_index"]["updated_at_epoch_s"]
                    .as_i64()
                    .unwrap_or_default(),
                snapshot.created_at_epoch_ms,
            )
        })?;
    let node = &snapshot.payload["continuity_thread_index"];
    let messages = snapshot_messages(node, count)
        .or_else(|| snapshot_rollout_messages(node, count).ok().flatten())
        .unwrap_or_default();
    Some(PreviousChatTail {
        thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
        title: node["title"].as_str().unwrap_or_default().to_string(),
        messages,
    })
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
        "rendered_transcript": transcript_path,
        "source_rollout": source_rollout,
        "created_at_epoch_s": record.as_ref().map(|item| item.created_at_epoch_s).unwrap_or_default(),
        "updated_at_epoch_s": record.as_ref().map(|item| item.updated_at_epoch_s).unwrap_or_default(),
    }))
}

fn previous_thread_record(
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
        LIMIT 1
        "#
    } else {
        r#"
        SELECT id, title, cwd, first_user_message, rollout_path, created_at, updated_at
        FROM threads
        WHERE (cwd = ?1 OR cwd LIKE ?2)
          AND (?3 IS NULL OR id != ?3)
        ORDER BY updated_at DESC, id DESC
        LIMIT 1
        "#
    };

    let record = if let Some(current) = current {
        conn.query_row(
            query,
            params![
                repo_root,
                repo_prefix,
                current.updated_at_epoch_s,
                current.thread_id
            ],
            map_thread_record,
        )
        .optional()?
    } else {
        conn.query_row(
            query,
            params![repo_root, repo_prefix, current_thread_id],
            map_thread_record,
        )
        .optional()?
    };
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
) -> Result<PreviousChatTail> {
    let summary = rollout_summary_from_path(Path::new(rollout_path), count)?;
    Ok(PreviousChatTail {
        thread_id: thread_id.to_string(),
        title: title.to_string(),
        messages: summary.tail_messages,
    })
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

fn rollout_summary_from_path(path: &Path, count: usize) -> Result<RolloutSummary> {
    if !path.exists() {
        return Ok(RolloutSummary {
            started_at: String::new(),
            ended_at: String::new(),
            messages_count: 0,
            last_user_message: String::new(),
            last_assistant_message: String::new(),
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
    Ok(RolloutSummary {
        started_at,
        ended_at,
        messages_count: messages.len(),
        last_user_message,
        last_assistant_message,
        tail_messages: select_tail_messages(&messages, count),
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
            text: collapse_text(&text, 280),
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
        collapse_text, extract_chat_messages_from_rollout_text, extract_last_messages,
        parse_role_heading, rendered_transcript_summary, rollout_summary_from_path,
        select_tail_messages,
    };
    use serde_json::json;
    use std::fs;

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
}
