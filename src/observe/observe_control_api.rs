use super::*;

pub(super) async fn client_budget_compact_chat_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetCompactChatRequest>,
) -> impl IntoResponse {
    let refresh_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = tokio::time::timeout(
            std::time::Duration::from_secs(6),
            refresh_client_live_meter_on_request(&refresh_state),
        )
        .await
        {
            eprintln!("client_budget_compact_chat preflight refresh timed out: {error:#}");
        }
    });
    let repo_root = match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()),
    )
    .await
    {
        Ok(Ok(path)) => path,
        Ok(Err(error)) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&json!({
                    "status": "down",
                    "error": format!("{error:#}"),
                }))
                .unwrap_or_default(),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&json!({
                    "status": "down",
                    "error": format!("client_budget_compact_chat repo_root timed out: {error:#}"),
                }))
                .unwrap_or_default(),
            )
                .into_response();
        }
    };
    let response: Result<Value> = async {
        let args = ContinuityCompactChatArgs {
            project: request.project.clone(),
            repo_root: Some(repo_root),
            namespace: request.namespace.clone(),
            headline: None,
            next_step: None,
            details_file: None,
            launch_host: request.launch_host,
            runtime_fallback: true,
            skip_handoff: !request.refresh_handoff,
            json: true,
        };
        let mut update = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            continuity::compact_chat_payload(&state.cfg, &args, query.thread_id.as_deref()),
        )
        .await
        .map_err(|_| anyhow!("client_budget_compact_chat timed out"))??;
        tokio::time::timeout(
            std::time::Duration::from_secs(6),
            continuity::maybe_launch_compact_chat_host(&mut update, request.launch_host, false),
        )
        .await
        .map_err(|_| anyhow!("client_budget_compact_chat launch timed out"))??;
        tokio::spawn({
            let cache = state.cache.clone();
            let cfg = state.cfg.clone();
            let bind = state.bind.clone();
            let refresh_ms = state.dashboard_refresh_ms;
            async move {
                if let Err(error) =
                    refresh_observe_cache(cache, cfg, bind, refresh_ms).await
                {
                    eprintln!("client_budget_compact_chat refresh failed: {error:#}");
                }
            }
        });
        let snapshot = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            cached_snapshot_with_meta(&state),
        )
        .await
        .map_err(|_| anyhow!("client_budget_compact_chat snapshot timed out"))??;
        let host_launch_status = update["continuity_compact_chat"]["host_launch"]["status"]
            .as_str()
            .unwrap_or("unknown");
        let notice_kind = match host_launch_status {
            "requested" => "client_budget_compact_chat_launch_requested",
            "bridge_unavailable" => "client_budget_compact_chat_bridge_unavailable",
            "launch_failed" => "client_budget_compact_chat_launch_failed",
            "available_not_requested" => "client_budget_compact_chat_launch_not_requested",
            _ => "client_budget_compact_chat_requested",
        };
        let compact_chat_summary = compact_chat_api_summary(&update["continuity_compact_chat"]);
        Ok(json!({
            "status": "ok",
            "continuity_compact_chat": compact_chat_summary,
            "client_budget_live": dashboard::client_budget_live_payload(&snapshot),
            "chat_notice": {
                "kind": notice_kind,
                "thread_id": query.thread_id.clone(),
                "message_text": update["continuity_compact_chat"]["operator_notice"]["message_text"].clone(),
                "reply_prefix": update["continuity_compact_chat"]["operator_notice"]["reply_prefix"].clone(),
                "exact_chat_command": update["continuity_compact_chat"]["operator_notice"]["exact_chat_command"].clone(),
                "prompt_text": update["continuity_compact_chat"]["chat_start_restore"]["prompt_text"].clone(),
                "prompt_file": update["continuity_compact_chat"]["operator_notice"]["prompt_file"].clone(),
                "client_surface": update["continuity_compact_chat"]["client_surface"].clone(),
                "required_host_action": update["continuity_compact_chat"]["operator_notice"]["required_host_action"].clone(),
                "note": update["continuity_compact_chat"]["operator_notice"]["note"].clone(),
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => {
            let message = format!("{error:#}");
            let status = if message.contains("timed out") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_REQUEST
            };
            let snapshot = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                cached_snapshot_with_meta(&state),
            )
            .await
            .ok()
            .and_then(|result| result.ok());
            let fallback_budget_live = snapshot.as_ref().map(dashboard::client_budget_live_payload);
            (
                status,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string(&json!({
                    "status": "error",
                    "error": message,
                    "client_budget_live": fallback_budget_live,
                }))
                .unwrap_or_default(),
            )
                .into_response()
        }
    }
}

