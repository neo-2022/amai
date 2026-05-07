use super::*;

pub async fn upsert_project(
    client: &Client,
    code: &str,
    display_name: &str,
    repo_root: &str,
    default_branch: Option<&str>,
    workspace_code: &str,
    visibility_scope: &str,
    default_mode: &str,
) -> Result<ProjectRecord> {
    let workspace = get_workspace_by_code(client, workspace_code).await?;
    let canonical_repo_root = canonical_repo_root_string(repo_root)?;
    if let Some(existing) = get_bound_project_for_repo_root(client, &canonical_repo_root).await? {
        if existing.code != code {
            return Err(anyhow!(
                "canonical repo_root {} is already registered as project {} (display_name: {}); alias code {} is blocked",
                canonical_repo_root,
                existing.code,
                existing.display_name,
                code
            ));
        }
    }

    let previous_project = client
        .query_opt(
            r#"
            SELECT
                project_id,
                code,
                display_name,
                repo_root,
                visibility_scope,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.projects
            WHERE code = $1
            "#,
            &[&code],
        )
        .await?
        .as_ref()
        .map(project_record_from_row);

    let row = client
        .query_one(
            r#"
            INSERT INTO ami.projects(workspace_id, code, display_name, repo_root, default_branch, visibility_scope)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (code) DO UPDATE SET
                workspace_id = EXCLUDED.workspace_id,
                display_name = EXCLUDED.display_name,
                repo_root = EXCLUDED.repo_root,
                default_branch = EXCLUDED.default_branch,
                visibility_scope = EXCLUDED.visibility_scope,
                updated_at = now()
            RETURNING
                project_id,
                code,
                display_name,
                repo_root,
                visibility_scope,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            "#,
            &[
                &workspace.workspace_id,
                &code,
                &display_name,
                &canonical_repo_root,
                &default_branch,
                &visibility_scope,
            ],
        )
        .await
        .context("failed to upsert project")?;

    let project = project_record_from_row(&row);
    sync_project_repo_roots(
        client,
        &project,
        previous_project
            .as_ref()
            .map(|item| item.repo_root.as_str()),
    )
    .await?;

    ensure_namespace(
        client,
        project.project_id,
        "default",
        Some("Default"),
        default_mode,
    )
    .await?;

    Ok(project)
}

pub async fn list_projects(
    client: &Client,
    project_code: Option<&str>,
    repo_root: Option<&str>,
) -> Result<Vec<ProjectRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                project_id,
                code,
                display_name,
                repo_root,
                visibility_scope,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.projects
            WHERE ($1::text IS NULL OR code = $1)
              AND ($2::text IS NULL OR repo_root = $2)
            ORDER BY code
            "#,
            &[&project_code, &repo_root],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| project_record_from_row(&row))
        .collect())
}

pub async fn get_project_by_code(client: &Client, code: &str) -> Result<ProjectRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                project_id,
                code,
                display_name,
                repo_root,
                visibility_scope,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.projects
            WHERE code = $1
            "#,
            &[&code],
        )
        .await?
        .ok_or_else(|| anyhow!("project not found: {code}"))?;
    Ok(project_record_from_row(&row))
}

pub async fn get_project_by_id(client: &Client, project_id: Uuid) -> Result<ProjectRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                project_id,
                code,
                display_name,
                repo_root,
                visibility_scope,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.projects
            WHERE project_id = $1
            "#,
            &[&project_id],
        )
        .await?
        .ok_or_else(|| anyhow!("project not found by id: {project_id}"))?;
    Ok(project_record_from_row(&row))
}

pub async fn get_project_by_repo_root(client: &Client, repo_root: &str) -> Result<ProjectRecord> {
    let canonical_repo_root = canonical_repo_root_string(repo_root)?;
    get_bound_project_for_repo_root(client, &canonical_repo_root)
        .await?
        .ok_or_else(|| anyhow!("project not found for repo_root: {canonical_repo_root}"))
}

pub async fn project_has_repo_root(
    client: &Client,
    project_id: Uuid,
    repo_root: &str,
) -> Result<bool> {
    let canonical_repo_root = canonical_repo_root_string(repo_root)?;
    let row = client
        .query_one(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM ami.project_repo_roots
                WHERE project_id = $1
                  AND repo_root = $2
            )
            "#,
            &[&project_id, &canonical_repo_root],
        )
        .await
        .context("failed to check project repo_root binding")?;
    Ok(row.get(0))
}

