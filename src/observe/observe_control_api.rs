use super::*;

const REMEDIATION_BUNDLES_DEFAULT_LIMIT: usize = 50;
const REMEDIATION_BUNDLES_MAX_LIMIT: usize = 200;

fn append_working_state_warning_to_message(base_message: &str, write_status: &Value) -> String {
    let warning = write_status["warning"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match warning {
        Some(warning) => format!("{base_message} {warning}"),
        None => base_message.to_string(),
    }
}

#[cfg(test)]
mod observe_control_warning_tests {
    use super::*;

    #[test]
    fn append_working_state_warning_to_message_keeps_base_without_warning() {
        let message =
            append_working_state_warning_to_message("Host control opened.", &json!({}));
        assert_eq!(message, "Host control opened.");
    }

    #[test]
    fn append_working_state_warning_to_message_appends_degraded_warning() {
        let message = append_working_state_warning_to_message(
            "Host control opened.",
            &json!({
                "status": "degraded_after_primary_write",
                "warning": "restore refresh degraded"
            }),
        );
        assert_eq!(message, "Host control opened. restore refresh degraded");
    }
}

pub(super) async fn remediation_bundles_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<RemediationBundlesQuery>,
) -> impl IntoResponse {
    let repo_root = match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        resolve_request_repo_root_for_project(&state.cfg, query.project.as_deref()),
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
                    "error": format!("remediation_bundles repo_root timed out: {error:#}"),
                }))
                .unwrap_or_default(),
            )
                .into_response();
        }
    };

    let limit = query
        .limit
        .unwrap_or(REMEDIATION_BUNDLES_DEFAULT_LIMIT)
        .clamp(1, REMEDIATION_BUNDLES_MAX_LIMIT);

    match collect_remediation_bundles_payload(&repo_root, limit) {
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

pub(super) async fn remediation_bundle_detail_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<RemediationBundleDetailQuery>,
) -> impl IntoResponse {
    let repo_root = match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        resolve_request_repo_root_for_project(&state.cfg, query.project.as_deref()),
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
                    "error": format!("remediation_bundle_detail repo_root timed out: {error:#}"),
                }))
                .unwrap_or_default(),
            )
                .into_response();
        }
    };

    match collect_remediation_bundle_detail_payload(&repo_root, query.file_name.as_deref()) {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => {
            let message = format!("{error:#}");
            let status = if message.contains("not found") || message.contains("not a file") {
                StatusCode::NOT_FOUND
            } else if message.contains("missing required file_name")
                || message.contains("must be a plain file name")
                || message.contains("must not contain control characters")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            (
                status,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&json!({
                    "status": "error",
                    "error": message,
                }))
                .unwrap_or_default(),
            )
                .into_response()
        }
    }
}

fn collect_remediation_bundles_payload(repo_root: &Path, limit: usize) -> Result<Value> {
    let bundle_dir = crate::indexer::qdrant_postgres_remediation_bundle_dir(repo_root);
    if !bundle_dir.exists() {
        return Ok(json!({
            "status": "ok",
            "read_model_kind": "read_only_remediation_inbox",
            "repo_root": repo_root,
            "bundle_dir": bundle_dir,
            "source_of_truth": "bundle_files_plus_runtime_logs",
            "mutating_actions_available": false,
            "total_items": 0,
            "visible_items": 0,
            "invalid_items": 0,
            "items": [],
        }));
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&bundle_dir).with_context(|| {
        format!(
            "failed to read remediation bundle dir {}",
            bundle_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed to iterate remediation bundle dir {}",
                bundle_dir.display()
            )
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        items.push(read_remediation_bundle_item(&path));
    }
    items.sort_by(|left, right| {
        remediation_bundle_sort_key(right).cmp(&remediation_bundle_sort_key(left))
    });
    let total_items = items.len();
    let invalid_items = items
        .iter()
        .filter(|item| item["status"] != json!("ok"))
        .count();
    if items.len() > limit {
        items.truncate(limit);
    }
    Ok(json!({
        "status": "ok",
        "read_model_kind": "read_only_remediation_inbox",
        "repo_root": repo_root,
        "bundle_dir": bundle_dir,
        "source_of_truth": "bundle_files_plus_runtime_logs",
        "mutating_actions_available": false,
        "total_items": total_items,
        "visible_items": items.len(),
        "invalid_items": invalid_items,
        "items": items,
    }))
}

fn collect_remediation_bundle_detail_payload(
    repo_root: &Path,
    file_name: Option<&str>,
) -> Result<Value> {
    let file_name = file_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("missing required file_name query parameter"))?;
    if Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(file_name)
        || file_name.contains(std::path::MAIN_SEPARATOR)
        || file_name.contains('/')
        || file_name.contains('\\')
    {
        return Err(anyhow!(
            "file_name must be a plain file name without path separators"
        ));
    }
    if file_name.chars().any(|ch| ch.is_control()) {
        return Err(anyhow!("file_name must not contain control characters"));
    }

    let bundle_dir = crate::indexer::qdrant_postgres_remediation_bundle_dir(repo_root);
    let bundle_path = bundle_dir.join(file_name);
    if !bundle_path.exists() {
        return Err(anyhow!(
            "remediation bundle not found: {}",
            bundle_path.display()
        ));
    }
    if !bundle_path.is_file() {
        return Err(anyhow!(
            "remediation bundle path is not a file: {}",
            bundle_path.display()
        ));
    }

    Ok(json!({
        "status": "ok",
        "read_model_kind": "read_only_remediation_bundle_detail",
        "repo_root": repo_root,
        "bundle_dir": bundle_dir,
        "source_of_truth": "bundle_files_plus_runtime_logs",
        "mutating_actions_available": false,
        "item": read_remediation_bundle_item(&bundle_path),
    }))
}

