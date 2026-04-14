use super::*;

pub(crate) fn human_dashboard_base_url(bind: &str) -> String {
    dashboard::browser_base_url(bind)
}

pub(crate) async fn collect_snapshot(cfg: &AppConfig) -> Result<Value> {
    build_snapshot(cfg, true).await
}

pub(crate) async fn collect_snapshot_preview(cfg: &AppConfig) -> Result<Value> {
    build_snapshot(cfg, false).await
}

pub(super) async fn collect_budget_snapshot_preview(cfg: &AppConfig) -> Result<Value> {
    let repo_root = discover_repo_root(None)?;
    if let Some(thread_id) = codex_threads::current_thread_id() {
        if let Some(snapshot) = load_shared_budget_snapshot_preview(&repo_root, Some(&thread_id)) {
            return Ok(snapshot);
        }
    }
    collect_client_budget_snapshot_with_thread_hint(
        cfg,
        &repo_root,
        codex_threads::current_thread_id().as_deref(),
        None,
        None,
    )
    .await
}

pub(super) fn load_shared_budget_snapshot_preview(
    repo_root: &Path,
    thread_id: Option<&str>,
) -> Option<Value> {
    let thread_id = thread_id.map(str::trim).filter(|value| !value.is_empty())?;
    load_shared_thread_bound_budget_snapshot_preview(repo_root, current_epoch_ms_u64(), thread_id)
}

pub(super) async fn latest_repo_working_state_restore_payload(
    db: &Client,
    repo_root: &Path,
) -> Result<Option<Value>> {
    let repo_root_string = repo_root.display().to_string();
    let project = match postgres::get_project_by_repo_root(db, &repo_root_string).await {
        Ok(project) => project,
        Err(_) => return Ok(None),
    };
    let latest_snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    let Some(snapshot_payload) = latest_snapshot else {
        return Ok(None);
    };
    let mut snapshot_payload = snapshot_payload;
    working_state::ensure_runtime_workspace_restore_pack(&mut snapshot_payload);
    let namespace_code = snapshot_payload["working_state_restore"]["namespace"]["code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(namespace_code) = namespace_code else {
        return Ok(Some(snapshot_payload));
    };
    let namespace =
        match postgres::get_namespace_by_code(db, project.project_id, namespace_code).await {
            Ok(namespace) => namespace,
            Err(_) => return Ok(Some(snapshot_payload)),
        };
    let Some(bundle) =
        working_state::load_recent_restore_bundle_without_live_guard(db, &project, &namespace)
            .await?
    else {
        return Ok(Some(snapshot_payload));
    };
    Ok(Some(json!({
        "working_state_restore": bundle["working_state_restore"].clone()
    })))
}

async fn continuity_restore_bundle_for_repo_root(
    db: &Client,
    repo_root: &Path,
) -> Result<Option<Value>> {
    let repo_root_string = repo_root.display().to_string();
    let project = match postgres::get_project_by_repo_root(db, &repo_root_string).await {
        Ok(project) => project,
        Err(_) => return Ok(None),
    };
    let latest_snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    if let Some(mut snapshot_payload) = latest_snapshot {
        working_state::ensure_runtime_workspace_restore_pack(&mut snapshot_payload);
        return Ok(Some(json!({
            "working_state_restore": snapshot_payload["working_state_restore"].clone()
        })));
    }
    let Some(namespace) =
        postgres::find_namespace_by_code(db, project.project_id, "continuity").await?
    else {
        return Ok(None);
    };
    working_state::load_recent_restore_bundle_without_live_guard(db, &project, &namespace).await
}

