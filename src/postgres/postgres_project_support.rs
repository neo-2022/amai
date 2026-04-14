use super::*;

pub async fn find_agent_display_name_by_code(
    client: &Client,
    code: &str,
) -> Result<Option<String>> {
    let code = code.trim();
    if code.is_empty() {
        return Ok(None);
    }
    let row = client
        .query_opt(
            r#"
            SELECT NULLIF(TRIM(display_name), '')
            FROM ami.agents
            WHERE code = $1
            "#,
            &[&code],
        )
        .await
        .with_context(|| format!("failed to lookup agent display_name for code {code}"))?;
    Ok(row.and_then(|row| row.get::<_, Option<String>>(0)))
}

pub async fn upsert_agent_display_name_by_code(
    client: &Client,
    code: &str,
    display_name: &str,
) -> Result<()> {
    let code = code.trim();
    let display_name = display_name.trim();
    if code.is_empty() {
        return Err(anyhow!("agent code must not be empty"));
    }
    if display_name.is_empty() {
        return Err(anyhow!("agent display_name must not be empty"));
    }
    let metadata = json!({
        "updated_via": "dashboard_active_agent_label",
        "user_defined_display_name": true,
    });
    let default_workspace = get_workspace_by_code(client, "default").await?;
    client
        .execute(
            r#"
            INSERT INTO ami.agents(code, display_name, workspace_id, visibility_scope, status, metadata)
            VALUES ($1, $2, $3, 'agent_private', 'active', $4::jsonb)
            ON CONFLICT (code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                workspace_id = EXCLUDED.workspace_id,
                visibility_scope = EXCLUDED.visibility_scope,
                status = EXCLUDED.status,
                metadata = COALESCE(ami.agents.metadata, '{}'::jsonb) || EXCLUDED.metadata
            "#,
            &[&code, &display_name, &default_workspace.workspace_id, &metadata],
        )
        .await
        .with_context(|| format!("failed to upsert agent display_name for code {code}"))?;
    Ok(())
}

pub(super) async fn get_bound_project_for_repo_root(
    client: &Client,
    canonical_repo_root: &str,
) -> Result<Option<ProjectRecord>> {
    let row = client
        .query_opt(
            r#"
            SELECT
                p.project_id,
                p.code,
                p.display_name,
                r.repo_root,
                p.visibility_scope,
                to_char(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.project_repo_roots r
            INNER JOIN ami.projects p ON p.project_id = r.project_id
            WHERE r.repo_root = $1
            "#,
            &[&canonical_repo_root],
        )
        .await?;
    Ok(row.as_ref().map(project_record_from_row))
}

pub(super) async fn get_project_workspace_id(client: &Client, project_id: Uuid) -> Result<Uuid> {
    let row = client
        .query_one(
            r#"
            SELECT workspace_id
            FROM ami.projects
            WHERE project_id = $1
            "#,
            &[&project_id],
        )
        .await
        .context("failed to lookup project workspace_id")?;
    Ok(row.get(0))
}

async fn ensure_project_repo_root_binding(
    client: &Client,
    project: &ProjectRecord,
    repo_root: &str,
    root_kind: &str,
) -> Result<()> {
    if let Some(existing) = get_bound_project_for_repo_root(client, repo_root).await? {
        if existing.project_id != project.project_id {
            return Err(anyhow!(
                "canonical repo_root {} is already bound to project {} (display_name: {}); project {} cannot claim it",
                repo_root,
                existing.code,
                existing.display_name,
                project.code
            ));
        }
    }

    client
        .execute(
            r#"
            INSERT INTO ami.project_repo_roots(project_id, repo_root, root_kind)
            VALUES ($1, $2, $3)
            ON CONFLICT (repo_root) DO UPDATE SET
                root_kind = EXCLUDED.root_kind,
                updated_at = now()
            "#,
            &[&project.project_id, &repo_root, &root_kind],
        )
        .await
        .context("failed to bind project repo_root alias")?;
    Ok(())
}

pub(super) async fn sync_project_repo_roots(
    client: &Client,
    project: &ProjectRecord,
    previous_repo_root: Option<&str>,
) -> Result<()> {
    client
        .execute(
            r#"
            UPDATE ami.project_repo_roots
            SET root_kind = 'relocated_from',
                updated_at = now()
            WHERE project_id = $1
              AND repo_root <> $2
              AND root_kind = 'primary'
            "#,
            &[&project.project_id, &project.repo_root],
        )
        .await
        .context("failed to demote previous primary repo_root aliases")?;
    ensure_project_repo_root_binding(client, project, &project.repo_root, "primary").await?;
    if let Some(previous_repo_root) = previous_repo_root {
        if previous_repo_root != project.repo_root {
            ensure_project_repo_root_binding(client, project, previous_repo_root, "relocated_from")
                .await?;
        }
    }
    Ok(())
}

pub(super) async fn ensure_app_role(client: &Client, cfg: &AppConfig) -> Result<()> {
    let user = sql_ident(&cfg.app_db_user)?;
    let db = sql_ident(&cfg.pg_db)?;
    let password = sql_literal(&cfg.app_db_password);
    let role_sql = format!(
        r#"
        DO $$
        BEGIN
            IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{raw_user}') THEN
                CREATE ROLE {user} LOGIN PASSWORD {password};
            ELSE
                ALTER ROLE {user} LOGIN PASSWORD {password};
            END IF;
        END
        $$;

        GRANT CONNECT ON DATABASE {db} TO {user};
        GRANT USAGE ON SCHEMA ami TO {user};
        REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON ALL TABLES IN SCHEMA ami FROM {user};
        REVOKE USAGE, UPDATE ON ALL SEQUENCES IN SCHEMA ami FROM {user};
        GRANT SELECT ON ALL TABLES IN SCHEMA ami TO {user};
        GRANT SELECT ON ALL SEQUENCES IN SCHEMA ami TO {user};
        ALTER DEFAULT PRIVILEGES IN SCHEMA ami REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON TABLES FROM {user};
        ALTER DEFAULT PRIVILEGES IN SCHEMA ami REVOKE USAGE, UPDATE ON SEQUENCES FROM {user};
        ALTER DEFAULT PRIVILEGES IN SCHEMA ami GRANT SELECT ON TABLES TO {user};
        ALTER DEFAULT PRIVILEGES IN SCHEMA ami GRANT SELECT ON SEQUENCES TO {user};
        "#,
        raw_user = cfg.app_db_user.replace('\'', "''"),
    );
    client
        .batch_execute(&role_sql)
        .await
        .context("failed to create/grant app role")?;
    Ok(())
}

pub(super) async fn record_scope_override_event(
    client: &Client,
    workspace_id: Uuid,
    entity_kind: &str,
    entity_id: Uuid,
    actor_agent_id: Option<Uuid>,
    event_kind: &str,
    reason: &str,
    details: &Value,
) -> Result<()> {
    client
        .execute(
            r#"
            INSERT INTO ami.scope_override_events(
                workspace_id,
                entity_kind,
                entity_id,
                actor_agent_id,
                event_kind,
                reason,
                details
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
            "#,
            &[
                &workspace_id,
                &entity_kind,
                &entity_id,
                &actor_agent_id,
                &event_kind,
                &reason,
                details,
            ],
        )
        .await
        .context("failed to record scope override event")?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AccessPolicyAction {
    Read,
    Import,
    Promote,
    ApproveTransfer,
}

impl AccessPolicyAction {
    fn column_name(self) -> &'static str {
        match self {
            AccessPolicyAction::Read => "can_read",
            AccessPolicyAction::Import => "can_import",
            AccessPolicyAction::Promote => "can_promote",
            AccessPolicyAction::ApproveTransfer => "can_approve_transfer",
        }
    }
}

async fn active_access_policy_grants(
    client: &Client,
    workspace_id: Uuid,
    project_id: Uuid,
    object_class: &str,
    allowed_scope_types: &[&str],
    action: AccessPolicyAction,
) -> Result<bool> {
    if allowed_scope_types.is_empty() {
        return Ok(false);
    }
    let rows = client
        .query(
            r#"
            SELECT
                ap.scope_type,
                ap.precedence,
                ap.can_read,
                ap.can_import,
                ap.can_promote,
                ap.can_approve_transfer,
                ap.can_quarantine
            FROM ami.access_policies ap
            WHERE ap.workspace_id = $1
              AND ap.status = 'active'
              AND ap.object_class = $2
              AND ap.scope_type = ANY($3::text[])
              AND (ap.project_id = $4 OR ap.project_id IS NULL)
            ORDER BY
                CASE WHEN ap.project_id = $4 THEN 0 ELSE 1 END,
                ap.precedence DESC,
                ap.created_at DESC
            "#,
            &[&workspace_id, &object_class, &allowed_scope_types, &project_id],
        )
        .await
        .with_context(|| {
            format!(
                "failed to evaluate access policies for workspace={workspace_id} project={project_id} object_class={object_class} action={}",
                action.column_name()
            )
        })?;
    let Some(row) = rows.first() else {
        return Ok(false);
    };
    let allowed = match action {
        AccessPolicyAction::Read => row.get::<_, bool>(2),
        AccessPolicyAction::Import => row.get::<_, bool>(3),
        AccessPolicyAction::Promote => row.get::<_, bool>(4),
        AccessPolicyAction::ApproveTransfer => row.get::<_, bool>(5),
    };
    Ok(allowed)
}

pub(super) async fn ensure_cross_project_policy_access(
    client: &Client,
    workspace_id: Uuid,
    project_id: Uuid,
    object_class: &str,
    allowed_scope_types: &[&str],
    action: AccessPolicyAction,
    context_label: &str,
) -> Result<()> {
    if active_access_policy_grants(
        client,
        workspace_id,
        project_id,
        object_class,
        allowed_scope_types,
        action,
    )
    .await?
    {
        return Ok(());
    }
    Err(anyhow!(
        "default_deny: no active access policy grants {} for {} on project {} with scopes [{}]",
        action.column_name(),
        context_label,
        project_id,
        allowed_scope_types.join(", ")
    ))
}

pub async fn project_allows_cross_project_read(
    client: &Client,
    project_id: Uuid,
    visibility_scope: &str,
) -> Result<bool> {
    let workspace_id = get_project_workspace_id(client, project_id).await?;
    let mut allowed_scope_types = vec!["org_global"];
    if visibility_scope == "cross_project_linked" {
        allowed_scope_types.insert(0, "cross_project_linked");
    } else if visibility_scope == "org_global" {
        allowed_scope_types = vec!["org_global"];
    }
    active_access_policy_grants(
        client,
        workspace_id,
        project_id,
        "fact",
        &allowed_scope_types,
        AccessPolicyAction::Read,
    )
    .await
}

pub(super) async fn find_project_link_context(
    client: &Client,
    source_project_id: Uuid,
    target_project_id: Uuid,
) -> Result<Option<(Uuid, String, bool, Option<Uuid>, Uuid)>> {
    let row = client
        .query_opt(
            r#"
            SELECT
                r.relation_id,
                r.project_link_type,
                r.requires_approval,
                r.transfer_policy_id,
                source.workspace_id
            FROM ami.project_relations r
            INNER JOIN ami.projects source ON source.project_id = r.source_project_id
            WHERE r.source_project_id = $1
              AND r.target_project_id = $2
              AND r.relation_status = 'active'
              AND r.project_link_type <> 'forbidden_transfer'
            ORDER BY r.created_at DESC
            LIMIT 1
            "#,
            &[&source_project_id, &target_project_id],
        )
        .await
        .context("failed to lookup project link context")?;
    Ok(row.map(|row| (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))))
}

pub(super) fn string_array_json(items: &[String]) -> Value {
    Value::Array(items.iter().cloned().map(Value::String).collect())
}

pub(super) fn value_string_array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map_or(0, Vec::len)
}

pub(super) fn current_epoch_ms() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64)
}