fn read_remediation_bundle_item(path: &Path) -> Value {
    let display_path = path.display().to_string();
    match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(payload)
                if payload["artifact_version"]
                    == json!(
                        crate::indexer::QDRANT_POSTGRES_REMEDIATION_BUNDLE_ARTIFACT_VERSION
                    ) =>
            {
                if let Err(error) = validate_remediation_bundle_payload(&payload) {
                    return json!({
                        "status": "invalid",
                        "artifact_path": display_path,
                        "file_name": path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
                        "error": error,
                        "payload": payload,
                    });
                }
                json!({
                    "status": "ok",
                    "artifact_path": display_path,
                    "file_name": path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
                    "bundle_id": payload["bundle_id"].clone(),
                    "created_at_epoch_ms": payload["created_at_epoch_ms"].clone(),
                    "relative_path": payload["relative_path"].clone(),
                    "document_id": payload["document_id"].clone(),
                    "had_existing_document": payload["had_existing_document"].clone(),
                    "failure_mode": payload["failure_mode"].clone(),
                    "failure_phase": payload["failure_phase"].clone(),
                    "failure_sqlstate": payload["failure_sqlstate"].clone(),
                    "compensation_attempted": payload["compensation_attempted"].clone(),
                    "compensation_succeeded": payload["compensation_succeeded"].clone(),
                    "consistency_state": payload["consistency_state"].clone(),
                    "required_action": payload["required_action"].clone(),
                    "operator_summary": payload["operator_summary"].clone(),
                    "operator_checklist": payload["operator_checklist"].clone(),
                    "observability_stage": payload["observability_stage"].clone(),
                    "payload": payload,
                })
            }
            Ok(payload) => json!({
                "status": "invalid",
                "artifact_path": display_path,
                "file_name": path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
                "error": format!(
                    "unexpected artifact_version: {}",
                    payload["artifact_version"].as_str().unwrap_or("none")
                ),
                "payload": payload,
            }),
            Err(error) => json!({
                "status": "invalid",
                "artifact_path": display_path,
                "file_name": path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
                "error": format!("failed to parse remediation bundle JSON: {error:#}"),
            }),
        },
        Err(error) => json!({
            "status": "invalid",
            "artifact_path": display_path,
            "file_name": path.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
            "error": format!("failed to read remediation bundle: {error:#}"),
        }),
    }
}

fn validate_remediation_bundle_payload(payload: &Value) -> std::result::Result<(), String> {
    let required_string_fields = [
        "bundle_id",
        "relative_path",
        "document_id",
        "failure_mode",
        "failure_phase",
        "consistency_state",
        "required_action",
        "operator_summary",
        "observability_stage",
    ];
    for field in required_string_fields {
        if payload[field].as_str().is_none() {
            return Err(format!(
                "remediation bundle missing required string field: {field}"
            ));
        }
    }
    if payload["created_at_epoch_ms"].as_u64().is_none() {
        return Err(
            "remediation bundle missing required integer field: created_at_epoch_ms".to_string(),
        );
    }
    if payload["had_existing_document"].as_bool().is_none() {
        return Err(
            "remediation bundle missing required boolean field: had_existing_document".to_string(),
        );
    }
    if payload["compensation_attempted"].as_bool().is_none() {
        return Err(
            "remediation bundle missing required boolean field: compensation_attempted".to_string(),
        );
    }
    if !payload["operator_checklist"].is_array() {
        return Err(
            "remediation bundle missing required array field: operator_checklist".to_string(),
        );
    }
    Ok(())
}

fn remediation_bundle_sort_key(item: &Value) -> (u64, String) {
    (
        item["created_at_epoch_ms"].as_u64().unwrap_or(0),
        item["artifact_path"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

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
        Ok(compact_chat_response_payload(
            &update["continuity_compact_chat"],
            &dashboard::client_budget_live_payload(&snapshot),
            query.thread_id.as_deref(),
        ))
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
        "delivery_surface_restore": {
            "headline": payload["chat_start_restore"]["headline"].clone(),
            "next_step": payload["chat_start_restore"]["next_step"].clone(),
            "prompt_text": payload["chat_start_restore"]["prompt_text"].clone(),
        },
        "operator_notice": {
            "kind": payload["operator_notice"]["kind"].clone(),
            "message_text": payload["operator_notice"]["message_text"].clone(),
            "reply_prefix": payload["operator_notice"]["reply_prefix"].clone(),
            "exact_chat_command": payload["operator_notice"]["exact_chat_command"].clone(),
            "prompt_file": payload["operator_notice"]["prompt_file"].clone(),
            "launch_clean_chat_command": payload["operator_notice"]["launch_clean_chat_command"].clone(),
            "launch_clean_chat_fallback_command": payload["operator_notice"]["launch_clean_chat_fallback_command"].clone(),
            "launch_clean_chat_command_kind": payload["operator_notice"]["launch_clean_chat_command_kind"].clone(),
            "clean_chat_launch": payload["operator_notice"]["clean_chat_launch"].clone(),
            "manual_fallback_steps": payload["operator_notice"]["manual_fallback_steps"].clone(),
            "required_host_action": payload["operator_notice"]["required_host_action"].clone(),
            "note": payload["operator_notice"]["note"].clone(),
        },
        "client_surface": payload["client_surface"].clone(),
        "host_launch": payload["host_launch"].clone(),
    })
}

