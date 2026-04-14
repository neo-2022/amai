use super::*;

#[derive(Debug, Clone, Serialize)]
struct MemoryEdgePolicyScopeFilter {
    workspace_code: String,
    project_code: String,
    namespace_code: String,
    source_memory_item_found: bool,
    target_memory_item_found: bool,
    source_memory_item_scope_matches: bool,
    target_memory_item_scope_matches: bool,
    self_link_conflict: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryEdgeVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    self_link_conflict: bool,
    write_allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryConflictPolicyScopeFilter {
    workspace_code: String,
    project_code: String,
    namespace_code: String,
    left_memory_item_present: bool,
    right_memory_item_present: bool,
    left_memory_item_found: bool,
    right_memory_item_found: bool,
    left_memory_item_scope_matches: bool,
    right_memory_item_scope_matches: bool,
    self_link_conflict: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryConflictVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    self_link_conflict: bool,
    write_allowed: bool,
}

fn memory_edge_or_conflict_marks_poisoned(evidence_span: &Value) -> bool {
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

async fn run_memory_edge_policy_scope_filter(
    client: &Client,
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &MemoryEdgeInsert<'_>,
) -> MemoryEdgePolicyScopeFilter {
    let source_memory_item = get_memory_item_by_id(client, record.source_memory_item_id)
        .await
        .ok();
    let target_memory_item = get_memory_item_by_id(client, record.target_memory_item_id)
        .await
        .ok();
    let source_memory_item_found = source_memory_item.is_some();
    let target_memory_item_found = target_memory_item.is_some();
    let source_memory_item_scope_matches = source_memory_item.as_ref().is_some_and(|item| {
        item.project_code == project.code
            && item.namespace_code.as_deref() == Some(namespace.code.as_str())
    });
    let target_memory_item_scope_matches = target_memory_item.as_ref().is_some_and(|item| {
        item.project_code == project.code
            && item.namespace_code.as_deref() == Some(namespace.code.as_str())
    });
    let self_link_conflict = record.source_memory_item_id == record.target_memory_item_id;
    MemoryEdgePolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        source_memory_item_found,
        target_memory_item_found,
        source_memory_item_scope_matches,
        target_memory_item_scope_matches,
        self_link_conflict,
        scope_binding_valid: source_memory_item_found
            && target_memory_item_found
            && source_memory_item_scope_matches
            && target_memory_item_scope_matches
            && !self_link_conflict,
    }
}

fn validate_memory_edge_policy_scope_filter(filter: &MemoryEdgePolicyScopeFilter) -> Result<()> {
    if !filter.source_memory_item_found {
        return Err(anyhow!("memory edge references missing source memory item"));
    }
    if !filter.target_memory_item_found {
        return Err(anyhow!("memory edge references missing target memory item"));
    }
    if !filter.source_memory_item_scope_matches {
        return Err(anyhow!(
            "memory edge source memory item scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.target_memory_item_scope_matches {
        return Err(anyhow!(
            "memory edge target memory item scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if filter.self_link_conflict {
        return Err(anyhow!("memory edge cannot self-link the same memory item"));
    }
    Ok(())
}

fn run_memory_edge_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    policy_filter: &MemoryEdgePolicyScopeFilter,
) -> MemoryEdgeVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || value_string_array_len(Some(source_event_ids)) > 0
        || value_string_array_len(Some(artifact_refs)) > 0
        || value_string_array_len(Some(message_refs)) > 0
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let poisoned_detected = memory_edge_or_conflict_marks_poisoned(evidence_span);
    MemoryEdgeVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        self_link_conflict: policy_filter.self_link_conflict,
        write_allowed: policy_filter.scope_binding_valid && evidence_present && !poisoned_detected,
    }
}

fn validate_memory_edge_verification_conflict_check(
    check: &MemoryEdgeVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!("memory edge evidence_span is flagged poisoned"));
    }
    if check.self_link_conflict {
        return Err(anyhow!("memory edge cannot self-link the same memory item"));
    }
    if !check.evidence_present {
        return Err(anyhow!("memory edge requires recorded evidence"));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory edge verification/conflict check blocked write"
        ));
    }
    Ok(())
}

fn augment_memory_edge_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &MemoryEdgePolicyScopeFilter,
    verification_check: &MemoryEdgeVerificationConflictCheck,
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

