use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use super::{
    ACTIVE_THREAD_HINT_MAX_AGE_MS, ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH,
    ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION, CLIENT_BUDGET_GATE_SHARED_CACHE_RELATIVE_PATH,
    CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION, CLIENT_BUDGET_SURFACES_SHARED_CACHE_RELATIVE_PATH,
    CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS, CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION,
    COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS, PersistedActiveThreadHint,
    PersistedClientBudgetGateCache, PersistedClientBudgetSurfacesCache,
    PersistedThreadBoundBudgetSnapshotCache, PersistedThreadBoundSnapshotInvalidation,
    THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION,
    THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION, current_epoch_ms_u64,
};

pub(super) fn observe_cache_thread_suffix(thread_id: &str) -> String {
    thread_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(super) fn client_budget_surfaces_shared_cache_path(
    repo_root: &Path,
    thread_id: Option<&str>,
) -> PathBuf {
    if let Some(thread_id) = thread_id.map(str::trim).filter(|value| !value.is_empty()) {
        return repo_root.join(format!(
            "state/observe/client_budget_surfaces_cache.thread-{}.json",
            observe_cache_thread_suffix(thread_id)
        ));
    }
    repo_root.join(CLIENT_BUDGET_SURFACES_SHARED_CACHE_RELATIVE_PATH)
}

pub(super) fn build_compact_client_budget_surfaces_cache(
    root_cause_payload: &Value,
    gate_payload: &Value,
    guard_payload: &Value,
    thread_id: Option<&str>,
) -> PersistedClientBudgetSurfacesCache {
    PersistedClientBudgetSurfacesCache {
        cache_version: CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.map(str::to_string),
        root_cause: root_cause_payload.clone(),
        gate: gate_payload.clone(),
        guard: guard_payload.clone(),
    }
}

pub(super) fn client_budget_gate_shared_cache_path(
    repo_root: &Path,
    thread_id: Option<&str>,
) -> PathBuf {
    if let Some(thread_id) = thread_id.map(str::trim).filter(|value| !value.is_empty()) {
        return repo_root.join(format!(
            "state/observe/client_budget_gate_cache.thread-{}.json",
            observe_cache_thread_suffix(thread_id)
        ));
    }
    repo_root.join(CLIENT_BUDGET_GATE_SHARED_CACHE_RELATIVE_PATH)
}

pub(super) fn active_thread_hint_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH)
}

pub(super) fn thread_bound_snapshot_invalidation_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_snapshot_invalidation.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

pub(super) fn thread_bound_budget_snapshot_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_budget_snapshot.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

pub(super) fn load_shared_active_thread_hint(
    repo_root: &Path,
    now_epoch_ms: u64,
) -> Option<String> {
    let path = active_thread_hint_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedActiveThreadHint = serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION {
        return None;
    }
    let thread_id = persisted.thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    if now_epoch_ms.saturating_sub(persisted.updated_at_epoch_ms) > ACTIVE_THREAD_HINT_MAX_AGE_MS {
        return None;
    }
    Some(thread_id.to_string())
}

pub(super) fn write_shared_active_thread_hint(repo_root: &Path, thread_id: &str) -> Result<()> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok(());
    }
    let path = active_thread_hint_shared_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedActiveThreadHint {
        cache_version: ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION.to_string(),
        updated_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.to_string(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn load_shared_thread_bound_snapshot_invalidation(
    repo_root: &Path,
    thread_id: &str,
) -> Option<u64> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    let path = thread_bound_snapshot_invalidation_shared_cache_path(repo_root, thread_id);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedThreadBoundSnapshotInvalidation =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION {
        return None;
    }
    if persisted.thread_id.trim() != thread_id {
        return None;
    }
    Some(persisted.invalidated_at_epoch_ms)
}

pub(super) fn write_shared_thread_bound_snapshot_invalidation(
    repo_root: &Path,
    thread_id: &str,
) -> Result<()> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok(());
    }
    let path = thread_bound_snapshot_invalidation_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedThreadBoundSnapshotInvalidation {
        cache_version: THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION.to_string(),
        invalidated_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.to_string(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn load_shared_thread_bound_budget_snapshot(
    repo_root: &Path,
    now_epoch_ms: u64,
    thread_id: &str,
) -> Option<Value> {
    load_shared_thread_bound_budget_snapshot_with_surface_check(
        repo_root,
        now_epoch_ms,
        thread_id,
        thread_bound_budget_snapshot_has_fresh_exact_limit_surfaces,
    )
}

pub(super) fn load_shared_thread_bound_budget_snapshot_preview(
    repo_root: &Path,
    now_epoch_ms: u64,
    thread_id: &str,
) -> Option<Value> {
    load_shared_thread_bound_budget_snapshot_with_surface_check(
        repo_root,
        now_epoch_ms,
        thread_id,
        thread_bound_budget_snapshot_has_fresh_budget_preview_surfaces,
    )
}

fn load_shared_thread_bound_budget_snapshot_with_surface_check(
    repo_root: &Path,
    now_epoch_ms: u64,
    thread_id: &str,
    surface_check: fn(&Value, u64) -> bool,
) -> Option<Value> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    let path = thread_bound_budget_snapshot_shared_cache_path(repo_root, thread_id);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedThreadBoundBudgetSnapshotCache = serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION {
        return None;
    }
    if persisted.thread_id.trim() != thread_id {
        return None;
    }
    if now_epoch_ms.saturating_sub(persisted.fetched_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return None;
    }
    if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id).is_some_and(
        |invalidated_at_epoch_ms| invalidated_at_epoch_ms >= persisted.fetched_at_epoch_ms,
    ) {
        return None;
    }
    if !surface_check(&persisted.snapshot, now_epoch_ms) {
        return None;
    }
    Some(persisted.snapshot)
}