pub(super) fn maybe_rfc3339_utc(row: &Row, idx: usize) -> Option<String> {
    row.get::<_, Option<String>>(idx)
}

pub(super) fn memory_card_record_from_row(row: &Row) -> MemoryCardRecord {
    MemoryCardRecord {
        memory_card_id: row.get(0),
        project_code: row.get(1),
        namespace_code: row.get(2),
        title: row.get(3),
        summary: row.get(4),
        body: row.get(5),
        tags: row.get(6),
        provenance: row.get(7),
        fact_subject: row.get(8),
        fact_predicate: row.get(9),
        fact_object: row.get(10),
        truth_state: row.get(11),
        verification_state: row.get(12),
        status: row.get(13),
        derivation_kind: row.get(14),
        candidate_class: row.get(15),
        source_kind: row.get(16),
        hot_path_write_eligible: row.get(17),
        background_consolidation_recommended: row.get(18),
        observed_at_epoch_ms: row.get(19),
        recorded_at_epoch_ms: row.get(20),
        valid_from_epoch_ms: row.get(21),
        valid_to_epoch_ms: row.get(22),
        last_verified_at_epoch_ms: row.get(23),
        superseded_by_memory_card_id: row.get(24),
        created_at: maybe_rfc3339_utc(row, 25),
    }
}

#[derive(Debug, Clone)]
pub(super) struct MemoryCardCandidateExtraction {
    pub(super) source_basis_status: String,
    pub(super) source_event_count: usize,
    pub(super) artifact_ref_count: usize,
    pub(super) message_ref_count: usize,
    pub(super) has_evidence_span: bool,
    pub(super) derivation_kind: String,
    pub(super) candidate_class: String,
    pub(super) source_kind: Option<String>,
    pub(super) hot_path_write_eligible: bool,
    pub(super) background_consolidation_recommended: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryCardPolicyScopeFilter {
    pub(super) visibility_scope: String,
    pub(super) sensitivity_class: String,
    pub(super) project_code: String,
    pub(super) namespace_code: String,
    pub(super) owner_agent_required: bool,
    pub(super) owner_agent_present: bool,
    pub(super) private_contour_violation: bool,
    pub(super) scope_allowed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryCardVerificationConflictCheck {
    pub(super) evidence_present: bool,
    pub(super) current_truth_conflict: bool,
    pub(super) poisoned_detected: bool,
    pub(super) private_contour_violation: bool,
    pub(super) truth_state: String,
    pub(super) verification_state: String,
    pub(super) status: String,
    pub(super) write_allowed: bool,
}

pub(super) fn extract_memory_card_candidate(
    title: &str,
    tags: &[String],
    provenance: &Value,
    fact_subject: Option<&str>,
    fact_predicate: Option<&str>,
    fact_object: Option<&str>,
) -> MemoryCardCandidateExtraction {
    let source_event_count = value_string_array_len(provenance.get("source_event_ids"));
    let artifact_ref_count = value_string_array_len(provenance.get("artifact_refs"));
    let message_ref_count = value_string_array_len(provenance.get("message_refs"));
    let has_evidence_span = provenance
        .get("evidence_span")
        .and_then(Value::as_object)
        .is_some_and(|span| !span.is_empty());
    let source_basis_status = if source_event_count > 0
        || artifact_ref_count > 0
        || message_ref_count > 0
        || has_evidence_span
    {
        "recorded"
    } else {
        "missing"
    }
    .to_string();
    let derivation_kind = provenance
        .get("derivation_kind")
        .and_then(Value::as_str)
        .unwrap_or("extract")
        .to_string();
    let candidate_class = canonical_candidate_class_from_hints(
        provenance.get("candidate_class").and_then(Value::as_str),
        None,
        Some(title),
        tags,
        fact_subject.is_some() && fact_predicate.is_some() && fact_object.is_some(),
        "fact",
    );
    let source_kind = provenance
        .get("source_kind")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            if source_event_count > 0 {
                Some("raw_event_append".to_string())
            } else if artifact_ref_count > 0 {
                Some("artifact_basis".to_string())
            } else if message_ref_count > 0 {
                Some("message_basis".to_string())
            } else if has_evidence_span {
                Some("evidence_span_basis".to_string())
            } else {
                None
            }
        });
    let (hot_path_write_eligible, background_consolidation_recommended) =
        runtime_contract_for_candidate_class(&candidate_class, &derivation_kind);
    MemoryCardCandidateExtraction {
        source_basis_status,
        source_event_count,
        artifact_ref_count,
        message_ref_count,
        has_evidence_span,
        derivation_kind,
        candidate_class,
        source_kind,
        hot_path_write_eligible,
        background_consolidation_recommended,
    }
}

