use super::*;

pub(super) async fn dashboard_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    spawn_client_live_meter_refresh(&state).await;
    let response: Result<Value> = async {
        let payload =
            if let Some(thread_id_hint) = normalized_thread_id_hint(query.thread_id.as_deref()) {
                thread_bound_dashboard_payload(&state, thread_id_hint).await?
            } else {
                cached_dashboard_payload(&state).await?
            };
        Ok(payload)
    }
    .await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => dashboard_api_error_response(&state, &error).await,
    }
}

pub(super) async fn dashboard_live_summary_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    let response =
        dashboard_live_summary_payload_for_request(&state, query.thread_id.as_deref()).await;
    match response {
        Ok(payload) => (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
            .into_response(),
        Err(error) => dashboard_live_summary_error_response(&state, &error).await,
    }
}

pub(super) async fn client_budget_live_api_handler(
    State(state): State<ObserveState>,
    Query(query): Query<ThreadBindingQuery>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    let resolved_thread_id = resolved_request_thread_hint(&state, query.thread_id.as_deref()).await;
    {
        let cache = state.cache.read().await;
        let same_thread =
            cache.client_budget_live_thread_id.as_deref() == resolved_thread_id.as_deref();
        let cache_fresh = cache
            .client_budget_live_completed_epoch_ms
            .is_some_and(|completed_at| {
                now_epoch_ms().saturating_sub(completed_at)
                    <= CLIENT_BUDGET_LIVE_PAYLOAD_CACHE_TTL_MS
            });
        if same_thread
            && cache_fresh
            && let Some(payload) = cache.client_budget_live_payload.clone()
        {
            return (
                StatusCode::OK,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
                .into_response();
        }
    }
    if let Some(thread_id) = resolved_thread_id.as_deref() {
        if let Ok(repo_root) = discover_repo_root(None) {
            if let Some(snapshot) = load_shared_budget_snapshot_preview(&repo_root, Some(thread_id))
            {
                let payload = dashboard::client_budget_live_payload(&snapshot);
                let mut cache = state.cache.write().await;
                cache.client_budget_live_payload = Some(payload.clone());
                cache.client_budget_live_thread_id = Some(thread_id.to_string());
                cache.client_budget_live_completed_epoch_ms = Some(now_epoch_ms());
                return (
                    StatusCode::OK,
                    no_store_headers("application/json; charset=utf-8"),
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                )
                    .into_response();
            }
        }
    }
    spawn_client_live_meter_refresh(&state).await;
    let response =
        compact_client_budget_snapshot_for_request(&state, resolved_thread_id.as_deref()).await;
    match response {
        Ok(snapshot) => {
            let payload = dashboard::client_budget_live_payload(&snapshot);
            let mut cache = state.cache.write().await;
            cache.client_budget_live_payload = Some(payload.clone());
            cache.client_budget_live_thread_id = resolved_thread_id.clone();
            cache.client_budget_live_completed_epoch_ms = Some(now_epoch_ms());
            (
                StatusCode::OK,
                no_store_headers("application/json; charset=utf-8"),
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )
                .into_response()
        }
        Err(error) => client_budget_live_error_response(&state, &error).await,
    }
}

pub(super) async fn client_budget_live_error_response(
    state: &ObserveState,
    error: &anyhow::Error,
) -> Response {
    let (refresh_in_progress, snapshot_age_ms) = {
        let cache = state.cache.read().await;
        (cache.refresh_in_progress, cache_snapshot_age_ms(&cache))
    };
    if refresh_in_progress {
        return (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&json!({
                "status": "warming_up",
                "rows": [],
                "reply_prefix": Value::Null,
                "global_reply_prefix": Value::Null,
                "reply_prefix_source": "warmup_pending",
                "thread_binding_state": "warmup_pending",
                "current_thread_bound": false,
                "ended_at_epoch_ms": Value::Null,
                "warmup_pending": true,
                "snapshot_age_ms": snapshot_age_ms,
            }))
            .unwrap_or_default(),
        )
            .into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        no_store_headers("application/json; charset=utf-8"),
        serde_json::to_string_pretty(&json!({
            "status": "down",
            "error": format!("{error:#}"),
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

pub(super) async fn dashboard_api_error_response(
    state: &ObserveState,
    error: &anyhow::Error,
) -> Response {
    let (refresh_in_progress, snapshot_age_ms) = {
        let cache = state.cache.read().await;
        (cache.refresh_in_progress, cache_snapshot_age_ms(&cache))
    };
    if refresh_in_progress {
        return (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&dashboard_warmup_payload(snapshot_age_ms))
                .unwrap_or_default(),
        )
            .into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        no_store_headers("application/json; charset=utf-8"),
        serde_json::to_string_pretty(&json!({
            "status": "down",
            "error": format!("{error:#}"),
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

pub(super) async fn dashboard_live_summary_error_response(
    state: &ObserveState,
    error: &anyhow::Error,
) -> Response {
    let (refresh_in_progress, snapshot_age_ms) = {
        let cache = state.cache.read().await;
        (cache.refresh_in_progress, cache_snapshot_age_ms(&cache))
    };
    if refresh_in_progress {
        return (
            StatusCode::OK,
            no_store_headers("application/json; charset=utf-8"),
            serde_json::to_string_pretty(&dashboard_live_summary_warmup_payload(snapshot_age_ms))
                .unwrap_or_default(),
        )
            .into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        no_store_headers("application/json; charset=utf-8"),
        serde_json::to_string_pretty(&json!({
            "status": "down",
            "error": format!("{error:#}"),
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

pub(super) fn dashboard_warmup_payload(snapshot_age_ms: Option<u64>) -> Value {
    json!({
        "meta": {
            "package_version": env!("CARGO_PKG_VERSION"),
            "cache_stale": true,
            "cache_snapshot_age_ms": snapshot_age_ms,
            "observe_refresh_total_ms": Value::Null,
            "observe_refresh_slowest_stage": Value::Null,
            "observe_refresh_slowest_stage_ms": Value::Null,
            "cache_refresh_completed_at_label": Value::Null,
            "cache_refresh_duration_ms": Value::Null,
        },
        "headline": {
            "status": "waiting",
            "status_label": "идёт прогрев",
            "status_tooltip": "Observe cache ещё materialize-ится. Панель вернёт полный live snapshot после завершения первого refresh.",
            "status_reason": "Первый observe refresh ещё не завершён.",
            "token_value": "ещё нет данных",
            "token_scope": Value::Null,
        },
        "links": [],
        "hero_cards": [],
        "top_cards": [],
        "benchmark_cards": [],
        "service_cards": [],
        "machine_cards": [],
        "warnings": [
            "Панель ещё прогревается: первый live snapshot не готов."
        ],
        "glossary": []
    })
}

pub(super) fn dashboard_live_summary_warmup_payload(snapshot_age_ms: Option<u64>) -> Value {
    json!({
        "meta": {
            "package_version": env!("CARGO_PKG_VERSION"),
            "cache_stale": true,
            "cache_snapshot_age_ms": snapshot_age_ms,
            "observe_refresh_total_ms": Value::Null,
            "observe_refresh_slowest_stage": Value::Null,
            "observe_refresh_slowest_stage_ms": Value::Null,
            "cache_refresh_completed_at_label": Value::Null,
            "cache_refresh_duration_ms": Value::Null,
        },
        "headline": {
            "status": "waiting",
            "status_label": "идёт прогрев",
            "status_tooltip": "Live summary ещё не готов: observe cache materialize-ится.",
            "status_reason": "Первый live summary snapshot ещё не готов.",
            "token_value": "ещё нет данных",
            "token_scope": Value::Null,
        },
        "active_agent_card": Value::Null,
        "top_cards": [],
        "warmup_pending": true
    })
}

pub(super) async fn active_agent_budget_live_api_handler(
    State(state): State<ObserveState>,
) -> impl IntoResponse {
    mark_observe_http_activity(&state).await;
    spawn_client_live_meter_refresh(&state).await;
    let response = live_active_agent_budget_card_payload(&state).await;
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
