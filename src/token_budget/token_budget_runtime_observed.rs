use super::*;

pub(super) fn compact_token_budget_retrieval_runtime(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "cache_hit": value["cache_hit"].clone(),
        "scope_signature": value["scope_signature"].clone(),
        "resolve_scope_ms": value["resolve_scope_ms"].clone(),
        "cache_lookup_ms": value["cache_lookup_ms"].clone(),
        "exact_lookup_ms": value["exact_lookup_ms"].clone(),
        "symbol_lookup_ms": value["symbol_lookup_ms"].clone(),
        "lexical_lookup_ms": value["lexical_lookup_ms"].clone(),
        "query_embed_ms": value["query_embed_ms"].clone(),
        "semantic_search_ms": value["semantic_search_ms"].clone(),
        "semantic_hydrate_ms": value["semantic_hydrate_ms"].clone(),
        "retrieval_lower_bound_ms": value["retrieval_lower_bound_ms"].clone(),
        "total_ms": value["total_ms"].clone(),
    })
}

pub(super) fn observed_tool_overhead_payload(text: &str, structured_content: &Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": text
        }],
        "structuredContent": structured_content
    })
}

pub(crate) fn count_tool_overhead_tokens(
    measurement: &MeasurementConfig,
    text: &str,
    structured_content: &Value,
) -> Result<u64> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let payload = observed_tool_overhead_payload(text, structured_content);
    let rendered =
        serde_json::to_string(&payload).context("failed to serialize tool overhead payload")?;
    Ok(tokenizer.encode_with_special_tokens(&rendered).len() as u64)
}

pub(crate) fn count_cli_context_pack_output_overhead_tokens(
    measurement: &MeasurementConfig,
    output_json: &str,
    delivered_tokens: u64,
) -> Result<u64> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let total_output_tokens = tokenizer.encode_with_special_tokens(output_json).len() as u64;
    Ok(total_output_tokens.saturating_sub(delivered_tokens))
}

pub(super) fn cli_context_pack_tool_overhead_source_status() -> Value {
    json!({
        "state": "context_pack_cli_model_visible_output_materialized",
        "source_kind": "context_pack_cli_output_v2",
        "tool_overhead_contract_version": CLI_CONTEXT_PACK_TOOL_OVERHEAD_CONTRACT_VERSION,
        "legacy_replace_contract_version": CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION,
        "payload_surface": "model_visible_compact_output",
        "resolution_condition": "already_materialized",
        "note": "Tool-overhead tokens were counted from the compact model-visible CLI JSON that is actually printed to the caller."
    })
}

pub(crate) fn mcp_context_pack_tool_overhead_source_status() -> Value {
    json!({
        "state": "context_pack_mcp_structured_content_materialized",
        "source_kind": "context_pack_mcp_structured_content_v2",
        "tool_overhead_contract_version": MCP_CONTEXT_PACK_TOOL_OVERHEAD_CONTRACT_VERSION,
        "payload_surface": "mcp_tool_result_text_plus_structured_content",
        "resolution_condition": "already_materialized",
        "note": "Tool-overhead tokens were counted from the MCP tool result that is actually surfaced to the model: summary text plus structuredContent."
    })
}

pub(super) async fn legacy_context_pack_tool_overhead_replacement(
    db: &Client,
    context_pack_id: &str,
    measurement: &MeasurementConfig,
    row: &ObservabilitySnapshotRecord,
    delivered_tokens: u64,
    new_tool_overhead_tokens: u64,
) -> Result<Option<u64>> {
    let existing_tool_overhead =
        row.payload["token_budget_event"]["whole_cycle_observed"]["tool_overhead_tokens"].as_u64();
    let existing_source =
        row.payload["token_budget_event"]["whole_cycle_observed_source"]["tool_overhead"]
            .as_object()
            .map(|_| {
                row.payload["token_budget_event"]["whole_cycle_observed_source"]["tool_overhead"]
                    .clone()
            });
    let Some(existing_tool_overhead) = existing_tool_overhead else {
        return Ok(None);
    };
    if existing_tool_overhead == new_tool_overhead_tokens {
        return Ok(None);
    }
    if !existing_source
        .as_ref()
        .is_none_or(|value| is_replaceable_legacy_cli_tool_overhead_source(value))
    {
        return Ok(None);
    }
    if existing_source
        .as_ref()
        .is_some_and(|value| legacy_cli_tool_overhead_source_allows_direct_replacement(value))
    {
        return Ok(Some(existing_tool_overhead));
    }
    let Some(legacy_output_json) = stored_context_pack_payload_json(db, context_pack_id).await?
    else {
        return Ok(None);
    };
    let legacy_raw_output_tool_overhead = count_cli_context_pack_output_overhead_tokens(
        measurement,
        &legacy_output_json,
        delivered_tokens,
    )?;
    if legacy_raw_output_tool_overhead == existing_tool_overhead {
        Ok(Some(existing_tool_overhead))
    } else {
        Ok(None)
    }
}

