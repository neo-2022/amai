use crate::postgres::ObservabilitySnapshotRecord;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
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
    pub selected_time_slice: Option<ThreadTimeSliceSummary>,
    pub messages: Vec<TranscriptMessage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadTimeSliceSummary {
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub started_at_epoch_s: i64,
    #[serde(default)]
    pub ended_at_epoch_s: i64,
    #[serde(default)]
    pub user_anchor: String,
    #[serde(default)]
    pub assistant_anchor: String,
    #[serde(default)]
    pub summary_headline: String,
    #[serde(default)]
    pub summary_next_step: String,
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
    time_slices: Vec<ThreadTimeSliceSummary>,
    selected_time_slice: Option<ThreadTimeSliceSummary>,
    tail_messages: Vec<TranscriptMessage>,
}

#[derive(Debug, Clone, Default)]
struct RolloutTurnObservation {
    turn_id: String,
    context_pack_ids: BTreeSet<String>,
    assistant_generation_tokens: u64,
    token_count_events: usize,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    approved_context_pack_calls: usize,
}

#[derive(Debug, Clone)]
struct CachedRolloutTurnObservations {
    file_signature: String,
    turns: Vec<RolloutTurnObservation>,
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
    #[serde(default)]
    pub time_slices: Vec<ThreadTimeSliceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolloutAssistantGenerationObservation {
    pub thread_id: String,
    pub rollout_path: String,
    pub turn_id: String,
    pub context_pack_id: String,
    pub assistant_generation_tokens: u64,
    pub token_count_events: usize,
    pub observation_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolloutAssistantGenerationTurnObservation {
    pub thread_id: String,
    pub rollout_path: String,
    pub turn_id: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: i64,
    pub assistant_generation_tokens: u64,
    pub token_count_events: usize,
    pub approved_context_pack_calls: usize,
    pub observation_source: String,
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
    #[serde(default)]
    time_slices: Vec<ThreadTimeSliceSummary>,
}

const SYNTHETIC_AGENTS_PREFIX: &str = "# AGENTS.md instructions for ";
const SYNTHETIC_INSTRUCTIONS_MARKER: &str = "<INSTRUCTIONS>";
const EXACT_TIME_MAX_SLICE_DRIFT_S: i64 = 3 * 60 * 60;
static ROLLOUT_TURN_OBSERVATION_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, CachedRolloutTurnObservations>>,
> = OnceLock::new();

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
        time_slices: summary.time_slices,
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
    let selected_time_slice = entry.time_slices.last().cloned();
    Ok(Some(ChatTail {
        thread_id: entry.thread_id,
        title: sanitize_chat_title(&entry.title, &messages),
        summary_headline: selected_time_slice
            .as_ref()
            .map(|slice| slice.summary_headline.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if entry.summary_headline.is_empty() {
                    messages
                        .iter()
                        .rev()
                        .find(|message| message.role == "assistant")
                        .and_then(|message| compact_headline_from_text(&message.text, 220))
                } else {
                    Some(entry.summary_headline)
                }
            }),
        summary_next_step: selected_time_slice
            .as_ref()
            .map(|slice| slice.summary_next_step.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if entry.summary_next_step.is_empty() {
                    messages
                        .iter()
                        .rev()
                        .find(|message| message.role == "assistant")
                        .and_then(|message| compact_next_step_from_text(&message.text))
                } else {
                    Some(entry.summary_next_step)
                }
            }),
        selected_time_slice,
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
    let selected_time_slice = entry.time_slices.last().cloned();
    Ok(Some(ChatTail {
        thread_id: entry.thread_id,
        title: sanitize_chat_title(&entry.title, &messages),
        summary_headline: selected_time_slice
            .as_ref()
            .map(|slice| slice.summary_headline.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if entry.summary_headline.is_empty() {
                    messages
                        .iter()
                        .rev()
                        .find(|message| message.role == "assistant")
                        .and_then(|message| compact_headline_from_text(&message.text, 220))
                } else {
                    Some(entry.summary_headline)
                }
            }),
        summary_next_step: selected_time_slice
            .as_ref()
            .map(|slice| slice.summary_next_step.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if entry.summary_next_step.is_empty() {
                    messages
                        .iter()
                        .rev()
                        .find(|message| message.role == "assistant")
                        .and_then(|message| compact_next_step_from_text(&message.text))
                } else {
                    Some(entry.summary_next_step)
                }
            }),
        selected_time_slice,
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
        summary_headline: last_snapshot_time_slice(node)
            .as_ref()
            .map(|slice| slice.summary_headline.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                node["summary_headline"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            }),
        summary_next_step: last_snapshot_time_slice(node)
            .as_ref()
            .map(|slice| slice.summary_next_step.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                node["summary_next_step"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            }),
        selected_time_slice: last_snapshot_time_slice(node),
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
        summary_headline: last_snapshot_time_slice(node)
            .as_ref()
            .map(|slice| slice.summary_headline.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                node["summary_headline"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            }),
        summary_next_step: last_snapshot_time_slice(node)
            .as_ref()
            .map(|slice| slice.summary_next_step.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                node["summary_next_step"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            }),
        selected_time_slice: last_snapshot_time_slice(node),
        messages,
    })
}

fn last_snapshot_time_slice(node: &Value) -> Option<ThreadTimeSliceSummary> {
    let slices: Vec<ThreadTimeSliceSummary> =
        serde_json::from_value(node["time_slices"].clone()).unwrap_or_default();
    slices.last().cloned()
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
    build_chat_tail_at_time(&record, target_epoch_s, count)
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
            let selected_time_slice = snapshot_selected_time_slice(node, target_epoch_s);
            let (started_at_epoch_s, ended_at_epoch_s) = selected_time_slice
                .as_ref()
                .map(|slice| (slice.started_at_epoch_s, slice.ended_at_epoch_s))
                .unwrap_or_else(|| snapshot_window_epoch_s(node));
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
            let distance = if contains {
                0
            } else if before {
                target_epoch_s - ended_at_epoch_s
            } else if after {
                started_at_epoch_s - target_epoch_s
            } else {
                i64::MAX
            };
            if selected_time_slice.is_some() && !contains && distance > EXACT_TIME_MAX_SLICE_DRIFT_S
            {
                return None;
            }
            let rank = if contains {
                (
                    0_i32,
                    width,
                    target_epoch_s - ended_at_epoch_s,
                    started_at_epoch_s,
                )
            } else if before {
                (1_i32, distance, width, started_at_epoch_s)
            } else if after {
                (2_i32, distance, width, started_at_epoch_s)
            } else {
                return None;
            };
            Some((rank, selected_time_slice, snapshot))
        })
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, selected_time_slice, snapshot)| (selected_time_slice, snapshot));
    let Some((preselected_time_slice, snapshot)) = snapshot else {
        return Ok(None);
    };
    let node = &snapshot.payload["continuity_thread_index"];
    if let Some(messages) = snapshot_rollout_messages_at_time(node, target_epoch_s, count)? {
        let selected_time_slice =
            preselected_time_slice.or_else(|| snapshot_selected_time_slice(node, target_epoch_s));
        return Ok(Some(ChatTail {
            thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
            title: sanitize_chat_title(node["title"].as_str().unwrap_or_default(), &messages),
            summary_headline: selected_time_slice
                .as_ref()
                .map(|slice| slice.summary_headline.clone())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    node["summary_headline"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                }),
            summary_next_step: selected_time_slice
                .as_ref()
                .map(|slice| slice.summary_next_step.clone())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    node["summary_next_step"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                }),
            selected_time_slice,
            messages,
        }));
    }
    let selected_time_slice =
        preselected_time_slice.or_else(|| snapshot_selected_time_slice(node, target_epoch_s));
    let Some(selected_time_slice) = selected_time_slice else {
        return Ok(None);
    };
    let messages = time_slice_messages(&selected_time_slice, count);
    Ok(Some(ChatTail {
        thread_id: node["thread_id"].as_str().unwrap_or_default().to_string(),
        title: sanitize_chat_title(node["title"].as_str().unwrap_or_default(), &messages),
        summary_headline: Some(selected_time_slice.summary_headline.clone())
            .filter(|value| !value.is_empty()),
        summary_next_step: Some(selected_time_slice.summary_next_step.clone())
            .filter(|value| !value.is_empty()),
        selected_time_slice: Some(selected_time_slice),
        messages,
    }))
}