pub(super) fn validate_memory_card_candidate(
    candidate: &MemoryCardCandidateExtraction,
) -> Result<()> {
    if candidate.derivation_kind != "operator_write" && candidate.source_basis_status != "recorded"
    {
        return Err(anyhow!(
            "memory card candidate requires recorded provenance basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn run_memory_card_policy_scope_filter(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    provenance: &Value,
) -> MemoryCardPolicyScopeFilter {
    let visibility_scope = project.visibility_scope.clone();
    let sensitivity_class = provenance
        .get("sensitivity_class")
        .and_then(Value::as_str)
        .unwrap_or("internal")
        .to_string();
    let owner_agent_required = visibility_scope == "agent_private";
    let owner_agent_present = provenance
        .get("owner_agent_code")
        .and_then(Value::as_str)
        .is_some()
        || provenance
            .get("owner_agent_id")
            .and_then(Value::as_str)
            .is_some();
    let private_contour_violation = owner_agent_required && !owner_agent_present;
    let scope_allowed = !private_contour_violation;
    MemoryCardPolicyScopeFilter {
        visibility_scope,
        sensitivity_class,
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        owner_agent_required,
        owner_agent_present,
        private_contour_violation,
        scope_allowed,
    }
}

pub(super) fn validate_memory_card_policy_scope_filter(
    filter: &MemoryCardPolicyScopeFilter,
) -> Result<()> {
    if filter.visibility_scope == "quarantine" {
        return Err(anyhow!(
            "memory card violates scope filter: visibility_scope=quarantine requires dedicated quarantine_item path"
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "memory card violates scope filter: visibility_scope={} requires owner_agent binding in provenance",
            filter.visibility_scope
        ));
    }
    Ok(())
}

pub(super) fn provenance_marks_memory_card_poisoned(provenance: &Value) -> bool {
    provenance
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || provenance
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub(super) async fn run_memory_card_verification_conflict_check(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    candidate: &MemoryCardCandidateExtraction,
    provenance: &Value,
    fact_subject: Option<&str>,
    fact_predicate: Option<&str>,
    fact_object: Option<&str>,
    truth_state: Option<&str>,
    verification_state: Option<&str>,
    status: Option<&str>,
    scope_filter: &MemoryCardPolicyScopeFilter,
) -> Result<MemoryCardVerificationConflictCheck> {
    let evidence_present = candidate.derivation_kind == "operator_write"
        || candidate.source_basis_status == "recorded";
    let poisoned_detected = provenance_marks_memory_card_poisoned(provenance);
    let current_truth_conflict = if truth_state == Some("current")
        && status.unwrap_or("active") == "active"
        && fact_subject.is_some()
        && fact_predicate.is_some()
        && fact_object.is_some()
    {
        let row = client
            .query_one(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM ami.memory_cards mc
                    WHERE mc.project_id = $1
                      AND mc.namespace_id = $2
                      AND mc.fact_subject = $3
                      AND mc.fact_predicate = $4
                      AND mc.fact_object = $5
                      AND mc.truth_state = 'current'
                      AND mc.status = 'active'
                      AND mc.superseded_by_memory_card_id IS NULL
                )
                "#,
                &[
                    &project.project_id,
                    &namespace.namespace_id,
                    &fact_subject,
                    &fact_predicate,
                    &fact_object,
                ],
            )
            .await
            .context("failed to check memory card current-truth conflict")?;
        row.get::<_, bool>(0)
    } else {
        false
    };
    let truth_state_value = truth_state.unwrap_or("current").to_string();
    let verification_state_value = verification_state.unwrap_or("raw").to_string();
    let status_value = status.unwrap_or("active").to_string();
    let write_allowed = evidence_present
        && !poisoned_detected
        && !current_truth_conflict
        && !scope_filter.private_contour_violation;
    Ok(MemoryCardVerificationConflictCheck {
        evidence_present,
        current_truth_conflict,
        poisoned_detected,
        private_contour_violation: scope_filter.private_contour_violation,
        truth_state: truth_state_value,
        verification_state: verification_state_value,
        status: status_value,
        write_allowed,
    })
}

pub(super) fn validate_memory_card_verification_conflict_check(
    check: &MemoryCardVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "memory card is flagged poisoned by provenance and cannot be written"
        ));
    }
    if check.current_truth_conflict {
        return Err(anyhow!(
            "memory card conflicts with existing current active truth for the same fact triple"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "memory card must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory card failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn augment_memory_card_provenance_with_stage2_preflight(
    provenance: &Value,
    candidate: &MemoryCardCandidateExtraction,
    scope_filter: &MemoryCardPolicyScopeFilter,
    verification_check: &MemoryCardVerificationConflictCheck,
) -> Value {
    let mut object = match provenance {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_provenance".to_string(), provenance.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "candidate_class": candidate.candidate_class,
            "source_kind": candidate.source_kind,
            "source_basis_status": candidate.source_basis_status,
            "hot_path_write_eligible": candidate.hot_path_write_eligible,
            "background_consolidation_recommended": candidate.background_consolidation_recommended,
            "policy_and_scope_filter": {
                "visibility_scope": scope_filter.visibility_scope,
                "sensitivity_class": scope_filter.sensitivity_class,
                "project_code": scope_filter.project_code,
                "namespace_code": scope_filter.namespace_code,
                "owner_agent_required": scope_filter.owner_agent_required,
                "owner_agent_present": scope_filter.owner_agent_present,
                "private_contour_violation": scope_filter.private_contour_violation,
                "scope_allowed": scope_filter.scope_allowed,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "current_truth_conflict": verification_check.current_truth_conflict,
                "poisoned_detected": verification_check.poisoned_detected,
                "private_contour_violation": verification_check.private_contour_violation,
                "truth_state": verification_check.truth_state,
                "verification_state": verification_check.verification_state,
                "status": verification_check.status,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

const MEMORY_CARD_ALLOWED_TRUTH_STATES: &[&str] = &[
    "current",
    "superseded",
    "conflicted",
    "retracted",
    "unverified",
];
const MEMORY_CARD_ALLOWED_VERIFICATION_STATES: &[&str] = &[
    "raw",
    "proposed",
    "verified",
    "disputed",
    "deprecated",
    "quarantined",
];
const MEMORY_CARD_ALLOWED_STATUS_STATES: &[&str] =
    &["active", "inactive", "superseded", "archived"];

fn validate_memory_card_state_value(
    field_name: &str,
    value: Option<&str>,
    allowed_values: &[&str],
    operation_name: &str,
) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if allowed_values.contains(&value) {
        return Ok(());
    }
    Err(anyhow!(
        "invalid memory card {} '{}' for {}; allowed values: {}",
        field_name,
        value,
        operation_name,
        allowed_values.join(", ")
    ))
}

pub(super) fn validate_memory_card_runtime_states(
    truth_state: Option<&str>,
    verification_state: Option<&str>,
    status: Option<&str>,
    operation_name: &str,
) -> Result<()> {
    validate_memory_card_state_value(
        "truth_state",
        truth_state,
        MEMORY_CARD_ALLOWED_TRUTH_STATES,
        operation_name,
    )?;
    validate_memory_card_state_value(
        "verification_state",
        verification_state,
        MEMORY_CARD_ALLOWED_VERIFICATION_STATES,
        operation_name,
    )?;
    validate_memory_card_state_value(
        "status",
        status,
        MEMORY_CARD_ALLOWED_STATUS_STATES,
        operation_name,
    )?;
    Ok(())
}

pub(super) fn task_node_source_event_ids_json(record: &TaskNodeInsert<'_>) -> Value {
    if let Some(value) = record.source_event_ids {
        return value.clone();
    }
    if let Some(items) = record
        .status_payload
        .get("source_event_ids")
        .and_then(Value::as_array)
    {
        return Value::Array(items.clone());
    }
    if let Some(source_event_id) = record
        .status_payload
        .get("source_event_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return json!([source_event_id]);
    }
    Value::Array(Vec::new())
}

pub(super) fn task_node_artifact_refs_json(record: &TaskNodeInsert<'_>) -> Value {
    if let Some(value) = record.artifact_refs {
        return value.clone();
    }
    if let Some(items) = record
        .status_payload
        .get("artifact_refs")
        .and_then(Value::as_array)
    {
        return Value::Array(items.clone());
    }
    if let Some(items) = record
        .metadata
        .get("artifact_refs")
        .and_then(Value::as_array)
    {
        return Value::Array(items.clone());
    }
    if let Some(local_path) = record
        .metadata
        .get("local_path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return json!([format!("file://{local_path}")]);
    }
    Value::Array(Vec::new())
}

pub(super) fn task_node_evidence_span_json(record: &TaskNodeInsert<'_>) -> Value {
    if let Some(value) = record.evidence_span {
        return value.clone();
    }
    if let Some(value) = record.status_payload.get("evidence_span") {
        return value.clone();
    }
    if let Some(value) = record.metadata.get("evidence_span") {
        return value.clone();
    }
    json!({
        "source_event_id": record.status_payload.get("source_event_id").cloned().unwrap_or(Value::Null),
        "source_snapshot_id": record.status_payload.get("source_snapshot_id").cloned().unwrap_or(Value::Null),
    })
}

pub(super) fn extract_task_node_candidate(
    record: &TaskNodeInsert<'_>,
    source_event_ids: &Value,
    artifact_refs: &Value,
    evidence_span: &Value,
) -> TaskNodeCandidateExtraction {
    let source_event_count = value_string_array_len(Some(source_event_ids));
    let artifact_ref_count = value_string_array_len(Some(artifact_refs));
    let has_evidence_span = evidence_span
        .as_object()
        .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let source_basis_status =
        if source_event_count > 0 || artifact_ref_count > 0 || has_evidence_span {
            "recorded"
        } else {
            "missing"
        }
        .to_string();
    let derivation_kind = record.derivation_kind.unwrap_or("extract").to_string();
    let item_kind_hint = match record.task_role.unwrap_or("workline") {
        "proposal" | "commitment" | "workline" => Some("commitment"),
        "decision" => Some("decision"),
        _ => Some("task"),
    };
    let candidate_class = canonical_candidate_class_from_hints(
        record
            .metadata
            .get("candidate_class")
            .and_then(Value::as_str)
            .or_else(|| {
                record
                    .status_payload
                    .get("candidate_class")
                    .and_then(Value::as_str)
            }),
        item_kind_hint,
        Some(record.headline),
        &[],
        false,
        "commitment",
    );
    let source_kind = record
        .status_payload
        .get("source_kind")
        .and_then(Value::as_str)
        .or_else(|| record.metadata.get("source_kind").and_then(Value::as_str))
        .map(ToString::to_string)
        .or_else(|| {
            if source_event_count > 0 {
                Some("raw_event_append".to_string())
            } else if artifact_ref_count > 0 {
                Some("artifact_basis".to_string())
            } else if has_evidence_span {
                Some("evidence_span_basis".to_string())
            } else {
                None
            }
        });
    let (hot_path_write_eligible, background_consolidation_recommended) =
        runtime_contract_for_candidate_class(&candidate_class, &derivation_kind);
    TaskNodeCandidateExtraction {
        source_basis_status,
        source_event_count,
        artifact_ref_count,
        has_evidence_span,
        candidate_class,
        derivation_kind,
        source_kind,
        hot_path_write_eligible,
        background_consolidation_recommended,
    }
}

pub(super) fn validate_task_node_candidate(candidate: &TaskNodeCandidateExtraction) -> Result<()> {
    if candidate.derivation_kind != "operator_write" && candidate.source_basis_status != "recorded"
    {
        return Err(anyhow!(
            "task node candidate requires recorded provenance basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn run_task_node_policy_scope_filter(
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &TaskNodeInsert<'_>,
) -> TaskNodePolicyScopeFilter {
    let visibility_scope = project.visibility_scope.clone();
    let owner_agent_required = visibility_scope == "agent_private";
    let owner_agent_present = record
        .metadata
        .get("owner_agent_code")
        .and_then(Value::as_str)
        .is_some()
        || record
            .metadata
            .get("owner_agent_id")
            .and_then(Value::as_str)
            .is_some()
        || record
            .status_payload
            .get("owner_agent_code")
            .and_then(Value::as_str)
            .is_some()
        || record
            .status_payload
            .get("owner_agent_id")
            .and_then(Value::as_str)
            .is_some();
    let private_contour_violation = owner_agent_required && !owner_agent_present;
    let scope_allowed = !private_contour_violation;
    TaskNodePolicyScopeFilter {
        visibility_scope,
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        owner_agent_required,
        owner_agent_present,
        private_contour_violation,
        scope_allowed,
    }
}

pub(super) fn validate_task_node_policy_scope_filter(
    filter: &TaskNodePolicyScopeFilter,
) -> Result<()> {
    if filter.visibility_scope == "quarantine" {
        return Err(anyhow!(
            "task node violates scope filter: visibility_scope=quarantine requires dedicated quarantine_item path"
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "task node violates scope filter: visibility_scope={} requires owner_agent binding",
            filter.visibility_scope
        ));
    }
    Ok(())
}

pub(super) fn task_node_marks_poisoned(record: &TaskNodeInsert<'_>) -> bool {
    record
        .metadata
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || record
            .metadata
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || record
            .status_payload
            .get("poisoned")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || record
            .status_payload
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub(super) async fn run_task_node_verification_conflict_check(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &TaskNodeInsert<'_>,
    candidate: &TaskNodeCandidateExtraction,
    scope_filter: &TaskNodePolicyScopeFilter,
) -> Result<TaskNodeVerificationConflictCheck> {
    let evidence_present = candidate.derivation_kind == "operator_write"
        || candidate.source_basis_status == "recorded";
    let poisoned_detected = task_node_marks_poisoned(record);
    let duplicate_task_key_conflict = if let Some(task_key) = record.task_key {
        client
            .query_one(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM ami.task_nodes tn
                    WHERE tn.project_id = $1
                      AND tn.namespace_id = $2
                      AND tn.task_key = $3
                )
                "#,
                &[&project.project_id, &namespace.namespace_id, &task_key],
            )
            .await
            .context("failed to check task node duplicate task_key conflict")?
            .get::<_, bool>(0)
    } else {
        false
    };
    let write_allowed = evidence_present
        && !poisoned_detected
        && !duplicate_task_key_conflict
        && !scope_filter.private_contour_violation;
    Ok(TaskNodeVerificationConflictCheck {
        evidence_present,
        duplicate_task_key_conflict,
        poisoned_detected,
        private_contour_violation: scope_filter.private_contour_violation,
        task_key: record.task_key.map(ToString::to_string),
        write_allowed,
    })
}

pub(super) fn validate_task_node_verification_conflict_check(
    check: &TaskNodeVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "task node is flagged poisoned by metadata/status payload and cannot be written"
        ));
    }
    if check.duplicate_task_key_conflict {
        return Err(anyhow!(
            "task node conflicts with existing task_key in the same namespace"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "task node must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "task node failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn augment_task_node_metadata_with_stage2_runtime(
    metadata: &Value,
    candidate: &TaskNodeCandidateExtraction,
    scope_filter: &TaskNodePolicyScopeFilter,
    verification_check: &TaskNodeVerificationConflictCheck,
) -> Value {
    let mut object = match metadata {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_metadata".to_string(), metadata.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "candidate_class": candidate.candidate_class,
            "source_kind": candidate.source_kind,
            "source_basis_status": candidate.source_basis_status,
            "hot_path_write_eligible": candidate.hot_path_write_eligible,
            "background_consolidation_recommended": candidate.background_consolidation_recommended,
            "source_event_count": candidate.source_event_count,
            "artifact_ref_count": candidate.artifact_ref_count,
            "has_evidence_span": candidate.has_evidence_span,
            "policy_and_scope_filter": {
                "visibility_scope": scope_filter.visibility_scope,
                "project_code": scope_filter.project_code,
                "namespace_code": scope_filter.namespace_code,
                "owner_agent_required": scope_filter.owner_agent_required,
                "owner_agent_present": scope_filter.owner_agent_present,
                "private_contour_violation": scope_filter.private_contour_violation,
                "scope_allowed": scope_filter.scope_allowed,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "duplicate_task_key_conflict": verification_check.duplicate_task_key_conflict,
                "duplicate_active_task_key_conflict": verification_check.duplicate_task_key_conflict,
                "poisoned_detected": verification_check.poisoned_detected,
                "private_contour_violation": verification_check.private_contour_violation,
                "task_key": verification_check.task_key,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}

pub(super) fn task_event_json_or_empty(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or_else(|| json!([]))
}

pub(super) fn canonical_task_event_kind(event_kind: &str) -> &str {
    match event_kind {
        // Operator/runtime wording may use "state_transition", but the
        // truth-layer stores the canonical event kind as "state_change".
        "state_transition" => "state_change",
        other => other,
    }
}

fn task_event_reopens_line(
    event_kind: &str,
    prior_lifecycle_state: Option<&str>,
    next_lifecycle_state: Option<&str>,
) -> bool {
    matches!(event_kind, "resumed" | "reopened")
        || matches!(prior_lifecycle_state, Some("closed" | "archived"))
            && matches!(next_lifecycle_state, Some("hot"))
}

#[derive(Debug, Clone)]
struct TaskNodeMaterializationState {
    parent_task_node_id: Option<Uuid>,
    execution_state: String,
    lifecycle_state: String,
}

async fn load_task_node_materialization_state(
    client: &Client,
    task_node_id: Uuid,
) -> Result<TaskNodeMaterializationState> {
    let row = client
        .query_opt(
            r#"
            SELECT
                parent_task_node_id,
                task_role,
                execution_state,
                lifecycle_state
            FROM ami.task_nodes
            WHERE task_node_id = $1
            "#,
            &[&task_node_id],
        )
        .await
        .with_context(|| format!("failed to load task node state {task_node_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("task node {task_node_id} not found"));
    };
    Ok(TaskNodeMaterializationState {
        parent_task_node_id: row.get(0),
        execution_state: row.get(2),
        lifecycle_state: row.get(3),
    })
}

pub(super) async fn refresh_task_node_rollups(client: &Client, task_node_id: Uuid) -> Result<()> {
    let row_count = client
        .execute(
            r#"
            WITH rollups AS (
                SELECT
                    COUNT(*)::integer AS child_count,
                    COUNT(*) FILTER (
                        WHERE lifecycle_state IN ('closed', 'archived', 'deprecated', 'quarantined')
                    )::integer AS closed_child_count,
                    COUNT(*) FILTER (
                        WHERE task_role = 'pending_return'
                    )::integer AS pending_return_count
                FROM ami.task_nodes
                WHERE parent_task_node_id = $1
            )
            UPDATE ami.task_nodes tn
            SET child_count = rollups.child_count,
                closed_child_count = rollups.closed_child_count,
                pending_return_count = rollups.pending_return_count,
                updated_at = now()
            FROM rollups
            WHERE tn.task_node_id = $1
            "#,
            &[&task_node_id],
        )
        .await
        .with_context(|| format!("failed to refresh task node rollups for {task_node_id}"))?;
    if row_count == 0 {
        return Err(anyhow!("task node {task_node_id} not found"));
    }
    Ok(())
}

async fn reparent_task_node(
    client: &Client,
    task_node_id: Uuid,
    new_parent_task_node_id: Option<Uuid>,
    next_task_role: Option<&str>,
    opened_at_epoch_ms: i64,
) -> Result<TaskNodeMaterializationState> {
    if Some(task_node_id) == new_parent_task_node_id {
        return Err(anyhow!("task node cannot become its own parent"));
    }
    let prior_state = load_task_node_materialization_state(client, task_node_id).await?;
    let row_count = client
        .execute(
            r#"
            UPDATE ami.task_nodes
            SET parent_task_node_id = $2,
                task_role = COALESCE($3, task_role),
                opened_at_epoch_ms = CASE
                    WHEN $2 IS DISTINCT FROM parent_task_node_id THEN COALESCE(opened_at_epoch_ms, $4)
                    ELSE opened_at_epoch_ms
                END,
                updated_at = now()
            WHERE task_node_id = $1
            "#,
            &[&task_node_id, &new_parent_task_node_id, &next_task_role, &opened_at_epoch_ms],
        )
        .await
        .with_context(|| format!("failed to reparent task node {task_node_id}"))?;
    if row_count == 0 {
        return Err(anyhow!("task node {task_node_id} not found"));
    }
    if let Some(parent_task_node_id) = prior_state.parent_task_node_id {
        refresh_task_node_rollups(client, parent_task_node_id).await?;
    }
    if let Some(parent_task_node_id) = new_parent_task_node_id {
        refresh_task_node_rollups(client, parent_task_node_id).await?;
    }
    Ok(prior_state)
}

pub(super) async fn apply_task_event_to_task_node(
    client: &Client,
    task_node_id: Uuid,
    event_kind: &str,
    next_execution_state: Option<&str>,
    prior_lifecycle_state: Option<&str>,
    next_lifecycle_state: Option<&str>,
    recorded_at_epoch_ms: i64,
) -> Result<()> {
    let reopens_line =
        task_event_reopens_line(event_kind, prior_lifecycle_state, next_lifecycle_state);
    let row_count = client
        .execute(
            r#"
            UPDATE ami.task_nodes
            SET execution_state = COALESCE($2, execution_state),
                lifecycle_state = COALESCE($3, lifecycle_state),
                opened_at_epoch_ms = CASE
                    WHEN $4 THEN COALESCE(opened_at_epoch_ms, $5)
                    ELSE opened_at_epoch_ms
                END,
                closed_at_epoch_ms = CASE
                    WHEN $3 = 'hot' THEN NULL
                    WHEN $3 = 'closed' OR $1 = 'closed' THEN COALESCE($5, closed_at_epoch_ms)
                    ELSE closed_at_epoch_ms
                END,
                archived_at_epoch_ms = CASE
                    WHEN $3 = 'hot' THEN NULL
                    WHEN $3 = 'archived' OR $1 = 'archived' THEN COALESCE($5, archived_at_epoch_ms)
                    ELSE archived_at_epoch_ms
                END,
                reopened_count = CASE
                    WHEN $4 THEN reopened_count + 1
                    ELSE reopened_count
                END,
                updated_at = now()
            WHERE task_node_id = $6
            "#,
            &[
                &event_kind,
                &next_execution_state,
                &next_lifecycle_state,
                &reopens_line,
                &recorded_at_epoch_ms,
                &task_node_id,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to materialize task event onto task node {task_node_id}")
        })?;
    if row_count == 0 {
        return Err(anyhow!(
            "task event references missing task node {task_node_id}"
        ));
    }
    Ok(())
}

fn task_event_next_execution_state_for_child_branch(
    state: &TaskNodeMaterializationState,
) -> &'static str {
    match state.execution_state.as_str() {
        "proposed" | "ready" => "ready",
        "done" | "failed" | "canceled" | "superseded" => "active",
        _ => "active",
    }
}

fn task_event_kind_for_continue(state: &TaskNodeMaterializationState) -> &'static str {
    if matches!(state.lifecycle_state.as_str(), "closed" | "archived")
        || matches!(
            state.execution_state.as_str(),
            "done" | "failed" | "canceled" | "superseded"
        )
    {
        "resumed"
    } else {
        "continued"
    }
}

fn memory_link_decision_payload_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) async fn materialize_memory_link_decision_onto_task_graph(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    decision: &MemoryLinkDecisionRecord,
) -> Result<()> {
    let recorded_at_epoch_ms = decision.recorded_at_epoch_ms.unwrap_or(current_epoch_ms()?);
    let source_event_id = format!("memory_link_decision:{}", decision.memory_link_decision_id);
    let evidence_span = json!({
        "memory_link_decision_id": decision.memory_link_decision_id,
        "decision_outcome": decision.decision_outcome,
        "classifier_label": decision.classifier_label,
        "classifier_score": decision.classifier_score,
        "legality_passed": decision.legality_passed,
        "scope_filter_passed": decision.scope_filter_passed,
        "evidence_sufficient": decision.evidence_sufficient,
    });
    match decision.decision_outcome.as_str() {
        "continue" => {
            let Some(candidate_task_node_id) = decision.candidate_task_node_id else {
                return Ok(());
            };
            let candidate_state =
                load_task_node_materialization_state(client, candidate_task_node_id).await?;
            let event_kind = task_event_kind_for_continue(&candidate_state);
            let event_payload = json!({
                "memory_link_decision_id": decision.memory_link_decision_id,
                "decision_outcome": decision.decision_outcome,
                "candidate_task_node_id": candidate_task_node_id,
                "incoming_task_node_id": decision.task_node_id,
            });
            create_task_event(
                client,
                project_code,
                namespace_code,
                &TaskEventInsert {
                    task_node_id: candidate_task_node_id,
                    source_snapshot_id: None,
                    source_event_id: Some(&source_event_id),
                    event_kind,
                    prior_execution_state: Some(candidate_state.execution_state.as_str()),
                    next_execution_state: Some("active"),
                    prior_lifecycle_state: Some(candidate_state.lifecycle_state.as_str()),
                    next_lifecycle_state: Some("hot"),
                    source_kind: Some("memory_link_decision"),
                    artifact_refs: Some(&decision.artifact_refs),
                    message_refs: Some(&decision.message_refs),
                    evidence_span: Some(&evidence_span),
                    derivation_kind: Some("extract"),
                    schema_version: Some("task-event-envelope-v1"),
                    event_payload: &event_payload,
                    recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
                },
            )
            .await?;
        }
        "child" => {
            let (Some(task_node_id), Some(candidate_task_node_id)) =
                (decision.task_node_id, decision.candidate_task_node_id)
            else {
                return Ok(());
            };
            let prior_state = reparent_task_node(
                client,
                task_node_id,
                Some(candidate_task_node_id),
                Some("child"),
                recorded_at_epoch_ms,
            )
            .await?;
            let next_execution_state =
                task_event_next_execution_state_for_child_branch(&prior_state);
            let event_payload = json!({
                "memory_link_decision_id": decision.memory_link_decision_id,
                "decision_outcome": decision.decision_outcome,
                "parent_task_node_id": candidate_task_node_id,
            });
            create_task_event(
                client,
                project_code,
                namespace_code,
                &TaskEventInsert {
                    task_node_id,
                    source_snapshot_id: None,
                    source_event_id: Some(&source_event_id),
                    event_kind: "branched_child",
                    prior_execution_state: Some(prior_state.execution_state.as_str()),
                    next_execution_state: Some(next_execution_state),
                    prior_lifecycle_state: Some(prior_state.lifecycle_state.as_str()),
                    next_lifecycle_state: Some("hot"),
                    source_kind: Some("memory_link_decision"),
                    artifact_refs: Some(&decision.artifact_refs),
                    message_refs: Some(&decision.message_refs),
                    evidence_span: Some(&evidence_span),
                    derivation_kind: Some("extract"),
                    schema_version: Some("task-event-envelope-v1"),
                    event_payload: &event_payload,
                    recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
                },
            )
            .await?;
        }
        "new" => {
            let Some(task_node_id) = decision.task_node_id else {
                return Ok(());
            };
            let prior_state = reparent_task_node(
                client,
                task_node_id,
                None,
                Some("workline"),
                recorded_at_epoch_ms,
            )
            .await?;
            let event_payload = json!({
                "memory_link_decision_id": decision.memory_link_decision_id,
                "decision_outcome": decision.decision_outcome,
            });
            create_task_event(
                client,
                project_code,
                namespace_code,
                &TaskEventInsert {
                    task_node_id,
                    source_snapshot_id: None,
                    source_event_id: Some(&source_event_id),
                    event_kind: "branched_new",
                    prior_execution_state: Some(prior_state.execution_state.as_str()),
                    next_execution_state: Some("active"),
                    prior_lifecycle_state: Some(prior_state.lifecycle_state.as_str()),
                    next_lifecycle_state: Some("hot"),
                    source_kind: Some("memory_link_decision"),
                    artifact_refs: Some(&decision.artifact_refs),
                    message_refs: Some(&decision.message_refs),
                    evidence_span: Some(&evidence_span),
                    derivation_kind: Some("extract"),
                    schema_version: Some("task-event-envelope-v1"),
                    event_payload: &event_payload,
                    recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
                },
            )
            .await?;
        }
        "abstain" => {
            let Some(task_node_id) = decision.task_node_id else {
                return Ok(());
            };
            let state = load_task_node_materialization_state(client, task_node_id).await?;
            let event_payload = json!({
                "memory_link_decision_id": decision.memory_link_decision_id,
                "decision_outcome": decision.decision_outcome,
                "decision_reason": decision.decision_reason,
                "candidate_task_node_id": decision.candidate_task_node_id,
            });
            create_task_event(
                client,
                project_code,
                namespace_code,
                &TaskEventInsert {
                    task_node_id,
                    source_snapshot_id: None,
                    source_event_id: Some(&source_event_id),
                    event_kind: "state_change",
                    prior_execution_state: Some(state.execution_state.as_str()),
                    next_execution_state: Some(state.execution_state.as_str()),
                    prior_lifecycle_state: Some(state.lifecycle_state.as_str()),
                    next_lifecycle_state: Some(state.lifecycle_state.as_str()),
                    source_kind: Some("memory_link_decision"),
                    artifact_refs: Some(&decision.artifact_refs),
                    message_refs: Some(&decision.message_refs),
                    evidence_span: Some(&evidence_span),
                    derivation_kind: Some("extract"),
                    schema_version: Some("task-event-envelope-v1"),
                    event_payload: &event_payload,
                    recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
                },
            )
            .await?;
        }
        "escalate" => {
            let Some(task_node_id) = decision.task_node_id else {
                return Ok(());
            };
            let state = load_task_node_materialization_state(client, task_node_id).await?;
            let additional_evidence_request = memory_link_decision_payload_string(
                &decision.decision_payload,
                "additional_evidence_request",
            );
            let event_payload = json!({
                "memory_link_decision_id": decision.memory_link_decision_id,
                "decision_outcome": decision.decision_outcome,
                "decision_reason": decision.decision_reason,
                "additional_evidence_request": additional_evidence_request,
                "candidate_task_node_id": decision.candidate_task_node_id,
            });
            create_task_event(
                client,
                project_code,
                namespace_code,
                &TaskEventInsert {
                    task_node_id,
                    source_snapshot_id: None,
                    source_event_id: Some(&source_event_id),
                    event_kind: "evidence_request",
                    prior_execution_state: Some(state.execution_state.as_str()),
                    next_execution_state: Some(state.execution_state.as_str()),
                    prior_lifecycle_state: Some(state.lifecycle_state.as_str()),
                    next_lifecycle_state: Some(state.lifecycle_state.as_str()),
                    source_kind: Some("memory_link_decision"),
                    artifact_refs: Some(&decision.artifact_refs),
                    message_refs: Some(&decision.message_refs),
                    evidence_span: Some(&evidence_span),
                    derivation_kind: Some("extract"),
                    schema_version: Some("task-event-envelope-v1"),
                    event_payload: &event_payload,
                    recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
                },
            )
            .await?;
        }
        "pending_link_proposal" => {
            let Some(task_node_id) = decision.task_node_id else {
                return Ok(());
            };
            let state = load_task_node_materialization_state(client, task_node_id).await?;
            let additional_evidence_request = memory_link_decision_payload_string(
                &decision.decision_payload,
                "additional_evidence_request",
            );
            let pending_link_ttl_epoch_ms = decision
                .decision_payload
                .get("pending_link_ttl_epoch_ms")
                .and_then(Value::as_i64);
            let event_payload = json!({
                "memory_link_decision_id": decision.memory_link_decision_id,
                "decision_outcome": decision.decision_outcome,
                "decision_reason": decision.decision_reason,
                "additional_evidence_request": additional_evidence_request,
                "pending_link_ttl_epoch_ms": pending_link_ttl_epoch_ms,
                "candidate_task_node_id": decision.candidate_task_node_id,
            });
            create_task_event(
                client,
                project_code,
                namespace_code,
                &TaskEventInsert {
                    task_node_id,
                    source_snapshot_id: None,
                    source_event_id: Some(&source_event_id),
                    event_kind: "evidence_request",
                    prior_execution_state: Some(state.execution_state.as_str()),
                    next_execution_state: Some(state.execution_state.as_str()),
                    prior_lifecycle_state: Some(state.lifecycle_state.as_str()),
                    next_lifecycle_state: Some(state.lifecycle_state.as_str()),
                    source_kind: Some("memory_link_decision"),
                    artifact_refs: Some(&decision.artifact_refs),
                    message_refs: Some(&decision.message_refs),
                    evidence_span: Some(&evidence_span),
                    derivation_kind: Some("extract"),
                    schema_version: Some("task-event-envelope-v1"),
                    event_payload: &event_payload,
                    recorded_at_epoch_ms: Some(recorded_at_epoch_ms),
                },
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) async fn materialize_pending_link_proposal_onto_task_graph(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    proposal: &PendingLinkProposalRecord,
) -> Result<()> {
    let Some(task_node_id) = proposal.task_node_id else {
        return Ok(());
    };
    let state = load_task_node_materialization_state(client, task_node_id).await?;
    let source_event_id = format!(
        "pending_link_proposal:{}",
        proposal.pending_link_proposal_id
    );
    let evidence_span = json!({
        "pending_link_proposal_id": proposal.pending_link_proposal_id,
        "proposal_state": proposal.proposal_state,
        "ttl_epoch_ms": proposal.ttl_epoch_ms,
    });
    let event_payload = json!({
        "pending_link_proposal_id": proposal.pending_link_proposal_id,
        "proposal_reason": proposal.proposal_reason,
        "evidence_request": proposal.evidence_request,
        "candidate_task_node_id": proposal.candidate_task_node_id,
    });
    create_task_event(
        client,
        project_code,
        namespace_code,
        &TaskEventInsert {
            task_node_id,
            source_snapshot_id: None,
            source_event_id: Some(&source_event_id),
            event_kind: "evidence_request",
            prior_execution_state: Some(state.execution_state.as_str()),
            next_execution_state: Some(state.execution_state.as_str()),
            prior_lifecycle_state: Some(state.lifecycle_state.as_str()),
            next_lifecycle_state: Some(state.lifecycle_state.as_str()),
            source_kind: Some("pending_link_proposal"),
            artifact_refs: Some(&proposal.artifact_refs),
            message_refs: Some(&proposal.message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            schema_version: Some("task-event-envelope-v1"),
            event_payload: &event_payload,
            recorded_at_epoch_ms: Some(current_epoch_ms()?),
        },
    )
    .await?;
    Ok(())
}

pub(super) fn task_event_evidence_span_json(record: &TaskEventInsert<'_>) -> Value {
    if let Some(value) = record.evidence_span {
        return value.clone();
    }
    if let Some(value) = record.event_payload.get("evidence_span") {
        return value.clone();
    }
    json!({
        "source_event_id": record.source_event_id,
        "source_snapshot_id": record.source_snapshot_id,
        "event_kind": record.event_kind,
    })
}

pub(super) fn validate_task_event_basis(
    record: &TaskEventInsert<'_>,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let has_source_event_id = record.source_event_id.is_some();
    let has_source_snapshot_id = record.source_snapshot_id.is_some();
    let has_artifact_refs = value_string_array_len(Some(artifact_refs)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let derivation_kind = record.derivation_kind.unwrap_or("raw_capture");
    if derivation_kind != "operator_write"
        && !has_source_event_id
        && !has_source_snapshot_id
        && !has_artifact_refs
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "task event requires recorded basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn link_surface_json_or_empty(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or_else(|| json!([]))
}

pub(super) fn link_surface_evidence_span_json(explicit: Option<&Value>, payload: &Value) -> Value {
    if let Some(value) = explicit {
        return value.clone();
    }
    if let Some(value) = payload.get("evidence_span") {
        return value.clone();
    }
    json!({})
}

pub(super) fn validate_memory_link_decision_basis(
    record: &MemoryLinkDecisionInsert<'_>,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let derivation_kind = record.derivation_kind.unwrap_or("extract");
    let has_trace = record.retrieval_trace_id.is_some();
    let has_source_events = value_string_array_len(Some(source_event_ids)) > 0;
    let has_artifact_refs = value_string_array_len(Some(artifact_refs)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    if derivation_kind != "operator_write"
        && !has_trace
        && !has_source_events
        && !has_artifact_refs
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "memory link decision requires retrieval trace or recorded basis unless derivation_kind=operator_write"
        ));
    }
    let decision_outcome = record.decision_outcome.trim();
    if matches!(
        decision_outcome,
        "continue" | "child" | "new" | "abstain" | "escalate" | "pending_link_proposal"
    ) && record.task_node_id.is_none()
    {
        return Err(anyhow!(
            "memory link decision outcome {} requires task_node_id",
            decision_outcome
        ));
    }
    if matches!(decision_outcome, "continue" | "child") && record.candidate_task_node_id.is_none() {
        return Err(anyhow!(
            "memory link decision outcome {} requires candidate_task_node_id",
            decision_outcome
        ));
    }
    if decision_outcome == "abstain"
        && !record
            .decision_reason
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(anyhow!(
            "memory link decision outcome abstain requires non-empty decision_reason"
        ));
    }
    if decision_outcome == "escalate" {
        let has_reason = record
            .decision_reason
            .is_some_and(|value| !value.trim().is_empty());
        let has_request = memory_link_decision_payload_string(
            record.decision_payload,
            "additional_evidence_request",
        )
        .is_some();
        if !has_reason || !has_request {
            return Err(anyhow!(
                "memory link decision outcome escalate requires decision_reason and decision_payload.additional_evidence_request"
            ));
        }
    }
    if decision_outcome == "pending_link_proposal" {
        let has_reason = record
            .decision_reason
            .is_some_and(|value| !value.trim().is_empty());
        let has_ttl = record
            .decision_payload
            .get("pending_link_ttl_epoch_ms")
            .and_then(Value::as_i64)
            .is_some();
        let has_request = memory_link_decision_payload_string(
            record.decision_payload,
            "additional_evidence_request",
        )
        .is_some();
        if !has_reason || !has_ttl || !has_request {
            return Err(anyhow!(
                "memory link decision outcome pending_link_proposal requires decision_reason, decision_payload.pending_link_ttl_epoch_ms and decision_payload.additional_evidence_request"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct MemoryLinkDecisionPolicyScopeFilter {
    project_code: String,
    namespace_code: String,
    task_node_present: bool,
    task_node_found: bool,
    task_node_scope_matches: bool,
    candidate_task_node_present: bool,
    candidate_task_node_found: bool,
    candidate_task_node_scope_matches: bool,
    retrieval_trace_present: bool,
    retrieval_trace_found: bool,
    retrieval_trace_scope_matches: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct MemoryLinkDecisionVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

fn memory_link_decision_marks_poisoned(evidence_span: &Value) -> bool {
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

pub(super) async fn run_memory_link_decision_policy_scope_filter(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    task_node_id: Option<Uuid>,
    candidate_task_node_id: Option<Uuid>,
    retrieval_trace_id: Option<Uuid>,
) -> MemoryLinkDecisionPolicyScopeFilter {
    let task_node_present = task_node_id.is_some();
    let task_node = match task_node_id {
        Some(task_node_id) => get_task_node(client, task_node_id).await.ok(),
        None => None,
    };
    let task_node_found = task_node.is_some() || !task_node_present;
    let task_node_scope_matches = !task_node_present
        || task_node.as_ref().is_some_and(|node| {
            node.project_code == project.code
                && node.namespace_code.as_deref() == Some(namespace.code.as_str())
        });

    let candidate_task_node_present = candidate_task_node_id.is_some();
    let candidate_task_node = match candidate_task_node_id {
        Some(task_node_id) => get_task_node(client, task_node_id).await.ok(),
        None => None,
    };
    let candidate_task_node_found = candidate_task_node.is_some() || !candidate_task_node_present;
    let candidate_task_node_scope_matches = !candidate_task_node_present
        || candidate_task_node.as_ref().is_some_and(|node| {
            node.project_code == project.code
                && node.namespace_code.as_deref() == Some(namespace.code.as_str())
        });

    let retrieval_trace_present = retrieval_trace_id.is_some();
    let retrieval_trace = match retrieval_trace_id {
        Some(trace_id) => get_retrieval_trace(client, trace_id).await.ok(),
        None => None,
    };
    let retrieval_trace_found = retrieval_trace.is_some() || !retrieval_trace_present;
    let retrieval_trace_scope_matches = !retrieval_trace_present
        || retrieval_trace.as_ref().is_some_and(|trace| {
            trace.project_code == project.code
                && trace.namespace_code.as_deref() == Some(namespace.code.as_str())
        });

    let scope_binding_valid = (!task_node_present || (task_node_found && task_node_scope_matches))
        && (!candidate_task_node_present
            || (candidate_task_node_found && candidate_task_node_scope_matches))
        && (!retrieval_trace_present || (retrieval_trace_found && retrieval_trace_scope_matches));

    MemoryLinkDecisionPolicyScopeFilter {
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        task_node_present,
        task_node_found,
        task_node_scope_matches,
        candidate_task_node_present,
        candidate_task_node_found,
        candidate_task_node_scope_matches,
        retrieval_trace_present,
        retrieval_trace_found,
        retrieval_trace_scope_matches,
        scope_binding_valid,
    }
}

pub(super) fn validate_memory_link_decision_policy_scope_filter(
    filter: &MemoryLinkDecisionPolicyScopeFilter,
) -> Result<()> {
    if filter.task_node_present && !filter.task_node_found {
        return Err(anyhow!("memory link decision references missing task node"));
    }
    if filter.candidate_task_node_present && !filter.candidate_task_node_found {
        return Err(anyhow!(
            "memory link decision references missing candidate task node"
        ));
    }
    if filter.retrieval_trace_present && !filter.retrieval_trace_found {
        return Err(anyhow!(
            "memory link decision references missing retrieval trace"
        ));
    }
    if !filter.task_node_scope_matches {
        return Err(anyhow!(
            "memory link decision task node scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.candidate_task_node_scope_matches {
        return Err(anyhow!(
            "memory link decision candidate task node scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.retrieval_trace_scope_matches {
        return Err(anyhow!(
            "memory link decision retrieval trace scope does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.scope_binding_valid {
        return Err(anyhow!(
            "memory link decision failed policy/scope filter before truth write"
        ));
    }
    Ok(())
}

pub(super) fn run_memory_link_decision_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    policy_filter: &MemoryLinkDecisionPolicyScopeFilter,
    retrieval_trace_present: bool,
) -> MemoryLinkDecisionVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || retrieval_trace_present
        || value_string_array_len(Some(source_event_ids)) > 0
        || value_string_array_len(Some(artifact_refs)) > 0
        || value_string_array_len(Some(message_refs)) > 0
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let poisoned_detected = memory_link_decision_marks_poisoned(evidence_span);
    MemoryLinkDecisionVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed: policy_filter.scope_binding_valid && evidence_present && !poisoned_detected,
    }
}

pub(super) fn validate_memory_link_decision_verification_conflict_check(
    check: &MemoryLinkDecisionVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "memory link decision evidence_span is flagged poisoned"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "memory link decision requires recorded evidence or retrieval trace"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory link decision failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn augment_memory_link_decision_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &MemoryLinkDecisionPolicyScopeFilter,
    verification_check: &MemoryLinkDecisionVerificationConflictCheck,
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

pub(super) fn validate_pending_link_proposal_basis(
    record: &PendingLinkProposalInsert<'_>,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let derivation_kind = record.derivation_kind.unwrap_or("extract");
    let has_trace = record.retrieval_trace_id.is_some();
    let has_source_events = value_string_array_len(Some(source_event_ids)) > 0;
    let has_artifact_refs = value_string_array_len(Some(artifact_refs)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    if derivation_kind != "operator_write"
        && !has_trace
        && !has_source_events
        && !has_artifact_refs
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "pending link proposal requires retrieval trace or recorded basis unless derivation_kind=operator_write"
        ));
    }
    let proposal_state = record.proposal_state.unwrap_or("pending");
    if proposal_state == "pending" {
        if record.ttl_epoch_ms.is_none() {
            return Err(anyhow!(
                "pending link proposal requires ttl_epoch_ms while proposal_state=pending"
            ));
        }
        let has_request = record
            .evidence_request
            .is_some_and(|value| !value.trim().is_empty());
        let payload_has_request = record
            .evidence_payload
            .as_object()
            .is_some_and(|payload| !payload.is_empty());
        if !has_request && !payload_has_request {
            return Err(anyhow!(
                "pending link proposal requires evidence_request or evidence_payload while proposal_state=pending"
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_memory_relation_edge_basis(
    derivation_kind: &str,
    evidence: &Value,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let has_evidence = evidence
        .as_object()
        .map(|object| !object.is_empty())
        .unwrap_or_else(|| !evidence.is_null());
    let has_source_event_ids = value_string_array_len(Some(source_event_ids)) > 0;
    let has_artifact_refs = value_string_array_len(Some(artifact_refs)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .map(|object| !object.is_empty())
        .unwrap_or(false);
    if derivation_kind != "operator_write"
        && !has_evidence
        && !has_source_event_ids
        && !has_artifact_refs
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "memory relation edge requires evidence or recorded basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn memory_relation_edge_record_from_row(row: &Row) -> MemoryRelationEdgeRecord {
    MemoryRelationEdgeRecord {
        memory_relation_edge_id: row.get(0),
        project_code: row.get(1),
        namespace_code: row.get(2),
        source_memory_card_id: row.get(3),
        target_memory_card_id: row.get(4),
        relation_type: row.get(5),
        relation_state: row.get(6),
        evidence: row.get(7),
        source_kind: row.get(8),
        source_event_ids: row.get(9),
        artifact_refs: row.get(10),
        message_refs: row.get(11),
        evidence_span: row.get(12),
        derivation_kind: row.get(13),
        schema_version: row.get(14),
        recorded_at_epoch_ms: row.get(15),
        valid_from_epoch_ms: row.get(16),
        valid_to_epoch_ms: row.get(17),
        created_at: row.get(18),
    }
}

pub async fn find_namespace_by_code(
    client: &Client,
    project_id: Uuid,
    code: &str,
) -> Result<Option<NamespaceRecord>> {
    let row = client
        .query_opt(
            r#"
            SELECT namespace_id, code, display_name, retrieval_mode
            FROM ami.namespaces
            WHERE project_id = $1 AND code = $2
            "#,
            &[&project_id, &code],
        )
        .await?;
    Ok(row.map(|row| NamespaceRecord {
        namespace_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        retrieval_mode: row.get(3),
    }))
}

pub async fn list_namespaces_for_project(
    client: &Client,
    project_id: Uuid,
) -> Result<Vec<NamespaceRecord>> {
    let rows = client
        .query(
            r#"
            SELECT namespace_id, code, display_name, retrieval_mode
            FROM ami.namespaces
            WHERE project_id = $1
            ORDER BY code
            "#,
            &[&project_id],
        )
        .await
        .context("failed to list namespaces for project")?;
    Ok(rows
        .into_iter()
        .map(|row| NamespaceRecord {
            namespace_id: row.get(0),
            code: row.get(1),
            display_name: row.get(2),
            retrieval_mode: row.get(3),
        })
        .collect())
}

pub async fn add_relation(
    client: &Client,
    source_code: &str,
    target_code: &str,
    relation_type: &str,
    project_link_type: Option<&str>,
    shared_contour: &str,
    visibility_scope: &str,
    relation_status: &str,
    requires_approval: bool,
    transfer_policy_code: Option<&str>,
    access_mode: &str,
) -> Result<()> {
    let source = get_project_by_code(client, source_code).await?;
    let target = get_project_by_code(client, target_code).await?;
    let transfer_policy = match transfer_policy_code {
        Some(code) => find_transfer_policy_by_code(client, code).await?,
        None => None,
    };
    let project_link_type = project_link_type.unwrap_or(relation_type);
    client
        .execute(
            r#"
            INSERT INTO ami.project_relations(
                source_project_id,
                target_project_id,
                relation_type,
                project_link_type,
                shared_contour,
                visibility_scope,
                relation_status,
                requires_approval,
                transfer_policy_id,
                access_mode
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (source_project_id, target_project_id, relation_type, shared_contour) DO UPDATE SET
                project_link_type = EXCLUDED.project_link_type,
                visibility_scope = EXCLUDED.visibility_scope,
                relation_status = EXCLUDED.relation_status,
                requires_approval = EXCLUDED.requires_approval,
                transfer_policy_id = EXCLUDED.transfer_policy_id,
                access_mode = EXCLUDED.access_mode
            "#,
            &[
                &source.project_id,
                &target.project_id,
                &relation_type,
                &project_link_type,
                &shared_contour,
                &visibility_scope,
                &relation_status,
                &requires_approval,
                &transfer_policy.as_ref().map(|item| item.transfer_policy_id),
                &access_mode,
            ],
        )
        .await
        .context("failed to add relation")?;
    Ok(())
}

pub struct RelationUpdate<'a> {
    pub source_code: &'a str,
    pub target_code: &'a str,
    pub relation_type: &'a str,
    pub shared_contour: &'a str,
    pub project_link_type: Option<&'a str>,
    pub visibility_scope: Option<&'a str>,
    pub relation_status: Option<&'a str>,
    pub requires_approval: Option<bool>,
    pub transfer_policy_code: Option<&'a str>,
    pub access_mode: Option<&'a str>,
    pub actor_agent_code: Option<&'a str>,
    pub override_reason: Option<&'a str>,
}

pub async fn update_relation(client: &Client, update: RelationUpdate<'_>) -> Result<()> {
    if update.relation_status == Some("quarantined") && update.override_reason.is_none() {
        return Err(anyhow!(
            "relation {} -> {} [{} / {}] cannot be quarantined without override_reason",
            update.source_code,
            update.target_code,
            update.relation_type,
            update.shared_contour
        ));
    }
    let source = get_project_by_code(client, update.source_code).await?;
    let target = get_project_by_code(client, update.target_code).await?;
    let transfer_policy = match update.transfer_policy_code {
        Some(code) => find_transfer_policy_by_code(client, code).await?,
        None => None,
    };
    let actor_agent_id = match update.actor_agent_code {
        Some(code) => find_agent_id_by_code(client, code).await?,
        None => None,
    };
    let row = client
        .query_opt(
            r#"
            UPDATE ami.project_relations
            SET project_link_type = COALESCE($5, project_link_type),
                visibility_scope = COALESCE($6, visibility_scope),
                relation_status = COALESCE($7, relation_status),
                requires_approval = COALESCE($8, requires_approval),
                transfer_policy_id = COALESCE($9, transfer_policy_id),
                access_mode = COALESCE($10, access_mode)
            WHERE source_project_id = $1
              AND target_project_id = $2
              AND relation_type = $3
              AND shared_contour = $4
            RETURNING relation_id
            "#,
            &[
                &source.project_id,
                &target.project_id,
                &update.relation_type,
                &update.shared_contour,
                &update.project_link_type,
                &update.visibility_scope,
                &update.relation_status,
                &update.requires_approval,
                &transfer_policy.as_ref().map(|item| item.transfer_policy_id),
                &update.access_mode,
            ],
        )
        .await
        .context("failed to update relation")?;
    let Some(row) = row else {
        return Err(anyhow!(
            "relation not found: {} -> {} [{} / {}]",
            update.source_code,
            update.target_code,
            update.relation_type,
            update.shared_contour
        ));
    };
    if let Some(reason) = update.override_reason {
        let relation_id: Uuid = row.get(0);
        let workspace_id = get_project_workspace_id(client, source.project_id).await?;
        let event_kind = match update.relation_status {
            Some("forbidden") => "revoke",
            Some("quarantined") => "quarantine",
            _ => "rescope",
        };
        let details = json!({
            "source_code": update.source_code,
            "target_code": update.target_code,
            "relation_type": update.relation_type,
            "shared_contour": update.shared_contour,
            "project_link_type": update.project_link_type,
            "visibility_scope": update.visibility_scope,
            "relation_status": update.relation_status,
            "requires_approval": update.requires_approval,
            "transfer_policy_code": update.transfer_policy_code,
            "access_mode": update.access_mode,
        });
        record_scope_override_event(
            client,
            workspace_id,
            "project_relation",
            relation_id,
            actor_agent_id,
            event_kind,
            reason,
            &details,
        )
        .await?;
    }
    if let Some(relation_status) = update.relation_status {
        let relation_id: Uuid = row.get(0);
        let workspace_id = get_project_workspace_id(client, source.project_id).await?;
        match relation_status {
            "quarantined" => {
                let workspace_code = get_workspace_by_id(client, workspace_id).await?.code;
                let quarantine_source_event_ids = json!([format!(
                    "project_relation:{}:{}:{}:{}",
                    update.source_code,
                    update.target_code,
                    update.relation_type,
                    update.shared_contour
                )]);
                let quarantine_evidence = json!({
                    "source_code": update.source_code,
                    "target_code": update.target_code,
                    "relation_type": update.relation_type,
                    "shared_contour": update.shared_contour,
                    "project_link_type": update.project_link_type,
                    "visibility_scope": update.visibility_scope,
                    "requires_approval": update.requires_approval,
                    "transfer_policy_code": update.transfer_policy_code,
                    "access_mode": update.access_mode,
                    "override_reason": update.override_reason,
                });
                let quarantine_evidence_span = json!({
                    "kind": "project_relation_quarantine",
                    "relation_id": relation_id,
                    "source_code": update.source_code,
                    "target_code": update.target_code,
                    "relation_type": update.relation_type,
                    "shared_contour": update.shared_contour,
                    "relation_status": relation_status,
                    "override_reason": update.override_reason,
                });
                let _ = create_quarantine_item(
                    client,
                    &workspace_code,
                    &QuarantineItemInsert {
                        project_code: Some(update.source_code),
                        namespace_code: None,
                        entity_kind: "project_relation",
                        entity_id: Some(relation_id),
                        quarantine_reason: update
                            .override_reason
                            .unwrap_or("project relation quarantined"),
                        quarantine_state: Some("active"),
                        evidence: &quarantine_evidence,
                        source_kind: Some("project_relation_override"),
                        source_event_ids: Some(&quarantine_source_event_ids),
                        artifact_refs: None,
                        message_refs: None,
                        evidence_span: Some(&quarantine_evidence_span),
                        derivation_kind: Some("operator_write"),
                        schema_version: Some("quarantine-item-envelope-v1"),
                        quarantined_at_epoch_ms: Some(current_epoch_ms()?),
                        released_at_epoch_ms: None,
                    },
                )
                .await?;
            }
            "active" | "disabled" | "forbidden" => {
                let next_state = if relation_status == "forbidden" {
                    "rejected"
                } else {
                    "released"
                };
                let _ = set_quarantine_items_state_for_entity(
                    client,
                    workspace_id,
                    "project_relation",
                    relation_id,
                    next_state,
                    Some(current_epoch_ms()?),
                )
                .await?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub async fn list_related_projects(
    client: &Client,
    source_project_id: Uuid,
) -> Result<Vec<VisibleProjectRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                p.project_id,
                p.code,
                p.display_name,
                p.repo_root,
                p.visibility_scope,
                to_char(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                r.relation_type,
                r.project_link_type,
                r.shared_contour,
                r.visibility_scope,
                r.relation_status,
                r.requires_approval,
                tp.code,
                r.access_mode
            FROM ami.project_relations r
            JOIN ami.projects p ON p.project_id = r.target_project_id
            LEFT JOIN ami.transfer_policies tp ON tp.transfer_policy_id = r.transfer_policy_id
            WHERE r.source_project_id = $1
              AND r.relation_status = 'active'
            ORDER BY p.code, r.project_link_type, r.shared_contour
            "#,
            &[&source_project_id],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| VisibleProjectRecord {
            project: ProjectRecord {
                project_id: row.get(0),
                code: row.get(1),
                display_name: row.get(2),
                repo_root: row.get(3),
                visibility_scope: row.get(4),
                updated_at: row.get(5),
            },
            relation_type: row.get(6),
            project_link_type: row.get(7),
            shared_contour: row.get(8),
            visibility_scope: row.get(9),
            relation_status: row.get(10),
            requires_approval: row.get(11),
            transfer_policy_code: row.get(12),
            access_mode: row.get(13),
        })
        .collect())
}
