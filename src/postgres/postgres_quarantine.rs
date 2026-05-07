use super::*;

#[derive(Debug, Clone)]
struct QuarantineItemPolicyScopeFilter {
    workspace_code: String,
    entity_kind: String,
    entity_id_present: bool,
    project_code: Option<String>,
    namespace_code: Option<String>,
    namespace_without_project: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone)]
struct QuarantineItemVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

fn quarantine_item_marks_poisoned(evidence_span: &Value) -> bool {
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

fn run_quarantine_item_policy_scope_filter(
    workspace_code: &str,
    project: Option<&ProjectRecord>,
    namespace: Option<&NamespaceRecord>,
    record: &QuarantineItemInsert<'_>,
) -> QuarantineItemPolicyScopeFilter {
    let entity_id_present = record.entity_id.is_some();
    let namespace_without_project = namespace.is_some() && project.is_none();
    let scope_binding_valid = !namespace_without_project
        && match record.entity_kind {
            "policy_rule" | "project_relation" => entity_id_present,
            _ => true,
        };
    QuarantineItemPolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        entity_kind: record.entity_kind.to_string(),
        entity_id_present,
        project_code: project.map(|item| item.code.clone()),
        namespace_code: namespace.map(|item| item.code.clone()),
        namespace_without_project,
        scope_binding_valid,
    }
}

fn validate_quarantine_item_policy_scope_filter(
    filter: &QuarantineItemPolicyScopeFilter,
) -> Result<()> {
    if filter.namespace_without_project {
        return Err(anyhow!(
            "quarantine item cannot target namespace {:?} without project binding",
            filter.namespace_code
        ));
    }
    if matches!(
        filter.entity_kind.as_str(),
        "policy_rule" | "project_relation"
    ) && !filter.entity_id_present
    {
        return Err(anyhow!(
            "quarantine item for {} requires entity_id",
            filter.entity_kind
        ));
    }
    if !filter.scope_binding_valid {
        return Err(anyhow!(
            "quarantine item failed policy/scope filter for workspace={} project={:?} namespace={:?}",
            filter.workspace_code,
            filter.project_code,
            filter.namespace_code
        ));
    }
    Ok(())
}

fn run_quarantine_item_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    scope_filter: &QuarantineItemPolicyScopeFilter,
) -> QuarantineItemVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = quarantine_item_marks_poisoned(evidence_span);
    let write_allowed = evidence_present && !poisoned_detected && scope_filter.scope_binding_valid;
    QuarantineItemVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed,
    }
}

