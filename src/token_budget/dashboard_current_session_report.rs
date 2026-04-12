use super::*;

pub(crate) async fn collect_dashboard_current_session_budget_report_with_thread_hint_and_base(
    db: &Client,
    base_report: Option<&Value>,
    explicit_thread_id_hint: Option<&str>,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let now_epoch_ms = current_epoch_ms().unwrap_or_default() as u64;
    if let Some(report) = reusable_exact_thread_current_session_budget_report_from_base_report(
        base_report,
        explicit_thread_id_hint,
    ) {
        return Ok(report);
    }
    let repo_root_str = repo_root
        .to_str()
        .ok_or_else(|| anyhow!("repo_root must be valid UTF-8"))?;
    let config = load_config(&repo_root)?;
    let profile = resolve_profile(&config, None, &repo_root)?;
    let include_verify_events = config.measurement.include_verify_events_by_default;
    let session_gap_ms = profile.session_gap_minutes.saturating_mul(60_000) as i64;

    let (rollout_observation_signature, rollout_observations) =
        dashboard_rollout_assistant_generation_observations_for_repo(&repo_root)?;
    let mut session_events = load_dashboard_current_session_events(
        db,
        &repo_root,
        include_verify_events,
        session_gap_ms,
    )
    .await?;
    let dashboard_sync_signature =
        dashboard_same_meter_sync_signature(&session_events, &rollout_observation_signature);

    let (tool_overhead_changed, assistant_generation_changed, continuity_baseline_changed) =
        if should_run_dashboard_same_meter_sync(&repo_root, &dashboard_sync_signature) {
            (
                sync_context_pack_tool_overhead_for_events(db, &repo_root, &session_events).await?,
                sync_rollout_assistant_generation_for_events(
                    db,
                    &session_events,
                    &rollout_observations,
                )
                .await?,
                sync_continuity_pre_amai_baseline_for_events(db, &repo_root, &session_events)
                    .await?,
            )
        } else {
            (false, false, false)
        };

    if tool_overhead_changed || assistant_generation_changed || continuity_baseline_changed {
        session_events = load_dashboard_current_session_events(
            db,
            &repo_root,
            include_verify_events,
            session_gap_ms,
        )
        .await?;
    }

    let personal_agent_scope =
        current_workspace_personal_kpi_selector(db, &repo_root, explicit_thread_id_hint).await?;
    let snapshot_kinds = dashboard_event_snapshot_kinds(include_verify_events);
    let dashboard_summary =
        postgres::summarize_observability_snapshots_by_kinds(db, snapshot_kinds).await?;
    let mut personal_kpi_source_events = load_dashboard_token_events_with_summary(
        db,
        &repo_root,
        include_verify_events,
        snapshot_kinds,
        &dashboard_summary,
    )
    .await?;
    personal_kpi_source_events.sort_by_key(|event| event.created_at_epoch_ms);
    let personal_kpi_source_events =
        reconcile_followup_recovery(&personal_kpi_source_events, session_gap_ms);
    let personal_agent_5h_events = personal_kpi_window_events(
        &personal_kpi_source_events,
        personal_agent_scope.as_ref(),
        now_epoch_ms as i64,
    );
    let personal_agent_5h_summary = summarize_events(
        &personal_agent_5h_events,
        now_epoch_ms as i64,
        &config.measurement,
        &config.contract,
    );

    let reusable_live_surfaces = reusable_exact_thread_budget_live_surfaces_from_base_report(
        base_report,
        explicit_thread_id_hint,
        now_epoch_ms,
        DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
    );
    let reusable_current_session_report = base_report.filter(|report| {
        report["current_session"].is_object()
            && report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                .is_object()
    });
    let reusable_client_limit_hourly_burn = reusable_client_limit_hourly_burn_from_base_report(
        base_report,
        now_epoch_ms,
        DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
    );
    let client_budget_target_percent = base_report
        .and_then(|report| report["client_budget_target_percent"].as_u64())
        .unwrap_or(client_budget_target_percent_for_repo(db, &repo_root).await?);
    let (current_session_summary, current_session_statement_preview) = if let Some(report) =
        reusable_current_session_report
    {
        (
            report["current_session"].clone(),
            report["statement_previews"]["current_session"].clone(),
        )
    } else {
        let now_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before unix epoch")?
            .as_millis() as i64;
        let current_session_assistant_scope = derive_dashboard_rollout_assistant_generation_scopes(
            db,
            &repo_root,
            &[session_events.as_slice()],
        )
        .await?
        .0
        .into_iter()
        .next()
        .unwrap_or_default();
        let current_session_summary = summarize_events(
            &session_events,
            now_epoch_ms,
            &config.measurement,
            &config.contract,
        );
        let current_session_statement_preview = build_dashboard_current_session_statement_preview(
            &current_session_summary,
            &session_events,
            &config.contract,
            &rollout_observations,
            materialized_assistant_scope(&current_session_assistant_scope),
        );
        (current_session_summary, current_session_statement_preview)
    };
    let client_limit_hourly_burn = if let Some(client_limit_hourly_burn) =
        reusable_client_limit_hourly_burn
    {
        client_limit_hourly_burn
    } else {
        let exact_client_limits_resolution =
            dashboard_exact_client_rate_limits_resolution().await?;
        let exact_client_limits_observation = exact_client_limits_resolution.observation.clone();
        collect_exact_client_limit_hourly_burn(
            db,
            exact_client_limits_observation.as_ref(),
            exact_client_limits_resolution
                .source
                .should_persist_exact_sample(),
            DEFAULT_CLIENT_LIMIT_HOURLY_BURN_WINDOW_MINUTES,
            DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MAX_LIVE_AGE_SECONDS,
            DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MIN_HISTORY_SPAN_MINUTES,
        )
        .await?
    };
    let (client_live_meter, current_live_turn) =
        if let Some((client_live_meter, current_live_turn)) = reusable_live_surfaces {
            (client_live_meter, current_live_turn)
        } else {
            let client_live_meter_binding_hint =
                preferred_dashboard_thread_binding_hint_with_override(
                    db,
                    &repo_root,
                    explicit_thread_id_hint,
                )
                .await?;
            let client_live_meter_observation = preferred_rollout_client_meter_observation(
                db,
                &repo_root,
                repo_root_str,
                client_live_meter_binding_hint.as_deref(),
            )
            .await?;
            let exact_client_limits_resolution =
                dashboard_exact_client_rate_limits_resolution().await?;
            let exact_client_limits_observation =
                exact_client_limits_resolution.observation.clone();
            let current_live_turn = build_current_live_turn_surface(
                &repo_root,
                db,
                &session_events,
                client_live_meter_observation.as_ref(),
                client_live_meter_binding_hint.as_deref(),
                &config.measurement,
                &config.contract,
                &rollout_observations,
            )
            .await?;
            let client_live_meter = build_client_live_meter_json(
                client_live_meter_observation.as_ref(),
                client_live_meter_binding_hint.as_deref(),
                exact_client_limits_observation.as_ref(),
            );
            (client_live_meter, current_live_turn)
        };
    let personal_agent_kpi = preferred_personal_agent_kpi(
        &personal_agent_5h_summary,
        personal_agent_scope.as_ref(),
        Some(&client_live_meter),
    );
    Ok(json!({
        "token_budget_report": {
            "surface": "dashboard_current_session_budget_only",
            "filters": {
                "include_verify_events": include_verify_events,
            },
            "client_budget_target_percent": client_budget_target_percent,
            "current_session": current_session_summary,
            "statement_previews": {
                "current_session": current_session_statement_preview,
            },
            "client_live_meter": client_live_meter,
            "personal_agent_kpi": personal_agent_kpi,
            "client_limit_hourly_burn": client_limit_hourly_burn,
            "current_live_turn": current_live_turn,
            "note": "Это минимальный current-session budget report для root-cause/gate surfaces: он не строит rolling/lifetime/export контуры и существует только для дешёвого live client-budget enforcement."
        }
    }))
}

