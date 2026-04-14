use super::benchmark_payload_contaminated;
use crate::artifact_cleanup;
use crate::config::{AppConfig, discover_repo_root};
use crate::{observability_policy, postgres};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;

pub(super) async fn latest_clean_benchmark_snapshot(
    db: &tokio_postgres::Client,
    snapshot_kind: &str,
    expected_root: &str,
) -> Result<(Option<Value>, Option<Value>)> {
    let latest_raw = postgres::latest_observability_snapshot(db, snapshot_kind).await?;
    let latest_clean =
        postgres::latest_clean_benchmark_snapshot_payload(db, snapshot_kind, expected_root).await?;
    Ok((latest_clean, latest_raw))
}

pub(super) async fn latest_dashboard_cold_benchmark_snapshot(
    db: &tokio_postgres::Client,
) -> Result<Option<Value>> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["cold_path_benchmark"], Some(64))
            .await?;
    let payloads: Vec<Value> = rows.into_iter().map(|row| row.payload).collect();
    Ok(select_latest_dashboard_cold_benchmark_snapshot(&payloads))
}

#[cfg(test)]
pub(super) fn select_latest_clean_benchmark_snapshot(
    payloads: &[Value],
    expected_root: &str,
) -> Option<Value> {
    payloads
        .iter()
        .find(|payload| benchmark_payload_contaminated(payload, expected_root) == Some(false))
        .cloned()
}

pub(super) fn select_latest_dashboard_cold_benchmark_snapshot(payloads: &[Value]) -> Option<Value> {
    payloads
        .iter()
        .find(|payload| {
            benchmark_payload_contaminated(payload, "cold_benchmark") == Some(false)
                && cold_benchmark_dashboard_scope(payload) == Some("canonical")
        })
        .cloned()
        .or_else(|| {
            payloads
                .iter()
                .find(|payload| {
                    benchmark_payload_contaminated(payload, "cold_benchmark") == Some(false)
                })
                .cloned()
        })
}

fn cold_benchmark_dashboard_scope(payload: &Value) -> Option<&str> {
    let root = payload.get("cold_benchmark")?;
    if let Some(scope) = root["dashboard_scope"]["class"].as_str() {
        return Some(scope);
    }
    let profile_name = root["profile"]["display_name"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if profile_name.contains("proof") {
        return Some("proof");
    }
    let sample_count = root["machine_readable_summary"]["sample_count"].as_u64()?;
    let repo_count = root["machine_readable_summary"]["repo_count"].as_u64()?;
    let query_slice_count = root["machine_readable_summary"]["query_slice_count"].as_u64()?;
    let min_sample_count = root["profile"]["min_sample_count"].as_u64()?;
    let min_repo_count = root["profile"]["min_repo_count"].as_u64()?;
    let min_query_slice_count = root["profile"]["min_query_slice_count"].as_u64()?;
    if sample_count >= min_sample_count
        && repo_count >= min_repo_count
        && query_slice_count >= min_query_slice_count
    {
        Some("canonical")
    } else {
        Some("smoke")
    }
}

pub(super) async fn maybe_cleanup_observability_snapshots(cfg: &AppConfig) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    maybe_cleanup_observability_snapshots_with_db(&db).await
}

