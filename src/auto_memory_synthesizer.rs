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
const ADVISORY_LOCK_MAGIC_INT: i64 = 0x5caff00111;

fn advisory_lock_key(project_code: &str, namespace_code: &str) -> i64 {
    // ponytail: stable FNV-1a 64-bit hash; DefaultHasher is seeded per-process.
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };
    mix(b"amai:auto_extract:");
    mix(project_code.as_bytes());
    mix(b":");
    mix(namespace_code.as_bytes());
    hash as i64
}
/// Scan raw memory events and promote unprocessed ones to memory_items.
///
/// ponytail: reuse existing create_memory_item pipeline; no new tables, no LLM, no manual tags.
/// Concurrent extractors for the same scope are serialized via a Postgres advisory lock
/// to avoid duplicate memory_items under high load.
pub async fn run_auto_extract(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    limit: i64,
) -> Result<AutoExtractBatchResult> {
    let lock_key = advisory_lock_key(project_code, namespace_code);
    let got_lock: bool = client
        .query_one("SELECT pg_try_advisory_lock($1)", &[&lock_key])
        .await
        .context("failed to acquire auto-extract advisory lock")?
        .get(0);
    if !got_lock {
        info!(
            project_code,
            namespace_code, "auto-extract skipped: another extractor is running"
        );
        return Ok(AutoExtractBatchResult {
            scanned: 0,
            created: 0,
            skipped_already_extracted: 0,
            errors: 0,
        });
    }

    let result = run_auto_extract_locked(client, project_code, namespace_code, limit).await;

    let _ = client
        .query_one("SELECT pg_advisory_unlock($1)", &[&lock_key])
        .await;

    result
}

async fn run_auto_extract_locked(
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

        // ponytail: preserve raw event text; no trigger heuristics because upstream already
        // filtered to memory_candidate_write/raw_fact.
        let composed_body = body.unwrap_or_else(|| summary.clone().unwrap_or_default());
        let summary_text = summary.as_deref().unwrap_or(&title);

        let record = MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: None,
            title: &title,
            summary: Some(summary_text),
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
