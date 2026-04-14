pub(crate) async fn collect_dashboard_report(db: &Client) -> Result<Value> {
    let collect_started_at = Instant::now();
    let mut pre_cache_timings = DashboardReportPreCacheTimings::default();

    let stage_started_at = Instant::now();
    let repo_root = config::discover_repo_root(None)?;
    let repo_root_str = repo_root
        .to_str()
        .ok_or_else(|| anyhow!("repo_root must be valid UTF-8"))?;
    let config = load_config(&repo_root)?;
    let profile = resolve_profile(&config, None, &repo_root)?;
    let client_budget_target_percent =
        client_budget_target_percent_for_repo(db, &repo_root).await?;
    let include_verify_events = config.measurement.include_verify_events_by_default;
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "config_and_profile",
        stage_started_at,
    );

    let stage_started_at = Instant::now();
    let (rollout_observation_signature, rollout_observations) =
        dashboard_rollout_assistant_generation_observations_for_repo(&repo_root)?;
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "rollout_observations",
        stage_started_at,
    );

    let stage_started_at = Instant::now();
    let mut events = load_dashboard_token_events(db, &repo_root, include_verify_events).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let mut events =
        reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
    record_dashboard_precache_stage_ms(&mut pre_cache_timings, "token_events", stage_started_at);

    let stage_started_at = Instant::now();
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let session_gap_ms = profile.session_gap_minutes.saturating_mul(60_000) as i64;
    let mut session_events = current_session_events(&events, session_gap_ms);
    let mut rolling_window_events = profile
        .rolling_window_hours
        .map(|hours| {
            let lower_bound = now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
            events
                .iter()
                .filter(|event| event.created_at_epoch_ms >= lower_bound)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let sync_scope_events = active_same_meter_scope_events(&session_events, &rolling_window_events);
    let dashboard_sync_signature =
        dashboard_same_meter_sync_signature(&sync_scope_events, &rollout_observation_signature);
    record_dashboard_precache_stage_ms(&mut pre_cache_timings, "scope_partition", stage_started_at);

    let stage_started_at = Instant::now();
    let (tool_overhead_changed, assistant_generation_changed, continuity_baseline_changed) =
        if should_run_dashboard_same_meter_sync(&repo_root, &dashboard_sync_signature) {
            (
                sync_context_pack_tool_overhead_for_events(db, &repo_root, &sync_scope_events)
                    .await?,
                sync_rollout_assistant_generation_for_events(
                    db,
                    &sync_scope_events,
                    &rollout_observations,
                )
                .await?,
                sync_continuity_pre_amai_baseline_for_events(db, &repo_root, &sync_scope_events)
                    .await?,
            )
        } else {
            (false, false, false)
        };
    record_dashboard_precache_stage_ms(&mut pre_cache_timings, "same_meter_sync", stage_started_at);

    if tool_overhead_changed || assistant_generation_changed || continuity_baseline_changed {
        let stage_started_at = Instant::now();
        let mut refreshed_events =
            load_dashboard_token_events(db, &repo_root, include_verify_events).await?;
        refreshed_events.sort_by_key(|event| event.created_at_epoch_ms);
        events = reconcile_followup_recovery(
            &refreshed_events,
            profile.session_gap_minutes as i64 * 60_000,
        );
        session_events = current_session_events(&events, session_gap_ms);
        rolling_window_events = profile
            .rolling_window_hours
            .map(|hours| {
                let lower_bound =
                    now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
                events
                    .iter()
                    .filter(|event| event.created_at_epoch_ms >= lower_bound)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        record_dashboard_precache_stage_ms(
            &mut pre_cache_timings,
            "token_events_reload",
            stage_started_at,
        );
    }

    let stage_started_at = Instant::now();
    let scope_events = if profile.rolling_window_hours.is_some() {
        vec![
            session_events.as_slice(),
            rolling_window_events.as_slice(),
            events.as_slice(),
        ]
    } else {
        vec![session_events.as_slice(), events.as_slice()]
    };
    let (scope_observations, assistant_scope_debug) =
        derive_dashboard_rollout_assistant_generation_scopes(db, &repo_root, &scope_events).await?;
    record_dashboard_precache_stage_ms(&mut pre_cache_timings, "assistant_scope", stage_started_at);

    let mut scope_iter = scope_observations.into_iter();
    let current_session_assistant_scope = scope_iter.next().unwrap_or_default();
    let rolling_window_assistant_scope = if profile.rolling_window_hours.is_some() {
        scope_iter.next()
    } else {
        None
    };
    let lifetime_assistant_scope = scope_iter.next().unwrap_or_default();
    let stage_started_at = Instant::now();
    let client_live_meter_binding_hint =
        preferred_dashboard_thread_binding_hint(db, &repo_root).await?;
    let client_live_meter_observation = preferred_rollout_client_meter_observation(
        db,
        &repo_root,
        repo_root_str,
        client_live_meter_binding_hint.as_deref(),
    )
    .await?;
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "client_live_meter_observation",
        stage_started_at,
    );

    let stage_started_at = Instant::now();
    let exact_client_limits_resolution = dashboard_exact_client_rate_limits_resolution().await?;
    let exact_client_limits_observation = exact_client_limits_resolution.observation.clone();
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "exact_client_limits_observation",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let client_limit_hourly_burn = collect_exact_client_limit_hourly_burn(
        db,
        exact_client_limits_observation.as_ref(),
        exact_client_limits_resolution
            .source
            .should_persist_exact_sample(),
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_WINDOW_MINUTES,
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MAX_LIVE_AGE_SECONDS,
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MIN_HISTORY_SPAN_MINUTES,
    )
    .await?;
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "client_limit_hourly_burn",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let current_live_turn = build_current_live_turn_surface(
        &repo_root,
        db,
        &events,
        client_live_meter_observation.as_ref(),
        client_live_meter_binding_hint.as_deref(),
        &config.measurement,
        &config.contract,
        &rollout_observations,
    )
    .await?;
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "current_live_turn",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let live_response_scope = current_workspace_live_response_scope(db, &repo_root).await?;
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "live_response_scope",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let mut live_response_latency = build_live_response_latency_surface(
        &repo_root,
        live_response_scope
            .as_ref()
            .map(|(project_code, _)| project_code.as_str()),
        live_response_scope
            .as_ref()
            .map(|(_, namespace_code)| namespace_code.as_str()),
        if profile.rolling_window_hours.is_some() {
            &rolling_window_events
        } else {
            &events
        },
        profile.rolling_window_hours,
        profile.session_gap_minutes,
        60,
        now_epoch_ms,
    )?;
    annotate_live_response_latency_surface(&mut live_response_latency, &current_live_turn);
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "live_response_latency",
        stage_started_at,
    );
    let personal_agent_scope =
        current_workspace_personal_kpi_selector(db, &repo_root, None).await?;
    let personal_agent_5h_events =
        personal_kpi_window_events(&events, personal_agent_scope.as_ref(), now_epoch_ms);

    let stage_started_at = Instant::now();
    let report_components = dashboard_report_signature_components(
        &session_events,
        &rolling_window_events,
        &events,
        personal_agent_scope.as_ref(),
        &personal_agent_5h_events,
        &current_session_assistant_scope,
        rolling_window_assistant_scope
            .as_ref()
            .and_then(|scope| materialized_assistant_scope(scope)),
        &lifetime_assistant_scope,
        client_live_meter_observation.as_ref(),
        client_live_meter_binding_hint.as_deref(),
        exact_client_limits_observation.as_ref(),
        &live_response_latency,
        client_budget_target_percent,
    );
    let report_signature = dashboard_report_signature(&report_components);
    record_dashboard_precache_stage_ms(
        &mut pre_cache_timings,
        "report_signature",
        stage_started_at,
    );
    pre_cache_timings.total_ms = collect_started_at.elapsed().as_millis() as u64;
    let cached_report_entry = cached_dashboard_report_entry(&repo_root);
    if let Some(entry) = cached_report_entry.as_ref() {
        if entry.signature == report_signature {
            return Ok(refresh_dashboard_report_live_ages(
                entry.report.clone(),
                now_epoch_ms,
                &session_events,
                &rolling_window_events,
                &events,
                &pre_cache_timings,
                &assistant_scope_debug,
            ));
        }
    }
    let cache_debug = dashboard_report_cache_debug(
        cached_report_entry.as_ref(),
        &report_signature,
        &report_components,
        &pre_cache_timings,
        &assistant_scope_debug,
    );
    let current_session_summary = summarize_events(
        &session_events,
        now_epoch_ms,
        &config.measurement,
        &config.contract,
    );
    let rolling_window_summary = if profile.rolling_window_hours.is_some() {
        summarize_events(
            &rolling_window_events,
            now_epoch_ms,
            &config.measurement,
            &config.contract,
        )
    } else {
        Value::Null
    };
    let lifetime_summary =
        summarize_events(&events, now_epoch_ms, &config.measurement, &config.contract);
    let personal_agent_5h_summary = summarize_events(
        &personal_agent_5h_events,
        now_epoch_ms,
        &config.measurement,
        &config.contract,
    );
    let client_live_meter = build_client_live_meter_json(
        client_live_meter_observation.as_ref(),
        client_live_meter_binding_hint.as_deref(),
        exact_client_limits_observation.as_ref(),
    );
    let personal_agent_kpi = preferred_personal_agent_kpi(
        &personal_agent_5h_summary,
        personal_agent_scope.as_ref(),
        Some(&client_live_meter),
    );
    let DashboardReadOnlyStatementSurfaces {
        current_session_statement_preview,
        rolling_window_statement_preview,
        lifetime_statement_preview,
        current_session_statement_export_preview,
        rolling_window_statement_export_preview,
        lifetime_statement_export_preview,
        headline_summary,
    } = build_dashboard_read_only_statement_surfaces(
        &profile,
        &config.contract,
        &rollout_observations,
        &current_session_summary,
        &session_events,
        materialized_assistant_scope(&current_session_assistant_scope),
        &rolling_window_summary,
        &rolling_window_events,
        rolling_window_assistant_scope
            .as_ref()
            .and_then(|scope| materialized_assistant_scope(scope)),
        &lifetime_summary,
        &events,
        materialized_assistant_scope(&lifetime_assistant_scope),
        client_budget_target_percent,
    );
    let mut report = json!({
        "token_budget_report": {
            "surface": "dashboard_read_only",
            "profile": {
                "code": profile.code,
                "display_name": profile.display_name,
                "description": profile.description,
                "session_gap_minutes": profile.session_gap_minutes,
                "rolling_window_hours": profile.rolling_window_hours,
            },
            "filters": {
                "include_verify_events": include_verify_events,
            },
            "client_budget_target_percent": client_budget_target_percent,
            "headline": headline_summary,
            "current_session": current_session_summary,
            "rolling_window": rolling_window_summary,
            "lifetime": lifetime_summary,
            "statement_previews": {
                "current_session": current_session_statement_preview,
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_statement_preview
                } else {
                    Value::Null
                },
                "lifetime": lifetime_statement_preview,
            },
            "statement_export_previews": {
                "current_session": current_session_statement_export_preview,
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_statement_export_preview
                } else {
                    Value::Null
                },
                "lifetime": lifetime_statement_export_preview,
            },
            "client_live_meter": client_live_meter,
            "personal_agent_kpi": personal_agent_kpi,
            "client_limit_hourly_burn": client_limit_hourly_burn,
            "current_live_turn": current_live_turn,
            "live_response_latency": live_response_latency,
            "cache_debug": cache_debug,
            "note": "Это облегчённый dashboard report: он сохраняет честные current_session / rolling_window / lifetime rollups, client-limit alignment и live client meter, не разворачивает полный contractual/export contour, но поднимает compact statement_export_previews для reviewed frozen-debt surfaces. Quiet same-meter sync/write-back всё так же ограничен active live scope текущей сессии и рабочего окна. Sync write-back может materialize-иться в этом тике, а обновлённые token events подхватываются следующим refresh, чтобы текущий pass не делал лишний full reload.",
        }
    });
    if let Some(node) = report["token_budget_report"]["cache_debug"].as_object_mut() {
        node.insert("status".to_string(), Value::from("miss"));
    }
    store_dashboard_report(&repo_root, &report_signature, &report_components, &report);
    Ok(report)
}
