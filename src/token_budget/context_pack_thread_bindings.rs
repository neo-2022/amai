use crate::postgres;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tokio_postgres::Client;

use super::{
    ObservabilitySnapshotRecord, WorkingStateContextPackMeta,
    cached_dashboard_working_state_metadata, dashboard_working_state_metadata_signature,
    filter_context_pack_metadata, preferred_dashboard_thread_binding_hint,
    store_dashboard_working_state_metadata,
};

pub(super) async fn latest_working_state_context_pack_metadata(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, WorkingStateContextPackMeta>> {
    if context_pack_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let summary =
        postgres::summarize_observability_snapshots_by_kinds(db, &["working_state_event"]).await?;
    let signature = dashboard_working_state_metadata_signature(&summary);
    if let Some(metadata) = cached_dashboard_working_state_metadata(&signature) {
        return Ok(filter_context_pack_metadata(&metadata, context_pack_ids));
    }
    let target_ids = context_pack_ids.iter().cloned().collect::<Vec<_>>();
    let rows = postgres::list_latest_observability_snapshots_by_payload_string_field(
        db,
        "working_state_event",
        "working_state_event",
        "context_pack_id",
        &target_ids,
    )
    .await?;
    let mut metadata = BTreeMap::new();
    for row in rows {
        let node = &row.payload["working_state_event"];
        if node["event_kind"].as_str() != Some("retrieval_context_pack") {
            continue;
        }
        let Some(context_pack_id) = node["context_pack_id"].as_str() else {
            continue;
        };
        if !context_pack_ids.contains(context_pack_id) || metadata.contains_key(context_pack_id) {
            continue;
        }
        let thread_id = node["thread_id"].as_str().unwrap_or_default().to_string();
        if thread_id.is_empty() {
            continue;
        }
        metadata.insert(
            context_pack_id.to_string(),
            WorkingStateContextPackMeta {
                thread_id,
                captured_at_epoch_ms: row.created_at_epoch_ms,
                turn_id: node["turn_id"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            },
        );
    }
    store_dashboard_working_state_metadata(&signature, &metadata);
    Ok(filter_context_pack_metadata(&metadata, context_pack_ids))
}

pub(super) fn merged_context_pack_rollout_metadata(
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    rows: &BTreeMap<String, ObservabilitySnapshotRecord>,
    context_pack_ids: &BTreeSet<String>,
) -> BTreeMap<String, WorkingStateContextPackMeta> {
    let mut merged = BTreeMap::new();
    for context_pack_id in context_pack_ids {
        let existing = metadata.get(context_pack_id);
        let row = rows.get(context_pack_id);
        let thread_id = existing
            .map(|item| item.thread_id.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                row.and_then(|item| {
                    item.payload["token_budget_event"]["thread_id"]
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
            });
        let Some(thread_id) = thread_id else {
            continue;
        };
        let turn_id = existing
            .map(|item| item.turn_id.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                row.and_then(|item| {
                    item.payload["token_budget_event"]["turn_id"]
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
            })
            .unwrap_or_default();
        merged.insert(
            context_pack_id.clone(),
            WorkingStateContextPackMeta {
                thread_id,
                captured_at_epoch_ms: existing
                    .map(|item| item.captured_at_epoch_ms)
                    .unwrap_or_default(),
                turn_id,
            },
        );
    }
    merged
}

pub(super) fn merged_context_pack_thread_ids(
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    rows: &BTreeMap<String, ObservabilitySnapshotRecord>,
    context_pack_ids: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut merged = metadata
        .iter()
        .map(|(context_pack_id, item)| (context_pack_id.clone(), item.thread_id.clone()))
        .collect::<BTreeMap<_, _>>();
    for (context_pack_id, row) in rows {
        if !context_pack_ids.contains(context_pack_id) || merged.contains_key(context_pack_id) {
            continue;
        }
        let thread_id = row.payload["token_budget_event"]["thread_id"]
            .as_str()
            .unwrap_or_default()
            .trim();
        if thread_id.is_empty() {
            continue;
        }
        merged.insert(context_pack_id.clone(), thread_id.to_string());
    }
    merged
}

pub(super) fn merged_context_pack_thread_ids_with_repo_fallback(
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    rows: &BTreeMap<String, ObservabilitySnapshotRecord>,
    repo_thread_ids: &BTreeMap<String, String>,
    context_pack_ids: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut merged = merged_context_pack_thread_ids(metadata, rows, context_pack_ids);
    for context_pack_id in context_pack_ids {
        if merged.contains_key(context_pack_id) {
            continue;
        }
        let Some(thread_id) = repo_thread_ids
            .get(context_pack_id)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        merged.insert(context_pack_id.clone(), thread_id.to_string());
    }
    merged
}

pub(super) async fn context_pack_repo_roots(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    if context_pack_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let target_ids = context_pack_ids.iter().cloned().collect::<Vec<_>>();
    let rows = db
        .query(
            r#"
            SELECT
                context_pack_id::text,
                payload->'project'->>'repo_root' AS repo_root
            FROM ami.context_packs
            WHERE context_pack_id::text = ANY($1)
            "#,
            &[&target_ids],
        )
        .await
        .context("failed to load context pack repo_root bindings")?;
    let mut repo_roots = BTreeMap::new();
    for row in rows {
        let context_pack_id: String = row.get(0);
        let repo_root = row.get::<_, Option<String>>(1).unwrap_or_default();
        if context_pack_ids.contains(&context_pack_id) && !repo_root.trim().is_empty() {
            repo_roots.insert(context_pack_id, repo_root);
        }
    }
    Ok(repo_roots)
}

pub(super) async fn repo_fallback_thread_ids_for_context_packs(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let repo_roots = context_pack_repo_roots(db, context_pack_ids).await?;
    let mut grouped_ids = BTreeMap::<String, Vec<String>>::new();
    for (context_pack_id, repo_root) in repo_roots {
        grouped_ids
            .entry(repo_root)
            .or_default()
            .push(context_pack_id);
    }

    let mut resolved = BTreeMap::new();
    for (repo_root, ids) in grouped_ids {
        let Some(thread_id) = preferred_dashboard_thread_binding_hint(db, Path::new(&repo_root))
            .await?
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        for context_pack_id in ids {
            resolved.insert(context_pack_id, thread_id.clone());
        }
    }
    Ok(resolved)
}