fn compact_chat_response_payload(
    compact_chat_payload: &Value,
    client_budget_live: &Value,
    thread_id: Option<&str>,
) -> Value {
    let compact_chat_summary = compact_chat_api_summary(compact_chat_payload);
    let delivery_surface_notice =
        compact_chat_delivery_surface_notice_payload(compact_chat_payload, thread_id);
    json!({
        "status": "ok",
        "continuity_compact_chat": compact_chat_summary,
        "client_budget_live": client_budget_live.clone(),
        "delivery_surface_notice": delivery_surface_notice.clone(),
        "chat_notice": delivery_surface_notice
    })
}

pub(super) fn compact_chat_notice_kind(host_launch_status: &str) -> &'static str {
    match host_launch_status {
        "requested" => "client_budget_compact_chat_launch_requested",
        "bridge_unavailable" => "client_budget_compact_chat_bridge_unavailable",
        "disabled_by_policy" => "client_budget_compact_chat_launch_disabled_by_policy",
        "launch_failed" => "client_budget_compact_chat_launch_failed",
        "available_not_requested" => "client_budget_compact_chat_launch_not_requested",
        _ => "client_budget_compact_chat_requested",
    }
}

fn compact_chat_delivery_surface_notice_kind(payload: &Value, host_launch_status: &str) -> String {
    payload["operator_notice"]["kind"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| compact_chat_notice_kind(host_launch_status).to_string())
}