async fn run_memory_conflict_policy_scope_filter(
    client: &Client,
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &MemoryConflictInsert<'_>,
) -> MemoryConflictPolicyScopeFilter {
    let left_memory_item = match record.left_memory_item_id {
        Some(memory_item_id) => get_memory_item_by_id(client, memory_item_id).await.ok(),
        None => None,
    };
    let right_memory_item = match record.right_memory_item_id {
        Some(memory_item_id) => get_memory_item_by_id(client, memory_item_id).await.ok(),
        None => None,
    };
    let left_memory_item_present = record.left_memory_item_id.is_some();
    let right_memory_item_present = record.right_memory_item_id.is_some();
    let left_memory_item_found = left_memory_item.is_some();
    let right_memory_item_found = right_memory_item.is_some();
    let left_memory_item_scope_matches = !left_memory_item_present
        || left_memory_item.as_ref().is_some_and(|item| {
            item.project_code == project.code
                && item.namespace_code.as_deref() == Some(namespace.code.as_str())
        });
    let right_memory_item_scope_matches = !right_memory_item_present
        || right_memory_item.as_ref().is_some_and(|item| {
            item.project_code == project.code
                && item.namespace_code.as_deref() == Some(namespace.code.as_str())
        });
    let self_link_conflict = match (record.left_memory_item_id, record.right_memory_item_id) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    };
    MemoryConflictPolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        left_memory_item_present,
        right_memory_item_present,
        left_memory_item_found,
        right_memory_item_found,
        left_memory_item_scope_matches,
        right_memory_item_scope_matches,
        self_link_conflict,
        scope_binding_valid: (!left_memory_item_present
            || (left_memory_item_found && left_memory_item_scope_matches))
            && (!right_memory_item_present
                || (right_memory_item_found && right_memory_item_scope_matches))
            && !self_link_conflict,
    }
}

fn validate_memory_conflict_policy_scope_filter(
    filter: &MemoryConflictPolicyScopeFilter,
) -> Result<()> {
    if filter.left_memory_item_present && !filter.left_memory_item_found {
        return Err(anyhow!(
            "memory conflict references missing left memory item"
        ));
    }
    if filter.right_memory_item_present && !filter.right_memory_item_found {
        return Err(anyhow!(
            "memory conflict references missing right memory item"
        ));
    }
    if !filter.left_memory_item_scope_matches {
        return Err(anyhow!(
            "memory conflict left memory item scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.right_memory_item_scope_matches {
        return Err(anyhow!(
            "memory conflict right memory item scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if filter.self_link_conflict {
        return Err(anyhow!(
            "memory conflict cannot reference the same memory item on both sides"
        ));
    }
    Ok(())
}

fn run_memory_conflict_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    policy_filter: &MemoryConflictPolicyScopeFilter,
) -> MemoryConflictVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || value_string_array_len(Some(source_event_ids)) > 0
        || value_string_array_len(Some(artifact_refs)) > 0
        || value_string_array_len(Some(message_refs)) > 0
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let poisoned_detected = memory_edge_or_conflict_marks_poisoned(evidence_span);
    MemoryConflictVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        self_link_conflict: policy_filter.self_link_conflict,
        write_allowed: policy_filter.scope_binding_valid && evidence_present && !poisoned_detected,
    }
}

fn validate_memory_conflict_verification_conflict_check(
    check: &MemoryConflictVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!("memory conflict evidence_span is flagged poisoned"));
    }
    if check.self_link_conflict {
        return Err(anyhow!(
            "memory conflict cannot reference the same memory item on both sides"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!("memory conflict requires recorded evidence"));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory conflict verification/conflict check blocked write"
        ));
    }
    Ok(())
}

fn augment_memory_conflict_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &MemoryConflictPolicyScopeFilter,
    verification_check: &MemoryConflictVerificationConflictCheck,
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