pub(super) fn is_replaceable_legacy_cli_tool_overhead_source(source_status: &Value) -> bool {
    let contract_version = source_status["tool_overhead_contract_version"].as_str();
    if contract_version == Some(CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION) {
        return true;
    }
    if contract_version.is_some() {
        return false;
    }
    matches!(
        source_status["state"].as_str(),
        Some("context_pack_payload_materialized" | "secondary_context_pack_match_materialized")
    )
}

pub(super) fn legacy_cli_tool_overhead_source_allows_direct_replacement(
    source_status: &Value,
) -> bool {
    source_status["tool_overhead_contract_version"].as_str()
        == Some(CLI_CONTEXT_PACK_TOOL_OVERHEAD_LEGACY_CONTRACT_VERSION)
}

pub(super) async fn latest_token_budget_snapshot_for_context_pack(
    db: &Client,
    context_pack_id: &str,
) -> Result<Option<ObservabilitySnapshotRecord>> {
    let mut rows = latest_token_budget_snapshots_for_context_packs(
        db,
        &std::iter::once(context_pack_id.to_string()).collect(),
    )
    .await?;
    Ok(rows.remove(context_pack_id))
}

pub(crate) async fn latest_token_budget_snapshots_for_context_packs(
    db: &Client,
    context_pack_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, ObservabilitySnapshotRecord>> {
    if context_pack_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let target_ids = context_pack_ids.iter().cloned().collect::<Vec<_>>();
    let rows = postgres::list_latest_observability_snapshots_by_payload_string_field(
        db,
        "token_budget_event",
        "token_budget_event",
        "context_pack_id",
        &target_ids,
    )
    .await?;
    let mut latest = BTreeMap::<String, ObservabilitySnapshotRecord>::new();
    for row in rows {
        let Some(context_pack_id) = row.payload["token_budget_event"]["context_pack_id"]
            .as_str()
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        if !context_pack_ids.contains(&context_pack_id) {
            continue;
        }
        match latest.get(&context_pack_id) {
            Some(existing) if existing.created_at_epoch_ms >= row.created_at_epoch_ms => {}
            _ => {
                latest.insert(context_pack_id, row);
            }
        }
    }
    Ok(latest)
}

pub(super) async fn stored_context_pack_payload_json(
    db: &Client,
    context_pack_id: &str,
) -> Result<Option<String>> {
    let Ok(context_pack_uuid) = Uuid::parse_str(context_pack_id) else {
        return Ok(None);
    };
    let row = db
        .query_opt(
            "SELECT payload FROM ami.context_packs WHERE context_pack_id = $1 LIMIT 1",
            &[&context_pack_uuid],
        )
        .await
        .context("failed to load stored context pack payload")?;
    let Some(row) = row else {
        return Ok(None);
    };
    let payload: Value = row.get(0);
    Ok(Some(
        serde_json::to_string(&payload).context("failed to serialize stored context pack")?,
    ))
}

pub(super) fn tool_overhead_secondary_match_reference_epoch_ms(event: &TokenBudgetEvent) -> i64 {
    [
        event.occurred_at_epoch_ms as i64,
        event.ingested_at_epoch_ms as i64,
        event.created_at_epoch_ms as i64,
    ]
    .into_iter()
    .find(|value| *value > 0)
    .unwrap_or_default()
}

pub(super) fn select_secondary_context_pack_payload_match(
    mut candidates: Vec<SecondaryContextPackPayloadMatch>,
) -> SecondaryContextPackPayloadLookup {
    if candidates.is_empty() {
        return SecondaryContextPackPayloadLookup::NoCandidates;
    }
    candidates.sort_by(|left, right| {
        left.delta_ms
            .cmp(&right.delta_ms)
            .then_with(|| left.context_pack_id.cmp(&right.context_pack_id))
    });
    let nearest = candidates[0].clone();
    if nearest.delta_ms > TOOL_OVERHEAD_SECONDARY_CONTEXT_PACK_MATCH_MAX_DELTA_MS {
        return SecondaryContextPackPayloadLookup::NearestTooFar {
            delta_ms: nearest.delta_ms,
        };
    }
    let tied_count = candidates
        .iter()
        .take_while(|candidate| candidate.delta_ms == nearest.delta_ms)
        .count();
    if tied_count > 1 {
        return SecondaryContextPackPayloadLookup::AmbiguousNearest {
            delta_ms: nearest.delta_ms,
            candidate_count: tied_count,
        };
    }
    SecondaryContextPackPayloadLookup::Resolved(nearest)
}

pub(super) async fn stored_context_pack_payload_json_by_secondary_identity(
    db: &Client,
    event: &TokenBudgetEvent,
) -> Result<SecondaryContextPackPayloadLookup> {
    let query_text = event.query.trim();
    let project_code = event.project.trim();
    let namespace_code = event.namespace.trim();
    let retrieval_mode = event.retrieval_mode.as_deref().unwrap_or_default().trim();
    let event_epoch_ms = tool_overhead_secondary_match_reference_epoch_ms(event);
    if query_text.is_empty()
        || project_code.is_empty()
        || namespace_code.is_empty()
        || retrieval_mode.is_empty()
        || event_epoch_ms <= 0
    {
        return Ok(SecondaryContextPackPayloadLookup::NoCandidates);
    }
    let rows = db
        .query(
            r#"
            SELECT
                cp.context_pack_id::text,
                cp.payload,
                ABS((EXTRACT(EPOCH FROM cp.created_at) * 1000)::bigint - $5::bigint) AS delta_ms
            FROM ami.context_packs cp
            JOIN ami.projects p ON p.project_id = cp.project_id
            JOIN ami.namespaces n ON n.namespace_id = cp.namespace_id
            WHERE p.code = $1
              AND n.code = $2
              AND cp.retrieval_mode = $3
              AND cp.query_text = $4
            ORDER BY delta_ms ASC, cp.context_pack_id ASC
            LIMIT 8
            "#,
            &[
                &project_code,
                &namespace_code,
                &retrieval_mode,
                &query_text,
                &event_epoch_ms,
            ],
        )
        .await
        .context("failed to load secondary context pack payload candidates")?;
    let candidates = rows
        .into_iter()
        .map(|row| {
            let payload: Value = row.get(1);
            Ok(SecondaryContextPackPayloadMatch {
                context_pack_id: row.get::<_, String>(0),
                payload_json: serde_json::to_string(&payload)
                    .context("failed to serialize secondary stored context pack")?,
                delta_ms: row.get::<_, i64>(2),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(select_secondary_context_pack_payload_match(candidates))
}

pub(super) async fn attach_context_pack_whole_cycle_observed(
    db: &Client,
    context_pack_id: &str,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) -> Result<Option<Value>> {
    if context_pack_id.trim().is_empty() {
        bail!("context_pack_id must not be empty");
    }
    if client_prompt_tokens.is_none()
        && assistant_generation_tokens.is_none()
        && tool_overhead_tokens.is_none()
        && continuity_restore_tokens.is_none()
    {
        bail!("whole-cycle attach requires at least one observed token field");
    }
    let Some(row) = latest_token_budget_snapshot_for_context_pack(db, context_pack_id).await?
    else {
        return Ok(None);
    };
    attach_whole_cycle_observed_to_snapshot(
        db,
        &row,
        Some(json!({ "context_pack_id": context_pack_id })),
        client_prompt_tokens,
        assistant_generation_tokens,
        tool_overhead_tokens,
        continuity_restore_tokens,
    )
    .await
}

pub(super) async fn attach_whole_cycle_observed_to_snapshot(
    db: &Client,
    row: &ObservabilitySnapshotRecord,
    selector: Option<Value>,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) -> Result<Option<Value>> {
    let mut payload = row.payload.clone();
    let preserved_source_event_id = preserved_snapshot_source_event_id(db, row).await?;
    preserve_observability_source_event_id(&mut payload, preserved_source_event_id.as_deref())?;
    let (
        event_id,
        correlation_id,
        source_kind,
        traffic_class,
        measurement_scope,
        updated_fields,
        retained_fields,
        whole_cycle_observed,
        attached,
    ) = {
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event"))?;
        let mut updated_fields = Vec::new();
        let mut retained_fields = Vec::new();
        {
            let whole_cycle = ensure_nested_object(node, "whole_cycle_observed")?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "client_prompt_tokens",
                client_prompt_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "assistant_generation_tokens",
                assistant_generation_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "tool_overhead_tokens",
                tool_overhead_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
            apply_whole_cycle_observed_token(
                whole_cycle,
                "continuity_restore_tokens",
                continuity_restore_tokens,
                &mut updated_fields,
                &mut retained_fields,
            )?;
        }
        (
            node.get("event_id").cloned().unwrap_or(Value::Null),
            node.get("correlation_id").cloned().unwrap_or(Value::Null),
            node.get("source_kind").cloned().unwrap_or(Value::Null),
            node.get("traffic_class").cloned().unwrap_or(Value::Null),
            node.get("measurement_scope")
                .cloned()
                .unwrap_or(Value::Null),
            updated_fields.clone(),
            retained_fields.clone(),
            node.get("whole_cycle_observed")
                .cloned()
                .unwrap_or(Value::Null),
            !updated_fields.is_empty(),
        )
    };
    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload).await?;
    Ok(Some(json!({
        "whole_cycle_observed_attach": {
            "selector": selector.unwrap_or(Value::Null),
            "snapshot_id": row.snapshot_id,
            "event_id": event_id,
            "correlation_id": correlation_id,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "measurement_scope": measurement_scope,
            "updated_fields": updated_fields,
            "retained_fields": retained_fields,
            "whole_cycle_observed": whole_cycle_observed,
            "attached": attached,
            "note": "Conflicting overwrite is fail-closed; reattaching the same observed value is allowed."
        }
    })))
}

pub(crate) fn apply_tool_overhead_observed_and_source_status(
    node: &mut serde_json::Map<String, Value>,
    tool_overhead_tokens: u64,
    source_status: &Value,
    legacy_replace_from: Option<u64>,
) -> Result<(Vec<String>, Vec<String>, Value, Value, bool)> {
    let mut updated_fields = Vec::new();
    let mut retained_fields = Vec::new();
    {
        let whole_cycle = ensure_nested_object(node, "whole_cycle_observed")?;
        apply_tool_overhead_observed_token(
            whole_cycle,
            tool_overhead_tokens,
            legacy_replace_from,
            &mut updated_fields,
            &mut retained_fields,
        )?;
    }
    let source_updated = {
        let whole_cycle_source = ensure_nested_object(node, "whole_cycle_observed_source")?;
        let existing = whole_cycle_source.get("tool_overhead");
        let updated = existing != Some(source_status);
        if updated {
            whole_cycle_source.insert("tool_overhead".to_string(), source_status.clone());
        }
        updated
    };
    Ok((
        updated_fields.clone(),
        retained_fields,
        node.get("whole_cycle_observed")
            .cloned()
            .unwrap_or(Value::Null),
        node.get("whole_cycle_observed_source")
            .and_then(|value| {
                value["tool_overhead"]
                    .as_object()
                    .map(|_| value["tool_overhead"].clone())
            })
            .unwrap_or_else(|| {
                node.get("whole_cycle_observed_source")
                    .and_then(|value| value.get("tool_overhead"))
                    .cloned()
                    .unwrap_or(Value::Null)
            }),
        !updated_fields.is_empty() || source_updated,
    ))
}

pub(super) async fn attach_tool_overhead_observed_and_source_status_to_snapshot(
    db: &Client,
    row: &ObservabilitySnapshotRecord,
    selector: Option<Value>,
    tool_overhead_tokens: u64,
    source_status: Value,
    legacy_replace_from: Option<u64>,
) -> Result<Option<Value>> {
    let mut payload = row.payload.clone();
    let preserved_source_event_id = preserved_snapshot_source_event_id(db, row).await?;
    preserve_observability_source_event_id(&mut payload, preserved_source_event_id.as_deref())?;
    let (
        event_id,
        correlation_id,
        updated_fields,
        retained_fields,
        whole_cycle_observed,
        tool_overhead_source,
        attached,
    ) = {
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event"))?;
        let event_id = node.get("event_id").cloned().unwrap_or(Value::Null);
        let correlation_id = node.get("correlation_id").cloned().unwrap_or(Value::Null);
        let (updated_fields, retained_fields, whole_cycle_observed, tool_overhead_source, attached) =
            apply_tool_overhead_observed_and_source_status(
                node,
                tool_overhead_tokens,
                &source_status,
                legacy_replace_from,
            )?;
        (
            event_id,
            correlation_id,
            updated_fields,
            retained_fields,
            whole_cycle_observed,
            tool_overhead_source,
            attached,
        )
    };
    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload).await?;
    Ok(Some(json!({
        "tool_overhead_attach": {
            "selector": selector.unwrap_or(Value::Null),
            "snapshot_id": row.snapshot_id,
            "event_id": event_id,
            "correlation_id": correlation_id,
            "updated_fields": updated_fields,
            "retained_fields": retained_fields,
            "whole_cycle_observed": whole_cycle_observed,
            "tool_overhead_source": tool_overhead_source,
            "attached": attached,
            "note": "Tool-overhead whole-cycle tokens and source status are attached atomically so provenance updates cannot overwrite freshly materialized tool_overhead tokens."
        }
    })))
}

pub(super) async fn attach_tool_overhead_source_status_to_snapshot(
    db: &Client,
    row: &ObservabilitySnapshotRecord,
    selector: Option<Value>,
    source_status: Value,
) -> Result<Option<Value>> {
    let mut payload = row.payload.clone();
    let preserved_source_event_id = preserved_snapshot_source_event_id(db, row).await?;
    preserve_observability_source_event_id(&mut payload, preserved_source_event_id.as_deref())?;
    let (event_id, correlation_id, updated, tool_overhead_source) = {
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event"))?;
        let event_id = node.get("event_id").cloned().unwrap_or(Value::Null);
        let correlation_id = node.get("correlation_id").cloned().unwrap_or(Value::Null);
        let whole_cycle_source = ensure_nested_object(node, "whole_cycle_observed_source")?;
        let existing = whole_cycle_source.get("tool_overhead");
        let updated = existing != Some(&source_status);
        if updated {
            whole_cycle_source.insert("tool_overhead".to_string(), source_status.clone());
        }
        (
            event_id,
            correlation_id,
            updated,
            whole_cycle_source
                .get("tool_overhead")
                .cloned()
                .unwrap_or(Value::Null),
        )
    };
    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload).await?;
    Ok(Some(json!({
        "tool_overhead_source_attach": {
            "selector": selector.unwrap_or(Value::Null),
            "snapshot_id": row.snapshot_id,
            "event_id": event_id,
            "correlation_id": correlation_id,
            "tool_overhead_source": tool_overhead_source,
            "attached": updated,
            "note": "Tool-overhead source status is mutable provenance metadata: it may advance from missing-source to materialized-source if historical context-pack payloads are later recovered."
        }
    })))
}

pub(super) async fn preserved_snapshot_source_event_id(
    db: &Client,
    row: &ObservabilitySnapshotRecord,
) -> Result<Option<String>> {
    if let Some(source_event_id) = row.payload["_observability"]["source_event_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(source_event_id.to_string()));
    }
    let existing_event_key = db
        .query_opt(
            "SELECT event_key FROM ami.observability_snapshots WHERE snapshot_id = $1",
            &[&row.snapshot_id],
        )
        .await
        .context("failed to load observability snapshot event_key")?
        .map(|record| record.get::<_, String>(0));
    Ok(existing_event_key.filter(|value| value.starts_with("legacy:")))
}

pub(super) fn preserve_observability_source_event_id(
    payload: &mut Value,
    source_event_id: Option<&str>,
) -> Result<()> {
    let Some(source_event_id) = source_event_id.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let root = payload
        .as_object_mut()
        .ok_or_else(|| anyhow!("observability payload root must be an object"))?;
    let observability = root
        .entry("_observability".to_string())
        .or_insert_with(|| json!({}));
    let observability = observability
        .as_object_mut()
        .ok_or_else(|| anyhow!("observability metadata must be an object"))?;
    observability
        .entry("source_event_id".to_string())
        .or_insert_with(|| Value::String(source_event_id.to_string()));
    Ok(())
}

pub(super) fn apply_whole_cycle_observed_token(
    whole_cycle: &mut serde_json::Map<String, Value>,
    field: &str,
    new_value: Option<u64>,
    updated_fields: &mut Vec<String>,
    retained_fields: &mut Vec<String>,
) -> Result<()> {
    let Some(new_value) = new_value else {
        return Ok(());
    };
    match whole_cycle.get(field).and_then(Value::as_u64) {
        Some(existing) if existing == new_value => {
            retained_fields.push(field.to_string());
        }
        Some(existing) => {
            bail!(
                "conflicting whole-cycle observed overwrite for {}: existing={} new={}",
                field,
                existing,
                new_value
            );
        }
        None => {
            whole_cycle.insert(field.to_string(), Value::from(new_value));
            updated_fields.push(field.to_string());
        }
    }
    Ok(())
}

pub(super) fn apply_tool_overhead_observed_token(
    whole_cycle: &mut serde_json::Map<String, Value>,
    tool_overhead_tokens: u64,
    legacy_replace_from: Option<u64>,
    updated_fields: &mut Vec<String>,
    retained_fields: &mut Vec<String>,
) -> Result<()> {
    match whole_cycle
        .get("tool_overhead_tokens")
        .and_then(Value::as_u64)
    {
        Some(existing) if existing == tool_overhead_tokens => {
            retained_fields.push("tool_overhead_tokens".to_string());
        }
        Some(existing) if legacy_replace_from == Some(existing) => {
            whole_cycle.insert(
                "tool_overhead_tokens".to_string(),
                Value::from(tool_overhead_tokens),
            );
            updated_fields.push("tool_overhead_tokens".to_string());
        }
        Some(existing) => {
            bail!(
                "conflicting whole-cycle observed overwrite for tool_overhead_tokens: existing={} new={}",
                existing,
                tool_overhead_tokens
            );
        }
        None => {
            whole_cycle.insert(
                "tool_overhead_tokens".to_string(),
                Value::from(tool_overhead_tokens),
            );
            updated_fields.push("tool_overhead_tokens".to_string());
        }
    }
    Ok(())
}

pub(crate) fn build_continuity_restore_observed_event(
    project_code: &str,
    namespace_code: &str,
    source_kind: &str,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    prompt_text: &str,
    continuity_restore_tokens: u64,
) -> Result<Value> {
    let timestamp_utc = current_epoch_ms()?;
    let event_id = Uuid::new_v4().to_string();
    let traffic_class = derive_traffic_class(source_kind);
    let agent_scope = working_state::current_agent_scope_for(project_code, namespace_code);
    let thread_id = codex_threads::current_thread_id()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let turn_id = thread_id
        .as_deref()
        .and_then(|thread_id| {
            codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)
                .ok()
                .flatten()
        })
        .and_then(|observation| {
            let turn_id = observation.turn_id.trim().to_string();
            if turn_id.is_empty() {
                None
            } else {
                Some(turn_id)
            }
        });
    Ok(json!({
        "token_budget_event": {
            "event_id": event_id,
            "correlation_id": event_id,
            "context_pack_id": Value::Null,
            "timestamp_utc": timestamp_utc,
            "occurred_at_epoch_ms": timestamp_utc,
            "ingested_at_epoch_ms": timestamp_utc,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "measurement_scope": "whole_cycle_observed_lower_bound",
            "payload_origin": "continuity_startup_observed_lower_bound",
            "contract": token_contract_metadata_json(contract),
            "project": project_code,
            "project_code": project_code,
            "namespace": namespace_code,
            "namespace_code": namespace_code,
            "agent_scope": agent_scope,
            "thread_id": thread_id,
            "turn_id": turn_id,
            "query": "CHAT_START_RESTORE",
            "query_hash": hex_sha256(prompt_text.as_bytes()),
            "query_type": "continuity_restore",
            "target_kind": "continuity_restore",
            "baseline_hit_target": false,
            "amai_hit_target": true,
            "cold_warm_state": "observed_only",
            "baseline_strategy": "observed_only",
            "retrieval_mode": Value::Null,
            "tokenizer": measurement.tokenizer,
            "latency_ms": 0,
            "baseline_tokens": 0,
            "delivered_tokens": 0,
            "gross_savings_pct": 0.0,
            "naive_limit_files": measurement.naive_limit_files,
            "naive_max_bytes_per_file": measurement.naive_max_bytes_per_file,
            "visible_projects": [project_code],
            "naive_scope": {
                "files_considered": 0,
                "files": [],
                "rendered_bytes": 0,
                "tokens": 0,
            },
            "context_pack_render": {
                "rendered_bytes": 0,
                "tokens": 0,
            },
            "whole_cycle_observed": {
                "client_prompt_tokens": Value::Null,
                "assistant_generation_tokens": Value::Null,
                "tool_overhead_tokens": Value::Null,
                "continuity_restore_tokens": continuity_restore_tokens,
            },
            "recovery": {
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0,
            },
            "quality": {
                "quality_ok": true,
                "quality_score": 1.0,
                "quality_method": "continuity_restore_observed",
                "quality_tier": "observed_only",
                "head_hit_target": true,
            },
            "followup": {
                "needed_followup": false,
                "followup_count": 0,
                "followup_of_event_id": Value::Null,
                "resolved_by_event_id": Value::Null,
            },
            "shape": {
                "document_hits": 0,
                "symbol_hits": 0,
                "file_hits": 0,
                "sources_count": 0,
                "chunks_count": 0,
                "pack_token_count": 0,
                "deduped_token_count": 0,
            },
            "savings": {
                "saved_tokens": 0,
                "effective_saved_tokens": 0,
                "savings_factor": 0.0,
                "savings_percent": 0.0,
                "effective_savings_percent": 0.0,
            },
            "continuity_restore_prompt_length_chars": prompt_text.len(),
            "continuity_restore_prompt_sha256": hex_sha256(prompt_text.as_bytes()),
        }
    }))
}

pub(super) fn continuity_snapshot_semantic_epoch_ms(snapshot: &ObservabilitySnapshotRecord) -> i64 {
    snapshot.payload["_observability"]["captured_at_epoch_ms"]
        .as_i64()
        .or_else(|| snapshot.payload["continuity_import"]["imported_at_epoch_ms"].as_i64())
        .or_else(|| snapshot.payload["continuity_handoff"]["captured_at_epoch_ms"].as_i64())
        .unwrap_or(snapshot.created_at_epoch_ms)
}

pub(super) fn continuity_snapshot_matches_scope(
    snapshot: &ObservabilitySnapshotRecord,
    snapshot_kind: &str,
    project_code: &str,
    namespace_code: &str,
) -> bool {
    let root = match snapshot_kind {
        "continuity_import" => &snapshot.payload["continuity_import"],
        "continuity_handoff" => &snapshot.payload["continuity_handoff"],
        _ => return false,
    };
    root["project"]["code"].as_str() == Some(project_code)
        && root["namespace"]["code"].as_str() == Some(namespace_code)
}

pub(super) fn latest_scoped_continuity_snapshot_before<'a>(
    snapshots: &'a [ObservabilitySnapshotRecord],
    snapshot_kind: &str,
    project_code: &str,
    namespace_code: &str,
    cutoff_epoch_ms: i64,
) -> Option<&'a ObservabilitySnapshotRecord> {
    snapshots
        .iter()
        .filter(|snapshot| snapshot.snapshot_kind == snapshot_kind)
        .filter(|snapshot| {
            continuity_snapshot_matches_scope(snapshot, snapshot_kind, project_code, namespace_code)
        })
        .filter(|snapshot| continuity_snapshot_semantic_epoch_ms(snapshot) <= cutoff_epoch_ms)
        .max_by_key(|snapshot| {
            (
                continuity_snapshot_semantic_epoch_ms(snapshot),
                snapshot.created_at_epoch_ms,
            )
        })
}

