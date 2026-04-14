use super::{
    AgentDisplayNameUpdateRequest, ClientBudgetTargetUpdateRequest, ObserveState,
    ThreadBindingQuery, cached_snapshot_with_meta, collect_compact_client_budget_surfaces,
    compact_budget_snapshot_preview_payload,
    compact_cli_client_budget_gate_from_root_cause_payload,
    compact_client_budget_snapshot_for_request, compact_client_budget_surfaces_from_snapshot,
    current_epoch_ms_u64, dashboard_live_summary_payload_for_request,
    live_active_agent_snapshot_for_request, maybe_auto_launch_same_thread_host_control_from_gate,
    maybe_refresh_stale_observe_cache_for_healthz, merged_thread_bound_snapshot_with_meta,
    normalize_front_door_client_budget_gate_payload_shape, now_epoch_ms,
    refresh_client_live_meter_on_request, refresh_observe_cache, resolved_request_thread_hint,
    thread_bound_snapshot_with_meta,
    try_load_fast_thread_bound_materialized_compact_client_budget_surfaces,
};
use crate::cli::ContinuityClientBudgetTargetArgs;
use crate::config::discover_repo_root;
use crate::{continuity, dashboard, postgres, token_budget};
use anyhow::{Result, anyhow};
use axum::{
    extract::{Json, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse},
};
use serde_json::{Value, json};
use std::path::PathBuf;

pub(super) async fn dashboard_page_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    super::spawn_client_live_meter_refresh(&state).await;
    let html = dashboard::render_html(state.dashboard_refresh_ms, None);
    (no_store_headers("text/html; charset=utf-8"), Html(html)).into_response()
}

pub(super) async fn grafana_password_help_handler() -> impl IntoResponse {
    let repo_root =
        discover_repo_root(None).unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let env_path = repo_root.join(".env");
    let html = format!(
        r#"<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Где менять пароль Grafana</title>
  <style>
    body {{
      margin: 0;
      padding: 32px 24px 48px;
      background: #0f171b;
      color: #edf3f5;
      font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
      line-height: 1.55;
    }}
    main {{
      max-width: 860px;
      margin: 0 auto;
      background: rgba(255, 255, 255, 0.04);
      border-radius: 18px;
      padding: 28px 28px 32px;
      box-shadow: 0 18px 42px rgba(0, 0, 0, 0.24);
    }}
    h1 {{ margin: 0 0 18px; font-size: 30px; }}
    p {{ margin: 0 0 14px; }}
    code {{
      background: rgba(255, 255, 255, 0.08);
      padding: 2px 6px;
      border-radius: 6px;
      font-family: "IBM Plex Mono", "SFMono-Regular", monospace;
      font-size: 0.95em;
    }}
    ol {{ margin: 0; padding-left: 22px; }}
    li {{ margin: 0 0 10px; }}
    a {{ color: #8de4da; }}
  </style>
</head>
<body>
  <main>
    <h1>Где менять пароль Grafana</h1>
    <p>Пароль Grafana задаётся не в самой карточке dashboard, а в локальном файле окружения проекта.</p>
    <ol>
      <li>Откройте файл <code>{}</code>.</li>
      <li>Найдите строку <code>AMI_GRAFANA_ADMIN_PASSWORD=...</code>.</li>
      <li>Поставьте новый пароль.</li>
      <li>Примените изменение: <code>./scripts/monitoring_up.sh</code>.</li>
    </ol>
    <p>Дополнительный контур: <code>AMI_GRAFANA_ADMIN_USER</code> задаёт логин администратора.</p>
    <p><a href="/dashboard">Вернуться в Amai dashboard</a></p>
  </main>
</body>
</html>"#,
        env_path.display()
    );
    Html(html).into_response()
}

pub(super) async fn brand_mark_handler() -> impl IntoResponse {
    let headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    )];
    (StatusCode::OK, headers, dashboard::brand_mark_svg()).into_response()
}

pub(super) async fn brand_lockup_handler() -> impl IntoResponse {
    let headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    )];
    (StatusCode::OK, headers, dashboard::brand_lockup_svg()).into_response()
}

pub(super) async fn favicon_handler() -> impl IntoResponse {
    let headers = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/x-icon"),
    )];
    (StatusCode::OK, headers, dashboard::favicon_ico()).into_response()
}

pub(super) async fn client_budget_snapshot_preview_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    let resolved_thread_id = resolved_request_thread_hint(&state, query.thread_id.as_deref()).await;
    let response =
        compact_client_budget_snapshot_for_request(&state, resolved_thread_id.as_deref()).await;
    match response {
        Ok(snapshot) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&compact_budget_snapshot_preview_payload(&snapshot))
                .unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