fn validate_quarantine_item_verification_conflict_check(
    check: &QuarantineItemVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "quarantine item is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "quarantine item must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "quarantine item failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

fn augment_quarantine_item_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &QuarantineItemPolicyScopeFilter,
    verification_check: &QuarantineItemVerificationConflictCheck,
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
            "policy_and_scope_filter": {
                "workspace_code": policy_filter.workspace_code,
                "entity_kind": policy_filter.entity_kind,
                "entity_id_present": policy_filter.entity_id_present,
                "project_code": policy_filter.project_code,
                "namespace_code": policy_filter.namespace_code,
                "namespace_without_project": policy_filter.namespace_without_project,
                "scope_binding_valid": policy_filter.scope_binding_valid,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "poisoned_detected": verification_check.poisoned_detected,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

pub async fn create_quarantine_item(
    client: &Client,
    workspace_code: &str,
    record: &QuarantineItemInsert<'_>,
) -> Result<QuarantineItemRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let project = match record.project_code {
        Some(code) => Some(get_project_by_code(client, code).await?),
        None => None,
    };
    let namespace = match (project.as_ref(), record.namespace_code) {
        (Some(project), Some(code)) => {
            Some(get_namespace_by_code(client, project.project_id, code).await?)
        }
        _ => None,
    };
    let source_event_ids = record
        .source_event_ids
        .cloned()
        .unwrap_or_else(|| json!([]));
    let artifact_refs = record.artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = record.message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = record.derivation_kind.unwrap_or("operator_write");
    let schema_version = record
        .schema_version
        .unwrap_or("quarantine-item-envelope-v1");
    let quarantined_at_epoch_ms = record.quarantined_at_epoch_ms.or(Some(current_epoch_ms()?));
    validate_stage2_basis(
        "quarantine item",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let policy_filter = run_quarantine_item_policy_scope_filter(
        &workspace.code,
        project.as_ref(),
        namespace.as_ref(),
        record,
    );
    validate_quarantine_item_policy_scope_filter(&policy_filter)?;
    let verification_check = run_quarantine_item_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_quarantine_item_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_quarantine_item_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.quarantine_items(
                workspace_id,
                project_id,
                namespace_id,
                entity_kind,
                entity_id,
                quarantine_reason,
                quarantine_state,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                quarantined_at_epoch_ms,
                released_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, COALESCE($7, 'active'), $8::jsonb, $9,
                $10::jsonb, $11::jsonb, $12::jsonb, $13::jsonb, $14, $15, $16, $17
            )
            RETURNING
                quarantine_item_id,
                $18::text,
                $19::text,
                $20::text,
                entity_kind,
                entity_id,
                quarantine_reason,
                quarantine_state,
                evidence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                quarantined_at_epoch_ms,
                released_at_epoch_ms
            "#,
            &[
                &workspace.workspace_id,
                &project.as_ref().map(|item| item.project_id),
                &namespace.as_ref().map(|item| item.namespace_id),
                &record.entity_kind,
                &record.entity_id,
                &record.quarantine_reason,
                &record.quarantine_state,
                record.evidence,
                &record.source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &quarantined_at_epoch_ms,
                &record.released_at_epoch_ms,
                &workspace.code,
                &project.as_ref().map(|item| item.code.clone()),
                &namespace.as_ref().map(|item| item.code.clone()),
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to create quarantine item for {} {:?}",
                record.entity_kind, record.entity_id
            )
        })?;
    Ok(quarantine_item_record_from_row(&row))
}

pub async fn get_quarantine_item(
    client: &Client,
    quarantine_item_id: Uuid,
) -> Result<QuarantineItemRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                qi.quarantine_item_id,
                w.code,
                p.code,
                n.code,
                qi.entity_kind,
                qi.entity_id,
                qi.quarantine_reason,
                qi.quarantine_state,
                qi.evidence,
                qi.source_kind,
                qi.source_event_ids,
                qi.artifact_refs,
                qi.message_refs,
                qi.evidence_span,
                qi.derivation_kind,
                qi.schema_version,
                qi.quarantined_at_epoch_ms,
                qi.released_at_epoch_ms
            FROM ami.quarantine_items qi
            INNER JOIN ami.workspaces w ON w.workspace_id = qi.workspace_id
            LEFT JOIN ami.projects p ON p.project_id = qi.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = qi.namespace_id
            WHERE qi.quarantine_item_id = $1
            "#,
            &[&quarantine_item_id],
        )
        .await
        .with_context(|| format!("failed to load quarantine item {}", quarantine_item_id))?;
    Ok(quarantine_item_record_from_row(&row))
}

pub(crate) async fn set_quarantine_items_state_for_entity(
    client: &Client,
    workspace_id: Uuid,
    entity_kind: &str,
    entity_id: Uuid,
    quarantine_state: &str,
    released_at_epoch_ms: Option<i64>,
) -> Result<u64> {
    let updated = client
        .execute(
            r#"
            UPDATE ami.quarantine_items
            SET quarantine_state = $4,
                released_at_epoch_ms = CASE
                    WHEN $5::bigint IS NULL THEN released_at_epoch_ms
                    ELSE $5::bigint
                END,
                updated_at = now()
            WHERE workspace_id = $1
              AND entity_kind = $2
              AND entity_id = $3
              AND quarantine_state = 'active'
            "#,
            &[
                &workspace_id,
                &entity_kind,
                &entity_id,
                &quarantine_state,
                &released_at_epoch_ms,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to update quarantine state for {} {:?}",
                entity_kind, entity_id
            )
        })?;
    Ok(updated)
}