pub async fn create_memory_edge(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &MemoryEdgeInsert<'_>,
) -> Result<MemoryEdgeRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let workspace = get_workspace_by_id(client, workspace_id).await?;
    let source_event_ids = record
        .source_event_ids
        .cloned()
        .unwrap_or_else(|| json!([]));
    let artifact_refs = record.artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = record.message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = record.derivation_kind.unwrap_or("extract");
    let schema_version = record.schema_version.unwrap_or("memory-edge-envelope-v1");
    validate_stage2_basis(
        "memory edge",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let policy_filter =
        run_memory_edge_policy_scope_filter(client, &workspace.code, &project, &namespace, record)
            .await;
    validate_memory_edge_policy_scope_filter(&policy_filter)?;
    let verification_check = run_memory_edge_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_memory_edge_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_memory_edge_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_edges(
                workspace_id,
                project_id,
                namespace_id,
                source_memory_item_id,
                target_memory_item_id,
                edge_kind,
                edge_state,
                trust_state,
                validity_basis,
                score,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                valid_from_epoch_ms,
                valid_to_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, COALESCE($7, 'active'), COALESCE($8, 'proposed'),
                COALESCE($9, 'explicit'), $10, $11::jsonb, $12, $13::jsonb, $14::jsonb,
                $15::jsonb, $16::jsonb, $17, $18, $19, $20
            )
            ON CONFLICT (source_memory_item_id, target_memory_item_id, edge_kind) DO UPDATE SET
                workspace_id = EXCLUDED.workspace_id,
                project_id = EXCLUDED.project_id,
                namespace_id = EXCLUDED.namespace_id,
                edge_state = EXCLUDED.edge_state,
                trust_state = EXCLUDED.trust_state,
                validity_basis = EXCLUDED.validity_basis,
                score = EXCLUDED.score,
                evidence = EXCLUDED.evidence,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                artifact_refs = EXCLUDED.artifact_refs,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                valid_from_epoch_ms = EXCLUDED.valid_from_epoch_ms,
                valid_to_epoch_ms = EXCLUDED.valid_to_epoch_ms,
                updated_at = now()
            RETURNING
                memory_edge_id,
                $21::text,
                $22::text,
                $23::text,
                source_memory_item_id,
                target_memory_item_id,
                edge_kind,
                edge_state,
                trust_state,
                validity_basis,
                score,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                valid_from_epoch_ms,
                valid_to_epoch_ms
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.source_memory_item_id,
                &record.target_memory_item_id,
                &record.edge_kind,
                &record.edge_state,
                &record.trust_state,
                &record.validity_basis,
                &record.score,
                record.evidence,
                &record.source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &record.valid_from_epoch_ms,
                &record.valid_to_epoch_ms,
                &workspace.code,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to create memory edge {}:{} -> {}:{} ({})",
                project_code,
                record.source_memory_item_id,
                project_code,
                record.target_memory_item_id,
                record.edge_kind
            )
        })?;
    Ok(memory_edge_record_from_row(&row))
}

pub async fn get_memory_edge(client: &Client, memory_edge_id: Uuid) -> Result<MemoryEdgeRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                me.memory_edge_id,
                w.code,
                p.code,
                n.code,
                me.source_memory_item_id,
                me.target_memory_item_id,
                me.edge_kind,
                me.edge_state,
                me.trust_state,
                me.validity_basis,
                me.score,
                me.evidence,
                me.source_kind,
                me.source_event_ids,
                me.artifact_refs,
                me.message_refs,
                me.evidence_span,
                me.derivation_kind,
                me.schema_version,
                me.valid_from_epoch_ms,
                me.valid_to_epoch_ms
            FROM ami.memory_edges me
            JOIN ami.workspaces w ON w.workspace_id = me.workspace_id
            JOIN ami.projects p ON p.project_id = me.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = me.namespace_id
            WHERE me.memory_edge_id = $1
            "#,
            &[&memory_edge_id],
        )
        .await
        .with_context(|| format!("failed to load memory edge {memory_edge_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("memory edge {memory_edge_id} not found"));
    };
    Ok(memory_edge_record_from_row(&row))
}

