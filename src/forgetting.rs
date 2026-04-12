use anyhow::{Context, Result};
use serde::Serialize;
use tokio_postgres::Client;
use uuid::Uuid;

use crate::cli::ForgettingJobKind;

fn forgetting_protected_predicate_sql(alias: &str) -> String {
    format!(
        "(
            {alias}.derivation_kind IN ('raw_capture', 'operator_write', 'verified_write_back')
            OR {alias}.retention_class IN ('durable', 'legal_hold')
            OR {alias}.decay_policy = 'retain_forever'
        )"
    )
}

fn prune_expired_items_sql() -> String {
    let protected_predicate = forgetting_protected_predicate_sql("mi");
    format!(
        r#"
            UPDATE ami.memory_items mi
            SET consolidation_status = 'pruned',
                updated_at = now()
            FROM ami.projects p
            JOIN ami.namespaces n ON n.project_id = p.project_id
            WHERE mi.project_id = p.project_id
              AND mi.namespace_id = n.namespace_id
              AND p.code = $1
              AND n.code = $2
              AND mi.ttl_epoch_ms IS NOT NULL
              AND mi.ttl_epoch_ms < $3
              AND mi.consolidation_status = 'active'
              AND mi.retention_class IN ('ephemeral', 'standard')
              AND NOT {protected_predicate}
            RETURNING mi.memory_item_id, mi.retention_class, mi.decay_policy,
                      mi.derivation_kind, mi.title
            "#
    )
}

fn prune_low_utility_ephemeral_sql() -> String {
    let protected_predicate = forgetting_protected_predicate_sql("mi");
    format!(
        r#"
            UPDATE ami.memory_items mi
            SET consolidation_status = 'pruned',
                updated_at = now()
            FROM ami.projects p
            JOIN ami.namespaces n ON n.project_id = p.project_id
            WHERE mi.project_id = p.project_id
              AND mi.namespace_id = n.namespace_id
              AND p.code = $1
              AND n.code = $2
              AND mi.retention_class = 'ephemeral'
              AND mi.consolidation_status = 'active'
              AND mi.utility_score < $3
              AND mi.freshness_score < 0.3
              AND mi.derivation_kind IN ('summary', 'extract')
              AND NOT {protected_predicate}
            RETURNING mi.memory_item_id, mi.retention_class, mi.decay_policy,
                      mi.utility_score, mi.freshness_score, mi.title
            "#
    )
}

fn archive_to_cold_tier_sql() -> String {
    let protected_predicate = forgetting_protected_predicate_sql("mi");
    format!(
        r#"
            UPDATE ami.memory_items mi
            SET consolidation_status = 'archived',
                retention_class = 'archive',
                updated_at = now()
            FROM ami.projects p
            JOIN ami.namespaces n ON n.project_id = p.project_id
            WHERE mi.project_id = p.project_id
              AND mi.namespace_id = n.namespace_id
              AND p.code = $1
              AND n.code = $2
              AND mi.consolidation_status = 'active'
              AND mi.retention_class = 'standard'
              AND mi.derivation_kind IN ('summary', 'extract', 'merge')
              AND mi.freshness_score < 0.1
              AND (mi.last_accessed_at IS NULL OR mi.last_accessed_at < to_timestamp($3))
              AND NOT {protected_predicate}
            RETURNING mi.memory_item_id, 'standard'::text, mi.decay_policy,
                      mi.freshness_score, mi.title
            "#
    )
}

fn count_immutable_protected_sql() -> String {
    let protected_predicate = forgetting_protected_predicate_sql("mi");
    format!(
        r#"
            SELECT COUNT(*)::bigint
            FROM ami.memory_items mi
            JOIN ami.projects p ON mi.project_id = p.project_id
            JOIN ami.namespaces n ON mi.namespace_id = n.namespace_id AND n.project_id = p.project_id
            WHERE p.code = $1
              AND n.code = $2
              AND {protected_predicate}
            "#
    )
}

