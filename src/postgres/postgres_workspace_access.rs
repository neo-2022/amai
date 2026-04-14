use super::*;

pub async fn ensure_workspace(
    client: &Client,
    code: &str,
    display_name: &str,
    status: &str,
) -> Result<WorkspaceRecord> {
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.workspaces(code, display_name, status)
            VALUES ($1, $2, $3)
            ON CONFLICT (code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                status = EXCLUDED.status,
                updated_at = now()
            RETURNING workspace_id, code, display_name, status
            "#,
            &[&code, &display_name, &status],
        )
        .await
        .context("failed to ensure workspace")?;
    Ok(workspace_record_from_row(&row))
}

pub async fn get_workspace_by_code(client: &Client, code: &str) -> Result<WorkspaceRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT workspace_id, code, display_name, status
            FROM ami.workspaces
            WHERE code = $1
            "#,
            &[&code],
        )
        .await?
        .ok_or_else(|| anyhow!("workspace not found: {code}"))?;
    Ok(workspace_record_from_row(&row))
}

pub async fn get_workspace_by_id(client: &Client, workspace_id: Uuid) -> Result<WorkspaceRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT workspace_id, code, display_name, status
            FROM ami.workspaces
            WHERE workspace_id = $1
            "#,
            &[&workspace_id],
        )
        .await?
        .ok_or_else(|| anyhow!("workspace not found: {workspace_id}"))?;
    Ok(workspace_record_from_row(&row))
}

pub async fn list_workspaces(
    client: &Client,
    workspace_code: Option<&str>,
) -> Result<Vec<WorkspaceRecord>> {
    let rows = client
        .query(
            r#"
            SELECT workspace_id, code, display_name, status
            FROM ami.workspaces
            WHERE ($1::text IS NULL OR code = $1)
            ORDER BY code
            "#,
            &[&workspace_code],
        )
        .await
        .context("failed to list workspaces")?;
    Ok(rows
        .into_iter()
        .map(|row| workspace_record_from_row(&row))
        .collect())
}

pub async fn ensure_team(
    client: &Client,
    workspace_code: &str,
    code: &str,
    display_name: &str,
    status: &str,
) -> Result<TeamRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.teams(workspace_id, code, display_name, status)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (workspace_id, code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                status = EXCLUDED.status,
                updated_at = now()
            RETURNING team_id, $5::text, code, display_name, status
            "#,
            &[
                &workspace.workspace_id,
                &code,
                &display_name,
                &status,
                &workspace.code,
            ],
        )
        .await
        .context("failed to ensure team")?;
    Ok(team_record_from_row(&row))
}

pub async fn list_teams(
    client: &Client,
    workspace_code: Option<&str>,
    team_code: Option<&str>,
) -> Result<Vec<TeamRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                t.team_id,
                w.code,
                t.code,
                t.display_name,
                t.status
            FROM ami.teams t
            INNER JOIN ami.workspaces w ON w.workspace_id = t.workspace_id
            WHERE ($1::text IS NULL OR w.code = $1)
              AND ($2::text IS NULL OR t.code = $2)
            ORDER BY w.code, t.code
            "#,
            &[&workspace_code, &team_code],
        )
        .await
        .context("failed to list teams")?;
    Ok(rows
        .into_iter()
        .map(|row| team_record_from_row(&row))
        .collect())
}

async fn find_team_by_workspace_and_code(
    client: &Client,
    workspace_id: Uuid,
    code: &str,
) -> Result<Option<(Uuid, String)>> {
    let row = client
        .query_opt(
            r#"
            SELECT team_id, code
            FROM ami.teams
            WHERE workspace_id = $1 AND code = $2
            "#,
            &[&workspace_id, &code],
        )
        .await
        .with_context(|| format!("failed to lookup team {code}"))?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

pub async fn ensure_agent_role(
    client: &Client,
    workspace_code: &str,
    code: &str,
    display_name: &str,
    status: &str,
) -> Result<AgentRoleRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.agent_roles(workspace_id, code, display_name, status)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (workspace_id, code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                status = EXCLUDED.status,
                updated_at = now()
            RETURNING role_id, $5::text, code, display_name, status
            "#,
            &[
                &workspace.workspace_id,
                &code,
                &display_name,
                &status,
                &workspace.code,
            ],
        )
        .await
        .context("failed to ensure agent role")?;
    Ok(agent_role_record_from_row(&row))
}

