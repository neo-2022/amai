use super::*;

pub(super) async fn metrics_handler(State(state): State<ObserveState>) -> impl IntoResponse {
    let snapshot = async {
        match cached_snapshot_with_meta(&state).await {
            Ok(snapshot) => Ok(snapshot),
            Err(_) => {
                refresh_observe_cache(
                    state.cache.clone(),
                    state.cfg.clone(),
                    state.bind.clone(),
                    state.dashboard_refresh_ms,
                )
                .await?;
                cached_snapshot_with_meta(&state).await
            }
        }
    }
    .await;
    match snapshot {
        Ok(snapshot) => {
            let body = render_prometheus_metrics(&snapshot);
            let headers = [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
            )];
            (StatusCode::OK, headers, body).into_response()
        }
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("observe exporter failed to read cached snapshot: {error:#}"),
        )
            .into_response(),
    }
}

pub(super) async fn refresh_observe_cache(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: AppConfig,
    bind: String,
    refresh_ms: u64,
) -> Result<()> {
    let started_epoch_ms = now_epoch_ms();
    {
        let mut state = cache.write().await;
        if state.refresh_in_progress {
            if !observe_refresh_stuck(&state) {
                return Ok(());
            }
            state.refresh_in_progress = false;
            state.last_error = Some(
                "previous observe refresh was declared stuck and recovered by watchdog".to_string(),
            );
        }
        state.last_refresh_started_epoch_ms = Some(started_epoch_ms);
        state.refresh_in_progress = true;
    }
    let cache_clone = cache.clone();
    let cfg_clone = cfg.clone();
    let bind_clone = bind.clone();
    let refresh_task = tokio::spawn(async move {
        let started = Instant::now();
        let result =
            match tokio::time::timeout(Duration::from_millis(OBSERVE_REFRESH_TIMEOUT_MS), async {
                build_snapshot(&cfg_clone, false)
                    .await
                    .and_then(|snapshot| {
                        dashboard::build_payload(&cfg_clone, &snapshot, &bind_clone, refresh_ms)
                            .map(|payload| (snapshot, payload))
                    })
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow!(
                    "observe refresh exceeded timeout of {} ms",
                    OBSERVE_REFRESH_TIMEOUT_MS
                )),
            };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let completed_epoch_ms = now_epoch_ms();
        let mut thread_id_to_prewarm = None;
        let outcome = {
            let mut state = cache_clone.write().await;
            state.refresh_in_progress = false;
            match result {
                Ok((snapshot, payload)) => {
                    state.last_refresh_completed_epoch_ms = Some(completed_epoch_ms);
                    state.last_refresh_duration_ms = Some(elapsed_ms);
                    state.snapshot = Some(snapshot);
                    state.dashboard_payload = Some(payload);
                    state.last_error = None;
                    thread_id_to_prewarm = match state.snapshot.as_ref() {
                        Some(value) => strict_auto_thread_binding_hint_from_snapshot(value.clone()),
                        None => None,
                    };
                    Ok(())
                }
                Err(error) => {
                    state.last_refresh_duration_ms = Some(elapsed_ms);
                    state.last_error = Some(format!("{error:#}"));
                    Err(error)
                }
            }
        };
        if let Some(thread_id) = thread_id_to_prewarm {
            if let Err(error) = prewarm_thread_bound_client_budget_surfaces_for_thread(
                cache_clone,
                &cfg_clone,
                &thread_id,
            )
            .await
            {
                eprintln!("refresh-triggered active thread prewarm failed: {error:#}");
            }
        }
        outcome
    });
    match refresh_task.await {
        Ok(outcome) => outcome,
        Err(error) => {
            let error_message = format!("{error:#}");
            let error_message_for_task = error_message.clone();
            let cache_cleanup = cache.clone();
            tokio::spawn(async move {
                let mut state = cache_cleanup.write().await;
                if state.refresh_in_progress {
                    state.refresh_in_progress = false;
                    state.last_error = Some(format!(
                        "observe refresh task aborted before completion: {error_message_for_task}"
                    ));
                }
            });
            Err(anyhow!("observe refresh task aborted: {error_message}"))
        }
    }
}

pub(super) async fn maybe_refresh_stale_observe_cache_for_healthz(
    state: &ObserveState,
) -> Result<()> {
    let should_refresh = {
        let cache = state.cache.read().await;
        let cache_stale = observe_cache_stale(&cache, state.dashboard_refresh_ms);
        let refresh_stuck = observe_refresh_stuck(&cache);
        cache_stale && (!cache.refresh_in_progress || refresh_stuck)
    };
    if !should_refresh {
        return Ok(());
    }
    refresh_observe_cache(
        state.cache.clone(),
        state.cfg.clone(),
        state.bind.clone(),
        state.dashboard_refresh_ms,
    )
    .await
}

pub(super) fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