pub(super) fn render_continuity_pre_amai_baseline_text(
    project_code: &str,
    namespace_code: &str,
    continuity_import_snapshot: Option<&ObservabilitySnapshotRecord>,
    continuity_handoff_snapshot: Option<&ObservabilitySnapshotRecord>,
) -> Option<(String, Vec<String>, Value)> {
    if continuity_import_snapshot.is_none() && continuity_handoff_snapshot.is_none() {
        return None;
    }

    let mut lines = vec![
        "PRE_AMAI_CONTINUITY_BASELINE".to_string(),
        format!("Project: {project_code}"),
        format!("Namespace: {namespace_code}"),
    ];
    let mut source_entries = Vec::new();

    let continuity_import_ref = continuity_import_snapshot.map(|snapshot| {
        let node = &snapshot.payload["continuity_import"];
        let imported_at_epoch_ms = continuity_snapshot_semantic_epoch_ms(snapshot);
        lines.push(String::new());
        lines.push("## Continuity Import".to_string());
        lines.push(format!("- imported_at_epoch_ms: {imported_at_epoch_ms}"));
        if let Some(value) = node["documents_imported"].as_u64() {
            lines.push(format!("- documents_imported: {value}"));
        }
        if let Some(value) = node["session_memory_files"].as_u64() {
            lines.push(format!("- session_memory_files: {value}"));
        }
        if let Some(value) = node["rendered_transcript_files"].as_u64() {
            lines.push(format!("- rendered_transcript_files: {value}"));
        }
        if let Some(value) = node["bootstrap_summary"]["details"]["thread_count"].as_u64() {
            lines.push(format!("- bootstrap_thread_count: {value}"));
        }
        if let Some(value) = node["bootstrap_summary"]["details"]["latest_rendered_transcript"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("- latest_rendered_transcript: {value}"));
        }
        if let Some(value) = node["active_workline_summary"]["details"]["headline"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("- active_workline_headline: {value}"));
        }
        if let Some(value) = node["active_workline_summary"]["details"]["next_step"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("- active_workline_next_step: {value}"));
        }
        source_entries.push(format!("continuity_import:{}", snapshot.snapshot_id));
        json!({
            "snapshot_id": snapshot.snapshot_id.to_string(),
            "semantic_epoch_ms": imported_at_epoch_ms,
        })
    });

    let continuity_handoff_ref = continuity_handoff_snapshot.map(|snapshot| {
        let node = &snapshot.payload["continuity_handoff"];
        let captured_at_epoch_ms = continuity_snapshot_semantic_epoch_ms(snapshot);
        lines.push(String::new());
        lines.push("## Continuity Handoff".to_string());
        lines.push(format!("- captured_at_epoch_ms: {captured_at_epoch_ms}"));
        if let Some(value) = node["headline"].as_str().filter(|value| !value.is_empty()) {
            lines.push(format!("- headline: {value}"));
        }
        if let Some(value) = node["next_step"].as_str().filter(|value| !value.is_empty()) {
            lines.push(format!("- next_step: {value}"));
        }
        if let Some(value) = node["details"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(String::new());
            lines.push("### Handoff Details".to_string());
            lines.push(value.trim().to_string());
        }
        source_entries.push(format!("continuity_handoff:{}", snapshot.snapshot_id));
        json!({
            "snapshot_id": snapshot.snapshot_id.to_string(),
            "semantic_epoch_ms": captured_at_epoch_ms,
        })
    });

    Some((
        lines.join("\n"),
        source_entries,
        json!({
            "state": "materialized",
            "source_kind": CONTINUITY_PRE_AMAI_BASELINE_STRATEGY,
            "source_family": "truthful_pre_amai_baseline_source",
            "source_scope": "continuity_observability_snapshots",
            "continuity_import": continuity_import_ref.unwrap_or(Value::Null),
            "continuity_handoff": continuity_handoff_ref.unwrap_or(Value::Null),
        }),
    ))
}