fn snapshot_selected_time_slice(
    node: &Value,
    target_epoch_s: i64,
) -> Option<ThreadTimeSliceSummary> {
    let slices: Vec<ThreadTimeSliceSummary> =
        serde_json::from_value(node["time_slices"].clone()).unwrap_or_default();
    select_time_slice_for_epoch(&slices, target_epoch_s)
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
        "time_slices": rollout_summary
            .as_ref()
            .map(|summary| serde_json::to_value(&summary.time_slices).unwrap_or_else(|_| json!([])))
            .unwrap_or_else(|| json!([])),
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
        time_slices: serde_json::from_value(value["time_slices"].clone()).unwrap_or_default(),
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

pub fn latest_rollout_assistant_generation_observation(
    repo_root: &str,
    explicit_rollout_path: Option<&Path>,
) -> Result<Option<RolloutAssistantGenerationObservation>> {
    Ok(
        rollout_assistant_generation_observations(repo_root, explicit_rollout_path)?
            .into_iter()
            .last(),
    )
}

pub fn rollout_assistant_generation_observations(
    repo_root: &str,
    explicit_rollout_path: Option<&Path>,
) -> Result<Vec<RolloutAssistantGenerationObservation>> {
    let (thread_id, rollout_path) = if let Some(path) = explicit_rollout_path {
        let path = path.to_path_buf();
        let thread_id = rollout_thread_id_from_path(&path).unwrap_or_default();
        (thread_id, path)
    } else {
        let Some(record) = current_thread_record(repo_root, current_thread_id().as_deref())? else {
            return Ok(Vec::new());
        };
        if record.rollout_path.is_empty() {
            return Ok(Vec::new());
        }
        (record.thread_id, PathBuf::from(record.rollout_path))
    };
    if !rollout_path.exists() {
        return Ok(Vec::new());
    }
    parse_rollout_assistant_generation_observations(&thread_id, &rollout_path)
}

pub fn current_rollout_source_signature(repo_root: &str) -> Result<Option<String>> {
    let Some(record) = current_thread_record(repo_root, current_thread_id().as_deref())? else {
        return Ok(None);
    };
    if record.rollout_path.is_empty() {
        return Ok(None);
    }
    Ok(Some(rollout_source_signature(
        &record.thread_id,
        &PathBuf::from(record.rollout_path),
    )))
}

pub fn rollout_assistant_generation_turn_observations_for_thread(
    thread_id: &str,
) -> Result<Vec<RolloutAssistantGenerationTurnObservation>> {
    let Some(record) = thread_record_by_id(thread_id)? else {
        return Ok(Vec::new());
    };
    if record.rollout_path.is_empty() {
        return Ok(Vec::new());
    }
    let rollout_path = PathBuf::from(record.rollout_path);
    if !rollout_path.exists() {
        return Ok(Vec::new());
    }
    parse_rollout_assistant_generation_turn_observations(thread_id, &rollout_path)
}

pub fn rollout_source_signature_for_thread(thread_id: &str) -> Result<Option<String>> {
    let Some(record) = thread_record_by_id(thread_id)? else {
        return Ok(None);
    };
    if record.rollout_path.is_empty() {
        return Ok(None);
    }
    Ok(Some(rollout_source_signature(
        thread_id,
        &PathBuf::from(record.rollout_path),
    )))
}

fn rollout_thread_id_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let candidate = stem.chars().rev().take(36).collect::<String>();
    if candidate.len() != 36 {
        return None;
    }
    let thread_id = candidate.chars().rev().collect::<String>();
    let hyphen_count = thread_id.chars().filter(|ch| *ch == '-').count();
    (hyphen_count == 4).then_some(thread_id)
}

fn rollout_source_signature(thread_id: &str, rollout_path: &Path) -> String {
    format!("{thread_id}:{}", rollout_file_signature(rollout_path))
}

fn rollout_file_signature(rollout_path: &Path) -> String {
    let canonical_path = canonical_rollout_path(rollout_path);
    let path_label = canonical_path.display().to_string();
    let metadata = fs::metadata(&canonical_path).ok();
    let size_bytes = metadata.as_ref().map(|item| item.len()).unwrap_or_default();
    let modified_epoch_ms = metadata
        .and_then(|item| item.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    format!("{path_label}:{size_bytes}:{modified_epoch_ms}")
}

fn canonical_rollout_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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
        selected_time_slice: None,
        messages: summary.tail_messages,
    })
}

