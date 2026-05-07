use crate::postgres;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tokio_postgres::Client;
use uuid::Uuid;

use super::{
    MAX_PERSISTED_PROJECT_TASK_LEDGER_HISTORICAL_ENTRIES, NamespaceRecord, ProjectRecord,
    WORKING_STATE_RESTORE_KIND, WORKSPACE_RESTORE_PACK_ENVELOPE_VERSION,
    build_workspace_restore_pack,
};

pub(super) fn compact_persisted_project_task_ledger(ledger: &Value) -> Value {
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

pub(super) fn persisted_restore_snapshot_payload(bundle: &Value) -> Value {
    let mut payload = json!({
        "working_state_restore": bundle["working_state_restore"].clone()
    });
    let authoritative_event_id =
        payload["working_state_restore"]["state_lineage"]["authoritative_event_id"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned);
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
    if let Some(authoritative_event_id) = authoritative_event_id {
        if let Some(root) = payload.as_object_mut() {
            let observability = root
                .entry("_observability".to_string())
                .or_insert_with(|| json!({}));
            if let Some(object) = observability.as_object_mut() {
                object.insert(
                    "source_event_id".to_string(),
                    Value::String(authoritative_event_id),
                );
                object.insert(
                    "source_kind".to_string(),
                    Value::String("working_state_restore_runtime".to_string()),
                );
            }
        }
    }
    payload
}

pub(super) fn restore_pack_source_event_ids(restore: &Value) -> Value {
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

pub(super) fn restore_pack_artifact_refs(restore: &Value) -> Value {
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

pub(super) fn restore_pack_message_refs(restore: &Value) -> Value {
    let mut refs = BTreeSet::new();
    if let Some(value) = restore["thread_id"].as_str() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            refs.insert(format!("thread:{trimmed}"));
        }
    }
    Value::Array(refs.into_iter().map(Value::String).collect())
}

pub(super) async fn materialize_restore_pack(
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
    match postgres::create_restore_pack_detailed(
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
    .await
    {
        Ok(_) => {}
        Err(error)
            if error.phase == postgres::RestorePackCreateErrorPhase::OutcomeUnknownAfterWrite =>
        {
            let recovered_pack = postgres::lookup_restore_pack_by_source_snapshot_id(
                db,
                project.project_id,
                namespace.namespace_id,
                "workspace_restore_pack",
                source_snapshot_id,
            )
            .await?
            .with_context(|| {
                format!(
                    "workspace_restore_pack create outcome unknown but no persisted restore_pack found for source_snapshot_id={}",
                    source_snapshot_id
                )
            })?;
            let recovered_snapshot_id = recovered_pack
                .source_snapshot_id
                .with_context(|| {
                    format!(
                        "workspace_restore_pack recovered after ambiguous create is missing source_snapshot_id for restore_pack_id={}",
                        recovered_pack.restore_pack_id
                    )
                })?;
            if recovered_snapshot_id != source_snapshot_id {
                return Err(anyhow::anyhow!(
                    "workspace_restore_pack recovered after ambiguous create points to unexpected source_snapshot_id={} expected={}",
                    recovered_snapshot_id,
                    source_snapshot_id
                ));
            }
        }
        Err(error) => return Err(error.error),
    }
    Ok(())
}