pub(super) async fn maybe_cleanup_observability_snapshots_with_db(db: &Client) -> Result<()> {
    let summary = run_retention_cleanup_with_db(db, true, Some(2048)).await?;
    let cleanup = &summary["observability_retention_cleanup"];
    let deleted = cleanup["deleted"].as_u64().unwrap_or(0);
    let expired = cleanup["expired"].as_u64().unwrap_or(0);
    if deleted > 0 || expired > 0 {
        println!(
            "Amai observability retention cleanup: deleted={}, expired={}, scanned={}",
            deleted,
            expired,
            cleanup["scanned"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
}

pub(super) async fn maybe_cleanup_local_artifacts() -> Result<()> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms = current_epoch_ms_u64();
    let min_interval_ms = artifact_cleanup::sweep_interval()?
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    if artifact_cleanup::read_latest_summary(&repo_root)?
        .filter(|summary| artifact_cleanup_summary_is_fresh(summary, now_epoch_ms, min_interval_ms))
        .is_some()
    {
        return Ok(());
    }
    let summary = collect_artifact_cleanup_summary(&repo_root, true, true, None, false, None)?;
    let _ = artifact_cleanup::write_latest_summary(&repo_root, &summary)?;
    let cleanup = &summary["artifact_cleanup"];
    let deleted = cleanup["deleted"].as_u64().unwrap_or(0);
    let reclaimed_bytes = cleanup["reclaimed_bytes"].as_u64().unwrap_or(0);
    if deleted > 0 || reclaimed_bytes > 0 {
        eprintln!(
            "Amai artifact cleanup: deleted={}, expired={}, reclaimed_bytes={}",
            deleted,
            cleanup["expired"].as_u64().unwrap_or(0),
            reclaimed_bytes
        );
    }
    Ok(())
}

fn artifact_cleanup_summary_captured_at_epoch_ms(summary: &Value) -> Option<u64> {
    summary
        .get("artifact_cleanup")?
        .get("captured_at_epoch_ms")?
        .as_u64()
}

pub(super) fn artifact_cleanup_summary_is_fresh(
    summary: &Value,
    now_epoch_ms: u64,
    min_interval_ms: u64,
) -> bool {
    artifact_cleanup_summary_captured_at_epoch_ms(summary).is_some_and(|captured_at_epoch_ms| {
        now_epoch_ms.saturating_sub(captured_at_epoch_ms) <= min_interval_ms
    })
}

pub(super) fn collect_artifact_cleanup_summary(
    repo_root: &Path,
    apply: bool,
    auto_only: bool,
    limit: Option<usize>,
    aggressive: bool,
    target: Option<&str>,
) -> Result<Value> {
    let existing_last_apply = artifact_cleanup::read_latest_summary(repo_root)?
        .and_then(|summary| extract_last_artifact_cleanup_apply(&summary));
    if !apply {
        let mut current =
            artifact_cleanup::run_cleanup(repo_root, false, auto_only, limit, aggressive, target)?;
        if let Some(last_apply) = existing_last_apply {
            if let Some(object) = current["artifact_cleanup"].as_object_mut() {
                object.insert("last_apply".to_string(), last_apply);
            }
        }
        return Ok(current);
    }

    let applied =
        artifact_cleanup::run_cleanup(repo_root, true, auto_only, limit, aggressive, target)?;
    let mut current =
        artifact_cleanup::run_cleanup(repo_root, false, auto_only, None, false, target)?;
    let applied_cleanup = &applied["artifact_cleanup"];
    let last_apply = if applied_cleanup["reclaimed_bytes"].as_u64().unwrap_or(0) > 0
        || applied_cleanup["deleted"].as_u64().unwrap_or(0) > 0
    {
        json!({
            "captured_at_epoch_ms": applied_cleanup["captured_at_epoch_ms"].clone(),
            "mode": applied_cleanup["mode"].clone(),
            "auto_only": applied_cleanup["auto_only"].clone(),
            "deleted": applied_cleanup["deleted"].clone(),
            "reclaimed_bytes": applied_cleanup["reclaimed_bytes"].clone(),
            "selected": applied_cleanup["selected"].clone(),
        })
    } else {
        existing_last_apply.unwrap_or(Value::Null)
    };
    if let Some(object) = current["artifact_cleanup"].as_object_mut() {
        if !last_apply.is_null() {
            object.insert("last_apply".to_string(), last_apply);
        }
    }
    Ok(current)
}

fn extract_last_artifact_cleanup_apply(summary: &Value) -> Option<Value> {
    let cleanup = summary.get("artifact_cleanup")?;
    if let Some(last_apply) = cleanup.get("last_apply").filter(|value| value.is_object()) {
        return Some(last_apply.clone());
    }
    if cleanup.get("apply").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let reclaimed_bytes = cleanup
        .get("reclaimed_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let deleted = cleanup.get("deleted").and_then(Value::as_u64).unwrap_or(0);
    if reclaimed_bytes == 0 && deleted == 0 {
        return None;
    }
    Some(json!({
        "captured_at_epoch_ms": cleanup["captured_at_epoch_ms"].clone(),
        "mode": cleanup["mode"].clone(),
        "auto_only": cleanup["auto_only"].clone(),
        "deleted": cleanup["deleted"].clone(),
        "reclaimed_bytes": cleanup["reclaimed_bytes"].clone(),
        "selected": cleanup["selected"].clone(),
    }))
}

pub(super) async fn run_retention_cleanup(
    cfg: &AppConfig,
    apply: bool,
    limit: Option<i64>,
) -> Result<Value> {
    let db = postgres::connect_admin(cfg).await?;
    run_retention_cleanup_with_db(&db, apply, limit).await
}

pub(super) async fn run_retention_cleanup_with_db(
    db: &Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<Value> {
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let minimum_retention_hours = observability_policy::minimum_retention_hours()?.unwrap_or(24);
    let cutoff_epoch_ms = now_epoch_ms.saturating_sub(minimum_retention_hours * 3_600_000) as i64;
    let candidates =
        postgres::list_observability_snapshots_older_than(db, cutoff_epoch_ms, limit).await?;
    let expired = expired_retention_candidates(&candidates, now_epoch_ms)?;
    let expired_snapshot_ids: Vec<_> = expired
        .iter()
        .filter_map(|candidate| candidate["snapshot_id"].as_str())
        .filter_map(|snapshot_id| uuid::Uuid::parse_str(snapshot_id).ok())
        .collect();
    let deleted = if apply {
        postgres::delete_observability_snapshots_by_ids(db, &expired_snapshot_ids).await?
    } else {
        0
    };
    Ok(json!({
        "observability_retention_cleanup": {
            "apply": apply,
            "minimum_retention_hours": minimum_retention_hours,
            "cutoff_epoch_ms": cutoff_epoch_ms,
            "scanned": candidates.len(),
            "expired": expired.len(),
            "deleted": deleted,
            "candidates": expired,
        }
    }))
}

pub(super) fn expired_retention_candidates(
    candidates: &[postgres::ObservabilityRetentionCandidate],
    now_epoch_ms: u64,
) -> Result<Vec<Value>> {
    let mut expired = Vec::new();
    for candidate in candidates {
        let rule = observability_policy::retention_rule(
            &candidate.snapshot_kind,
            &candidate.payload,
            &candidate.source_kind,
            &candidate.source_class,
        )?;
        let Some(ttl_hours) = rule.ttl_hours else {
            continue;
        };
        let basis_epoch_ms = candidate
            .captured_at_epoch_ms
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or_else(|| candidate.created_at_epoch_ms.max(0) as u64);
        let age_ms = now_epoch_ms.saturating_sub(basis_epoch_ms);
        let ttl_ms = ttl_hours.saturating_mul(3_600_000);
        if age_ms < ttl_ms {
            continue;
        }
        expired.push(json!({
            "snapshot_id": candidate.snapshot_id.to_string(),
            "snapshot_kind": candidate.snapshot_kind,
            "source_kind": candidate.source_kind,
            "source_class": candidate.source_class,
            "retention_class": rule.retention_class,
            "retention_ttl_hours": ttl_hours,
            "immutable_snapshot": rule.immutable_snapshot,
            "age_hours": age_ms as f64 / 3_600_000.0,
            "created_at_epoch_ms": candidate.created_at_epoch_ms,
            "captured_at_epoch_ms": candidate.captured_at_epoch_ms,
        }));
    }
    Ok(expired)
}

fn current_epoch_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