pub(super) fn continuity_pre_amai_baseline_materialization(
    tokenizer: &CoreBPE,
    project_code: &str,
    namespace_code: &str,
    continuity_import_snapshot: Option<&ObservabilitySnapshotRecord>,
    continuity_handoff_snapshot: Option<&ObservabilitySnapshotRecord>,
) -> Option<ContinuityPreAmaiBaselineMaterialization> {
    let (baseline_text, source_entries, mut source_ref) = render_continuity_pre_amai_baseline_text(
        project_code,
        namespace_code,
        continuity_import_snapshot,
        continuity_handoff_snapshot,
    )?;
    let baseline_tokens = tokenizer.encode_with_special_tokens(&baseline_text).len() as u64;
    let baseline_sha256 = hex_sha256(baseline_text.as_bytes());
    if let Some(object) = source_ref.as_object_mut() {
        object.insert(
            "baseline_sha256".to_string(),
            Value::String(baseline_sha256),
        );
        object.insert(
            "source_entries".to_string(),
            Value::Array(
                source_entries
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect::<Vec<_>>(),
            ),
        );
    }
    Some(ContinuityPreAmaiBaselineMaterialization {
        baseline_tokens,
        baseline_bytes: baseline_text.len(),
        source_entries,
        source_ref,
    })
}

