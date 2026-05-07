use crate::capacity_forecast;
use crate::config::{AppConfig, discover_repo_root};
use crate::regression_explain;
use crate::{
    artifact_cleanup,
    cli::{
        ContinuityCompactChatArgs, ObserveCapacityForecastArgs,
        ObserveClientBudgetHostControlLaunchArgs, ObserveRegressionExplainArgs,
    },
    codex_threads, compatibility, continuity, dashboard, external_benchmark, nats, postgres, s3,
    token_budget, working_state,
};
use anyhow::{Context, Result, anyhow, bail};
use axum::{
    Router,
    extract::{Json, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::process::Command as ProcessCommand;
use tokio::sync::RwLock;
use tokio_postgres::Client;
use uuid::Uuid;

mod observe_cache_meta;
mod observe_capacity_forecast_cli;
mod observe_cli;
mod observe_client_budget_cache;
mod observe_client_budget_runtime;
mod observe_control_api;
mod observe_front_door;
mod observe_governance_surface;
mod observe_guardrails;
mod observe_live_infra;
mod observe_live_surfaces;
mod observe_local_http;
mod observe_models;
mod observe_page_api;
mod observe_policy_models;
mod observe_refresh_runtime;
mod observe_retention;
mod observe_runtime_support;
mod observe_server_runtime;
mod observe_sla_metrics;
mod observe_snapshot_runtime;
mod observe_thread_bound;

use self::observe_cache_meta::*;
pub use self::observe_capacity_forecast_cli::print_capacity_forecast;
pub use self::observe_cli::{
    print_artifact_cleanup, print_budget_snapshot_preview, print_guardrails,
    print_regression_explain, print_retention_cleanup, print_snapshot, print_snapshot_preview,
    run_sla_check,
};
use self::observe_client_budget_cache::*;
#[cfg(test)]
use self::observe_client_budget_runtime::{
    client_budget_guard_blocks_reply, compact_client_budget_gate_payload,
    compact_working_state_restore_for_budget,
};
use self::observe_client_budget_runtime::{
    collect_client_budget_snapshot_from_db, collect_client_budget_snapshot_with_thread_hint,
    collect_compact_client_budget_surfaces, compact_budget_snapshot_preview_payload,
    compact_cli_client_budget_gate_from_root_cause_payload, compact_cli_client_budget_gate_payload,
    compact_client_budget_root_cause_payload, compact_client_budget_surfaces_from_snapshot,
    compact_current_session_budget_guard_payload, compact_host_control_client_budget_reply_gate,
    front_door_client_budget_gate_payload, maybe_auto_launch_same_thread_host_control_from_gate,
    normalize_front_door_client_budget_gate_payload_shape,
    prewarm_active_thread_bound_client_budget_surfaces,
    prewarm_thread_bound_client_budget_surfaces_for_thread,
    try_load_fast_thread_bound_materialized_compact_client_budget_surfaces,
};
pub(crate) use self::observe_client_budget_runtime::{
    print_client_budget_gate, print_client_budget_guard, print_client_budget_root_cause,
};
use self::observe_control_api::{
    client_budget_compact_chat_api_handler, client_budget_host_control_feedback_api_handler,
    client_budget_host_control_launch_api_handler, continuity_handoff_api_handler,
    observe_user_visible_client_thread, remediation_bundle_detail_api_handler,
    remediation_bundles_api_handler,
};
#[cfg(test)]
use self::observe_control_api::{
    client_budget_host_control_launch_api_summary, compact_chat_api_summary,
    evaluate_host_current_thread_control_window_targeting,
};
pub use self::observe_control_api::{
    client_budget_host_control_launch_payload, print_client_budget_host_control_launch,
};
use self::observe_front_door::{
    active_agent_budget_live_api_handler, client_budget_live_api_handler, dashboard_api_handler,
    dashboard_live_summary_api_handler,
};
use self::observe_governance_surface::collect_governance_surface;
use self::observe_guardrails::{
    cleanup_guardrail_rows, collect_guardrail_report, procedural_benchmark_history_surface,
};
#[cfg(test)]
use self::observe_live_infra::collect_qdrant_live_from;
use self::observe_live_infra::{
    collect_nats_live, collect_optional_benchmark_qdrant_live, collect_postgres_live,
    collect_qdrant_live, collect_s3_live, enrich_live_cold_benchmark_progress,
    read_live_cold_benchmark_progress, with_postgres_rates,
};
#[cfg(test)]
use self::observe_live_surfaces::{
    active_agent_budget_card_payload_from_snapshot,
    active_agent_card_refresh_needed_against_rollout, cached_client_live_meter_state,
    cached_exact_client_limit_refresh_needed, client_live_meter_refresh_needed,
    overlay_live_active_agent_surfaces,
};
use self::observe_live_surfaces::{
    cached_dashboard_payload, cached_snapshot_with_meta,
    dashboard_live_summary_payload_for_request, live_active_agent_budget_card_payload,
    live_active_agent_snapshot_for_request, refresh_client_live_meter_on_request,
    spawn_client_live_meter_refresh,
};
use self::observe_local_http::*;
use self::observe_models::*;
use self::observe_page_api::{
    agent_display_name_update_api_handler, brand_lockup_handler, brand_mark_handler,
    client_budget_gate_api_handler, client_budget_root_cause_api_handler,
    client_budget_snapshot_preview_api_handler, client_budget_target_update_api_handler,
    client_limit_hourly_burn_api_handler, dashboard_page_handler, favicon_handler,
    grafana_password_help_handler, healthz_handler, mark_observe_http_activity, no_store_headers,
    snapshot_api_handler,
};
use self::observe_policy_models::{build_continuity_correctness_model, build_degradation_model};
use self::observe_refresh_runtime::{
    maybe_refresh_stale_observe_cache_for_healthz, metrics_handler, now_epoch_ms,
    refresh_observe_cache,
};
#[cfg(test)]
use self::observe_retention::select_latest_clean_benchmark_snapshot;
#[cfg(test)]
use self::observe_retention::{
    artifact_cleanup_summary_is_fresh, expired_retention_candidates,
    select_latest_dashboard_cold_benchmark_snapshot,
};
use self::observe_retention::{
    collect_artifact_cleanup_summary, latest_clean_benchmark_snapshot,
    latest_dashboard_cold_benchmark_snapshot, maybe_cleanup_local_artifacts,
    maybe_cleanup_observability_snapshots, maybe_cleanup_observability_snapshots_with_db,
    run_retention_cleanup,
};
use self::observe_runtime_support::{timed_future, with_postgres_advisory_lock};
pub(crate) use self::observe_server_runtime::serve_metrics;
#[cfg(test)]
use self::observe_sla_metrics::benchmark_contamination_value;
use self::observe_sla_metrics::{
    benchmark_payload_contaminated, counter_delta, delta_rate, evaluate_sla,
    extract_nats_consumer_lag, http_client, load_profile, metric_value, metric_value_optional,
    parse_prometheus_sums, percentile_f64, profile_thresholds_json, ratio, ratio_f64,
    render_prometheus_metrics,
};
use self::observe_snapshot_runtime::{
    build_snapshot, collect_budget_snapshot_preview, latest_repo_working_state_restore_payload,
    load_shared_budget_snapshot_preview,
};
pub(crate) use self::observe_snapshot_runtime::{
    collect_snapshot, collect_snapshot_preview, human_dashboard_base_url,
};
use self::observe_thread_bound::{
    auto_thread_binding_hint_from_cache, cached_latest_repo_working_state_restore_snapshot,
    cached_token_budget_report_snapshot, compact_client_budget_snapshot_for_request,
    materialize_shared_thread_bound_client_budget_surfaces_from_snapshot,
    merged_thread_bound_snapshot_with_meta, normalized_thread_id_hint,
    populate_thread_bound_client_budget_surfaces_from_snapshot, resolved_request_thread_hint,
    strict_auto_thread_binding_hint_from_snapshot, thread_bound_dashboard_payload,
    thread_bound_snapshot_with_meta,
};
#[cfg(test)]
use self::observe_thread_bound::{
    compact_client_budget_snapshot_cache_too_old,
    merge_thread_bound_client_budget_snapshot_into_base_snapshot,
};

#[cfg(test)]
mod observe_tests;