pub(super) async fn client_budget_root_cause_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    let resolved_thread_id = resolved_request_thread_hint(&state, query.thread_id.as_deref()).await;
    let response: Result<Value> = async {
        if let Some(thread_id) = resolved_thread_id.as_deref() {
            let repo_root = discover_repo_root(None)?;
            if let Some(materialized) =
                try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                    &repo_root, thread_id,
                )
            {
                return Ok(materialized.surfaces.root_cause_payload);
            }
            refresh_client_live_meter_on_request(&state).await;
            let snapshot = thread_bound_snapshot_with_meta(&state, &thread_id).await?;
            return Ok(compact_client_budget_surfaces_from_snapshot(
                &repo_root,
                &snapshot,
                Some(thread_id),
            )
            .surfaces
            .root_cause_payload);
        }
        refresh_client_live_meter_on_request(&state).await;
        Ok(collect_compact_client_budget_surfaces(&state.cfg)
            .await?
            .root_cause_payload)
    }
    .await;
    match response {
        Ok(compact) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&compact).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

pub(super) async fn client_budget_gate_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    let response: Result<Value> = async {
        if let Some(thread_id) = super::normalized_thread_id_hint(query.thread_id.as_deref()) {
            let repo_root = discover_repo_root(None)?;
            let now_epoch_ms = current_epoch_ms_u64();
            if let Some(cached) = super::load_shared_compact_client_budget_gate(
                &repo_root,
                now_epoch_ms,
                Some(thread_id),
            ) {
                if let Some(launched_gate) = maybe_auto_launch_same_thread_host_control_from_gate(
                    &state.cfg,
                    &repo_root,
                    thread_id,
                    &cached.gate,
                )
                .await?
                {
                    return Ok(launched_gate);
                }
                return Ok(cached.gate);
            }
            if let Some(materialized) =
                try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                    &repo_root, thread_id,
                )
            {
                if let Some(launched_gate) = maybe_auto_launch_same_thread_host_control_from_gate(
                    &state.cfg,
                    &repo_root,
                    thread_id,
                    &materialized.gate.gate_payload,
                )
                .await?
                {
                    return Ok(launched_gate);
                }
                return Ok(materialized.gate.gate_payload);
            }
            refresh_client_live_meter_on_request(&state).await;
            let snapshot = thread_bound_snapshot_with_meta(&state, &thread_id).await?;
            let materialized = compact_client_budget_surfaces_from_snapshot(
                &repo_root,
                &snapshot,
                Some(thread_id),
            );
            if let Some(launched_gate) = maybe_auto_launch_same_thread_host_control_from_gate(
                &state.cfg,
                &repo_root,
                thread_id,
                &materialized.gate.gate_payload,
            )
            .await?
            {
                return Ok(launched_gate);
            }
            return Ok(materialized.gate.gate_payload);
        }
        refresh_client_live_meter_on_request(&state).await;
        let surfaces = collect_compact_client_budget_surfaces(&state.cfg).await?;
        compact_cli_client_budget_gate_from_root_cause_payload(&surfaces.root_cause_payload)
            .ok_or_else(|| anyhow!("compact client-budget root-cause payload missing gate"))
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string(&normalize_front_door_client_budget_gate_payload_shape(
                payload,
            ))
            .unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