pub(super) fn apply_continuity_pre_amai_baseline(
    payload: &mut Value,
    materialization: &ContinuityPreAmaiBaselineMaterialization,
) -> Result<bool> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event"))?;
    let continuity_tokens = node["whole_cycle_observed"]["continuity_restore_tokens"]
        .as_u64()
        .unwrap_or(0);
    if continuity_tokens == 0 {
        return Ok(false);
    }

    let effective_saved_tokens = materialization.baseline_tokens as i64 - continuity_tokens as i64;
    let saved_tokens = materialization
        .baseline_tokens
        .saturating_sub(continuity_tokens);
    let savings_factor = if continuity_tokens == 0 {
        materialization.baseline_tokens as f64
    } else {
        materialization.baseline_tokens as f64 / continuity_tokens as f64
    };
    let savings_percent = if materialization.baseline_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / materialization.baseline_tokens as f64
    };
    let effective_savings_percent =
        percent_from_signed(effective_saved_tokens, materialization.baseline_tokens);

    let already_materialized = node["baseline_strategy"].as_str()
        == Some(CONTINUITY_PRE_AMAI_BASELINE_STRATEGY)
        && node["baseline_tokens"].as_u64() == Some(materialization.baseline_tokens)
        && node["pre_amai_baseline_source"] == materialization.source_ref;
    if already_materialized {
        return Ok(false);
    }

    node.insert("baseline_hit_target".to_string(), Value::Bool(true));
    node.insert(
        "cold_warm_state".to_string(),
        json!("pre_amai_baseline_materialized"),
    );
    node.insert(
        "baseline_strategy".to_string(),
        json!(CONTINUITY_PRE_AMAI_BASELINE_STRATEGY),
    );
    node.insert(
        "baseline_tokens".to_string(),
        Value::from(materialization.baseline_tokens),
    );
    node.insert(
        "naive_tokens".to_string(),
        Value::from(materialization.baseline_tokens),
    );
    node.insert(
        "pre_amai_baseline_source".to_string(),
        materialization.source_ref.clone(),
    );

    let naive_scope = ensure_nested_object(node, "naive_scope")?;
    naive_scope.insert(
        "files_considered".to_string(),
        Value::from(materialization.source_entries.len() as u64),
    );
    naive_scope.insert(
        "files".to_string(),
        Value::Array(
            materialization
                .source_entries
                .iter()
                .cloned()
                .map(Value::String)
                .collect::<Vec<_>>(),
        ),
    );
    naive_scope.insert(
        "rendered_bytes".to_string(),
        Value::from(materialization.baseline_bytes as u64),
    );
    naive_scope.insert(
        "tokens".to_string(),
        Value::from(materialization.baseline_tokens),
    );

    let savings = ensure_nested_object(node, "savings")?;
    savings.insert("saved_tokens".to_string(), Value::from(saved_tokens));
    savings.insert(
        "effective_saved_tokens".to_string(),
        Value::from(effective_saved_tokens),
    );
    savings.insert("savings_factor".to_string(), Value::from(savings_factor));
    savings.insert("savings_percent".to_string(), Value::from(savings_percent));
    savings.insert(
        "effective_savings_percent".to_string(),
        Value::from(effective_savings_percent),
    );
    node.insert("saved_tokens".to_string(), Value::from(saved_tokens));
    node.insert(
        "effective_saved_tokens".to_string(),
        Value::from(effective_saved_tokens),
    );
    node.insert("savings_factor".to_string(), Value::from(savings_factor));
    node.insert("savings_percent".to_string(), Value::from(savings_percent));
    node.insert(
        "gross_savings_pct".to_string(),
        Value::from(savings_percent),
    );
    node.insert(
        "effective_savings_percent".to_string(),
        Value::from(effective_savings_percent),
    );
    Ok(true)
}