pub async fn list_agent_roles(
    client: &Client,
    workspace_code: Option<&str>,
    role_code: Option<&str>,
) -> Result<Vec<AgentRoleRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                r.role_id,
                w.code,
                r.code,
                r.display_name,
                r.status
            FROM ami.agent_roles r
            INNER JOIN ami.workspaces w ON w.workspace_id = r.workspace_id
            WHERE ($1::text IS NULL OR w.code = $1)
              AND ($2::text IS NULL OR r.code = $2)
            ORDER BY w.code, r.code
            "#,
            &[&workspace_code, &role_code],
        )
        .await
        .context("failed to list agent roles")?;
    Ok(rows
        .into_iter()
        .map(|row| agent_role_record_from_row(&row))
        .collect())
}

async fn find_agent_role_by_workspace_and_code(
    client: &Client,
    workspace_id: Uuid,
    code: &str,
) -> Result<Option<(Uuid, String)>> {
    let row = client
        .query_opt(
            r#"
            SELECT role_id, code
            FROM ami.agent_roles
            WHERE workspace_id = $1 AND code = $2
            "#,
            &[&workspace_id, &code],
        )
        .await
        .with_context(|| format!("failed to lookup agent role {code}"))?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

pub async fn ensure_agent(
    client: &Client,
    workspace_code: &str,
    team_code: Option<&str>,
    role_code: Option<&str>,
    code: &str,
    display_name: &str,
    visibility_scope: &str,
    status: &str,
) -> Result<AgentRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let team = match team_code {
        Some(item) => find_team_by_workspace_and_code(client, workspace.workspace_id, item).await?,
        None => None,
    };
    let role = match role_code {
        Some(item) => {
            find_agent_role_by_workspace_and_code(client, workspace.workspace_id, item).await?
        }
        None => None,
    };
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.agents(
                code,
                display_name,
                workspace_id,
                team_id,
                role_id,
                visibility_scope,
                status,
                metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::jsonb)
            ON CONFLICT (code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                workspace_id = EXCLUDED.workspace_id,
                team_id = EXCLUDED.team_id,
                role_id = EXCLUDED.role_id,
                visibility_scope = EXCLUDED.visibility_scope,
                status = EXCLUDED.status
            RETURNING
                agent_id,
                $8::text,
                $9::text,
                $10::text,
                code,
                display_name,
                visibility_scope,
                status
            "#,
            &[
                &code,
                &display_name,
                &workspace.workspace_id,
                &team.as_ref().map(|item| item.0),
                &role.as_ref().map(|item| item.0),
                &visibility_scope,
                &status,
                &workspace.code,
                &team.as_ref().map(|item| item.1.clone()),
                &role.as_ref().map(|item| item.1.clone()),
            ],
        )
        .await
        .context("failed to ensure agent")?;
    Ok(agent_record_from_row(&row))
}

pub async fn list_agents(
    client: &Client,
    workspace_code: Option<&str>,
    agent_code: Option<&str>,
) -> Result<Vec<AgentRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                a.agent_id,
                w.code,
                t.code,
                r.code,
                a.code,
                a.display_name,
                a.visibility_scope,
                a.status
            FROM ami.agents a
            INNER JOIN ami.workspaces w ON w.workspace_id = a.workspace_id
            LEFT JOIN ami.teams t ON t.team_id = a.team_id
            LEFT JOIN ami.agent_roles r ON r.role_id = a.role_id
            WHERE ($1::text IS NULL OR w.code = $1)
              AND ($2::text IS NULL OR a.code = $2)
            ORDER BY w.code, a.code
            "#,
            &[&workspace_code, &agent_code],
        )
        .await
        .context("failed to list agents")?;
    Ok(rows
        .into_iter()
        .map(|row| agent_record_from_row(&row))
        .collect())
}

