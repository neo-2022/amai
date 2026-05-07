use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
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

#[derive(Debug, Clone, Serialize)]
pub struct LifecycleTransitionStatsRow {
    pub observed_state: String,
    pub next_state: String,
    pub derivation_kind: String,
    pub retention_class: String,
    pub decay_policy: String,
    pub freshness_band: String,
    pub utility_band: String,
    pub access_band: String,
    pub transition_count: i64,
    pub total_dwell_ms: i64,
    pub avg_dwell_ms: i64,
    pub p50_dwell_ms: i64,
    pub p90_dwell_ms: i64,
    pub last_recorded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LifecycleTransitionStatsReport {
    pub contract_version: String,
    pub project_code: String,
    pub namespace_code: String,
    pub refreshed_materialized_view: bool,
    pub rows: Vec<LifecycleTransitionStatsRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LifecycleCohortRiskRow {
    pub observed_state: String,
    pub derivation_kind: String,
    pub retention_class: String,
    pub decay_policy: String,
    pub freshness_band: String,
    pub utility_band: String,
    pub access_band: String,
    pub sample_size: i64,
    pub expected_next_state: String,
    pub pending_review_risk_7d: f64,
    pub archive_risk_30d: f64,
    pub prune_risk_30d: f64,
    pub expected_residency_ms: i64,
    pub dwell_p50_ms: i64,
    pub dwell_p75_ms: i64,
    pub dwell_p90_ms: i64,
    pub transition_probabilities: BTreeMap<String, f64>,
    pub cohort_reason_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LifecycleCohortRiskReport {
    pub contract_version: String,
    pub project_code: String,
    pub namespace_code: String,
    pub refreshed_transition_stats: bool,
    pub model_kind: String,
    pub smoothing_alpha: f64,
    pub rows: Vec<LifecycleCohortRiskRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LifecyclePolicySimulationRow {
    pub observed_state: String,
    pub derivation_kind: String,
    pub retention_class: String,
    pub decay_policy: String,
    pub freshness_band: String,
    pub utility_band: String,
    pub access_band: String,
    pub sample_size: i64,
    pub expected_next_state: String,
    pub pending_review_risk_7d: f64,
    pub archive_risk_30d: f64,
    pub prune_risk_30d: f64,
    pub expected_residency_ms: i64,
    pub recommended_review_action: String,
    pub urgency: String,
    pub recommendation_reason: String,
    pub blocking_reasons: Vec<String>,
    pub authority_mode: String,
    pub cohort_reason_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LifecyclePolicySimulationReport {
    pub contract_version: String,
    pub project_code: String,
    pub namespace_code: String,
    pub source_model_kind: String,
    pub authority_mode: String,
    pub rows: Vec<LifecyclePolicySimulationRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LifecycleCohortKey {
    observed_state: String,
    derivation_kind: String,
    retention_class: String,
    decay_policy: String,
    freshness_band: String,
    utility_band: String,
    access_band: String,
}

#[derive(Debug, Clone)]
struct LifecycleCohortEvent {
    next_state: String,
    dwell_ms: i64,
}

#[cfg(test)]
fn lifecycle_transition_active_state(freshness_score: f64) -> &'static str {
    if freshness_score < 0.30 {
        "active_stale"
    } else {
        "active_hot"
    }
}

#[cfg(test)]
fn lifecycle_transition_freshness_band(freshness_score: f64) -> &'static str {
    if freshness_score < 0.05 {
        "critical_stale"
    } else if freshness_score < 0.30 {
        "stale"
    } else if freshness_score < 0.70 {
        "warm"
    } else {
        "fresh"
    }
}

#[cfg(test)]
fn lifecycle_transition_utility_band(utility_score: f64) -> &'static str {
    if utility_score < 0.05 {
        "low"
    } else if utility_score < 0.30 {
        "medium"
    } else {
        "high"
    }
}

#[cfg(test)]
fn lifecycle_transition_access_band(access_count: i64) -> &'static str {
    if access_count <= 0 {
        "none"
    } else if access_count < 3 {
        "low"
    } else if access_count < 10 {
        "medium"
    } else {
        "high"
    }
}

fn lifecycle_transition_is_protected(
    derivation_kind: &str,
    retention_class: &str,
    decay_policy: &str,
) -> bool {
    matches!(
        derivation_kind,
        "raw_capture" | "operator_write" | "verified_write_back"
    ) || matches!(retention_class, "durable" | "legal_hold")
        || decay_policy == "retain_forever"
}

#[cfg(test)]
fn lifecycle_transition_is_quarantined(
    item_kind: &str,
    visibility_scope: &str,
    truth_state: &str,
    trust_state: &str,
    verification_state: &str,
    lifecycle_state: &str,
) -> bool {
    visibility_scope == "quarantine"
        || item_kind == "quarantine"
        || truth_state == "quarantined"
        || trust_state == "quarantined"
        || verification_state == "quarantined"
        || lifecycle_state == "quarantined"
}

#[cfg(test)]
fn lifecycle_transition_state_for_active(
    action: Option<&str>,
    freshness_score: f64,
    is_protected: bool,
    is_quarantined: bool,
) -> &'static str {
    if is_quarantined {
        "quarantined"
    } else if is_protected {
        "protected"
    } else if matches!(
        action,
        Some(
            "prune_ttl_expired"
                | "prune_low_utility"
                | "archive_cold_tier"
                | "revalidate_stale"
                | "dedup_compacted"
        )
    ) {
        "active_stale"
    } else {
        lifecycle_transition_active_state(freshness_score)
    }
}

#[cfg(test)]
fn lifecycle_transition_normalize_state(
    raw_state: &str,
    action: Option<&str>,
    freshness_score: f64,
    is_protected: bool,
    is_quarantined: bool,
) -> &'static str {
    match raw_state {
        "pending_review" => "pending_review",
        "compacted" => "compacted",
        "archived" => "archived",
        "pruned" => "pruned",
        "quarantined" => "quarantined",
        "protected" => "protected",
        "active" => lifecycle_transition_state_for_active(
            action,
            freshness_score,
            is_protected,
            is_quarantined,
        ),
        _ => {
            if is_quarantined {
                "quarantined"
            } else if is_protected {
                "protected"
            } else {
                lifecycle_transition_active_state(freshness_score)
            }
        }
    }
}

