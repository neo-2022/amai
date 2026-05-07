use super::*;

pub(crate) fn live_turn_token_budget_events(
    events: &[TokenBudgetEvent],
    thread_id: &str,
    turn_id: &str,
    context_pack_ids: &BTreeSet<String>,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    grace_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let thread_id = thread_id.trim();
    let turn_id = turn_id.trim();
    let live_turn_bounds = current_live_turn_context_pack_match_bounds(
        started_at_epoch_ms,
        ended_at_epoch_ms,
        grace_ms,
    );
    let mut seen_event_ids = HashSet::new();
    let mut matched = Vec::new();
    for event in events {
        if event.traffic_class != "live" {
            continue;
        }
        let thread_turn_match = !thread_id.is_empty()
            && !turn_id.is_empty()
            && event.thread_id.as_deref().map(str::trim) == Some(thread_id)
            && event.turn_id.as_deref().map(str::trim) == Some(turn_id);
        let context_pack_match = event
            .context_pack_id
            .as_deref()
            .is_some_and(|value| context_pack_ids.contains(value));
        let continuity_restore_match = !thread_id.is_empty()
            && event.thread_id.as_deref().map(str::trim) == Some(thread_id)
            && is_live_continuity_restore_event(event)
            && live_turn_bounds.is_some_and(|(lower_bound, upper_bound)| {
                let observed_at_epoch_ms = if event.occurred_at_epoch_ms > 0 {
                    event.occurred_at_epoch_ms
                } else {
                    event.created_at_epoch_ms
                };
                observed_at_epoch_ms >= lower_bound && observed_at_epoch_ms <= upper_bound
            });
        if !(thread_turn_match || context_pack_match || continuity_restore_match) {
            continue;
        }
        if seen_event_ids.insert(event.event_id.clone()) {
            matched.push(event.clone());
        }
    }
    matched.sort_by_key(|event| event.created_at_epoch_ms);
    matched
}

