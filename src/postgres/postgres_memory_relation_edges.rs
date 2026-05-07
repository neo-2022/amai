use super::*;

#[derive(Debug, Clone, Serialize)]
struct MemoryRelationEdgePolicyScopeFilter {
    project_code: String,
    namespace_code: String,
    source_memory_card_found: bool,
    target_memory_card_found: bool,
    source_memory_card_scope_matches: bool,
    target_memory_card_scope_matches: bool,
    self_link_conflict: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryRelationEdgeVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    self_link_conflict: bool,
    write_allowed: bool,
}

fn memory_relation_edge_marks_poisoned(evidence_span: &Value) -> bool {
    evidence_span
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || evidence_span
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

async fn run_memory_relation_edge_policy_scope_filter(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    source_memory_card_id: Uuid,
    target_memory_card_id: Uuid,
) -> MemoryRelationEdgePolicyScopeFilter {
    let source_card = get_memory_card(client, source_memory_card_id).await.ok();
    let target_card = get_memory_card(client, target_memory_card_id).await.ok();
    let source_memory_card_found = source_card.is_some();
    let target_memory_card_found = target_card.is_some();
    let source_memory_card_scope_matches = source_card.as_ref().is_some_and(|card| {
        card.project_code == project.code && card.namespace_code == namespace.code
    });
    let target_memory_card_scope_matches = target_card.as_ref().is_some_and(|card| {
        card.project_code == project.code && card.namespace_code == namespace.code
    });
    let self_link_conflict = source_memory_card_id == target_memory_card_id;
    MemoryRelationEdgePolicyScopeFilter {
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        source_memory_card_found,
        target_memory_card_found,
        source_memory_card_scope_matches,
        target_memory_card_scope_matches,
        self_link_conflict,
        scope_binding_valid: source_memory_card_found
            && target_memory_card_found
            && source_memory_card_scope_matches
            && target_memory_card_scope_matches
            && !self_link_conflict,
    }
}

fn validate_memory_relation_edge_policy_scope_filter(
    filter: &MemoryRelationEdgePolicyScopeFilter,
) -> Result<()> {
    if !filter.source_memory_card_found {
        return Err(anyhow!(
            "memory relation edge references missing source memory card"
        ));
    }
    if !filter.target_memory_card_found {
        return Err(anyhow!(
            "memory relation edge references missing target memory card"
        ));
    }
    if !filter.source_memory_card_scope_matches {
        return Err(anyhow!(
            "memory relation edge source memory card scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.target_memory_card_scope_matches {
        return Err(anyhow!(
            "memory relation edge target memory card scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if filter.self_link_conflict {
        return Err(anyhow!(
            "memory relation edge cannot self-link the same memory card"
        ));
    }
    Ok(())
}

fn run_memory_relation_edge_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    policy_filter: &MemoryRelationEdgePolicyScopeFilter,
) -> MemoryRelationEdgeVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || value_string_array_len(Some(source_event_ids)) > 0
        || value_string_array_len(Some(artifact_refs)) > 0
        || value_string_array_len(Some(message_refs)) > 0
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let poisoned_detected = memory_relation_edge_marks_poisoned(evidence_span);
    MemoryRelationEdgeVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        self_link_conflict: policy_filter.self_link_conflict,
        write_allowed: policy_filter.scope_binding_valid && evidence_present && !poisoned_detected,
    }
}

fn validate_memory_relation_edge_verification_conflict_check(
    check: &MemoryRelationEdgeVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "memory relation edge evidence_span is flagged poisoned"
        ));
    }
    if check.self_link_conflict {
        return Err(anyhow!(
            "memory relation edge cannot self-link the same memory card"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!("memory relation edge requires recorded evidence"));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory relation edge verification/conflict check blocked write"
        ));
    }
    Ok(())
}

fn augment_memory_relation_edge_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &MemoryRelationEdgePolicyScopeFilter,
    verification_check: &MemoryRelationEdgeVerificationConflictCheck,
) -> Value {
    let mut object = match evidence_span {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_evidence_span".to_string(), evidence_span.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "policy_and_scope_filter": policy_filter,
            "verification_conflict_check": verification_check,
        }),
    );
    Value::Object(object)
}