pub(super) fn compact_chat_api_summary(payload: &Value) -> Value {
    json!({
        "project": payload["project"].clone(),
        "namespace": payload["namespace"].clone(),
        "chat_start_restore": {
            "headline": payload["chat_start_restore"]["headline"].clone(),
            "next_step": payload["chat_start_restore"]["next_step"].clone(),
            "prompt_text": payload["chat_start_restore"]["prompt_text"].clone(),
        },
        "operator_notice": {
            "message_text": payload["operator_notice"]["message_text"].clone(),
            "reply_prefix": payload["operator_notice"]["reply_prefix"].clone(),
            "exact_chat_command": payload["operator_notice"]["exact_chat_command"].clone(),
            "prompt_file": payload["operator_notice"]["prompt_file"].clone(),
            "launch_clean_chat_command": payload["operator_notice"]["launch_clean_chat_command"].clone(),
            "launch_clean_chat_fallback_command": payload["operator_notice"]["launch_clean_chat_fallback_command"].clone(),
            "launch_clean_chat_command_kind": payload["operator_notice"]["launch_clean_chat_command_kind"].clone(),
            "manual_fallback_steps": payload["operator_notice"]["manual_fallback_steps"].clone(),
            "required_host_action": payload["operator_notice"]["required_host_action"].clone(),
            "note": payload["operator_notice"]["note"].clone(),
        },
        "client_surface": payload["client_surface"].clone(),
        "host_launch": payload["host_launch"].clone(),
    })
}

pub(super) async fn resolve_continuity_project_and_namespace(
    db: &Client,
    repo_root_string: &str,
    project_code: Option<&str>,
    namespace_code: &str,
) -> Result<(postgres::ProjectRecord, postgres::NamespaceRecord)> {
    let project = if let Some(project_code) = project_code.map(str::trim) {
        if project_code.is_empty() {
            postgres::get_project_by_repo_root(db, repo_root_string).await?
        } else {
            let project = postgres::get_project_by_code(db, project_code).await?;
            if project.repo_root != repo_root_string {
                return Err(anyhow!(
                    "project {project_code} is not bound to repo_root {repo_root_string}"
                ));
            }
            project
        }
    } else {
        postgres::get_project_by_repo_root(db, repo_root_string).await?
    };
    let namespace = postgres::ensure_namespace(
        db,
        project.project_id,
        namespace_code,
        Some("Continuity"),
        "local_strict",
    )
    .await?;
    Ok((project, namespace))
}

fn observe_recent_thread_record_has_connected_model(
    thread: &codex_threads::RecentClientThreadRecord,
) -> bool {
    thread
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

fn observe_proof_like_runtime_marker(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            let lower = value.to_ascii_lowercase();
            value.starts_with("proof-")
                || value.starts_with("proof_")
                || value.starts_with("turn-proof-")
                || value.starts_with("turn_proof_")
                || value.contains("::proof_")
                || value.contains("::proof-")
                || lower.contains("proof_execctl_restore")
                || lower.contains("proof-execctl-restore")
                || lower.contains("execctl_restore_stress")
                || lower.contains("execctl restore stress")
        })
}