pub(super) fn write_shared_thread_bound_budget_snapshot(
    repo_root: &Path,
    thread_id: &str,
    snapshot: &Value,
) -> Result<()> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok(());
    }
    let path = thread_bound_budget_snapshot_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedThreadBoundBudgetSnapshotCache {
        cache_version: THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.to_string(),
        snapshot: snapshot.clone(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn thread_bound_budget_snapshot_has_fresh_exact_limit_surfaces(
    snapshot: &Value,
    now_epoch_ms: u64,
) -> bool {
    if !thread_bound_budget_snapshot_has_fresh_budget_preview_surfaces(snapshot, now_epoch_ms) {
        return false;
    }
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let current_live_turn = &report["current_live_turn"];
    if !current_live_turn.is_object() {
        return false;
    }
    let status = current_live_turn["status"].as_str().unwrap_or_default();
    let exact_pair_available = current_live_turn["exact_pair_available"].as_bool() == Some(true);
    let observed_activity = current_live_turn["matched_events_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
        || current_live_turn["retrieval_context_pack_count"]
            .as_u64()
            .unwrap_or(0)
            > 0;
    if observed_activity {
        return status == "exact_pair_materialized" && exact_pair_available;
    }
    matches!(
        status,
        "no_amai_activity_in_current_live_turn" | "exact_pair_materialized"
    ) && exact_pair_available
}

pub(super) fn thread_bound_budget_snapshot_has_fresh_budget_preview_surfaces(
    snapshot: &Value,
    now_epoch_ms: u64,
) -> bool {
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let hourly_burn = &report["client_limit_hourly_burn"];
    if hourly_burn["status"].as_str() != Some("observed") {
        return false;
    }
    let Some(hourly_burn_observed_at_epoch_ms) =
        hourly_burn["latest_observed_at_epoch_ms"].as_u64()
    else {
        return false;
    };
    if now_epoch_ms.saturating_sub(hourly_burn_observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return false;
    }
    let status_bar_rate_limits = &report["client_live_meter"]["status_bar_rate_limits"];
    if status_bar_rate_limits["status"].as_str() != Some("observed") {
        return false;
    }
    let Some(status_bar_observed_at_epoch_ms) = status_bar_rate_limits["observed_at_epoch_ms"]
        .as_u64()
        .or_else(|| status_bar_rate_limits["ended_at_epoch_ms"].as_u64())
    else {
        return false;
    };
    if now_epoch_ms.saturating_sub(status_bar_observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return false;
    }
    true
}

pub(super) fn build_compact_client_budget_gate_cache(
    gate_payload: &Value,
    guard_payload: &Value,
    thread_id: Option<&str>,
) -> PersistedClientBudgetGateCache {
    PersistedClientBudgetGateCache {
        cache_version: CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: current_epoch_ms_u64(),
        thread_id: thread_id.map(str::to_string),
        gate: gate_payload.clone(),
        guard: guard_payload.clone(),
    }
}

pub(super) fn shared_client_budget_cache_matches_thread(
    cached_thread_id: Option<&str>,
    expected_thread_id: Option<&str>,
) -> bool {
    match (
        cached_thread_id
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        expected_thread_id
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(cached), Some(expected)) => cached == expected,
        (None, None) => true,
        _ => false,
    }
}

pub(super) fn load_shared_compact_client_budget_gate(
    repo_root: &Path,
    now_epoch_ms: u64,
    expected_thread_id: Option<&str>,
) -> Option<PersistedClientBudgetGateCache> {
    let path = client_budget_gate_shared_cache_path(repo_root, expected_thread_id);
    let payload = fs::read_to_string(path).ok()?;
    let cached: PersistedClientBudgetGateCache = serde_json::from_str(&payload).ok()?;
    if cached.cache_version != CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION {
        return None;
    }
    if !shared_client_budget_cache_matches_thread(cached.thread_id.as_deref(), expected_thread_id) {
        return None;
    }
    if now_epoch_ms.saturating_sub(cached.fetched_at_epoch_ms)
        > CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS
    {
        return None;
    }
    let observed_at_epoch_ms = cached.gate["client_budget_reply_gate"]["observed_at_epoch_ms"]
        .as_u64()
        .or_else(|| cached.guard["observed_at_epoch_ms"].as_u64())?;
    if now_epoch_ms.saturating_sub(observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return None;
    }
    if let Some(thread_id) = expected_thread_id {
        if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id).is_some_and(
            |invalidated_at_epoch_ms| invalidated_at_epoch_ms >= cached.fetched_at_epoch_ms,
        ) {
            return None;
        }
    }
    if !compact_thread_bound_client_budget_gate_payload_is_consistent(
        expected_thread_id,
        &cached.gate,
    ) {
        return None;
    }
    Some(cached)
}

pub(super) fn write_shared_compact_client_budget_gate(
    repo_root: &Path,
    thread_id: Option<&str>,
    cache: &PersistedClientBudgetGateCache,
) -> Result<()> {
    let path = client_budget_gate_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec(cache)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn load_shared_compact_client_budget_surfaces(
    repo_root: &Path,
    now_epoch_ms: u64,
    expected_thread_id: Option<&str>,
) -> Option<PersistedClientBudgetSurfacesCache> {
    let path = client_budget_surfaces_shared_cache_path(repo_root, expected_thread_id);
    let payload = fs::read_to_string(path).ok()?;
    let cached: PersistedClientBudgetSurfacesCache = serde_json::from_str(&payload).ok()?;
    if cached.cache_version != CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION {
        return None;
    }
    if !shared_client_budget_cache_matches_thread(cached.thread_id.as_deref(), expected_thread_id) {
        return None;
    }
    if now_epoch_ms.saturating_sub(cached.fetched_at_epoch_ms)
        > CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS
    {
        return None;
    }
    let observed_at_epoch_ms =
        cached.root_cause["client_budget_reply_gate"]["observed_at_epoch_ms"]
            .as_u64()
            .or_else(|| cached.gate["client_budget_reply_gate"]["observed_at_epoch_ms"].as_u64())
            .or_else(|| cached.guard["observed_at_epoch_ms"].as_u64())?;
    if now_epoch_ms.saturating_sub(observed_at_epoch_ms)
        > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
    {
        return None;
    }
    if let Some(thread_id) = expected_thread_id {
        if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id).is_some_and(
            |invalidated_at_epoch_ms| invalidated_at_epoch_ms >= cached.fetched_at_epoch_ms,
        ) {
            return None;
        }
    }
    if !compact_thread_bound_client_budget_root_cause_payload_is_consistent(
        expected_thread_id,
        &cached.root_cause,
    ) {
        return None;
    }
    Some(cached)
}

pub(super) fn other_thread_feedback_confirmation_is_inconsistent(
    action_kind: Option<&str>,
    must_confirm_feedback: bool,
    feedback_pending: bool,
    effect_verdict: Option<&str>,
) -> bool {
    effect_verdict == Some("other_thread")
        && (feedback_pending
            || must_confirm_feedback
            || action_kind == Some("confirm_same_thread_host_control_feedback"))
}

pub(super) fn compact_thread_bound_client_budget_gate_payload_is_consistent(
    expected_thread_id: Option<&str>,
    payload: &Value,
) -> bool {
    if expected_thread_id.is_none() {
        return true;
    }
    let gate = &payload["client_budget_reply_gate"]["reply_execution_gate"];
    !other_thread_feedback_confirmation_is_inconsistent(
        gate["action_kind"].as_str(),
        gate["must_confirm_same_thread_host_control_feedback_before_reply"].as_bool() == Some(true),
        gate["action_bundle"]["host_current_thread_control"]["feedback_pending"].as_bool()
            == Some(true),
        gate["action_bundle"]["host_current_thread_control"]["effect_verdict"].as_str(),
    )
}

pub(super) fn compact_thread_bound_client_budget_root_cause_payload_is_consistent(
    expected_thread_id: Option<&str>,
    payload: &Value,
) -> bool {
    if expected_thread_id.is_none() {
        return true;
    }
    if payload["thread_binding_state"].as_str() != Some("current_thread_bound")
        || payload["current_live_turn"]["status"].as_str() == Some("current_thread_unbound")
    {
        return false;
    }
    let gate = &payload["client_budget_reply_gate"]["reply_execution_gate"];
    !other_thread_feedback_confirmation_is_inconsistent(
        gate["action_kind"].as_str(),
        gate["must_confirm_same_thread_host_control_feedback_before_reply"].as_bool() == Some(true),
        gate["action_bundle"]["host_current_thread_control"]["feedback_pending"].as_bool()
            == Some(true),
        payload["host_current_thread_control_effect"]["effect_verdict"].as_str(),
    )
}

pub(super) fn write_shared_compact_client_budget_surfaces(
    repo_root: &Path,
    thread_id: Option<&str>,
    cache: &PersistedClientBudgetSurfacesCache,
) -> Result<()> {
    let path = client_budget_surfaces_shared_cache_path(repo_root, thread_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec(cache)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}
