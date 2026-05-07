use super::*;

pub async fn ensure_shared_asset(
    client: &Client,
    workspace_code: &str,
    code: &str,
    display_name: &str,
    asset_kind: &str,
    source_project_code: Option<&str>,
    transfer_policy_code: Option<&str>,
    visibility_scope: &str,
    status: &str,
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    artifact_refs: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<SharedAssetRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let source_project = match source_project_code {
        Some(item) => Some(get_project_by_code(client, item).await?),
        None => None,
    };
    let transfer_policy = match transfer_policy_code {
        Some(code) => find_transfer_policy_by_code(client, code).await?,
        None => None,
    };
    let source_event_ids_value = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let artifact_refs_value = artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs_value = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span_value = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind_value = derivation_kind.unwrap_or("extract");
    let schema_version_value = schema_version.unwrap_or("shared-asset-envelope-v1");
    let source_project_workspace_matches = match source_project.as_ref() {
        Some(project) => {
            get_project_workspace_id(client, project.project_id).await? == workspace.workspace_id
        }
        None => true,
    };
    validate_stage2_basis(
        "shared asset",
        derivation_kind_value,
        &source_event_ids_value,
        &artifact_refs_value,
        &message_refs_value,
        &evidence_span_value,
    )?;
    let policy_filter = run_shared_asset_policy_scope_filter(
        &workspace.code,
        code,
        visibility_scope,
        source_project.is_some(),
        source_project_workspace_matches,
        transfer_policy.as_ref(),
    );
    validate_shared_asset_policy_scope_filter(&policy_filter)?;
    let verification_check = run_shared_asset_verification_conflict_check(
        derivation_kind_value,
        &source_event_ids_value,
        &artifact_refs_value,
        &message_refs_value,
        &evidence_span_value,
        &policy_filter,
    );
    validate_shared_asset_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_shared_asset_evidence_span_with_stage2_preflight(
        &evidence_span_value,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.shared_assets(
                workspace_id,
                source_project_id,
                transfer_policy_id,
                code,
                display_name,
                asset_kind,
                visibility_scope,
                status,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11::jsonb, $12::jsonb, $13::jsonb, $14, $15)
            ON CONFLICT (workspace_id, code) DO UPDATE SET
                source_project_id = EXCLUDED.source_project_id,
                transfer_policy_id = EXCLUDED.transfer_policy_id,
                display_name = EXCLUDED.display_name,
                asset_kind = EXCLUDED.asset_kind,
                visibility_scope = EXCLUDED.visibility_scope,
                status = EXCLUDED.status,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                artifact_refs = EXCLUDED.artifact_refs,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                updated_at = now()
            RETURNING
                shared_asset_id,
                $16::text,
                code,
                display_name,
                asset_kind,
                $17::text,
                $18::text,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                visibility_scope,
                status
            "#,
            &[
                &workspace.workspace_id,
                &source_project.as_ref().map(|item| item.project_id),
                &transfer_policy.as_ref().map(|item| item.transfer_policy_id),
                &code,
                &display_name,
                &asset_kind,
                &visibility_scope,
                &status,
                &source_kind,
                &source_event_ids_value,
                &artifact_refs_value,
                &message_refs_value,
                &stored_evidence_span,
                &derivation_kind_value,
                &schema_version_value,
                &workspace.code,
                &source_project.as_ref().map(|item| item.code.clone()),
                &transfer_policy.as_ref().map(|item| item.code.clone()),
            ],
        )
        .await
        .context("failed to ensure shared asset")?;
    Ok(shared_asset_record_from_row(&row))
}