pub(super) fn observe_user_visible_client_thread(
    thread: &codex_threads::RecentClientThreadRecord,
) -> bool {
    observe_recent_thread_record_has_connected_model(thread)
        && ![
            Some(thread.thread_id.as_str()),
            Some(thread.title.as_str()),
            thread.agent_nickname.as_deref(),
            thread.agent_role.as_deref(),
        ]
        .into_iter()
        .any(observe_proof_like_runtime_marker)
}

fn observe_thread_record_matches_repo_root(
    thread: &codex_threads::RecentClientThreadRecord,
    repo_root: &Path,
) -> bool {
    let repo_root = repo_root.display().to_string();
    thread.cwd == repo_root || thread.cwd.starts_with(&format!("{repo_root}/"))
}

pub(super) fn evaluate_host_current_thread_control_window_targeting(
    repo_root: &Path,
    target_thread_id: &str,
    recent_threads: &[codex_threads::RecentClientThreadRecord],
) -> Value {
    let visible_threads = recent_threads
        .iter()
        .filter(|thread| observe_user_visible_client_thread(thread))
        .map(|thread| {
            json!({
                "thread_id": thread.thread_id,
                "cwd": thread.cwd,
                "title": thread.title,
                "model": thread.model,
                "updated_at_epoch_ms": thread.updated_at_epoch_s.saturating_mul(1000),
            })
        })
        .collect::<Vec<_>>();
    let visible_count = visible_threads.len();
    let target_thread = visible_threads.iter().find(|thread| {
        thread["thread_id"]
            .as_str()
            .map(str::trim)
            .is_some_and(|value| value == target_thread_id)
    });
    let target_visible = target_thread.is_some();
    let target_repo_root_match = target_thread.is_some_and(|thread| {
        let Some(thread_id) = thread["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return false;
        };
        let record = recent_threads
            .iter()
            .find(|candidate| candidate.thread_id == thread_id);
        record.is_some_and(|record| observe_thread_record_matches_repo_root(record, repo_root))
    });
    let denial_reason = if visible_count == 0 {
        Some("no_visible_model_bound_threads")
    } else if visible_count > 1 {
        Some("ambiguous_multi_window_recent_threads")
    } else if !target_visible {
        Some("target_thread_not_visible_in_recent_threads")
    } else if !target_repo_root_match {
        Some("target_thread_repo_root_mismatch")
    } else {
        None
    };
    json!({
        "status": if denial_reason.is_none() { "allowed" } else { "denied" },
        "allowed": denial_reason.is_none(),
        "target_thread_id": target_thread_id,
        "visible_model_bound_thread_count": visible_count,
        "recent_window_minutes": 30,
        "target_thread_visible": target_visible,
        "target_thread_repo_root_match": target_repo_root_match,
        "denial_reason": denial_reason,
        "visible_model_bound_threads": visible_threads,
    })
}

fn host_current_thread_control_window_targeting_summary(
    repo_root: &Path,
    target_thread_id: &str,
) -> Result<Value> {
    let recent_threads = codex_threads::recent_client_thread_records(30 * 60)?;
    Ok(evaluate_host_current_thread_control_window_targeting(
        repo_root,
        target_thread_id,
        &recent_threads,
    ))
}

async fn execute_host_current_thread_control_launch(surface: &Value) -> Result<Value> {
    let external_launch = surface["external_uri_launch"]
        .as_object()
        .ok_or_else(|| anyhow!("host current-thread control surface missing external launch"))?;
    let uri = external_launch
        .get("uri")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("host current-thread control launch uri is unavailable"))?;
    if !external_launch
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "host current-thread control external launch surface is unavailable"
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            ProcessCommand::new("xdg-open").arg(uri).output(),
        )
        .await
        .context("timed out waiting for xdg-open to return")?
        .context("failed to run xdg-open for same-thread host control")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let status = output
                .status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "terminated-by-signal".to_string());
            let detail = if stderr.is_empty() {
                format!("xdg-open exited with status {status}")
            } else {
                format!("xdg-open exited with status {status}: {stderr}")
            };
            return Err(anyhow!(detail));
        }
        return Ok(json!({
            "launched": true,
            "launch_method": "xdg_open",
            "uri": uri,
            "exit_status": output.status.code(),
            "verification_state": "launch_command_executed_exit_zero",
        }));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = uri;
        Err(anyhow!(
            "server-side same-thread overlay launch is unavailable on this platform"
        ))
    }
}