pub async fn ensure_transfer_policy(
    client: &Client,
    workspace_code: &str,
    code: &str,
    display_name: &str,
    default_decision: &str,
    allow_cross_project_read: bool,
    allow_import: bool,
    allow_verified_writeback: bool,
    requires_human_approval: bool,
) -> Result<TransferPolicyRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.transfer_policies(
                workspace_id,
                code,
                display_name,
                default_decision,
                allow_cross_project_read,
                allow_import,
                allow_verified_writeback,
                requires_human_approval
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (workspace_id, code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                default_decision = EXCLUDED.default_decision,
                allow_cross_project_read = EXCLUDED.allow_cross_project_read,
                allow_import = EXCLUDED.allow_import,
                allow_verified_writeback = EXCLUDED.allow_verified_writeback,
                requires_human_approval = EXCLUDED.requires_human_approval,
                updated_at = now()
            RETURNING
                transfer_policy_id,
                $9::text,
                code,
                display_name,
                default_decision,
                allow_cross_project_read,
                allow_import,
                allow_verified_writeback,
                requires_human_approval
            "#,
            &[
                &workspace.workspace_id,
                &code,
                &display_name,
                &default_decision,
                &allow_cross_project_read,
                &allow_import,
                &allow_verified_writeback,
                &requires_human_approval,
                &workspace.code,
            ],
        )
        .await
        .context("failed to ensure transfer policy")?;
    let policy = transfer_policy_record_from_row(&row);
    let source_event_ids = json!([format!("transfer_policy:{}", policy.code)]);
    let evidence_span = json!({
        "kind": "transfer_policy",
        "workspace_code": workspace.code,
        "allow_cross_project_read": policy.allow_cross_project_read,
        "allow_import": policy.allow_import,
        "allow_verified_writeback": policy.allow_verified_writeback,
        "requires_human_approval": policy.requires_human_approval,
    });
    let rule_payload = json!({
        "policy_surface": "transfer_policy",
        "display_name": policy.display_name,
        "default_decision": policy.default_decision,
        "allow_cross_project_read": policy.allow_cross_project_read,
        "allow_import": policy.allow_import,
        "allow_verified_writeback": policy.allow_verified_writeback,
        "requires_human_approval": policy.requires_human_approval,
    });
    let _ = create_policy_rule(
        client,
        workspace_code,
        &PolicyRuleInsert {
            project_code: None,
            namespace_code: None,
            rule_code: &format!("transfer_policy:{}", policy.code),
            rule_scope: "workspace",
            rule_kind: "import",
            rule_status: Some("active"),
            precedence: Some(100),
            source_kind: Some("transfer_policy_runtime"),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: None,
            message_refs: None,
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("operator_write"),
            schema_version: Some("policy-rule-envelope-v1"),
            rule_payload: &rule_payload,
        },
    )
    .await?;
    Ok(policy)
}

pub async fn list_transfer_policies(
    client: &Client,
    workspace_code: Option<&str>,
    policy_code: Option<&str>,
) -> Result<Vec<TransferPolicyRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                tp.transfer_policy_id,
                w.code,
                tp.code,
                tp.display_name,
                tp.default_decision,
                tp.allow_cross_project_read,
                tp.allow_import,
                tp.allow_verified_writeback,
                tp.requires_human_approval
            FROM ami.transfer_policies tp
            INNER JOIN ami.workspaces w ON w.workspace_id = tp.workspace_id
            WHERE ($1::text IS NULL OR w.code = $1)
              AND ($2::text IS NULL OR tp.code = $2)
            ORDER BY w.code, tp.code
            "#,
            &[&workspace_code, &policy_code],
        )
        .await
        .context("failed to list transfer policies")?;
    Ok(rows
        .into_iter()
        .map(|row| transfer_policy_record_from_row(&row))
        .collect())
}