pub(crate) async fn build_current_live_turn_surface(
    repo_root: &Path,
    db: &Client,
    events: &[TokenBudgetEvent],
    client_live_meter_observation: Option<&codex_threads::RolloutClientMeterObservation>,
    client_live_meter_binding_hint: Option<&str>,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
) -> Result<Value> {
    let Some(observation) = client_live_meter_observation else {
        return Ok(json!({
            "status": "missing",
            "scope_code": "current_live_turn",
            "scope_label": "текущий live-turn",
            "thread_binding_state": "missing_rollout_client_meter",
            "current_thread_bound": false,
            "exact_pair_available": false,
            "exact_pair": Value::Null,
            "note": "Live rollout client meter для текущего turn ещё не materialized."
        }));
    };
    let (thread_binding_state, current_thread_bound) = client_live_meter_thread_binding_state(
        client_live_meter_binding_hint,
        &observation.thread_id,
    );
    let mut surface = json!({
        "status": "observed",
        "scope_code": "current_live_turn",
        "scope_label": "текущий live-turn",
        "thread_id": observation.thread_id,
        "turn_id": observation.turn_id,
        "started_at_epoch_ms": observation.started_at_epoch_ms,
        "ended_at_epoch_ms": observation.ended_at_epoch_ms,
        "thread_binding_state": thread_binding_state,
        "current_thread_bound": current_thread_bound,
        "exact_pair_available": false,
        "exact_pair": Value::Null,
        "matched_events_count": 0,
        "matched_context_pack_ids_count": 0,
        "retrieval_context_pack_count": 0,
        "note": "Current live turn surface is built only for the thread-bound rollout meter of the active chat."
    });
    if !current_thread_bound {
        surface["status"] = Value::from("current_thread_unbound");
        surface["note"] = Value::from(
            "Current-thread binding для live meter ещё не materialized, поэтому exact full-turn pair по текущему чату честно не доказывается.",
        );
        return Ok(surface);
    }
    if observation.started_at_epoch_ms <= 0 || observation.ended_at_epoch_ms <= 0 {
        surface["status"] = Value::from("invalid_live_turn_window");
        surface["note"] = Value::from(
            "У live meter отсутствует валидное окно текущего turn, поэтому точный full-turn процент пока не доказывается.",
        );
        return Ok(surface);
    }

    let grace_ms = current_live_turn_context_pack_match_grace_ms();
    let (matched_context_pack_ids, retrieval_context_pack_count) =
        live_turn_retrieval_context_pack_ids(
            repo_root,
            db,
            &observation.thread_id,
            observation.started_at_epoch_ms,
            observation.ended_at_epoch_ms,
            grace_ms,
        )
        .await?;
    let matched_events = live_turn_token_budget_events(
        events,
        &observation.thread_id,
        &observation.turn_id,
        &matched_context_pack_ids,
        observation.started_at_epoch_ms,
        observation.ended_at_epoch_ms,
        grace_ms,
    );
    surface["matched_events_count"] = Value::from(matched_events.len() as u64);
    surface["matched_context_pack_ids_count"] = Value::from(matched_context_pack_ids.len() as u64);
    surface["retrieval_context_pack_count"] = Value::from(retrieval_context_pack_count);

    if matched_events.is_empty() && retrieval_context_pack_count == 0 {
        let (pending_context_pack_ids, pending_retrieval_context_pack_count) =
            recent_thread_live_retrieval_context_pack_ids_after_turn(
                db,
                &observation.thread_id,
                observation.ended_at_epoch_ms,
                grace_ms,
            )
            .await?;
        if apply_open_turn_pending_activity_surface(
            &mut surface,
            pending_context_pack_ids.len() as u64,
            pending_retrieval_context_pack_count,
        ) {
            return Ok(surface);
        }
        surface["status"] = Value::from("no_amai_activity_in_current_live_turn");
        surface["exact_pair_available"] = Value::from(true);
        surface["exact_pair"] = json!({
            "without_amai_tokens": 0,
            "with_amai_tokens": 0,
            "saved_tokens": 0,
            "saved_pct": 0.0
        });
        surface["note"] = Value::from(
            "В текущем live-turn не наблюдалось ни одного retrieval_context_pack от Amai, поэтому честный вклад Amai в шкалу VS Code сейчас равен 0.00%.",
        );
        return Ok(surface);
    }

    if matched_events.is_empty() {
        surface["status"] = Value::from("activity_observed_exact_pair_unavailable");
        surface["note"] = Value::from(
            "Amai-активность в текущем live-turn уже observed по working_state, но соответствующие token_budget_event exact-pair для этого turn ещё не materialized.",
        );
        return Ok(surface);
    }

    let assistant_scope = current_live_turn_assistant_scope_from_client_meter(
        &matched_events,
        &matched_context_pack_ids,
        observation,
    );
    let summary = summarize_events(
        &matched_events,
        observation.ended_at_epoch_ms,
        measurement,
        contract,
    );
    let meter_summary = current_live_turn_meter_summary(&summary);
    let statement_preview = build_dashboard_statement_preview(
        "current_live_turn",
        "текущий live-turn",
        &meter_summary,
        &matched_events,
        contract,
        rollout_observations,
        assistant_scope.as_ref(),
    );
    surface["events_total"] = meter_summary["events_total"].clone();
    surface["counted_events"] = meter_summary["counted_events"].clone();
    surface["meter_counted_events"] = meter_summary["meter_counted_events"].clone();
    if let Some((without_amai_tokens, with_amai_tokens, saved_tokens, saved_pct)) =
        current_live_turn_full_turn_exact_pair(&statement_preview, observation)
    {
        surface["status"] = Value::from("exact_pair_materialized");
        surface["exact_pair_available"] = Value::from(true);
        surface["exact_pair"] = json!({
            "without_amai_tokens": without_amai_tokens,
            "with_amai_tokens": with_amai_tokens,
            "saved_tokens": saved_tokens,
            "saved_pct": saved_pct
        });
        surface["note"] = Value::from(
            "Exact full-turn pair materialized from the actual VS Code meter by substituting only the Amai-specific retrieval and orchestration parts with their truthful no-Amai baseline.",
        );
    } else {
        surface["status"] = Value::from("activity_observed_exact_pair_unavailable");
        surface["note"] = Value::from(
            "Amai-активность в текущем live-turn уже observed, но same-meter exact pair для этой live turn выборки ещё не materialized.",
        );
    }
    Ok(surface)
}