fn build_chat_tail_at_time(
    record: &ThreadRecord,
    target_epoch_s: i64,
    count: usize,
) -> Result<Option<ChatTail>> {
    let summary =
        rollout_summary_from_path_at_time(Path::new(&record.rollout_path), target_epoch_s, count)?;
    if let Some(slice) = summary.selected_time_slice.as_ref()
        && !time_slice_matches_exact_time(slice, target_epoch_s)
    {
        return Ok(None);
    }
    Ok(Some(ChatTail {
        thread_id: record.thread_id.clone(),
        title: sanitize_chat_title(&record.title, &summary.tail_messages),
        summary_headline: summary.summary_headline,
        summary_next_step: summary.summary_next_step,
        selected_time_slice: summary.selected_time_slice,
        messages: summary.tail_messages,
    }))
}

fn sanitize_chat_title(title: &str, messages: &[TranscriptMessage]) -> String {
    let collapsed_title = collapse_text(title, 160);
    let first_user_message = messages
        .iter()
        .find(|message| message.role == "user" && !message.text.trim().is_empty())
        .map(|message| primary_user_anchor(&message.text, 160))
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
    let mut remaining = value;
    for _ in 0..3 {
        let sentence = find_sentence_boundary(remaining)
            .map(|index| remaining[..=index].trim())
            .unwrap_or(remaining);
        if !looks_like_operational_prefix_sentence(sentence) {
            return Some(truncate_compact_value(sentence, max_chars));
        }
        let trimmed = remaining.strip_prefix(sentence).unwrap_or("").trim_start();
        if trimmed.is_empty() {
            return Some(truncate_compact_value(sentence, max_chars));
        }
        remaining = trimmed;
    }
    Some(truncate_compact_value(remaining, max_chars))
}

fn looks_like_operational_prefix_sentence(sentence: &str) -> bool {
    let normalized = sentence.trim().to_lowercase();
    normalized.starts_with("workspace совпадает:")
        || normalized.starts_with("workspace matches:")
        || normalized.starts_with("корень проекта совпадает:")
        || normalized.starts_with("корень подтверждён:")
        || normalized.starts_with("корень совпадает:")
}