pub async fn get_transfer_policy(
    client: &Client,
    transfer_policy_id: Uuid,
) -> Result<TransferPolicyRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                tp.transfer_policy_id,
                w.code,
                tp.code,
                tp.display_name,
                tp.default_decision,
                tp.allow_cross_project_read,
                tp.allow_import,
                tp.allow_verified_writeback,
                tp.requires_human_approval
            FROM ami.transfer_policies tp
            INNER JOIN ami.workspaces w ON w.workspace_id = tp.workspace_id
            WHERE tp.transfer_policy_id = $1
            "#,
            &[&transfer_policy_id],
        )
        .await
        .with_context(|| format!("failed to load transfer policy {}", transfer_policy_id))?;
    Ok(transfer_policy_record_from_row(&row))
}

pub async fn ensure_access_policy(
    client: &Client,
    workspace_code: &str,
    role_code: Option<&str>,
    team_code: Option<&str>,
    project_code: Option<&str>,
    code: &str,
    display_name: &str,
    object_class: &str,
    scope_type: &str,
    precedence: i32,
    can_read: bool,
    can_write: bool,
    can_link: bool,
    can_import: bool,
    can_promote: bool,
    can_share_further: bool,
    can_archive: bool,
    can_delete: bool,
    can_quarantine: bool,
    can_approve_transfer: bool,
    human_override: bool,
    override_reason: Option<&str>,
    status: &str,
) -> Result<AccessPolicyRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let team = match team_code {
        Some(item) => find_team_by_workspace_and_code(client, workspace.workspace_id, item).await?,
        None => None,
    };
    let role = match role_code {
        Some(item) => {
            find_agent_role_by_workspace_and_code(client, workspace.workspace_id, item).await?
        }
        None => None,
    };
    let project = match project_code {
        Some(item) => Some(get_project_by_code(client, item).await?),
        None => None,
    };
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.access_policies(
                workspace_id,
                team_id,
                project_id,
                role_id,
                code,
                display_name,
                object_class,
                scope_type,
                precedence,
                can_read,
                can_write,
                can_link,
                can_import,
                can_promote,
                can_share_further,
                can_archive,
                can_delete,
                can_quarantine,
                can_approve_transfer,
                human_override,
                override_reason,
                status
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22
            )
            ON CONFLICT (workspace_id, code) DO UPDATE SET
                team_id = EXCLUDED.team_id,
                project_id = EXCLUDED.project_id,
                role_id = EXCLUDED.role_id,
                display_name = EXCLUDED.display_name,
                object_class = EXCLUDED.object_class,
                scope_type = EXCLUDED.scope_type,
                precedence = EXCLUDED.precedence,
                can_read = EXCLUDED.can_read,
                can_write = EXCLUDED.can_write,
                can_link = EXCLUDED.can_link,
                can_import = EXCLUDED.can_import,
                can_promote = EXCLUDED.can_promote,
                can_share_further = EXCLUDED.can_share_further,
                can_archive = EXCLUDED.can_archive,
                can_delete = EXCLUDED.can_delete,
                can_quarantine = EXCLUDED.can_quarantine,
                can_approve_transfer = EXCLUDED.can_approve_transfer,
                human_override = EXCLUDED.human_override,
                override_reason = EXCLUDED.override_reason,
                status = EXCLUDED.status,
                updated_at = now()
            RETURNING
                access_policy_id,
                $23::text,
                $24::text,
                $25::text,
                $26::text,
                code,
                display_name,
                object_class,
                scope_type,
                precedence,
                can_read,
                can_write,
                can_link,
                can_import,
                can_promote,
                can_share_further,
                can_archive,
                can_delete,
                can_quarantine,
                can_approve_transfer,
                human_override,
                override_reason,
                status
            "#,
            &[
                &workspace.workspace_id,
                &team.as_ref().map(|item| item.0),
                &project.as_ref().map(|item| item.project_id),
                &role.as_ref().map(|item| item.0),
                &code,
                &display_name,
                &object_class,
                &scope_type,
                &precedence,
                &can_read,
                &can_write,
                &can_link,
                &can_import,
                &can_promote,
                &can_share_further,
                &can_archive,
                &can_delete,
                &can_quarantine,
                &can_approve_transfer,
                &human_override,
                &override_reason,
                &status,
                &workspace.code,
                &team.as_ref().map(|item| item.1.clone()),
                &project.as_ref().map(|item| item.code.clone()),
                &role.as_ref().map(|item| item.1.clone()),
            ],
        )
        .await
        .context("failed to ensure access policy")?;
    let policy = access_policy_record_from_row(&row);
    let source_event_ids = json!([format!("access_policy:{}", policy.code)]);
    let evidence_span = json!({
        "kind": "access_policy",
        "workspace_code": workspace.code,
        "object_class": policy.object_class,
        "scope_type": policy.scope_type,
        "role_code": policy.role_code,
        "team_code": policy.team_code,
        "project_code": policy.project_code,
    });
    let rule_payload = json!({
        "policy_surface": "access_policy",
        "display_name": policy.display_name,
        "object_class": policy.object_class,
        "scope_type": policy.scope_type,
        "rights": {
            "can_read": policy.can_read,
            "can_write": policy.can_write,
            "can_link": policy.can_link,
            "can_import": policy.can_import,
            "can_promote": policy.can_promote,
            "can_share_further": policy.can_share_further,
            "can_archive": policy.can_archive,
            "can_delete": policy.can_delete,
            "can_quarantine": policy.can_quarantine,
            "can_approve_transfer": policy.can_approve_transfer,
        },
        "human_override": policy.human_override,
        "override_reason": policy.override_reason,
        "role_code": policy.role_code,
        "team_code": policy.team_code,
        "project_code": policy.project_code,
    });
    let rule_scope = if policy.project_code.is_some() {
        "project"
    } else if policy.team_code.is_some() || policy.role_code.is_some() {
        "shared"
    } else {
        "workspace"
    };
    let rule_status = match policy.status.as_str() {
        "active" => "active",
        "disabled" => "disabled",
        "archived" => "archived",
        _ => "active",
    };
    let _ = create_policy_rule(
        client,
        workspace_code,
        &PolicyRuleInsert {
            project_code: policy.project_code.as_deref(),
            namespace_code: None,
            rule_code: &format!("access_policy:{}", policy.code),
            rule_scope,
            rule_kind: "scope_filter",
            rule_status: Some(rule_status),
            precedence: Some(policy.precedence),
            source_kind: Some("access_policy_runtime"),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: None,
            message_refs: None,
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("operator_write"),
            schema_version: Some("policy-rule-envelope-v1"),
            rule_payload: &rule_payload,
        },
    )
    .await?;
    Ok(policy)
}

