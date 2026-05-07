use super::*;

#[derive(Debug, Clone)]
struct PolicyRulePolicyScopeFilter {
    workspace_code: String,
    rule_code: String,
    rule_scope: String,
    project_code: Option<String>,
    namespace_code: Option<String>,
    project_present: bool,
    namespace_present: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone)]
struct PolicyRuleVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

fn policy_rule_marks_poisoned(evidence_span: &Value) -> bool {
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

fn run_policy_rule_policy_scope_filter(
    workspace_code: &str,
    project: Option<&ProjectRecord>,
    namespace: Option<&NamespaceRecord>,
    record: &PolicyRuleInsert<'_>,
) -> PolicyRulePolicyScopeFilter {
    let project_present = project.is_some();
    let namespace_present = namespace.is_some();
    let scope_binding_valid = match record.rule_scope {
        "workspace" => !project_present && !namespace_present,
        "project" => project_present,
        "namespace" => project_present && namespace_present,
        "agent" | "shared" => !namespace_present,
        _ => false,
    };
    PolicyRulePolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        rule_code: record.rule_code.to_string(),
        rule_scope: record.rule_scope.to_string(),
        project_code: project.map(|item| item.code.clone()),
        namespace_code: namespace.map(|item| item.code.clone()),
        project_present,
        namespace_present,
        scope_binding_valid,
    }
}

fn validate_policy_rule_policy_scope_filter(filter: &PolicyRulePolicyScopeFilter) -> Result<()> {
    if !filter.scope_binding_valid {
        return Err(anyhow!(
            "policy rule {} has invalid scope binding for rule_scope={} (project={:?}, namespace={:?})",
            filter.rule_code,
            filter.rule_scope,
            filter.project_code,
            filter.namespace_code
        ));
    }
    Ok(())
}

fn run_policy_rule_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    scope_filter: &PolicyRulePolicyScopeFilter,
) -> PolicyRuleVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = policy_rule_marks_poisoned(evidence_span);
    let write_allowed = evidence_present && !poisoned_detected && scope_filter.scope_binding_valid;
    PolicyRuleVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed,
    }
}

fn validate_policy_rule_verification_conflict_check(
    check: &PolicyRuleVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "policy rule is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "policy rule must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "policy rule failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

fn augment_policy_rule_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &PolicyRulePolicyScopeFilter,
    verification_check: &PolicyRuleVerificationConflictCheck,
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
                "rule_code": policy_filter.rule_code,
                "rule_scope": policy_filter.rule_scope,
                "project_code": policy_filter.project_code,
                "namespace_code": policy_filter.namespace_code,
                "project_present": policy_filter.project_present,
                "namespace_present": policy_filter.namespace_present,
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

pub async fn create_policy_rule(
    client: &Client,
    workspace_code: &str,
    record: &PolicyRuleInsert<'_>,
) -> Result<PolicyRuleRecord> {
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
    let schema_version = record.schema_version.unwrap_or("policy-rule-envelope-v1");
    validate_stage2_basis(
        "policy rule",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let policy_filter = run_policy_rule_policy_scope_filter(
        &workspace.code,
        project.as_ref(),
        namespace.as_ref(),
        record,
    );
    validate_policy_rule_policy_scope_filter(&policy_filter)?;
    let verification_check = run_policy_rule_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_policy_rule_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_policy_rule_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.policy_rules(
                workspace_id,
                project_id,
                namespace_id,
                rule_code,
                rule_scope,
                rule_kind,
                rule_status,
                precedence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                rule_payload
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, COALESCE($7, 'active'), COALESCE($8, 100),
                $9, $10::jsonb, $11::jsonb, $12::jsonb, $13::jsonb, $14, $15, $16::jsonb
            )
            ON CONFLICT (workspace_id, rule_code) DO UPDATE SET
                project_id = EXCLUDED.project_id,
                namespace_id = EXCLUDED.namespace_id,
                rule_scope = EXCLUDED.rule_scope,
                rule_kind = EXCLUDED.rule_kind,
                rule_status = EXCLUDED.rule_status,
                precedence = EXCLUDED.precedence,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                artifact_refs = EXCLUDED.artifact_refs,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                rule_payload = EXCLUDED.rule_payload,
                updated_at = now()
            RETURNING
                policy_rule_id,
                $17::text,
                $18::text,
                $19::text,
                rule_code,
                rule_scope,
                rule_kind,
                rule_status,
                precedence,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                rule_payload
            "#,
            &[
                &workspace.workspace_id,
                &project.as_ref().map(|item| item.project_id),
                &namespace.as_ref().map(|item| item.namespace_id),
                &record.rule_code,
                &record.rule_scope,
                &record.rule_kind,
                &record.rule_status,
                &record.precedence,
                &record.source_kind,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                record.rule_payload,
                &workspace.code,
                &project.as_ref().map(|item| item.code.clone()),
                &namespace.as_ref().map(|item| item.code.clone()),
            ],
        )
        .await
        .with_context(|| format!("failed to create policy rule {}", record.rule_code))?;
    Ok(policy_rule_record_from_row(&row))
}

pub async fn get_policy_rule(client: &Client, policy_rule_id: Uuid) -> Result<PolicyRuleRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                pr.policy_rule_id,
                w.code,
                p.code,
                n.code,
                pr.rule_code,
                pr.rule_scope,
                pr.rule_kind,
                pr.rule_status,
                pr.precedence,
                pr.source_kind,
                pr.source_event_ids,
                pr.artifact_refs,
                pr.message_refs,
                pr.evidence_span,
                pr.derivation_kind,
                pr.schema_version,
                pr.rule_payload
            FROM ami.policy_rules pr
            INNER JOIN ami.workspaces w ON w.workspace_id = pr.workspace_id
            LEFT JOIN ami.projects p ON p.project_id = pr.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = pr.namespace_id
            WHERE pr.policy_rule_id = $1
            "#,
            &[&policy_rule_id],
        )
        .await
        .with_context(|| format!("failed to load policy rule {}", policy_rule_id))?;
    Ok(policy_rule_record_from_row(&row))
}
