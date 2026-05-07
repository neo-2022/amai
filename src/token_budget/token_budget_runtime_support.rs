use super::*;

pub(crate) fn load_config(repo_root: &Path) -> Result<TokenBudgetConfigFile> {
    let path = resolve_token_budget_config_path(repo_root);
    let canonical_config_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if let Some((size_bytes, modified_epoch_ms)) = token_budget_config_file_signature(&path) {
        let cache = TOKEN_BUDGET_CONFIG_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(guard) = cache.lock() {
            if let Some(entry) = guard.get(&canonical_config_path) {
                if entry.size_bytes == size_bytes && entry.modified_epoch_ms == modified_epoch_ms {
                    return Ok(entry.config.clone());
                }
            }
        }
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let config: TokenBudgetConfigFile =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?;
    if let Some((size_bytes, modified_epoch_ms)) = token_budget_config_file_signature(&path) {
        let cache = TOKEN_BUDGET_CONFIG_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut guard) = cache.lock() {
            guard.insert(
                canonical_config_path,
                CachedTokenBudgetConfig {
                    size_bytes,
                    modified_epoch_ms,
                    config: config.clone(),
                },
            );
        }
    }
    Ok(config)
}

fn resolve_token_budget_config_path(repo_root: &Path) -> PathBuf {
    let project_local_path = repo_root.join(CONFIG_RELATIVE_PATH);
    if project_local_path.exists() {
        return project_local_path;
    }

    if let Ok(amai_repo_root) = config::discover_repo_root(None) {
        let shared_path = amai_repo_root.join(CONFIG_RELATIVE_PATH);
        if shared_path.exists() {
            return shared_path;
        }
    }

    project_local_path
}

fn token_budget_config_file_signature(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    let size_bytes = metadata.len();
    let modified_epoch_ms = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((size_bytes, modified_epoch_ms))
}

pub(crate) fn resolve_profile(
    config: &TokenBudgetConfigFile,
    requested_profile: Option<&str>,
    repo_root: &Path,
) -> Result<ResolvedProfile> {
    let install_state_path = repo_root.join("state/install_state.json");
    let install_state_client = fs::read_to_string(&install_state_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value["client_key"].as_str().map(ToOwned::to_owned));
    let profile_code = if let Some(requested) = requested_profile {
        requested.to_string()
    } else if let Ok(from_env) = std::env::var("AMAI_TOKEN_BUDGET_PROFILE") {
        from_env
    } else if let Some(client_key) = install_state_client {
        config
            .client_budget_overrides
            .get(&client_key)
            .cloned()
            .unwrap_or_else(|| config.default_profile.clone())
    } else {
        config.default_profile.clone()
    };
    let profile = config
        .profiles
        .get(&profile_code)
        .ok_or_else(|| anyhow!("unknown token budget profile: {profile_code}"))?;
    Ok(ResolvedProfile {
        code: profile_code,
        display_name: profile.display_name.clone(),
        description: profile.description.clone(),
        session_gap_minutes: profile.session_gap_minutes,
        rolling_window_hours: profile.rolling_window_hours,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_config_falls_back_to_amai_repo_config_for_plain_project_roots() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-token-budget-config-fallback-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_root).expect("create temp root");

        let resolved = resolve_token_budget_config_path(&temp_root);
        let expected = Path::new(env!("CARGO_MANIFEST_DIR")).join(CONFIG_RELATIVE_PATH);
        assert_eq!(
            fs::canonicalize(resolved).expect("resolved canonical path"),
            fs::canonicalize(expected).expect("expected canonical path")
        );

        let config = load_config(&temp_root).expect("fallback config");
        assert!(!config.default_profile.is_empty());
        assert!(
            config.profiles.contains_key(&config.default_profile),
            "default profile must resolve after fallback"
        );

        let _ = fs::remove_dir_all(temp_root);
    }
}

pub(crate) async fn load_events(
    db: &Client,
    include_verify_events: bool,
    limit: Option<i64>,
) -> Result<Vec<TokenBudgetEvent>> {
    let events = load_raw_events(
        db,
        dashboard_event_snapshot_kinds(include_verify_events),
        limit,
    )
    .await?;
    Ok(filter_dashboard_token_events(events, include_verify_events))
}

pub(crate) fn dashboard_event_snapshot_kinds(
    include_verify_events: bool,
) -> &'static [&'static str] {
    if include_verify_events {
        &["token_budget_event", "token_benchmark"]
    } else {
        &["token_budget_event"]
    }
}

async fn load_raw_events(
    db: &Client,
    snapshot_kinds: &[&str],
    limit: Option<i64>,
) -> Result<Vec<TokenBudgetEvent>> {
    let rows = postgres::list_observability_snapshots_by_kinds(db, snapshot_kinds, limit).await?;
    let mut events = Vec::new();
    for row in rows {
        if let Some(event) = parse_snapshot_event(&row)? {
            events.push(event);
        }
    }
    Ok(events)
}

pub(crate) fn filter_dashboard_token_events(
    raw_events: Vec<TokenBudgetEvent>,
    include_verify_events: bool,
) -> Vec<TokenBudgetEvent> {
    let events = suppress_shadowed_live_events(raw_events);
    events
        .into_iter()
        .filter(|event| {
            include_traffic_class_in_report(&event.traffic_class, include_verify_events)
                && !live_event_is_engineering_runtime_contamination(event)
                && !live_event_has_invalid_runtime_contract(event)
        })
        .collect()
}

pub(crate) async fn load_dashboard_token_events(
    db: &Client,
    repo_root: &Path,
    include_verify_events: bool,
) -> Result<Vec<TokenBudgetEvent>> {
    let snapshot_kinds = dashboard_event_snapshot_kinds(include_verify_events);
    let summary = postgres::summarize_observability_snapshots_by_kinds(db, snapshot_kinds).await?;
    load_dashboard_token_events_with_summary(
        db,
        repo_root,
        include_verify_events,
        snapshot_kinds,
        &summary,
    )
    .await
}

