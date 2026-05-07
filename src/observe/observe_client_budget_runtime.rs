use crate::config::discover_repo_root;
use crate::dashboard;
use crate::postgres;
use crate::token_budget;
use crate::working_state;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_postgres::Client;

use super::{
    AppConfig, ObserveCache, ObserveClientBudgetHostControlLaunchArgs,
    build_compact_client_budget_gate_cache, build_compact_client_budget_surfaces_cache,
    cached_latest_repo_working_state_restore_snapshot, cached_token_budget_report_snapshot,
    client_budget_host_control_launch_payload, default_continuity_namespace,
    load_shared_active_thread_hint, load_shared_budget_snapshot_preview,
    load_shared_compact_client_budget_gate, load_shared_compact_client_budget_surfaces,
    load_shared_thread_bound_budget_snapshot,
    populate_thread_bound_client_budget_surfaces_from_snapshot, resolved_local_observe_thread_id,
    strict_auto_thread_binding_hint_from_snapshot, try_fetch_local_observe_gate_payload_via_http,
    try_fetch_local_observe_root_cause_payload_via_http, write_shared_compact_client_budget_gate,
    write_shared_compact_client_budget_surfaces,
};

#[derive(Debug, Clone)]
pub(super) struct CompactClientBudgetSurfaces {
    pub(super) root_cause_payload: Value,
    pub(super) guard_payload: Value,
    pub(super) guard: Value,
}

#[derive(Debug, Clone)]
pub(super) struct CompactClientBudgetGateSurface {
    pub(super) gate_payload: Value,
}

#[derive(Debug, Clone)]
pub(super) struct MaterializedCompactClientBudgetSurfaces {
    pub(super) surfaces: CompactClientBudgetSurfaces,
    pub(super) gate: CompactClientBudgetGateSurface,
}

pub(crate) async fn print_client_budget_guard(
    cfg: &AppConfig,
    enforce_reply_gate: bool,
    explicit_thread_id: Option<&str>,
) -> Result<()> {
    let CompactClientBudgetSurfaces {
        guard,
        guard_payload: payload,
        ..
    } = if let Some(thread_id) = resolved_local_observe_thread_id(explicit_thread_id) {
        let repo_root = discover_repo_root(None)?;
        if let Some(materialized) =
            try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                &repo_root, &thread_id,
            )
        {
            materialized.surfaces
        } else {
            let snapshot = collect_client_budget_snapshot_with_thread_hint(
                cfg,
                &repo_root,
                Some(&thread_id),
                None,
                None,
            )
            .await?;
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(&thread_id))
                .surfaces
        }
    } else {
        collect_compact_client_budget_surfaces(cfg).await?
    };
    println!("{}", serde_json::to_string(&payload)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&guard) {
        let action_kind = guard["reply_execution_gate"]["action_kind"]
            .as_str()
            .unwrap_or("continue_current_chat");
        let blocked_reply_hint = match action_kind {
            "wait_for_global_client_budget_recovery" => {
                "wait for global client budget recovery before replying"
            }
            "rotate_chat_for_client_budget" => {
                "rotate into a new clean work surface before replying"
            }
            _ => "refresh the live client budget gate before replying",
        };
        return Err(anyhow!(
            "client budget reply gate blocked this reply: {blocked_reply_hint}"
        ));
    }
    Ok(())
}

pub(crate) async fn print_client_budget_gate(
    cfg: &AppConfig,
    enforce_reply_gate: bool,
    explicit_thread_id: Option<&str>,
) -> Result<()> {
    let payload = if let Some(payload) =
        try_fetch_local_observe_gate_payload_via_http(explicit_thread_id).await
    {
        payload
    } else if let Some(thread_id) = resolved_local_observe_thread_id(explicit_thread_id) {
        let repo_root = discover_repo_root(None)?;
        if let Some(cached) = load_shared_compact_client_budget_gate(
            &repo_root,
            super::current_epoch_ms_u64(),
            Some(&thread_id),
        ) {
            cached.gate
        } else if let Some(materialized) =
            try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                &repo_root, &thread_id,
            )
        {
            materialized.gate.gate_payload
        } else {
            let snapshot = collect_client_budget_snapshot_with_thread_hint(
                cfg,
                &repo_root,
                Some(&thread_id),
                None,
                None,
            )
            .await?;
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(&thread_id))
                .gate
                .gate_payload
        }
    } else {
        let CompactClientBudgetGateSurface { gate_payload, .. } =
            collect_compact_client_budget_gate_surface(cfg).await?;
        gate_payload
    };
    let payload = normalize_front_door_client_budget_gate_payload_shape(payload);
    println!("{}", serde_json::to_string(&payload)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&payload["client_budget_reply_gate"])
    {
        let action_kind =
            payload["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"]
                .as_str()
                .unwrap_or("continue_current_chat");
        let blocked_reply_hint = match action_kind {
            "wait_for_global_client_budget_recovery" => {
                "wait for global client budget recovery before replying"
            }
            "rotate_chat_for_client_budget" => {
                "rotate into a new clean work surface before replying"
            }
            _ => "refresh the live client budget gate before replying",
        };
        return Err(anyhow!(
            "client budget reply gate blocked this reply: {blocked_reply_hint}"
        ));
    }
    Ok(())
}

pub(crate) async fn print_client_budget_root_cause(
    cfg: &AppConfig,
    enforce_reply_gate: bool,
    explicit_thread_id: Option<&str>,
) -> Result<()> {
    let compact = if let Some(payload) =
        try_fetch_local_observe_root_cause_payload_via_http(explicit_thread_id).await
    {
        payload
    } else if let Some(thread_id) = resolved_local_observe_thread_id(explicit_thread_id) {
        let repo_root = discover_repo_root(None)?;
        if let Some(materialized) =
            try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
                &repo_root, &thread_id,
            )
        {
            materialized.surfaces.root_cause_payload
        } else {
            let snapshot = collect_client_budget_snapshot_with_thread_hint(
                cfg,
                &repo_root,
                Some(&thread_id),
                None,
                None,
            )
            .await?;
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(&thread_id))
                .surfaces
                .root_cause_payload
        }
    } else {
        collect_compact_client_budget_surfaces(cfg)
            .await?
            .root_cause_payload
    };
    println!("{}", serde_json::to_string(&compact)?);
    if enforce_reply_gate && client_budget_guard_blocks_reply(&compact["client_budget_reply_gate"])
    {
        let action_kind =
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"]
                .as_str()
                .unwrap_or("continue_current_chat");
        let blocked_reply_hint = match action_kind {
            "wait_for_global_client_budget_recovery" => {
                "wait for global client budget recovery before replying"
            }
            "rotate_chat_for_client_budget" => {
                "rotate into a new clean work surface before replying"
            }
            _ => "refresh the live client budget gate before replying",
        };
        return Err(anyhow!(
            "client budget reply gate blocked this reply: {blocked_reply_hint}"
        ));
    }
    Ok(())
}

pub(super) fn client_budget_guard_blocks_reply(guard: &Value) -> bool {
    working_state::client_budget_guard_blocks_reply(guard)
}

pub(super) async fn collect_compact_client_budget_gate_surface(
    cfg: &AppConfig,
) -> Result<CompactClientBudgetGateSurface> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms = super::current_epoch_ms_u64();
    if let Some(cached) = load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms, None) {
        return Ok(CompactClientBudgetGateSurface {
            gate_payload: cached.gate,
        });
    }
    if let Some(cached) = load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms, None)
    {
        let gate_cache = build_compact_client_budget_gate_cache(&cached.gate, &cached.guard, None);
        let _ = write_shared_compact_client_budget_gate(&repo_root, None, &gate_cache);
        return Ok(CompactClientBudgetGateSurface {
            gate_payload: cached.gate,
        });
    }
    let snapshot = collect_client_budget_snapshot(cfg, &repo_root).await?;
    Ok(compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, None).gate)
}