pub async fn list_access_policies(
    client: &Client,
    workspace_code: Option<&str>,
    policy_code: Option<&str>,
) -> Result<Vec<AccessPolicyRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                ap.access_policy_id,
                w.code,
                t.code,
                p.code,
                r.code,
                ap.code,
                ap.display_name,
                ap.object_class,
                ap.scope_type,
                ap.precedence,
                ap.can_read,
                ap.can_write,
                ap.can_link,
                ap.can_import,
                ap.can_promote,
                ap.can_share_further,
                ap.can_archive,
                ap.can_delete,
                ap.can_quarantine,
                ap.can_approve_transfer,
                ap.human_override,
                ap.override_reason,
                ap.status
            FROM ami.access_policies ap
            INNER JOIN ami.workspaces w ON w.workspace_id = ap.workspace_id
            LEFT JOIN ami.teams t ON t.team_id = ap.team_id
            LEFT JOIN ami.projects p ON p.project_id = ap.project_id
            LEFT JOIN ami.agent_roles r ON r.role_id = ap.role_id
            WHERE ($1::text IS NULL OR w.code = $1)
              AND ($2::text IS NULL OR ap.code = $2)
            ORDER BY w.code, ap.precedence DESC, ap.code
            "#,
            &[&workspace_code, &policy_code],
        )
        .await
        .context("failed to list access policies")?;
    Ok(rows
        .into_iter()
        .map(|row| access_policy_record_from_row(&row))
        .collect())
}