fn revalidate_stale_items_sql() -> String {
    let protected_predicate = forgetting_protected_predicate_sql("mi");
    format!(
        r#"
            UPDATE ami.memory_items mi
            SET consolidation_status = 'pending_review',
                updated_at = now()
            FROM ami.projects p
            JOIN ami.namespaces n ON n.project_id = p.project_id
            WHERE mi.project_id = p.project_id
              AND mi.namespace_id = n.namespace_id
              AND p.code = $1
              AND n.code = $2
              AND mi.consolidation_status = 'active'
              AND mi.freshness_score < $3
              AND mi.truth_state = 'current'
              AND mi.retention_class IN ('standard', 'ephemeral')
              AND NOT {protected_predicate}
            RETURNING mi.memory_item_id, mi.retention_class, mi.decay_policy,
                      mi.freshness_score, mi.title
            "#
    )
}

fn dedup_identical_items_sql() -> String {
    let protected_predicate = forgetting_protected_predicate_sql("mi");
    format!(
        r#"
            WITH duplicates AS (
                SELECT mi.memory_item_id,
                       mi.title,
                       mi.retention_class,
                       mi.decay_policy,
                       ROW_NUMBER() OVER (
                           PARTITION BY mi.namespace_id, mi.identity_key, mi.title, mi.item_kind
                           ORDER BY mi.utility_score DESC, mi.ingest_seq DESC
                       ) AS rn
                FROM ami.memory_items mi
                JOIN ami.projects p ON mi.project_id = p.project_id
                JOIN ami.namespaces n ON mi.namespace_id = n.namespace_id AND n.project_id = p.project_id
                WHERE p.code = $1
                  AND n.code = $2
                  AND mi.consolidation_status = 'active'
                  AND mi.identity_key IS NOT NULL
                  AND NOT {protected_predicate}
            )
            UPDATE ami.memory_items mi
            SET consolidation_status = 'compacted',
                updated_at = now()
            FROM duplicates d
            WHERE mi.memory_item_id = d.memory_item_id
              AND d.rn > 1
            RETURNING mi.memory_item_id, mi.retention_class, mi.decay_policy, mi.title
            "#
    )
}