pub async fn create_memory_conflict(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &MemoryConflictInsert<'_>,
) -> Result<MemoryConflictRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let workspace = get_workspace_by_id(client, workspace_id).await?;
    let source_event_ids = record
        .source_event_ids
        .cloned()
        .unwrap_or_else(|| json!([]));
    let artifact_refs = record.artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = record.message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = record.derivation_kind.unwrap_or("extract");
    let schema_version = record
        .schema_version
        .unwrap_or("memory-conflict-envelope-v1");
    let conflict_state = record.conflict_state.unwrap_or("open");
    let resolution = record.resolution.cloned().unwrap_or_else(|| json!({}));
    let resolved_at_epoch_ms = match conflict_state {
        "resolved" | "dismissed" | "archived" => {
            Some(record.resolved_at_epoch_ms.unwrap_or(current_epoch_ms()?))
        }
        _ => record.resolved_at_epoch_ms,
    };
    validate_stage2_basis(
        "memory conflict",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let policy_filter = run_memory_conflict_policy_scope_filter(
        client,
        &workspace.code,
        &project,
        &namespace,
        record,
    )
    .await;
    validate_memory_conflict_policy_scope_filter(&policy_filter)?;
    let verification_check = run_memory_conflict_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_memory_conflict_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_memory_conflict_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_conflicts(
                workspace_id,
                project_id,
                namespace_id,
                left_memory_item_id,
                right_memory_item_id,
                conflict_kind,
                conflict_state,
                severity,
                summary,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                resolution,
                detected_at_epoch_ms,
                resolved_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, COALESCE($8, 'medium'), $9, $10::jsonb, $11,
                $12::jsonb, $13::jsonb, $14::jsonb, $15::jsonb, $16, $17, $18::jsonb, $19, $20
            )
            RETURNING
                memory_conflict_id,
                $21::text,
                $22::text,
                $23::text,
                left_memory_item_id,
                right_memory_item_id,
                conflict_kind,
                conflict_state,
                severity,
                summary,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                resolution,
                detected_at_epoch_ms,
                resolved_at_epoch_ms
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.left_memory_item_id,
                &record.right_memory_item_id,
                &record.conflict_kind,
                &conflict_state,
                &record.severity,
                &record.summary,
                record.evidence,
                &record.source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &resolution,
                &record.detected_at_epoch_ms,
                &resolved_at_epoch_ms,
                &workspace.code,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create memory conflict in {project_code}:{namespace_code}")
        })?;
    let conflict = memory_conflict_record_from_row(&row);
    if let (Some(left_memory_item_id), Some(right_memory_item_id)) =
        (record.left_memory_item_id, record.right_memory_item_id)
    {
        let validity_basis = if derivation_kind == "operator_write" {
            "operator"
        } else {
            "derived"
        };
        let _ = create_memory_edge(
            client,
            project_code,
            namespace_code,
            &MemoryEdgeInsert {
                source_memory_item_id: left_memory_item_id,
                target_memory_item_id: right_memory_item_id,
                edge_kind: "conflicts_with",
                edge_state: Some(conflict_state_to_edge_state(conflict_state)),
                trust_state: Some(conflict_state_to_edge_trust_state(
                    conflict_state,
                    derivation_kind,
                )),
                validity_basis: Some(validity_basis),
                score: None,
                evidence: record.evidence,
                source_kind: record.source_kind,
                source_event_ids: Some(&source_event_ids),
                artifact_refs: Some(&artifact_refs),
                message_refs: Some(&message_refs),
                evidence_span: Some(&evidence_span),
                derivation_kind: Some(derivation_kind),
                schema_version: Some("memory-edge-envelope-v1"),
                valid_from_epoch_ms: record.detected_at_epoch_ms,
                valid_to_epoch_ms: resolved_at_epoch_ms,
            },
        )
        .await?;
    }
    Ok(conflict)
}

pub async fn get_memory_conflict(
    client: &Client,
    memory_conflict_id: Uuid,
) -> Result<MemoryConflictRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                mc.memory_conflict_id,
                w.code,
                p.code,
                n.code,
                mc.left_memory_item_id,
                mc.right_memory_item_id,
                mc.conflict_kind,
                mc.conflict_state,
                mc.severity,
                mc.summary,
                mc.evidence,
                mc.source_kind,
                mc.source_event_ids,
                mc.artifact_refs,
                mc.message_refs,
                mc.evidence_span,
                mc.derivation_kind,
                mc.schema_version,
                mc.resolution,
                mc.detected_at_epoch_ms,
                mc.resolved_at_epoch_ms
            FROM ami.memory_conflicts mc
            JOIN ami.workspaces w ON w.workspace_id = mc.workspace_id
            JOIN ami.projects p ON p.project_id = mc.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = mc.namespace_id
            WHERE mc.memory_conflict_id = $1
            "#,
            &[&memory_conflict_id],
        )
        .await
        .with_context(|| format!("failed to load memory conflict {memory_conflict_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("memory conflict {memory_conflict_id} not found"));
    };
    Ok(memory_conflict_record_from_row(&row))
}