pub(super) fn reusable_exact_thread_current_session_budget_report_from_base_report(
    base_report: Option<&Value>,
    explicit_thread_id_hint: Option<&str>,
) -> Option<Value> {
    let now_epoch_ms = current_epoch_ms().ok()? as u64;
    let report = base_report?;
    let client_budget_target_percent = report["client_budget_target_percent"].clone();
    let current_session = report["current_session"].clone();
    let current_session_statement_preview = report["statement_previews"]["current_session"].clone();
    let personal_agent_kpi = report["personal_agent_kpi"].clone();
    let client_limit_hourly_burn = reusable_client_limit_hourly_burn_from_base_report(
        base_report,
        now_epoch_ms,
        DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
    )?;
    let (client_live_meter, current_live_turn) =
        reusable_exact_thread_budget_live_surfaces_from_base_report(
            base_report,
            explicit_thread_id_hint,
            now_epoch_ms,
            DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
        )?;
    if !current_session.is_object()
        || !current_session_statement_preview["client_limit_meter_alignment"].is_object()
        || !client_limit_hourly_burn.is_object()
    {
        return None;
    }
    Some(json!({
        "token_budget_report": {
            "surface": "dashboard_current_session_budget_only",
            "filters": report["filters"].clone(),
            "client_budget_target_percent": client_budget_target_percent,
            "current_session": current_session,
            "statement_previews": {
                "current_session": current_session_statement_preview,
            },
            "client_live_meter": client_live_meter,
            "personal_agent_kpi": personal_agent_kpi,
            "client_limit_hourly_burn": client_limit_hourly_burn,
            "current_live_turn": current_live_turn,
            "note": "Это минимальный current-session budget report для root-cause/gate surfaces: он не строит rolling/lifetime/export контуры и существует только для дешёвого live client-budget enforcement."
        }
    }))
}