fn primary_user_anchor(text: &str, max_chars: usize) -> String {
    let mut cleaned = Vec::new();
    let mut skipping_open_tabs = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("# Context from my IDE setup")
            || trimmed.starts_with("## Active file:")
            || trimmed.starts_with("## Active selection of the file:")
            || trimmed.starts_with("## Open tabs:")
            || trimmed.starts_with("## Open tabs")
            || trimmed.starts_with("## My request for Codex:")
        {
            skipping_open_tabs = trimmed.starts_with("## Open tabs");
            continue;
        }
        if skipping_open_tabs {
            if trimmed.starts_with("- ") {
                continue;
            }
            skipping_open_tabs = false;
        }
        cleaned.push(trimmed);
    }
    let candidate = cleaned.join(" ");
    if candidate.trim().is_empty() {
        collapse_text(text, max_chars)
    } else {
        collapse_text(&candidate, max_chars)
    }
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
            time_slices: Vec::new(),
            selected_time_slice: None,
            tail_messages: Vec::new(),
        });
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let messages = extract_chat_messages_from_rollout_text(&text)?;
    let time_slices = build_time_slices(&messages, 32);
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
        time_slices,
        selected_time_slice: None,
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
            time_slices: Vec::new(),
            selected_time_slice: None,
            tail_messages: Vec::new(),
        });
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let messages = extract_chat_messages_from_rollout_text(&text)?;
    let time_slices = build_time_slices(&messages, 32);
    let selected_time_slice = select_time_slice_for_epoch(&time_slices, target_epoch_s);
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
    let summary_headline = selected_time_slice
        .as_ref()
        .map(|slice| slice.summary_headline.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            selected_tail_messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .and_then(|message| compact_headline_from_text(&message.text, 220))
        })
        .or_else(|| compact_headline_from_text(&last_assistant_message, 220));
    let summary_next_step = selected_time_slice
        .as_ref()
        .map(|slice| slice.summary_next_step.clone())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            selected_tail_messages
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
                .and_then(|message| compact_next_step_from_text(&message.text))
        })
        .or_else(|| compact_next_step_from_text(&last_assistant_message));
    Ok(RolloutSummary {
        started_at,
        ended_at,
        messages_count: messages.len(),
        last_user_message,
        last_assistant_message,
        summary_headline,
        summary_next_step,
        time_slices,
        selected_time_slice: selected_time_slice.clone(),
        tail_messages: if selected_tail_messages.is_empty() {
            selected_time_slice
                .as_ref()
                .map(|slice| time_slice_messages(slice, count))
                .unwrap_or_default()
        } else {
            selected_tail_messages
        },
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

fn parse_rollout_assistant_generation_observations(
    thread_id: &str,
    rollout_path: &Path,
) -> Result<Vec<RolloutAssistantGenerationObservation>> {
    let turns = load_rollout_turn_observations(rollout_path)?;
    Ok(turns
        .into_iter()
        .filter(|turn| turn.assistant_generation_tokens > 0 && turn.context_pack_ids.len() == 1)
        .filter_map(|turn| {
            let context_pack_id = turn.context_pack_ids.iter().next()?.to_string();
            if context_pack_id.is_empty() || turn.turn_id.is_empty() {
                return None;
            }
            Some(RolloutAssistantGenerationObservation {
                thread_id: thread_id.to_string(),
                rollout_path: rollout_path.display().to_string(),
                turn_id: turn.turn_id,
                context_pack_id,
                assistant_generation_tokens: turn.assistant_generation_tokens,
                token_count_events: turn.token_count_events,
                observation_source: "codex_rollout_last_token_usage_sum_v1".to_string(),
            })
        })
        .collect())
}

fn parse_rollout_assistant_generation_turn_observations(
    thread_id: &str,
    rollout_path: &Path,
) -> Result<Vec<RolloutAssistantGenerationTurnObservation>> {
    Ok(load_rollout_turn_observations(rollout_path)?
        .into_iter()
        .filter(|turn| turn.assistant_generation_tokens > 0 && turn.approved_context_pack_calls > 0)
        .map(|turn| RolloutAssistantGenerationTurnObservation {
            thread_id: thread_id.to_string(),
            rollout_path: rollout_path.display().to_string(),
            turn_id: turn.turn_id,
            started_at_epoch_ms: turn.started_at_epoch_ms,
            ended_at_epoch_ms: turn.ended_at_epoch_ms,
            assistant_generation_tokens: turn.assistant_generation_tokens,
            token_count_events: turn.token_count_events,
            approved_context_pack_calls: turn.approved_context_pack_calls,
            observation_source: "codex_rollout_turn_timeline_v1".to_string(),
        })
        .collect())
}

fn load_rollout_turn_observations(rollout_path: &Path) -> Result<Vec<RolloutTurnObservation>> {
    let file_signature = rollout_file_signature(rollout_path);
    if let Some(turns) = cached_rollout_turn_observations(rollout_path, &file_signature) {
        return Ok(turns);
    }
    let text = fs::read_to_string(rollout_path)
        .with_context(|| format!("failed to read {}", rollout_path.display()))?;
    let turns = collect_rollout_turn_observations(&text)?;
    store_cached_rollout_turn_observations(rollout_path, &file_signature, &turns);
    Ok(turns)
}

fn cached_rollout_turn_observations(
    rollout_path: &Path,
    file_signature: &str,
) -> Option<Vec<RolloutTurnObservation>> {
    let cache = ROLLOUT_TURN_OBSERVATION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = cache.lock().ok()?;
    let key = canonical_rollout_path(rollout_path);
    let entry = guard.get(&key)?;
    if entry.file_signature == file_signature {
        Some(entry.turns.clone())
    } else {
        None
    }
}

fn store_cached_rollout_turn_observations(
    rollout_path: &Path,
    file_signature: &str,
    turns: &[RolloutTurnObservation],
) {
    let cache = ROLLOUT_TURN_OBSERVATION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    guard.insert(
        canonical_rollout_path(rollout_path),
        CachedRolloutTurnObservations {
            file_signature: file_signature.to_string(),
            turns: turns.to_vec(),
        },
    );
}

fn collect_rollout_turn_observations(text: &str) -> Result<Vec<RolloutTurnObservation>> {
    let mut observations = Vec::new();
    let mut current = None::<RolloutTurnObservation>;
    let mut approved_context_pack_calls = std::collections::HashMap::<String, bool>::new();
    for line in text.lines() {
        let row: Value =
            serde_json::from_str(line).context("failed to parse rollout jsonl line")?;
        let row_type = row["type"].as_str().unwrap_or_default();
        let payload = &row["payload"];
        let timestamp_epoch_ms = row["timestamp"]
            .as_str()
            .and_then(|value| parse_rfc3339_epoch_s(value).ok())
            .map(|value| value.saturating_mul(1000))
            .unwrap_or_default();
        match (row_type, payload["type"].as_str().unwrap_or_default()) {
            ("event_msg", "task_started") => {
                if let Some(turn) = current.take() {
                    observations.push(turn);
                }
                current = Some(RolloutTurnObservation {
                    turn_id: payload["turn_id"].as_str().unwrap_or_default().to_string(),
                    started_at_epoch_ms: timestamp_epoch_ms,
                    ended_at_epoch_ms: timestamp_epoch_ms,
                    ..RolloutTurnObservation::default()
                });
            }
            ("event_msg", "task_complete") => {
                if let Some(mut turn) = current.take() {
                    turn.ended_at_epoch_ms = timestamp_epoch_ms;
                    observations.push(turn);
                }
            }
            ("event_msg", "token_count") => {
                if let Some(turn) = current.as_mut() {
                    turn.ended_at_epoch_ms = timestamp_epoch_ms;
                    let output_tokens = payload["info"]["last_token_usage"]["output_tokens"]
                        .as_u64()
                        .unwrap_or_default();
                    if output_tokens > 0 {
                        turn.assistant_generation_tokens = turn
                            .assistant_generation_tokens
                            .saturating_add(output_tokens);
                        turn.token_count_events += 1;
                    }
                }
            }
            ("response_item", "function_call") => {
                let call_id = payload["call_id"].as_str().unwrap_or_default();
                if !call_id.is_empty() {
                    approved_context_pack_calls.insert(
                        call_id.to_string(),
                        rollout_function_call_is_context_pack(
                            payload["name"].as_str().unwrap_or_default(),
                            payload["arguments"].as_str().unwrap_or_default(),
                        ),
                    );
                }
                if let Some(turn) = current.as_mut() {
                    turn.ended_at_epoch_ms = timestamp_epoch_ms;
                    if !call_id.is_empty()
                        && approved_context_pack_calls
                            .get(call_id)
                            .copied()
                            .unwrap_or(false)
                    {
                        turn.approved_context_pack_calls += 1;
                    }
                }
            }
            ("response_item", "function_call_output") => {
                if let Some(turn) = current.as_mut() {
                    turn.ended_at_epoch_ms = timestamp_epoch_ms;
                    let call_id = payload["call_id"].as_str().unwrap_or_default();
                    if call_id.is_empty()
                        || !approved_context_pack_calls
                            .get(call_id)
                            .copied()
                            .unwrap_or(false)
                    {
                        continue;
                    }
                    collect_context_pack_ids_from_rollout_output(
                        &payload["output"],
                        &mut turn.context_pack_ids,
                    );
                }
            }
            _ => {
                if let Some(turn) = current.as_mut() {
                    turn.ended_at_epoch_ms = timestamp_epoch_ms;
                }
            }
        }
    }
    if let Some(turn) = current.take() {
        observations.push(turn);
    }
    Ok(observations)
}

fn rollout_function_call_is_context_pack(name: &str, arguments: &str) -> bool {
    if name == "mcp__amai__amai_context_pack" || name == "mcp__echovault__amai_context_pack" {
        return true;
    }
    if name != "exec_command" {
        return false;
    }
    exec_command_invokes_context_pack(arguments)
}

fn exec_command_invokes_context_pack(arguments: &str) -> bool {
    let shell = extract_exec_command_shell(arguments);
    let shell = strip_shell_heredoc_bodies(&shell);
    shell
        .split(['\n', ';'])
        .flat_map(|line| line.split("&&"))
        .flat_map(|line| line.split("||"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(shell_segment_invokes_context_pack)
}

fn extract_exec_command_shell(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("cmd")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| arguments.to_string())
}

fn strip_shell_heredoc_bodies(shell: &str) -> String {
    let mut cleaned = Vec::new();
    let mut active_delimiter = None::<String>;
    for line in shell.lines() {
        if let Some(delimiter) = active_delimiter.as_ref() {
            if line.trim() == delimiter {
                active_delimiter = None;
            }
            continue;
        }
        if let Some(delimiter) = shell_heredoc_delimiter(line) {
            active_delimiter = Some(delimiter);
        }
        cleaned.push(line);
    }
    cleaned.join("\n")
}

fn shell_heredoc_delimiter(line: &str) -> Option<String> {
    let marker = line.find("<<")?;
    let token = line[marker + 2..].split_whitespace().next()?;
    let delimiter = token.trim_matches(|c| matches!(c, '\'' | '"' | '-'));
    (!delimiter.is_empty()).then_some(delimiter.to_string())
}

fn shell_segment_invokes_context_pack(segment: &str) -> bool {
    let words = segment
        .split_whitespace()
        .map(shell_token_normalized)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if words.is_empty() {
        return false;
    }
    shell_words_contain_context_pack_invocation(&words)
        || shell_words_contain_memory_search_invocation(&words)
}

fn shell_words_contain_context_pack_invocation(words: &[String]) -> bool {
    for (index, word) in words.iter().enumerate() {
        if word == "cargo" {
            if words[index + 1..].contains(&"run".to_string())
                && shell_words_have_context_pack_subcommand(&words[index + 1..])
            {
                return true;
            }
            continue;
        }
        if is_amai_command_token(word)
            && shell_words_have_context_pack_subcommand(&words[index + 1..])
        {
            return true;
        }
    }
    false
}

fn shell_words_have_context_pack_subcommand(words: &[String]) -> bool {
    words
        .windows(2)
        .any(|pair| pair[0] == "context" && pair[1] == "pack" || pair[0] == "context-pack")
}

fn shell_words_contain_memory_search_invocation(words: &[String]) -> bool {
    words
        .windows(2)
        .any(|pair| (pair[0] == "memory" || pair[0].ends_with("/memory")) && pair[1] == "search")
}

fn is_amai_command_token(word: &str) -> bool {
    word == "amai"
        || word == "$amai"
        || word == "${amai}"
        || word.ends_with("/amai")
        || word.ends_with("/target/release/amai")
}

fn shell_token_normalized(token: &str) -> String {
    token
        .trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '`'
            )
        })
        .to_ascii_lowercase()
}