async fn find_shared_asset_id_by_code_in_workspace(
    client: &Client,
    workspace_id: Uuid,
    code: &str,
) -> Result<Option<Uuid>> {
    let row = client
        .query_opt(
            r#"
            SELECT shared_asset_id
            FROM ami.shared_assets
            WHERE workspace_id = $1
              AND code = $2
            "#,
            &[&workspace_id, &code],
        )
        .await
        .with_context(|| format!("failed to lookup shared asset {code} in workspace"))?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn bind_shared_asset_to_project(
    client: &Client,
    asset_code: &str,
    project_code: &str,
    binding_kind: &str,
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    artifact_refs: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<()> {
    let project = get_project_by_code(client, project_code).await?;
    let project_workspace_id = get_project_workspace_id(client, project.project_id).await?;
    let project_workspace = get_workspace_by_id(client, project_workspace_id).await?;
    let shared_asset_id = find_shared_asset_id_by_code_in_workspace(
        client,
        project_workspace.workspace_id,
        asset_code,
    )
    .await?
    .ok_or_else(|| {
        anyhow!(
            "shared asset not found in workspace {}: {}",
            project_workspace.code,
            asset_code
        )
    })?;
    let asset = get_shared_asset(client, shared_asset_id).await?;
    let source_event_ids_value = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let artifact_refs_value = artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs_value = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span_value = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind_value = derivation_kind.unwrap_or("extract");
    let schema_version_value = schema_version.unwrap_or("shared-asset-project-binding-v1");
    validate_stage2_basis(
        "shared asset project binding",
        derivation_kind_value,
        &source_event_ids_value,
        &artifact_refs_value,
        &message_refs_value,
        &evidence_span_value,
    )?;
    let policy_filter = run_shared_asset_binding_policy_scope_filter(
        asset_code,
        project_code,
        &asset.workspace_code,
        &project_workspace.code,
        binding_kind,
    );
    validate_shared_asset_binding_policy_scope_filter(&policy_filter)?;
    let verification_check = run_shared_asset_binding_verification_conflict_check(
        derivation_kind_value,
        &source_event_ids_value,
        &artifact_refs_value,
        &message_refs_value,
        &evidence_span_value,
        &policy_filter,
    );
    validate_shared_asset_binding_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_shared_asset_binding_evidence_span_with_stage2_preflight(
        &evidence_span_value,
        &policy_filter,
        &verification_check,
    );
    client
        .execute(
            r#"
            INSERT INTO ami.shared_asset_projects(
                shared_asset_id,
                project_id,
                binding_kind,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5::jsonb, $6::jsonb, $7::jsonb, $8::jsonb, $9, $10)
            ON CONFLICT (shared_asset_id, project_id) DO UPDATE SET
                binding_kind = EXCLUDED.binding_kind,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                artifact_refs = EXCLUDED.artifact_refs,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                updated_at = now()
            "#,
            &[
                &shared_asset_id,
                &project.project_id,
                &binding_kind,
                &source_kind,
                &source_event_ids_value,
                &artifact_refs_value,
                &message_refs_value,
                &stored_evidence_span,
                &derivation_kind_value,
                &schema_version_value,
            ],
        )
        .await
        .context("failed to bind shared asset to project")?;
    Ok(())
}

pub async fn list_shared_assets(
    client: &Client,
    workspace_code: Option<&str>,
    project_code: Option<&str>,
    asset_code: Option<&str>,
) -> Result<Vec<SharedAssetRecord>> {
    let rows = client
        .query(
            r#"
            SELECT DISTINCT
                sa.shared_asset_id,
                w.code,
                sa.code,
                sa.display_name,
                sa.asset_kind,
                source.code,
                tp.code,
                sa.source_kind,
                sa.source_event_ids,
                sa.artifact_refs,
                sa.message_refs,
                sa.evidence_span,
                sa.derivation_kind,
                sa.schema_version,
                sa.visibility_scope,
                sa.status
            FROM ami.shared_assets sa
            INNER JOIN ami.workspaces w ON w.workspace_id = sa.workspace_id
            LEFT JOIN ami.projects source ON source.project_id = sa.source_project_id
            LEFT JOIN ami.transfer_policies tp ON tp.transfer_policy_id = sa.transfer_policy_id
            LEFT JOIN ami.shared_asset_projects sap ON sap.shared_asset_id = sa.shared_asset_id
            LEFT JOIN ami.projects p ON p.project_id = sap.project_id
            WHERE ($1::text IS NULL OR w.code = $1)
              AND ($2::text IS NULL OR p.code = $2 OR source.code = $2)
              AND ($3::text IS NULL OR sa.code = $3)
            ORDER BY w.code, sa.code
            "#,
            &[&workspace_code, &project_code, &asset_code],
        )
        .await
        .context("failed to list shared assets")?;
    Ok(rows
        .into_iter()
        .map(|row| shared_asset_record_from_row(&row))
        .collect())
}

pub async fn get_shared_asset(client: &Client, shared_asset_id: Uuid) -> Result<SharedAssetRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                sa.shared_asset_id,
                w.code,
                sa.code,
                sa.display_name,
                sa.asset_kind,
                source.code,
                tp.code,
                sa.source_kind,
                sa.source_event_ids,
                sa.artifact_refs,
                sa.message_refs,
                sa.evidence_span,
                sa.derivation_kind,
                sa.schema_version,
                sa.visibility_scope,
                sa.status
            FROM ami.shared_assets sa
            INNER JOIN ami.workspaces w ON w.workspace_id = sa.workspace_id
            LEFT JOIN ami.projects source ON source.project_id = sa.source_project_id
            LEFT JOIN ami.transfer_policies tp ON tp.transfer_policy_id = sa.transfer_policy_id
            WHERE sa.shared_asset_id = $1
            "#,
            &[&shared_asset_id],
        )
        .await
        .with_context(|| format!("failed to load shared asset {}", shared_asset_id))?;
    Ok(shared_asset_record_from_row(&row))
}