fn reusable_client_limit_hourly_burn_from_base_report(
    base_report: Option<&Value>,
    now_epoch_ms: u64,
    max_live_age_ms: u64,
) -> Option<Value> {
    let report = base_report?;
    let client_limit_hourly_burn = report["client_limit_hourly_burn"].clone();
    if !client_limit_hourly_burn.is_object()
        || client_limit_hourly_burn["status"].as_str() != Some("observed")
    {
        return None;
    }
    let observed_at_epoch_ms = client_limit_hourly_burn["latest_observed_at_epoch_ms"].as_u64()?;
    if now_epoch_ms.saturating_sub(observed_at_epoch_ms) > max_live_age_ms.max(1) {
        return None;
    }
    Some(client_limit_hourly_burn)
}

fn reusable_exact_client_limits_from_live_meter_are_fresh(
    client_live_meter: &Value,
    now_epoch_ms: u64,
    max_live_age_ms: u64,
) -> bool {
    let Some(observed_at_epoch_ms) = preferred_online_limit_surface(client_live_meter)
        .map(|surface| surface.observed_at_epoch_ms)
    else {
        return false;
    };
    now_epoch_ms.saturating_sub(observed_at_epoch_ms) <= max_live_age_ms.max(1)
}

fn observe_cache_thread_suffix(thread_id: &str) -> String {
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

#[cfg(test)]
pub(super) fn thread_bound_budget_snapshot_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_budget_snapshot.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

#[cfg(test)]
pub(super) fn thread_bound_snapshot_invalidation_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_snapshot_invalidation.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

fn runtime_thread_bound_budget_snapshot_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_budget_snapshot.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

fn runtime_thread_bound_snapshot_invalidation_shared_cache_path(
    repo_root: &Path,
    thread_id: &str,
) -> PathBuf {
    repo_root.join(format!(
        "state/observe/thread_bound_snapshot_invalidation.thread-{}.json",
        observe_cache_thread_suffix(thread_id)
    ))
}

fn load_shared_thread_bound_snapshot_invalidation(
    repo_root: &Path,
    thread_id: &str,
) -> Option<u64> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    let path = runtime_thread_bound_snapshot_invalidation_shared_cache_path(repo_root, thread_id);
    let bytes = fs::read(&path).ok()?;
    let persisted: Value = serde_json::from_slice(&bytes).ok()?;
    if persisted["cache_version"].as_str()
        != Some(THREAD_BOUND_SNAPSHOT_INVALIDATION_SHARED_CACHE_VERSION)
    {
        return None;
    }
    if persisted["thread_id"].as_str().map(str::trim) != Some(thread_id) {
        return None;
    }
    persisted["invalidated_at_epoch_ms"].as_u64()
}

fn load_shared_thread_bound_budget_snapshot(
    repo_root: &Path,
    now_epoch_ms: u64,
    thread_id: &str,
    max_age_ms: u64,
) -> Option<Value> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    let path = runtime_thread_bound_budget_snapshot_shared_cache_path(repo_root, thread_id);
    let bytes = fs::read(&path).ok()?;
    let persisted: Value = serde_json::from_slice(&bytes).ok()?;
    if persisted["cache_version"].as_str()
        != Some(THREAD_BOUND_BUDGET_SNAPSHOT_SHARED_CACHE_VERSION)
    {
        return None;
    }
    if persisted["thread_id"].as_str().map(str::trim) != Some(thread_id) {
        return None;
    }
    let fetched_at_epoch_ms = persisted["fetched_at_epoch_ms"].as_u64()?;
    if now_epoch_ms.saturating_sub(fetched_at_epoch_ms) > max_age_ms.max(1) {
        return None;
    }
    if load_shared_thread_bound_snapshot_invalidation(repo_root, thread_id)
        .is_some_and(|invalidated_at_epoch_ms| invalidated_at_epoch_ms >= fetched_at_epoch_ms)
    {
        return None;
    }
    Some(persisted["snapshot"].clone())
}

