use super::*;

pub(super) fn dashboard_same_meter_sync_signature(
    events: &[TokenBudgetEvent],
    rollout_observation_signature: &str,
) -> String {
    let assistant_generation_missing_context_pack_ids = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event.assistant_generation_tokens.is_none()
        })
        .filter_map(event_context_pack_id)
        .collect::<BTreeSet<_>>();
    let tool_overhead_missing_context_pack_ids = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event.tool_overhead_tokens.is_none()
        })
        .filter_map(event_context_pack_id)
        .collect::<BTreeSet<_>>();
    let payload = json!({
        "assistant_generation_missing_context_pack_ids": assistant_generation_missing_context_pack_ids,
        "tool_overhead_missing_context_pack_ids": tool_overhead_missing_context_pack_ids,
        "rollout_observation_signature": rollout_observation_signature,
    });
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(super) fn dashboard_same_meter_sync_shared_cache_path(repo_root: &Path) -> PathBuf {
    canonical_repo_root(repo_root).join(DASHBOARD_SAME_METER_SYNC_SHARED_CACHE_RELATIVE_PATH)
}

pub(super) fn load_shared_dashboard_same_meter_sync_signature(repo_root: &Path) -> Option<String> {
    let path = dashboard_same_meter_sync_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let cache: PersistedDashboardSameMeterSyncCache = serde_json::from_slice(&bytes).ok()?;
    if cache.cache_version != DASHBOARD_SAME_METER_SYNC_SHARED_CACHE_VERSION {
        return None;
    }
    let expected_repo_root = canonical_repo_root(repo_root);
    let cached_repo_root = canonical_repo_root(Path::new(&cache.repo_root));
    if cached_repo_root != expected_repo_root {
        return None;
    }
    Some(cache.signature)
}

pub(super) fn write_shared_dashboard_same_meter_sync_signature(
    repo_root: &Path,
    signature: &str,
) -> Result<()> {
    let canonical_repo_root = canonical_repo_root(repo_root);
    let path = canonical_repo_root.join(DASHBOARD_SAME_METER_SYNC_SHARED_CACHE_RELATIVE_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let cache = PersistedDashboardSameMeterSyncCache {
        cache_version: DASHBOARD_SAME_METER_SYNC_SHARED_CACHE_VERSION.to_string(),
        repo_root: canonical_repo_root.display().to_string(),
        signature: signature.to_string(),
    };
    fs::write(&path, serde_json::to_vec(&cache)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn should_run_dashboard_same_meter_sync(repo_root: &Path, signature: &str) -> bool {
    let cache = DASHBOARD_SAME_METER_SYNC_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return true;
    };
    let repo_root = canonical_repo_root(repo_root);
    if let Some(entry) = guard.as_ref() {
        if entry.repo_root == repo_root && entry.signature == signature {
            return false;
        }
    }
    if load_shared_dashboard_same_meter_sync_signature(&repo_root).as_deref() == Some(signature) {
        *guard = Some(DashboardSameMeterSyncCache {
            repo_root,
            signature: signature.to_string(),
        });
        return false;
    }
    *guard = Some(DashboardSameMeterSyncCache {
        repo_root: repo_root.clone(),
        signature: signature.to_string(),
    });
    let _ = write_shared_dashboard_same_meter_sync_signature(&repo_root, signature);
    true
}

pub(super) fn dashboard_report_cache_debug(
    previous_entry: Option<&DashboardReportCache>,
    report_signature: &str,
    components: &DashboardReportSignatureComponents,
    pre_cache_timings: &DashboardReportPreCacheTimings,
    assistant_scope_debug: &DashboardAssistantScopeDebug,
) -> Value {
    let mut reasons = Vec::new();
    let mut previous_signature = Value::Null;
    if let Some(previous) = previous_entry {
        previous_signature = Value::from(previous.signature.clone());
        if previous.components.current_session_events != components.current_session_events {
            reasons.push("current_session_events");
        }
        if previous.components.rolling_window_events != components.rolling_window_events {
            reasons.push("rolling_window_events");
        }
        if previous.components.lifetime_events != components.lifetime_events {
            reasons.push("lifetime_events");
        }
        if previous.components.current_session_assistant_scope
            != components.current_session_assistant_scope
        {
            reasons.push("current_session_assistant_scope");
        }
        if previous.components.rolling_window_assistant_scope
            != components.rolling_window_assistant_scope
        {
            reasons.push("rolling_window_assistant_scope");
        }
        if previous.components.lifetime_assistant_scope != components.lifetime_assistant_scope {
            reasons.push("lifetime_assistant_scope");
        }
        if previous.components.client_live_meter != components.client_live_meter {
            reasons.push("client_live_meter");
        }
        if previous.components.exact_client_limits != components.exact_client_limits {
            reasons.push("exact_client_limits");
        }
        if previous.components.live_response_latency != components.live_response_latency {
            reasons.push("live_response_latency");
        }
    } else {
        reasons.push("cold_start");
    }
    json!({
        "status": "miss",
        "signature": report_signature,
        "previous_signature": previous_signature,
        "changed_components": reasons,
        "pre_cache_total_ms": pre_cache_timings.total_ms,
        "pre_cache_stage_ms": dashboard_precache_stage_ms_value(pre_cache_timings),
        "assistant_scope_debug": dashboard_assistant_scope_debug_value(assistant_scope_debug),
    })
}

pub(super) fn record_dashboard_precache_stage_ms(
    timings: &mut DashboardReportPreCacheTimings,
    stage_key: &str,
    started_at: Instant,
) {
    record_dashboard_stage_ms(&mut timings.stage_ms, stage_key, started_at);
}

pub(super) fn dashboard_precache_stage_ms_value(timings: &DashboardReportPreCacheTimings) -> Value {
    Value::Object(
        timings
            .stage_ms
            .iter()
            .map(|(stage_key, elapsed_ms)| (stage_key.clone(), Value::from(*elapsed_ms)))
            .collect(),
    )
}

pub(super) fn record_dashboard_stage_ms(
    stage_ms: &mut BTreeMap<String, u64>,
    stage_key: &str,
    started_at: Instant,
) {
    stage_ms.insert(
        stage_key.to_string(),
        started_at.elapsed().as_millis() as u64,
    );
}