pub async fn get_access_policy(
    client: &Client,
    access_policy_id: Uuid,
) -> Result<AccessPolicyRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                ap.access_policy_id,
                w.code,
                t.code,
                p.code,
                r.code,
                ap.code,
                ap.display_name,
                ap.object_class,
                ap.scope_type,
                ap.precedence,
                ap.can_read,
                ap.can_write,
                ap.can_link,
                ap.can_import,
                ap.can_promote,
                ap.can_share_further,
                ap.can_archive,
                ap.can_delete,
                ap.can_quarantine,
                ap.can_approve_transfer,
                ap.human_override,
                ap.override_reason,
                ap.status
            FROM ami.access_policies ap
            INNER JOIN ami.workspaces w ON w.workspace_id = ap.workspace_id
            LEFT JOIN ami.teams t ON t.team_id = ap.team_id
            LEFT JOIN ami.projects p ON p.project_id = ap.project_id
            LEFT JOIN ami.agent_roles r ON r.role_id = ap.role_id
            WHERE ap.access_policy_id = $1
            "#,
            &[&access_policy_id],
        )
        .await
        .with_context(|| format!("failed to load access policy {}", access_policy_id))?;
    Ok(access_policy_record_from_row(&row))
}

pub(super) async fn find_transfer_policy_by_code(
    client: &Client,
    code: &str,
) -> Result<Option<TransferPolicyRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                tp.transfer_policy_id,
                w.code,
                tp.code,
                tp.display_name,
                tp.default_decision,
                tp.allow_cross_project_read,
                tp.allow_import,
                tp.allow_verified_writeback,
                tp.requires_human_approval
            FROM ami.transfer_policies tp
            INNER JOIN ami.workspaces w ON w.workspace_id = tp.workspace_id
            WHERE tp.code = $1
            ORDER BY w.code
            "#,
            &[&code],
        )
        .await
        .with_context(|| format!("failed to lookup transfer policy {code}"))?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() > 1 {
        return Err(anyhow!(
            "transfer policy code is ambiguous across workspaces: {code}"
        ));
    }
    Ok(rows
        .into_iter()
        .next()
        .map(|row| transfer_policy_record_from_row(&row)))
}

pub(super) async fn find_agent_id_by_code(client: &Client, code: &str) -> Result<Option<Uuid>> {
    let row = client
        .query_opt(
            r#"
            SELECT agent_id
            FROM ami.agents
            WHERE code = $1
            "#,
            &[&code],
        )
        .await
        .with_context(|| format!("failed to lookup agent id for code {code}"))?;
    Ok(row.map(|row| row.get(0)))
}

fn autonomous_quarantine_governor_agent_code(workspace_code: &str) -> String {
    let mut normalized = workspace_code
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    normalized = normalized.trim_matches('-').to_string();
    if normalized.is_empty() {
        normalized = "default".to_string();
    }
    format!("amai-autonomous-governor-{normalized}")
}

pub(super) async fn ensure_autonomous_quarantine_governor_agent(
    client: &Client,
    workspace_code: &str,
) -> Result<String> {
    let code = autonomous_quarantine_governor_agent_code(workspace_code);
    let display_name = format!("Amai Autonomous Governor ({workspace_code})");
    let _ = ensure_agent(
        client,
        workspace_code,
        None,
        None,
        &code,
        &display_name,
        "workspace_private",
        "active",
    )
    .await?;
    Ok(code)
}