pub async fn create_memory_relation_edge(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    source_memory_card_id: Uuid,
    target_memory_card_id: Uuid,
    relation_type: &str,
    relation_state: Option<&str>,
    evidence: &Value,
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    artifact_refs: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
    recorded_at_epoch_ms: Option<i64>,
    valid_from_epoch_ms: Option<i64>,
    valid_to_epoch_ms: Option<i64>,
) -> Result<MemoryRelationEdgeRecord> {
    let (_workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let source_event_ids = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let artifact_refs = artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = derivation_kind.unwrap_or("extract");
    let schema_version = schema_version.unwrap_or("memory-relation-edge-envelope-v1");
    validate_memory_relation_edge_basis(
        derivation_kind,
        evidence,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let policy_filter = run_memory_relation_edge_policy_scope_filter(
        client,
        &project,
        &namespace,
        source_memory_card_id,
        target_memory_card_id,
    )
    .await;
    validate_memory_relation_edge_policy_scope_filter(&policy_filter)?;
    let verification_check = run_memory_relation_edge_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_memory_relation_edge_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_memory_relation_edge_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_relation_edges(
                project_id,
                namespace_id,
                source_memory_card_id,
                target_memory_card_id,
                relation_type,
                relation_state,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5,
                COALESCE($6, 'active'),
                $7::jsonb,
                $8,
                $9::jsonb,
                $10::jsonb,
                $11::jsonb,
                $12::jsonb,
                $13,
                $14,
                $15,
                $16,
                $17
            )
            ON CONFLICT (source_memory_card_id, target_memory_card_id, relation_type) DO UPDATE SET
                relation_state = EXCLUDED.relation_state,
                evidence = EXCLUDED.evidence,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                artifact_refs = EXCLUDED.artifact_refs,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                recorded_at_epoch_ms = EXCLUDED.recorded_at_epoch_ms,
                valid_from_epoch_ms = EXCLUDED.valid_from_epoch_ms,
                valid_to_epoch_ms = EXCLUDED.valid_to_epoch_ms,
                created_at = ami.memory_relation_edges.created_at
            RETURNING
                memory_relation_edge_id,
                $18::text,
                $19::text,
                source_memory_card_id,
                target_memory_card_id,
                relation_type,
                relation_state,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
                ,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            "#,
            &[
                &project.project_id,
                &namespace.namespace_id,
                &source_memory_card_id,
                &target_memory_card_id,
                &relation_type,
                &relation_state,
                evidence,
                &source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &recorded_at_epoch_ms,
                &valid_from_epoch_ms,
                &valid_to_epoch_ms,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to create memory relation edge {project_code}:{namespace_code}:{relation_type}"
            )
        })?;
    Ok(memory_relation_edge_record_from_row(&row))
}

pub async fn get_memory_relation_edge(
    client: &Client,
    memory_relation_edge_id: Uuid,
) -> Result<MemoryRelationEdgeRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                mre.memory_relation_edge_id,
                p.code,
                n.code,
                mre.source_memory_card_id,
                mre.target_memory_card_id,
                mre.relation_type,
                mre.relation_state,
                mre.evidence,
                mre.source_kind,
                mre.source_event_ids,
                mre.artifact_refs,
                mre.message_refs,
                mre.evidence_span,
                mre.derivation_kind,
                mre.schema_version,
                mre.recorded_at_epoch_ms,
                mre.valid_from_epoch_ms,
                mre.valid_to_epoch_ms,
                to_char(mre.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            FROM ami.memory_relation_edges mre
            JOIN ami.projects p ON p.project_id = mre.project_id
            JOIN ami.namespaces n ON n.namespace_id = mre.namespace_id
            WHERE memory_relation_edge_id = $1
            "#,
            &[&memory_relation_edge_id],
        )
        .await
        .with_context(|| {
            format!("failed to load memory relation edge {memory_relation_edge_id}")
        })?;
    let Some(row) = row else {
        return Err(anyhow!(
            "memory relation edge {memory_relation_edge_id} not found"
        ));
    };
    Ok(memory_relation_edge_record_from_row(&row))
}