pub(super) async fn continuity_handoff_api_handler(
    State(state): State<ObserveState>,
    Json(request): Json<ContinuityHandoffRequest>,
) -> impl IntoResponse {
    let response: Result<Value> = async {
        let project_code = request
            .project
            .clone()
            .ok_or_else(|| anyhow!("project is required for continuity handoff API"))?;
        let mut db = postgres::connect_admin(&state.cfg).await?;
        let payload = continuity::handoff_payload_from_parts_with_db(
            &mut db,
            &state.cfg,
            &project_code,
            &request.namespace,
            &request.headline,
            &request.next_step,
            request.details.as_deref().unwrap_or_default(),
            request.resolve_current_goal,
            &request.resolved_headlines,
            &request.resolved_task_ids,
        )
        .await?;
        Ok(json!({
            "status": "ok",
            "continuity_handoff": payload["continuity_handoff"].clone(),
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "down",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

pub async fn client_budget_host_control_launch_payload(
    cfg: &AppConfig,
    args: &ObserveClientBudgetHostControlLaunchArgs,
) -> Result<Value> {
    let repo_root = match args.repo_root.as_deref() {
        Some(path) => path.to_path_buf(),
        None => resolve_request_repo_root_for_project(cfg, args.project.as_deref()).await?,
    };
    let thread_id = args.thread_id.trim();
    if thread_id.is_empty() {
        return Err(anyhow!(
            "thread_id is required for same-thread host control launch"
        ));
    }
    let command_id = if args.compact_window {
        Some(working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID)
    } else {
        args.command_id.as_deref()
    };
    let launch_targeting =
        host_current_thread_control_window_targeting_summary(&repo_root, thread_id)?;
    if !launch_targeting["allowed"].as_bool().unwrap_or(false) {
        let denial_reason = launch_targeting["denial_reason"]
            .as_str()
            .unwrap_or("ambiguous_host_launch_target");
        return Err(anyhow!(
            "same-thread host control launch refused: {denial_reason}"
        ));
    }
    let surface = working_state::build_host_current_thread_control_surface_for_thread_and_command(
        Some(thread_id),
        command_id,
    );
    let launch = execute_host_current_thread_control_launch(&surface).await?;
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let repo_root_string = repo_root.display().to_string();
    let (project, namespace) = resolve_continuity_project_and_namespace(
        &db,
        &repo_root_string,
        args.project.as_deref(),
        &args.namespace,
    )
    .await?;
    let command_id = surface["command_id"]
        .as_str()
        .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID);
    let launched_feedback_kind = working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED;
    working_state::record_host_current_thread_control_feedback_with_thread_hint(
        &db,
        &project,
        &namespace,
        launched_feedback_kind,
        Some(command_id),
        Some(thread_id),
    )
    .await?;
    let _ = write_shared_thread_bound_snapshot_invalidation(&repo_root, thread_id);
    let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
    if let Ok(snapshot) = collect_client_budget_snapshot_from_db(
        &db,
        &repo_root,
        Some(thread_id),
        None,
        restore.as_ref(),
    )
    .await
    {
        materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
            &repo_root, thread_id, &snapshot,
        );
    }
    let client_budget_guard =
        token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
    let client_budget_reply_gate =
        compact_host_control_client_budget_reply_gate(&client_budget_guard);
    let message_text = surface["external_uri_launch"]["observe_api_launch_summary"]
        .as_str()
        .or_else(|| surface["requested_message_text"].as_str())
        .unwrap_or("Запрошен same-thread host control.");
    Ok(json!({
        "status": "ok",
        "client_budget_host_control_launch": {
            "project": {
                "code": project.code.clone(),
                "display_name": project.display_name.clone(),
                "repo_root": project.repo_root.clone(),
            },
            "namespace": {
                "code": namespace.code.clone(),
                "display_name": namespace.display_name.clone(),
            },
            "thread_id": thread_id,
            "command_id": command_id,
            "launch_targeting": launch_targeting,
            "host_current_thread_control": surface,
            "launch": launch,
            "client_budget_reply_gate": client_budget_reply_gate,
            "operator_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "message_text": message_text,
                "feedback_kind": launched_feedback_kind,
                "command_id": command_id,
                "thread_id": thread_id,
            }
        }
    }))
}

pub async fn print_client_budget_host_control_launch(
    cfg: &AppConfig,
    args: &ObserveClientBudgetHostControlLaunchArgs,
) -> Result<()> {
    let payload = client_budget_host_control_launch_payload(cfg, args).await?;
    println!("{}", serde_json::to_string(&payload)?);
    Ok(())
}

pub(super) async fn client_budget_host_control_launch_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetHostControlLaunchRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let repo_root =
        match resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()).await {
            Ok(path) => path,
            Err(error) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&json!({
                        "status": "down",
                        "error": format!("{error:#}"),
                    }))
                    .unwrap_or_default(),
                )
                    .into_response();
            }
        };
    let response: Result<Value> = async {
        let thread_id = query
            .thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("thread_id is required for same-thread host control launch"))?;
        let payload = client_budget_host_control_launch_payload(
            &state.cfg,
            &ObserveClientBudgetHostControlLaunchArgs {
                thread_id: thread_id.to_string(),
                compact_window: false,
                command_id: request.command_id.clone(),
                project: request.project.clone(),
                repo_root: Some(repo_root.clone()),
                namespace: request.namespace.clone(),
            },
        )
        .await?;
        let message_text = payload["client_budget_host_control_launch"]["operator_notice"]
            ["message_text"]
            .as_str()
            .unwrap_or("Запрошен same-thread host control.");
        let launch_summary = client_budget_host_control_launch_api_summary(
            &payload["client_budget_host_control_launch"],
        );
        Ok(json!({
            "status": "ok",
            "client_budget_host_control_launch": launch_summary,
            "chat_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "thread_id": query.thread_id.clone(),
                "message_text": message_text,
                "reply_prefix":
                    payload["client_budget_host_control_launch"]["client_budget_reply_gate"]["reply_prefix"].clone(),
                "feedback_kind": working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED,
                "command_id":
                    payload["client_budget_host_control_launch"]["command_id"].clone(),
                "thread_id_hint": thread_id,
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&json!({
                "status": "error",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}

pub(super) fn client_budget_host_control_launch_api_summary(payload: &Value) -> Value {
    json!({
        "project": payload["project"].clone(),
        "namespace": payload["namespace"].clone(),
        "thread_id": payload["thread_id"].clone(),
        "command_id": payload["command_id"].clone(),
        "launch_targeting": {
            "status": payload["launch_targeting"]["status"].clone(),
            "summary": payload["launch_targeting"]["summary"].clone(),
        },
        "client_budget_reply_gate": {
            "reply_prefix": payload["client_budget_reply_gate"]["reply_prefix"].clone(),
        },
        "operator_notice": payload["operator_notice"].clone(),
    })
}

pub(super) async fn client_budget_host_control_feedback_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetHostControlFeedbackRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let repo_root =
        match resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref()).await {
            Ok(path) => path,
            Err(error) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&json!({
                        "status": "down",
                        "error": format!("{error:#}"),
                    }))
                    .unwrap_or_default(),
                )
                    .into_response();
            }
        };
    let response: Result<Value> = async {
        let db = postgres::connect_admin(&state.cfg).await?;
        postgres::bootstrap_schema(&db, &state.cfg).await?;
        let repo_root_string = repo_root.display().to_string();
        let (project, namespace) = resolve_continuity_project_and_namespace(
            &db,
            &repo_root_string,
            request.project.as_deref(),
            &request.namespace,
        )
        .await?;
        let feedback_kind = working_state::normalize_host_current_thread_control_feedback_kind(
            &request.feedback_kind,
        )
        .ok_or_else(|| {
            anyhow!("host current-thread control feedback must be one of requested, opened, failed")
        })?;
        let command_id = working_state::normalize_host_current_thread_control_command_id(
            request
                .command_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        );
        working_state::record_host_current_thread_control_feedback_with_thread_hint(
            &db,
            &project,
            &namespace,
            feedback_kind,
            Some(command_id),
            query.thread_id.as_deref(),
        )
        .await?;
        if let Some(thread_id) = query
            .thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let _ = write_shared_thread_bound_snapshot_invalidation(&repo_root, thread_id);
            let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
            if let Ok(snapshot) = collect_client_budget_snapshot_from_db(
                &db,
                &repo_root,
                Some(thread_id),
                None,
                restore.as_ref(),
            )
            .await
            {
                materialize_shared_thread_bound_client_budget_surfaces_from_snapshot(
                    &repo_root, thread_id, &snapshot,
                );
            }
            let client_budget_guard =
                token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref())
                    .await?;
            let client_budget_reply_gate =
                compact_host_control_client_budget_reply_gate(&client_budget_guard);
            let message_text =
                working_state::host_current_thread_control_feedback_notice_text_for_command(
                    feedback_kind,
                    Some(command_id),
                );
            return Ok(json!({
                "status": "ok",
                "client_budget_host_control_feedback": {
                    "project": {
                        "code": project.code.clone(),
                        "display_name": project.display_name.clone(),
                        "repo_root": project.repo_root.clone(),
                    },
                    "namespace": {
                        "code": namespace.code.clone(),
                        "display_name": namespace.display_name.clone(),
                    },
                    "thread_id": thread_id,
                    "command_id": command_id,
                    "feedback_kind": feedback_kind,
                    "message_text": message_text,
                    "client_budget_reply_gate": client_budget_reply_gate,
                }
            }));
        }
        let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
        let client_budget_guard =
            token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
        let client_budget_reply_gate =
            compact_host_control_client_budget_reply_gate(&client_budget_guard);
        let message_text =
            working_state::host_current_thread_control_feedback_notice_text_for_command(
                feedback_kind,
                Some(command_id),
            );
        Ok(json!({
            "status": "ok",
            "client_budget_host_control_feedback": {
                "project": {
                    "code": project.code.clone(),
                    "display_name": project.display_name.clone(),
                    "repo_root": project.repo_root.clone(),
                },
                "namespace": {
                    "code": namespace.code.clone(),
                    "display_name": namespace.display_name.clone(),
                },
                "feedback_kind": feedback_kind,
                "command_id": command_id,
                "client_budget_reply_gate": client_budget_reply_gate.clone(),
                "operator_notice": {
                    "kind": format!("host_current_thread_control_feedback_{feedback_kind}"),
                    "message_text": message_text,
                    "feedback_kind": feedback_kind,
                    "command_id": command_id,
                }
            },
            "chat_notice": {
                "kind": format!("host_current_thread_control_feedback_{feedback_kind}"),
                "thread_id": query.thread_id.clone(),
                "message_text": message_text,
                "reply_prefix": client_budget_reply_gate["reply_prefix"].clone(),
                "feedback_kind": feedback_kind,
                "command_id": command_id,
            }
        }))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "error",
                "error": format!("{error:#}"),
            }))
            .unwrap_or_default(),
        )
            .into_response(),
    }
}