#[cfg(test)]
pub(super) fn reusable_exact_thread_budget_live_surfaces_from_thread_bound_snapshot(
    repo_root: &Path,
    thread_id: &str,
    now_epoch_ms: u64,
) -> Option<(Value, Value)> {
    let snapshot = load_shared_thread_bound_budget_snapshot(
        repo_root,
        now_epoch_ms,
        thread_id,
        DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
    )?;
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    reusable_exact_thread_budget_live_surfaces_from_base_report(
        Some(report),
        Some(thread_id),
        now_epoch_ms,
        DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
    )
}

pub(super) fn active_agent_budget_fields_from_thread_bound_snapshot(
    repo_root: &Path,
    selector: &PersonalKpiSelector,
    now_epoch_ms: u64,
) -> Option<(Value, Value)> {
    let thread_id = selector.thread_id.as_deref()?.trim();
    if thread_id.is_empty() {
        return None;
    }
    let snapshot = load_shared_thread_bound_budget_snapshot(
        repo_root,
        now_epoch_ms,
        thread_id,
        ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS as u64,
    )?;
    let report = if snapshot["token_budget_report"]["token_budget_report"].is_object() {
        &snapshot["token_budget_report"]["token_budget_report"]
    } else {
        &snapshot["token_budget_report"]
    };
    let (client_live_meter, _) = reusable_exact_thread_budget_live_surfaces_from_base_report(
        Some(report),
        Some(thread_id),
        now_epoch_ms,
        ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS as u64,
    )?;
    let personal_agent_kpi =
        preferred_personal_agent_kpi(&json!({}), Some(selector), Some(&client_live_meter));
    Some((client_live_meter, personal_agent_kpi))
}

pub(super) fn reusable_exact_thread_budget_live_surfaces_from_base_report(
    base_report: Option<&Value>,
    explicit_thread_id_hint: Option<&str>,
    now_epoch_ms: u64,
    max_live_age_ms: u64,
) -> Option<(Value, Value)> {
    let thread_id_hint = explicit_thread_id_hint?.trim();
    if thread_id_hint.is_empty() {
        return None;
    }
    let report = base_report?;
    let client_live_meter = report["client_live_meter"].clone();
    let current_live_turn = report["current_live_turn"].clone();
    if !client_live_meter.is_object() || !current_live_turn.is_object() {
        return None;
    }
    let meter_thread_id = client_live_meter["thread_id"]
        .as_str()
        .unwrap_or_default()
        .trim();
    let meter_bound = client_live_meter["current_thread_bound"]
        .as_bool()
        .unwrap_or(false);
    let turn_thread_id = current_live_turn["thread_id"]
        .as_str()
        .unwrap_or_default()
        .trim();
    if !meter_bound || meter_thread_id != thread_id_hint || turn_thread_id != thread_id_hint {
        return None;
    }
    if !reusable_exact_client_limits_from_live_meter_are_fresh(
        &client_live_meter,
        now_epoch_ms,
        max_live_age_ms,
    ) {
        return None;
    }
    Some((client_live_meter, current_live_turn))
}

pub(crate) async fn collect_live_current_session_budget_guard(
    db: &Client,
    restore_context: Option<&Value>,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let explicit_thread_id_hint = restore_context_thread_id_hint(restore_context);
    let latest_repo_working_state_restore = restore_context
        .cloned()
        .unwrap_or_else(|| json!({ "working_state_restore": {} }));
    let now_epoch_ms = current_epoch_ms().unwrap_or_default() as u64;

    if let Some(thread_id) = explicit_thread_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Some(snapshot) = load_shared_thread_bound_budget_snapshot(
            &repo_root,
            now_epoch_ms,
            thread_id,
            DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS,
        )
    {
        return Ok(dashboard::current_session_budget_guard(&json!({
            "token_budget_report": snapshot["token_budget_report"].clone(),
            "latest_repo_working_state_restore": latest_repo_working_state_restore,
        })));
    }

    working_state::maintain_same_thread_execctl_active_lease_for_guard(
        db,
        restore_context,
        explicit_thread_id_hint,
    )
    .await?;
    let report = collect_dashboard_current_session_budget_report_with_thread_hint_and_base(
        db,
        None,
        explicit_thread_id_hint,
    )
    .await?;
    let snapshot = json!({
        "token_budget_report": {
            "token_budget_report": report["token_budget_report"].clone(),
        },
        "latest_repo_working_state_restore": latest_repo_working_state_restore,
    });
    Ok(dashboard::current_session_budget_guard(&snapshot))
}

pub(super) fn restore_context_thread_id_hint(restore_context: Option<&Value>) -> Option<&str> {
    restore_context
        .and_then(|value| {
            value["working_state_restore"]["thread_id"]
                .as_str()
                .or_else(|| value["thread_id"].as_str())
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