#[derive(Debug, Clone)]
struct SharedAssetPolicyScopeFilter {
    workspace_code: String,
    asset_code: String,
    visibility_scope: String,
    source_project_present: bool,
    source_project_workspace_matches: bool,
    transfer_policy_code: Option<String>,
    transfer_policy_present: bool,
    transfer_policy_workspace_matches: bool,
    transfer_policy_allows_cross_project_read: bool,
    transfer_policy_required: bool,
    scope_allowed: bool,
}

#[derive(Debug, Clone)]
struct SharedAssetVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

#[derive(Debug, Clone)]
struct SharedAssetBindingPolicyScopeFilter {
    asset_code: String,
    project_code: String,
    asset_workspace_code: String,
    project_workspace_code: String,
    binding_kind: String,
    workspace_match: bool,
    scope_allowed: bool,
}

#[derive(Debug, Clone)]
struct SharedAssetBindingVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

fn shared_asset_marks_poisoned(evidence_span: &Value) -> bool {
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

fn run_shared_asset_policy_scope_filter(
    workspace_code: &str,
    asset_code: &str,
    visibility_scope: &str,
    source_project_present: bool,
    source_project_workspace_matches: bool,
    transfer_policy: Option<&TransferPolicyRecord>,
) -> SharedAssetPolicyScopeFilter {
    let transfer_policy_required = matches!(
        visibility_scope,
        "cross_project_linked" | "org_global" | "imported"
    );
    let transfer_policy_present = transfer_policy.is_some();
    let transfer_policy_workspace_matches = transfer_policy
        .as_ref()
        .is_none_or(|policy| policy.workspace_code == workspace_code);
    let transfer_policy_allows_cross_project_read = transfer_policy
        .as_ref()
        .is_none_or(|policy| policy.allow_cross_project_read);
    let scope_allowed = (!source_project_present || source_project_workspace_matches)
        && transfer_policy_workspace_matches
        && (!transfer_policy_required || transfer_policy_present)
        && (!transfer_policy_present || transfer_policy_allows_cross_project_read);
    SharedAssetPolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        asset_code: asset_code.to_string(),
        visibility_scope: visibility_scope.to_string(),
        source_project_present,
        source_project_workspace_matches,
        transfer_policy_code: transfer_policy.as_ref().map(|policy| policy.code.clone()),
        transfer_policy_present,
        transfer_policy_workspace_matches,
        transfer_policy_allows_cross_project_read,
        transfer_policy_required,
        scope_allowed,
    }
}

fn validate_shared_asset_policy_scope_filter(filter: &SharedAssetPolicyScopeFilter) -> Result<()> {
    if filter.source_project_present && !filter.source_project_workspace_matches {
        return Err(anyhow!(
            "shared asset {} requires source_project to belong to workspace {}",
            filter.asset_code,
            filter.workspace_code
        ));
    }
    if filter.transfer_policy_present && !filter.transfer_policy_workspace_matches {
        return Err(anyhow!(
            "shared asset {} requires transfer_policy {:?} to belong to workspace {}",
            filter.asset_code,
            filter.transfer_policy_code,
            filter.workspace_code
        ));
    }
    if filter.transfer_policy_required && !filter.transfer_policy_present {
        return Err(anyhow!(
            "shared asset {} with visibility_scope={} requires transfer_policy",
            filter.asset_code,
            filter.visibility_scope
        ));
    }
    if filter.transfer_policy_present && !filter.transfer_policy_allows_cross_project_read {
        return Err(anyhow!(
            "shared asset {} is blocked because transfer_policy {:?} disallows cross-project read",
            filter.asset_code,
            filter.transfer_policy_code,
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "shared asset {} failed policy/scope filter before truth write",
            filter.asset_code
        ));
    }
    Ok(())
}