pub(crate) async fn load_dashboard_token_events_with_summary(
    db: &Client,
    repo_root: &Path,
    include_verify_events: bool,
    snapshot_kinds: &[&str],
    summary: &[postgres::ObservabilitySnapshotKindSummary],
) -> Result<Vec<TokenBudgetEvent>> {
    let invalidation_epoch_ms = current_dashboard_token_events_invalidation_epoch_ms(repo_root);
    let signature =
        dashboard_token_events_signature(snapshot_kinds, &summary, invalidation_epoch_ms);
    if let Some(raw_events) = cached_dashboard_token_events(repo_root, snapshot_kinds, &signature) {
        return Ok(filter_dashboard_token_events(
            raw_events,
            include_verify_events,
        ));
    }
    if let Some((previous_summary, previous_raw_events)) =
        cached_dashboard_token_events_entry(repo_root, snapshot_kinds)
    {
        if let Some(delta_limit) =
            dashboard_token_events_delta_limit(&previous_summary, &summary, 32)
        {
            let delta_events = load_raw_events(db, snapshot_kinds, Some(delta_limit)).await?;
            let merged = merge_dashboard_token_events(previous_raw_events, delta_events);
            store_dashboard_token_events(repo_root, snapshot_kinds, &signature, &summary, &merged);
            return Ok(filter_dashboard_token_events(merged, include_verify_events));
        }
    }
    let raw_events = load_raw_events(db, snapshot_kinds, None).await?;
    store_dashboard_token_events(repo_root, snapshot_kinds, &signature, &summary, &raw_events);
    Ok(filter_dashboard_token_events(
        raw_events,
        include_verify_events,
    ))
}

pub(crate) fn recent_current_session_slice_complete(
    events: &[TokenBudgetEvent],
    fetched_rows: usize,
    limit: i64,
    session_gap_ms: i64,
) -> bool {
    if fetched_rows == 0 {
        return true;
    }
    if (fetched_rows as i64) < limit {
        return true;
    }
    let Some(latest) = events.last() else {
        return false;
    };
    if !latest.session_id.trim().is_empty() {
        return events
            .iter()
            .rev()
            .skip_while(|event| event.session_id == latest.session_id)
            .next()
            .is_some();
    }
    let session_events = current_session_events(events, session_gap_ms);
    if session_events.is_empty() {
        return false;
    }
    if session_events.len() == events.len() {
        return false;
    }
    let boundary_index = events.len().saturating_sub(session_events.len() + 1);
    let Some(boundary_event) = events.get(boundary_index) else {
        return false;
    };
    let Some(session_start_event) = session_events.first() else {
        return false;
    };
    session_start_event
        .created_at_epoch_ms
        .saturating_sub(boundary_event.created_at_epoch_ms)
        > session_gap_ms
}

pub(crate) async fn load_dashboard_current_session_events(
    db: &Client,
    repo_root: &Path,
    include_verify_events: bool,
    session_gap_ms: i64,
) -> Result<Vec<TokenBudgetEvent>> {
    let snapshot_kinds = dashboard_event_snapshot_kinds(include_verify_events);
    let summary = postgres::summarize_observability_snapshots_by_kinds(db, snapshot_kinds).await?;
    let invalidation_epoch_ms = current_dashboard_token_events_invalidation_epoch_ms(repo_root);
    let signature =
        dashboard_token_events_signature(snapshot_kinds, &summary, invalidation_epoch_ms);
    if let Some(events) =
        cached_dashboard_current_session_events(repo_root, &signature, session_gap_ms)
    {
        return Ok(events);
    }
    if let Some(raw_events) = cached_dashboard_token_events(repo_root, snapshot_kinds, &signature) {
        let mut events = filter_dashboard_token_events(raw_events, include_verify_events);
        events.sort_by_key(|event| event.created_at_epoch_ms);
        let events = reconcile_followup_recovery(&events, session_gap_ms);
        let events = current_session_events(&events, session_gap_ms);
        store_dashboard_current_session_events(repo_root, &signature, session_gap_ms, &events);
        return Ok(events);
    }
    let raw_recent_events = load_raw_events(
        db,
        snapshot_kinds,
        Some(DASHBOARD_CURRENT_SESSION_RECENT_EVENTS_LIMIT),
    )
    .await?;
    let fetched_rows = raw_recent_events.len();
    let mut recent_events = filter_dashboard_token_events(raw_recent_events, include_verify_events);
    recent_events.sort_by_key(|event| event.created_at_epoch_ms);
    let recent_events = reconcile_followup_recovery(&recent_events, session_gap_ms);
    if recent_current_session_slice_complete(
        &recent_events,
        fetched_rows,
        DASHBOARD_CURRENT_SESSION_RECENT_EVENTS_LIMIT,
        session_gap_ms,
    ) {
        let events = current_session_events(&recent_events, session_gap_ms);
        store_dashboard_current_session_events(repo_root, &signature, session_gap_ms, &events);
        return Ok(events);
    }

    let mut events = load_dashboard_token_events_with_summary(
        db,
        repo_root,
        include_verify_events,
        snapshot_kinds,
        &summary,
    )
    .await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let events = reconcile_followup_recovery(&events, session_gap_ms);
    let events = current_session_events(&events, session_gap_ms);
    store_dashboard_current_session_events(repo_root, &signature, session_gap_ms, &events);
    Ok(events)
}