pub(super) async fn collect_compact_client_budget_surfaces(
    cfg: &AppConfig,
) -> Result<CompactClientBudgetSurfaces> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms = super::current_epoch_ms_u64();
    if let Some(cached) = load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms, None)
    {
        return Ok(CompactClientBudgetSurfaces {
            root_cause_payload: cached.root_cause,
            guard_payload: cached.guard.clone(),
            guard: cached.guard,
        });
    }
    let snapshot = collect_client_budget_snapshot(cfg, &repo_root).await?;
    Ok(compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, None).surfaces)
}

pub(super) async fn prewarm_thread_bound_client_budget_surfaces_for_thread(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: &AppConfig,
    thread_id: &str,
) -> Result<()> {
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms_value = super::current_epoch_ms_u64();
    if load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms_value, Some(thread_id))
        .is_some()
        && let Some(cached_gate) =
            load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms_value, Some(thread_id))
    {
        let maybe_launched = maybe_auto_launch_same_thread_host_control_from_gate(
            cfg,
            &repo_root,
            thread_id,
            &cached_gate.gate,
        )
        .await?;
        if maybe_launched.is_none() {
            return Ok(());
        }
        return Ok(());
    }
    if let Some(snapshot) =
        load_shared_thread_bound_budget_snapshot(&repo_root, now_epoch_ms_value, thread_id)
    {
        let materialized =
            compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(thread_id));
        let _ = maybe_auto_launch_same_thread_host_control_from_gate(
            cfg,
            &repo_root,
            thread_id,
            &materialized.gate.gate_payload,
        )
        .await?;
        populate_thread_bound_client_budget_surfaces_from_snapshot(
            cache, &repo_root, thread_id, snapshot,
        )
        .await;
        return Ok(());
    }

    let (latest_repo_restore_override, base_report_override) = {
        let state = cache.read().await;
        (
            cached_latest_repo_working_state_restore_snapshot(&state),
            cached_token_budget_report_snapshot(&state),
        )
    };
    let snapshot = collect_client_budget_snapshot_with_thread_hint(
        cfg,
        &repo_root,
        Some(thread_id),
        base_report_override.as_ref(),
        latest_repo_restore_override.as_ref(),
    )
    .await?;
    let materialized =
        compact_client_budget_surfaces_from_snapshot(&repo_root, &snapshot, Some(thread_id));
    let _ = maybe_auto_launch_same_thread_host_control_from_gate(
        cfg,
        &repo_root,
        thread_id,
        &materialized.gate.gate_payload,
    )
    .await?;
    populate_thread_bound_client_budget_surfaces_from_snapshot(
        cache, &repo_root, thread_id, snapshot,
    )
    .await;
    Ok(())
}

pub(super) async fn prewarm_active_thread_bound_client_budget_surfaces(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: &AppConfig,
) -> Result<()> {
    let (snapshot_thread_id, activity) = {
        let state = cache.read().await;
        let snapshot = state.snapshot.as_ref();
        (
            match snapshot {
                Some(value) => strict_auto_thread_binding_hint_from_snapshot(value.clone()),
                None => None,
            },
            snapshot.map(|item| item["agent_scope_activity"].clone()),
        )
    };
    if let Some(activity) = activity.as_ref() {
        let thread_ids = token_budget::active_agent_thread_ids_from_activity(
            activity,
            super::current_epoch_ms_u64() as i64,
        );
        if !thread_ids.is_empty() {
            return prewarm_active_agent_thread_bound_client_budget_surfaces(cache, cfg, activity)
                .await;
        }
    }
    let repo_root = discover_repo_root(None)?;
    let now_epoch_ms_value = super::current_epoch_ms_u64();
    let Some(thread_id) = snapshot_thread_id
        .or_else(|| load_shared_active_thread_hint(&repo_root, now_epoch_ms_value))
    else {
        return Ok(());
    };
    prewarm_thread_bound_client_budget_surfaces_for_thread(cache, cfg, &thread_id).await?;
    Ok(())
}

async fn prewarm_active_agent_thread_bound_client_budget_surfaces(
    cache: Arc<RwLock<ObserveCache>>,
    cfg: &AppConfig,
    activity: &Value,
) -> Result<()> {
    let thread_ids = token_budget::active_agent_thread_ids_from_activity(
        activity,
        super::current_epoch_ms_u64() as i64,
    );
    for thread_id in thread_ids {
        prewarm_thread_bound_client_budget_surfaces_for_thread(cache.clone(), cfg, &thread_id)
            .await?;
    }
    Ok(())
}

pub(super) async fn collect_client_budget_snapshot(
    cfg: &AppConfig,
    repo_root: &Path,
) -> Result<Value> {
    collect_client_budget_snapshot_with_thread_hint(cfg, repo_root, None, None, None).await
}

pub(super) async fn collect_client_budget_snapshot_with_thread_hint(
    cfg: &AppConfig,
    repo_root: &Path,
    thread_id_hint: Option<&str>,
    base_report_override: Option<&Value>,
    latest_repo_restore_override: Option<&Value>,
) -> Result<Value> {
    if let Ok(db) = postgres::connect_app(cfg).await {
        if let Ok(snapshot) = collect_client_budget_snapshot_from_db(
            &db,
            repo_root,
            thread_id_hint,
            base_report_override,
            latest_repo_restore_override,
        )
        .await
        {
            return Ok(snapshot);
        }
    }

    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    collect_client_budget_snapshot_from_db(
        &db,
        repo_root,
        thread_id_hint,
        base_report_override,
        latest_repo_restore_override,
    )
    .await
}

pub(super) async fn collect_client_budget_snapshot_from_db(
    db: &Client,
    repo_root: &Path,
    thread_id_hint: Option<&str>,
    base_report_override: Option<&Value>,
    latest_repo_restore_override: Option<&Value>,
) -> Result<Value> {
    let latest_repo_restore_raw = latest_repo_restore_override
        .cloned()
        .or(super::latest_repo_working_state_restore_payload(&db, repo_root).await?);
    working_state::maintain_same_thread_execctl_active_lease_for_guard(
        db,
        latest_repo_restore_raw.as_ref(),
        thread_id_hint,
    )
    .await?;
    let report =
        token_budget::collect_dashboard_current_session_budget_report_with_thread_hint_and_base(
            &db,
            base_report_override,
            thread_id_hint,
        )
        .await?;
    let agent_scope_activity = token_budget::collect_agent_scope_activity(db).await?;
    let active_agent_budget = token_budget::collect_active_agent_live_budget_surface(
        db,
        repo_root,
        &agent_scope_activity,
    )
    .await?;
    let latest_repo_working_state_restore =
        compact_latest_repo_working_state_restore_from_optional_payload(
            latest_repo_restore_raw.as_ref(),
        );
    Ok(json!({
        "token_budget_report": {
            "token_budget_report": report["token_budget_report"].clone(),
        },
        "active_agent_budget": active_agent_budget,
        "latest_repo_working_state_restore": latest_repo_working_state_restore,
    }))
}

fn compact_latest_repo_working_state_restore_for_budget(payload: &Value) -> Value {
    json!({
        "working_state_restore": compact_working_state_restore_for_budget(
            &payload["working_state_restore"]
        )
    })
}

fn compact_latest_repo_working_state_restore_from_optional_payload(payload: Option<&Value>) -> Value {
    payload
        .map(compact_latest_repo_working_state_restore_for_budget)
        .unwrap_or_else(|| json!({ "working_state_restore": {} }))
}

