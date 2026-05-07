pub async fn build_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<()> {
    let prepared = execute_context_pack_capture_with_options(cfg, db, args, true, true).await?;
    let context_pack_id = prepared
        .payload
        .get("context_pack_id")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .unwrap_or_else(|| prepared.stats.context_pack_id.to_string());
    let caller_payload =
        materialize_context_pack_caller_payload(db, args, &prepared.payload, &prepared.stats)
            .await?;
    let compact_output =
        cli_visible_context_pack_payload(&caller_payload, args.token_source_kind.as_str());
    let compact_output_json = serde_json::to_string(&compact_output)?;
    let _ = token_budget::observe_cli_context_pack_tool_overhead(
        db,
        &context_pack_id,
        compact_output_json.as_str(),
    )
    .await?;
    if prepared.stats.cache_hit {
        eprintln!(
            "context pack cache hit: {} :: scope={}",
            context_pack_id, prepared.stats.scope_signature
        );
    } else {
        let artifact_location = match (
            prepared
                .payload
                .get("artifact_bucket")
                .and_then(Value::as_str),
            prepared
                .payload
                .get("artifact_object_key")
                .and_then(Value::as_str),
        ) {
            (Some(bucket), Some(object_key)) => format!("s3://{bucket}/{object_key}"),
            _ => "artifact://unavailable".to_string(),
        };
        let artifact_state = prepared
            .payload
            .get("artifact_state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        eprintln!(
            "context pack truth persisted: postgres :: artifact_state={} :: {} :: {}",
            artifact_state, artifact_location, context_pack_id
        );
    }
    println!("{compact_output_json}");
    Ok(())
}

fn cli_visible_context_pack_payload(payload: &Value, token_source_kind: &str) -> Value {
    let mut compact = model_visible_context_pack_payload(payload);
    if token_source_kind == "proof_context_pack" && payload["decision_trace"].is_object() {
        compact["decision_trace"] = payload["decision_trace"].clone();
    }
    compact
}

pub async fn execute_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<ContextPackStats> {
    Ok(
        execute_context_pack_capture_with_options(cfg, db, args, persist, true)
            .await?
            .stats,
    )
}

pub async fn execute_context_pack_capture(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<ContextPackResult> {
    execute_context_pack_capture_with_options(cfg, db, args, persist, true).await
}

pub async fn execute_context_pack_with_options(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
    track_token_usage: bool,
) -> Result<ContextPackStats> {
    if !track_token_usage
        && let Some(stats) = try_execute_context_pack_stats_cached(cfg, db, args, persist).await?
    {
        return Ok(stats);
    }
    Ok(
        execute_context_pack_capture_with_options(cfg, db, args, persist, track_token_usage)
            .await?
            .stats,
    )
}

pub async fn execute_context_pack_capture_with_options(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
    track_token_usage: bool,
) -> Result<ContextPackResult> {
    if let Some(cached) = try_execute_context_pack_fast_cached(cfg, args, persist)? {
        let mut payload =
            materialize_context_pack_caller_payload(db, args, &cached.payload, &cached.stats)
                .await?;
        if track_token_usage {
            record_context_pack_token_budget_event(db, &mut payload, args).await?;
        }
        return Ok(ContextPackResult {
            payload,
            stats: cached.stats,
        });
    }
    let mut prepared = prepare_context_pack(cfg, db, args).await?;
    if persist {
        ensure_context_pack_persisted(cfg, db, args, &mut prepared).await?;
    }
    let durably_persisted = persist || prepared.durably_persisted;
    cache_context_pack_entry(cfg, args, &prepared, durably_persisted)?;
    let mut payload = materialize_context_pack_caller_payload(
        db,
        args,
        prepared.payload.as_ref(),
        &prepared.stats,
    )
    .await?;
    if track_token_usage {
        record_context_pack_token_budget_event(db, &mut payload, args).await?;
    }
    Ok(ContextPackResult {
        payload,
        stats: prepared.stats,
    })
}

async fn record_context_pack_token_budget_event(
    db: &Client,
    payload: &mut Value,
    args: &ContextPackArgs,
) -> Result<()> {
    if !payload.is_object() {
        return Ok(());
    }
    let token_budget_payload = with_whole_cycle_observed_overrides(payload, args);
    token_budget::record_context_pack_event(db, &token_budget_payload, &args.token_source_kind)
        .await?;
    let skip_working_state_live_repeat = args.token_source_kind == "live_context_pack"
        && payload["retrieval_runtime"]["cache_hit"].as_bool() == Some(true);
    if !skip_working_state_live_repeat {
        let write_status =
            working_state::record_context_pack_event(db, payload, &args.token_source_kind).await?;
        if let Some(root) = payload.as_object_mut() {
            root.insert(
                "working_state_write_status".to_string(),
                serde_json::to_value(&write_status)?,
            );
        }
    }
    remember_thread_context_pack_delivery(payload)?;
    Ok(())
}

async fn materialize_context_pack_caller_payload(
    db: &Client,
    args: &ContextPackArgs,
    payload: &Value,
    stats: &ContextPackStats,
) -> Result<Value> {
    let payload = if let Some(compact) =
        same_thread_cache_reuse_payload_if_needed(db, payload, stats, args).await?
    {
        compact
    } else {
        payload.clone()
    };
    Ok(with_whole_cycle_observed_overrides(&payload, args))
}

async fn same_thread_cache_reuse_payload_if_needed(
    db: &Client,
    payload: &Value,
    stats: &ContextPackStats,
    args: &ContextPackArgs,
) -> Result<Option<Value>> {
    if !should_emit_same_thread_cache_reuse_payload(stats, args) {
        return Ok(None);
    }
    let Some(thread_id) = codex_threads::current_thread_id() else {
        return Ok(None);
    };
    let Some(context_pack_id) = payload["context_pack_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if !same_thread_context_pack_previously_delivered(db, &thread_id, context_pack_id).await? {
        return Ok(None);
    }
    Ok(Some(build_same_thread_cache_reuse_payload(
        payload, &thread_id,
    )))
}

fn should_emit_same_thread_cache_reuse_payload(
    stats: &ContextPackStats,
    args: &ContextPackArgs,
) -> bool {
    stats.cache_hit && args.token_source_kind != "proof_context_pack"
}

async fn same_thread_context_pack_previously_delivered(
    _db: &Client,
    thread_id: &str,
    context_pack_id: &str,
) -> Result<bool> {
    thread_context_pack_delivery_seen(thread_id, context_pack_id)
}

#[cfg(test)]
mod context_pack_api_contract_tests {
    use super::*;

    #[test]
    fn context_pack_payload_preserves_working_state_write_status_marker() {
        let payload = json!({
            "context_pack_id": "ctx-pack-1",
            "query": "restore pack degraded path",
            "working_state_write_status": {
                "status": "degraded_after_primary_write",
                "primary_write_persisted": true,
                "restore_refresh_status": "degraded",
                "warning": "context_pack.refresh_restore_snapshot degraded"
            }
        });

        let visible = cli_visible_context_pack_payload(&payload, "live_context_pack");
        assert_eq!(
            visible["working_state_write_status"]["status"],
            json!("degraded_after_primary_write")
        );
        assert_eq!(
            visible["working_state_write_status"]["warning"],
            json!("context_pack.refresh_restore_snapshot degraded")
        );
    }
}