pub(super) async fn client_limit_hourly_burn_api_handler(
    State(state): State<ObserveState>,
) -> impl IntoResponse {
    let response: Result<Value> = async {
        let db = postgres::connect_admin(&state.cfg).await?;
        postgres::bootstrap_schema(&db, &state.cfg).await?;
        token_budget::collect_default_client_limit_hourly_burn_surface(&db).await
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
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

fn normalize_agent_display_name_input(raw: &str) -> Result<String> {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(anyhow!("agent display_name must not be empty"));
    }
    if normalized.chars().count() > 120 {
        return Err(anyhow!("agent display_name must be at most 120 characters"));
    }
    Ok(normalized)
}

pub(super) async fn agent_display_name_update_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<AgentDisplayNameUpdateRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let response: Result<Value> = async {
        let agent_scope = request.agent_scope.trim();
        if agent_scope.is_empty() {
            return Err(anyhow!("agent_scope is required"));
        }
        let display_name = normalize_agent_display_name_input(&request.display_name)?;
        let db = postgres::connect_admin(&state.cfg).await?;
        postgres::bootstrap_schema(&db, &state.cfg).await?;
        postgres::upsert_agent_display_name_by_code(&db, agent_scope, &display_name).await?;
        refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await?;
        let live_summary =
            dashboard_live_summary_payload_for_request(&state, query.thread_id.as_deref()).await?;
        Ok(json!({
            "status": "ok",
            "agent_display_name_update": {
                "agent_scope": agent_scope,
                "display_name": display_name,
            },
            "dashboard_live_summary": live_summary,
            "chat_notice": {
                "kind": "agent_display_name_updated",
                "thread_id": query.thread_id.clone(),
                "message_text": format!("Имя агента сохранено: {display_name}."),
                "agent_scope": agent_scope,
                "display_name": display_name,
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

pub(super) async fn client_budget_target_update_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
    Json(request): Json<ClientBudgetTargetUpdateRequest>,
) -> impl IntoResponse {
    refresh_client_live_meter_on_request(&state).await;
    let repo_root =
        match super::resolve_request_repo_root_for_project(&state.cfg, request.project.as_deref())
            .await
        {
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
        let args = ContinuityClientBudgetTargetArgs {
            project: request.project.clone(),
            repo_root: Some(repo_root),
            namespace: request.namespace.clone(),
            percent: request.percent,
            json: true,
        };
        let update = continuity::client_budget_target_payload(
            &state.cfg,
            &args,
            query.thread_id.as_deref(),
        )
        .await?;
        refresh_observe_cache(
            state.cache.clone(),
            state.cfg.clone(),
            state.bind.clone(),
            state.dashboard_refresh_ms,
        )
        .await?;
        let snapshot = if let Some(thread_id_hint) =
            resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
        {
            thread_bound_snapshot_with_meta(&state, &thread_id_hint).await?
        } else {
            cached_snapshot_with_meta(&state).await?
        };
        Ok(json!({
            "status": "ok",
            "client_budget_target_update": update["client_budget_target_update"].clone(),
            "client_budget_live": dashboard::client_budget_live_payload(&snapshot),
            "chat_notice": {
                "kind": "client_budget_target_changed",
                "thread_id": query.thread_id.clone(),
                "message_text": update["client_budget_target_update"]["operator_notice"]["message_text"].clone(),
                "reply_prefix": update["client_budget_target_update"]["client_budget_guard"]["reply_prefix"].clone(),
                "exact_chat_command": update["client_budget_target_update"]["operator_notice"]["exact_chat_command"].clone(),
                "target_percent": update["client_budget_target_update"]["target_percent"].clone(),
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

pub(super) async fn snapshot_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    super::spawn_client_live_meter_refresh(&state).await;
    let response = if let Some(thread_id_hint) =
        resolved_request_thread_hint(&state, query.thread_id.as_deref()).await
    {
        merged_thread_bound_snapshot_with_meta(&state, &thread_id_hint).await
    } else {
        live_active_agent_snapshot_for_request(&state).await
    };
    match response {
        Ok(snapshot) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&snapshot).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

pub(super) async fn healthz_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    if let Err(error) = maybe_refresh_stale_observe_cache_for_healthz(&state).await {
        eprintln!("observe healthz refresh recovery failed: {error:#}");
    }
    let snapshot = cached_snapshot_with_meta(&state).await;
    match snapshot {
        Ok(snapshot) => {
            let summary = &snapshot["sla"]["summary"];
            let critical = summary["critical"].as_u64().unwrap_or(0);
            let unknown = summary["unknown"].as_u64().unwrap_or(0);
            let cache_stale = snapshot["observe_cache"]["stale"].as_bool().unwrap_or(true);
            let status = if critical == 0 && unknown == 0 && !cache_stale {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            let status_label = if status == StatusCode::OK {
                "up"
            } else {
                "down"
            };
            let headers = no_store_headers("application/json; charset=utf-8");
            let payload = json!({
                "status": status_label,
                "critical": critical,
                "unknown": unknown,
                "cache_stale": cache_stale,
                "refresh_in_progress": snapshot["observe_cache"]["refresh_in_progress"].clone(),
                "snapshot_age_ms": snapshot["observe_cache"]["snapshot_age_ms"].clone(),
                "last_refresh_completed_epoch_ms": snapshot["observe_cache"]["last_refresh_completed_epoch_ms"].clone(),
                "last_error": snapshot["observe_cache"]["last_error"].clone(),
            });
            (
                status,
                headers,
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
                .into_response()
        }
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{{\"status\":\"down\",\"error\":\"{error:#}\"}}"),
        )
            .into_response(),
    }
}

pub(super) fn no_store_headers(
    content_type: &'static str,
) -> [(header::HeaderName, HeaderValue); 4] {
    [
        (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
        ),
        (header::PRAGMA, HeaderValue::from_static("no-cache")),
        (header::EXPIRES, HeaderValue::from_static("0")),
    ]
}

pub(super) async fn mark_observe_http_activity(state: &ObserveState) {
    let mut cache = state.cache.write().await;
    cache.last_http_request_epoch_ms = Some(now_epoch_ms());
}
