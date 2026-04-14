use serde_json::Value;

use super::{
    OBSERVE_REFRESH_STUCK_GRACE_MS, OBSERVE_REFRESH_TIMEOUT_MS, ObserveCache, now_epoch_ms,
};

pub(super) fn attach_observe_cache_to_dashboard_payload(
    mut payload: Value,
    cache: &ObserveCache,
    refresh_ms: u64,
) -> Value {
    let cache_meta = observe_cache_meta(cache, refresh_ms);
    if let Some(root) = payload.as_object_mut() {
        root.insert("observe_cache".to_string(), cache_meta.clone());
    }
    if let Some(meta) = payload["meta"].as_object_mut() {
        if let Some(started_at) = cache.last_refresh_started_epoch_ms {
            meta.insert(
                "cache_refresh_started_at_epoch_ms".to_string(),
                Value::from(started_at),
            );
        }
        if let Some(completed_at) = cache.last_refresh_completed_epoch_ms {
            meta.insert(
                "cache_refresh_completed_at_epoch_ms".to_string(),
                Value::from(completed_at),
            );
            meta.insert(
                "cache_refresh_completed_at_label".to_string(),
                Value::from(completed_at.to_string()),
            );
        }
        if let Some(duration_ms) = cache.last_refresh_duration_ms {
            meta.insert(
                "cache_refresh_duration_ms".to_string(),
                Value::from(duration_ms),
            );
        }
        meta.insert(
            "cache_snapshot_age_ms".to_string(),
            Value::from(cache_snapshot_age_ms(cache).unwrap_or_default()),
        );
        meta.insert(
            "cache_stale".to_string(),
            Value::Bool(observe_cache_stale(cache, refresh_ms)),
        );
        if let Some(error) = &cache.last_error {
            meta.insert("cache_last_error".to_string(), Value::from(error.clone()));
        }
    }
    payload
}

pub(super) fn attach_observe_cache_to_snapshot(
    mut snapshot: Value,
    cache: &ObserveCache,
    refresh_ms: u64,
) -> Value {
    if let Some(root) = snapshot.as_object_mut() {
        root.insert(
            "observe_cache".to_string(),
            observe_cache_meta(cache, refresh_ms),
        );
    }
    snapshot
}

pub(super) fn observe_cache_meta(cache: &ObserveCache, refresh_ms: u64) -> Value {
    let age_ms = cache_snapshot_age_ms(cache);
    serde_json::json!({
        "refresh_ms": refresh_ms,
        "last_refresh_started_epoch_ms": cache.last_refresh_started_epoch_ms,
        "last_refresh_completed_epoch_ms": cache.last_refresh_completed_epoch_ms,
        "last_refresh_completed_label": cache
            .last_refresh_completed_epoch_ms
            .map(|epoch_ms| epoch_ms.to_string()),
        "last_refresh_duration_ms": cache.last_refresh_duration_ms,
        "refresh_in_progress": cache.refresh_in_progress,
        "snapshot_age_ms": age_ms,
        "stale": observe_cache_stale(cache, refresh_ms),
        "last_error": cache.last_error.clone(),
    })
}

pub(super) fn observe_refresh_stuck(cache: &ObserveCache) -> bool {
    if !cache.refresh_in_progress {
        return false;
    }
    let Some(started_at_epoch_ms) = cache.last_refresh_started_epoch_ms else {
        return true;
    };
    now_epoch_ms().saturating_sub(started_at_epoch_ms)
        > OBSERVE_REFRESH_TIMEOUT_MS.saturating_add(OBSERVE_REFRESH_STUCK_GRACE_MS)
}

pub(super) fn cache_snapshot_age_ms(cache: &ObserveCache) -> Option<u64> {
    cache
        .last_refresh_completed_epoch_ms
        .map(|completed_at| now_epoch_ms().saturating_sub(completed_at))
}

pub(super) fn observe_cache_stale(cache: &ObserveCache, refresh_ms: u64) -> bool {
    if cache.snapshot.is_none() {
        return true;
    }
    let max_age_ms = refresh_ms.max(1000).saturating_mul(3).max(
        cache
            .last_refresh_duration_ms
            .unwrap_or_default()
            .saturating_add(refresh_ms.max(1000).saturating_mul(2)),
    );
    cache_snapshot_age_ms(cache)
        .map(|age_ms| age_ms > max_age_ms)
        .unwrap_or(true)
}
