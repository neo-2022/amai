use super::*;

pub(crate) async fn collect_default_report_with_overrides(
    db: &Client,
    requested_profile: Option<&str>,
    include_verify_events: Option<bool>,
) -> Result<Value> {
    if requested_profile.is_none() && include_verify_events.is_none() {
        return collect_default_report(db).await;
    }
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    collect_report(
        &repo_root,
        db,
        requested_profile,
        include_verify_events.unwrap_or(config.measurement.include_verify_events_by_default),
        None,
    )
    .await
}

pub(crate) async fn record_context_pack_event(
    db: &Client,
    payload: &Value,
    source_kind: &str,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let traffic_class = derive_traffic_class(source_kind);
    let payload_origin = if traffic_class == "live" {
        "context_pack_token_budget_v13"
    } else {
        source_kind
    };
    let mut event = build_event_payload(
        payload,
        &config.measurement,
        &config.contract,
        source_kind,
        payload_origin,
    )?;
    if let Some(node) = event["token_budget_event"].as_object_mut() {
        let thread_id_missing = node
            .get("thread_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none();
        if thread_id_missing
            && let Some(thread_id) = preferred_dashboard_thread_binding_hint(db, &repo_root).await?
        {
            node.insert("thread_id".to_string(), Value::String(thread_id.clone()));
            let turn_id_missing = node
                .get("turn_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none();
            if turn_id_missing
                && let Ok(Some(observation)) =
                    codex_threads::latest_rollout_client_meter_observation_for_thread(&thread_id)
                && !observation.turn_id.trim().is_empty()
            {
                node.insert("turn_id".to_string(), Value::String(observation.turn_id));
            }
        }
    }
    if traffic_class == "live" {
        let profile = resolve_profile(&config, None, &repo_root)?;
        enrich_live_event_payload(db, &mut event, &profile, &repo_root).await?;
    }
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}

pub(crate) async fn observe_context_pack_tool_overhead(
    db: &Client,
    context_pack_id: &str,
    text: &str,
    structured_content: &Value,
) -> Result<bool> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let tool_overhead_tokens =
        count_tool_overhead_tokens(&config.measurement, text, structured_content)?;
    let Some(row) = latest_token_budget_snapshot_for_context_pack(db, context_pack_id).await?
    else {
        return Ok(false);
    };
    let delivered_tokens = row.payload["token_budget_event"]["context_pack_render"]["tokens"]
        .as_u64()
        .or_else(|| row.payload["token_budget_event"]["delivered_tokens"].as_u64())
        .unwrap_or(0);
    let legacy_replace_from = legacy_context_pack_tool_overhead_replacement(
        db,
        context_pack_id,
        &config.measurement,
        &row,
        delivered_tokens,
        tool_overhead_tokens,
    )
    .await?;
    Ok(attach_tool_overhead_observed_and_source_status_to_snapshot(
        db,
        &row,
        Some(json!({ "context_pack_id": context_pack_id })),
        tool_overhead_tokens,
        mcp_context_pack_tool_overhead_source_status(),
        legacy_replace_from,
    )
    .await?
    .is_some_and(|value| {
        value["tool_overhead_attach"]["attached"]
            .as_bool()
            .unwrap_or(false)
    }))
}

pub(crate) async fn observe_cli_context_pack_tool_overhead(
    db: &Client,
    context_pack_id: &str,
    output_json: &str,
) -> Result<bool> {
    let output_json = output_json.trim();
    if output_json.is_empty() {
        return Ok(false);
    }
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let Some(row) = latest_token_budget_snapshot_for_context_pack(db, context_pack_id).await?
    else {
        return Ok(false);
    };
    let delivered_tokens = row.payload["token_budget_event"]["context_pack_render"]["tokens"]
        .as_u64()
        .or_else(|| row.payload["token_budget_event"]["delivered_tokens"].as_u64())
        .unwrap_or(0);
    let tool_overhead_tokens = count_cli_context_pack_output_overhead_tokens(
        &config.measurement,
        output_json,
        delivered_tokens,
    )?;
    let legacy_replace_from = legacy_context_pack_tool_overhead_replacement(
        db,
        context_pack_id,
        &config.measurement,
        &row,
        delivered_tokens,
        tool_overhead_tokens,
    )
    .await?;
    Ok(attach_tool_overhead_observed_and_source_status_to_snapshot(
        db,
        &row,
        Some(json!({ "context_pack_id": context_pack_id })),
        tool_overhead_tokens,
        cli_context_pack_tool_overhead_source_status(),
        legacy_replace_from,
    )
    .await?
    .is_some_and(|value| {
        value["tool_overhead_attach"]["attached"]
            .as_bool()
            .unwrap_or(false)
    }))
}

pub(crate) async fn attach_whole_cycle_observed_to_context_pack(
    db: &Client,
    context_pack_id: &str,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) -> Result<Value> {
    let Some(result) = attach_context_pack_whole_cycle_observed(
        db,
        context_pack_id,
        client_prompt_tokens,
        assistant_generation_tokens,
        tool_overhead_tokens,
        continuity_restore_tokens,
    )
    .await?
    else {
        bail!("token_budget_event not found for context_pack_id={context_pack_id}");
    };
    Ok(result)
}

pub(crate) async fn attach_whole_cycle_observed_for_context_pack(
    db: &Client,
    args: &ObserveTokenWholeCycleAttachArgs,
) -> Result<()> {
    let payload = attach_whole_cycle_observed_to_context_pack(
        db,
        &args.context_pack_id,
        args.client_prompt_tokens,
        args.assistant_generation_tokens,
        args.tool_overhead_tokens,
        args.continuity_restore_tokens,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub(crate) async fn observe_rollout_assistant_generation(
    db: &Client,
    args: &ObserveTokenRolloutAssistantGenerationArgs,
) -> Result<()> {
    let repo_root = if let Some(path) = args.repo_root.as_ref() {
        path.clone()
    } else {
        config::discover_repo_root(None)?
    };
    let repo_root_str = repo_root
        .to_str()
        .ok_or_else(|| anyhow!("repo_root must be valid UTF-8"))?;
    let observation = codex_threads::latest_rollout_assistant_generation_observation(
        repo_root_str,
        args.rollout_path.as_deref(),
    )?
    .ok_or_else(|| {
        anyhow!(
            "no unambiguous rollout assistant-generation observation found for repo_root={}",
            repo_root.display()
        )
    })?;
    let attach = if args.apply {
        Some(
            attach_whole_cycle_observed_to_context_pack(
                db,
                &observation.context_pack_id,
                None,
                Some(observation.assistant_generation_tokens),
                None,
                None,
            )
            .await?,
        )
    } else {
        None
    };
    let payload = json!({
        "rollout_assistant_generation_observation": {
            "repo_root": repo_root.display().to_string(),
            "apply_requested": args.apply,
            "applied": attach.is_some(),
            "candidate": observation,
            "attach_result": attach,
        }
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub(super) fn rollout_assistant_generation_observations_for_repo(
    repo_root: &Path,
) -> Result<Vec<codex_threads::RolloutAssistantGenerationObservation>> {
    let Some(repo_root_str) = repo_root.to_str() else {
        return Ok(Vec::new());
    };
    codex_threads::rollout_assistant_generation_observations(repo_root_str, None)
}

pub(super) fn dashboard_rollout_assistant_generation_observations_for_repo(
    repo_root: &Path,
) -> Result<(
    String,
    Vec<codex_threads::RolloutAssistantGenerationObservation>,
)> {
    let source_signature = dashboard_rollout_observation_source_signature(repo_root)?;
    if let Some(observations) = cached_dashboard_rollout_observations(repo_root, &source_signature)
    {
        let semantic_signature = dashboard_rollout_observation_signature(&observations);
        return Ok((semantic_signature, observations));
    }
    let observations = rollout_assistant_generation_observations_for_repo(repo_root)?;
    store_dashboard_rollout_observations(repo_root, &source_signature, &observations);
    let semantic_signature = dashboard_rollout_observation_signature(&observations);
    Ok((semantic_signature, observations))
}

pub(super) fn cached_dashboard_rollout_observations(
    repo_root: &Path,
    signature: &str,
) -> Option<Vec<codex_threads::RolloutAssistantGenerationObservation>> {
    let cache = DASHBOARD_ROLLOUT_OBSERVATION_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let entry = guard.as_ref()?;
    if canonical_repo_root(repo_root) == entry.repo_root && entry.signature == signature {
        Some(entry.observations.clone())
    } else {
        None
    }
}

pub(super) fn dashboard_rollout_observation_source_signature(repo_root: &Path) -> Result<String> {
    let Some(repo_root_str) = repo_root.to_str() else {
        return Ok("no_current_rollout_source".to_string());
    };
    Ok(
        codex_threads::current_rollout_source_signature(repo_root_str)?
            .unwrap_or_else(|| "no_current_rollout_source".to_string()),
    )
}

pub(super) fn dashboard_rollout_observation_signature(
    observations: &[codex_threads::RolloutAssistantGenerationObservation],
) -> String {
    let payload = observations
        .iter()
        .map(|item| {
            json!({
                "thread_id": item.thread_id,
                "turn_id": item.turn_id,
                "context_pack_id": item.context_pack_id,
                "assistant_generation_tokens": item.assistant_generation_tokens,
                "token_count_events": item.token_count_events,
            })
        })
        .collect::<Vec<_>>();
    let payload = Value::Array(payload);
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(super) fn dashboard_client_live_meter_signature(
    observation: Option<&codex_threads::RolloutClientMeterObservation>,
    binding_thread_id_hint: Option<&str>,
) -> String {
    let payload = observation
        .map(|item| {
            let (thread_binding_state, current_thread_bound) =
                client_live_meter_thread_binding_state(binding_thread_id_hint, &item.thread_id);
            json!({
                "thread_id": item.thread_id,
                "turn_id": item.turn_id,
                "client_turn_total_tokens": item.client_turn_total_tokens,
                "latest_cumulative_total_tokens": item.latest_cumulative_total_tokens,
                "latest_model_context_window": item.latest_model_context_window,
                "latest_primary_limit_used_percent": item.latest_primary_limit_used_percent,
                "latest_secondary_limit_used_percent": item.latest_secondary_limit_used_percent,
                "latest_primary_window_duration_mins": item.latest_primary_window_duration_mins,
                "latest_primary_resets_at_epoch_seconds": item.latest_primary_resets_at_epoch_seconds,
                "latest_secondary_window_duration_mins": item.latest_secondary_window_duration_mins,
                "latest_secondary_resets_at_epoch_seconds": item.latest_secondary_resets_at_epoch_seconds,
                "thread_binding_state": thread_binding_state,
                "current_thread_bound": current_thread_bound,
            })
        })
        .unwrap_or(Value::Null);
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(super) fn dashboard_exact_client_limits_signature(
    observation: Option<&CodexAppServerRateLimitsObservation>,
) -> String {
    let payload = observation
        .map(|item| {
            json!({
                "observed_at_epoch_ms": item.observed_at_epoch_ms,
                "limit_id": item.rate_limits.limit_id,
                "limit_name": item.rate_limits.limit_name,
                "plan_type": item.rate_limits.plan_type,
                "primary_used_percent": item.rate_limits.primary.as_ref().map(|window| window.used_percent),
                "primary_window_duration_mins": item
                    .rate_limits
                    .primary
                    .as_ref()
                    .and_then(|window| window.window_duration_mins),
                "primary_resets_at": item
                    .rate_limits
                    .primary
                    .as_ref()
                    .and_then(|window| window.resets_at),
                "secondary_used_percent": item
                    .rate_limits
                    .secondary
                    .as_ref()
                    .map(|window| window.used_percent),
                "secondary_window_duration_mins": item
                    .rate_limits
                    .secondary
                    .as_ref()
                    .and_then(|window| window.window_duration_mins),
                "secondary_resets_at": item
                    .rate_limits
                    .secondary
                    .as_ref()
                    .and_then(|window| window.resets_at),
                "credits_has_credits": item
                    .rate_limits
                    .credits
                    .as_ref()
                    .map(|credits| credits.has_credits),
                "credits_unlimited": item
                    .rate_limits
                    .credits
                    .as_ref()
                    .map(|credits| credits.unlimited),
                "credits_balance": item
                    .rate_limits
                    .credits
                    .as_ref()
                    .and_then(|credits| credits.balance),
            })
        })
        .unwrap_or(Value::Null);
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(super) fn client_live_meter_thread_binding_state(
    current_thread_id: Option<&str>,
    observation_thread_id: &str,
) -> (&'static str, bool) {
    let current_thread_id = current_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let observation_thread_id = observation_thread_id.trim();
    match current_thread_id {
        Some(current_thread_id) if !observation_thread_id.is_empty() => {
            if current_thread_id == observation_thread_id {
                ("current_thread_bound", true)
            } else {
                ("current_thread_mismatch", false)
            }
        }
        Some(_) => ("current_thread_mismatch", false),
        None => ("no_current_thread_binding", false),
    }
}

pub(super) fn dashboard_thread_binding_hint_from_working_state_restore(
    restore: &Value,
) -> Option<String> {
    if restore["restore_freshness_state"].as_str() != Some("fresh") {
        return None;
    }
    let thread_id = restore["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let session_id = restore["session_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let lease = &restore["execctl_active_lease"];
    let lease_state = lease["lease_state"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let owner_thread_id = lease["owner_thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let owner_session_id = lease["owner_session_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if lease_state.is_some() || owner_thread_id.is_some() || owner_session_id.is_some() {
        if lease_state != Some("active") {
            return None;
        }
        if owner_thread_id != Some(thread_id) {
            return None;
        }
        match (session_id, owner_session_id) {
            (Some(session_id), Some(owner_session_id)) if session_id == owner_session_id => {}
            (None, None) => {}
            (Some(_), None) => {}
            _ => return None,
        }
    }

    Some(thread_id.to_string())
}

pub(crate) async fn preferred_dashboard_thread_binding_hint(
    db: &Client,
    repo_root: &Path,
) -> Result<Option<String>> {
    if let Some(current_thread_id) = codex_threads::current_thread_id()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(current_thread_id));
    }

    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    if let Some(thread_id) = load_shared_active_thread_hint(repo_root, now_epoch_ms) {
        return Ok(Some(thread_id));
    }

    if let Some(thread_id) = repo_root
        .to_str()
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| {
            codex_threads::preferred_thread_id_for_repo(value)
                .ok()
                .flatten()
        })
    {
        return Ok(Some(thread_id));
    }

    let repo_root_display = repo_root.display().to_string();
    let Ok(project) = postgres::get_project_by_repo_root(db, &repo_root_display).await else {
        return Ok(None);
    };
    let snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    Ok(snapshot.and_then(|value| {
        dashboard_thread_binding_hint_from_working_state_restore(&value["working_state_restore"])
    }))
}

pub(super) async fn client_budget_target_percent_for_repo(
    db: &Client,
    repo_root: &Path,
) -> Result<u64> {
    let repo_root_display = repo_root.display().to_string();
    let Ok(project) = postgres::get_project_by_repo_root(db, &repo_root_display).await else {
        return Ok(working_state::default_client_budget_target_percent());
    };
    let snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    Ok(snapshot
        .as_ref()
        .map(|value| {
            working_state::client_budget_target_percent_from_restore_context(
                &value["working_state_restore"],
            )
        })
        .unwrap_or_else(working_state::default_client_budget_target_percent))
}

pub(super) fn store_dashboard_rollout_observations(
    repo_root: &Path,
    signature: &str,
    observations: &[codex_threads::RolloutAssistantGenerationObservation],
) {
    let cache = DASHBOARD_ROLLOUT_OBSERVATION_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    *guard = Some(DashboardRolloutObservationCache {
        repo_root: canonical_repo_root(repo_root),
        signature: signature.to_string(),
        observations: observations.to_vec(),
    });
}

pub(super) fn event_context_pack_id(event: &TokenBudgetEvent) -> Option<String> {
    if let Some(context_pack_id) = event.context_pack_id.as_ref() {
        let value = context_pack_id.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let value = event.correlation_id.trim();
    if value.is_empty() || value == event.event_id {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) async fn sync_rollout_assistant_generation_for_events(
    db: &Client,
    events: &[TokenBudgetEvent],
    observations: &[codex_threads::RolloutAssistantGenerationObservation],
) -> Result<bool> {
    let target_context_pack_ids = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event_has_model_visible_with_amai_tokens(event)
                && !is_legacy_continuity_bootstrap_context_pack(event)
                && event.assistant_generation_tokens.is_none()
        })
        .filter_map(event_context_pack_id)
        .collect::<BTreeSet<_>>();
    if target_context_pack_ids.is_empty() {
        return Ok(false);
    }
    if observations.is_empty() {
        return Ok(false);
    }
    let latest_rows =
        latest_token_budget_snapshots_for_context_packs(db, &target_context_pack_ids).await?;
    let mut changed = false;
    for observation in observations {
        if !target_context_pack_ids.contains(&observation.context_pack_id) {
            continue;
        }
        let Some(row) = latest_rows.get(&observation.context_pack_id) else {
            continue;
        };
        let existing = row.payload["token_budget_event"]["whole_cycle_observed"]
            ["assistant_generation_tokens"]
            .as_u64();
        match existing {
            Some(tokens) if tokens == observation.assistant_generation_tokens => {}
            Some(_) => {}
            None => {
                let attached = attach_whole_cycle_observed_to_snapshot(
                    db,
                    row,
                    Some(json!({ "context_pack_id": observation.context_pack_id })),
                    None,
                    Some(observation.assistant_generation_tokens),
                    None,
                    None,
                )
                .await?;
                if attached
                    .as_ref()
                    .and_then(|value| value["whole_cycle_observed_attach"]["attached"].as_bool())
                    .unwrap_or(false)
                {
                    changed = true;
                }
            }
        }
    }
    Ok(changed)
}

pub(super) async fn sync_context_pack_tool_overhead_for_events(
    db: &Client,
    repo_root: &Path,
    events: &[TokenBudgetEvent],
) -> Result<bool> {
    let target_events = events
        .iter()
        .filter(|event| {
            event.snapshot_kind == "token_budget_event"
                && event.snapshot_id.is_some()
                && event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event.tool_overhead_tokens.is_none()
        })
        .collect::<Vec<_>>();
    if target_events.is_empty() {
        return Ok(false);
    }
    let config = load_config(repo_root)?;
    let mut stored_payloads = BTreeMap::<String, Option<String>>::new();
    let mut changed = false;
    for event in target_events {
        let Some(snapshot_id) = event.snapshot_id.as_ref() else {
            continue;
        };
        let Some(row) = postgres::get_observability_snapshot_record(db, snapshot_id).await? else {
            continue;
        };
        let existing =
            row.payload["token_budget_event"]["whole_cycle_observed"]["tool_overhead_tokens"]
                .as_u64();
        if existing.is_some() {
            continue;
        }
        let delivered_tokens = row.payload["token_budget_event"]["context_pack_render"]["tokens"]
            .as_u64()
            .or_else(|| row.payload["token_budget_event"]["delivered_tokens"].as_u64())
            .unwrap_or(event.context_tokens);
        let (context_pack_id, output_json, source_status) = if let Some(context_pack_id) =
            event_context_pack_id(event)
        {
            let output_json = if let Some(cached) = stored_payloads.get(&context_pack_id) {
                cached.clone()
            } else {
                let loaded = stored_context_pack_payload_json(db, &context_pack_id).await?;
                stored_payloads.insert(context_pack_id.clone(), loaded.clone());
                loaded
            };
            let Some(output_json) = output_json else {
                let attached = attach_tool_overhead_source_status_to_snapshot(
                        db,
                        &row,
                        Some(json!({ "context_pack_id": context_pack_id })),
                        json!({
                            "state": "context_pack_payload_missing_irrecoverable",
                            "source_kind": "ami.context_packs",
                            "resolution_condition": "recover_historical_tool_overhead_source_or_freeze_irrecoverable_gap",
                            "note": "Stored context pack payload for this context_pack_id is missing, so payload-based tool_overhead reconstruction cannot proceed from ami.context_packs."
                        }),
                    )
                    .await?;
                if attached
                    .as_ref()
                    .and_then(|value| value["tool_overhead_source_attach"]["attached"].as_bool())
                    .unwrap_or(false)
                {
                    changed = true;
                }
                continue;
            };
            (
                context_pack_id,
                output_json,
                json!({
                    "state": "context_pack_payload_materialized",
                    "source_kind": "ami.context_packs",
                    "tool_overhead_contract_version": CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION,
                    "payload_surface": "stored_full_context_pack_json",
                    "resolution_condition": "already_materialized",
                    "note": "Tool-overhead payload was recovered directly from the stored context pack referenced by context_pack_id."
                }),
            )
        } else {
            match stored_context_pack_payload_json_by_secondary_identity(db, event).await? {
                SecondaryContextPackPayloadLookup::Resolved(candidate) => (
                    candidate.context_pack_id,
                    candidate.payload_json,
                    json!({
                        "state": "secondary_context_pack_match_materialized",
                        "source_kind": "context_pack_query_time_match_v1",
                        "tool_overhead_contract_version": CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION,
                        "payload_surface": "stored_full_context_pack_json",
                        "match_delta_ms": candidate.delta_ms,
                        "max_allowed_delta_ms": TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS,
                        "resolution_condition": "already_materialized",
                        "note": "Tool-overhead payload was recovered by matching the legacy live retrieval event to a stored context pack through project+namespace+retrieval_mode+query_text and a unique nearest created_at timestamp."
                    }),
                ),
                SecondaryContextPackPayloadLookup::NoCandidates => {
                    let attached = attach_tool_overhead_source_status_to_snapshot(
                            db,
                            &row,
                            Some(json!({ "event_id": event.event_id })),
                            json!({
                                "state": "missing_context_pack_identity_irrecoverable",
                                "source_kind": "token_budget_event_identity_v1",
                                "resolution_condition": "recover_legacy_context_pack_identity_or_freeze_irrecoverable_gap",
                                "note": "This live retrieval event has no effective context_pack_id, and no stored context pack could be matched by secondary identity fields."
                            }),
                        )
                        .await?;
                    if attached
                        .as_ref()
                        .and_then(|value| {
                            value["tool_overhead_source_attach"]["attached"].as_bool()
                        })
                        .unwrap_or(false)
                    {
                        changed = true;
                    }
                    continue;
                }
                SecondaryContextPackPayloadLookup::AmbiguousNearest {
                    delta_ms,
                    candidate_count,
                } => {
                    let attached = attach_tool_overhead_source_status_to_snapshot(
                            db,
                            &row,
                            Some(json!({ "event_id": event.event_id })),
                            json!({
                                "state": "secondary_context_pack_match_ambiguous_irrecoverable",
                                "source_kind": "context_pack_query_time_match_v1",
                                "match_delta_ms": delta_ms,
                                "candidate_count": candidate_count,
                                "max_allowed_delta_ms": TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS,
                                "resolution_condition": "recover_legacy_context_pack_identity_or_freeze_irrecoverable_gap",
                                "note": "Secondary context-pack recovery found more than one equally-nearest stored context pack, so payload-based tool_overhead reconstruction remains fail-closed."
                            }),
                        )
                        .await?;
                    if attached
                        .as_ref()
                        .and_then(|value| {
                            value["tool_overhead_source_attach"]["attached"].as_bool()
                        })
                        .unwrap_or(false)
                    {
                        changed = true;
                    }
                    continue;
                }
                SecondaryContextPackPayloadLookup::NearestTooFar { delta_ms } => {
                    let attached = attach_tool_overhead_source_status_to_snapshot(
                            db,
                            &row,
                            Some(json!({ "event_id": event.event_id })),
                            json!({
                                "state": "secondary_context_pack_match_out_of_window_irrecoverable",
                                "source_kind": "context_pack_query_time_match_v1",
                                "match_delta_ms": delta_ms,
                                "max_allowed_delta_ms": TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS,
                                "resolution_condition": "recover_legacy_context_pack_identity_or_freeze_irrecoverable_gap",
                                "note": "Secondary context-pack recovery found only far-away stored candidates outside the allowed time window, so payload-based tool_overhead reconstruction remains fail-closed."
                            }),
                        )
                        .await?;
                    if attached
                        .as_ref()
                        .and_then(|value| {
                            value["tool_overhead_source_attach"]["attached"].as_bool()
                        })
                        .unwrap_or(false)
                    {
                        changed = true;
                    }
                    continue;
                }
            }
        };
        let tool_overhead_tokens = count_cli_context_pack_output_overhead_tokens(
            &config.measurement,
            &output_json,
            delivered_tokens,
        )?;
        let attached = attach_tool_overhead_observed_and_source_status_to_snapshot(
            db,
            &row,
            Some(json!({ "context_pack_id": context_pack_id })),
            tool_overhead_tokens,
            source_status,
            None,
        )
        .await?;
        if attached
            .as_ref()
            .and_then(|value| value["tool_overhead_attach"]["attached"].as_bool())
            .unwrap_or(false)
        {
            changed = true;
        }
    }
    Ok(changed)
}

pub(super) fn is_irrecoverable_tool_overhead_source_state(state: &str) -> bool {
    state.ends_with("_irrecoverable")
}

pub(super) fn is_live_continuity_restore_event(event: &TokenBudgetEvent) -> bool {
    event.traffic_class == "live"
        && event.measurement_scope == "whole_cycle_observed_lower_bound"
        && (event.query_type == "continuity_restore" || event.target_kind == "continuity_restore")
}

pub(super) async fn sync_continuity_pre_amai_baseline_for_events(
    db: &Client,
    repo_root: &Path,
    events: &[TokenBudgetEvent],
) -> Result<bool> {
    let targets = events
        .iter()
        .filter(|event| is_live_continuity_restore_event(event))
        .filter(|event| {
            event.naive_tokens == 0
                || event.baseline_strategy != CONTINUITY_PRE_AMAI_BASELINE_STRATEGY
                || event.pre_amai_baseline_source.is_none()
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(false);
    }

    let config = load_config(repo_root)?;
    let tokenizer = build_tokenizer(&config.measurement.tokenizer)?;
    let mut snapshots_by_scope =
        BTreeMap::<(String, String), Vec<ObservabilitySnapshotRecord>>::new();
    let mut changed = false;

    for event in targets {
        let Some(snapshot_id) = event.snapshot_id.as_ref() else {
            continue;
        };
        let scope_key = (event.project.clone(), event.namespace.clone());
        if !snapshots_by_scope.contains_key(&scope_key) {
            let scoped = postgres::list_scoped_observability_snapshots_by_kinds(
                db,
                &["continuity_import", "continuity_handoff"],
                &scope_key.0,
                &scope_key.1,
                None,
            )
            .await?;
            snapshots_by_scope.insert(scope_key.clone(), scoped);
        }
        let scoped_snapshots = snapshots_by_scope
            .get(&scope_key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let continuity_import_snapshot = latest_scoped_continuity_snapshot_before(
            scoped_snapshots,
            "continuity_import",
            &event.project,
            &event.namespace,
            event.occurred_at_epoch_ms,
        );
        let continuity_handoff_snapshot = latest_scoped_continuity_snapshot_before(
            scoped_snapshots,
            "continuity_handoff",
            &event.project,
            &event.namespace,
            event.occurred_at_epoch_ms,
        );
        let Some(materialization) = continuity_pre_amai_baseline_materialization(
            &tokenizer,
            &event.project,
            &event.namespace,
            continuity_import_snapshot,
            continuity_handoff_snapshot,
        ) else {
            continue;
        };
        let Some(row) = postgres::get_observability_snapshot_record(db, snapshot_id).await? else {
            continue;
        };
        let mut payload = row.payload.clone();
        if apply_continuity_pre_amai_baseline(&mut payload, &materialization)? {
            postgres::update_observability_snapshot_payload(db, snapshot_id, &payload).await?;
            changed = true;
        }
    }

    Ok(changed)
}

pub(super) async fn assistant_generation_turn_observed_snapshots_for_context_packs(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<Vec<AssistantGenerationTurnObservedSnapshot>> {
    if context_pack_ids.is_empty() {
        return Ok(Vec::new());
    }
    let target_ids = context_pack_ids.iter().cloned().collect::<Vec<_>>();
    let rows = postgres::list_observability_snapshots_by_payload_text_array_overlap(
        db,
        ASSISTANT_GENERATION_TURN_OBSERVED_SNAPSHOT_KIND,
        "assistant_generation_turn_observed",
        "context_pack_ids",
        &target_ids,
    )
    .await?;
    let mut latest = BTreeMap::<String, AssistantGenerationTurnObservedSnapshot>::new();
    for row in rows {
        let node = &row.payload["assistant_generation_turn_observed"];
        let thread_id = node["thread_id"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let turn_id = node["turn_id"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let assistant_generation_tokens = node["assistant_generation_tokens"]
            .as_u64()
            .unwrap_or_default();
        let observed_context_pack_ids = node["context_pack_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| context_pack_ids.contains(*value))
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        if thread_id.is_empty()
            || turn_id.is_empty()
            || assistant_generation_tokens == 0
            || observed_context_pack_ids.is_empty()
        {
            continue;
        }
        let key = format!("{thread_id}:{turn_id}");
        let observation = AssistantGenerationTurnObservedSnapshot {
            thread_id,
            turn_id,
            assistant_generation_tokens,
            context_pack_ids: observed_context_pack_ids,
        };
        latest.entry(key).or_insert(observation);
    }
    Ok(latest.into_values().collect())
}

pub(super) async fn attach_whole_cycle_observed_to_turn_group(
    db: &Client,
    thread_id_hint: Option<&str>,
    turn_id: &str,
    context_pack_ids: &BTreeSet<String>,
    assistant_generation_tokens: u64,
) -> Result<Value> {
    let thread_id_hint = thread_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let turn_id = turn_id.trim();
    if turn_id.is_empty() {
        bail!("turn_id must not be empty");
    }
    if assistant_generation_tokens == 0 {
        bail!("assistant_generation_tokens must be greater than zero");
    }
    if context_pack_ids.is_empty() {
        bail!("context_pack_ids must not be empty");
    }
    let rows = latest_token_budget_snapshots_for_context_packs(db, context_pack_ids).await?;
    let metadata = latest_working_state_context_pack_metadata(db, context_pack_ids).await?;
    let repo_thread_ids = repo_fallback_thread_ids_for_context_packs(db, context_pack_ids).await?;
    let merged_thread_ids = merged_context_pack_thread_ids_with_repo_fallback(
        &metadata,
        &rows,
        &repo_thread_ids,
        context_pack_ids,
    );
    let missing_context_pack_thread_ids = context_pack_ids
        .difference(&merged_thread_ids.keys().cloned().collect::<BTreeSet<_>>())
        .cloned()
        .collect::<BTreeSet<_>>();
    if !missing_context_pack_thread_ids.is_empty() && thread_id_hint.is_none() {
        let sample = missing_context_pack_thread_ids
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        bail!("thread_id not found for context_pack_ids={sample}");
    }
    let thread_ids = merged_thread_ids.values().cloned().collect::<BTreeSet<_>>();
    let resolved_thread_id = if let Some(thread_id) = thread_id_hint.as_ref() {
        if !thread_ids.is_empty() && !thread_ids.contains(thread_id) {
            bail!("thread_id does not match observed scope for requested context_pack_ids");
        }
        thread_id.clone()
    } else if thread_ids.len() == 1 {
        thread_ids.iter().next().cloned().unwrap_or_default()
    } else {
        let sample = thread_ids
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "thread_id inference is ambiguous for requested context_pack_ids; candidates={sample}"
        );
    };
    let mut missing_context_pack_ids = context_pack_ids
        .difference(&rows.keys().cloned().collect::<BTreeSet<_>>())
        .cloned()
        .collect::<BTreeSet<_>>();
    if !missing_context_pack_ids.is_empty() {
        let sample = missing_context_pack_ids
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        bail!("token_budget_event not found for context_pack_ids={sample}");
    }
    let row_sample = rows
        .values()
        .next()
        .ok_or_else(|| anyhow!("token_budget_event not found for requested turn group"))?;
    let project_code = row_sample.payload["token_budget_event"]["project_code"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let namespace_code = row_sample.payload["token_budget_event"]["namespace_code"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let invalid_context_pack_ids = rows
        .iter()
        .filter_map(|(context_pack_id, row)| {
            let node = &row.payload["token_budget_event"];
            let measurement_scope = node["measurement_scope"].as_str().unwrap_or_default();
            let valid = measurement_scope == "retrieval_lower_bound"
                || measurement_scope == "whole_cycle_observed_lower_bound";
            if valid {
                None
            } else {
                Some(context_pack_id.clone())
            }
        })
        .collect::<BTreeSet<_>>();
    if !invalid_context_pack_ids.is_empty() {
        let sample = invalid_context_pack_ids
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "turn-group attach requires retrieval or whole_cycle_observed events; invalid context_pack_ids={sample}"
        );
    }
    let payload = json!({
        "_observability": {
            "source_event_id": format!("assistant_generation_turn:{resolved_thread_id}:{turn_id}"),
            "source_kind": "live_assistant_generation_turn_observed",
            "scope_project_code": project_code,
            "scope_namespace_code": namespace_code,
            "captured_at_epoch_ms": current_epoch_ms()?,
        },
        "project": {
            "code": project_code,
        },
        "namespace": {
            "code": namespace_code,
        },
        "assistant_generation_turn_observed": {
            "schema_version": "assistant-generation-turn-observed-v1",
            "thread_id": resolved_thread_id,
            "turn_id": turn_id,
            "assistant_generation_tokens": assistant_generation_tokens,
            "context_pack_ids": context_pack_ids.iter().cloned().collect::<Vec<_>>(),
            "observation_source": "client_turn_attach_v1",
            "note": "Turn-scoped assistant_generation is counted once per matched turn-group and must not be duplicated across every context_pack event."
        }
    });
    let snapshot_id = postgres::insert_observability_snapshot(
        db,
        ASSISTANT_GENERATION_TURN_OBSERVED_SNAPSHOT_KIND,
        &payload,
    )
    .await?;
    let result = json!({
        "assistant_generation_turn_observed_attach": {
            "snapshot_id": snapshot_id,
            "thread_id": resolved_thread_id,
            "turn_id": turn_id,
            "assistant_generation_tokens": assistant_generation_tokens,
            "context_pack_ids": context_pack_ids.iter().cloned().collect::<Vec<_>>(),
            "attached": true,
            "thread_id_inferred": thread_id_hint.is_none(),
            "observation_source": "client_turn_attach_v1",
            "note": "Turn-scoped attach is replay-protected by thread_id + turn_id and keeps assistant_generation at group scope instead of duplicating it into every token_budget_event."
        }
    });
    missing_context_pack_ids.clear();
    Ok(result)
}

pub(crate) async fn attach_whole_cycle_observed_for_turn_group(
    db: &Client,
    args: &ObserveTokenWholeCycleTurnAttachArgs,
) -> Result<()> {
    let context_pack_ids = args
        .context_pack_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    let payload = attach_whole_cycle_observed_to_turn_group(
        db,
        Some(&args.thread_id),
        &args.turn_id,
        &context_pack_ids,
        args.assistant_generation_tokens,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub(crate) async fn attach_whole_cycle_observed_to_turn_group_with_thread_hint(
    db: &Client,
    thread_id_hint: Option<&str>,
    turn_id: &str,
    context_pack_ids: &BTreeSet<String>,
    assistant_generation_tokens: u64,
) -> Result<Value> {
    attach_whole_cycle_observed_to_turn_group(
        db,
        thread_id_hint,
        turn_id,
        context_pack_ids,
        assistant_generation_tokens,
    )
    .await
}

pub(super) async fn derive_rollout_assistant_generation_scope(
    db: &Client,
    events: &[TokenBudgetEvent],
) -> Result<AssistantGenerationScopeObservation> {
    let target_context_pack_ids = assistant_generation_missing_scope_context_pack_ids(Some(events));
    if target_context_pack_ids.is_empty() {
        return Ok(AssistantGenerationScopeObservation::default());
    }

    let direct_turns = assistant_generation_turn_observed_snapshots_for_context_packs(
        db,
        &target_context_pack_ids,
    )
    .await?;
    let token_budget_rows =
        latest_token_budget_snapshots_for_context_packs(db, &target_context_pack_ids).await?;
    let metadata = merged_context_pack_rollout_metadata(
        &latest_working_state_context_pack_metadata(db, &target_context_pack_ids).await?,
        &token_budget_rows,
        &target_context_pack_ids,
    );
    let thread_ids = metadata
        .values()
        .map(|item| item.thread_id.clone())
        .collect::<BTreeSet<_>>();
    let mut turns_by_thread =
        BTreeMap::<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>::new();
    let mut helper_only_context_pack_ids_by_thread = BTreeMap::<String, BTreeSet<String>>::new();
    for thread_id in thread_ids {
        let turns =
            codex_threads::rollout_assistant_generation_turn_observations_for_thread(&thread_id)?;
        if !turns.is_empty() {
            turns_by_thread.insert(thread_id.clone(), turns);
        }
        let helper_only_context_pack_ids =
            codex_threads::rollout_helper_only_context_pack_ids_for_thread(&thread_id)?;
        if !helper_only_context_pack_ids.is_empty() {
            helper_only_context_pack_ids_by_thread.insert(thread_id, helper_only_context_pack_ids);
        }
    }
    let helper_only_context_pack_ids =
        helper_only_context_pack_ids_for_scope(&metadata, &helper_only_context_pack_ids_by_thread);
    let helper_only_non_model_visible_context_pack_ids =
        helper_only_non_model_visible_context_pack_ids(
            events,
            &metadata,
            &helper_only_context_pack_ids_by_thread,
        );
    Ok(derive_rollout_assistant_generation_scope_from_sources(
        &target_context_pack_ids,
        &direct_turns,
        &metadata,
        &turns_by_thread,
        &helper_only_context_pack_ids,
        &helper_only_non_model_visible_context_pack_ids,
    ))
}

pub(super) fn dashboard_report_events_signature(events: &[TokenBudgetEvent]) -> String {
    let mut buffer = String::new();
    for event in events {
        let parts = [
            event.created_at_epoch_ms.to_string(),
            event.event_id.clone(),
            event.correlation_id.clone(),
            event.session_id.clone(),
            event.source_kind.clone(),
            event.traffic_class.clone(),
            event.measurement_scope.clone(),
            event.query.clone(),
            event.query_type.clone(),
            event.target_kind.clone(),
            event.tokenizer.clone(),
            event.latency_ms.to_string(),
            event.saved_tokens.to_string(),
            event.naive_tokens.to_string(),
            event.context_tokens.to_string(),
            event.recovery_tokens.to_string(),
            event.effective_saved_tokens.to_string(),
            event.quality_ok.to_string(),
            event.quality_method.clone(),
            event.quality_tier.clone(),
            event.head_hit_target.to_string(),
            event.needed_followup.to_string(),
            event.resolved_by_event_id.clone().unwrap_or_default(),
            event.fallback_triggered.to_string(),
            event.document_hits.to_string(),
            event.symbol_hits_count.to_string(),
            event.file_hits.to_string(),
            event.sources_count.to_string(),
            event.chunks_count.to_string(),
            event
                .client_prompt_tokens
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .assistant_generation_tokens
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .tool_overhead_tokens
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .continuity_restore_tokens
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ];
        buffer.push_str(&parts.join("\\u{1f}"));
        buffer.push('\u{1e}');
    }
    hex_sha256(buffer.as_bytes())
}

pub(super) fn dashboard_report_assistant_scope_signature(
    scope: &AssistantGenerationScopeObservation,
) -> String {
    let mut buffer = String::new();
    let header = [
        scope.target_group_count.to_string(),
        scope.observed_group_count.to_string(),
        scope.observed_tokens.to_string(),
        scope.available_turns.to_string(),
        scope.available_direct_turns.to_string(),
        scope.available_rollout_turns.to_string(),
        scope.helper_only_context_pack_ids.len().to_string(),
        scope
            .helper_only_non_model_visible_context_pack_ids
            .len()
            .to_string(),
        scope.target_context_pack_ids.len().to_string(),
    ];
    buffer.push_str(&header.join("\\u{1f}"));
    buffer.push('\u{1f}');
    append_sorted_signature_items(&mut buffer, &scope.target_context_pack_ids);
    append_sorted_signature_items(&mut buffer, &scope.helper_only_context_pack_ids);
    append_sorted_signature_items(
        &mut buffer,
        &scope.helper_only_non_model_visible_context_pack_ids,
    );
    append_sorted_signature_items(&mut buffer, &scope.matched_context_pack_ids);
    append_sorted_signature_items(&mut buffer, &scope.unmatched_context_pack_ids);
    append_sorted_signature_items(&mut buffer, &scope.matched_turn_ids);
    append_sorted_signature_items(&mut buffer, &scope.matched_direct_turn_ids);
    append_sorted_signature_items(&mut buffer, &scope.matched_rollout_turn_ids);
    hex_sha256(buffer.as_bytes())
}

pub(super) fn append_sorted_signature_items(buffer: &mut String, values: &BTreeSet<String>) {
    for value in values {
        let _ = write!(buffer, "{}\u{1f}", value);
    }
    buffer.push('\u{1e}');
}

pub(super) fn canonical_repo_root(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn derive_rollout_assistant_generation_scope_from_sources(
    target_context_pack_ids: &BTreeSet<String>,
    direct_turns: &[AssistantGenerationTurnObservedSnapshot],
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    turns_by_thread: &BTreeMap<
        String,
        Vec<codex_threads::RolloutAssistantGenerationTurnObservation>,
    >,
    helper_only_context_pack_ids: &BTreeSet<String>,
    helper_only_non_model_visible_context_pack_ids: &BTreeSet<String>,
) -> AssistantGenerationScopeObservation {
    let target_context_pack_ids = target_context_pack_ids
        .difference(helper_only_context_pack_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    let metadata_lineage_context_pack_ids = target_context_pack_ids
        .iter()
        .filter(|context_pack_id| {
            metadata
                .get(*context_pack_id)
                .map(|meta| meta.captured_at_epoch_ms > 0 || !meta.turn_id.trim().is_empty())
                .unwrap_or(false)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let direct_turn_lineage_context_pack_ids = direct_turns
        .iter()
        .flat_map(|turn| turn.context_pack_ids.iter())
        .filter(|context_pack_id| target_context_pack_ids.contains(*context_pack_id))
        .cloned()
        .collect::<BTreeSet<_>>();
    let target_context_pack_ids = target_context_pack_ids
        .into_iter()
        .filter(|context_pack_id| {
            metadata_lineage_context_pack_ids.contains(context_pack_id)
                || direct_turn_lineage_context_pack_ids.contains(context_pack_id)
        })
        .collect::<BTreeSet<_>>();
    if target_context_pack_ids.is_empty() {
        return AssistantGenerationScopeObservation {
            helper_only_context_pack_ids: helper_only_context_pack_ids.clone(),
            helper_only_non_model_visible_context_pack_ids:
                helper_only_non_model_visible_context_pack_ids.clone(),
            ..AssistantGenerationScopeObservation::default()
        };
    }

    let mut matched_context_pack_ids = BTreeSet::new();
    let mut matched_turn_ids = BTreeSet::new();
    let mut matched_direct_turn_ids = BTreeSet::new();
    let mut matched_rollout_turn_ids = BTreeSet::new();
    let mut observed_tokens = 0_u64;
    let mut observed_group_count = 0_u64;
    let mut target_group_keys = BTreeSet::new();
    let mut grouped_context_pack_ids = BTreeMap::<(String, String), BTreeSet<String>>::new();
    let mut direct_turns_by_key =
        BTreeMap::<String, AssistantGenerationTurnObservedSnapshot>::new();

    for turn in direct_turns {
        let matched_ids = turn
            .context_pack_ids
            .intersection(&target_context_pack_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        if matched_ids.is_empty() {
            continue;
        }
        let turn_key = format!("{}:{}", turn.thread_id, turn.turn_id);
        target_group_keys.insert(turn_key.clone());
        matched_turn_ids.insert(turn_key.clone());
        matched_direct_turn_ids.insert(turn_key.clone());
        direct_turns_by_key.insert(turn_key, turn.clone());
        matched_context_pack_ids.extend(matched_ids);
        observed_group_count = observed_group_count.saturating_add(1);
        observed_tokens = observed_tokens.saturating_add(turn.assistant_generation_tokens);
    }

    let mut by_thread = BTreeMap::<String, Vec<(String, WorkingStateContextPackMeta)>>::new();
    for context_pack_id in &target_context_pack_ids {
        if matched_context_pack_ids.contains(context_pack_id) {
            continue;
        }
        if let Some(meta) = metadata.get(context_pack_id.as_str()) {
            by_thread
                .entry(meta.thread_id.clone())
                .or_default()
                .push((context_pack_id.clone(), meta.clone()));
        }
    }

    for (thread_id, entries) in by_thread {
        let Some(turns) = turns_by_thread.get(&thread_id) else {
            continue;
        };
        if turns.is_empty() {
            continue;
        }
        for (context_pack_id, meta) in entries {
            let matched_turn = turns
                .iter()
                .find(|turn| !meta.turn_id.is_empty() && turn.turn_id == meta.turn_id)
                .or_else(|| {
                    (meta.captured_at_epoch_ms > 0).then(|| {
                        turns.iter().find(|turn| {
                            turn.started_at_epoch_ms
                                .saturating_sub(ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS)
                                <= meta.captured_at_epoch_ms
                                && meta.captured_at_epoch_ms
                                    <= turn
                                        .ended_at_epoch_ms
                                        .saturating_add(ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS)
                        })
                    })?
                });
            let Some(turn) = matched_turn else {
                continue;
            };
            let turn_key = format!("{thread_id}:{}", turn.turn_id);
            if direct_turns_by_key.contains_key(&turn_key) {
                matched_context_pack_ids.insert(context_pack_id);
                continue;
            }
            grouped_context_pack_ids
                .entry((thread_id.clone(), turn.turn_id.clone()))
                .or_default()
                .insert(context_pack_id);
        }
    }

    let available_turns: u64 = turns_by_thread
        .values()
        .map(|turns| turns.len() as u64)
        .sum();

    for ((thread_id, turn_id), context_pack_ids) in &grouped_context_pack_ids {
        let Some(turns) = turns_by_thread.get(thread_id) else {
            continue;
        };
        let Some(turn) = turns.iter().find(|candidate| candidate.turn_id == *turn_id) else {
            continue;
        };
        if context_pack_ids.is_empty() {
            continue;
        }
        let turn_key = format!("{thread_id}:{turn_id}");
        target_group_keys.insert(turn_key.clone());
        matched_turn_ids.insert(turn_key.clone());
        matched_rollout_turn_ids.insert(turn_key);
        matched_context_pack_ids.extend(context_pack_ids.iter().cloned());
        observed_group_count = observed_group_count.saturating_add(1);
        observed_tokens = observed_tokens.saturating_add(turn.assistant_generation_tokens);
    }

    let unmatched_context_pack_ids = target_context_pack_ids
        .difference(&matched_context_pack_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    for context_pack_id in &unmatched_context_pack_ids {
        target_group_keys.insert(format!("context_pack:{context_pack_id}"));
    }

    AssistantGenerationScopeObservation {
        target_group_count: target_group_keys.len() as u64,
        observed_group_count,
        observed_tokens,
        helper_only_context_pack_ids: helper_only_context_pack_ids.clone(),
        helper_only_non_model_visible_context_pack_ids:
            helper_only_non_model_visible_context_pack_ids.clone(),
        target_context_pack_ids,
        matched_context_pack_ids,
        unmatched_context_pack_ids,
        matched_turn_ids,
        available_turns: available_turns.saturating_add(direct_turns.len() as u64),
        available_direct_turns: direct_turns.len() as u64,
        available_rollout_turns: available_turns,
        matched_direct_turn_ids,
        matched_rollout_turn_ids,
    }
}

pub(super) fn helper_only_context_pack_ids_for_scope(
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    helper_only_context_pack_ids_by_thread: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    metadata
        .iter()
        .filter_map(|(context_pack_id, meta)| {
            let helper_ids = helper_only_context_pack_ids_by_thread.get(meta.thread_id.as_str())?;
            helper_ids
                .contains(context_pack_id)
                .then(|| context_pack_id.clone())
        })
        .collect()
}

pub(super) fn event_has_model_visible_with_amai_tokens(event: &TokenBudgetEvent) -> bool {
    event.context_tokens > 0
        || event.recovery_tokens > 0
        || event.client_prompt_tokens.unwrap_or(0) > 0
        || event.assistant_generation_tokens.unwrap_or(0) > 0
        || event.tool_overhead_tokens.unwrap_or(0) > 0
        || event.continuity_restore_tokens.unwrap_or(0) > 0
}

pub(super) fn is_legacy_continuity_bootstrap_context_pack(event: &TokenBudgetEvent) -> bool {
    event.namespace == "continuity"
        && event.source_kind == "live_context_pack"
        && event.query_type == "code_lookup"
        && event.target_kind == "file"
        && event.query == CONTINUITY_SNAPSHOT_QUERY_LABEL
}

pub(super) fn helper_only_non_model_visible_context_pack_ids(
    events: &[TokenBudgetEvent],
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    helper_only_context_pack_ids_by_thread: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| {
            let context_pack_id = event_context_pack_id(event)?;
            let thread_id = metadata.get(context_pack_id.as_str())?.thread_id.as_str();
            let helper_ids = helper_only_context_pack_ids_by_thread.get(thread_id)?;
            if helper_ids.contains(&context_pack_id)
                && !event_has_model_visible_with_amai_tokens(event)
            {
                Some(context_pack_id)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn current_live_turn_assistant_scope_from_client_meter(
    events: &[TokenBudgetEvent],
    matched_context_pack_ids: &BTreeSet<String>,
    observation: &codex_threads::RolloutClientMeterObservation,
) -> Option<AssistantGenerationScopeObservation> {
    let target_context_pack_ids = assistant_generation_missing_scope_context_pack_ids(Some(events))
        .into_iter()
        .filter(|context_pack_id| matched_context_pack_ids.contains(context_pack_id))
        .collect::<BTreeSet<_>>();
    if target_context_pack_ids.is_empty() {
        return None;
    }

    let thread_id = observation.thread_id.trim();
    let turn_id = observation.turn_id.trim();
    let turn_key = if !thread_id.is_empty()
        && !turn_id.is_empty()
        && observation.started_at_epoch_ms > 0
        && observation.ended_at_epoch_ms > 0
    {
        Some(format!("{thread_id}:{turn_id}"))
    } else {
        None
    };
    let observed = turn_key.is_some() && observation.client_turn_output_tokens > 0;
    let matched_turn_ids = turn_key
        .clone()
        .into_iter()
        .filter(|_| observed)
        .collect::<BTreeSet<_>>();
    let matched_context_pack_ids = if observed {
        target_context_pack_ids.clone()
    } else {
        BTreeSet::new()
    };
    let unmatched_context_pack_ids = target_context_pack_ids
        .difference(&matched_context_pack_ids)
        .cloned()
        .collect::<BTreeSet<_>>();

    Some(AssistantGenerationScopeObservation {
        target_group_count: 1,
        observed_group_count: if observed { 1 } else { 0 },
        observed_tokens: if observed {
            observation.client_turn_output_tokens
        } else {
            0
        },
        helper_only_context_pack_ids: BTreeSet::new(),
        helper_only_non_model_visible_context_pack_ids: BTreeSet::new(),
        target_context_pack_ids,
        matched_context_pack_ids,
        unmatched_context_pack_ids,
        matched_turn_ids: matched_turn_ids.clone(),
        available_turns: turn_key.iter().count() as u64,
        available_direct_turns: 0,
        available_rollout_turns: turn_key.iter().count() as u64,
        matched_direct_turn_ids: BTreeSet::new(),
        matched_rollout_turn_ids: matched_turn_ids,
    })
}

pub(super) fn current_live_turn_meter_summary(summary: &Value) -> Value {
    let mut adjusted = summary.clone();
    let live_events_count = adjusted["live_events_count"].as_u64().unwrap_or(0);
    if live_events_count > 0 {
        adjusted["meter_counted_events"] = Value::from(live_events_count);
    }
    adjusted
}

pub(super) fn assistant_scope_is_materialized(scope: &AssistantGenerationScopeObservation) -> bool {
    scope.target_group_count > 0
        || scope.observed_group_count > 0
        || scope.observed_tokens > 0
        || scope.available_turns > 0
        || !scope.target_context_pack_ids.is_empty()
        || !scope.matched_context_pack_ids.is_empty()
        || !scope.unmatched_context_pack_ids.is_empty()
        || !scope.helper_only_context_pack_ids.is_empty()
        || !scope
            .helper_only_non_model_visible_context_pack_ids
            .is_empty()
}

pub(super) fn materialized_assistant_scope<'a>(
    scope: &'a AssistantGenerationScopeObservation,
) -> Option<&'a AssistantGenerationScopeObservation> {
    assistant_scope_is_materialized(scope).then_some(scope)
}

pub(crate) fn continuity_restore_observed_config(
    repo_root: &Path,
) -> Result<TokenBudgetConfigFile> {
    load_config(repo_root)
}

pub(crate) fn prewarm_shared_tokenizer(name: &str) -> Result<()> {
    let _ = shared_tokenizer(name)?;
    Ok(())
}

pub(crate) async fn record_continuity_restore_observed_event(
    db: &Client,
    project_code: &str,
    namespace_code: &str,
    prompt_text: &str,
    source_kind: &str,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    record_continuity_restore_observed_event_with_config(
        db,
        project_code,
        namespace_code,
        prompt_text,
        source_kind,
        &repo_root,
        &config,
    )
    .await
}

pub(crate) async fn record_continuity_restore_observed_event_with_config(
    db: &Client,
    project_code: &str,
    namespace_code: &str,
    prompt_text: &str,
    source_kind: &str,
    repo_root: &Path,
    config: &TokenBudgetConfigFile,
) -> Result<()> {
    let total_started = Instant::now();
    let prompt_text = prompt_text.trim();
    if prompt_text.is_empty() {
        return Ok(());
    }

    let prompt_hash = hex_sha256(prompt_text.as_bytes());
    let now_epoch_ms = current_epoch_ms().unwrap_or_default() as u64;
    if continuity_restore_observed_event_recently_recorded(
        repo_root,
        project_code,
        namespace_code,
        source_kind,
        &prompt_hash,
        now_epoch_ms,
    ) {
        return Ok(());
    }
    let step_started = Instant::now();
    let tokenizer = shared_tokenizer(&config.measurement.tokenizer)?;
    continuity_profile_log(
        "record_continuity_restore_observed_event.load_config_and_tokenizer",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    let step_started = Instant::now();
    let continuity_restore_tokens = tokenizer.encode_with_special_tokens(prompt_text).len() as u64;
    let traffic_class = derive_traffic_class(source_kind);
    let mut event = build_continuity_restore_observed_event(
        project_code,
        namespace_code,
        source_kind,
        &config.measurement,
        &config.contract,
        prompt_text,
        continuity_restore_tokens,
    )?;
    continuity_profile_log(
        "record_continuity_restore_observed_event.build_event",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    let occurred_at_epoch_ms = event["token_budget_event"]["occurred_at_epoch_ms"]
        .as_i64()
        .unwrap_or_else(|| current_epoch_ms().unwrap_or_default());
    let step_started = Instant::now();
    let latest_continuity_import = postgres::list_scoped_observability_snapshots_by_kinds(
        db,
        &["continuity_import"],
        project_code,
        namespace_code,
        Some(1),
    )
    .await?;
    continuity_profile_log(
        "record_continuity_restore_observed_event.latest_continuity_import",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    let step_started = Instant::now();
    let latest_continuity_handoff = postgres::list_scoped_observability_snapshots_by_kinds(
        db,
        &["continuity_handoff"],
        project_code,
        namespace_code,
        Some(1),
    )
    .await?;
    continuity_profile_log(
        "record_continuity_restore_observed_event.latest_continuity_handoff",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    let step_started = Instant::now();
    if let Some(materialization) = continuity_pre_amai_baseline_materialization(
        &tokenizer,
        project_code,
        namespace_code,
        latest_scoped_continuity_snapshot_before(
            &latest_continuity_import,
            "continuity_import",
            project_code,
            namespace_code,
            occurred_at_epoch_ms,
        ),
        latest_scoped_continuity_snapshot_before(
            &latest_continuity_handoff,
            "continuity_handoff",
            project_code,
            namespace_code,
            occurred_at_epoch_ms,
        ),
    ) {
        let _ = apply_continuity_pre_amai_baseline(&mut event, &materialization)?;
    }
    continuity_profile_log(
        "record_continuity_restore_observed_event.materialize_baseline",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    if traffic_class == "live" {
        let step_started = Instant::now();
        let profile = resolve_profile(config, None, repo_root)?;
        enrich_live_event_payload(db, &mut event, &profile, repo_root).await?;
        continuity_profile_log(
            "record_continuity_restore_observed_event.enrich_live_event_payload",
            step_started.elapsed().as_millis(),
            &format!("project={} namespace={}", project_code, namespace_code),
        );
    }
    let step_started = Instant::now();
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    continuity_profile_log(
        "record_continuity_restore_observed_event.insert_snapshot",
        step_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    let _ = write_continuity_restore_observed_dedupe_cache(
        &repo_root,
        project_code,
        namespace_code,
        source_kind,
        &prompt_hash,
        now_epoch_ms,
    );
    continuity_profile_log(
        "record_continuity_restore_observed_event.total",
        total_started.elapsed().as_millis(),
        &format!("project={} namespace={}", project_code, namespace_code),
    );
    Ok(())
}

pub(crate) async fn record_verify_context_pack_event(db: &Client, payload: &Value) -> Result<()> {
    record_context_pack_event(db, payload, "verify_context_pack").await
}

pub(crate) async fn record_verify_benchmark_event(
    db: &Client,
    benchmark_payload: &Value,
) -> Result<()> {
    let benchmark = benchmark_payload
        .get("token_benchmark")
        .cloned()
        .ok_or_else(|| anyhow!("token benchmark payload missing token_benchmark root"))?;
    let repo_root = config::discover_repo_root(None)?;
    let contract = load_config(&repo_root)?.contract;
    let timestamp_utc = current_epoch_ms()?;
    let event = json!({
        "token_budget_event": {
            "event_id": Uuid::new_v4(),
            "correlation_id": benchmark["context_pack_id"].clone(),
            "timestamp_utc": timestamp_utc,
            "occurred_at_epoch_ms": timestamp_utc,
            "ingested_at_epoch_ms": timestamp_utc,
            "source_kind": "verify_token_benchmark",
            "traffic_class": "verify",
            "measurement_scope": "retrieval_lower_bound",
            "payload_origin": "verify_token_benchmark",
            "contract": token_contract_metadata_json(&contract),
            "project": benchmark["project"].clone(),
            "namespace": benchmark["namespace"].clone(),
            "query": benchmark["query"].clone(),
            "query_hash": hex_sha256(benchmark["query"].as_str().unwrap_or_default().as_bytes()),
            "query_type": "unknown",
            "cold_warm_state": "benchmark",
            "baseline_strategy": "naive_top_files",
            "retrieval_mode": benchmark["retrieval_mode"].clone(),
            "tokenizer": benchmark["tokenizer"].clone(),
            "naive_limit_files": benchmark["naive_limit_files"].clone(),
            "naive_max_bytes_per_file": benchmark["naive_max_bytes_per_file"].clone(),
            "visible_projects": benchmark["visible_projects"].clone(),
            "naive_scope": benchmark["naive_scope"].clone(),
            "context_pack_render": benchmark["context_pack_render"].clone(),
            "recovery": {
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0,
            },
            "quality": {
                "quality_ok": true,
                "quality_score": 1.0,
                "quality_method": "benchmark_assumption",
                "quality_tier": "benchmark",
                "head_hit_target": true,
            },
            "shape": {
                "sources_count": 0,
                "chunks_count": 0,
            },
            "savings": benchmark["savings"].clone()
        }
    });
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}