fn compact_chat_delivery_surface_notice_thread_id(
    payload: &Value,
    thread_id: Option<&str>,
) -> Value {
    payload["operator_notice"]["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Value::String(value.to_string()))
        .or_else(|| thread_id.map(|value| Value::String(value.to_string())))
        .unwrap_or(Value::Null)
}

fn compact_chat_delivery_surface_notice_payload(payload: &Value, thread_id: Option<&str>) -> Value {
    let host_launch_status = payload["host_launch"]["status"]
        .as_str()
        .unwrap_or("unknown");
    json!({
        "kind": compact_chat_delivery_surface_notice_kind(payload, host_launch_status),
        "thread_id": compact_chat_delivery_surface_notice_thread_id(payload, thread_id),
        "message_text": payload["operator_notice"]["message_text"].clone(),
        "reply_prefix": payload["operator_notice"]["reply_prefix"].clone(),
        "exact_chat_command": payload["operator_notice"]["exact_chat_command"].clone(),
        "prompt_text": payload["chat_start_restore"]["prompt_text"].clone(),
        "prompt_file": payload["operator_notice"]["prompt_file"].clone(),
        "launch_clean_chat_command": payload["operator_notice"]["launch_clean_chat_command"].clone(),
        "launch_clean_chat_fallback_command": payload["operator_notice"]["launch_clean_chat_fallback_command"].clone(),
        "launch_clean_chat_command_kind": payload["operator_notice"]["launch_clean_chat_command_kind"].clone(),
        "clean_chat_launch": payload["operator_notice"]["clean_chat_launch"].clone(),
        "manual_fallback_steps": payload["operator_notice"]["manual_fallback_steps"].clone(),
        "client_surface": payload["client_surface"].clone(),
        "required_host_action": payload["operator_notice"]["required_host_action"].clone(),
        "note": payload["operator_notice"]["note"].clone(),
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
    let working_state_write_status =
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
    let working_state_write_status_value = serde_json::to_value(&working_state_write_status)?;
    let message_text = surface["external_uri_launch"]["observe_api_launch_summary"]
        .as_str()
        .or_else(|| surface["requested_message_text"].as_str())
        .unwrap_or("Запрошен same-thread host control.");
    let message_text =
        append_working_state_warning_to_message(message_text, &working_state_write_status_value);
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
            "working_state_write_status": working_state_write_status_value.clone(),
            "client_budget_reply_gate": client_budget_reply_gate,
            "operator_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "message_text": message_text,
                "reply_prefix": client_budget_reply_gate["reply_prefix"].clone(),
                "feedback_kind": launched_feedback_kind,
                "command_id": command_id,
                "thread_id": thread_id,
                "working_state_write_status": working_state_write_status_value,
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
        let launch_summary = client_budget_host_control_launch_api_summary(
            &payload["client_budget_host_control_launch"],
        );
        Ok(json!({
            "status": "ok",
            "client_budget_host_control_launch": launch_summary,
            "chat_notice": client_budget_host_control_launch_chat_notice(&payload, thread_id)
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
        "working_state_write_status": payload["working_state_write_status"].clone(),
        "client_budget_reply_gate": {
            "reply_prefix": payload["client_budget_reply_gate"]["reply_prefix"].clone(),
        },
        "operator_notice": payload["operator_notice"].clone(),
    })
}

fn client_budget_host_control_launch_chat_notice(payload: &Value, thread_id: &str) -> Value {
    let source_notice =
        &payload["client_budget_host_control_launch"]["operator_notice"];
    let fallback_thread_id = source_notice["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(thread_id);
    let fallback_feedback_kind = payload["client_budget_host_control_launch"]["working_state_write_status"]
        .as_object()
        .map(|_| working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED)
        .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED);
    let fallback_message_text = append_working_state_warning_to_message(
        "Запрошен same-thread host control.",
        &payload["client_budget_host_control_launch"]["working_state_write_status"],
    );
    json!({
        "kind": source_notice["kind"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("host_current_thread_control_launch_opened"),
        "thread_id": fallback_thread_id,
        "message_text": source_notice["message_text"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fallback_message_text),
        "reply_prefix": source_notice["reply_prefix"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                payload["client_budget_host_control_launch"]["client_budget_reply_gate"]["reply_prefix"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            }),
        "feedback_kind": source_notice["feedback_kind"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_feedback_kind),
        "command_id": source_notice["command_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                payload["client_budget_host_control_launch"]["command_id"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
                    .to_string()
            }),
        "thread_id_hint": fallback_thread_id,
        "working_state_write_status":
            payload["client_budget_host_control_launch"]["working_state_write_status"].clone(),
    })
}

fn client_budget_host_control_feedback_chat_notice(
    payload: &Value,
    thread_id: Option<&str>,
) -> Value {
    let feedback_payload = &payload["client_budget_host_control_feedback"];
    let source_notice = &feedback_payload["operator_notice"];
    let fallback_thread_id = source_notice["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(thread_id);
    let fallback_feedback_kind = feedback_payload["feedback_kind"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED);
    let fallback_message_text = append_working_state_warning_to_message(
        &working_state::host_current_thread_control_feedback_notice_text_for_command(
            fallback_feedback_kind,
            feedback_payload["command_id"].as_str(),
        ),
        &feedback_payload["working_state_write_status"],
    );
    json!({
        "kind": source_notice["kind"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!(
                "host_current_thread_control_feedback_{fallback_feedback_kind}"
            )),
        "thread_id": fallback_thread_id,
        "message_text": source_notice["message_text"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or(fallback_message_text),
        "reply_prefix": source_notice["reply_prefix"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                feedback_payload["client_budget_reply_gate"]["reply_prefix"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            }),
        "feedback_kind": source_notice["feedback_kind"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_feedback_kind),
        "command_id": source_notice["command_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                feedback_payload["command_id"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
                    .to_string()
            }),
        "working_state_write_status": feedback_payload["working_state_write_status"].clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct EnvVarRestore {
        key: &'static str,
        original: Option<String>,
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            if let Some(value) = self.original.as_deref() {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn set_env_var_for_test(key: &'static str, value: &str) -> EnvVarRestore {
        let original = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        EnvVarRestore { key, original }
    }

    #[test]
    fn client_budget_host_control_launch_api_summary_preserves_working_state_write_status() {
        let payload = json!({
            "project": { "code": "amai" },
            "namespace": { "code": "continuity" },
            "thread_id": "thread-1",
            "command_id": "thread-overlay-open-current",
            "launch_targeting": {
                "status": "ok",
                "summary": "allowed"
            },
            "working_state_write_status": {
                "status": "degraded_after_primary_write",
                "primary_write_persisted": true,
                "restore_refresh_status": "degraded",
                "warning": "test warning"
            },
            "client_budget_reply_gate": {
                "reply_prefix": "5ч KPI: переплата 0.00%"
            },
            "operator_notice": {
                "kind": "host_current_thread_control_launch_opened"
            }
        });

        let summary = client_budget_host_control_launch_api_summary(&payload);
        assert_eq!(
            summary["working_state_write_status"]["status"],
            json!("degraded_after_primary_write")
        );
        assert_eq!(
            summary["working_state_write_status"]["warning"],
            json!("test warning")
        );
    }

    #[test]
    fn client_budget_host_control_launch_chat_notice_preserves_working_state_write_status() {
        let payload = json!({
            "client_budget_host_control_launch": {
                "command_id": "thread-overlay-open-current",
                "working_state_write_status": {
                    "status": "degraded_after_primary_write",
                    "warning": "test warning"
                },
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 0.00%"
                },
                "operator_notice": {
                    "kind": "host_current_thread_control_launch_opened",
                    "reply_prefix": "5ч KPI: экономия 12.00%",
                    "feedback_kind": "opened",
                    "message_text": "opened with warning",
                    "command_id": "source-command-id",
                    "thread_id": "source-thread-id"
                }
            }
        });

        let notice = client_budget_host_control_launch_chat_notice(&payload, "thread-1");
        assert_eq!(
            notice["working_state_write_status"]["status"],
            json!("degraded_after_primary_write")
        );
        assert_eq!(
            notice["working_state_write_status"]["warning"],
            json!("test warning")
        );
        assert_eq!(
            notice["kind"],
            json!("host_current_thread_control_launch_opened")
        );
        assert_eq!(notice["feedback_kind"], json!("opened"));
        assert_eq!(notice["command_id"], json!("source-command-id"));
        assert_eq!(notice["message_text"], json!("opened with warning"));
        assert_eq!(notice["reply_prefix"], json!("5ч KPI: экономия 12.00%"));
        assert_eq!(notice["thread_id"], json!("source-thread-id"));
        assert_eq!(notice["thread_id_hint"], json!("source-thread-id"));
    }

    #[test]
    fn client_budget_host_control_launch_chat_notice_preserves_missing_write_status_as_null() {
        let payload = json!({
            "client_budget_host_control_launch": {
                "command_id": "thread-overlay-open-current",
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 0.00%"
                },
                "operator_notice": {
                    "kind": "   ",
                    "reply_prefix": "   ",
                    "feedback_kind": "   ",
                    "message_text": "opened without status",
                    "thread_id": "   "
                }
            }
        });

        let notice = client_budget_host_control_launch_chat_notice(&payload, "thread-1");
        assert!(notice["working_state_write_status"].is_null());
        assert_eq!(notice["kind"], json!("host_current_thread_control_launch_opened"));
        assert_eq!(notice["reply_prefix"], json!("5ч KPI: переплата 0.00%"));
        assert_eq!(notice["feedback_kind"], json!("opened"));
        assert_eq!(notice["thread_id"], json!("thread-1"));
        assert_eq!(notice["thread_id_hint"], json!("thread-1"));
    }

    #[test]
    fn client_budget_host_control_launch_chat_notice_falls_back_when_source_fields_blank() {
        let payload = json!({
            "client_budget_host_control_launch": {
                "command_id": "thread-overlay-open-current",
                "working_state_write_status": {
                    "status": "degraded_after_primary_write",
                    "warning": "refresh degraded"
                },
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 0.00%"
                },
                "operator_notice": {
                    "kind": "   ",
                    "reply_prefix": "   ",
                    "feedback_kind": "   ",
                    "message_text": "   ",
                    "command_id": "   "
                }
            }
        });

        let notice = client_budget_host_control_launch_chat_notice(&payload, "thread-1");
        assert_eq!(notice["kind"], json!("host_current_thread_control_launch_opened"));
        assert_eq!(notice["feedback_kind"], json!("opened"));
        assert_eq!(notice["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(
            notice["message_text"],
            json!("Запрошен same-thread host control. refresh degraded")
        );
        assert_eq!(notice["reply_prefix"], json!("5ч KPI: переплата 0.00%"));
    }

    #[test]
    fn client_budget_host_control_feedback_chat_notice_prefers_source_notice_fields() {
        let payload = json!({
            "client_budget_host_control_feedback": {
                "feedback_kind": "failed",
                "command_id": "thread-overlay-open-current",
                "working_state_write_status": {
                    "status": "degraded_after_primary_write",
                    "warning": "refresh degraded"
                },
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 0.00%"
                },
                "operator_notice": {
                    "kind": "host_current_thread_control_feedback_failed",
                    "message_text": "source notice text",
                    "reply_prefix": "5ч KPI: экономия 12.00%",
                    "feedback_kind": "failed",
                    "command_id": "source-command-id",
                    "thread_id": "source-thread-id"
                }
            }
        });

        let notice = client_budget_host_control_feedback_chat_notice(&payload, None);
        assert_eq!(
            notice["kind"],
            json!("host_current_thread_control_feedback_failed")
        );
        assert_eq!(notice["message_text"], json!("source notice text"));
        assert_eq!(notice["reply_prefix"], json!("5ч KPI: экономия 12.00%"));
        assert_eq!(notice["feedback_kind"], json!("failed"));
        assert_eq!(notice["command_id"], json!("source-command-id"));
        assert_eq!(notice["thread_id"], json!("source-thread-id"));
        assert_eq!(
            notice["working_state_write_status"]["warning"],
            json!("refresh degraded")
        );
    }

    #[test]
    fn client_budget_host_control_feedback_chat_notice_falls_back_when_source_notice_blank() {
        let payload = json!({
            "client_budget_host_control_feedback": {
                "feedback_kind": "opened",
                "command_id": "thread-overlay-open-current",
                "working_state_write_status": {
                    "status": "degraded_after_primary_write",
                    "warning": "refresh degraded"
                },
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 0.00%"
                },
                "operator_notice": {
                    "kind": "   ",
                    "message_text": "   ",
                    "reply_prefix": "   ",
                    "feedback_kind": "   ",
                    "command_id": "   ",
                    "thread_id": "   "
                }
            }
        });

        let notice =
            client_budget_host_control_feedback_chat_notice(&payload, Some("thread-1"));
        assert_eq!(
            notice["kind"],
            json!("host_current_thread_control_feedback_opened")
        );
        assert_eq!(notice["feedback_kind"], json!("opened"));
        assert_eq!(notice["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(notice["thread_id"], json!("thread-1"));
        assert_eq!(notice["reply_prefix"], json!("5ч KPI: переплата 0.00%"));
        assert_eq!(
            notice["message_text"],
            json!("Подтверждено: same-thread overlay открылся. refresh degraded")
        );
        assert_eq!(notice["thread_id"], json!("thread-1"));
    }

    #[test]
    fn compact_chat_delivery_surface_notice_kind_prefers_source_notice_kind() {
        let payload = json!({
            "operator_notice": {
                "kind": "client_budget_compact_chat_requested"
            }
        });

        assert_eq!(
            compact_chat_delivery_surface_notice_kind(&payload, "launch_failed"),
            "client_budget_compact_chat_requested"
        );
    }

    #[test]
    fn compact_chat_delivery_surface_notice_kind_falls_back_to_host_launch_status() {
        let payload = json!({
            "operator_notice": {
                "kind": "   "
            }
        });

        assert_eq!(
            compact_chat_delivery_surface_notice_kind(&payload, "launch_failed"),
            "client_budget_compact_chat_launch_failed"
        );
    }

    #[test]
    fn compact_chat_delivery_surface_notice_thread_id_prefers_source_notice_thread_id() {
        let payload = json!({
            "operator_notice": {
                "thread_id": "source-thread-id"
            }
        });

        assert_eq!(
            compact_chat_delivery_surface_notice_thread_id(&payload, Some("query-thread-id")),
            json!("source-thread-id")
        );
    }

    #[test]
    fn compact_chat_delivery_surface_notice_thread_id_falls_back_to_query_thread_id() {
        let payload = json!({
            "operator_notice": {
                "thread_id": "   "
            }
        });

        assert_eq!(
            compact_chat_delivery_surface_notice_thread_id(&payload, Some("query-thread-id")),
            json!("query-thread-id")
        );
    }

    #[test]
    fn compact_chat_delivery_surface_notice_payload_prefers_source_notice_fields() {
        let payload = json!({
            "operator_notice": {
                "kind": "client_budget_compact_chat_requested",
                "thread_id": "source-thread-id",
                "message_text": "source message",
                "reply_prefix": "5ч KPI: экономия 12.00%",
                "exact_chat_command": "/source",
                "prompt_file": "/tmp/prompt.md",
                "launch_clean_chat_command": "code chat --mode agent",
                "launch_clean_chat_fallback_command": "code chat --reuse-window",
                "launch_clean_chat_command_kind": "vscode_code_chat_cli",
                "clean_chat_launch": {
                    "status": "launch_command_available"
                },
                "manual_fallback_steps": ["step 1", "step 2"],
                "required_host_action": {
                    "status": "opened"
                },
                "note": "source note"
            },
            "chat_start_restore": {
                "prompt_text": "restore prompt"
            },
            "client_surface": {
                "surface": "compact"
            },
            "host_launch": {
                "status": "launch_failed"
            }
        });

        let notice = compact_chat_delivery_surface_notice_payload(&payload, Some("query-thread-id"));

        assert_eq!(notice["kind"], json!("client_budget_compact_chat_requested"));
        assert_eq!(notice["thread_id"], json!("source-thread-id"));
        assert_eq!(notice["message_text"], json!("source message"));
        assert_eq!(notice["reply_prefix"], json!("5ч KPI: экономия 12.00%"));
        assert_eq!(notice["exact_chat_command"], json!("/source"));
        assert_eq!(notice["prompt_text"], json!("restore prompt"));
        assert_eq!(notice["prompt_file"], json!("/tmp/prompt.md"));
        assert_eq!(notice["launch_clean_chat_command"], json!("code chat --mode agent"));
        assert_eq!(
            notice["launch_clean_chat_fallback_command"],
            json!("code chat --reuse-window")
        );
        assert_eq!(
            notice["launch_clean_chat_command_kind"],
            json!("vscode_code_chat_cli")
        );
        assert_eq!(
            notice["clean_chat_launch"]["status"],
            json!("launch_command_available")
        );
        assert_eq!(notice["manual_fallback_steps"], json!(["step 1", "step 2"]));
        assert_eq!(notice["client_surface"]["surface"], json!("compact"));
        assert_eq!(notice["required_host_action"]["status"], json!("opened"));
        assert_eq!(notice["note"], json!("source note"));
    }

    #[test]
    fn compact_chat_delivery_surface_notice_payload_falls_back_when_source_notice_fields_blank() {
        let payload = json!({
            "operator_notice": {
                "kind": "   ",
                "thread_id": Value::Null,
                "message_text": Value::Null,
                "reply_prefix": Value::Null,
                "exact_chat_command": Value::Null,
                "prompt_file": Value::Null,
                "required_host_action": Value::Null,
                "note": Value::Null
            },
            "chat_start_restore": {
                "prompt_text": "restore prompt"
            },
            "client_surface": {
                "surface": "compact"
            },
            "host_launch": {
                "status": "launch_failed"
            }
        });

        let notice = compact_chat_delivery_surface_notice_payload(&payload, Some("query-thread-id"));

        assert_eq!(notice["kind"], json!("client_budget_compact_chat_launch_failed"));
        assert_eq!(notice["thread_id"], json!("query-thread-id"));
        assert!(notice["message_text"].is_null());
        assert!(notice["reply_prefix"].is_null());
        assert!(notice["exact_chat_command"].is_null());
        assert_eq!(notice["prompt_text"], json!("restore prompt"));
        assert!(notice["prompt_file"].is_null());
        assert!(notice["launch_clean_chat_command"].is_null());
        assert!(notice["launch_clean_chat_fallback_command"].is_null());
        assert!(notice["launch_clean_chat_command_kind"].is_null());
        assert!(notice["clean_chat_launch"].is_null());
        assert!(notice["manual_fallback_steps"].is_null());
        assert_eq!(notice["client_surface"]["surface"], json!("compact"));
        assert!(notice["required_host_action"].is_null());
        assert!(notice["note"].is_null());
    }

    #[test]
    fn compact_chat_delivery_surface_notice_payload_handles_missing_operator_notice() {
        let payload = json!({
            "chat_start_restore": {
                "prompt_text": "restore prompt"
            },
            "client_surface": {
                "surface": "compact"
            },
            "host_launch": {
                "status": "launch_failed"
            }
        });

        let notice = compact_chat_delivery_surface_notice_payload(&payload, Some("query-thread-id"));

        assert_eq!(notice["kind"], json!("client_budget_compact_chat_launch_failed"));
        assert_eq!(notice["thread_id"], json!("query-thread-id"));
        assert!(notice["message_text"].is_null());
        assert!(notice["reply_prefix"].is_null());
        assert!(notice["exact_chat_command"].is_null());
        assert_eq!(notice["prompt_text"], json!("restore prompt"));
        assert!(notice["prompt_file"].is_null());
        assert_eq!(notice["client_surface"]["surface"], json!("compact"));
        assert!(notice["required_host_action"].is_null());
        assert!(notice["note"].is_null());
    }

    #[test]
    fn compact_chat_response_payload_keeps_summary_and_notice_launch_contract_aligned() {
        let compact_chat_payload = json!({
            "project": { "code": "amai" },
            "namespace": { "code": "continuity" },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT"
            },
            "operator_notice": {
                "kind": "client_budget_compact_chat_launch_requested",
                "message_text": "message",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "thread_id": "source-thread-id",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "note",
                "launch_clean_chat_command": "code chat --mode agent --new-window",
                "launch_clean_chat_fallback_command": "code chat --mode agent --reuse-window",
                "launch_clean_chat_command_kind": "vscode_code_chat_cli",
                "clean_chat_launch": {
                    "status": "requested"
                },
                "manual_fallback_steps": ["fallback step"]
            },
            "client_surface": {
                "client_key": "vscode",
                "display_name": "VS Code"
            },
            "host_launch": {
                "status": "requested"
            }
        });

        let response = compact_chat_response_payload(
            &compact_chat_payload,
            &json!({ "rows": [] }),
            Some("query-thread-id"),
        );

        assert_eq!(response["status"], json!("ok"));
        assert_eq!(
            response["continuity_compact_chat"]["operator_notice"]["launch_clean_chat_command"],
            json!("code chat --mode agent --new-window")
        );
        assert_eq!(
            response["continuity_compact_chat"]["operator_notice"]["launch_clean_chat_fallback_command"],
            json!("code chat --mode agent --reuse-window")
        );
        assert_eq!(
            response["delivery_surface_notice"]["launch_clean_chat_command"],
            json!("code chat --mode agent --new-window")
        );
        assert_eq!(
            response["delivery_surface_notice"]["launch_clean_chat_fallback_command"],
            json!("code chat --mode agent --reuse-window")
        );
        assert_eq!(
            response["chat_notice"]["launch_clean_chat_command_kind"],
            json!("vscode_code_chat_cli")
        );
        assert_eq!(response["chat_notice"]["thread_id"], json!("source-thread-id"));
        assert_eq!(response["chat_notice"]["prompt_text"], json!("PROMPT"));
        assert_eq!(
            response["chat_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert_eq!(
            response["chat_notice"]["clean_chat_launch"]["status"],
            json!("requested")
        );
        assert_eq!(
            response["chat_notice"]["manual_fallback_steps"][0],
            json!("fallback step")
        );
    }

    #[test]
    fn compact_chat_response_payload_marks_launch_failed_in_final_notice_surface() {
        let compact_chat_payload = json!({
            "project": { "code": "amai" },
            "namespace": { "code": "continuity" },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT"
            },
            "operator_notice": {
                "message_text": "manual fallback required",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "launch failed"
            },
            "client_surface": {
                "client_key": "vscode",
                "display_name": "VS Code"
            },
            "host_launch": {
                "status": "launch_failed",
                "reason": "code chat launch failed"
            }
        });

        let response = compact_chat_response_payload(
            &compact_chat_payload,
            &json!({ "rows": [] }),
            Some("query-thread-id"),
        );

        assert_eq!(
            response["delivery_surface_notice"]["kind"],
            json!("client_budget_compact_chat_launch_failed")
        );
        assert_eq!(
            response["chat_notice"]["kind"],
            json!("client_budget_compact_chat_launch_failed")
        );
        assert_eq!(
            response["continuity_compact_chat"]["host_launch"]["status"],
            json!("launch_failed")
        );
        assert_eq!(
            response["chat_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
    }

    #[test]
    fn compact_chat_response_payload_marks_available_not_requested_in_final_notice_surface() {
        let compact_chat_payload = json!({
            "project": { "code": "amai" },
            "namespace": { "code": "continuity" },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT"
            },
            "operator_notice": {
                "message_text": "launch not requested",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "user did not request launch"
            },
            "client_surface": {
                "client_key": "vscode",
                "display_name": "VS Code"
            },
            "host_launch": {
                "status": "available_not_requested",
                "reason": "launch_not_requested"
            }
        });

        let response = compact_chat_response_payload(
            &compact_chat_payload,
            &json!({ "rows": [] }),
            Some("query-thread-id"),
        );

        assert_eq!(
            response["delivery_surface_notice"]["kind"],
            json!("client_budget_compact_chat_launch_not_requested")
        );
        assert_eq!(
            response["chat_notice"]["kind"],
            json!("client_budget_compact_chat_launch_not_requested")
        );
        assert_eq!(
            response["continuity_compact_chat"]["host_launch"]["status"],
            json!("available_not_requested")
        );
        assert_eq!(
            response["chat_notice"]["note"],
            json!("user did not request launch")
        );
    }

    #[test]
    fn compact_chat_response_payload_marks_bridge_unavailable_in_final_notice_surface() {
        let compact_chat_payload = json!({
            "project": { "code": "amai" },
            "namespace": { "code": "continuity" },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT"
            },
            "operator_notice": {
                "message_text": "launch bridge unavailable",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "bridge missing"
            },
            "client_surface": {
                "client_key": "vscode",
                "display_name": "VS Code"
            },
            "host_launch": {
                "status": "bridge_unavailable",
                "reason": "vscode_code_cli_unavailable"
            }
        });

        let response = compact_chat_response_payload(
            &compact_chat_payload,
            &json!({ "rows": [] }),
            Some("query-thread-id"),
        );

        assert_eq!(
            response["delivery_surface_notice"]["kind"],
            json!("client_budget_compact_chat_bridge_unavailable")
        );
        assert_eq!(
            response["chat_notice"]["kind"],
            json!("client_budget_compact_chat_bridge_unavailable")
        );
        assert_eq!(
            response["continuity_compact_chat"]["host_launch"]["reason"],
            json!("vscode_code_cli_unavailable")
        );
        assert_eq!(
            response["chat_notice"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
    }

    #[test]
    fn compact_chat_response_payload_marks_disabled_by_policy_in_final_notice_surface() {
        let compact_chat_payload = json!({
            "project": { "code": "amai" },
            "namespace": { "code": "continuity" },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT"
            },
            "operator_notice": {
                "message_text": "policy blocked auto launch",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "policy disabled"
            },
            "client_surface": {
                "client_key": "vscode",
                "display_name": "VS Code"
            },
            "host_launch": {
                "status": "disabled_by_policy",
                "reason": "auto_launch_disabled"
            }
        });

        let response = compact_chat_response_payload(
            &compact_chat_payload,
            &json!({ "rows": [] }),
            Some("query-thread-id"),
        );

        assert_eq!(
            response["delivery_surface_notice"]["kind"],
            json!("client_budget_compact_chat_launch_disabled_by_policy")
        );
        assert_eq!(
            response["chat_notice"]["kind"],
            json!("client_budget_compact_chat_launch_disabled_by_policy")
        );
        assert_eq!(
            response["continuity_compact_chat"]["host_launch"]["reason"],
            json!("auto_launch_disabled")
        );
        assert_eq!(
            response["chat_notice"]["note"],
            json!("policy disabled")
        );
    }

    #[test]
    fn execute_host_current_thread_control_launch_rejects_unavailable_surface() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let error = rt
            .block_on(async {
                execute_host_current_thread_control_launch(&json!({
                    "external_uri_launch": {
                        "available": false,
                        "uri": "vscode://openai.chatgpt/thread-overlay/thread-current"
                    }
                }))
                .await
            })
            .expect_err("unavailable surface must fail");
        assert!(
            format!("{error:#}").contains("external launch surface is unavailable"),
            "{error:#}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn execute_host_current_thread_control_launch_reports_xdg_open_success() {
        use std::os::unix::fs::PermissionsExt;

        let unique = format!(
            "amai-host-control-xdg-open-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let base = std::env::temp_dir().join(unique);
        let fakebin = base.join("bin");
        fs::create_dir_all(&fakebin).expect("fakebin");
        let marker_path = base.join("uri.txt");
        let script_path = fakebin.join("xdg-open");
        let marker_display = marker_path.display().to_string().replace('\'', "'\\''");
        fs::write(
            &script_path,
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s' \"$1\" > {}\n",
                format!("'{}'", marker_display)
            ),
        )
        .expect("write fake xdg-open");
        let mut perms = fs::metadata(&script_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");
        let path_prefix = format!("{}:/usr/bin:/bin", fakebin.display());
        let _path_restore = set_env_var_for_test("PATH", &path_prefix);
        let uri = "vscode://openai.chatgpt/thread-overlay/thread-current";

        let launch = execute_host_current_thread_control_launch(&json!({
            "external_uri_launch": {
                "available": true,
                "uri": uri
            }
        }))
        .await
        .expect("xdg-open launch should succeed");

        assert_eq!(
            fs::read_to_string(&marker_path).expect("marker"),
            uri
        );
        assert_eq!(launch["launched"], json!(true));
        assert_eq!(launch["launch_method"], json!("xdg_open"));
        assert_eq!(launch["uri"], json!(uri));
        assert_eq!(
            launch["verification_state"],
            json!("launch_command_executed_exit_zero")
        );
        fs::remove_dir_all(&base).expect("cleanup");
    }
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
        let working_state_write_status =
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
            let working_state_write_status_value =
                serde_json::to_value(&working_state_write_status)?;
            let message_text =
                working_state::host_current_thread_control_feedback_notice_text_for_command(
                    feedback_kind,
                    Some(command_id),
                );
            let message_text = append_working_state_warning_to_message(
                &message_text,
                &working_state_write_status_value,
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
                    "working_state_write_status": working_state_write_status_value,
                    "client_budget_reply_gate": client_budget_reply_gate,
                }
            }));
        }
        let restore = working_state::build_restore_bundle(&db, &project, &namespace).await?;
        let client_budget_guard =
            token_budget::collect_live_current_session_budget_guard(&db, restore.as_ref()).await?;
        let client_budget_reply_gate =
            compact_host_control_client_budget_reply_gate(&client_budget_guard);
        let working_state_write_status_value = serde_json::to_value(&working_state_write_status)?;
        let message_text =
            working_state::host_current_thread_control_feedback_notice_text_for_command(
                feedback_kind,
                Some(command_id),
            );
        let message_text = append_working_state_warning_to_message(
            &message_text,
            &working_state_write_status_value,
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
                "working_state_write_status": working_state_write_status_value.clone(),
                "client_budget_reply_gate": client_budget_reply_gate.clone(),
                "operator_notice": {
                    "kind": format!("host_current_thread_control_feedback_{feedback_kind}"),
                    "message_text": message_text,
                    "reply_prefix": client_budget_reply_gate["reply_prefix"].clone(),
                    "feedback_kind": feedback_kind,
                    "command_id": command_id,
                    "thread_id": query.thread_id.clone(),
                    "working_state_write_status": working_state_write_status_value.clone(),
                }
            },
            "chat_notice": client_budget_host_control_feedback_chat_notice(
                &json!({
                    "client_budget_host_control_feedback": {
                        "feedback_kind": feedback_kind,
                        "command_id": command_id,
                        "working_state_write_status": working_state_write_status_value,
                        "client_budget_reply_gate": client_budget_reply_gate,
                        "operator_notice": {
                            "kind": format!("host_current_thread_control_feedback_{feedback_kind}"),
                            "message_text": message_text,
                            "reply_prefix": client_budget_reply_gate["reply_prefix"].clone(),
                            "feedback_kind": feedback_kind,
                            "command_id": command_id,
                            "thread_id": query.thread_id.clone(),
                        }
                    }
                }),
                query.thread_id.as_deref(),
            )
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
