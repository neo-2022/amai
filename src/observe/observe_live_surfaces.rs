use super::observe_front_door::dashboard_live_summary_warmup_payload;
use super::*;

pub(super) async fn cached_dashboard_payload(state: &ObserveState) -> Result<Value> {
    let cache = state.cache.read().await;
    let mut payload = cache
        .dashboard_payload
        .clone()
        .ok_or_else(|| anyhow!("dashboard cache not ready"))?;
    if payload.get("client_budget_live").is_none() {
        if let Some(snapshot) = cache.snapshot.as_ref() {
            if let Some(root) = payload.as_object_mut() {
                root.insert(
                    "client_budget_live".to_string(),
                    dashboard::client_budget_live_payload(snapshot),
                );
            }
        }
    }
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

pub(super) async fn cached_snapshot_with_meta(state: &ObserveState) -> Result<Value> {
    let cache = state.cache.read().await;
    let snapshot = cache
        .snapshot
        .clone()
        .ok_or_else(|| anyhow!("snapshot cache not ready"))?;
    Ok(attach_observe_cache_to_snapshot(
        snapshot,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

pub(super) async fn live_active_agent_snapshot_for_request(state: &ObserveState) -> Result<Value> {
    let snapshot = if let Some(thread_id) = auto_thread_binding_hint_from_cache(state).await {
        merged_thread_bound_snapshot_with_meta(state, &thread_id).await?
    } else {
        cached_snapshot_with_meta(state).await?
    };
    let cached_activity = snapshot["agent_scope_activity"].clone();
    if !cached_activity.is_object() {
        return Ok(snapshot);
    }
    let db = postgres::connect_admin(&state.cfg).await?;
    postgres::bootstrap_schema(&db, &state.cfg).await?;
    let repo_root = discover_repo_root(None)?;
    let active_agent_budget =
        token_budget::collect_active_agent_live_budget_surface(&db, &repo_root, &cached_activity)
            .await?;
    Ok(overlay_live_active_agent_surfaces(
        snapshot,
        cached_activity,
        active_agent_budget,
    ))
}

pub(super) fn active_agent_budget_card_payload_from_snapshot(snapshot: &Value) -> Result<Value> {
    let surface = &snapshot["active_agent_budget"];
    let card = dashboard::build_active_agent_budget_session_card_from_surface(surface)
        .ok_or_else(|| anyhow!("active agent budget card not ready"))?;
    Ok(json!({
        "card": card,
        "captured_at_epoch_ms": surface["captured_at_epoch_ms"].clone(),
        "source": surface["source"].clone(),
    }))
}

pub(super) async fn live_active_agent_budget_card_payload(state: &ObserveState) -> Result<Value> {
    let snapshot = live_dashboard_summary_snapshot_for_request(state, None).await?;
    active_agent_budget_card_payload_from_snapshot(&snapshot)
}

pub(super) async fn dashboard_live_summary_payload_for_request(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Result<Value> {
    let resolved_thread_id = resolved_request_thread_hint(state, explicit_thread_id).await;
    {
        let cache = state.cache.read().await;
        let cache_age_ms = cache
            .dashboard_live_summary_completed_epoch_ms
            .map(|completed_at| now_epoch_ms().saturating_sub(completed_at));
        if cache.dashboard_live_summary_refresh_in_progress {
            if let Some(payload) = cache.dashboard_live_summary_payload.clone() {
                return Ok(attach_observe_cache_to_dashboard_payload(
                    payload,
                    &cache,
                    state.dashboard_refresh_ms,
                ));
            }
            return Ok(dashboard_live_summary_warmup_payload(
                cache_snapshot_age_ms(&cache),
            ));
        }
        if let Some(payload) = cache.dashboard_live_summary_payload.clone() {
            let cache_stale = cache_age_ms
                .map(|age_ms| age_ms > DASHBOARD_LIVE_SUMMARY_CACHE_TTL_MS)
                .unwrap_or(true);
            let refresh_needed = cache_stale && !cache.dashboard_live_summary_refresh_in_progress;
            drop(cache);
            if refresh_needed {
                spawn_dashboard_live_summary_refresh(state, resolved_thread_id.clone()).await;
            }
            let cache = state.cache.read().await;
            return Ok(attach_observe_cache_to_dashboard_payload(
                payload,
                &cache,
                state.dashboard_refresh_ms,
            ));
        }
    }

    let payload = refresh_dashboard_live_summary_cache(state, resolved_thread_id.clone()).await?;
    let cache = state.cache.read().await;
    Ok(attach_observe_cache_to_dashboard_payload(
        payload,
        &cache,
        state.dashboard_refresh_ms,
    ))
}

pub(super) async fn spawn_dashboard_live_summary_refresh(
    state: &ObserveState,
    resolved_thread_id: Option<String>,
) {
    {
        let mut cache = state.cache.write().await;
        if cache.dashboard_live_summary_refresh_in_progress {
            return;
        }
        cache.dashboard_live_summary_refresh_in_progress = true;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = refresh_dashboard_live_summary_cache(&state, resolved_thread_id).await {
            eprintln!("dashboard live summary refresh failed: {error:#}");
            let mut cache = state.cache.write().await;
            cache.dashboard_live_summary_refresh_in_progress = false;
        }
    });
}

pub(super) async fn refresh_dashboard_live_summary_cache(
    state: &ObserveState,
    resolved_thread_id: Option<String>,
) -> Result<Value> {
    let snapshot =
        live_dashboard_summary_snapshot_for_request(state, resolved_thread_id.as_deref()).await?;
    let payload = dashboard::build_live_summary_payload(
        &state.cfg,
        &snapshot,
        &state.bind,
        state.dashboard_refresh_ms,
    )?;
    let mut cache = state.cache.write().await;
    cache.dashboard_live_summary_payload = Some(payload.clone());
    cache.dashboard_live_summary_thread_id = resolved_thread_id;
    cache.dashboard_live_summary_completed_epoch_ms = Some(now_epoch_ms());
    cache.dashboard_live_summary_refresh_in_progress = false;
    Ok(payload)
}

pub(super) fn overlay_live_active_agent_surfaces(
    mut snapshot: Value,
    activity: Value,
    active_agent_budget: Value,
) -> Value {
    if let Some(root) = snapshot.as_object_mut() {
        root.insert("agent_scope_activity".to_string(), activity);
        root.insert("active_agent_budget".to_string(), active_agent_budget);
    }
    snapshot
}

pub(super) fn overlay_dashboard_live_summary_surfaces(
    mut snapshot: Value,
    activity: Value,
    latest_repo_working_state_restore: Value,
    active_agent_budget: Value,
) -> Value {
    if let Some(root) = snapshot.as_object_mut() {
        root.insert("agent_scope_activity".to_string(), activity);
        root.insert(
            "latest_repo_working_state_restore".to_string(),
            latest_repo_working_state_restore,
        );
        root.insert("active_agent_budget".to_string(), active_agent_budget);
    }
    snapshot
}

pub(super) async fn live_dashboard_summary_snapshot_for_request(
    state: &ObserveState,
    explicit_thread_id: Option<&str>,
) -> Result<Value> {
    let base_snapshot =
        if let Some(thread_id) = resolved_request_thread_hint(state, explicit_thread_id).await {
            merged_thread_bound_snapshot_with_meta(state, &thread_id).await?
        } else {
            cached_snapshot_with_meta(state).await?
        };
    let db = postgres::connect_admin(&state.cfg).await?;
    postgres::bootstrap_schema(&db, &state.cfg).await?;
    let repo_root = discover_repo_root(None)?;
    let agent_scope_activity = token_budget::collect_agent_scope_activity(&db).await?;
    let active_agent_budget = token_budget::collect_active_agent_live_budget_surface(
        &db,
        &repo_root,
        &agent_scope_activity,
    )
    .await?;
    let latest_repo_working_state_restore =
        latest_repo_working_state_restore_payload(&db, &repo_root)
            .await?
            .unwrap_or_else(|| json!({ "working_state_restore": {} }));
    Ok(overlay_dashboard_live_summary_surfaces(
        base_snapshot,
        agent_scope_activity,
        latest_repo_working_state_restore,
        active_agent_budget,
    ))
}

pub(super) async fn refresh_client_live_meter_on_request(state: &ObserveState) {
    if let Err(error) = maybe_refresh_client_live_meter(state).await {
        eprintln!("observe request-side client meter refresh failed: {error:#}");
    }
    let mut cache = state.cache.write().await;
    cache.client_live_meter_refresh_in_progress = false;
}

pub(super) async fn spawn_client_live_meter_refresh(state: &ObserveState) {
    let now = now_epoch_ms();
    {
        let mut cache = state.cache.write().await;
        if cache.client_live_meter_refresh_in_progress {
            return;
        }
        if cache
            .client_live_meter_refresh_started_epoch_ms
            .is_some_and(|started_at| {
                now.saturating_sub(started_at) < CLIENT_LIMIT_LIVE_SOURCE_TTL_MS
            })
        {
            return;
        }
        cache.client_live_meter_refresh_in_progress = true;
        cache.client_live_meter_refresh_started_epoch_ms = Some(now);
    }
    let state = state.clone();
    tokio::spawn(async move {
        refresh_client_live_meter_on_request(&state).await;
    });
}

pub(super) async fn maybe_refresh_client_live_meter(state: &ObserveState) -> Result<()> {
    let cache_snapshot = {
        let cache = state.cache.read().await;
        Some((
            cache.snapshot.clone(),
            cache.last_refresh_completed_epoch_ms,
            cache.refresh_in_progress,
            observe_cache_stale(&cache, state.dashboard_refresh_ms),
        ))
    };

    let Some((snapshot, _last_refresh_completed_epoch_ms, refresh_in_progress, cache_stale)) =
        cache_snapshot
    else {
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    };

    if cache_stale {
        if refresh_in_progress {
            return Ok(());
        }
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    }

    let Some(snapshot) = snapshot else {
        if refresh_in_progress {
            return Ok(());
        }
        return refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await;
    };

    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if cached_exact_client_limit_refresh_needed(
        &snapshot,
        now_epoch_ms,
        CLIENT_LIMIT_LIVE_SOURCE_TTL_MS,
    ) {
        if refresh_in_progress {
            return Ok(());
        }
        return prewarm_active_thread_bound_client_budget_surfaces(state.cache.clone(), &state.cfg)
            .await;
    }

    if active_agent_budget_refresh_needed(&snapshot, ACTIVE_AGENT_CARD_MAX_SOURCE_DRIFT_MS)? {
        if refresh_in_progress {
            return Ok(());
        }
        return prewarm_active_thread_bound_client_budget_surfaces(state.cache.clone(), &state.cfg)
            .await;
    }

    let cached_meter = cached_client_live_meter_state(&snapshot);
    let preferred_thread_id = codex_threads::current_thread_id()
        .or_else(|| cached_meter.working_state_thread_id.clone())
        .or_else(|| cached_meter.thread_id.clone());
    let Some(thread_id) = preferred_thread_id else {
        return Ok(());
    };
    let latest_rollout =
        codex_threads::latest_rollout_client_meter_observation_for_thread(&thread_id)?;
    if !client_live_meter_refresh_needed(&cached_meter, latest_rollout.as_ref()) {
        return Ok(());
    }
    if refresh_in_progress {
        return Ok(());
    }
    prewarm_thread_bound_client_budget_surfaces_for_thread(
        state.cache.clone(),
        &state.cfg,
        &thread_id,
    )
    .await
}

pub(super) fn active_agent_budget_refresh_needed(
    snapshot: &Value,
    max_source_drift_ms: i64,
) -> Result<bool> {
    let Some(agents) = snapshot["active_agent_budget"]["agents"].as_array() else {
        return Ok(false);
    };

    for agent in agents {
        let Some(thread_id) = agent["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let card_ended_at_epoch_ms = agent["client_live_meter"]["ended_at_epoch_ms"]
            .as_i64()
            .unwrap_or_default();
        let latest_rollout =
            codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)?;
        let Some(latest_rollout) = latest_rollout else {
            continue;
        };
        if active_agent_card_refresh_needed_against_rollout(
            card_ended_at_epoch_ms,
            &latest_rollout,
            max_source_drift_ms,
        ) {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(super) fn active_agent_card_refresh_needed_against_rollout(
    card_ended_at_epoch_ms: i64,
    rollout: &codex_threads::RolloutClientMeterObservation,
    max_source_drift_ms: i64,
) -> bool {
    if card_ended_at_epoch_ms <= 0 {
        return true;
    }
    rollout.ended_at_epoch_ms > card_ended_at_epoch_ms.saturating_add(max_source_drift_ms.max(0))
}

pub(super) fn cached_exact_client_limit_refresh_needed(
    snapshot: &Value,
    now_epoch_ms: u64,
    max_source_age_ms: u64,
) -> bool {
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let hourly_burn = &report["client_limit_hourly_burn"];
    if hourly_burn["status"].as_str() != Some("observed") {
        return true;
    }
    let Some(observed_at_epoch_ms) = hourly_burn["latest_observed_at_epoch_ms"].as_u64() else {
        return true;
    };
    now_epoch_ms.saturating_sub(observed_at_epoch_ms) > max_source_age_ms.max(1)
}

pub(super) fn cached_client_live_meter_state(snapshot: &Value) -> CachedClientLiveMeterState {
    let meter = &snapshot["token_budget_report"]["client_live_meter"];
    let working_state_thread_id =
        snapshot["latest_repo_working_state_restore"]["working_state_restore"]["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    CachedClientLiveMeterState {
        working_state_thread_id,
        thread_id: meter["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        turn_id: meter["turn_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        ended_at_epoch_ms: meter["ended_at_epoch_ms"].as_i64(),
        client_turn_total_tokens: meter["client_turn_total_tokens"].as_u64(),
        primary_limit_used_percent: meter["primary_limit_used_percent"].as_u64(),
        secondary_limit_used_percent: meter["secondary_limit_used_percent"].as_u64(),
    }
}

pub(super) fn client_live_meter_refresh_needed(
    cached: &CachedClientLiveMeterState,
    rollout: Option<&codex_threads::RolloutClientMeterObservation>,
) -> bool {
    if let Some(working_state_thread_id) = cached.working_state_thread_id.as_deref() {
        if cached.thread_id.as_deref() != Some(working_state_thread_id) {
            return true;
        }
    }

    let Some(rollout) = rollout else {
        return cached.thread_id.is_none();
    };

    cached.thread_id.as_deref() != Some(rollout.thread_id.as_str())
        || cached.turn_id.as_deref() != Some(rollout.turn_id.as_str())
        || cached.ended_at_epoch_ms.unwrap_or_default() < rollout.ended_at_epoch_ms
        || cached.client_turn_total_tokens != Some(rollout.client_turn_total_tokens)
        || cached.primary_limit_used_percent != Some(rollout.latest_primary_limit_used_percent)
        || cached.secondary_limit_used_percent != Some(rollout.latest_secondary_limit_used_percent)
}