pub(crate) async fn collect_report(
    repo_root: &Path,
    db: &Client,
    requested_profile: Option<&str>,
    include_verify_events: bool,
    limit: Option<i64>,
) -> Result<Value> {
    let config = load_config(repo_root)?;
    let profile = resolve_profile(&config, requested_profile, repo_root)?;
    let client_budget_target_percent = client_budget_target_percent_for_repo(db, repo_root).await?;
    let repo_root_str = repo_root
        .to_str()
        .ok_or_else(|| anyhow!("repo_root must be valid UTF-8"))?;
    let rollout_observations = rollout_assistant_generation_observations_for_repo(repo_root)?;
    let mut events = if limit.is_some() {
        load_events(db, include_verify_events, limit).await?
    } else {
        load_dashboard_token_events(db, repo_root, include_verify_events).await?
    };
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let mut events =
        reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
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
    let tool_overhead_changed =
        sync_context_pack_tool_overhead_for_events(db, repo_root, &events).await?;
    let assistant_generation_changed =
        sync_rollout_assistant_generation_for_events(db, &events, &rollout_observations).await?;
    let continuity_baseline_changed =
        sync_continuity_pre_amai_baseline_for_events(db, repo_root, &events).await?;
    if tool_overhead_changed || assistant_generation_changed || continuity_baseline_changed {
        let mut refreshed = if limit.is_some() {
            load_events(db, include_verify_events, limit).await?
        } else {
            load_dashboard_token_events(db, repo_root, include_verify_events).await?
        };
        refreshed.sort_by_key(|event| event.created_at_epoch_ms);
        events =
            reconcile_followup_recovery(&refreshed, profile.session_gap_minutes as i64 * 60_000);
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
    }

    let current_session_assistant_scope =
        derive_rollout_assistant_generation_scope(db, &session_events).await?;
    let rolling_window_assistant_scope = if profile.rolling_window_hours.is_some() {
        Some(derive_rollout_assistant_generation_scope(db, &rolling_window_events).await?)
    } else {
        None
    };
    let lifetime_assistant_scope = derive_rollout_assistant_generation_scope(db, &events).await?;
    let client_live_meter_binding_hint =
        preferred_dashboard_thread_binding_hint(db, repo_root).await?;
    let client_live_meter_observation = preferred_rollout_client_meter_observation(
        db,
        repo_root,
        repo_root_str,
        client_live_meter_binding_hint.as_deref(),
    )
    .await?;
    let exact_client_limits_observation = dashboard_exact_client_rate_limits_resolution()
        .await?
        .observation;
    let current_live_turn = build_current_live_turn_surface(
        repo_root,
        db,
        &events,
        client_live_meter_observation.as_ref(),
        client_live_meter_binding_hint.as_deref(),
        &config.measurement,
        &config.contract,
        &rollout_observations,
    )
    .await?;
    let personal_agent_scope = current_workspace_personal_kpi_selector(db, repo_root, None).await?;
    let personal_agent_5h_events =
        personal_kpi_window_events(&events, personal_agent_scope.as_ref(), now_epoch_ms);

    let latest_event = events
        .last()
        .map(event_to_json)
        .unwrap_or_else(|| json!(null));
    let source_breakdown = source_breakdown(&events, &config.measurement, &config.contract);
    let query_slices = query_slice_breakdown(&events, &config.measurement, &config.contract);
    let baseline_strategy_slices =
        baseline_strategy_breakdown(&events, &config.measurement, &config.contract);
    let temperature_slices =
        temperature_slice_breakdown(&events, &config.measurement, &config.contract);
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
        json!(null)
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
    let agent_cycle_economics = build_agent_cycle_economics(
        &config.measurement,
        &config.contract,
        now_epoch_ms,
        &session_events,
        profile
            .rolling_window_hours
            .map(|_| rolling_window_events.as_slice()),
        &events,
        &profile.display_name,
        &rollout_observations,
        &current_session_assistant_scope,
        rolling_window_assistant_scope
            .as_ref()
            .and_then(|scope| materialized_assistant_scope(scope)),
        materialized_assistant_scope(&lifetime_assistant_scope),
    );
    let current_session_metering_freshness = build_metering_freshness_summary(
        &config.contract,
        &config.measurement,
        now_epoch_ms,
        &session_events,
    );
    let rolling_window_metering_freshness = if profile.rolling_window_hours.is_some() {
        build_metering_freshness_summary(
            &config.contract,
            &config.measurement,
            now_epoch_ms,
            &rolling_window_events,
        )
    } else {
        Value::Null
    };
    let lifetime_metering_freshness = build_metering_freshness_summary(
        &config.contract,
        &config.measurement,
        now_epoch_ms,
        &events,
    );
    let external_truth_sources = external_truth::build_external_truth_sources_json(repo_root);
    let rate_card = external_truth::build_rate_card_json(repo_root, &config.contract);
    let provider_usage_binding = external_truth::load_provider_usage_binding_from_source(
        &external_truth_sources["provider_usage_export"],
        &rate_card,
    );
    let provider_invoice_binding = external_truth::load_provider_invoice_binding_from_source(
        &external_truth_sources["provider_invoice_export"],
    );
    let reconciliation_contract = build_reconciliation_contract_json(
        &config.contract,
        &external_truth_sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let adjustment_registry =
        token_adjustments::build_adjustment_registry_json(repo_root, &config.contract);
    let infra_cost_profile =
        external_truth::build_infra_cost_profile_json(repo_root, &config.contract);
    let current_session_statement_preview = build_statement_preview(
        "current_session",
        "текущая сессия",
        now_epoch_ms,
        &session_events,
        &profile,
        &current_session_summary,
        &config.contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &current_session_metering_freshness,
        &rollout_observations,
        materialized_assistant_scope(&current_session_assistant_scope),
    );
    let rolling_window_statement_preview = if profile.rolling_window_hours.is_some() {
        build_statement_preview(
            "rolling_window",
            &format!("окно {}", profile.display_name),
            now_epoch_ms,
            &rolling_window_events,
            &profile,
            &rolling_window_summary,
            &config.contract,
            &adjustment_registry,
            &rate_card,
            &reconciliation_contract,
            &rolling_window_metering_freshness,
            &rollout_observations,
            rolling_window_assistant_scope
                .as_ref()
                .and_then(|scope| materialized_assistant_scope(scope)),
        )
    } else {
        Value::Null
    };
    let lifetime_statement_preview = build_statement_preview(
        "lifetime",
        "всё время записи",
        now_epoch_ms,
        &events,
        &profile,
        &lifetime_summary,
        &config.contract,
        &adjustment_registry,
        &rate_card,
        &reconciliation_contract,
        &lifetime_metering_freshness,
        &rollout_observations,
        materialized_assistant_scope(&lifetime_assistant_scope),
    );
    let current_session_reconciliation_preview = build_reconciliation_preview(
        "current_session",
        "текущая сессия",
        &current_session_statement_preview,
        &config.contract,
        &external_truth_sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let rolling_window_reconciliation_preview = if profile.rolling_window_hours.is_some() {
        build_reconciliation_preview(
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_statement_preview,
            &config.contract,
            &external_truth_sources,
            &provider_usage_binding,
            &provider_invoice_binding,
            &rate_card,
        )
    } else {
        Value::Null
    };
    let lifetime_reconciliation_preview = build_reconciliation_preview(
        "lifetime",
        "всё время записи",
        &lifetime_statement_preview,
        &config.contract,
        &external_truth_sources,
        &provider_usage_binding,
        &provider_invoice_binding,
        &rate_card,
    );
    let margin_contract = build_margin_contract_json(
        &config.contract,
        &external_truth_sources,
        &rate_card,
        &infra_cost_profile,
        &reconciliation_contract,
    );
    let current_session_margin_scope = build_margin_scope(
        &external_truth_sources,
        "current_session",
        "текущая сессия",
        &current_session_statement_preview,
        &current_session_reconciliation_preview,
        &rate_card,
        &infra_cost_profile,
    );
    let rolling_window_margin_scope = if profile.rolling_window_hours.is_some() {
        build_margin_scope(
            &external_truth_sources,
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_statement_preview,
            &rolling_window_reconciliation_preview,
            &rate_card,
            &infra_cost_profile,
        )
    } else {
        Value::Null
    };
    let lifetime_margin_scope = build_margin_scope(
        &external_truth_sources,
        "lifetime",
        "всё время записи",
        &lifetime_statement_preview,
        &lifetime_reconciliation_preview,
        &rate_card,
        &infra_cost_profile,
    );
    let current_session_contractual_summary = build_contractual_statement_summary(
        &config.contract,
        "current_session",
        "текущая сессия",
        &current_session_statement_preview,
        &current_session_reconciliation_preview,
        &current_session_margin_scope,
        &current_session_metering_freshness,
    );
    let rolling_window_contractual_summary = if profile.rolling_window_hours.is_some() {
        build_contractual_statement_summary(
            &config.contract,
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_statement_preview,
            &rolling_window_reconciliation_preview,
            &rolling_window_margin_scope,
            &rolling_window_metering_freshness,
        )
    } else {
        Value::Null
    };
    let lifetime_contractual_summary = build_contractual_statement_summary(
        &config.contract,
        "lifetime",
        "всё время записи",
        &lifetime_statement_preview,
        &lifetime_reconciliation_preview,
        &lifetime_margin_scope,
        &lifetime_metering_freshness,
    );
    let external_truth_manifest = build_external_truth_manifest(
        &config.contract,
        &rate_card,
        &infra_cost_profile,
        &provider_usage_binding,
        &provider_invoice_binding,
        &adjustment_registry,
    );
    let current_session_statement_export = build_statement_export_preview(
        &json!({
            "token_budget_report": {
                "external_truth_manifest": external_truth_manifest.clone(),
                "statement_previews": {
                    "current_session": current_session_statement_preview.clone(),
                },
                "reconciliation_previews": {
                    "current_session": current_session_reconciliation_preview.clone(),
                },
                "margin_view": {
                    "current_session": current_session_margin_scope.clone(),
                },
                "contractual_statement_summaries": {
                    "current_session": current_session_contractual_summary.clone(),
                }
            }
        }),
        "current_session",
        "текущая сессия",
        &session_events,
        &config.contract,
        include_verify_events,
    )?;
    let rolling_window_statement_export = if profile.rolling_window_hours.is_some() {
        build_statement_export_preview(
            &json!({
                "token_budget_report": {
                    "external_truth_manifest": external_truth_manifest.clone(),
                    "statement_previews": {
                        "rolling_window": rolling_window_statement_preview.clone(),
                    },
                    "reconciliation_previews": {
                        "rolling_window": rolling_window_reconciliation_preview.clone(),
                    },
                    "margin_view": {
                        "rolling_window": rolling_window_margin_scope.clone(),
                    },
                    "contractual_statement_summaries": {
                        "rolling_window": rolling_window_contractual_summary.clone(),
                    }
                }
            }),
            "rolling_window",
            &format!("окно {}", profile.display_name),
            &rolling_window_events,
            &config.contract,
            include_verify_events,
        )?
    } else {
        Value::Null
    };
    let lifetime_statement_export = build_statement_export_preview(
        &json!({
            "token_budget_report": {
                "external_truth_manifest": external_truth_manifest.clone(),
                "statement_previews": {
                    "lifetime": lifetime_statement_preview.clone(),
                },
                "reconciliation_previews": {
                    "lifetime": lifetime_reconciliation_preview.clone(),
                },
                "margin_view": {
                    "lifetime": lifetime_margin_scope.clone(),
                },
                "contractual_statement_summaries": {
                    "lifetime": lifetime_contractual_summary.clone(),
                }
            }
        }),
        "lifetime",
        "всё время записи",
        &events,
        &config.contract,
        include_verify_events,
    )?;
    let current_session_settlement_report_preview =
        current_session_statement_export["settlement_report_preview"].clone();
    let rolling_window_settlement_report_preview = if profile.rolling_window_hours.is_some() {
        rolling_window_statement_export["settlement_report_preview"].clone()
    } else {
        Value::Null
    };
    let lifetime_settlement_report_preview =
        lifetime_statement_export["settlement_report_preview"].clone();
    let headline_summary = if profile.rolling_window_hours.is_some() {
        build_product_headline_with_target(
            &rolling_window_summary,
            &format!("окно {}", profile.display_name),
            Some(&rolling_window_contractual_summary["client_limit_boundary_semantics"]),
            client_budget_target_percent,
        )
    } else {
        build_product_headline_with_target(
            &lifetime_summary,
            "всё время записи",
            Some(&lifetime_contractual_summary["client_limit_boundary_semantics"]),
            client_budget_target_percent,
        )
    };

    Ok(json!({
        "token_budget_report": {
            "profile": {
                "code": profile.code,
                "display_name": profile.display_name,
                "description": profile.description,
                "session_gap_minutes": profile.session_gap_minutes,
                "rolling_window_hours": profile.rolling_window_hours,
                "metering_ingest_warning_seconds": config.measurement.metering_ingest_warning_seconds,
                "metering_ingest_slo_seconds": config.measurement.metering_ingest_slo_seconds,
                "late_arrival_grace_minutes": config.measurement.late_arrival_grace_minutes,
                "preliminary_min_events": config.measurement.preliminary_min_events,
                "preliminary_min_baseline_tokens": config.measurement.preliminary_min_baseline_tokens,
            },
            "client_budget_target_percent": client_budget_target_percent,
            "contract": report_contract_json(&config.contract),
            "usage_event_schema": build_usage_event_schema_json(&config.contract),
            "metering_freshness_contract": build_metering_freshness_contract_json(&config.contract, &config.measurement),
            "baseline_contract": build_baseline_contract_json(&config.contract),
            "billing_policy": build_billing_policy_json(&config.contract, &config.measurement),
            "suitability_contract": build_suitability_contract_json(&config.contract),
            "rate_card": rate_card.clone(),
            "settlement_contract": build_settlement_contract_json(&config.contract),
            "telemetry_surfaces": build_telemetry_surfaces_json(&config.contract),
            "adjustment_request_schema": token_adjustments::build_adjustment_request_schema_json(&config.contract),
            "adjustment_registry": adjustment_registry.clone(),
            "reconciliation_contract": reconciliation_contract.clone(),
            "external_truth_sources": external_truth_sources.clone(),
            "external_truth_manifest": external_truth_manifest,
            "provider_usage_binding": provider_usage_binding.clone(),
            "provider_invoice_binding": provider_invoice_binding.clone(),
            "infra_cost_profile": infra_cost_profile.clone(),
            "margin_contract": margin_contract.clone(),
            "filters": {
                "include_verify_events": include_verify_events,
            },
            "headline": headline_summary,
            "latest_event": latest_event,
            "current_session": current_session_summary,
            "rolling_window": rolling_window_summary,
            "lifetime": lifetime_summary,
            "agent_cycle_economics": agent_cycle_economics,
            "metering_freshness": {
                "current_session": current_session_metering_freshness.clone(),
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_metering_freshness.clone()
                } else {
                    Value::Null
                },
                "lifetime": lifetime_metering_freshness.clone(),
            },
            "statement_previews": {
                "current_session": current_session_statement_preview.clone(),
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_statement_preview.clone()
                } else {
                    Value::Null
                },
                "lifetime": lifetime_statement_preview.clone(),
            },
            "reconciliation_previews": {
                "current_session": current_session_reconciliation_preview.clone(),
                "rolling_window": if profile.rolling_window_hours.is_some() {
                    rolling_window_reconciliation_preview.clone()
                } else {
                    Value::Null
                },
                "lifetime": lifetime_reconciliation_preview.clone(),
            },
            "margin_view": {
                "model_version": config.contract.margin_model_version.clone(),
                "status": margin_contract["status"].clone(),
                "current_session": current_session_margin_scope.clone(),
                "rolling_window": rolling_window_margin_scope.clone(),
                "lifetime": lifetime_margin_scope.clone(),
            },
            "contractual_statement_summaries": {
                "current_session": current_session_contractual_summary,
                "rolling_window": rolling_window_contractual_summary,
                "lifetime": lifetime_contractual_summary,
            },
            "statement_export_previews": {
                "current_session": current_session_statement_export,
                "rolling_window": rolling_window_statement_export,
                "lifetime": lifetime_statement_export,
            },
            "settlement_report_previews": {
                "current_session": current_session_settlement_report_preview,
                "rolling_window": rolling_window_settlement_report_preview,
                "lifetime": lifetime_settlement_report_preview,
            },
            "client_live_meter": client_live_meter,
            "personal_agent_kpi": personal_agent_kpi,
            "current_live_turn": current_live_turn,
            "source_breakdown": source_breakdown,
            "query_slices": query_slices,
            "baseline_strategy_slices": baseline_strategy_slices,
            "temperature_slices": temperature_slices,
        }
    }))
}