pub(crate) fn dashboard_token_events_signature(
    snapshot_kinds: &[&str],
    summary: &[postgres::ObservabilitySnapshotKindSummary],
    invalidation_epoch_ms: i64,
) -> String {
    let payload = json!({
        "snapshot_kinds": snapshot_kinds,
        "invalidation_epoch_ms": invalidation_epoch_ms,
        "summary": summary
            .iter()
            .map(|item| {
                json!({
                    "snapshot_kind": item.snapshot_kind,
                    "snapshots_count": item.snapshots_count,
                    "latest_created_at_epoch_ms": item.latest_created_at_epoch_ms,
                })
            })
            .collect::<Vec<_>>(),
    });
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

#[cfg(test)]
pub(crate) fn dashboard_token_events_signature_from_summary(
    snapshot_kinds: &[&str],
    summary: &[postgres::ObservabilitySnapshotKindSummary],
) -> String {
    dashboard_token_events_signature(snapshot_kinds, summary, 0)
}

pub(crate) fn dashboard_working_state_metadata_signature(
    summary: &[postgres::ObservabilitySnapshotKindSummary],
) -> String {
    let payload = json!({
        "summary": summary
            .iter()
            .map(|item| {
                json!({
                    "snapshot_kind": item.snapshot_kind,
                    "snapshots_count": item.snapshots_count,
                    "latest_created_at_epoch_ms": item.latest_created_at_epoch_ms,
                })
            })
            .collect::<Vec<_>>(),
    });
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(crate) fn cached_dashboard_working_state_metadata(
    signature: &str,
) -> Option<BTreeMap<String, WorkingStateContextPackMeta>> {
    let cache = DASHBOARD_WORKING_STATE_METADATA_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.signature == signature {
        Some(entry.metadata.clone())
    } else {
        None
    }
}

pub(crate) fn store_dashboard_working_state_metadata(
    signature: &str,
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
) {
    let cache = DASHBOARD_WORKING_STATE_METADATA_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    *guard = Some(DashboardWorkingStateMetadataCache {
        signature: signature.to_string(),
        metadata: metadata.clone(),
    });
}

pub(crate) fn filter_context_pack_metadata(
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    context_pack_ids: &BTreeSet<String>,
) -> BTreeMap<String, WorkingStateContextPackMeta> {
    context_pack_ids
        .iter()
        .filter_map(|context_pack_id| {
            metadata
                .get(context_pack_id)
                .cloned()
                .map(|item| (context_pack_id.clone(), item))
        })
        .collect()
}

fn can_shadow_live_report_event(event: &TokenBudgetEvent) -> bool {
    if event.traffic_class == "live" {
        return false;
    }
    event.source_kind.starts_with("proof_")
        || event.source_kind.starts_with("verify_")
        || event.source_kind.starts_with("benchmark_")
        || event.payload_origin == "operator_source_kind_rewrite"
}

fn live_event_is_engineering_runtime_contamination(event: &TokenBudgetEvent) -> bool {
    if event.traffic_class != "live" {
        return false;
    }
    if event.source_kind.starts_with("live_proof_") {
        return true;
    }
    if event.source_kind.starts_with("live_matrix_turn_")
        || event.source_kind.starts_with("live_art_continuity_")
        || event.source_kind.starts_with("live_debug_art_probe")
    {
        return true;
    }
    if event.project.starts_with("proof_") || event.project.starts_with("proofshape_") {
        return true;
    }
    if event.project.starts_with("proof_shape_") {
        return true;
    }
    let thread_id = event.thread_id.as_deref().unwrap_or_default().trim();
    if thread_id.starts_with("proof-")
        || thread_id.starts_with("proof_")
        || thread_id.starts_with("matrix-live-turn-")
        || thread_id.starts_with("art-live-turn-")
        || thread_id.starts_with("art-debug-")
        || thread_id.starts_with("debug-art-")
    {
        return true;
    }
    let turn_id = event.turn_id.as_deref().unwrap_or_default().trim();
    turn_id.starts_with("turn-proof-")
        || turn_id.starts_with("turn_proof_")
        || turn_id.starts_with("turn-art-")
}

fn live_event_has_invalid_runtime_contract(event: &TokenBudgetEvent) -> bool {
    event.traffic_class == "live"
        && event.source_kind == "live_context_pack"
        && matches!(
            event.payload_origin.as_str(),
            "context_pack_token_budget_v6"
                | "context_pack_token_budget_v7"
                | "context_pack_token_budget_v8"
                | "context_pack_token_budget_v9"
                | "context_pack_token_budget_v10"
                | "context_pack_token_budget_v11"
                | "context_pack_token_budget_v13"
                | "context_pack_token_budget_v12"
        )
        && event.retrieval_scope_signature.is_none()
}

fn shadowed_live_event_key(event: &TokenBudgetEvent) -> Option<String> {
    let correlation_id = event.correlation_id.trim();
    if correlation_id.is_empty() {
        return None;
    }
    Some(format!(
        "{}:{}:{}:{}:{}",
        event.project, event.namespace, event.agent_scope, event.measurement_scope, correlation_id
    ))
}

pub(crate) fn suppress_shadowed_live_events(
    events: Vec<TokenBudgetEvent>,
) -> Vec<TokenBudgetEvent> {
    let mut newest_shadow_by_key = BTreeMap::<String, (i64, i64, String)>::new();
    let mut newest_live_by_usage_identity = BTreeMap::<String, (i64, i64, String)>::new();
    for event in &events {
        if let Some(key) = live_usage_identity_shadow_key(event) {
            let version = event_shadow_version_key(event);
            newest_live_by_usage_identity
                .entry(key)
                .and_modify(|current| {
                    if *current < version {
                        *current = version.clone();
                    }
                })
                .or_insert(version);
        }
        if !can_shadow_live_report_event(event) {
            continue;
        }
        let Some(key) = shadowed_live_event_key(event) else {
            continue;
        };
        let version = event_shadow_version_key(event);
        newest_shadow_by_key
            .entry(key)
            .and_modify(|current| {
                if *current < version {
                    *current = version.clone();
                }
            })
            .or_insert(version);
    }
    events
        .into_iter()
        .filter(|event| {
            if let Some(key) = live_usage_identity_shadow_key(event) {
                let current_version = event_shadow_version_key(event);
                if newest_live_by_usage_identity
                    .get(&key)
                    .is_some_and(|latest_version| *latest_version > current_version)
                {
                    return false;
                }
            }
            if event.traffic_class != "live" {
                return true;
            }
            let Some(key) = shadowed_live_event_key(event) else {
                return true;
            };
            let Some(shadow_version) = newest_shadow_by_key.get(&key) else {
                return true;
            };
            *shadow_version < event_shadow_version_key(event)
        })
        .collect()
}

pub(crate) fn parse_snapshot_event(
    row: &ObservabilitySnapshotRecord,
) -> Result<Option<TokenBudgetEvent>> {
    let (node, fallback_source_kind) = match row.snapshot_kind.as_str() {
        "token_budget_event" => (&row.payload["token_budget_event"], None),
        "token_benchmark" => (
            &row.payload["token_benchmark"],
            Some("verify_token_benchmark_legacy"),
        ),
        _ => return Ok(None),
    };
    if !node.is_object() {
        return Ok(None);
    }
    let source_kind = node["source_kind"]
        .as_str()
        .or(fallback_source_kind)
        .unwrap_or("unknown")
        .to_string();
    let traffic_class =
        normalize_token_event_traffic_class(node["traffic_class"].as_str(), &source_kind);
    let project = node["project"]
        .as_str()
        .or_else(|| node["project_code"].as_str())
        .unwrap_or_default()
        .to_string();
    let namespace = node["namespace"]
        .as_str()
        .or_else(|| node["namespace_code"].as_str())
        .unwrap_or_default()
        .to_string();
    let query = node["query"].as_str().unwrap_or_default().to_string();
    let query_hash = node["query_hash"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| hex_sha256(query.as_bytes()));
    let query_type = node["query_type"]
        .as_str()
        .filter(|value| !value.is_empty() && *value != "unknown")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_query_type(&query).to_string());
    let target_kind = node["target_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let baseline_hit_target = node["baseline_hit_target"].as_bool().unwrap_or(false);
    let amai_hit_target = node["amai_hit_target"].as_bool().unwrap_or(false);
    let cold_warm_state = node["cold_warm_state"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let baseline_strategy = node["baseline_strategy"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_baseline_strategy(&query_type).to_string());
    let retrieval_mode = node["retrieval_mode"].as_str().map(ToOwned::to_owned);
    let retrieval_scope_signature = node["retrieval_runtime"]["scope_signature"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let tokenizer = node["tokenizer"].as_str().unwrap_or_default().to_string();
    let latency_ms = node["latency_ms"].as_f64().unwrap_or(0.0);
    let event_id = node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{}-{}", row.snapshot_kind, row.created_at_epoch_ms));
    let correlation_id = node["correlation_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| node["context_pack_id"].as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| event_id.clone());
    let context_pack_id = node["context_pack_id"].as_str().map(ToOwned::to_owned);
    let thread_id = node["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let turn_id = node["turn_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let agent_scope =
        normalize_token_event_agent_scope(node["agent_scope"].as_str(), &project, &namespace);
    let payload_origin = node["payload_origin"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let session_id = node["session_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let rolling_window_profile = node["rolling_window_profile"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let timestamp_utc = node["timestamp_utc"]
        .as_i64()
        .unwrap_or(row.created_at_epoch_ms);
    let occurred_at_epoch_ms = node["occurred_at_epoch_ms"]
        .as_i64()
        .unwrap_or(timestamp_utc);
    let ingested_at_epoch_ms = node["ingested_at_epoch_ms"]
        .as_i64()
        .unwrap_or(row.created_at_epoch_ms);
    let measurement_scope = node["measurement_scope"]
        .as_str()
        .unwrap_or("retrieval_lower_bound")
        .to_string();
    let usage_event_schema_version = node["contract"]["usage_event_schema_version"]
        .as_str()
        .unwrap_or("billing-usage-event-v0")
        .to_string();
    let settlement_statement_version = node["contract"]["settlement_statement_version"]
        .as_str()
        .unwrap_or("settlement-preview-v0")
        .to_string();
    let metering_event_schema_version = node["contract"]["metering_event_schema_version"]
        .as_str()
        .unwrap_or("token-budget-event-v1")
        .to_string();
    let usage_lifecycle_model_version = node["contract"]["usage_lifecycle_model_version"]
        .as_str()
        .unwrap_or("usage-lifecycle-v0")
        .to_string();
    let baseline_method_version = node["contract"]["baseline_method_version"]
        .as_str()
        .unwrap_or("retrieval-baseline-v0")
        .to_string();
    let quality_method_version = node["contract"]["quality_method_version"]
        .as_str()
        .unwrap_or("quality-gate-v0")
        .to_string();
    let coverage_model_version = node["contract"]["coverage_model_version"]
        .as_str()
        .unwrap_or("token-coverage-v0")
        .to_string();
    let metering_freshness_model_version = node["contract"]["metering_freshness_model_version"]
        .as_str()
        .unwrap_or("metering-freshness-v0")
        .to_string();
    let excluded_taxonomy_version = node["contract"]["excluded_taxonomy_version"]
        .as_str()
        .unwrap_or("token-excluded-usage-v0")
        .to_string();
    let dedup_contract_version = node["contract"]["dedup_contract_version"]
        .as_str()
        .unwrap_or("event-id-source-kind-v0")
        .to_string();
    let backfill_policy_version = node["contract"]["backfill_policy_version"]
        .as_str()
        .unwrap_or("report-only-backfill-v0")
        .to_string();
    let correction_policy_version = node["contract"]["correction_policy_version"]
        .as_str()
        .unwrap_or("report-only-correction-v0")
        .to_string();
    let freeze_close_policy_version = node["contract"]["freeze_close_policy_version"]
        .as_str()
        .unwrap_or("freeze-close-v0")
        .to_string();
    let late_arrival_policy_version = node["contract"]["late_arrival_policy_version"]
        .as_str()
        .unwrap_or("late-arrival-v0")
        .to_string();
    let dispute_policy_version = node["contract"]["dispute_policy_version"]
        .as_str()
        .unwrap_or("report-only-dispute-v0")
        .to_string();
    let settlement_lifecycle_model_version = node["contract"]["settlement_lifecycle_model_version"]
        .as_str()
        .unwrap_or("settlement-lifecycle-v0")
        .to_string();
    let statement_period_governance_version =
        node["contract"]["statement_period_governance_version"]
            .as_str()
            .unwrap_or("statement-period-governance-v0")
            .to_string();
    let adjustment_preview_model_version = node["contract"]["adjustment_preview_model_version"]
        .as_str()
        .unwrap_or("adjustment-preview-v0")
        .to_string();
    let adjustment_request_schema_version = node["contract"]["adjustment_request_schema_version"]
        .as_str()
        .unwrap_or("adjustment-request-v0")
        .to_string();
    let adjustment_registry_version = node["contract"]["adjustment_registry_version"]
        .as_str()
        .unwrap_or("adjustment-registry-v0")
        .to_string();
    let rate_card_binding_model_version = node["contract"]["rate_card_binding_model_version"]
        .as_str()
        .unwrap_or("rate-card-binding-v0")
        .to_string();
    let telemetry_surface_split_version = node["contract"]["telemetry_surface_split_version"]
        .as_str()
        .unwrap_or("tokenonomics-surface-split-v0")
        .to_string();
    let event_time_policy_version = node["contract"]["event_time_policy_version"]
        .as_str()
        .unwrap_or("client-visible-ingest-v0")
        .to_string();
    let billing_policy_version = node["contract"]["billing_policy_version"]
        .as_str()
        .unwrap_or("report-only-v0")
        .to_string();
    let suitability_model_version = node["contract"]["suitability_model_version"]
        .as_str()
        .unwrap_or("token-suitability-v0")
        .to_string();
    let billing_mode = node["contract"]["billing_mode"]
        .as_str()
        .unwrap_or("report_only")
        .to_string();
    let reconciliation_contract_version = node["contract"]["reconciliation_contract_version"]
        .as_str()
        .unwrap_or("provider-reconciliation-v0")
        .to_string();
    let margin_model_version = node["contract"]["margin_model_version"]
        .as_str()
        .unwrap_or("margin-view-v0")
        .to_string();
    let infra_cost_profile_version = node["contract"]["infra_cost_profile_version"]
        .as_str()
        .unwrap_or("unpriced-infra-v0")
        .to_string();
    let contractual_evidence_pack_version = node["contract"]["contractual_evidence_pack_version"]
        .as_str()
        .unwrap_or("contractual-evidence-pack-v0")
        .to_string();
    let rate_card_version = node["contract"]["rate_card_version"]
        .as_str()
        .unwrap_or("unpriced-v0")
        .to_string();
    let currency_profile = node["contract"]["currency_profile"]
        .as_str()
        .unwrap_or("unpriced")
        .to_string();
    let settlement_status = node["contract"]["settlement_status"]
        .as_str()
        .unwrap_or("unsettled_report_only")
        .to_string();
    let saved_tokens = node["savings"]["saved_tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"]
        .as_u64()
        .or_else(|| node["baseline_tokens"].as_u64())
        .unwrap_or(0);
    let context_tokens = node["context_pack_render"]["tokens"]
        .as_u64()
        .or_else(|| node["delivered_tokens"].as_u64())
        .unwrap_or(0);
    let recovery_tokens = node["recovery"]["recovery_tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens = node["savings"]["effective_saved_tokens"]
        .as_i64()
        .unwrap_or_else(|| naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
    let savings_factor = node["savings"]["savings_factor"].as_f64().unwrap_or(0.0);
    let savings_percent = node["savings"]["savings_percent"]
        .as_f64()
        .or_else(|| node["gross_savings_pct"].as_f64())
        .unwrap_or(0.0);
    let effective_savings_percent = node["savings"]["effective_savings_percent"]
        .as_f64()
        .unwrap_or_else(|| percent_from_signed(effective_saved_tokens, naive_tokens));
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let quality_score = node["quality"]["quality_score"]
        .as_f64()
        .unwrap_or(if quality_ok { 1.0 } else { 0.0 });
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or(if node["quality"].is_object() {
            "unknown"
        } else {
            "legacy_unverified"
        })
        .to_string();
    let quality_tier = node["quality"]["quality_tier"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let head_hit_target = node["quality"]["head_hit_target"]
        .as_bool()
        .unwrap_or(false);
    let needed_followup = node["followup"]["needed_followup"]
        .as_bool()
        .unwrap_or(!quality_ok);
    let followup_count = node["followup"]["followup_count"].as_u64().unwrap_or(0);
    let followup_of_event_id = node["followup"]["followup_of_event_id"]
        .as_str()
        .map(ToOwned::to_owned);
    let resolved_by_event_id = node["followup"]["resolved_by_event_id"]
        .as_str()
        .map(ToOwned::to_owned);
    let fallback_triggered = node["recovery"]["fallback_triggered"]
        .as_bool()
        .unwrap_or(false);
    let fallback_count = node["recovery"]["fallback_count"].as_u64().unwrap_or(0);
    let document_hits = node["shape"]["document_hits"].as_u64().unwrap_or(0);
    let symbol_hits_count = node["shape"]["symbol_hits"].as_u64().unwrap_or(0);
    let file_hits = node["shape"]["file_hits"].as_u64().unwrap_or(0);
    let sources_count = node["shape"]["sources_count"].as_u64().unwrap_or(0);
    let chunks_count = node["shape"]["chunks_count"].as_u64().unwrap_or(0);
    let pack_token_count = node["shape"]["pack_token_count"]
        .as_u64()
        .unwrap_or(context_tokens);
    let deduped_token_count = node["shape"]["deduped_token_count"]
        .as_u64()
        .unwrap_or(context_tokens);
    let client_prompt_tokens = node["whole_cycle_observed"]["client_prompt_tokens"].as_u64();
    let assistant_generation_tokens =
        node["whole_cycle_observed"]["assistant_generation_tokens"].as_u64();
    let tool_overhead_tokens = node["whole_cycle_observed"]["tool_overhead_tokens"].as_u64();
    let continuity_restore_tokens =
        node["whole_cycle_observed"]["continuity_restore_tokens"].as_u64();
    let tool_overhead_source = node["whole_cycle_observed_source"]["tool_overhead"]
        .as_object()
        .map(|_| node["whole_cycle_observed_source"]["tool_overhead"].clone());
    let pre_amai_baseline_source = node["pre_amai_baseline_source"]
        .as_object()
        .map(|_| node["pre_amai_baseline_source"].clone());

    Ok(Some(TokenBudgetEvent {
        snapshot_id: Some(row.snapshot_id),
        created_at_epoch_ms: row.created_at_epoch_ms,
        event_id,
        correlation_id,
        context_pack_id,
        thread_id,
        turn_id,
        agent_scope,
        payload_origin,
        session_id,
        rolling_window_profile,
        timestamp_utc,
        occurred_at_epoch_ms,
        ingested_at_epoch_ms,
        snapshot_kind: row.snapshot_kind.clone(),
        source_kind,
        traffic_class,
        measurement_scope,
        usage_event_schema_version,
        settlement_statement_version,
        metering_event_schema_version,
        usage_lifecycle_model_version,
        baseline_method_version,
        quality_method_version,
        coverage_model_version,
        metering_freshness_model_version,
        excluded_taxonomy_version,
        dedup_contract_version,
        backfill_policy_version,
        correction_policy_version,
        freeze_close_policy_version,
        late_arrival_policy_version,
        dispute_policy_version,
        settlement_lifecycle_model_version,
        statement_period_governance_version,
        adjustment_preview_model_version,
        adjustment_request_schema_version,
        adjustment_registry_version,
        rate_card_binding_model_version,
        telemetry_surface_split_version,
        event_time_policy_version,
        billing_policy_version,
        suitability_model_version,
        billing_mode,
        reconciliation_contract_version,
        margin_model_version,
        infra_cost_profile_version,
        contractual_evidence_pack_version,
        rate_card_version,
        currency_profile,
        settlement_status,
        project,
        namespace,
        query,
        query_hash,
        query_type,
        target_kind,
        baseline_hit_target,
        amai_hit_target,
        cold_warm_state,
        baseline_strategy,
        retrieval_mode,
        retrieval_scope_signature,
        tokenizer,
        latency_ms,
        saved_tokens,
        naive_tokens,
        context_tokens,
        recovery_tokens,
        effective_saved_tokens,
        savings_factor,
        savings_percent,
        effective_savings_percent,
        quality_ok,
        quality_score,
        quality_method,
        quality_tier,
        head_hit_target,
        needed_followup,
        followup_count,
        followup_of_event_id,
        resolved_by_event_id,
        fallback_triggered,
        fallback_count,
        document_hits,
        symbol_hits_count,
        file_hits,
        sources_count,
        chunks_count,
        pack_token_count,
        deduped_token_count,
        client_prompt_tokens,
        assistant_generation_tokens,
        tool_overhead_tokens,
        continuity_restore_tokens,
        tool_overhead_source,
        pre_amai_baseline_source,
    }))
}

pub(crate) fn needs_live_reverification(payload: &Value) -> bool {
    let node = &payload["token_budget_event"];
    if !node.is_object() {
        return false;
    }
    let source_kind = node["source_kind"].as_str().unwrap_or_default();
    let traffic_class =
        normalize_token_event_traffic_class(node["traffic_class"].as_str(), source_kind);
    if traffic_class != "live" {
        return false;
    }
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or_default();
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let needs_shape_upgrade = node["target_kind"]
        .as_str()
        .map(|value| value.is_empty() || value == "unknown")
        .unwrap_or(true)
        || node.get("latency_ms").is_none()
        || node["quality"].get("quality_tier").is_none()
        || node["quality"].get("head_hit_target").is_none()
        || node["shape"].get("pack_token_count").is_none()
        || node["shape"].get("deduped_token_count").is_none()
        || node["followup"].is_null()
        || node["shape"].get("file_hits").is_none();
    quality_method == "legacy_unverified"
        || (quality_method.is_empty() && !quality_ok)
        || needs_shape_upgrade
}

pub(crate) fn matches_token_ledger_repair_selector(
    event: &TokenBudgetEvent,
    request: &TokenLedgerRepairRequest,
) -> bool {
    if request
        .project
        .as_deref()
        .is_some_and(|expected| event.project != expected)
    {
        return false;
    }
    if request
        .project_prefix
        .as_deref()
        .is_some_and(|prefix| !event.project.starts_with(prefix))
    {
        return false;
    }
    if request
        .namespace
        .as_deref()
        .is_some_and(|expected| event.namespace != expected)
    {
        return false;
    }
    if request
        .source_kind
        .as_deref()
        .is_some_and(|expected| event.source_kind != expected)
    {
        return false;
    }
    if request
        .correlation_id
        .as_deref()
        .is_some_and(|expected| event.correlation_id != expected)
    {
        return false;
    }
    true
}

pub(crate) fn rewrite_token_ledger_source_kind_payload(
    row: &ObservabilitySnapshotRecord,
    rewrite_source_kind: &str,
    repair_reason: &str,
) -> Result<Option<Value>> {
    let mut updated = row.payload.clone();
    let root = updated
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload root is not an object"))?;
    let node = root
        .get_mut("token_budget_event")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event"))?;
    let previous_source_kind = node
        .get("source_kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let previous_traffic_class = node
        .get("traffic_class")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_traffic_class(&previous_source_kind));
    let rewritten_traffic_class = derive_traffic_class(rewrite_source_kind);
    if previous_source_kind == rewrite_source_kind
        && previous_traffic_class == rewritten_traffic_class
    {
        return Ok(None);
    }

    let event_id = node
        .get("event_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let correlation_id = node
        .get("correlation_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            node.get("context_pack_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| event_id.clone());
    let payload_origin = if rewritten_traffic_class == "live" {
        "context_pack_token_budget".to_string()
    } else {
        rewrite_source_kind.to_string()
    };
    node.insert(
        "source_kind".to_string(),
        Value::String(rewrite_source_kind.to_string()),
    );
    node.insert(
        "traffic_class".to_string(),
        Value::String(rewritten_traffic_class.clone()),
    );
    node.insert(
        "payload_origin".to_string(),
        Value::String(payload_origin.clone()),
    );
    node.insert(
        "usage_identity".to_string(),
        json!({
            "dedup_key": usage_dedup_key(rewrite_source_kind, &event_id),
            "idempotency_scope": "source_kind + event_id",
            "canonical_window_time_field": "occurred_at_epoch_ms",
            "event_id": event_id,
            "correlation_id": correlation_id,
        }),
    );
    let repair = ensure_nested_object(node, "repair")?;
    repair.insert(
        "operator_source_kind_rewrite".to_string(),
        json!({
            "repaired_at_utc": current_epoch_ms()?,
            "repair_reason": repair_reason,
            "previous_source_kind": previous_source_kind,
            "previous_traffic_class": previous_traffic_class,
            "rewritten_source_kind": rewrite_source_kind,
            "rewritten_traffic_class": rewritten_traffic_class,
            "rewritten_payload_origin": payload_origin,
        }),
    );
    let observability = ensure_nested_object(root, "_observability")?;
    observability.insert(
        "source_kind".to_string(),
        Value::String(rewrite_source_kind.to_string()),
    );

    let rebuilt = parse_snapshot_event(&ObservabilitySnapshotRecord {
        snapshot_id: row.snapshot_id,
        snapshot_kind: row.snapshot_kind.clone(),
        payload: updated.clone(),
        created_at_epoch_ms: row.created_at_epoch_ms,
    })?
    .ok_or_else(|| anyhow!("failed to rebuild token budget event after source kind rewrite"))?;
    let node = updated["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event after rebuild"))?;
    node.insert(
        "usage_state".to_string(),
        json!({
            "lifecycle_status": usage_lifecycle_status(&rebuilt),
            "reporting_layer": usage_reporting_layer(&rebuilt),
            "included_in_verified_rollup": usage_excluded_reason_code(&rebuilt).is_none(),
            "excluded_reason_code": usage_excluded_reason_code(&rebuilt),
            "backfill_status": usage_backfill_status(&rebuilt),
            "settlement_status": rebuilt.settlement_status,
        }),
    );
    Ok(Some(updated))
}

pub(crate) fn usage_dedup_key(source_kind: &str, event_id: &str) -> String {
    format!("{source_kind}:{event_id}")
}

pub(crate) fn usage_excluded_reason_code(event: &TokenBudgetEvent) -> Option<&'static str> {
    if event.traffic_class != "live" {
        return Some("non_live_other");
    }
    if event.quality_ok {
        return None;
    }
    Some(excluded_event_code(event))
}

pub(crate) fn usage_lifecycle_status(event: &TokenBudgetEvent) -> &'static str {
    match usage_excluded_reason_code(event) {
        None => "verified_included",
        Some("quality_gate_failed") => "excluded_quality_gate_failed",
        Some("awaiting_followup_reconciliation") => "excluded_awaiting_followup_reconciliation",
        Some("legacy_unverified") => "excluded_legacy_unverified",
        Some(_) => "excluded_non_live",
    }
}

pub(crate) fn usage_reporting_layer(event: &TokenBudgetEvent) -> &'static str {
    if usage_excluded_reason_code(event).is_none() {
        "measured_non_billable"
    } else {
        "excluded"
    }
}

pub(crate) fn usage_backfill_status(event: &TokenBudgetEvent) -> &'static str {
    if event.traffic_class != "live" {
        "synthetic_ingest"
    } else if event.payload_origin == "reverified_live_context_pack" {
        "reverified_backfill"
    } else if event.metering_event_schema_version != default_metering_event_schema_version() {
        "legacy_ingest"
    } else {
        "live_ingest"
    }
}

pub(crate) async fn reverify_live_event_payload(
    cfg: &AppConfig,
    db: &mut Client,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    row: &ObservabilitySnapshotRecord,
) -> Result<Option<Value>> {
    let node = &row.payload["token_budget_event"];
    if !node.is_object() {
        return Ok(None);
    }

    let project = node["project"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing project"))?;
    let namespace = node["namespace"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing namespace"))?;
    let query = node["query"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing query"))?;

    let args = ContextPackArgs {
        project: project.to_string(),
        namespace: namespace.to_string(),
        query: query.to_string(),
        retrieval_mode: node["retrieval_mode"].as_str().map(ToOwned::to_owned),
        disable_cache: false,
        limit_documents: 5,
        limit_symbols: 8,
        limit_chunks: 8,
        limit_semantic_chunks: 8,
        at_epoch_ms: None,
        token_source_kind: "proof_reverify_context_pack".to_string(),
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };

    let result =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &args, false, false).await?;
    let source_kind = node["source_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("live_context_pack");
    let mut rebuilt = build_event_payload(
        &result.payload,
        measurement,
        contract,
        source_kind,
        "reverified_live_context_pack",
    )?;
    apply_reverification_metadata(&mut rebuilt, node, row.created_at_epoch_ms)?;
    Ok(Some(rebuilt))
}

pub(crate) fn apply_reverification_metadata(
    rebuilt_payload: &mut Value,
    original_node: &Value,
    fallback_timestamp_utc: i64,
) -> Result<()> {
    let target_kind_owned = rebuilt_payload["token_budget_event"]["target_kind"]
        .as_str()
        .unwrap_or("file")
        .to_string();
    let exact_hits = rebuilt_payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len);
    let symbol_hits = rebuilt_payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len);
    let lexical_hits = rebuilt_payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_hits = rebuilt_payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let rebuilt_node = rebuilt_payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("rebuilt token event payload missing token_budget_event object"))?;

    let event_id = original_node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp_utc = original_node["timestamp_utc"]
        .as_i64()
        .unwrap_or(fallback_timestamp_utc);
    let source_kind = original_node["source_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("live_context_pack");
    let quality_ok = rebuilt_node
        .get("quality")
        .and_then(|value| value.get("quality_ok"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reverified_at_utc = current_epoch_ms()?;

    rebuilt_node.insert("event_id".to_string(), Value::String(event_id));
    rebuilt_node.insert("timestamp_utc".to_string(), Value::from(timestamp_utc));
    rebuilt_node.insert(
        "source_kind".to_string(),
        Value::String(source_kind.to_string()),
    );
    rebuilt_node.insert(
        "traffic_class".to_string(),
        Value::String(derive_traffic_class(source_kind)),
    );
    rebuilt_node.insert(
        "payload_origin".to_string(),
        Value::String("reverified_live_context_pack".to_string()),
    );
    if let Some(quality) = rebuilt_node
        .get_mut("quality")
        .and_then(Value::as_object_mut)
    {
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
                    "reverified_answer_proxy".to_string()
                } else if head_hit_target {
                    "reverified_task_proxy".to_string()
                } else {
                    "reverified_retrieval_parity".to_string()
                }
            } else {
                "reverified_retrieval_miss".to_string()
            }),
        );
        quality.insert(
            "quality_tier".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "answer_proxy".to_string()
                } else if head_hit_target {
                    "task_proxy".to_string()
                } else {
                    "retrieval".to_string()
                }
            } else {
                "partial".to_string()
            }),
        );
        quality.insert(
            "reverified_at_utc".to_string(),
            Value::from(reverified_at_utc),
        );
    }
    rebuilt_node.insert(
        "reverification".to_string(),
        json!({
            "reverified_at_utc": reverified_at_utc,
            "previous_quality_method": original_node["quality"]["quality_method"]
                .as_str()
                .unwrap_or("missing"),
            "previous_quality_ok": original_node["quality"]["quality_ok"]
                .as_bool()
                .unwrap_or(false),
        }),
    );
    Ok(())
}

pub(crate) fn repair_legacy_token_event_payload(payload: &Value) -> Option<Value> {
    let mut updated = payload.clone();
    let node = updated.get_mut("token_budget_event")?;
    let object = node.as_object_mut()?;
    let source_kind = object
        .get("source_kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let query = object
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let query_type = object
        .get("query_type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && *value != "unknown")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_query_type(query).to_string());
    let mut changed = false;

    if !object.contains_key("traffic_class") {
        object.insert(
            "traffic_class".to_string(),
            Value::String(derive_traffic_class(source_kind)),
        );
        changed = true;
    }
    if !object.contains_key("query_type") {
        object.insert("query_type".to_string(), Value::String(query_type.clone()));
        changed = true;
    }
    if !object.contains_key("baseline_strategy") {
        object.insert(
            "baseline_strategy".to_string(),
            Value::String(derive_baseline_strategy(&query_type).to_string()),
        );
        changed = true;
    }
    if !object.contains_key("recovery") {
        object.insert(
            "recovery".to_string(),
            json!({
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0
            }),
        );
        changed = true;
    }
    if !object.contains_key("shape") {
        object.insert(
            "shape".to_string(),
            json!({
                "sources_count": 0,
                "chunks_count": 0
            }),
        );
        changed = true;
    }
    if !object.contains_key("quality") {
        object.insert(
            "quality".to_string(),
            json!({
                "quality_ok": false,
                "quality_score": 0.0,
                "quality_method": "legacy_unverified",
                "quality_tier": "unverified",
                "head_hit_target": false
            }),
        );
        changed = true;
    }
    let naive_tokens = object
        .get("naive_scope")
        .and_then(|value| value.get("tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let context_tokens = object
        .get("context_pack_render")
        .and_then(|value| value.get("tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let recovery_tokens = object
        .get("recovery")
        .and_then(|value| value.get("recovery_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if let Some(savings) = object.get_mut("savings").and_then(Value::as_object_mut) {
        if !savings.contains_key("effective_saved_tokens") {
            savings.insert(
                "effective_saved_tokens".to_string(),
                Value::from(naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64)),
            );
            changed = true;
        }
        if !savings.contains_key("effective_savings_percent") {
            let effective_saved_tokens = savings
                .get("effective_saved_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
            savings.insert(
                "effective_savings_percent".to_string(),
                Value::from(percent_from_signed(effective_saved_tokens, naive_tokens)),
            );
            changed = true;
        }
    }

    changed.then_some(updated)
}

pub(crate) fn current_session_events(
    events: &[TokenBudgetEvent],
    session_gap_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let Some(latest) = events.last() else {
        return Vec::new();
    };
    if !latest.session_id.trim().is_empty() {
        let session = events
            .iter()
            .filter(|event| event.session_id == latest.session_id)
            .cloned()
            .collect::<Vec<_>>();
        if !session.is_empty() {
            return session;
        }
    }
    let mut session = vec![latest.clone()];
    let mut newer_ts = latest.created_at_epoch_ms;
    for event in events.iter().rev().skip(1) {
        if newer_ts.saturating_sub(event.created_at_epoch_ms) > session_gap_ms {
            break;
        }
        session.push(event.clone());
        newer_ts = event.created_at_epoch_ms;
    }
    session.reverse();
    session
}

fn source_kind_starts_new_session(source_kind: &str) -> bool {
    source_kind.ends_with("continuity_startup")
}

pub(crate) fn active_same_meter_scope_events(
    session_events: &[TokenBudgetEvent],
    rolling_window_events: &[TokenBudgetEvent],
) -> Vec<TokenBudgetEvent> {
    let mut seen = BTreeSet::new();
    let mut scoped = Vec::new();
    for event in session_events.iter().chain(rolling_window_events.iter()) {
        if seen.insert(event.event_id.clone()) {
            scoped.push(event.clone());
        }
    }
    scoped
}

pub(crate) fn resolve_session_id(
    events: &[TokenBudgetEvent],
    current_ts: i64,
    session_gap_ms: i64,
    current_source_kind: &str,
    current_project: &str,
    current_namespace: &str,
    current_agent_scope: &str,
) -> String {
    if source_kind_starts_new_session(current_source_kind) {
        return Uuid::new_v4().to_string();
    }
    events
        .iter()
        .rev()
        .find(|event| {
            event.traffic_class == "live"
                && event.project == current_project
                && event.namespace == current_namespace
                && event.agent_scope == current_agent_scope
                && current_ts.saturating_sub(event.created_at_epoch_ms) <= session_gap_ms
        })
        .map(|event| {
            if event.session_id.is_empty() {
                event.event_id.clone()
            } else {
                event.session_id.clone()
            }
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

pub(crate) fn set_recovery_penalty(
    payload: &mut Value,
    recovery_tokens: u64,
    followup_count: u64,
) -> Result<()> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
    let recovery = ensure_nested_object(node, "recovery")?;
    recovery.insert("recovery_tokens".to_string(), Value::from(recovery_tokens));
    let followup = ensure_nested_object(node, "followup")?;
    followup.insert("followup_count".to_string(), Value::from(followup_count));

    let context_tokens = node["context_pack_render"]["tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens =
        naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64);
    let effective_savings_percent = percent_from_signed(effective_saved_tokens, naive_tokens);
    let savings = ensure_nested_object(node, "savings")?;
    savings.insert(
        "effective_saved_tokens".to_string(),
        Value::from(effective_saved_tokens),
    );
    savings.insert(
        "effective_savings_percent".to_string(),
        Value::from(effective_savings_percent),
    );
    Ok(())
}

pub(crate) fn ensure_nested_object<'a>(
    parent: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    if !parent.get(key).is_some_and(Value::is_object) {
        parent.insert(key.to_string(), json!({}));
    }
    parent
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("payload field {key} is not an object"))
}

pub(crate) fn reconcile_followup_recovery(
    events: &[TokenBudgetEvent],
    session_gap_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let mut reconciled = events.to_vec();
    for current_index in 1..reconciled.len() {
        if reconciled[current_index].traffic_class != "live"
            || reconciled[current_index].followup_of_event_id.is_some()
        {
            continue;
        }
        let current_ts = reconciled[current_index].created_at_epoch_ms;
        let current_project = reconciled[current_index].project.clone();
        let current_namespace = reconciled[current_index].namespace.clone();
        let current_key = followup_event_key(&reconciled[current_index]);

        for previous_index in (0..current_index).rev() {
            if reconciled[previous_index].traffic_class != "live"
                || !reconciled[previous_index].needed_followup
                || reconciled[previous_index].resolved_by_event_id.is_some()
            {
                continue;
            }
            if current_ts.saturating_sub(reconciled[previous_index].created_at_epoch_ms)
                > session_gap_ms
            {
                break;
            }
            if reconciled[previous_index].project != current_project
                || reconciled[previous_index].namespace != current_namespace
            {
                continue;
            }
            if !followup_queries_related(
                followup_event_key(&reconciled[previous_index]),
                current_key,
            ) {
                continue;
            }
            let recovery_tokens = reconciled[current_index].recovery_tokens.saturating_add(
                reconciled[previous_index]
                    .context_tokens
                    .saturating_add(reconciled[previous_index].recovery_tokens),
            );
            reconciled[current_index].recovery_tokens = recovery_tokens;
            reconciled[current_index].followup_count =
                reconciled[previous_index].followup_count.saturating_add(1);
            reconciled[current_index].followup_of_event_id =
                Some(reconciled[previous_index].event_id.clone());
            reconciled[current_index].effective_saved_tokens = reconciled[current_index]
                .naive_tokens as i64
                - (reconciled[current_index].context_tokens as i64 + recovery_tokens as i64);
            reconciled[current_index].effective_savings_percent = percent_from_signed(
                reconciled[current_index].effective_saved_tokens,
                reconciled[current_index].naive_tokens,
            );
            reconciled[previous_index].resolved_by_event_id =
                Some(reconciled[current_index].event_id.clone());
            break;
        }
    }
    reconciled
}

pub(crate) fn followup_event_key(event: &TokenBudgetEvent) -> FollowupEventKey<'_> {
    FollowupEventKey {
        query: &event.query,
        query_hash: &event.query_hash,
        query_type: &event.query_type,
        target_kind: &event.target_kind,
    }
}

pub(crate) fn followup_queries_related(
    current: FollowupEventKey<'_>,
    follower: FollowupEventKey<'_>,
) -> bool {
    if !current.query_hash.is_empty() && current.query_hash == follower.query_hash {
        return true;
    }
    if current.query_type != follower.query_type {
        return false;
    }
    if current.target_kind != follower.target_kind {
        return false;
    }
    if normalized_query(current.query) == normalized_query(follower.query) {
        return true;
    }
    query_terms_overlap_count(current.query, follower.query) >= 2
}

fn query_terms_overlap_count(left: &str, right: &str) -> usize {
    let left_terms = extract_query_terms(left);
    if left_terms.is_empty() {
        return 0;
    }
    let right_terms = extract_query_terms(right);
    if right_terms.is_empty() {
        return 0;
    }
    let right_set = right_terms.into_iter().collect::<HashSet<_>>();
    left_terms
        .into_iter()
        .filter(|term| right_set.contains(term))
        .count()
}

fn normalized_query(query: &str) -> String {
    extract_query_terms(query).join(" ")
}