fn collect_context_pack_ids_from_rollout_output(value: &Value, target: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(context_pack_id) = map.get("context_pack_id").and_then(Value::as_str)
                && !context_pack_id.is_empty()
            {
                target.insert(context_pack_id.to_string());
            }
            for value in map.values() {
                collect_context_pack_ids_from_rollout_output(value, target);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_context_pack_ids_from_rollout_output(item, target);
            }
        }
        Value::String(text) => {
            let trimmed = text.trim();
            if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
                return;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                collect_context_pack_ids_from_rollout_output(&parsed, target);
            }
        }
        _ => {}
    }
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

fn build_time_slices(messages: &[RolloutMessage], limit: usize) -> Vec<ThreadTimeSliceSummary> {
    if messages.is_empty() {
        return Vec::new();
    }
    let mut slices = Vec::new();
    let mut segment_start = 0usize;
    for index in 1..messages.len() {
        if messages[index].role == "user" {
            if let Some(slice) = build_time_slice_from_segment(&messages[segment_start..index]) {
                slices.push(slice);
            }
            segment_start = index;
        }
    }
    if let Some(slice) = build_time_slice_from_segment(&messages[segment_start..]) {
        slices.push(slice);
    }
    if slices.len() > limit {
        slices = slices[slices.len().saturating_sub(limit)..].to_vec();
    }
    slices
}

fn build_time_slice_from_segment(segment: &[RolloutMessage]) -> Option<ThreadTimeSliceSummary> {
    if segment.is_empty() {
        return None;
    }
    let first = segment.first()?;
    let user = segment
        .iter()
        .find(|message| message.role == "user" && !message.text.trim().is_empty());
    let assistant = segment
        .iter()
        .rev()
        .find(|message| {
            message.role == "assistant"
                && !message.text.trim().is_empty()
                && matches!(
                    message.phase.as_deref(),
                    Some("final_answer") | Some("final") | Some("final_response") | None
                )
        })
        .or_else(|| {
            segment
                .iter()
                .rev()
                .find(|message| message.role == "assistant" && !message.text.trim().is_empty())
        });
    let last = assistant.unwrap_or_else(|| segment.last().expect("non-empty segment"));
    let started_at_epoch_s = parse_rfc3339_epoch_s(&first.timestamp)
        .ok()
        .unwrap_or_default();
    let ended_at_epoch_s = parse_rfc3339_epoch_s(&last.timestamp)
        .ok()
        .unwrap_or(started_at_epoch_s);
    let user_anchor = user
        .map(|message| primary_user_anchor(&message.text, 220))
        .unwrap_or_default();
    let assistant_anchor = assistant
        .map(|message| collapse_text(&message.text, 220))
        .unwrap_or_default();
    let summary_headline = assistant
        .and_then(|message| compact_headline_from_text(&message.text, 220))
        .or_else(|| {
            (!user_anchor.is_empty()).then_some(if user_anchor.chars().count() > 220 {
                user_anchor.chars().take(220).collect::<String>()
            } else {
                user_anchor.clone()
            })
        })
        .unwrap_or_default();
    if summary_headline.is_empty() && assistant_anchor.is_empty() && user_anchor.is_empty() {
        return None;
    }
    Some(ThreadTimeSliceSummary {
        started_at: first.timestamp.clone(),
        ended_at: last.timestamp.clone(),
        started_at_epoch_s,
        ended_at_epoch_s,
        user_anchor,
        assistant_anchor,
        summary_headline,
        summary_next_step: assistant
            .and_then(|message| compact_next_step_from_text(&message.text))
            .unwrap_or_default(),
    })
}

fn select_time_slice_for_epoch(
    slices: &[ThreadTimeSliceSummary],
    target_epoch_s: i64,
) -> Option<ThreadTimeSliceSummary> {
    slices
        .iter()
        .filter_map(|slice| {
            let contains = slice.started_at_epoch_s > 0
                && slice.ended_at_epoch_s > 0
                && slice.started_at_epoch_s <= target_epoch_s
                && target_epoch_s <= slice.ended_at_epoch_s;
            let width = if slice.started_at_epoch_s > 0
                && slice.ended_at_epoch_s > 0
                && slice.ended_at_epoch_s >= slice.started_at_epoch_s
            {
                slice.ended_at_epoch_s - slice.started_at_epoch_s
            } else {
                i64::MAX
            };
            let before = slice.ended_at_epoch_s > 0 && slice.ended_at_epoch_s <= target_epoch_s;
            let after = slice.started_at_epoch_s > 0 && slice.started_at_epoch_s >= target_epoch_s;
            let rank = if contains {
                (
                    0_i32,
                    width,
                    target_epoch_s - slice.ended_at_epoch_s,
                    slice.started_at_epoch_s,
                )
            } else if before {
                (
                    1_i32,
                    target_epoch_s - slice.ended_at_epoch_s,
                    width,
                    slice.started_at_epoch_s,
                )
            } else if after {
                (
                    2_i32,
                    slice.started_at_epoch_s - target_epoch_s,
                    width,
                    slice.started_at_epoch_s,
                )
            } else {
                return None;
            };
            Some((rank, slice))
        })
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, slice)| slice.clone())
}