fn run_shared_asset_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    scope_filter: &SharedAssetPolicyScopeFilter,
) -> SharedAssetVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = shared_asset_marks_poisoned(evidence_span);
    let write_allowed = evidence_present && !poisoned_detected && scope_filter.scope_allowed;
    SharedAssetVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed,
    }
}

fn validate_shared_asset_verification_conflict_check(
    check: &SharedAssetVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "shared asset is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "shared asset must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "shared asset failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

fn augment_shared_asset_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &SharedAssetPolicyScopeFilter,
    verification_check: &SharedAssetVerificationConflictCheck,
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
                "asset_code": policy_filter.asset_code,
                "visibility_scope": policy_filter.visibility_scope,
                "source_project_present": policy_filter.source_project_present,
                "source_project_workspace_matches": policy_filter.source_project_workspace_matches,
                "transfer_policy_code": policy_filter.transfer_policy_code,
                "transfer_policy_present": policy_filter.transfer_policy_present,
                "transfer_policy_workspace_matches": policy_filter.transfer_policy_workspace_matches,
                "transfer_policy_allows_cross_project_read": policy_filter.transfer_policy_allows_cross_project_read,
                "transfer_policy_required": policy_filter.transfer_policy_required,
                "scope_allowed": policy_filter.scope_allowed,
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

fn run_shared_asset_binding_policy_scope_filter(
    asset_code: &str,
    project_code: &str,
    asset_workspace_code: &str,
    project_workspace_code: &str,
    binding_kind: &str,
) -> SharedAssetBindingPolicyScopeFilter {
    let workspace_match = asset_workspace_code == project_workspace_code;
    SharedAssetBindingPolicyScopeFilter {
        asset_code: asset_code.to_string(),
        project_code: project_code.to_string(),
        asset_workspace_code: asset_workspace_code.to_string(),
        project_workspace_code: project_workspace_code.to_string(),
        binding_kind: binding_kind.to_string(),
        workspace_match,
        scope_allowed: workspace_match,
    }
}

fn validate_shared_asset_binding_policy_scope_filter(
    filter: &SharedAssetBindingPolicyScopeFilter,
) -> Result<()> {
    if !filter.workspace_match {
        return Err(anyhow!(
            "shared asset {} cannot bind {} across workspaces ({} != {})",
            filter.asset_code,
            filter.project_code,
            filter.asset_workspace_code,
            filter.project_workspace_code
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "shared asset {} failed binding policy/scope filter before truth write",
            filter.asset_code
        ));
    }
    Ok(())
}

fn run_shared_asset_binding_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    scope_filter: &SharedAssetBindingPolicyScopeFilter,
) -> SharedAssetBindingVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || !source_event_ids.as_array().unwrap_or(&vec![]).is_empty()
        || !artifact_refs.as_array().unwrap_or(&vec![]).is_empty()
        || !message_refs.as_array().unwrap_or(&vec![]).is_empty()
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty());
    let poisoned_detected = shared_asset_marks_poisoned(evidence_span);
    let write_allowed = evidence_present && !poisoned_detected && scope_filter.scope_allowed;
    SharedAssetBindingVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed,
    }
}

fn validate_shared_asset_binding_verification_conflict_check(
    check: &SharedAssetBindingVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "shared asset binding is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "shared asset binding must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "shared asset binding failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

fn augment_shared_asset_binding_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &SharedAssetBindingPolicyScopeFilter,
    verification_check: &SharedAssetBindingVerificationConflictCheck,
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
                "asset_code": policy_filter.asset_code,
                "project_code": policy_filter.project_code,
                "asset_workspace_code": policy_filter.asset_workspace_code,
                "project_workspace_code": policy_filter.project_workspace_code,
                "binding_kind": policy_filter.binding_kind,
                "workspace_match": policy_filter.workspace_match,
                "scope_allowed": policy_filter.scope_allowed,
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
