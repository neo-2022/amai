use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use tokio_postgres::Client;
use tracing::{info, warn};
use uuid::Uuid;

use crate::postgres::{MemoryItemInsert, create_memory_item};

/// Result of one auto-extract batch.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AutoExtractBatchResult {
    pub scanned: usize,
    pub created: usize,
    pub skipped_already_extracted: usize,
    pub errors: usize,
}

const BATCH_SIZE: i64 = 100;

/// Scan raw memory events and promote unprocessed ones to memory_items.
///
/// ponytail: reuse existing create_memory_item pipeline; no new tables, no LLM, no manual tags.
pub async fn run_auto_extract(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    limit: i64,
) -> Result<AutoExtractBatchResult> {
    let limit = if limit <= 0 { BATCH_SIZE } else { limit };
    let rows = client
        .query(
            r#"
            SELECT
                mre.memory_raw_event_id,
                mre.title,
                mre.summary,
                mre.body,
                mre.payload,
                mre.evidence_span,
                mre.server_received_at_epoch_ms
            FROM ami.memory_raw_events mre
            WHERE mre.project_id = (SELECT project_id FROM ami.projects WHERE code = $1)
              AND mre.namespace_id = (SELECT namespace_id FROM ami.namespaces WHERE code = $2
                                      AND project_id = (SELECT project_id FROM ami.projects WHERE code = $1))
              AND mre.event_kind = 'memory_candidate_write'
              AND mre.item_kind = 'raw_fact'
              AND mre.derivation_kind IN ('raw_capture', 'auto_extract')
              AND NOT EXISTS (
                  SELECT 1 FROM ami.memory_provenance mp
                  WHERE mp.project_id = mre.project_id
                    AND mp.namespace_id = mre.namespace_id
                    AND mp.source_event_id = mre.memory_raw_event_id::text
              )
            ORDER BY mre.server_order_seq
            LIMIT $3
            "#,
            &[&project_code, &namespace_code, &limit],
        )
        .await
        .context("failed to list raw memory events for auto-extract")?;

    let mut result = AutoExtractBatchResult {
        scanned: rows.len(),
        created: 0,
        skipped_already_extracted: 0,
        errors: 0,
    };

    for row in rows {
        let raw_event_id: Uuid = row.get(0);
        let title: String = row.get(1);
        let summary: Option<String> = row.get(2);
        let body: Option<String> = row.get(3);
        let payload: JsonValue = row.get::<_, JsonValue>(4);
        let evidence_span: JsonValue = row.get::<_, JsonValue>(5);
        let server_received_at: i64 = row.get(6);

        let event_id_str = raw_event_id.to_string();
        let source_event_ids = vec![event_id_str.clone()];

        let mut composed_body = body.unwrap_or_default();
        if !composed_body.is_empty() && summary.is_some() {
            composed_body.push_str("\n\n");
        }
        if let Some(s) = &summary {
            composed_body.push_str(s);
        }
        if composed_body.is_empty() {
            composed_body = format!("auto-extracted from raw event {}", raw_event_id);
        }

        let record = MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: None,
            title: &title,
            summary: Some(&composed_body[..composed_body.len().min(512)]),
            body: Some(&composed_body),
            sensitivity_class: Some("internal"),
            truth_state: Some("raw"),
            trust_state: Some("raw"),
            verification_state: Some("unverified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &[],
            message_refs: &[],
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(server_received_at),
            recorded_at_epoch_ms: None,
            valid_from_epoch_ms: Some(server_received_at),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: None,
            object_version: None,
            causation_id: Some(&event_id_str),
            correlation_id: Some(&event_id_str),
            utility_score: None,
            freshness_score: None,
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: None,
            superseded_by_memory_item_id: None,
            metadata: &payload,
        };

        match create_memory_item(client, project_code, namespace_code, &record).await {
            Ok(item) => {
                info!(raw_event_id=%raw_event_id, memory_item_id=%item.memory_item_id, "auto-extracted memory item");
                result.created += 1;
            }
            Err(error) => {
                let msg = format!("{error:#}");
                if msg.contains("duplicate") || msg.contains("already exists") {
                    result.skipped_already_extracted += 1;
                } else {
                    warn!(raw_event_id=%raw_event_id, error=%msg, "auto-extract failed");
                    result.errors += 1;
                }
            }
        }
    }

    Ok(result)
}