pub(crate) async fn enrich_live_event_payload(
    db: &Client,
    payload: &mut Value,
    profile: &ResolvedProfile,
    repo_root: &Path,
) -> Result<()> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
    let timestamp_utc = node
        .get("timestamp_utc")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| current_epoch_ms().unwrap_or_default());
    let current_event_id = node
        .get("event_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let project = node
        .get("project")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let namespace = node
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query = node
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_hash = node
        .get("query_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_type = node
        .get("query_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let target_kind = node
        .get("target_kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let current_source_kind = node
        .get("source_kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let is_continuity_restore_event =
        query_type == "continuity_restore" || target_kind == "continuity_restore";
    let current_agent_scope = working_state::current_agent_scope_for(&project, &namespace);
    let session_gap_ms = profile.session_gap_minutes as i64 * 60_000;
    let session_lookup_limit = if is_continuity_restore_event { 8 } else { 64 };
    let mut events = load_events(db, false, Some(session_lookup_limit)).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let session_id = resolve_session_id(
        &events,
        timestamp_utc,
        session_gap_ms,
        &current_source_kind,
        &project,
        &namespace,
        &current_agent_scope,
    );
    node.insert(
        "agent_scope".to_string(),
        Value::String(current_agent_scope.clone()),
    );
    node.insert("session_id".to_string(), Value::String(session_id));
    let thread_id = node
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            codex_threads::current_thread_id()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .or(preferred_dashboard_thread_binding_hint(db, repo_root).await?);
    if let Some(thread_id) = thread_id {
        node.insert("thread_id".to_string(), Value::String(thread_id.clone()));
        let turn_id_missing = node
            .get("turn_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none();
        if turn_id_missing {
            if let Ok(Some(observation)) =
                codex_threads::latest_rollout_client_meter_observation_for_thread(&thread_id)
            {
                if !observation.turn_id.trim().is_empty() {
                    node.insert("turn_id".to_string(), Value::String(observation.turn_id));
                }
            }
        }
    }
    node.insert(
        "rolling_window_profile".to_string(),
        Value::String(profile.code.clone()),
    );
    node.insert(
        "budget_profile".to_string(),
        Value::String(profile.code.clone()),
    );

    if is_continuity_restore_event {
        return Ok(());
    }

    let current_key = FollowupEventKey {
        query: &query,
        query_hash: &query_hash,
        query_type: &query_type,
        target_kind: &target_kind,
    };

    let candidate_rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], Some(64))
            .await?;
    let mut candidates = candidate_rows
        .into_iter()
        .filter_map(|row| {
            parse_snapshot_event(&row)
                .ok()
                .flatten()
                .filter(|event| event.traffic_class == "live")
                .map(|event| (row, event))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, event)| event.created_at_epoch_ms);

    if let Some((row, previous)) = candidates.into_iter().rev().find(|(_, previous)| {
        previous.traffic_class == "live"
            && previous.needed_followup
            && previous.resolved_by_event_id.is_none()
            && previous.project == project
            && previous.namespace == namespace
            && previous.agent_scope == current_agent_scope
            && timestamp_utc.saturating_sub(previous.created_at_epoch_ms) <= session_gap_ms
            && followup_queries_related(followup_event_key(previous), current_key)
    }) {
        let previous_cost = previous
            .context_tokens
            .saturating_add(previous.recovery_tokens);
        set_recovery_penalty(
            payload,
            previous_cost,
            previous.followup_count.saturating_add(1),
        )?;
        let exact_hits = payload["retrieval"]["exact_documents"]
            .as_array()
            .map_or(0, Vec::len);
        let symbol_hits = payload["retrieval"]["symbol_hits"]
            .as_array()
            .map_or(0, Vec::len);
        let lexical_hits = payload["retrieval"]["lexical_chunks"]
            .as_array()
            .map_or(0, Vec::len);
        let semantic_hits = payload["retrieval"]["semantic_chunks"]
            .as_array()
            .map_or(0, Vec::len);
        let target_kind_owned = payload["token_budget_event"]["target_kind"]
            .as_str()
            .unwrap_or("file")
            .to_string();
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
        let followup = ensure_nested_object(node, "followup")?;
        followup.insert(
            "followup_of_event_id".to_string(),
            Value::String(previous.event_id.clone()),
        );
        let quality = ensure_nested_object(node, "quality")?;
        let quality_ok = quality
            .get("quality_ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let head_hit_target = quality
            .get("head_hit_target")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer_like_proxy = answer_like_from_counts(
            &target_kind_owned,
            head_hit_target,
            exact_hits,
            symbol_hits,
            lexical_hits,
            semantic_hits,
        );
        quality.insert(
            "quality_method".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "hybrid_answer_success".to_string()
                } else {
                    "hybrid_task_success".to_string()
                }
            } else {
                "hybrid_followup_pending".to_string()
            }),
        );
        quality.insert(
            "quality_tier".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "answer_success_recovered".to_string()
                } else {
                    "task_success_recovered".to_string()
                }
            } else {
                "partial".to_string()
            }),
        );

        let mut previous_payload = row.payload.clone();
        let previous_node = previous_payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("previous token budget payload missing token_budget_event"))?;
        let previous_followup = ensure_nested_object(previous_node, "followup")?;
        previous_followup.insert(
            "resolved_by_event_id".to_string(),
            Value::String(current_event_id),
        );
        previous_followup.insert("recovery_resolved".to_string(), Value::Bool(true));
        previous_followup.insert(
            "recovery_resolved_at_utc".to_string(),
            Value::from(timestamp_utc),
        );
        postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &previous_payload)
            .await?;
    }

    Ok(())
}