async fn reconcile_visible_recent_thread_execctl_activity(db: &Client) -> Result<()> {
    let mut latest_visible_thread_by_repo_root: std::collections::BTreeMap<
        String,
        codex_threads::RecentClientThreadRecord,
    > = std::collections::BTreeMap::new();
    for thread in codex_threads::recent_client_thread_records(30 * 60)?
        .into_iter()
        .filter(observe_user_visible_client_thread)
    {
        let key = thread.cwd.trim().to_string();
        if key.is_empty() {
            continue;
        }
        match latest_visible_thread_by_repo_root.get(&key) {
            Some(existing) if existing.updated_at_epoch_s >= thread.updated_at_epoch_s => {}
            _ => {
                latest_visible_thread_by_repo_root.insert(key, thread);
            }
        }
    }
    let mut recent_threads = latest_visible_thread_by_repo_root
        .into_values()
        .collect::<Vec<_>>();
    recent_threads.sort_by_key(|thread| thread.updated_at_epoch_s);
    for thread in &recent_threads {
        let repo_root = Path::new(&thread.cwd);
        let restore = continuity_restore_bundle_for_repo_root(db, repo_root).await?;
        working_state::maintain_same_thread_execctl_active_lease_for_guard(
            db,
            restore.as_ref(),
            Some(thread.thread_id.as_str()),
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn build_snapshot(cfg: &AppConfig, persist_snapshot: bool) -> Result<Value> {
    let profile = load_profile()?;
    let repo_root = discover_repo_root(None)?;
    let db = postgres::connect_admin(cfg).await?;
    if persist_snapshot {
        return with_postgres_advisory_lock(
            &db,
            OBSERVE_SYSTEM_SNAPSHOT_PERSIST_ADVISORY_LOCK_KEY,
            "failed to acquire observe system snapshot advisory lock",
            "failed to release observe system snapshot advisory lock",
            || async {
                build_snapshot_with_connected_admin_db(cfg, &profile, &repo_root, &db, true).await
            },
        )
        .await;
    }
    build_snapshot_with_connected_admin_db(cfg, &profile, &repo_root, &db, false).await
}

async fn build_snapshot_with_connected_admin_db(
    cfg: &AppConfig,
    profile: &ObservabilityProfile,
    repo_root: &Path,
    db: &Client,
    persist_snapshot: bool,
) -> Result<Value> {
    let snapshot_started = Instant::now();
    if persist_snapshot {
        maybe_cleanup_observability_snapshots_with_db(db).await?;
    }
    let mut observe_refresh_stage_ms = serde_json::Map::new();
    let previous = timed_future(
        &mut observe_refresh_stage_ms,
        "previous_system_snapshot",
        postgres::latest_observability_snapshot(db, "system_snapshot"),
    )
    .await?;
    let http = http_client()?;
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    timed_future(
        &mut observe_refresh_stage_ms,
        "reconcile_recent_visible_thread_execctl_activity",
        reconcile_visible_recent_thread_execctl_activity(db),
    )
    .await?;

    let mut postgres_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_postgres_live",
        collect_postgres_live(db, &profile.snapshot),
    )
    .await?;
    if let Some(object) = postgres_live.as_object_mut() {
        object.insert(
            "captured_at_epoch_ms".to_string(),
            Value::from(captured_at_epoch_ms),
        );
    }
    let qdrant_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_qdrant_live",
        collect_qdrant_live(cfg, &http),
    )
    .await?;
    let benchmark_qdrant_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_benchmark_qdrant_live",
        collect_optional_benchmark_qdrant_live(cfg, &http),
    )
    .await;
    let nats_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_nats_live",
        collect_nats_live(cfg, &http, &profile.snapshot),
    )
    .await?;
    let s3_live = timed_future(
        &mut observe_refresh_stage_ms,
        "collect_s3_live",
        collect_s3_live(cfg),
    )
    .await?;
    let compatibility_report = timed_future(
        &mut observe_refresh_stage_ms,
        "compatibility_check",
        compatibility::check(cfg),
    )
    .await?;

    let latest_hot = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_hot",
        postgres::latest_observability_snapshot(db, "retrieval_benchmark_hot"),
    )
    .await?;
    let latest_cold = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_cold",
        postgres::latest_observability_snapshot(db, "retrieval_benchmark_cold"),
    )
    .await?;
    let latest_index = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_index_project",
        postgres::latest_observability_snapshot(db, "index_project"),
    )
    .await?;
    let latest_accuracy = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_accuracy",
        postgres::latest_observability_snapshot(db, "retrieval_accuracy"),
    )
    .await?;
    let (latest_load_hot, latest_load_hot_raw) = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_load_hot",
        latest_clean_benchmark_snapshot(db, "retrieval_load_hot", "load_verification"),
    )
    .await?;
    let (latest_load_cold, latest_load_cold_raw) = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_retrieval_load_cold",
        latest_clean_benchmark_snapshot(db, "retrieval_load_cold", "load_verification"),
    )
    .await?;
    let latest_token_benchmark = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_token_benchmark",
        postgres::latest_observability_snapshot(db, "token_benchmark"),
    )
    .await?;
    let latest_procedural_benchmark = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_procedural_benchmark",
        postgres::latest_observability_snapshot(db, "procedural_benchmark"),
    )
    .await?;
    let procedural_benchmark_history = timed_future(
        &mut observe_refresh_stage_ms,
        "procedural_benchmark_history",
        procedural_benchmark_history_surface(db),
    )
    .await?;
    let latest_memory_benchmark_score = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_memory_benchmark_score",
        postgres::latest_observability_snapshot(db, "memory_benchmark_score"),
    )
    .await?;
    let latest_cold_path_benchmark = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_cold_path_benchmark",
        latest_dashboard_cold_benchmark_snapshot(db),
    )
    .await?;
    let cold_path_benchmark_progress = read_live_cold_benchmark_progress(repo_root);
    let cold_path_benchmark_progress = timed_future(
        &mut observe_refresh_stage_ms,
        "cold_path_benchmark_progress",
        enrich_live_cold_benchmark_progress(db, cold_path_benchmark_progress),
    )
    .await?;
    let latest_working_state_restore = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_working_state_restore",
        postgres::latest_observability_snapshot(db, "working_state_restore"),
    )
    .await?;
    let latest_repo_working_state_restore = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_repo_working_state_restore",
        latest_repo_working_state_restore_payload(db, repo_root),
    )
    .await?;
    let agent_scope_activity = timed_future(
        &mut observe_refresh_stage_ms,
        "agent_scope_activity",
        token_budget::collect_agent_scope_activity(db),
    )
    .await?;
    let active_agent_budget = timed_future(
        &mut observe_refresh_stage_ms,
        "active_agent_budget",
        token_budget::collect_active_agent_live_budget_surface(
            db,
            repo_root,
            &agent_scope_activity,
        ),
    )
    .await?;
    let latest_degradation_verification = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_degradation_verification",
        postgres::latest_observability_snapshot(db, "degradation_verification"),
    )
    .await?;
    let latest_continuity_verification = timed_future(
        &mut observe_refresh_stage_ms,
        "latest_continuity_verification",
        postgres::latest_observability_snapshot(db, "continuity_verification"),
    )
    .await?;
    let token_budget_report = if !persist_snapshot {
        if let Some(thread_id) = codex_threads::current_thread_id()
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(thread_bound_snapshot) = load_shared_thread_bound_budget_snapshot(
                repo_root,
                current_epoch_ms_u64(),
                thread_id,
            ) {
                thread_bound_snapshot["token_budget_report"].clone()
            } else {
                timed_future(
                    &mut observe_refresh_stage_ms,
                    "token_budget_dashboard_report",
                    token_budget::collect_dashboard_report(db),
                )
                .await?
            }
        } else {
            timed_future(
                &mut observe_refresh_stage_ms,
                "token_budget_dashboard_report",
                token_budget::collect_dashboard_report(db),
            )
            .await?
        }
    } else {
        timed_future(
            &mut observe_refresh_stage_ms,
            "token_budget_dashboard_report",
            token_budget::collect_dashboard_report(db),
        )
        .await?
    };
    let artifact_cleanup_summary = timed_future(
        &mut observe_refresh_stage_ms,
        "artifact_cleanup_summary",
        async { artifact_cleanup::read_latest_summary(repo_root) },
    )
    .await?
    .unwrap_or_else(|| {
        json!({
            "artifact_cleanup": {
                "status": "ещё нет данных"
            }
        })
    });

    let payload = json!({
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "stack_name": cfg.stack_name,
        "thresholds": profile_thresholds_json(profile),
        "postgres": with_postgres_rates(&postgres_live, previous.as_ref()),
        "qdrant": qdrant_live,
        "benchmark_qdrant": benchmark_qdrant_live,
        "nats": nats_live,
        "s3": s3_live,
        "compatibility": compatibility::report_json(&compatibility_report),
        "latest_index_project": latest_index,
        "latest_retrieval_hot": latest_hot,
        "latest_retrieval_cold": latest_cold,
        "latest_retrieval_accuracy": latest_accuracy,
        "latest_retrieval_load_hot": latest_load_hot,
        "latest_retrieval_load_hot_raw": latest_load_hot_raw,
        "latest_retrieval_load_cold": latest_load_cold,
        "latest_retrieval_load_cold_raw": latest_load_cold_raw,
        "latest_token_benchmark": latest_token_benchmark,
        "latest_procedural_benchmark": latest_procedural_benchmark,
        "procedural_benchmark_history": procedural_benchmark_history,
        "latest_memory_benchmark_score": latest_memory_benchmark_score,
        "latest_cold_path_benchmark": latest_cold_path_benchmark,
        "cold_path_benchmark_progress": cold_path_benchmark_progress,
        "latest_working_state_restore": latest_working_state_restore,
        "latest_repo_working_state_restore": latest_repo_working_state_restore,
        "agent_scope_activity": agent_scope_activity,
        "active_agent_budget": active_agent_budget,
        "latest_degradation_verification": latest_degradation_verification,
        "latest_continuity_verification": latest_continuity_verification,
        "token_budget_report": token_budget_report,
        "artifact_cleanup": artifact_cleanup_summary["artifact_cleanup"].clone(),
    });
    let governance_surface = timed_future(
        &mut observe_refresh_stage_ms,
        "governance_surface",
        collect_governance_surface(db),
    )
    .await?;
    let degradation_model = build_degradation_model(&payload)?;
    let continuity_correctness_model = build_continuity_correctness_model(&payload)?;
    let sla = evaluate_sla(&payload, profile);
    let snapshot = json!({
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "stack_name": cfg.stack_name,
        "thresholds": payload["thresholds"].clone(),
        "postgres": payload["postgres"].clone(),
        "qdrant": payload["qdrant"].clone(),
        "benchmark_qdrant": payload["benchmark_qdrant"].clone(),
        "nats": payload["nats"].clone(),
        "s3": payload["s3"].clone(),
        "compatibility": payload["compatibility"].clone(),
        "latest_index_project": payload["latest_index_project"].clone(),
        "latest_retrieval_hot": payload["latest_retrieval_hot"].clone(),
        "latest_retrieval_cold": payload["latest_retrieval_cold"].clone(),
        "latest_retrieval_accuracy": payload["latest_retrieval_accuracy"].clone(),
        "latest_retrieval_load_hot": payload["latest_retrieval_load_hot"].clone(),
        "latest_retrieval_load_hot_raw": payload["latest_retrieval_load_hot_raw"].clone(),
        "latest_retrieval_load_cold": payload["latest_retrieval_load_cold"].clone(),
        "latest_retrieval_load_cold_raw": payload["latest_retrieval_load_cold_raw"].clone(),
        "latest_token_benchmark": payload["latest_token_benchmark"].clone(),
        "latest_procedural_benchmark": payload["latest_procedural_benchmark"].clone(),
        "procedural_benchmark_history": payload["procedural_benchmark_history"].clone(),
        "latest_memory_benchmark_score": payload["latest_memory_benchmark_score"].clone(),
        "latest_cold_path_benchmark": payload["latest_cold_path_benchmark"].clone(),
        "cold_path_benchmark_progress": payload["cold_path_benchmark_progress"].clone(),
        "latest_working_state_restore": payload["latest_working_state_restore"].clone(),
        "latest_repo_working_state_restore": payload["latest_repo_working_state_restore"].clone(),
        "agent_scope_activity": payload["agent_scope_activity"].clone(),
        "active_agent_budget": payload["active_agent_budget"].clone(),
        "latest_degradation_verification": payload["latest_degradation_verification"].clone(),
        "latest_continuity_verification": payload["latest_continuity_verification"].clone(),
        "token_budget_report": payload["token_budget_report"].clone(),
        "client_budget_guard": dashboard::current_session_budget_guard(&payload),
        "artifact_cleanup": payload["artifact_cleanup"].clone(),
        "observe_refresh": {
            "total_ms": snapshot_started.elapsed().as_millis() as u64,
            "stage_ms": observe_refresh_stage_ms,
        },
        "degradation_model": degradation_model,
        "continuity_correctness_model": continuity_correctness_model,
        "governance_surface": governance_surface,
        "sla": sla,
    });
    if persist_snapshot {
        let _ = postgres::insert_observability_snapshot(db, "system_snapshot", &snapshot).await?;
    }
    Ok(snapshot)
}