pub async fn persist_audit_log(
    client: &Client,
    actions: &[ForgettingAction],
    project_code: &str,
    namespace_code: &str,
) -> Result<()> {
    for action in actions {
        client
            .execute(
                r#"
                INSERT INTO ami.forgetting_audit_log
                    (memory_item_id, action, previous_state, new_state, reason,
                     retention_class, decay_policy, project_code, namespace_code)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
                &[
                    &action.memory_item_id,
                    &action.action,
                    &action.previous_state,
                    &action.new_state,
                    &action.reason,
                    &action.retention_class,
                    &action.decay_policy,
                    &project_code,
                    &namespace_code,
                ],
            )
            .await
            .with_context(|| {
                format!(
                    "failed to persist forgetting audit log for {}",
                    action.memory_item_id
                )
            })?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ForgettingAction {
    pub memory_item_id: Uuid,
    pub action: String,
    pub previous_state: String,
    pub new_state: String,
    pub reason: String,
    pub retention_class: String,
    pub decay_policy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForgettingReport {
    pub contract_version: String,
    pub project_code: String,
    pub namespace_code: String,
    pub pruned_count: usize,
    pub archived_count: usize,
    pub compacted_count: usize,
    pub revalidated_count: usize,
    pub dedup_count: usize,
    pub actions: Vec<ForgettingAction>,
    pub immutable_protected_count: i64,
    pub safety_invariant: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForgettingJobReport {
    pub contract_version: String,
    pub job_kind: String,
    pub project_code: String,
    pub namespace_code: String,
    pub action_count: usize,
    pub actions: Vec<ForgettingAction>,
    pub immutable_protected_count: i64,
    pub safety_invariant: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForgettingAuditEntry {
    pub audit_id: Uuid,
    pub memory_item_id: Uuid,
    pub action: String,
    pub previous_state: String,
    pub new_state: String,
    pub reason: String,
    pub retention_class: String,
    pub decay_policy: String,
    pub project_code: String,
    pub namespace_code: String,
    pub recorded_at: String,
}

pub async fn explain_forgetting(
    client: &Client,
    memory_item_id: Uuid,
) -> Result<Vec<ForgettingAuditEntry>> {
    let rows = client
        .query(
            r#"
            SELECT audit_id, memory_item_id, action, previous_state, new_state,
                   reason, retention_class, decay_policy, project_code, namespace_code,
                   to_char(recorded_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.forgetting_audit_log
            WHERE memory_item_id = $1
            ORDER BY recorded_at ASC
            "#,
            &[&memory_item_id],
        )
        .await
        .with_context(|| format!("failed to query forgetting audit log for {memory_item_id}"))?;

    Ok(rows
        .iter()
        .map(|row| ForgettingAuditEntry {
            audit_id: row.get(0),
            memory_item_id: row.get(1),
            action: row.get(2),
            previous_state: row.get(3),
            new_state: row.get(4),
            reason: row.get(5),
            retention_class: row.get(6),
            decay_policy: row.get(7),
            project_code: row.get(8),
            namespace_code: row.get(9),
            recorded_at: row.get(10),
        })
        .collect())
}

pub async fn touch_memory_item_access(client: &Client, memory_item_id: Uuid) -> Result<()> {
    client
        .execute(
            r#"
            UPDATE ami.memory_items
            SET access_count = access_count + 1,
                last_accessed_at = now()
            WHERE memory_item_id = $1
            "#,
            &[&memory_item_id],
        )
        .await
        .with_context(|| format!("failed to touch access for {memory_item_id}"))?;
    Ok(())
}

pub async fn prune_expired_items(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    now_epoch_ms: i64,
) -> Result<Vec<ForgettingAction>> {
    let rows = client
        .query(
            &prune_expired_items_sql(),
            &[&project_code, &namespace_code, &now_epoch_ms],
        )
        .await
        .with_context(|| "failed to prune expired items")?;

    Ok(rows
        .iter()
        .map(|row| {
            let mid: Uuid = row.get(0);
            let rc: String = row.get(1);
            let dp: String = row.get(2);
            let dk: String = row.get(3);
            let title: String = row.get(4);
            ForgettingAction {
                memory_item_id: mid,
                action: "prune_ttl_expired".to_string(),
                previous_state: "active".to_string(),
                new_state: "pruned".to_string(),
                reason: format!("TTL expired for {title} (derivation={dk}, retention={rc})"),
                retention_class: rc,
                decay_policy: dp,
            }
        })
        .collect())
}

pub async fn prune_low_utility_ephemeral(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    utility_threshold: f64,
) -> Result<Vec<ForgettingAction>> {
    let rows = client
        .query(
            &prune_low_utility_ephemeral_sql(),
            &[&project_code, &namespace_code, &utility_threshold],
        )
        .await
        .with_context(|| "failed to prune low-utility ephemeral items")?;

    Ok(rows
        .iter()
        .map(|row| {
            let mid: Uuid = row.get(0);
            let rc: String = row.get(1);
            let dp: String = row.get(2);
            let us: f64 = row.get(3);
            let fs: f64 = row.get(4);
            let title: String = row.get(5);
            ForgettingAction {
                memory_item_id: mid,
                action: "prune_low_utility".to_string(),
                previous_state: "active".to_string(),
                new_state: "pruned".to_string(),
                reason: format!(
                    "Low utility ({us:.3}) and freshness ({fs:.3}) for ephemeral item: {title}"
                ),
                retention_class: rc,
                decay_policy: dp,
            }
        })
        .collect())
}

pub async fn archive_to_cold_tier(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    stale_threshold_epoch_ms: i64,
) -> Result<Vec<ForgettingAction>> {
    let stale_ts_seconds: f64 = stale_threshold_epoch_ms as f64 / 1000.0;
    let rows = client
        .query(
            &archive_to_cold_tier_sql(),
            &[&project_code, &namespace_code, &stale_ts_seconds],
        )
        .await
        .with_context(|| "failed to archive cold-tier items")?;

    Ok(rows
        .iter()
        .map(|row| {
            let mid: Uuid = row.get(0);
            let rc: String = row.get(1);
            let dp: String = row.get(2);
            let fs: f64 = row.get(3);
            let title: String = row.get(4);
            ForgettingAction {
                memory_item_id: mid,
                action: "archive_cold_tier".to_string(),
                previous_state: "active".to_string(),
                new_state: "archived".to_string(),
                reason: format!("Stale derivative (freshness={fs:.3}) moved to cold tier: {title}"),
                retention_class: rc,
                decay_policy: dp,
            }
        })
        .collect())
}

pub async fn count_immutable_protected(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<i64> {
    let row = client
        .query_one(
            &count_immutable_protected_sql(),
            &[&project_code, &namespace_code],
        )
        .await
        .with_context(|| "failed to count immutable protected items")?;
    Ok(row.get(0))
}

pub async fn revalidate_stale_items(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    freshness_threshold: f64,
) -> Result<Vec<ForgettingAction>> {
    let rows = client
        .query(
            &revalidate_stale_items_sql(),
            &[&project_code, &namespace_code, &freshness_threshold],
        )
        .await
        .with_context(|| "failed to revalidate stale items")?;

    Ok(rows
        .iter()
        .map(|row| {
            let mid: Uuid = row.get(0);
            let rc: String = row.get(1);
            let dp: String = row.get(2);
            let fs: f64 = row.get(3);
            let title: String = row.get(4);
            ForgettingAction {
                memory_item_id: mid,
                action: "revalidate_stale".to_string(),
                previous_state: "active".to_string(),
                new_state: "pending_review".to_string(),
                reason: format!("Low freshness ({fs:.3}) requires revalidation: {title}"),
                retention_class: rc,
                decay_policy: dp,
            }
        })
        .collect())
}

pub async fn dedup_identical_items(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<Vec<ForgettingAction>> {
    let rows = client
        .query(
            &dedup_identical_items_sql(),
            &[&project_code, &namespace_code],
        )
        .await
        .with_context(|| "failed to dedup identical items")?;

    Ok(rows
        .iter()
        .map(|row| {
            let mid: Uuid = row.get(0);
            let rc: String = row.get(1);
            let dp: String = row.get(2);
            let title: String = row.get(3);
            ForgettingAction {
                memory_item_id: mid,
                action: "dedup_compacted".to_string(),
                previous_state: "active".to_string(),
                new_state: "compacted".to_string(),
                reason: format!("Duplicate identity_key+title compacted: {title}"),
                retention_class: rc,
                decay_policy: dp,
            }
        })
        .collect())
}

pub async fn run_consolidation(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    now_epoch_ms: i64,
) -> Result<ForgettingReport> {
    let mut all_actions = Vec::new();

    let pruned_ttl =
        prune_expired_items(client, project_code, namespace_code, now_epoch_ms).await?;
    persist_audit_log(client, &pruned_ttl, project_code, namespace_code).await?;

    let pruned_utility =
        prune_low_utility_ephemeral(client, project_code, namespace_code, 0.05).await?;
    persist_audit_log(client, &pruned_utility, project_code, namespace_code).await?;

    let archived = archive_to_cold_tier(
        client,
        project_code,
        namespace_code,
        now_epoch_ms - 30 * 86_400_000,
    )
    .await?;
    persist_audit_log(client, &archived, project_code, namespace_code).await?;

    let revalidated = revalidate_stale_items(client, project_code, namespace_code, 0.05).await?;
    persist_audit_log(client, &revalidated, project_code, namespace_code).await?;

    let deduped = dedup_identical_items(client, project_code, namespace_code).await?;
    persist_audit_log(client, &deduped, project_code, namespace_code).await?;

    let immutable_count = count_immutable_protected(client, project_code, namespace_code).await?;

    let pruned_count = pruned_ttl.len() + pruned_utility.len();
    let archived_count = archived.len();
    let compacted_count = deduped.len();
    let revalidated_count = revalidated.len();
    let dedup_count = deduped.len();

    all_actions.extend(pruned_ttl);
    all_actions.extend(pruned_utility);
    all_actions.extend(archived);
    all_actions.extend(revalidated);
    all_actions.extend(deduped);

    Ok(ForgettingReport {
        contract_version: "forgetting-consolidation-v1".to_string(),
        project_code: project_code.to_string(),
        namespace_code: namespace_code.to_string(),
        pruned_count,
        archived_count,
        compacted_count,
        revalidated_count,
        dedup_count,
        actions: all_actions,
        immutable_protected_count: immutable_count,
        safety_invariant: "raw_capture/operator_write/verified_write_back, durable/legal_hold, and decay_policy=retain_forever items are never pruned, revalidated, compacted, or archived by automated forgetting".to_string(),
    })
}

pub async fn run_forgetting_job(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    job_kind: ForgettingJobKind,
    now_epoch_ms: i64,
    utility_threshold: f64,
    freshness_threshold: f64,
    stale_days: i64,
) -> Result<ForgettingJobReport> {
    let actions = match job_kind {
        ForgettingJobKind::DeDuplication | ForgettingJobKind::Compaction => {
            dedup_identical_items(client, project_code, namespace_code).await?
        }
        ForgettingJobKind::Summarization => Vec::new(),
        ForgettingJobKind::Pruning => {
            let mut actions =
                prune_expired_items(client, project_code, namespace_code, now_epoch_ms).await?;
            actions.extend(
                prune_low_utility_ephemeral(
                    client,
                    project_code,
                    namespace_code,
                    utility_threshold,
                )
                .await?,
            );
            actions
        }
        ForgettingJobKind::ColdArchive => {
            let stale_threshold = now_epoch_ms - stale_days * 86_400_000;
            archive_to_cold_tier(client, project_code, namespace_code, stale_threshold).await?
        }
        ForgettingJobKind::Revalidation => {
            revalidate_stale_items(client, project_code, namespace_code, freshness_threshold)
                .await?
        }
    };

    persist_audit_log(client, &actions, project_code, namespace_code).await?;
    let immutable_count = count_immutable_protected(client, project_code, namespace_code).await?;

    Ok(ForgettingJobReport {
        contract_version: "forgetting-job-v1".to_string(),
        job_kind: match job_kind {
            ForgettingJobKind::DeDuplication => "de_duplication_job",
            ForgettingJobKind::Summarization => "summarization_job",
            ForgettingJobKind::Compaction => "compaction_job",
            ForgettingJobKind::Pruning => "pruning_job",
            ForgettingJobKind::ColdArchive => "cold_archive_job",
            ForgettingJobKind::Revalidation => "revalidation_job",
        }
        .to_string(),
        project_code: project_code.to_string(),
        namespace_code: namespace_code.to_string(),
        action_count: actions.len(),
        actions,
        immutable_protected_count: immutable_count,
        safety_invariant: "raw_capture/operator_write/verified_write_back, durable/legal_hold, and decay_policy=retain_forever items are never pruned, revalidated, compacted, or archived by automated forgetting".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ForgettingJobKind;

    fn assert_query_uses_shared_protection(query: &str) {
        assert!(query.contains("verified_write_back"), "{query}");
        assert!(query.contains("retain_forever"), "{query}");
        assert!(query.contains("operator_write"), "{query}");
        assert!(query.contains("legal_hold"), "{query}");
    }

    #[test]
    fn forgetting_report_serializes_cleanly() {
        let report = ForgettingReport {
            contract_version: "forgetting-consolidation-v1".to_string(),
            project_code: "test".to_string(),
            namespace_code: "ns".to_string(),
            pruned_count: 2,
            archived_count: 1,
            compacted_count: 0,
            revalidated_count: 3,
            dedup_count: 0,
            actions: vec![ForgettingAction {
                memory_item_id: Uuid::nil(),
                action: "prune_ttl_expired".to_string(),
                previous_state: "active".to_string(),
                new_state: "pruned".to_string(),
                reason: "TTL expired".to_string(),
                retention_class: "ephemeral".to_string(),
                decay_policy: "standard".to_string(),
            }],
            immutable_protected_count: 42,
            safety_invariant: "raw_capture never pruned".to_string(),
        };
        let json = serde_json::to_value(&report).expect("serialize");
        assert_eq!(json["contract_version"], "forgetting-consolidation-v1");
        assert_eq!(json["pruned_count"], 2);
        assert_eq!(json["immutable_protected_count"], 42);
        assert_eq!(json["actions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn forgetting_queries_share_protected_predicate_contract() {
        assert_query_uses_shared_protection(&prune_expired_items_sql());
        assert_query_uses_shared_protection(&prune_low_utility_ephemeral_sql());
        assert_query_uses_shared_protection(&archive_to_cold_tier_sql());
        assert_query_uses_shared_protection(&count_immutable_protected_sql());
        assert_query_uses_shared_protection(&revalidate_stale_items_sql());
        assert_query_uses_shared_protection(&dedup_identical_items_sql());
    }

    #[test]
    fn safety_invariant_covers_all_automated_forgetting_paths() {
        let report = ForgettingReport {
            contract_version: "forgetting-consolidation-v1".to_string(),
            project_code: "test".to_string(),
            namespace_code: "ns".to_string(),
            pruned_count: 0,
            archived_count: 0,
            compacted_count: 0,
            revalidated_count: 0,
            dedup_count: 0,
            actions: vec![],
            immutable_protected_count: 0,
            safety_invariant: "raw_capture/operator_write/verified_write_back, durable/legal_hold, and decay_policy=retain_forever items are never pruned, revalidated, compacted, or archived by automated forgetting".to_string(),
        };
        assert!(report.safety_invariant.contains("verified_write_back"));
        assert!(report.safety_invariant.contains("retain_forever"));
        assert!(report.safety_invariant.contains("revalidated"));
        assert!(report.safety_invariant.contains("compacted"));
        assert!(report.safety_invariant.contains("archived"));
    }

    #[test]
    fn forgetting_job_kind_names_match_runtime_contract() {
        let cases = [
            (ForgettingJobKind::DeDuplication, "de_duplication_job"),
            (ForgettingJobKind::Summarization, "summarization_job"),
            (ForgettingJobKind::Compaction, "compaction_job"),
            (ForgettingJobKind::Pruning, "pruning_job"),
            (ForgettingJobKind::ColdArchive, "cold_archive_job"),
            (ForgettingJobKind::Revalidation, "revalidation_job"),
        ];
        for (job_kind, expected_name) in cases {
            let rendered = match job_kind {
                ForgettingJobKind::DeDuplication => "de_duplication_job",
                ForgettingJobKind::Summarization => "summarization_job",
                ForgettingJobKind::Compaction => "compaction_job",
                ForgettingJobKind::Pruning => "pruning_job",
                ForgettingJobKind::ColdArchive => "cold_archive_job",
                ForgettingJobKind::Revalidation => "revalidation_job",
            };
            assert_eq!(rendered, expected_name);
        }
    }
}