fn time_slice_matches_exact_time(slice: &ThreadTimeSliceSummary, target_epoch_s: i64) -> bool {
    if slice.started_at_epoch_s > 0
        && slice.ended_at_epoch_s > 0
        && slice.started_at_epoch_s <= target_epoch_s
        && target_epoch_s <= slice.ended_at_epoch_s
    {
        return true;
    }
    let distance = if slice.ended_at_epoch_s > 0 && slice.ended_at_epoch_s <= target_epoch_s {
        target_epoch_s - slice.ended_at_epoch_s
    } else if slice.started_at_epoch_s > 0 && slice.started_at_epoch_s >= target_epoch_s {
        slice.started_at_epoch_s - target_epoch_s
    } else {
        i64::MAX
    };
    distance <= EXACT_TIME_MAX_SLICE_DRIFT_S
}

fn time_slice_messages(slice: &ThreadTimeSliceSummary, count: usize) -> Vec<TranscriptMessage> {
    if count == 0 {
        return Vec::new();
    }
    let mut messages = Vec::new();
    if count >= 2 && !slice.user_anchor.is_empty() {
        messages.push(TranscriptMessage {
            role: "user".to_string(),
            text: slice.user_anchor.clone(),
        });
    }
    if !slice.assistant_anchor.is_empty() {
        messages.push(TranscriptMessage {
            role: "assistant".to_string(),
            text: slice.assistant_anchor.clone(),
        });
    } else if !slice.summary_headline.is_empty() {
        messages.push(TranscriptMessage {
            role: "assistant".to_string(),
            text: slice.summary_headline.clone(),
        });
    }
    if count == 1 && messages.len() > 1 {
        messages = messages.split_off(messages.len() - 1);
    }
    messages
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
        EXACT_TIME_MAX_SLICE_DRIFT_S, RolloutAssistantGenerationObservation,
        ThreadTimeSliceSummary, chat_tail_at_time_from_snapshots, collapse_text,
        compact_headline_from_text, compact_next_step_from_text,
        extract_chat_messages_from_rollout_text, extract_last_messages,
        latest_rollout_assistant_generation_observation, nth_previous_chat_tail_from_snapshots,
        parse_rfc3339_epoch_s, parse_role_heading, rendered_transcript_summary,
        rollout_assistant_generation_observations, rollout_source_signature,
        rollout_summary_from_path, rollout_thread_id_from_path, select_messages_for_time,
        select_tail_messages, time_slice_matches_exact_time,
    };
    use crate::postgres::ObservabilitySnapshotRecord;
    use proptest::prelude::*;
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
    fn compact_headline_skips_workspace_status_prefix() {
        let text = "Workspace совпадает: `/home/art/Art`. По каноническому `Amai continuity startup` продолжаем с текущей materialized-линии: `Amai working-state continuity recovery materialized`.";
        let headline = compact_headline_from_text(text, 220).expect("headline");
        assert_eq!(
            headline,
            "По каноническому `Amai continuity startup` продолжаем с текущей materialized-линии: `Amai working-state continuity recovery materialized`."
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
    fn rollout_thread_id_from_path_extracts_uuid_tail() {
        let path = std::path::Path::new(
            "/tmp/rollout-2026-03-13T01-44-23-019ce438-f3bd-7c60-aa28-3284a96bfeb5.jsonl",
        );
        assert_eq!(
            rollout_thread_id_from_path(path).as_deref(),
            Some("019ce438-f3bd-7c60-aa28-3284a96bfeb5")
        );
    }

    #[test]
    fn rollout_source_signature_changes_with_file_metadata() {
        let rollout_path =
            std::env::temp_dir().join(format!("amai-rollout-signature-{}.jsonl", Uuid::new_v4()));
        fs::write(&rollout_path, "{\"type\":\"noop\"}\n").expect("write rollout");
        let first = rollout_source_signature("thread-1", &rollout_path);
        fs::write(&rollout_path, "{\"type\":\"noop\"}\n{\"type\":\"noop2\"}\n")
            .expect("rewrite rollout");
        let second = rollout_source_signature("thread-1", &rollout_path);
        let _ = fs::remove_file(&rollout_path);

        assert_ne!(first, second);
    }

    #[test]
    fn latest_rollout_assistant_generation_observation_reads_unambiguous_turn() {
        let rollout_path =
            std::env::temp_dir().join(format!("amai-rollout-observed-{}.jsonl", Uuid::new_v4()));
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-03-25T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"timestamp":"2026-03-25T10:00:00Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"cargo run --quiet -- context pack --project amai --namespace default --query 'x'\"}"}}
{"timestamp":"2026-03-25T10:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"{\"context_pack_id\":\"ctx-pack-1\"}"}}
{"timestamp":"2026-03-25T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":17}}}}
{"timestamp":"2026-03-25T10:00:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":23}}}}
{"timestamp":"2026-03-25T10:00:04Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}
"#,
        )
        .expect("write rollout");

        let observation =
            latest_rollout_assistant_generation_observation("/home/art/Art", Some(&rollout_path))
                .expect("observation")
                .expect("candidate");
        let _ = fs::remove_file(&rollout_path);

        assert_eq!(
            observation,
            RolloutAssistantGenerationObservation {
                thread_id: rollout_thread_id_from_path(&rollout_path).expect("thread id"),
                rollout_path: rollout_path.display().to_string(),
                turn_id: "turn-1".to_string(),
                context_pack_id: "ctx-pack-1".to_string(),
                assistant_generation_tokens: 40,
                token_count_events: 2,
                observation_source: "codex_rollout_last_token_usage_sum_v1".to_string(),
            }
        );
    }

    #[test]
    fn latest_rollout_assistant_generation_observation_fails_closed_on_ambiguous_context_pack_ids()
    {
        let rollout_path = std::env::temp_dir().join(format!(
            "amai-rollout-observed-ambiguous-{}.jsonl",
            Uuid::new_v4()
        ));
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-03-25T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"timestamp":"2026-03-25T10:00:00Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"cargo run --quiet -- context pack --project amai --namespace default --query 'x'\"}"}}
{"timestamp":"2026-03-25T10:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"{\"context_pack_id\":\"ctx-pack-1\"}"}}
{"timestamp":"2026-03-25T10:00:02Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"{\"context_pack_id\":\"ctx-pack-2\"}"}}
{"timestamp":"2026-03-25T10:00:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":17}}}}
{"timestamp":"2026-03-25T10:00:04Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}
"#,
        )
        .expect("write rollout");

        let observation =
            latest_rollout_assistant_generation_observation("/home/art/Art", Some(&rollout_path))
                .expect("observation");
        let _ = fs::remove_file(&rollout_path);

        assert!(observation.is_none());
    }

    #[test]
    fn rollout_assistant_generation_observations_collect_multiple_unambiguous_turns() {
        let rollout_path = std::env::temp_dir().join(format!(
            "amai-rollout-observed-many-{}.jsonl",
            Uuid::new_v4()
        ));
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-03-25T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"timestamp":"2026-03-25T10:00:00Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"cargo run --quiet -- context pack --project amai --namespace default --query 'x'\"}"}}
{"timestamp":"2026-03-25T10:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"{\"context_pack_id\":\"ctx-pack-1\"}"}}
{"timestamp":"2026-03-25T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":17}}}}
{"timestamp":"2026-03-25T10:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}
{"timestamp":"2026-03-25T10:00:04Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-2"}}
{"timestamp":"2026-03-25T10:00:04Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-2","arguments":"{\"cmd\":\"cargo run --quiet -- context pack --project amai --namespace default --query 'y'\"}"}}
{"timestamp":"2026-03-25T10:00:05Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-2","output":"{\"context_pack_id\":\"ctx-pack-2\"}"}}
{"timestamp":"2026-03-25T10:00:06Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":23}}}}
{"timestamp":"2026-03-25T10:00:07Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-2"}}
"#,
        )
        .expect("write rollout");

        let observations =
            rollout_assistant_generation_observations("/home/art/Art", Some(&rollout_path))
                .expect("observations");
        let _ = fs::remove_file(&rollout_path);

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].turn_id, "turn-1");
        assert_eq!(observations[0].context_pack_id, "ctx-pack-1");
        assert_eq!(observations[0].assistant_generation_tokens, 17);
        assert_eq!(observations[1].turn_id, "turn-2");
        assert_eq!(observations[1].context_pack_id, "ctx-pack-2");
        assert_eq!(observations[1].assistant_generation_tokens, 23);
    }

    #[test]
    fn rollout_assistant_generation_turn_observations_require_approved_context_pack_calls() {
        let rollout_path =
            std::env::temp_dir().join(format!("amai-rollout-turns-{}.jsonl", Uuid::new_v4()));
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-03-25T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"timestamp":"2026-03-25T10:00:00Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"cargo run --quiet -- context pack --project amai --namespace default --query 'x'\"}"}}
{"timestamp":"2026-03-25T10:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"{\"context_pack_id\":\"ctx-pack-1\"}"}}
{"timestamp":"2026-03-25T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":17}}}}
{"timestamp":"2026-03-25T10:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}
{"timestamp":"2026-03-25T10:00:04Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-2"}}
{"timestamp":"2026-03-25T10:00:04Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-2","arguments":"{\"cmd\":\"./target/release/amai observe token-report\"}"}}
{"timestamp":"2026-03-25T10:00:05Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-2","output":"{\"context_pack_id\":\"ctx-pack-noise\"}"}}
{"timestamp":"2026-03-25T10:00:06Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":23}}}}
{"timestamp":"2026-03-25T10:00:07Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-2"}}
"#,
        )
        .expect("write rollout");

        let direct = super::parse_rollout_assistant_generation_turn_observations(
            rollout_thread_id_from_path(&rollout_path)
                .as_deref()
                .unwrap_or(""),
            &rollout_path,
        )
        .expect("direct parse");
        let _ = fs::remove_file(&rollout_path);
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].turn_id, "turn-1");
        assert_eq!(direct[0].approved_context_pack_calls, 1);
        assert_eq!(direct[0].assistant_generation_tokens, 17);
    }

    #[test]
    fn rollout_assistant_generation_turn_observations_ignore_heredoc_mentions() {
        let rollout_path = std::env::temp_dir().join(format!(
            "amai-rollout-turns-heredoc-{}.jsonl",
            Uuid::new_v4()
        ));
        fs::write(
            &rollout_path,
            r#"{"timestamp":"2026-03-25T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-noise"}}
{"timestamp":"2026-03-25T10:00:00Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-noise","arguments":"{\"cmd\":\"cat >/tmp/handoff.txt <<'EOF'\n./target/release/amai context pack --project amai --namespace default --query 'noise'\nThese context packs belong to one thread only in this note.\nEOF\"}"}}
{"timestamp":"2026-03-25T10:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-noise","output":"{\"context_pack_id\":\"ctx-pack-noise\"}"}}
{"timestamp":"2026-03-25T10:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":19}}}}
{"timestamp":"2026-03-25T10:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-noise"}}
{"timestamp":"2026-03-25T10:00:04Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-real"}}
{"timestamp":"2026-03-25T10:00:04Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-real","arguments":"{\"cmd\":\"AMAI=/home/art/agent-memory-index/target/release/amai\n$AMAI context pack --project amai --namespace default --query 'real'\"}"}}
{"timestamp":"2026-03-25T10:00:05Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-real","output":"{\"context_pack_id\":\"ctx-pack-real\"}"}}
{"timestamp":"2026-03-25T10:00:06Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"output_tokens":23}}}}
{"timestamp":"2026-03-25T10:00:07Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-real"}}
"#,
        )
        .expect("write rollout");

        let direct = super::parse_rollout_assistant_generation_turn_observations(
            rollout_thread_id_from_path(&rollout_path)
                .as_deref()
                .unwrap_or(""),
            &rollout_path,
        )
        .expect("direct parse");
        let _ = fs::remove_file(&rollout_path);

        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].turn_id, "turn-real");
        assert_eq!(direct[0].approved_context_pack_calls, 1);
        assert_eq!(direct[0].assistant_generation_tokens, 23);
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

    proptest! {
        #[test]
        fn time_slice_matches_exact_time_for_any_target_inside_slice(
            start in 1_700_000_000_i64..1_800_000_000_i64,
            width in 0_i64..7_200_i64,
            inside_offset in 0_i64..7_200_i64,
        ) {
            let width = width.max(inside_offset);
            let slice = ThreadTimeSliceSummary {
                started_at: String::new(),
                ended_at: String::new(),
                started_at_epoch_s: start,
                ended_at_epoch_s: start + width,
                user_anchor: String::new(),
                assistant_anchor: String::new(),
                summary_headline: String::new(),
                summary_next_step: String::new(),
            };
            let target = start + inside_offset.min(width);
            prop_assert!(time_slice_matches_exact_time(&slice, target));
        }

        #[test]
        fn time_slice_matches_exact_time_for_targets_within_allowed_drift(
            start in 1_700_000_000_i64..1_800_000_000_i64,
            width in 1_i64..3_600_i64,
            drift in 0_i64..10_800_i64,
            before in any::<bool>(),
        ) {
            let drift = drift.min(EXACT_TIME_MAX_SLICE_DRIFT_S);
            let slice = ThreadTimeSliceSummary {
                started_at: String::new(),
                ended_at: String::new(),
                started_at_epoch_s: start,
                ended_at_epoch_s: start + width,
                user_anchor: String::new(),
                assistant_anchor: String::new(),
                summary_headline: String::new(),
                summary_next_step: String::new(),
            };
            let target = if before {
                start - drift
            } else {
                start + width + drift
            };
            prop_assert!(time_slice_matches_exact_time(&slice, target));
        }

        #[test]
        fn time_slice_rejects_targets_beyond_exact_time_drift(
            start in 1_700_000_000_i64..1_800_000_000_i64,
            width in 1_i64..3_600_i64,
            extra in 1_i64..10_800_i64,
            before in any::<bool>(),
        ) {
            let slice = ThreadTimeSliceSummary {
                started_at: String::new(),
                ended_at: String::new(),
                started_at_epoch_s: start,
                ended_at_epoch_s: start + width,
                user_anchor: String::new(),
                assistant_anchor: String::new(),
                summary_headline: String::new(),
                summary_next_step: String::new(),
            };
            let distance = EXACT_TIME_MAX_SLICE_DRIFT_S + extra;
            let target = if before {
                start - distance
            } else {
                start + width + distance
            };
            prop_assert!(!time_slice_matches_exact_time(&slice, target));
        }
    }

    #[test]
    fn chat_tail_at_time_from_snapshots_works_with_time_slices_only() {
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
                        "last_assistant_message": "старый ответ",
                        "time_slices": [
                            {
                                "started_at": "2025-03-21T10:40:00Z",
                                "ended_at": "2025-03-21T10:41:20Z",
                                "started_at_epoch_s": 1742553600,
                                "ended_at_epoch_s": 1742553680,
                                "user_anchor": "старый вопрос",
                                "assistant_anchor": "старый ответ",
                                "summary_headline": "старый чат",
                                "summary_next_step": ""
                            }
                        ]
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
                        "last_assistant_message": "новый ответ",
                        "time_slices": [
                            {
                                "started_at": "2025-03-21T10:46:00Z",
                                "ended_at": "2025-03-21T10:47:00Z",
                                "started_at_epoch_s": 1742553960,
                                "ended_at_epoch_s": 1742554020,
                                "user_anchor": "новый вопрос",
                                "assistant_anchor": "новый ответ",
                                "summary_headline": "новый чат",
                                "summary_next_step": ""
                            }
                        ]
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
    fn chat_tail_at_time_from_snapshots_prefers_time_slice_summary_without_rollout() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: "continuity_thread_index".to_string(),
            created_at_epoch_ms: 1_744_087_815_000,
            payload: json!({
                "continuity_thread_index": {
                    "project": {"code": "art"},
                    "namespace": {"code": "continuity"},
                    "thread_id": "thread-slice",
                    "title": "сырой заголовок",
                    "started_at": "2025-03-21T10:39:00Z",
                    "ended_at": "2025-03-21T10:45:00Z",
                    "created_at_epoch_s": 1_742_553_540,
                    "updated_at_epoch_s": 1_742_553_900,
                    "summary_headline": "thread-level headline",
                    "summary_next_step": "thread-level next step",
                    "time_slices": [
                        {
                            "started_at": "2025-03-21T10:40:00Z",
                            "ended_at": "2025-03-21T10:41:10Z",
                            "started_at_epoch_s": 1742553600,
                            "ended_at_epoch_s": 1742553670,
                            "user_anchor": "разбирали как exact-time lookup должен брать готовый смысловой срез",
                            "assistant_anchor": "нужно materialize time-slices upstream",
                            "summary_headline": "exact-time lookup должен брать готовый смысловой срез",
                            "summary_next_step": "materialize time-slices upstream"
                        }
                    ]
                }
            }),
        }];

        let tail = chat_tail_at_time_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            "2025-03-21T10:40:30Z",
            2,
        )
        .expect("tail result")
        .expect("tail");

        assert_eq!(tail.thread_id, "thread-slice");
        assert_eq!(
            tail.summary_headline.as_deref(),
            Some("exact-time lookup должен брать готовый смысловой срез")
        );
        assert_eq!(
            tail.summary_next_step.as_deref(),
            Some("materialize time-slices upstream")
        );
        assert_eq!(tail.messages.len(), 2);
        assert_eq!(tail.messages[0].role, "user");
        assert!(
            tail.messages[0]
                .text
                .contains("exact-time lookup должен брать готовый смысловой срез")
        );
        assert_eq!(tail.messages[1].role, "assistant");
        assert_eq!(
            tail.messages[1].text,
            "нужно materialize time-slices upstream"
        );
        assert_eq!(
            tail.selected_time_slice,
            Some(ThreadTimeSliceSummary {
                started_at: "2025-03-21T10:40:00Z".to_string(),
                ended_at: "2025-03-21T10:41:10Z".to_string(),
                started_at_epoch_s: 1742553600,
                ended_at_epoch_s: 1742553670,
                user_anchor: "разбирали как exact-time lookup должен брать готовый смысловой срез"
                    .to_string(),
                assistant_anchor: "нужно materialize time-slices upstream".to_string(),
                summary_headline: "exact-time lookup должен брать готовый смысловой срез"
                    .to_string(),
                summary_next_step: "materialize time-slices upstream".to_string(),
            })
        );
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
    fn chat_tail_at_time_from_snapshots_fail_closes_when_nearest_slice_is_too_far() {
        let snapshots = vec![ObservabilitySnapshotRecord {
            snapshot_id: Uuid::nil(),
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

        let tail = chat_tail_at_time_from_snapshots(
            &snapshots,
            "art",
            "continuity",
            "2026-03-18T12:00:00+03:00",
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
                    "last_assistant_message": "про temporal lookup",
                    "time_slices": [
                        {
                            "started_at": "2025-03-21T10:40:00Z",
                            "ended_at": "2025-03-21T10:41:20Z",
                            "started_at_epoch_s": 1742553600,
                            "ended_at_epoch_s": 1742553680,
                            "user_anchor": "о чем мы говорили?",
                            "assistant_anchor": "про temporal lookup",
                            "summary_headline": "про temporal lookup",
                            "summary_next_step": ""
                        }
                    ]
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