pub fn canonical_repo_root_string(repo_root: &str) -> Result<String> {
    let canonical = Path::new(repo_root)
        .canonicalize()
        .with_context(|| format!("failed to resolve repo_root {}", repo_root))?;
    if !canonical.is_dir() {
        return Err(anyhow!(
            "repo_root must resolve to a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical.display().to_string())
}

pub async fn ensure_namespace(
    client: &Client,
    project_id: Uuid,
    code: &str,
    display_name: Option<&str>,
    retrieval_mode: &str,
) -> Result<NamespaceRecord> {
    let display_name = display_name.unwrap_or(code);
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.namespaces(project_id, code, display_name, retrieval_mode)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (project_id, code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                retrieval_mode = EXCLUDED.retrieval_mode,
                updated_at = now()
            RETURNING namespace_id, code, display_name, retrieval_mode
            "#,
            &[&project_id, &code, &display_name, &retrieval_mode],
        )
        .await
        .context("failed to ensure namespace")?;
    Ok(NamespaceRecord {
        namespace_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        retrieval_mode: row.get(3),
    })
}

pub async fn get_namespace_by_code(
    client: &Client,
    project_id: Uuid,
    code: &str,
) -> Result<NamespaceRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT namespace_id, code, display_name, retrieval_mode
            FROM ami.namespaces
            WHERE project_id = $1 AND code = $2
            "#,
            &[&project_id, &code],
        )
        .await?
        .ok_or_else(|| anyhow!("namespace not found: {code}"))?;
    Ok(NamespaceRecord {
        namespace_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        retrieval_mode: row.get(3),
    })
}

pub async fn get_namespace_by_id(client: &Client, namespace_id: Uuid) -> Result<NamespaceRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT namespace_id, code, display_name, retrieval_mode
            FROM ami.namespaces
            WHERE namespace_id = $1
            "#,
            &[&namespace_id],
        )
        .await?
        .ok_or_else(|| anyhow!("namespace not found by id: {namespace_id}"))?;
    Ok(NamespaceRecord {
        namespace_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        retrieval_mode: row.get(3),
    })
}

pub async fn get_memory_item_by_id(
    client: &Client,
    memory_item_id: Uuid,
) -> Result<MemoryItemRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                mi.memory_item_id,
                w.code,
                p.code,
                n.code,
                sp.code,
                mi.import_packet_id,
                mi.owner_agent_id,
                mi.visibility_scope,
                mi.item_kind,
                mi.identity_key,
                mi.title,
                mi.summary,
                mi.body,
                mi.sensitivity_class,
                mi.truth_state,
                mi.trust_state,
                mi.verification_state,
                mi.lifecycle_state,
                mi.source_event_ids,
                mi.artifact_refs,
                mi.message_refs,
                mi.evidence_span,
                mi.derivation_kind,
                mi.observed_at_epoch_ms,
                mi.recorded_at_epoch_ms,
                mi.valid_from_epoch_ms,
                mi.valid_to_epoch_ms,
                mi.last_verified_at_epoch_ms,
                mi.ingest_seq,
                mi.object_version,
                mi.causation_id,
                mi.correlation_id,
                mi.utility_score,
                mi.freshness_score,
                mi.retention_class,
                mi.ttl_epoch_ms,
                mi.access_count,
                to_char(mi.last_accessed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                mi.decay_policy,
                mi.consolidation_status,
                mi.imported_from,
                mi.schema_version,
                mi.superseded_by_memory_item_id,
                mi.metadata
            FROM ami.memory_items mi
            INNER JOIN ami.workspaces w ON w.workspace_id = mi.workspace_id
            INNER JOIN ami.projects p ON p.project_id = mi.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = mi.namespace_id
            LEFT JOIN ami.projects sp ON sp.project_id = mi.source_project_id
            WHERE mi.memory_item_id = $1
            "#,
            &[&memory_item_id],
        )
        .await?
        .ok_or_else(|| anyhow!("memory item not found: {memory_item_id}"))?;
    Ok(memory_item_record_from_row(&row))
}