const LIFECYCLE_QUEUE_TWO_STATES: [&str; 8] = [
    "active_hot",
    "active_stale",
    "pending_review",
    "compacted",
    "archived",
    "pruned",
    "protected",
    "quarantined",
];

fn rounded_ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator <= 0 {
        return 0.0;
    }
    let value = (numerator as f64) / (denominator as f64);
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn rounded_f64(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn percentile_from_sorted(sorted: &[i64], percentile: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn smoothed_transition_probabilities(
    counts_by_state: &HashMap<String, i64>,
    sample_size: i64,
    alpha: f64,
) -> BTreeMap<String, f64> {
    let state_count = LIFECYCLE_QUEUE_TWO_STATES.len() as f64;
    let denominator = sample_size as f64 + alpha * state_count;
    let mut probabilities = BTreeMap::new();
    for state in LIFECYCLE_QUEUE_TWO_STATES {
        let count = *counts_by_state.get(state).unwrap_or(&0) as f64;
        probabilities.insert(
            state.to_string(),
            rounded_f64((count + alpha) / denominator),
        );
    }
    probabilities
}

fn fast_transition_fraction(
    events: &[LifecycleCohortEvent],
    next_state: &str,
    horizon_ms: i64,
) -> f64 {
    let mut total = 0_i64;
    let mut within_horizon = 0_i64;
    for event in events {
        if event.next_state == next_state {
            total += 1;
            if event.dwell_ms <= horizon_ms {
                within_horizon += 1;
            }
        }
    }
    rounded_ratio(within_horizon, total)
}

fn expected_next_state_from_probabilities(probabilities: &BTreeMap<String, f64>) -> String {
    let mut best_state = "active_hot";
    let mut best_probability = f64::MIN;
    for state in LIFECYCLE_QUEUE_TWO_STATES {
        let probability = *probabilities.get(state).unwrap_or(&0.0);
        if probability > best_probability {
            best_probability = probability;
            best_state = state;
        }
    }
    best_state.to_string()
}

fn expected_residency_ms(events: &[LifecycleCohortEvent]) -> i64 {
    if events.is_empty() {
        return 0;
    }
    let total: i64 = events.iter().map(|event| event.dwell_ms).sum();
    ((total as f64) / (events.len() as f64)).round() as i64
}

fn lifecycle_cohort_reason_summary(
    key: &LifecycleCohortKey,
    sample_size: i64,
    expected_next_state: &str,
) -> String {
    format!(
        "observed={} cohort={} / {} / {} / {} / {} / {} sample_size={} expected_next_state={}",
        key.observed_state,
        key.derivation_kind,
        key.retention_class,
        key.decay_policy,
        key.freshness_band,
        key.utility_band,
        key.access_band,
        sample_size,
        expected_next_state
    )
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

pub async fn refresh_lifecycle_transition_stats(client: &Client) -> Result<()> {
    crate::postgres::with_postgres_advisory_lock(
        client,
        crate::postgres::BOOTSTRAP_SCHEMA_ADVISORY_LOCK_KEY,
        "failed to acquire bootstrap/schema advisory lock for lifecycle transition stats refresh",
        "failed to release bootstrap/schema advisory lock after lifecycle transition stats refresh",
        || async {
            client
                .batch_execute("REFRESH MATERIALIZED VIEW ami.lifecycle_transition_stats_v1;")
                .await
                .with_context(
                    || "failed to refresh lifecycle transition stats materialized view",
                )?;
            Ok(())
        },
    )
    .await
}

pub async fn transition_stats(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<LifecycleTransitionStatsReport> {
    refresh_lifecycle_transition_stats(client).await?;
    let rows = client
        .query(
            r#"
            SELECT
                observed_state,
                next_state,
                derivation_kind,
                retention_class,
                decay_policy,
                freshness_band,
                utility_band,
                access_band,
                transition_count,
                total_dwell_ms,
                avg_dwell_ms,
                p50_dwell_ms,
                p90_dwell_ms,
                last_recorded_at::text
            FROM ami.lifecycle_transition_stats_v1
            WHERE project_code = $1
              AND namespace_code = $2
            ORDER BY
                transition_count DESC,
                observed_state,
                next_state,
                derivation_kind,
                retention_class,
                decay_policy
            "#,
            &[&project_code, &namespace_code],
        )
        .await
        .with_context(|| {
            format!(
                "failed to query lifecycle transition stats for {project_code}/{namespace_code}"
            )
        })?;
    Ok(LifecycleTransitionStatsReport {
        contract_version: "lifecycle-transition-stats-v1".to_string(),
        project_code: project_code.to_string(),
        namespace_code: namespace_code.to_string(),
        refreshed_materialized_view: true,
        rows: rows
            .into_iter()
            .map(|row| LifecycleTransitionStatsRow {
                observed_state: row.get(0),
                next_state: row.get(1),
                derivation_kind: row.get(2),
                retention_class: row.get(3),
                decay_policy: row.get(4),
                freshness_band: row.get(5),
                utility_band: row.get(6),
                access_band: row.get(7),
                transition_count: row.get(8),
                total_dwell_ms: row.get(9),
                avg_dwell_ms: row.get(10),
                p50_dwell_ms: row.get(11),
                p90_dwell_ms: row.get(12),
                last_recorded_at: row.get(13),
            })
            .collect(),
    })
}

pub async fn cohort_risk(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<LifecycleCohortRiskReport> {
    refresh_lifecycle_transition_stats(client).await?;
    let rows = client
        .query(
            r#"
            SELECT
                observed_state,
                next_state,
                derivation_kind,
                retention_class,
                decay_policy,
                freshness_band,
                utility_band,
                access_band,
                dwell_ms
            FROM ami.lifecycle_transition_events_v1
            WHERE project_code = $1
              AND namespace_code = $2
            ORDER BY
                observed_state,
                derivation_kind,
                retention_class,
                decay_policy,
                freshness_band,
                utility_band,
                access_band,
                recorded_at,
                memory_item_id
            "#,
            &[&project_code, &namespace_code],
        )
        .await
        .with_context(|| {
            format!("failed to query lifecycle cohort events for {project_code}/{namespace_code}")
        })?;

    let mut grouped_events: HashMap<LifecycleCohortKey, Vec<LifecycleCohortEvent>> = HashMap::new();
    for row in rows {
        let key = LifecycleCohortKey {
            observed_state: row.get(0),
            derivation_kind: row.get(2),
            retention_class: row.get(3),
            decay_policy: row.get(4),
            freshness_band: row.get(5),
            utility_band: row.get(6),
            access_band: row.get(7),
        };
        grouped_events
            .entry(key)
            .or_default()
            .push(LifecycleCohortEvent {
                next_state: row.get(1),
                dwell_ms: row.get(8),
            });
    }

    let mut grouped_rows: Vec<(LifecycleCohortKey, Vec<LifecycleCohortEvent>)> =
        grouped_events.into_iter().collect();
    grouped_rows.sort_by(|(left_key, _), (right_key, _)| {
        (
            left_key.observed_state.as_str(),
            left_key.derivation_kind.as_str(),
            left_key.retention_class.as_str(),
            left_key.decay_policy.as_str(),
            left_key.freshness_band.as_str(),
            left_key.utility_band.as_str(),
            left_key.access_band.as_str(),
        )
            .cmp(&(
                right_key.observed_state.as_str(),
                right_key.derivation_kind.as_str(),
                right_key.retention_class.as_str(),
                right_key.decay_policy.as_str(),
                right_key.freshness_band.as_str(),
                right_key.utility_band.as_str(),
                right_key.access_band.as_str(),
            ))
    });

    let rows = grouped_rows
        .into_iter()
        .map(|(key, events)| {
            let sample_size = events.len() as i64;
            let mut counts_by_state: HashMap<String, i64> = HashMap::new();
            let mut dwell_values: Vec<i64> = events.iter().map(|event| event.dwell_ms).collect();
            for event in &events {
                *counts_by_state.entry(event.next_state.clone()).or_insert(0) += 1;
            }
            dwell_values.sort_unstable();

            let transition_probabilities =
                smoothed_transition_probabilities(&counts_by_state, sample_size, 1.0);
            let expected_next_state =
                expected_next_state_from_probabilities(&transition_probabilities);
            let pending_review_risk_7d = rounded_f64(
                transition_probabilities
                    .get("pending_review")
                    .copied()
                    .unwrap_or(0.0)
                    * fast_transition_fraction(&events, "pending_review", 7 * 24 * 60 * 60 * 1000),
            );
            let archive_risk_30d = rounded_f64(
                transition_probabilities
                    .get("archived")
                    .copied()
                    .unwrap_or(0.0)
                    * fast_transition_fraction(&events, "archived", 30 * 24 * 60 * 60 * 1000),
            );
            let prune_risk_30d = rounded_f64(
                transition_probabilities
                    .get("pruned")
                    .copied()
                    .unwrap_or(0.0)
                    * fast_transition_fraction(&events, "pruned", 30 * 24 * 60 * 60 * 1000),
            );

            LifecycleCohortRiskRow {
                observed_state: key.observed_state.clone(),
                derivation_kind: key.derivation_kind.clone(),
                retention_class: key.retention_class.clone(),
                decay_policy: key.decay_policy.clone(),
                freshness_band: key.freshness_band.clone(),
                utility_band: key.utility_band.clone(),
                access_band: key.access_band.clone(),
                sample_size,
                expected_next_state: expected_next_state.clone(),
                pending_review_risk_7d,
                archive_risk_30d,
                prune_risk_30d,
                expected_residency_ms: expected_residency_ms(&events),
                dwell_p50_ms: percentile_from_sorted(&dwell_values, 0.50),
                dwell_p75_ms: percentile_from_sorted(&dwell_values, 0.75),
                dwell_p90_ms: percentile_from_sorted(&dwell_values, 0.90),
                transition_probabilities,
                cohort_reason_summary: lifecycle_cohort_reason_summary(
                    &key,
                    sample_size,
                    &expected_next_state,
                ),
            }
        })
        .collect();

    Ok(LifecycleCohortRiskReport {
        contract_version: "lifecycle-cohort-risk-v1".to_string(),
        project_code: project_code.to_string(),
        namespace_code: namespace_code.to_string(),
        refreshed_transition_stats: true,
        model_kind: "cohort-separated-empirical-markov-hazard-v1".to_string(),
        smoothing_alpha: 1.0,
        rows,
    })
}

pub async fn policy_simulate(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<LifecyclePolicySimulationReport> {
    let cohort_risk = cohort_risk(client, project_code, namespace_code).await?;
    let rows = cohort_risk
        .rows
        .iter()
        .map(|row| lifecycle_policy_simulation_row(row))
        .collect();
    Ok(LifecyclePolicySimulationReport {
        contract_version: "lifecycle-policy-simulate-v1".to_string(),
        project_code: cohort_risk.project_code,
        namespace_code: cohort_risk.namespace_code,
        source_model_kind: cohort_risk.model_kind,
        authority_mode: "advisory_only_no_runtime_authority".to_string(),
        rows,
    })
}

fn lifecycle_policy_simulation_row(row: &LifecycleCohortRiskRow) -> LifecyclePolicySimulationRow {
    let mut blocking_reasons = vec!["advisory_only_no_runtime_authority".to_string()];
    if lifecycle_transition_is_protected(
        &row.derivation_kind,
        &row.retention_class,
        &row.decay_policy,
    ) {
        blocking_reasons.push("protected_or_retained_cohort".to_string());
    }
    if row.observed_state == "quarantined" {
        blocking_reasons.push("quarantined_requires_manual_truth_review".to_string());
    }

    let (recommended_review_action, urgency, recommendation_reason) = if blocking_reasons.len() > 1
    {
        (
            "hold_current_policy".to_string(),
            "manual_only".to_string(),
            format!(
                "Cohort remains protected from automated lifecycle change: {}",
                row.cohort_reason_summary
            ),
        )
    } else if row.pending_review_risk_7d >= 0.20 || row.expected_next_state == "pending_review" {
        (
            "review_revalidation_queue".to_string(),
            lifecycle_policy_urgency(
                row.pending_review_risk_7d,
                row.expected_residency_ms,
                7 * 24 * 60 * 60 * 1000,
            ),
            format!(
                "Expected next state is pending_review with 7d risk {:.2}% and residency {} ms",
                row.pending_review_risk_7d * 100.0,
                row.expected_residency_ms
            ),
        )
    } else if row.prune_risk_30d >= 0.20 || row.expected_next_state == "pruned" {
        (
            "review_prune_candidate".to_string(),
            lifecycle_policy_urgency(
                row.prune_risk_30d,
                row.expected_residency_ms,
                30 * 24 * 60 * 60 * 1000,
            ),
            format!(
                "Expected next state is pruned with 30d risk {:.2}% and residency {} ms",
                row.prune_risk_30d * 100.0,
                row.expected_residency_ms
            ),
        )
    } else if row.archive_risk_30d >= 0.20 || row.expected_next_state == "archived" {
        (
            "review_archive_candidate".to_string(),
            lifecycle_policy_urgency(
                row.archive_risk_30d,
                row.expected_residency_ms,
                30 * 24 * 60 * 60 * 1000,
            ),
            format!(
                "Expected next state is archived with 30d risk {:.2}% and residency {} ms",
                row.archive_risk_30d * 100.0,
                row.expected_residency_ms
            ),
        )
    } else {
        (
            "observe_only".to_string(),
            "low".to_string(),
            format!(
                "Current cohort remains advisory-only without a strong review trigger: {}",
                row.cohort_reason_summary
            ),
        )
    };

    LifecyclePolicySimulationRow {
        observed_state: row.observed_state.clone(),
        derivation_kind: row.derivation_kind.clone(),
        retention_class: row.retention_class.clone(),
        decay_policy: row.decay_policy.clone(),
        freshness_band: row.freshness_band.clone(),
        utility_band: row.utility_band.clone(),
        access_band: row.access_band.clone(),
        sample_size: row.sample_size,
        expected_next_state: row.expected_next_state.clone(),
        pending_review_risk_7d: row.pending_review_risk_7d,
        archive_risk_30d: row.archive_risk_30d,
        prune_risk_30d: row.prune_risk_30d,
        expected_residency_ms: row.expected_residency_ms,
        recommended_review_action,
        urgency,
        recommendation_reason,
        blocking_reasons,
        authority_mode: "advisory_only_no_runtime_authority".to_string(),
        cohort_reason_summary: row.cohort_reason_summary.clone(),
    }
}

fn lifecycle_policy_urgency(risk: f64, expected_residency_ms: i64, horizon_ms: i64) -> String {
    if risk >= 0.50 || expected_residency_ms <= horizon_ms / 4 {
        "high".to_string()
    } else if risk >= 0.20 || expected_residency_ms <= horizon_ms / 2 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
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

    #[test]
    fn lifecycle_transition_state_prefers_quarantine_and_protection() {
        assert_eq!(
            lifecycle_transition_normalize_state("active", None, 0.9, true, false),
            "protected"
        );
        assert_eq!(
            lifecycle_transition_normalize_state("active", None, 0.9, false, true),
            "quarantined"
        );
        assert_eq!(
            lifecycle_transition_normalize_state(
                "active",
                Some("archive_cold_tier"),
                0.9,
                false,
                false
            ),
            "active_stale"
        );
        assert_eq!(
            lifecycle_transition_normalize_state("active", None, 0.9, false, false),
            "active_hot"
        );
    }

    #[test]
    fn lifecycle_transition_bands_match_queue_two_contract() {
        assert_eq!(lifecycle_transition_freshness_band(0.01), "critical_stale");
        assert_eq!(lifecycle_transition_freshness_band(0.10), "stale");
        assert_eq!(lifecycle_transition_freshness_band(0.50), "warm");
        assert_eq!(lifecycle_transition_freshness_band(0.95), "fresh");

        assert_eq!(lifecycle_transition_utility_band(0.01), "low");
        assert_eq!(lifecycle_transition_utility_band(0.10), "medium");
        assert_eq!(lifecycle_transition_utility_band(0.90), "high");

        assert_eq!(lifecycle_transition_access_band(0), "none");
        assert_eq!(lifecycle_transition_access_band(1), "low");
        assert_eq!(lifecycle_transition_access_band(5), "medium");
        assert_eq!(lifecycle_transition_access_band(15), "high");
    }

    #[test]
    fn lifecycle_transition_guards_match_stage_nine_invariants() {
        assert!(lifecycle_transition_is_protected(
            "verified_write_back",
            "standard",
            "standard"
        ));
        assert!(lifecycle_transition_is_protected(
            "summary",
            "legal_hold",
            "standard"
        ));
        assert!(lifecycle_transition_is_protected(
            "summary",
            "standard",
            "retain_forever"
        ));
        assert!(lifecycle_transition_is_quarantined(
            "fact",
            "quarantine",
            "current",
            "verified",
            "verified",
            "hot"
        ));
    }

    #[test]
    fn lifecycle_cohort_risk_uses_laplace_smoothing_for_all_states() {
        let mut counts = HashMap::new();
        counts.insert("archived".to_string(), 2);
        counts.insert("pending_review".to_string(), 1);

        let probabilities = smoothed_transition_probabilities(&counts, 3, 1.0);

        assert_eq!(probabilities.len(), LIFECYCLE_QUEUE_TWO_STATES.len());
        assert_eq!(probabilities.get("archived"), Some(&0.272727));
        assert_eq!(probabilities.get("pending_review"), Some(&0.181818));
        assert_eq!(probabilities.get("pruned"), Some(&0.090909));
        assert_eq!(
            expected_next_state_from_probabilities(&probabilities),
            "archived".to_string()
        );
    }

    #[test]
    fn lifecycle_cohort_risk_keeps_cohorts_separated() {
        let warm_summary = lifecycle_cohort_reason_summary(
            &LifecycleCohortKey {
                observed_state: "active_stale".to_string(),
                derivation_kind: "summary".to_string(),
                retention_class: "standard".to_string(),
                decay_policy: "default".to_string(),
                freshness_band: "warm".to_string(),
                utility_band: "medium".to_string(),
                access_band: "low".to_string(),
            },
            3,
            "archived",
        );
        let fresh_summary = lifecycle_cohort_reason_summary(
            &LifecycleCohortKey {
                observed_state: "active_stale".to_string(),
                derivation_kind: "summary".to_string(),
                retention_class: "standard".to_string(),
                decay_policy: "default".to_string(),
                freshness_band: "fresh".to_string(),
                utility_band: "medium".to_string(),
                access_band: "low".to_string(),
            },
            3,
            "archived",
        );

        assert_ne!(warm_summary, fresh_summary);
        assert!(warm_summary.contains("warm"));
        assert!(fresh_summary.contains("fresh"));
    }

    #[test]
    fn lifecycle_cohort_risk_horizon_respects_fast_transition_fraction() {
        let events = vec![
            LifecycleCohortEvent {
                next_state: "pruned".to_string(),
                dwell_ms: 1_000,
            },
            LifecycleCohortEvent {
                next_state: "pruned".to_string(),
                dwell_ms: 40 * 24 * 60 * 60 * 1000,
            },
            LifecycleCohortEvent {
                next_state: "archived".to_string(),
                dwell_ms: 10,
            },
        ];

        assert_eq!(
            fast_transition_fraction(&events, "pruned", 30 * 24 * 60 * 60 * 1000),
            0.5
        );
        assert_eq!(
            fast_transition_fraction(&events, "archived", 30 * 24 * 60 * 60 * 1000),
            1.0
        );
        assert_eq!(expected_residency_ms(&events), 1_152_000_337);
        let sorted = vec![1_000, 10_000, 20_000, 90_000];
        assert_eq!(percentile_from_sorted(&sorted, 0.50), 20_000);
        assert_eq!(percentile_from_sorted(&sorted, 0.75), 20_000);
        assert_eq!(percentile_from_sorted(&sorted, 0.90), 90_000);
    }

    #[test]
    fn lifecycle_policy_simulation_stays_advisory_for_protected_cohort() {
        let row = LifecycleCohortRiskRow {
            observed_state: "active_stale".to_string(),
            derivation_kind: "verified_write_back".to_string(),
            retention_class: "standard".to_string(),
            decay_policy: "standard".to_string(),
            freshness_band: "critical_stale".to_string(),
            utility_band: "medium".to_string(),
            access_band: "low".to_string(),
            sample_size: 4,
            expected_next_state: "pending_review".to_string(),
            pending_review_risk_7d: 0.85,
            archive_risk_30d: 0.30,
            prune_risk_30d: 0.10,
            expected_residency_ms: 1_000,
            dwell_p50_ms: 1_000,
            dwell_p75_ms: 1_000,
            dwell_p90_ms: 1_000,
            transition_probabilities: BTreeMap::new(),
            cohort_reason_summary: "protected cohort".to_string(),
        };

        let simulation = lifecycle_policy_simulation_row(&row);
        assert_eq!(simulation.recommended_review_action, "hold_current_policy");
        assert_eq!(simulation.urgency, "manual_only");
        assert!(
            simulation
                .blocking_reasons
                .contains(&"advisory_only_no_runtime_authority".to_string())
        );
        assert!(
            simulation
                .blocking_reasons
                .contains(&"protected_or_retained_cohort".to_string())
        );
    }

    #[test]
    fn lifecycle_policy_simulation_recommends_review_for_pending_review_risk() {
        let row = LifecycleCohortRiskRow {
            observed_state: "active_stale".to_string(),
            derivation_kind: "summary".to_string(),
            retention_class: "standard".to_string(),
            decay_policy: "default".to_string(),
            freshness_band: "stale".to_string(),
            utility_band: "medium".to_string(),
            access_band: "low".to_string(),
            sample_size: 5,
            expected_next_state: "pending_review".to_string(),
            pending_review_risk_7d: 0.42,
            archive_risk_30d: 0.19,
            prune_risk_30d: 0.03,
            expected_residency_ms: 2 * 24 * 60 * 60 * 1000,
            dwell_p50_ms: 1_000,
            dwell_p75_ms: 1_000,
            dwell_p90_ms: 1_000,
            transition_probabilities: BTreeMap::new(),
            cohort_reason_summary: "review candidate".to_string(),
        };

        let simulation = lifecycle_policy_simulation_row(&row);
        assert_eq!(
            simulation.recommended_review_action,
            "review_revalidation_queue"
        );
        assert_eq!(simulation.urgency, "medium");
        assert_eq!(
            simulation.authority_mode,
            "advisory_only_no_runtime_authority"
        );
    }
}
