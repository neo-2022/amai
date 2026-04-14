use super::*;

const IMPORT_PACKET_QUARANTINE_RESOLUTION_INTERVAL: Duration = Duration::from_secs(60);

pub(super) async fn persist_periodic_client_limit_trend_analysis(cfg: &AppConfig) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let _ = token_budget::collect_exact_client_limit_trend_analysis(
        &db,
        300,
        10,
        token_budget::DEFAULT_CLIENT_LIMIT_TREND_ANALYSIS_LOOKBACK_MINUTES,
        true,
    )
    .await?;
    Ok(())
}

pub(super) async fn persist_periodic_import_packet_quarantine_resolution(
    cfg: &AppConfig,
) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let summary = postgres::reconcile_import_packet_quarantines(&db, true, Some(64)).await?;
    if summary.released > 0 || summary.rejected > 0 {
        eprintln!(
            "Amai import packet quarantine resolver: released={}, rejected={}, held={}, scanned={}",
            summary.released, summary.rejected, summary.held, summary.scanned
        );
    }
    Ok(())
}

pub(crate) async fn serve_metrics(cfg: &AppConfig, bind: &str) -> Result<()> {
    let profile = load_profile()?;
    maybe_cleanup_observability_snapshots(cfg).await?;
    maybe_cleanup_local_artifacts().await?;
    let bootstrap_db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&bootstrap_db, cfg).await?;
    let cache = Arc::new(RwLock::new(ObserveCache::default()));
    let cleanup_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SNAPSHOT_RETENTION_SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = maybe_cleanup_observability_snapshots(&cleanup_cfg).await {
                eprintln!("observability retention cleanup failed: {error:#}");
            }
        }
    });
    let artifact_cleanup_interval = artifact_cleanup::sweep_interval()?;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(artifact_cleanup_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = maybe_cleanup_local_artifacts().await {
                eprintln!("artifact cleanup failed: {error:#}");
            }
        }
    });
    let trend_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLIENT_LIMIT_TREND_ANALYSIS_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = persist_periodic_client_limit_trend_analysis(&trend_cfg).await {
                eprintln!("client limit trend analysis refresh failed: {error:#}");
            }
        }
    });
    let quarantine_resolution_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(IMPORT_PACKET_QUARANTINE_RESOLUTION_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) =
                persist_periodic_import_packet_quarantine_resolution(&quarantine_resolution_cfg)
                    .await
            {
                eprintln!("import packet quarantine resolution failed: {error:#}");
            }
        }
    });
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid observe bind address: {bind}"))?;
    let app = Router::new()
        .route("/", get(dashboard_page_handler))
        .route("/dashboard", get(dashboard_page_handler))
        .route("/help/grafana-password", get(grafana_password_help_handler))
        .route("/brand/amai_mark.svg", get(brand_mark_handler))
        .route("/brand/amai_lockup.svg", get(brand_lockup_handler))
        .route("/favicon.ico", get(favicon_handler))
        .route("/api/dashboard", get(dashboard_api_handler))
        .route(
            "/api/dashboard-live-summary",
            get(dashboard_live_summary_api_handler),
        )
        .route(
            "/api/client-budget-live",
            get(client_budget_live_api_handler),
        )
        .route(
            "/api/active-agent-budget-live",
            get(active_agent_budget_live_api_handler),
        )
        .route(
            "/api/client-budget-snapshot-preview",
            get(client_budget_snapshot_preview_api_handler),
        )
        .route(
            "/api/client-budget-root-cause",
            get(client_budget_root_cause_api_handler),
        )
        .route(
            "/api/client-budget-gate",
            get(client_budget_gate_api_handler),
        )
        .route(
            "/api/client-limit-hourly-burn",
            get(client_limit_hourly_burn_api_handler),
        )
        .route(
            "/api/client-budget-target",
            post(client_budget_target_update_api_handler),
        )
        .route(
            "/api/client-budget-compact-chat",
            post(client_budget_compact_chat_api_handler),
        )
        .route(
            "/api/continuity-handoff",
            post(continuity_handoff_api_handler),
        )
        .route(
            "/api/client-budget-host-control-launch",
            post(client_budget_host_control_launch_api_handler),
        )
        .route(
            "/api/client-budget-host-control-feedback",
            post(client_budget_host_control_feedback_api_handler),
        )
        .route(
            "/api/agent-display-name",
            post(agent_display_name_update_api_handler),
        )
        .route("/api/snapshot", get(snapshot_api_handler))
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(healthz_handler))
        .with_state(ObserveState {
            dashboard_refresh_ms: profile.dashboard.refresh_ms,
            cfg: cfg.clone(),
            bind: bind.to_string(),
            cache: cache.clone(),
        });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind observe exporter on {bind}"))?;
    let base_url = human_dashboard_base_url(bind);
    println!("Amai human dashboard: {base_url}/");
    println!("Amai dashboard JSON: {base_url}/api/dashboard");
    println!("Amai live client budget JSON: {base_url}/api/client-budget-live");
    println!("Amai raw snapshot JSON: {base_url}/api/snapshot");
    println!("Amai health JSON: {base_url}/healthz");
    println!("Amai Prometheus metrics: {base_url}/metrics");
    axum::serve(listener, app)
        .await
        .context("observe exporter stopped unexpectedly")
}
