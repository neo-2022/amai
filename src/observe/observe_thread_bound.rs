use super::*;

pub(super) fn normalized_thread_id_hint(thread_id: Option<&str>) -> Option<&str> {
    thread_id.map(str::trim).filter(|value| !value.is_empty())
}

pub(super) async fn resolved_request_thread_hint(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Option<String> {
    if let Some(thread_id) = normalized_thread_id_hint(explicit_thread_id) {
        Some(thread_id.to_string())
    } else {
        auto_thread_binding_hint_from_cache(state).await
    }
}

pub(super) async fn auto_thread_binding_hint_from_cache(state: &ObserveState) -> Option<String> {
    let cache = state.cache.read().await;
    let snapshot = cache.snapshot.clone()?;
    strict_auto_thread_binding_hint_from_snapshot(snapshot)
}

pub(super) fn strict_auto_thread_binding_hint_from_snapshot(snapshot: Value) -> Option<String> {
    strict_auto_thread_binding_hint_from_agent_scope_activity(
        snapshot["agent_scope_activity"].clone(),
    )
}

fn strict_auto_thread_binding_hint_from_agent_scope_activity(activity: Value) -> Option<String> {
    let recent_thread_ids = activity["client_recent_threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item["thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    if recent_thread_ids.is_empty() {
        return None;
    }

    let active_thread_ids = activity["active_now_scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item["owner_thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    if active_thread_ids.len() != 1 {
        return None;
    }

    let thread_id = active_thread_ids.into_iter().next()?;
    recent_thread_ids.contains(&thread_id).then_some(thread_id)
}

pub(super) async fn thread_bound_dashboard_payload(
    state: &ObserveState,
    thread_id: &str,
) -> Result<Value> {
    let snapshot = merged_thread_bound_snapshot_with_meta(state, thread_id).await?;
    let mut payload = dashboard::build_payload(
        &state.cfg,
        &snapshot,
        &state.bind,
        state.dashboard_refresh_ms,
    )?;
    if payload.get("client_budget_live").is_none() {
        if let Some(root) = payload.as_object_mut() {
            root.insert(
                "client_budget_live".to_string(),
                dashboard::client_budget_live_payload(&snapshot),
            );
        }
    }
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

pub(super) async fn merged_thread_bound_snapshot_with_meta(
    state: &ObserveState,
    thread_id: &str,
) -> Result<Value> {
    let thread_bound_snapshot = thread_bound_snapshot_with_meta(state, thread_id).await?;
    let base_snapshot = match cached_snapshot_with_meta(state).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            refresh_observe_cache(
                state.cache.clone(),
                state.cfg.clone(),
                state.bind.clone(),
                state.dashboard_refresh_ms,
            )
            .await?;
            cached_snapshot_with_meta(state).await?
        }
    };
    let snapshot = merge_thread_bound_client_budget_snapshot_into_base_snapshot(
        &base_snapshot,
        &thread_bound_snapshot,
    );
    Ok(snapshot)
}

pub(super) fn merge_thread_bound_client_budget_snapshot_into_base_snapshot(
    base_snapshot: &Value,
    thread_bound_snapshot: &Value,
) -> Value {
    let mut merged = base_snapshot.clone();
    let token_budget_report = &thread_bound_snapshot["token_budget_report"];
    if token_budget_report.is_object() {
        let base_outer = &base_snapshot["token_budget_report"];
        if base_outer.is_object()
            && base_outer["token_budget_report"].is_object()
            && token_budget_report["token_budget_report"].is_object()
        {
            let mut merged_outer = base_outer.clone();
            let mut merged_inner = base_outer["token_budget_report"].clone();
            if let Some(merged_inner_object) = merged_inner.as_object_mut() {
                for key in [
                    "current_session",
                    "live_response_latency",
                    "client_live_meter",
                    "personal_agent_kpi",
                    "client_limit_hourly_burn",
                    "current_live_turn",
                    "client_budget_target_percent",
                    "filters",
                ] {
                    if token_budget_report["token_budget_report"][key].is_null() {
                        continue;
                    }
                    if key == "live_response_latency"
                        && should_keep_base_live_response_latency(
                            &base_outer["token_budget_report"][key],
                            &token_budget_report["token_budget_report"][key],
                        )
                    {
                        continue;
                    }
                    merged_inner_object.insert(
                        key.to_string(),
                        token_budget_report["token_budget_report"][key].clone(),
                    );
                }
                if base_outer["token_budget_report"]["statement_previews"].is_object()
                    || token_budget_report["token_budget_report"]["statement_previews"].is_object()
                {
                    let mut merged_statement_previews =
                        base_outer["token_budget_report"]["statement_previews"].clone();
                    if let Some(statement_previews_object) =
                        merged_statement_previews.as_object_mut()
                    {
                        if token_budget_report["token_budget_report"]["statement_previews"]
                            ["current_session"]
                            .is_object()
                        {
                            statement_previews_object.insert(
                                "current_session".to_string(),
                                token_budget_report["token_budget_report"]["statement_previews"]
                                    ["current_session"]
                                    .clone(),
                            );
                        }
                    }
                    merged_inner_object
                        .insert("statement_previews".to_string(), merged_statement_previews);
                }
            }
            if let Some(merged_outer_object) = merged_outer.as_object_mut() {
                merged_outer_object.insert("token_budget_report".to_string(), merged_inner);
            }
            merged["token_budget_report"] = merged_outer;
        } else {
            merged["token_budget_report"] = token_budget_report.clone();
        }
    }
    let latest_repo_restore = &thread_bound_snapshot["latest_repo_working_state_restore"];
    if latest_repo_restore.is_object() {
        merged["latest_repo_working_state_restore"] = latest_repo_restore.clone();
    }
    merged
}

fn live_response_latency_total_sample_count(surface: &Value) -> u64 {
    ["current_session", "rolling_window"]
        .into_iter()
        .filter_map(|scope| surface[scope]["sample_count"].as_u64())
        .sum()
}

fn should_keep_base_live_response_latency(base_surface: &Value, thread_surface: &Value) -> bool {
    let base_count = live_response_latency_total_sample_count(base_surface);
    let thread_count = live_response_latency_total_sample_count(thread_surface);
    base_count > 0 && thread_count == 0
}

pub(super) async fn thread_bound_snapshot_with_meta(
    state: &ObserveState,
    thread_id: &str,
) -> Result<Value> {
    let repo_root = discover_repo_root(None)?;
    let _ = write_shared_active_thread_hint(&repo_root, thread_id);
    if let Some(snapshot) = cached_thread_bound_snapshot_with_meta(state, thread_id).await {
        return Ok(snapshot);
    }
    let (latest_repo_restore_override, base_report_override) = {
        let cache = state.cache.read().await;
        (
            cached_latest_repo_working_state_restore_snapshot(&cache),
            cached_token_budget_report_snapshot(&cache),
        )
    };
    let snapshot = collect_client_budget_snapshot_with_thread_hint(
        &state.cfg,
        &repo_root,
        Some(thread_id),
        base_report_override.as_ref(),
        latest_repo_restore_override.as_ref(),
    )
    .await?;
    let _ = write_shared_thread_bound_budget_snapshot(&repo_root, thread_id, &snapshot);
    let cached_snapshot = {
        let mut cache = state.cache.write().await;
        cache.thread_bound_snapshot = Some(snapshot);
        cache.thread_bound_snapshot_thread_id = Some(thread_id.to_string());
        cache.thread_bound_snapshot_completed_epoch_ms = Some(now_epoch_ms());
        cache.thread_bound_snapshot.clone().unwrap_or(Value::Null)
    };
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_snapshot(
        cached_snapshot,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

pub(super) fn materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
    repo_root: &Path,
    thread_id: &str,
    snapshot: &Value,
) {
    let _ = write_shared_thread_bound_budget_snapshot(repo_root, thread_id, snapshot);
    let guard = dashboard::current_session_budget_guard(snapshot);
    let root_cause_payload =
        dashboard::client_budget_root_cause_payload_with_guard(snapshot, &guard);
    let compact_root_cause =
        compact_client_budget_root_cause_payload(&root_cause_payload, Some(&guard));
    let compact_gate =
        front_door_client_budget_gate_payload(compact_cli_client_budget_gate_payload(&guard));
    let compact_guard = compact_current_session_budget_guard_payload(&guard);
    let surfaces_cache = build_compact_client_budget_surfaces_cache(
        &compact_root_cause,
        &compact_gate,
        &compact_guard,
        Some(thread_id),
    );
    let _ =
        write_shared_compact_client_budget_surfaces(repo_root, Some(thread_id), &surfaces_cache);
    let gate_cache =
        build_compact_client_budget_gate_cache(&compact_gate, &compact_guard, Some(thread_id));
    let _ = write_shared_compact_client_budget_gate(repo_root, Some(thread_id), &gate_cache);
}

pub(super) async fn populate_thread_bound_client_budget_surfaces_from_snapshot(
    cache: Arc<RwLock<ObserveCache>>,
    repo_root: &Path,
    thread_id: &str,
    snapshot: Value,
) {
    materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
        repo_root, thread_id, &snapshot,
    );

    let completed_epoch_ms = now_epoch_ms();
    let mut state = cache.write().await;
    state.thread_bound_snapshot = Some(snapshot);
    state.thread_bound_snapshot_thread_id = Some(thread_id.to_string());
    state.thread_bound_snapshot_completed_epoch_ms = Some(completed_epoch_ms);
}

async fn cached_thread_bound_snapshot_with_meta(
    state: &ObserveState,
    thread_id: &str,
) -> Option<Value> {
    let repo_root = discover_repo_root(None).ok()?;
    {
        let cache = state.cache.read().await;
        if let (Some(cached_thread_id), Some(completed_at), Some(snapshot)) = (
            cache.thread_bound_snapshot_thread_id.as_deref(),
            cache.thread_bound_snapshot_completed_epoch_ms,
            cache.thread_bound_snapshot.clone(),
        ) {
            let now_epoch_ms_value = now_epoch_ms();
            if cached_thread_id == thread_id
                && now_epoch_ms_value.saturating_sub(completed_at)
                    <= COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS
                && thread_bound_budget_snapshot_has_fresh_exact_limit_surfaces(
                    &snapshot,
                    now_epoch_ms_value,
                )
                && !load_shared_thread_bound_snapshot_invalidation(&repo_root, thread_id)
                    .is_some_and(|invalidated_at_epoch_ms| invalidated_at_epoch_ms >= completed_at)
            {
                return Some(attach_observe_cache_to_snapshot(
                    snapshot,
                    &cache,
                    state.dashboard_refresh_ms,
                ));
            }
        }
    }
    let now_epoch_ms_value = current_epoch_ms_u64();
    if let Some(snapshot) =
        load_shared_thread_bound_budget_snapshot(&repo_root, now_epoch_ms_value, thread_id)
    {
        let mut cache = state.cache.write().await;
        cache.thread_bound_snapshot = Some(snapshot.clone());
        cache.thread_bound_snapshot_thread_id = Some(thread_id.to_string());
        cache.thread_bound_snapshot_completed_epoch_ms = Some(now_epoch_ms_value);
        return Some(attach_observe_cache_to_snapshot(
            snapshot,
            &cache,
            state.dashboard_refresh_ms,
        ));
    }
    None
}

pub(super) fn cached_latest_repo_working_state_restore_snapshot(
    cache: &ObserveCache,
) -> Option<Value> {
    let snapshot = cache.snapshot.as_ref()?;
    let latest_repo_restore = snapshot["latest_repo_working_state_restore"].clone();
    latest_repo_restore
        .get("working_state_restore")
        .is_some()
        .then_some(latest_repo_restore)
}

pub(super) fn cached_token_budget_report_snapshot(cache: &ObserveCache) -> Option<Value> {
    let snapshot = cache.snapshot.as_ref()?;
    let report = snapshot["token_budget_report"]["token_budget_report"].clone();
    report["current_session"].is_object().then_some(report)
}

pub(super) async fn compact_client_budget_snapshot_for_request(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Result<Value> {
    refresh_compact_client_budget_snapshot_on_request(state).await?;
    if let Some(thread_id) = normalized_thread_id_hint(explicit_thread_id) {
        thread_bound_snapshot_with_meta(state, thread_id).await
    } else {
        cached_snapshot_with_meta(state).await
    }
}

async fn refresh_compact_client_budget_snapshot_on_request(state: &ObserveState) -> Result<()> {
    let (snapshot_present, snapshot_age_ms, refresh_in_progress) = {
        let cache = state.cache.read().await;
        (
            cache.snapshot.is_some(),
            cache_snapshot_age_ms(&cache),
            cache.refresh_in_progress,
        )
    };

    let cache_too_old = compact_client_budget_snapshot_cache_too_old(snapshot_age_ms);
    if !snapshot_present || cache_too_old {
        if refresh_in_progress {
            return Err(anyhow!(
                "compact client-budget snapshot cache is unavailable or too stale while refresh is still in progress"
            ));
        }
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    }

    Ok(())
}

pub(super) fn compact_client_budget_snapshot_cache_too_old(snapshot_age_ms: Option<u64>) -> bool {
    snapshot_age_ms
        .map(|age_ms| age_ms > COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS)
        .unwrap_or(true)
}