pub(super) fn compact_working_state_restore_for_budget(restore: &Value) -> Value {
    if !restore.is_object() {
        return json!({});
    }

    let recent_actions = restore["recent_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| {
            action["source_kind"].as_str()
                == Some(working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND)
                && action["host_current_thread_control_feedback"].is_object()
        })
        .map(|action| {
            json!({
                "source_kind": action["source_kind"].clone(),
                "summary": action["summary"].clone(),
                "recorded_at_epoch_ms": action["recorded_at_epoch_ms"].clone(),
                "host_current_thread_control_feedback": {
                    "feedback_kind": action["host_current_thread_control_feedback"]["feedback_kind"].clone(),
                    "command_id": action["host_current_thread_control_feedback"]["command_id"].clone(),
                    "working_state_write_status":
                        action["host_current_thread_control_feedback"]["working_state_write_status"].clone(),
                    "feedback_snapshot": {
                        "thread_id": action["host_current_thread_control_feedback"]["feedback_snapshot"]["thread_id"].clone(),
                        "client_live_meter": {
                            "client_turn_total_tokens":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]["client_turn_total_tokens"].clone(),
                            "context_used_percent":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]["context_used_percent"].clone(),
                            "primary_limit_used_percent":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]["primary_limit_used_percent"].clone()
                        },
                        "host_context_compaction": {
                            "compaction_count":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["compaction_count"].clone(),
                            "growth_since_compaction_tokens":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["growth_since_compaction_tokens"].clone(),
                            "compacted_at_epoch_ms":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["compacted_at_epoch_ms"].clone(),
                            "stage":
                                action["host_current_thread_control_feedback"]["feedback_snapshot"]["host_context_compaction"]["stage"].clone(),
                        }
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    json!({
        "client_budget_target_percent": restore["client_budget_target_percent"].clone(),
        "thread_id": restore["thread_id"].clone(),
        "current_goal": restore["current_goal"].clone(),
        "next_step": restore["next_step"].clone(),
        "execctl_resume_state": restore["execctl_resume_state"].clone(),
        "project": {
            "code": restore["project"]["code"].clone(),
            "repo_root": restore["project"]["repo_root"].clone()
        },
        "namespace": {
            "code": restore["namespace"]["code"].clone()
        },
        "state_lineage": {
            "authoritative_event_id": restore["state_lineage"]["authoritative_event_id"].clone(),
            "authoritative_source_kind": restore["state_lineage"]["authoritative_source_kind"].clone(),
            "authoritative_local_path": restore["state_lineage"]["authoritative_local_path"].clone()
        },
        "recent_actions": recent_actions
    })
}

fn compact_current_session_for_budget_snapshot_preview(current_session: &Value) -> Value {
    if !current_session.is_object() {
        return Value::Null;
    }
    json!({
        "started_at_epoch_ms": current_session["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": current_session["ended_at_epoch_ms"].clone(),
        "counted_events": current_session["counted_events"].clone(),
        "live_events_count": current_session["live_events_count"].clone(),
        "total_saved_tokens": current_session["total_saved_tokens"].clone(),
        "savings_percent": rounded_json_number(&current_session["savings_percent"], 2),
        "effective_savings_pct": rounded_json_number(&current_session["effective_savings_pct"], 2),
        "observed_client_prompt_tokens": current_session["observed_client_prompt_tokens"].clone(),
        "observed_tool_overhead_tokens": current_session["observed_tool_overhead_tokens"].clone(),
        "observed_continuity_restore_tokens": current_session["observed_continuity_restore_tokens"].clone(),
        "observed_assistant_generation_tokens": current_session["observed_assistant_generation_tokens"].clone(),
        "age_ms_since_latest": current_session["age_ms_since_latest"].clone()
    })
}

fn compact_client_live_meter_for_budget_snapshot_preview(client_live_meter: &Value) -> Value {
    if !client_live_meter.is_object() {
        return Value::Null;
    }
    json!({
        "status": client_live_meter["status"].clone(),
        "thread_binding_state": client_live_meter["thread_binding_state"].clone(),
        "current_thread_bound": client_live_meter["current_thread_bound"].clone(),
        "thread_id": client_live_meter["thread_id"].clone(),
        "turn_id": client_live_meter["turn_id"].clone(),
        "started_at_epoch_ms": client_live_meter["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": client_live_meter["ended_at_epoch_ms"].clone(),
        "client_turn_total_tokens": client_live_meter["client_turn_total_tokens"].clone(),
        "context_used_percent": rounded_json_number(&client_live_meter["context_used_percent"], 2),
        "primary_limit_used_percent": client_live_meter["primary_limit_used_percent"].clone(),
        "secondary_limit_used_percent": client_live_meter["secondary_limit_used_percent"].clone(),
        "rollout_jsonl_tolerance_summary": client_live_meter["rollout_jsonl_tolerance_summary"].clone(),
        "rollout_jsonl_tolerated_skips_present": client_live_meter["rollout_jsonl_tolerated_skips_present"].clone(),
        "rollout_jsonl_malformed_objects_fail_closed": client_live_meter["rollout_jsonl_malformed_objects_fail_closed"].clone()
    })
}

fn compact_current_live_turn_for_budget_snapshot_preview(current_live_turn: &Value) -> Value {
    if !current_live_turn.is_object() {
        return Value::Null;
    }
    json!({
        "status": current_live_turn["status"].clone(),
        "scope_code": current_live_turn["scope_code"].clone(),
        "thread_binding_state": current_live_turn["thread_binding_state"].clone(),
        "current_thread_bound": current_live_turn["current_thread_bound"].clone(),
        "thread_id": current_live_turn["thread_id"].clone(),
        "turn_id": current_live_turn["turn_id"].clone(),
        "started_at_epoch_ms": current_live_turn["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": current_live_turn["ended_at_epoch_ms"].clone(),
        "exact_pair_available": current_live_turn["exact_pair_available"].clone(),
        "exact_pair": current_live_turn["exact_pair"].clone(),
        "matched_events_count": current_live_turn["matched_events_count"].clone(),
        "matched_context_pack_ids_count": current_live_turn["matched_context_pack_ids_count"].clone(),
        "retrieval_context_pack_count": current_live_turn["retrieval_context_pack_count"].clone()
    })
}

fn compact_client_limit_hourly_burn_for_budget_snapshot_preview(
    client_limit_hourly_burn: &Value,
) -> Value {
    if !client_limit_hourly_burn.is_object() {
        return Value::Null;
    }
    json!({
        "status": client_limit_hourly_burn["status"].clone(),
        "reply_prefix": client_limit_hourly_burn["reply_prefix"].clone(),
        "kpi_percent": rounded_json_number(&client_limit_hourly_burn["kpi_percent"], 2),
        "actual_used_percent": rounded_json_number(&client_limit_hourly_burn["actual_used_percent"], 2),
        "actual_remaining_percent":
            rounded_json_number(&client_limit_hourly_burn["actual_remaining_percent"], 2),
        "ideal_used_percent": rounded_json_number(&client_limit_hourly_burn["ideal_used_percent"], 2),
        "ideal_remaining_percent":
            rounded_json_number(&client_limit_hourly_burn["ideal_remaining_percent"], 2),
        "projected_primary_used_per_hour_percent": rounded_json_number(
            &client_limit_hourly_burn["projected_primary_used_per_hour_percent"],
            2
        ),
        "ideal_primary_used_per_hour_percent": rounded_json_number(
            &client_limit_hourly_burn["ideal_primary_used_per_hour_percent"],
            2
        ),
        "latest_observed_at_epoch_ms": client_limit_hourly_burn["latest_observed_at_epoch_ms"].clone(),
        "latest_live_age_seconds":
            rounded_json_number(&client_limit_hourly_burn["latest_live_age_seconds"], 2)
    })
}

fn compact_client_limit_meter_alignment_for_budget_snapshot_preview(alignment: &Value) -> Value {
    if !alignment.is_object() {
        return Value::Null;
    }
    let baseline_component_semantics = alignment["baseline_equivalence"]["component_semantics"]
        .as_array()
        .map(|items| {
            Value::Array(
                items
                    .iter()
                    .map(|item| {
                        json!({
                            "code": item["code"].clone(),
                            "baseline_measured_tokens": item["baseline_measured_tokens"].clone(),
                            "observed_tokens": item["observed_tokens"].clone(),
                            "whole_cycle_observed_complete":
                                item["whole_cycle_observed_complete"].clone()
                        })
                    })
                    .collect(),
            )
        })
        .unwrap_or(Value::Null);
    json!({
        "alignment_state": alignment["alignment_state"].clone(),
        "same_meter_as_client_limit": alignment["same_meter_as_client_limit"].clone(),
        "exact_pair_status": alignment["exact_pair_status"].clone(),
        "strict_client_meter_slice": {
            "lower_bound_tokens": alignment["strict_client_meter_slice"]["lower_bound_tokens"].clone()
        },
        "baseline_equivalence": {
            "measured_baseline_tokens_lower_bound":
                alignment["baseline_equivalence"]["measured_baseline_tokens_lower_bound"].clone(),
            "component_semantics": baseline_component_semantics
        },
        "blocking_reasons": alignment["blocking_reasons"].clone(),
        "measured_components": alignment["measured_components"].clone(),
        "missing_components": alignment["missing_components"].clone(),
        "not_applicable_components": alignment["not_applicable_components"].clone()
    })
}

pub(super) fn compact_budget_snapshot_preview_payload(snapshot: &Value) -> Value {
    let report = &snapshot["token_budget_report"]["token_budget_report"];
    json!({
        "latest_repo_working_state_restore": compact_latest_repo_working_state_restore_for_budget(
            &snapshot["latest_repo_working_state_restore"]
        ),
        "token_budget_report": {
            "token_budget_report": {
                "surface": report["surface"].clone(),
                "client_budget_target_percent": report["client_budget_target_percent"].clone(),
                "current_session":
                    compact_current_session_for_budget_snapshot_preview(&report["current_session"]),
                "statement_previews": {
                    "current_session": {
                        "observed_whole_cycle_with_amai_tokens":
                            report["statement_previews"]["current_session"]["observed_whole_cycle_with_amai_tokens"].clone(),
                        "verified_observed_whole_cycle_with_amai_tokens":
                            report["statement_previews"]["current_session"]["verified_observed_whole_cycle_with_amai_tokens"].clone(),
                        "with_amai_measured_tokens":
                            report["statement_previews"]["current_session"]["with_amai_measured_tokens"].clone(),
                        "verified_with_amai_measured_tokens":
                            report["statement_previews"]["current_session"]["verified_with_amai_measured_tokens"].clone(),
                        "client_limit_meter_alignment":
                            compact_client_limit_meter_alignment_for_budget_snapshot_preview(
                                &report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                            )
                    }
                },
                "client_limit_hourly_burn": compact_client_limit_hourly_burn_for_budget_snapshot_preview(
                    &report["client_limit_hourly_burn"]
                ),
                "client_live_meter":
                    compact_client_live_meter_for_budget_snapshot_preview(&report["client_live_meter"]),
                "current_live_turn":
                    compact_current_live_turn_for_budget_snapshot_preview(&report["current_live_turn"])
            }
        }
    })
}

pub(super) fn compact_client_budget_surfaces_from_snapshot(
    repo_root: &Path,
    snapshot: &Value,
    thread_id: Option<&str>,
) -> MaterializedCompactClientBudgetSurfaces {
    let guard = dashboard::current_session_budget_guard(snapshot);
    let root_cause_payload =
        dashboard::client_budget_root_cause_payload_with_guard(snapshot, &guard);
    let compact_root_cause =
        compact_client_budget_root_cause_payload(&root_cause_payload, Some(&guard));
    let compact_gate =
        front_door_client_budget_gate_payload(compact_cli_client_budget_gate_payload(&guard));
    let compact_guard = compact_current_session_budget_guard_payload(&guard);
    let surfaces_cache = build_compact_client_budget_surfaces_cache(
        &compact_root_cause,
        &compact_gate,
        &compact_guard,
        thread_id,
    );
    let _ = write_shared_compact_client_budget_surfaces(repo_root, thread_id, &surfaces_cache);
    let gate_cache =
        build_compact_client_budget_gate_cache(&compact_gate, &compact_guard, thread_id);
    let _ = write_shared_compact_client_budget_gate(repo_root, thread_id, &gate_cache);
    MaterializedCompactClientBudgetSurfaces {
        surfaces: CompactClientBudgetSurfaces {
            root_cause_payload: compact_root_cause,
            guard_payload: compact_guard.clone(),
            guard: compact_guard.clone(),
        },
        gate: CompactClientBudgetGateSurface {
            gate_payload: compact_gate,
        },
    }
}

pub(super) fn try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
    repo_root: &Path,
    thread_id: &str,
) -> Option<MaterializedCompactClientBudgetSurfaces> {
    let now_epoch_ms = super::current_epoch_ms_u64();
    if let Some(cached) =
        load_shared_compact_client_budget_surfaces(repo_root, now_epoch_ms, Some(thread_id))
    {
        return Some(MaterializedCompactClientBudgetSurfaces {
            surfaces: CompactClientBudgetSurfaces {
                root_cause_payload: cached.root_cause,
                guard_payload: cached.guard.clone(),
                guard: cached.guard.clone(),
            },
            gate: CompactClientBudgetGateSurface {
                gate_payload: cached.gate,
            },
        });
    }
    let snapshot = load_shared_budget_snapshot_preview(repo_root, Some(thread_id))?;
    Some(compact_client_budget_surfaces_from_snapshot(
        repo_root,
        &snapshot,
        Some(thread_id),
    ))
}

pub(super) fn compact_current_session_budget_guard_payload(guard: &Value) -> Value {
    let mut payload = serde_json::Map::from_iter([
        ("status_label".to_string(), guard["status_label"].clone()),
        (
            "full_turn_savings_proven".to_string(),
            guard["full_turn_savings_proven"].clone(),
        ),
        (
            "full_turn_savings_percent".to_string(),
            guard["full_turn_savings_percent"].clone(),
        ),
        (
            "should_rotate_chat_now".to_string(),
            guard["should_rotate_chat_now"].clone(),
        ),
        (
            "should_rotate_chat_soon".to_string(),
            guard["should_rotate_chat_soon"].clone(),
        ),
        (
            "requires_global_budget_recovery_before_reply".to_string(),
            guard["requires_global_budget_recovery_before_reply"].clone(),
        ),
        ("next_action".to_string(), guard["next_action"].clone()),
        ("last_request".to_string(), guard["last_request"].clone()),
        ("client_limits".to_string(), guard["client_limits"].clone()),
        ("tracked_slice".to_string(), guard["tracked_slice"].clone()),
        (
            "tracked_slice_truth".to_string(),
            guard["tracked_slice_truth"].clone(),
        ),
        (
            "client_live_meter_current_thread_bound".to_string(),
            guard["client_live_meter_current_thread_bound"].clone(),
        ),
        (
            "client_live_meter_thread_binding_state".to_string(),
            guard["client_live_meter_thread_binding_state"].clone(),
        ),
        (
            "observed_at_epoch_ms".to_string(),
            guard["observed_at_epoch_ms"].clone(),
        ),
        (
            "max_guard_age_seconds".to_string(),
            guard["max_guard_age_seconds"].clone(),
        ),
        (
            "reply_execution_gate".to_string(),
            compact_reply_execution_gate(&guard["reply_execution_gate"]),
        ),
    ]);
    if !guard["delivery_surface_status_label"].is_null() {
        payload.insert(
            "delivery_surface_status_label".to_string(),
            guard["delivery_surface_status_label"].clone(),
        );
    }
    Value::Object(payload)
}

pub(super) fn compact_client_budget_gate_payload(guard: &Value) -> Value {
    let reply_execution_gate = &guard["reply_execution_gate"];
    let carries_heavy_host_context = guard["host_context_compaction"].is_object();
    let mut payload = serde_json::Map::from_iter([
        ("status_label".to_string(), guard["status_label"].clone()),
        (
            "observed_at_epoch_ms".to_string(),
            guard["observed_at_epoch_ms"].clone(),
        ),
        (
            "max_guard_age_seconds".to_string(),
            guard["max_guard_age_seconds"].clone(),
        ),
        (
            "reply_execution_gate".to_string(),
            compact_reply_execution_gate(reply_execution_gate),
        ),
    ]);
    if !carries_heavy_host_context && !guard["host_context_compaction"].is_null() {
        payload.insert(
            "host_context_compaction".to_string(),
            guard["host_context_compaction"].clone(),
        );
    }
    for field in ["delivery_surface_status_label"] {
        if !guard[field].is_null() {
            payload.insert(field.to_string(), guard[field].clone());
        }
    }
    if !carries_heavy_host_context {
        for field in ["reply_prefix", "global_reply_prefix", "reply_prefix_source"] {
            if !guard[field].is_null() {
                payload.insert(field.to_string(), guard[field].clone());
            }
        }
    }
    Value::Object(payload)
}

fn compact_host_context_compaction_for_cli(host_context_compaction: &Value) -> Value {
    json!({
        "stage": host_context_compaction["stage"].clone(),
        "current_thread_bound": host_context_compaction["current_thread_bound"].clone(),
        "current_turn_total_tokens": host_context_compaction["current_turn_total_tokens"].clone(),
        "growth_since_compaction_tokens":
            host_context_compaction["growth_since_compaction_tokens"].clone(),
        "regrowth_of_recovered_surface_ratio":
            host_context_compaction["regrowth_of_recovered_surface_ratio"].clone(),
        "critical_regrowth_active":
            host_context_compaction["critical_regrowth_active"].clone(),
        "preserve_active": host_context_compaction["preserve_active"].clone(),
    })
}

fn compact_host_context_compaction_for_root_cause_cli(host_context_compaction: &Value) -> Value {
    json!({
        "stage": host_context_compaction["stage"].clone(),
        "growth_since_compaction_tokens":
            host_context_compaction["growth_since_compaction_tokens"].clone(),
        "regrowth_of_recovered_surface_ratio":
            rounded_json_number(&host_context_compaction["regrowth_of_recovered_surface_ratio"], 2),
    })
}

fn compact_current_live_meter_for_root_cause_cli(current_live_meter: &Value) -> Value {
    json!({
        "client_turn_total_tokens": current_live_meter["client_turn_total_tokens"].clone(),
        "context_used_percent": rounded_json_number(&current_live_meter["context_used_percent"], 2),
    })
}

fn compact_current_live_turn_for_root_cause_cli(current_live_turn: &Value) -> Value {
    json!({
        "saved_pct": rounded_json_number(&current_live_turn["saved_pct"], 2),
        "status": current_live_turn["status"].clone(),
    })
}

fn compact_same_meter_economics_for_root_cause_cli(same_meter_economics: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    for field in [
        "strict_lower_bound_tokens",
        "same_meter_without_amai_tokens",
        "same_meter_with_amai_tokens",
        "same_meter_saved_tokens",
        "continuity_restore_baseline_tokens",
        "continuity_restore_observed_tokens",
        "continuity_restore_delta_tokens",
        "full_turn_overhang_tokens",
        "dominant_cost_surface",
    ] {
        if !same_meter_economics[field].is_null() {
            compact.insert(field.to_string(), same_meter_economics[field].clone());
        }
    }
    for (field, value) in [
        (
            "same_meter_saved_pct",
            rounded_json_number(&same_meter_economics["same_meter_saved_pct"], 2),
        ),
        (
            "full_turn_vs_strict_ratio",
            rounded_json_number(&same_meter_economics["full_turn_vs_strict_ratio"], 2),
        ),
    ] {
        if !value.is_null() {
            compact.insert(field.to_string(), value);
        }
    }
    Value::Object(compact)
}

fn compact_guard_for_root_cause_cli(guard: &Value) -> Value {
    json!({
        "should_rotate_chat_now": guard["should_rotate_chat_now"].clone(),
    })
}

fn rounded_json_number(value: &Value, decimals: u32) -> Value {
    let Some(number) = value.as_f64() else {
        return value.clone();
    };
    let scale = 10f64.powi(decimals as i32);
    serde_json::Number::from_f64((number * scale).round() / scale)
        .map(Value::Number)
        .unwrap_or_else(|| value.clone())
}

fn compact_host_current_thread_control_effect_for_cli(effect: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    for field in [
        "command_id",
        "surface_label",
        "thread_id",
        "current_thread_id",
        "current_stage",
        "recorded_at_epoch_ms",
        "elapsed_ms",
        "elapsed_label",
        "measurement_pending",
        "measurement_sufficient",
        "feedback_kind",
        "retry_allowed",
        "effect_verdict",
        "full_scale_client_burn_worsened",
        "rotate_fallback_recommended",
        "overlay_trial_recommended",
        "verified_host_compaction_observed_after_feedback",
        "compaction_count_delta",
        "primary_limit_used_percent_point_delta",
        "primary_limit_ideal_percent_point_delta",
        "primary_limit_used_overrun_percent_points",
        "turn_token_delta",
        "context_used_percent_point_delta",
        "regrowth_since_feedback_tokens",
    ] {
        if !effect[field].is_null() {
            compact.insert(field.to_string(), effect[field].clone());
        }
    }
    if effect["surface_exhausted_after_verified_failure"].as_bool() == Some(true) {
        compact.insert(
            "surface_exhausted_after_verified_failure".to_string(),
            json!(true),
        );
    }
    Value::Object(compact)
}

fn compact_client_budget_reply_gate_for_root_cause(guard: &Value) -> Value {
    let operator_flow = &guard["reply_execution_gate"]["action_bundle"]["operator_flow"];
    let mut compact_reply_execution_gate = serde_json::Map::from_iter([
        (
            "action_kind".to_string(),
            guard["reply_execution_gate"]["action_kind"].clone(),
        ),
        (
            "blocking".to_string(),
            guard["reply_execution_gate"]["blocking"].clone(),
        ),
        (
            "must_rotate_before_reply".to_string(),
            guard["reply_execution_gate"]["must_rotate_before_reply"].clone(),
        ),
        (
            "must_wait_for_budget_recovery_before_reply".to_string(),
            guard["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"].clone(),
        ),
        (
            "reply_budget_mode".to_string(),
            guard["reply_execution_gate"]["reply_budget_mode"].clone(),
        ),
        (
            "reply_prefix".to_string(),
            guard["reply_execution_gate"]["reply_prefix"].clone(),
        ),
        (
            "global_reply_prefix".to_string(),
            guard["global_reply_prefix"].clone(),
        ),
        (
            "reply_prefix_source".to_string(),
            guard["reply_prefix_source"].clone(),
        ),
    ]);
    compact_reply_execution_gate.extend(compact_reply_budget_pressure_hints(
        &guard["reply_execution_gate"],
    ));
    let mut compact_action_bundle = serde_json::Map::new();
    if operator_flow.is_object() {
        let mut compact_operator_flow = serde_json::Map::new();
        for field in [
            "primary_command_kind",
            "primary_command",
            "rotate_helper_command",
            "host_current_thread_control_launch_command",
        ] {
            if !operator_flow[field].is_null() {
                compact_operator_flow.insert(field.to_string(), operator_flow[field].clone());
            }
        }
        if !compact_operator_flow.is_empty() {
            compact_action_bundle.insert(
                "operator_flow".to_string(),
                Value::Object(compact_operator_flow),
            );
        }
    }
    if guard["reply_execution_gate"]["action_bundle"]["host_current_thread_control"].is_object() {
        compact_action_bundle.insert(
            "host_current_thread_control".to_string(),
            working_state::compact_host_current_thread_control_surface_for_runtime(
                &guard["reply_execution_gate"]["action_bundle"]["host_current_thread_control"],
            ),
        );
    }
    if !compact_action_bundle.is_empty() {
        compact_reply_execution_gate.insert(
            "action_bundle".to_string(),
            Value::Object(compact_action_bundle),
        );
    }
    json!({
        "observed_at_epoch_ms": guard["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": guard["max_guard_age_seconds"].clone(),
        "global_reply_prefix": guard["global_reply_prefix"].clone(),
        "reply_prefix_source": guard["reply_prefix_source"].clone(),
        "reply_execution_gate": Value::Object(compact_reply_execution_gate),
    })
}

fn compact_reply_budget_pressure_hints(
    reply_execution_gate: &Value,
) -> serde_json::Map<String, Value> {
    let contract = &reply_execution_gate["reply_budget_contract"];
    let mut compact = serde_json::Map::new();
    for field in [
        "must_confirm_same_thread_host_control_feedback_before_reply",
        "must_wait_for_same_thread_effect_measurement_before_reply",
        "host_context_compaction_inactive_target_pressure_active",
        "current_live_turn_no_amai_activity",
        "same_meter_pure_burn_turn_active",
        "must_prefer_short_paragraphs",
        "must_avoid_commentary_only_updates",
        "must_batch_all_tool_reads_before_reply",
        "must_wait_for_meaningful_result_before_progress_reply",
        "must_require_material_delta_before_next_reply",
        "must_avoid_progress_reply_when_only_guard_changed",
        "must_avoid_new_tool_turn_without_specific_delta_goal",
        "max_bullets_soft",
        "max_sentences_soft",
        "max_tool_roundtrips_soft",
    ] {
        let value = if !reply_execution_gate[field].is_null() {
            reply_execution_gate[field].clone()
        } else {
            contract[field].clone()
        };
        if !value.is_null() {
            compact.insert(field.to_string(), value);
        }
    }
    compact
}

pub(super) fn compact_client_budget_root_cause_payload(
    payload: &Value,
    guard: Option<&Value>,
) -> Value {
    let mut compact = serde_json::Map::new();
    if !payload["thread_binding_state"].is_null() {
        compact.insert(
            "thread_binding_state".to_string(),
            payload["thread_binding_state"].clone(),
        );
    }
    if payload["current_live_meter"].is_object() {
        compact.insert(
            "current_live_meter".to_string(),
            compact_current_live_meter_for_root_cause_cli(&payload["current_live_meter"]),
        );
    }
    if payload["current_live_turn"].is_object() {
        compact.insert(
            "current_live_turn".to_string(),
            compact_current_live_turn_for_root_cause_cli(&payload["current_live_turn"]),
        );
    }
    if payload["same_meter_economics"].is_object() {
        compact.insert(
            "same_meter_economics".to_string(),
            compact_same_meter_economics_for_root_cause_cli(&payload["same_meter_economics"]),
        );
    }
    let exact_pair_state = payload["exact_pair_status"]["state"].as_str();
    let current_live_turn_status = payload["current_live_turn"]["status"].as_str();
    let exact_pair_status_redundant_for_live_turn = matches!(
        (exact_pair_state, current_live_turn_status),
        (
            Some("not_applicable_current_live_turn_has_no_amai_activity"),
            Some("no_amai_activity_in_current_live_turn")
        )
    );
    if !payload["exact_pair_status"].is_null() && !exact_pair_status_redundant_for_live_turn {
        compact.insert(
            "exact_pair_status".to_string(),
            payload["exact_pair_status"].clone(),
        );
    }
    if payload["host_context_compaction"].is_object() {
        compact.insert(
            "host_context_compaction".to_string(),
            compact_host_context_compaction_for_root_cause_cli(&payload["host_context_compaction"]),
        );
    }
    if let Some(exact_pair_status) = compact.get_mut("exact_pair_status")
        && exact_pair_status.is_object()
    {
        *exact_pair_status = json!({
            "state": exact_pair_status["state"].clone()
        });
    }
    if payload["host_current_thread_control_effect"].is_object() {
        compact.insert(
            "host_current_thread_control_effect".to_string(),
            compact_host_current_thread_control_effect_for_cli(
                &payload["host_current_thread_control_effect"],
            ),
        );
    }
    if payload["guard"].is_object() {
        compact.insert(
            "guard".to_string(),
            compact_guard_for_root_cause_cli(&payload["guard"]),
        );
    }
    if let Some(guard) = guard {
        compact.insert(
            "client_budget_reply_gate".to_string(),
            compact_client_budget_reply_gate_for_root_cause(guard),
        );
    }
    for field in [
        "missing_components",
        "partially_measured_components",
        "blocking_reasons",
    ] {
        if payload[field]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            compact.insert(field.to_string(), payload[field].clone());
        }
    }
    Value::Object(compact)
}

pub(super) fn compact_cli_client_budget_gate_payload(guard: &Value) -> Value {
    let compact_gate = compact_client_budget_gate_payload(guard);
    let mut compact_action_bundle = serde_json::Map::new();
    for field in [
        "measurement_before_retry_required",
        "feedback_confirmation_before_retry_required",
    ] {
        if !compact_gate["reply_execution_gate"]["action_bundle"][field].is_null() {
            compact_action_bundle.insert(
                field.to_string(),
                compact_gate["reply_execution_gate"]["action_bundle"][field].clone(),
            );
        }
    }
    if compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"].is_object() {
        let mut operator_flow =
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]
                .as_object()
                .cloned()
                .unwrap_or_default();
        operator_flow.remove("startup_command");
        if !operator_flow.is_empty() {
            compact_action_bundle.insert("operator_flow".to_string(), Value::Object(operator_flow));
        }
    }
    if compact_gate["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
        .is_object()
    {
        compact_action_bundle.insert(
            "host_current_thread_control".to_string(),
            working_state::compact_host_current_thread_control_surface_for_runtime(
                &compact_gate["reply_execution_gate"]["action_bundle"]
                    ["host_current_thread_control"],
            ),
        );
    }
    let mut compact_reply_execution_gate = serde_json::Map::from_iter([
        (
            "action_kind".to_string(),
            compact_gate["reply_execution_gate"]["action_kind"].clone(),
        ),
        (
            "blocking".to_string(),
            compact_gate["reply_execution_gate"]["blocking"].clone(),
        ),
        (
            "must_rotate_before_reply".to_string(),
            compact_gate["reply_execution_gate"]["must_rotate_before_reply"].clone(),
        ),
        (
            "must_wait_for_budget_recovery_before_reply".to_string(),
            compact_gate["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"]
                .clone(),
        ),
        (
            "reply_budget_mode".to_string(),
            compact_gate["reply_execution_gate"]["reply_budget_mode"].clone(),
        ),
        (
            "reply_prefix".to_string(),
            compact_gate["reply_execution_gate"]["reply_prefix"].clone(),
        ),
        (
            "global_reply_prefix".to_string(),
            compact_gate["reply_execution_gate"]["global_reply_prefix"].clone(),
        ),
        (
            "reply_prefix_source".to_string(),
            compact_gate["reply_execution_gate"]["reply_prefix_source"].clone(),
        ),
        (
            "host_context_compaction_stage".to_string(),
            compact_gate["reply_execution_gate"]["host_context_compaction_stage"].clone(),
        ),
        (
            "host_context_compaction_preserve_active".to_string(),
            compact_gate["reply_execution_gate"]["host_context_compaction_preserve_active"]
                .clone(),
        ),
        (
            "host_context_compaction_critical_regrowth_active".to_string(),
            compact_gate["reply_execution_gate"]
                ["host_context_compaction_critical_regrowth_active"]
                .clone(),
        ),
        (
            "preserves_return_obligation".to_string(),
            compact_gate["reply_execution_gate"]["preserves_return_obligation"].clone(),
        ),
        ("action_bundle".to_string(), Value::Object(compact_action_bundle)),
    ]);
    for field in [
        "host_context_compaction_inactive_target_pressure_active",
        "current_live_turn_no_amai_activity",
        "same_meter_pure_burn_turn_active",
        "must_prefer_short_paragraphs",
        "must_avoid_commentary_only_updates",
        "must_batch_all_tool_reads_before_reply",
        "must_wait_for_meaningful_result_before_progress_reply",
        "must_require_material_delta_before_next_reply",
        "must_avoid_progress_reply_when_only_guard_changed",
        "must_avoid_new_tool_turn_without_specific_delta_goal",
        "max_bullets_soft",
        "max_sentences_soft",
        "max_tool_roundtrips_soft",
    ] {
        if !compact_gate["reply_execution_gate"][field].is_null() {
            compact_reply_execution_gate.insert(
                field.to_string(),
                compact_gate["reply_execution_gate"][field].clone(),
            );
        }
    }
    let mut compact = serde_json::Map::from_iter([
        (
            "status_label".to_string(),
            compact_gate["status_label"].clone(),
        ),
        (
            "observed_at_epoch_ms".to_string(),
            compact_gate["observed_at_epoch_ms"].clone(),
        ),
        (
            "max_guard_age_seconds".to_string(),
            compact_gate["max_guard_age_seconds"].clone(),
        ),
        (
            "reply_execution_gate".to_string(),
            Value::Object(compact_reply_execution_gate),
        ),
    ]);
    for field in [
        "delivery_surface_status_label",
        "reply_prefix",
        "global_reply_prefix",
        "reply_prefix_source",
    ] {
        if !compact_gate[field].is_null() {
            compact.insert(field.to_string(), compact_gate[field].clone());
        }
    }
    Value::Object(compact)
}

pub(super) fn front_door_client_budget_gate_payload(gate: Value) -> Value {
    json!({
        "reply_prefix": gate["reply_execution_gate"]["reply_prefix"].clone(),
        "global_reply_prefix": gate["reply_execution_gate"]["global_reply_prefix"].clone(),
        "reply_prefix_source": gate["reply_execution_gate"]["reply_prefix_source"].clone(),
        "status_label": gate["status_label"].clone(),
        "observed_at_epoch_ms": gate["observed_at_epoch_ms"].clone(),
        "max_guard_age_seconds": gate["max_guard_age_seconds"].clone(),
        "client_budget_reply_gate": gate,
    })
}

pub(super) fn normalize_front_door_client_budget_gate_payload_shape(payload: Value) -> Value {
    if payload["reply_prefix"].is_null() && payload["client_budget_reply_gate"].is_object() {
        return front_door_client_budget_gate_payload(payload["client_budget_reply_gate"].clone());
    }
    payload
}

fn same_thread_host_control_auto_launch_args_from_gate(
    repo_root: &Path,
    thread_id: &str,
    payload: &Value,
) -> Option<ObserveClientBudgetHostControlLaunchArgs> {
    let reply_execution_gate = &payload["client_budget_reply_gate"]["reply_execution_gate"];
    let host_current_thread_control =
        &reply_execution_gate["action_bundle"]["host_current_thread_control"];
    if reply_execution_gate["action_kind"].as_str()
        != Some("compact_current_thread_for_client_budget")
        || reply_execution_gate["same_meter_pure_burn_turn_active"].as_bool() != Some(true)
        || reply_execution_gate["must_avoid_new_tool_turn_without_specific_delta_goal"].as_bool()
            != Some(true)
        || reply_execution_gate["max_tool_roundtrips_soft"].as_i64() != Some(0)
        || host_current_thread_control["automation_ready"].as_bool() != Some(true)
        || host_current_thread_control["retry_allowed"].as_bool() != Some(true)
        || host_current_thread_control["measurement_pending"].as_bool() == Some(true)
        || host_current_thread_control["feedback_pending"].as_bool() == Some(true)
    {
        return None;
    }
    let command_id = host_current_thread_control["command_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let surface_thread_id = host_current_thread_control["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(thread_id);
    if surface_thread_id != thread_id {
        return None;
    }
    Some(ObserveClientBudgetHostControlLaunchArgs {
        thread_id: thread_id.to_string(),
        compact_window: command_id == working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID,
        command_id: if command_id == working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID {
            None
        } else {
            Some(command_id.to_string())
        },
        project: None,
        repo_root: Some(repo_root.to_path_buf()),
        namespace: default_continuity_namespace(),
    })
}

pub(super) async fn maybe_auto_launch_same_thread_host_control_from_gate(
    cfg: &AppConfig,
    repo_root: &Path,
    thread_id: &str,
    payload: &Value,
) -> Result<Option<Value>> {
    let Some(args) =
        same_thread_host_control_auto_launch_args_from_gate(repo_root, thread_id, payload)
    else {
        return Ok(None);
    };
    let launch_payload = client_budget_host_control_launch_payload(cfg, &args).await?;
    Ok(Some(front_door_client_budget_gate_payload(
        launch_payload["client_budget_host_control_launch"]["client_budget_reply_gate"].clone(),
    )))
}

pub(super) fn compact_cli_client_budget_gate_from_root_cause_payload(
    payload: &Value,
) -> Option<Value> {
    let gate = payload.get("client_budget_reply_gate")?.clone();
    if gate["reply_execution_gate"]["action_kind"].is_null() {
        return None;
    }
    Some(front_door_client_budget_gate_payload(gate))
}

pub(super) fn compact_host_control_client_budget_reply_gate(guard: &Value) -> Value {
    let compact_gate = compact_client_budget_gate_payload(guard);
    let host_context_compaction =
        compact_host_context_compaction_for_cli(&compact_gate["host_context_compaction"]);
    let compact_operator_flow = json!({
        "primary_command_kind":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"].clone(),
        "same_thread_effect_measurement_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_effect_measurement_required"].clone(),
        "same_thread_effect_measurement_summary":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_effect_measurement_summary"].clone(),
        "same_thread_feedback_confirmation_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"].clone(),
        "same_thread_feedback_confirmation_summary":
            compact_gate["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_summary"].clone(),
    });
    let compact_action_bundle = json!({
        "bundle_version":
            compact_gate["reply_execution_gate"]["action_bundle"]["bundle_version"].clone(),
        "ready_for_automation":
            compact_gate["reply_execution_gate"]["action_bundle"]["ready_for_automation"].clone(),
        "preserves_return_obligation":
            compact_gate["reply_execution_gate"]["action_bundle"]["preserves_return_obligation"].clone(),
        "measurement_before_retry_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["measurement_before_retry_required"].clone(),
        "feedback_confirmation_before_retry_required":
            compact_gate["reply_execution_gate"]["action_bundle"]["feedback_confirmation_before_retry_required"].clone(),
        "order": compact_gate["reply_execution_gate"]["action_bundle"]["order"].clone(),
        "host_current_thread_control": working_state::compact_host_current_thread_control_surface_for_runtime(
            &compact_gate["reply_execution_gate"]["action_bundle"]["host_current_thread_control"],
        ),
        "operator_flow": compact_operator_flow,
    });
    json!({
        "status_label": compact_gate["status_label"].clone(),
        "reply_prefix": compact_gate["reply_prefix"].clone(),
        "global_reply_prefix": compact_gate["global_reply_prefix"].clone(),
        "reply_prefix_source": compact_gate["reply_prefix_source"].clone(),
        "host_context_compaction": host_context_compaction,
        "reply_execution_gate": {
            "action_kind": compact_gate["reply_execution_gate"]["action_kind"].clone(),
            "blocking": compact_gate["reply_execution_gate"]["blocking"].clone(),
            "must_rotate_before_reply":
                compact_gate["reply_execution_gate"]["must_rotate_before_reply"].clone(),
            "must_wait_for_budget_recovery_before_reply":
                compact_gate["reply_execution_gate"]["must_wait_for_budget_recovery_before_reply"].clone(),
            "reply_budget_mode": compact_gate["reply_execution_gate"]["reply_budget_mode"].clone(),
            "reply_prefix": compact_gate["reply_execution_gate"]["reply_prefix"].clone(),
            "global_reply_prefix":
                compact_gate["reply_execution_gate"]["global_reply_prefix"].clone(),
            "reply_prefix_source":
                compact_gate["reply_execution_gate"]["reply_prefix_source"].clone(),
            "host_context_compaction_stage":
                compact_gate["reply_execution_gate"]["host_context_compaction_stage"].clone(),
            "host_context_compaction_preserve_active":
                compact_gate["reply_execution_gate"]["host_context_compaction_preserve_active"].clone(),
            "host_context_compaction_critical_regrowth_active":
                compact_gate["reply_execution_gate"]["host_context_compaction_critical_regrowth_active"].clone(),
            "preserves_return_obligation":
                compact_gate["reply_execution_gate"]["preserves_return_obligation"].clone(),
            "action_bundle": compact_action_bundle,
        },
    })
}

fn compact_reply_execution_gate(reply_execution_gate: &Value) -> Value {
    let preserves_return_obligation = reply_execution_gate["preserves_return_obligation"]
        .as_bool()
        .map(Value::from)
        .unwrap_or_else(|| {
            reply_execution_gate["action_bundle"]["preserves_return_obligation"].clone()
        });
    let action_bundle =
        compact_reply_execution_action_bundle(&reply_execution_gate["action_bundle"]);
    let mut compact = serde_json::Map::from_iter([
        (
            "action_kind".to_string(),
            reply_execution_gate["action_kind"].clone(),
        ),
        (
            "blocking".to_string(),
            reply_execution_gate["blocking"].clone(),
        ),
        (
            "must_rotate_before_reply".to_string(),
            reply_execution_gate["must_rotate_before_reply"].clone(),
        ),
        (
            "must_wait_for_budget_recovery_before_reply".to_string(),
            reply_execution_gate["must_wait_for_budget_recovery_before_reply"].clone(),
        ),
        (
            "reply_budget_mode".to_string(),
            reply_execution_gate["reply_budget_mode"].clone(),
        ),
        (
            "reply_prefix".to_string(),
            reply_execution_gate["reply_prefix"].clone(),
        ),
        (
            "global_reply_prefix".to_string(),
            reply_execution_gate["global_reply_prefix"].clone(),
        ),
        (
            "reply_prefix_source".to_string(),
            reply_execution_gate["reply_prefix_source"].clone(),
        ),
        (
            "delivery_surface_status_label".to_string(),
            reply_execution_gate["delivery_surface_status_label"].clone(),
        ),
        (
            "host_context_compaction_stage".to_string(),
            reply_execution_gate["host_context_compaction_stage"].clone(),
        ),
        (
            "host_context_compaction_preserve_active".to_string(),
            reply_execution_gate["host_context_compaction_preserve_active"].clone(),
        ),
        (
            "host_context_compaction_critical_regrowth_active".to_string(),
            reply_execution_gate["host_context_compaction_critical_regrowth_active"].clone(),
        ),
        (
            "preserves_return_obligation".to_string(),
            preserves_return_obligation,
        ),
        (
            "delivery_surface_requires_continuity_startup".to_string(),
            reply_execution_gate["delivery_surface_requires_continuity_startup"].clone(),
        ),
        ("action_bundle".to_string(), action_bundle),
    ]);
    compact.extend(compact_reply_budget_pressure_hints(reply_execution_gate));
    Value::Object(compact)
}

fn normalize_compact_cli_command(command: &str) -> String {
    command
        .replace('\'', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_reply_execution_action_bundle(action_bundle: &Value) -> Value {
    let Some(bundle) = action_bundle.as_object() else {
        return Value::Null;
    };
    let mut compact = serde_json::Map::new();
    for field in [
        "bundle_version",
        "ready_for_automation",
        "preserves_return_obligation",
        "measurement_before_retry_required",
        "feedback_confirmation_before_retry_required",
        "order",
        "delivery_surface_order",
    ] {
        if !bundle.get(field).unwrap_or(&Value::Null).is_null() {
            compact.insert(field.to_string(), action_bundle[field].clone());
        }
    }
    if action_bundle["host_current_thread_control"].is_object() {
        compact.insert(
            "host_current_thread_control".to_string(),
            action_bundle["host_current_thread_control"].clone(),
        );
    }
    if action_bundle["operator_flow"].is_object() {
        let mut operator_flow = serde_json::Map::new();
        for field in [
            "primary_command_kind",
            "same_thread_effect_measurement_required",
            "same_thread_effect_measurement_summary",
            "same_thread_feedback_confirmation_required",
            "same_thread_feedback_confirmation_summary",
        ] {
            if !action_bundle["operator_flow"][field].is_null() {
                operator_flow.insert(
                    field.to_string(),
                    action_bundle["operator_flow"][field].clone(),
                );
            }
        }
        for field in [
            "primary_command",
            "host_current_thread_control_launch_command",
            "rotate_helper_command",
            "startup_command",
            "startup_after_recovery_command",
        ] {
            if let Some(command) = action_bundle["operator_flow"][field]
                .as_str()
                .map(normalize_compact_cli_command)
                .filter(|value| !value.is_empty())
            {
                operator_flow.insert(field.to_string(), Value::from(command));
            }
        }
        if operator_flow
            .get("primary_command_kind")
            .and_then(Value::as_str)
            == Some("rotate_helper_command")
            && operator_flow.get("primary_command") == operator_flow.get("rotate_helper_command")
        {
            operator_flow.remove("primary_command");
        }
        if operator_flow
            .get("primary_command_kind")
            .and_then(Value::as_str)
            == Some("same_thread_host_control_launch_command")
            && operator_flow.get("primary_command")
                == operator_flow.get("host_current_thread_control_launch_command")
        {
            operator_flow.remove("host_current_thread_control_launch_command");
        }
        if !operator_flow.is_empty() {
            compact.insert("operator_flow".to_string(), Value::Object(operator_flow));
        }
    }
    if compact.contains_key("operator_flow") {
        Value::Object(compact)
    } else {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compact_latest_repo_working_state_restore_from_optional_payload,
        compact_working_state_restore_for_budget,
    };
    use serde_json::json;

    #[test]
    fn compact_working_state_restore_for_budget_preserves_host_feedback_write_status() {
        let restore = json!({
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "summary": "feedback summary",
                    "recorded_at_epoch_ms": 123,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "opened",
                        "command_id": "compact_window",
                        "working_state_write_status": {
                            "status": "degraded_after_primary_write",
                            "warning": "Refresh degraded after primary write."
                        },
                        "feedback_snapshot": {
                            "thread_id": "thread-1",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100,
                                "context_used_percent": 25.0,
                                "primary_limit_used_percent": 10
                            },
                            "host_context_compaction": {
                                "compaction_count": 1,
                                "growth_since_compaction_tokens": 20,
                                "compacted_at_epoch_ms": 111,
                                "stage": "verified"
                            }
                        }
                    }
                }
            ]
        });

        let compact = compact_working_state_restore_for_budget(&restore);
        assert_eq!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]
                ["working_state_write_status"]["status"],
            "degraded_after_primary_write"
        );
        assert_eq!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]
                ["working_state_write_status"]["warning"],
            "Refresh degraded after primary write."
        );
    }

    #[test]
    fn compact_working_state_restore_for_budget_preserves_missing_host_feedback_write_status_as_null(
    ) {
        let restore = json!({
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "summary": "feedback summary",
                    "recorded_at_epoch_ms": 123,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "opened",
                        "command_id": "compact_window",
                        "feedback_snapshot": {
                            "thread_id": "thread-1",
                            "client_live_meter": {},
                            "host_context_compaction": {}
                        }
                    }
                }
            ]
        });

        let compact = compact_working_state_restore_for_budget(&restore);
        assert!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]
                ["working_state_write_status"]
                .is_null()
        );
    }

    #[test]
    fn compact_latest_repo_working_state_restore_from_optional_payload_preserves_override_payload() {
        let payload = json!({
            "working_state_restore": {
                "current_goal": "override goal",
                "recent_actions": [
                    {
                        "source_kind": "host_current_thread_control_feedback",
                        "summary": "feedback summary",
                        "recorded_at_epoch_ms": 123,
                        "host_current_thread_control_feedback": {
                            "feedback_kind": "opened",
                            "command_id": "compact_window",
                            "working_state_write_status": {
                                "status": "degraded_after_primary_write",
                                "warning": "Refresh degraded after primary write."
                            },
                            "feedback_snapshot": {
                                "thread_id": "thread-1",
                                "client_live_meter": {},
                                "host_context_compaction": {}
                            }
                        }
                    }
                ]
            }
        });

        let compact =
            compact_latest_repo_working_state_restore_from_optional_payload(Some(&payload));
        assert_eq!(
            compact["working_state_restore"]["current_goal"],
            "override goal"
        );
        assert_eq!(
            compact["working_state_restore"]["recent_actions"][0]
                ["host_current_thread_control_feedback"]["working_state_write_status"]["status"],
            "degraded_after_primary_write"
        );
    }

    #[test]
    fn compact_latest_repo_working_state_restore_from_optional_payload_preserves_missing_payload_as_empty_restore(
    ) {
        let compact = compact_latest_repo_working_state_restore_from_optional_payload(None);
        assert_eq!(compact, json!({ "working_state_restore": {} }));
    }
}
