use crate::{config::AppConfig, observability_policy};
use anyhow::{Context, Result, anyhow};
use native_tls::TlsConnector as NativeTlsConnector;
use postgres_native_tls::MakeTlsConnector;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio_postgres::config::{Host, SslMode};
use tokio_postgres::{Client, Config as PostgresConfig, NoTls, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProjectRecord {
    pub project_id: Uuid,
    pub code: String,
    pub display_name: String,
    pub repo_root: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NamespaceRecord {
    pub namespace_id: Uuid,
    pub code: String,
    pub display_name: String,
    pub retrieval_mode: String,
}

#[derive(Debug, Clone)]
pub struct VisibleProjectRecord {
    pub project: ProjectRecord,
    pub relation_type: String,
    pub shared_contour: String,
    pub access_mode: String,
}

#[derive(Debug, Clone)]
pub struct SymbolRecord {
    pub name: String,
    pub kind: String,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub chunk_id: Uuid,
    pub qdrant_point_id: Option<Uuid>,
    pub qdrant_collection_alias: Option<String>,
    pub chunk_index: i32,
    pub total_chunks: i32,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub content: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DocumentRecord {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub repo_root: String,
    pub absolute_path: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub file_sha256: String,
    pub line_count: i32,
    pub byte_count: i64,
    pub content: String,
    pub metrics: Value,
    pub structure: Value,
    pub imports: Value,
    pub exports: Value,
    pub diagnostics: Value,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DocumentStructureRecord {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub structure: Value,
    pub imports: Value,
    pub exports: Value,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DocumentScopedSymbolRecord {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub name: String,
    pub kind: String,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ObservabilitySnapshotRecord {
    pub snapshot_id: Uuid,
    pub snapshot_kind: String,
    pub payload: Value,
    pub created_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ExecCtlTaskLedgerEntryRecord {
    pub ledger_entry_id: Uuid,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: String,
    pub event_kind: String,
    pub source_kind: String,
    pub agent_scope: String,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub headline: String,
    pub next_step: String,
    pub summary: String,
    pub active_files: Value,
    pub open_questions: Value,
    pub materialized_notes: Value,
    pub pending_return_queue: Value,
    pub local_path: Option<String>,
    pub recorded_at_epoch_ms: i64,
    pub created_at_epoch_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ObservabilityRetentionCandidate {
    pub snapshot_id: Uuid,
    pub snapshot_kind: String,
    pub payload: Value,
    pub source_kind: String,
    pub source_class: String,
    pub created_at_epoch_ms: i64,
    pub captured_at_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservabilityInsertMeta {
    event_key: String,
    source_event_id: Option<String>,
    source_kind: String,
    source_class: String,
    scope_project_code: Option<String>,
    scope_namespace_code: Option<String>,
    captured_at_epoch_ms: Option<i64>,
    payload_sha256: String,
}

#[derive(Debug, Clone)]
pub struct DocumentHit {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub language: Option<String>,
    pub source_kind: String,
    pub git_commit_sha: Option<String>,
    pub score: f32,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct SymbolHit {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub name: String,
    pub kind: String,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub score: f32,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ChunkHit {
    pub project_code: String,
    pub namespace_code: String,
    pub repo_root: String,
    pub relative_path: String,
    pub chunk_id: Uuid,
    pub chunk_index: i32,
    pub start_line: i32,
    pub end_line: i32,
    pub score: f32,
    pub content: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ArtifactRefInsert<'a> {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub artifact_kind: &'a str,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub content_type: Option<&'a str>,
    pub metadata: &'a Value,
}

#[derive(Debug, Clone)]
pub struct ContextPackInsert<'a> {
    pub context_pack_id: Uuid,
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub retrieval_mode: &'a str,
    pub query_text: &'a str,
    pub visible_projects: &'a Value,
    pub payload: &'a Value,
    pub artifact_ref_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ExecCtlTaskLedgerEntryInsert<'a> {
    pub project_id: Uuid,
    pub namespace_id: Uuid,
    pub agent_scope: &'a str,
    pub session_id: Option<&'a str>,
    pub thread_id: Option<&'a str>,
    pub source_snapshot_id: Option<Uuid>,
    pub source_event_id: &'a str,
    pub event_kind: &'a str,
    pub source_kind: &'a str,
    pub headline: &'a str,
    pub next_step: &'a str,
    pub summary: &'a str,
    pub active_files: &'a Value,
    pub open_questions: &'a Value,
    pub materialized_notes: &'a Value,
    pub pending_return_queue: &'a Value,
    pub local_path: Option<&'a str>,
    pub recorded_at_epoch_ms: i64,
}

pub async fn connect_admin(cfg: &AppConfig) -> Result<Client> {
    connect(&cfg.postgres_dsn).await
}

pub async fn connect_app(cfg: &AppConfig) -> Result<Client> {
    connect(&cfg.app_postgres_dsn).await
}

async fn connect(dsn: &str) -> Result<Client> {
    let config: PostgresConfig = dsn
        .parse()
        .with_context(|| format!("invalid postgres dsn {}", safe_postgres_descriptor(dsn)))?;
    let masked_descriptor = safe_postgres_descriptor_from_config(&config);
    let ssl_mode = config.get_ssl_mode();
    match ssl_mode {
        SslMode::Disable => {
            let (client, connection) = config.connect(NoTls).await.with_context(|| {
                format!("failed to connect to postgres via {masked_descriptor}")
            })?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(?error, "postgres connection task ended with error");
                }
            });
            Ok(client)
        }
        _ => {
            let connector = build_tls_connector().with_context(|| {
                format!("failed to initialize postgres TLS for {masked_descriptor}")
            })?;
            let (client, connection) = config.connect(connector).await.with_context(|| {
                format!("failed to connect to postgres via {masked_descriptor}")
            })?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(?error, "postgres connection task ended with error");
                }
            });
            Ok(client)
        }
    }
}

fn build_tls_connector() -> Result<MakeTlsConnector> {
    let connector = NativeTlsConnector::builder()
        .build()
        .context("failed to build native TLS connector")?;
    Ok(MakeTlsConnector::new(connector))
}

fn safe_postgres_descriptor(dsn: &str) -> String {
    dsn.parse::<PostgresConfig>()
        .map(|config| safe_postgres_descriptor_from_config(&config))
        .unwrap_or_else(|_| "postgres://[redacted-invalid-dsn]".to_string())
}

fn safe_postgres_descriptor_from_config(config: &PostgresConfig) -> String {
    let user = config.get_user().unwrap_or("unknown");
    let dbname = config.get_dbname().unwrap_or("postgres");
    let ssl_mode = match config.get_ssl_mode() {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        _ => "unknown",
    };
    let host = config
        .get_hosts()
        .first()
        .map(postgres_host_label)
        .unwrap_or_else(|| "localhost".to_string());
    let port = config.get_ports().first().copied().unwrap_or(5432);
    format!(
        "postgres://{}:***@{}:{}/{}?sslmode={}",
        user, host, port, dbname, ssl_mode
    )
}

fn postgres_host_label(host: &Host) -> String {
    match host {
        Host::Tcp(host) => host.clone(),
        #[cfg(unix)]
        Host::Unix(path) => format!("unix:{}", path.display()),
    }
}

pub async fn bootstrap_schema(client: &Client, cfg: &AppConfig) -> Result<()> {
    client
        .batch_execute(include_str!("../sql/000_bootstrap.sql"))
        .await
        .context("failed to apply postgres schema")?;
    ensure_app_role(client, cfg).await?;
    Ok(())
}

fn project_record_from_row(row: &Row) -> ProjectRecord {
    ProjectRecord {
        project_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        repo_root: row.get(3),
        updated_at: row.get(4),
    }
}

async fn get_bound_project_for_repo_root(
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

async fn sync_project_repo_roots(
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

async fn ensure_app_role(client: &Client, cfg: &AppConfig) -> Result<()> {
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
        GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA ami TO {user};
        GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA ami TO {user};
        ALTER DEFAULT PRIVILEGES IN SCHEMA ami GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {user};
        ALTER DEFAULT PRIVILEGES IN SCHEMA ami GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO {user};
        "#,
        raw_user = cfg.app_db_user.replace('\'', "''"),
    );
    client
        .batch_execute(&role_sql)
        .await
        .context("failed to create/grant app role")?;
    Ok(())
}

pub async fn upsert_project(
    client: &Client,
    code: &str,
    display_name: &str,
    repo_root: &str,
    default_branch: Option<&str>,
    default_mode: &str,
) -> Result<ProjectRecord> {
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
            INSERT INTO ami.projects(code, display_name, repo_root, default_branch)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (code) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                repo_root = EXCLUDED.repo_root,
                default_branch = EXCLUDED.default_branch,
                updated_at = now()
            RETURNING
                project_id,
                code,
                display_name,
                repo_root,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            "#,
            &[&code, &display_name, &canonical_repo_root, &default_branch],
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

pub async fn list_projects(client: &Client) -> Result<Vec<ProjectRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                project_id,
                code,
                display_name,
                repo_root,
                to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.projects
            ORDER BY code
            "#,
            &[],
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

fn canonical_repo_root_string(repo_root: &str) -> Result<String> {
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
    shared_contour: &str,
    access_mode: &str,
) -> Result<()> {
    let source = get_project_by_code(client, source_code).await?;
    let target = get_project_by_code(client, target_code).await?;
    client
        .execute(
            r#"
            INSERT INTO ami.project_relations(source_project_id, target_project_id, relation_type, shared_contour, access_mode)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (source_project_id, target_project_id, relation_type, shared_contour) DO UPDATE SET
                access_mode = EXCLUDED.access_mode
            "#,
            &[&source.project_id, &target.project_id, &relation_type, &shared_contour, &access_mode],
        )
        .await
        .context("failed to add relation")?;
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
                to_char(p.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                r.relation_type,
                r.shared_contour,
                r.access_mode
            FROM ami.project_relations r
            JOIN ami.projects p ON p.project_id = r.target_project_id
            WHERE r.source_project_id = $1
            ORDER BY p.code, r.relation_type, r.shared_contour
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
                updated_at: row.get(4),
            },
            relation_type: row.get(5),
            shared_contour: row.get(6),
            access_mode: row.get(7),
        })
        .collect())
}

pub async fn search_documents_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<DocumentHit>> {
    let rows = client
        .query(
            r#"
            SELECT
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                d.language,
                d.source_kind,
                d.git_commit_sha,
                ts_rank_cd(d.search_vector, websearch_to_tsquery('simple', $3)) AS score,
                LEFT(d.content, 1600)
            FROM ami.code_documents d
            JOIN ami.projects p ON p.project_id = d.project_id
            JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
            WHERE d.project_id = $1
              AND d.namespace_id = $2
              AND d.search_vector @@ websearch_to_tsquery('simple', $3)
            ORDER BY score DESC, d.relative_path
            LIMIT $4
            "#,
            &[&project_id, &namespace_id, &query, &limit],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| DocumentHit {
            project_code: row.get(0),
            namespace_code: row.get(1),
            repo_root: row.get(2),
            relative_path: row.get(3),
            language: row.get(4),
            source_kind: row.get(5),
            git_commit_sha: row.get(6),
            score: row.get(7),
            snippet: row.get(8),
        })
        .collect())
}

pub async fn search_documents_exact_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<DocumentHit>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let basename_query = exact_match_basename(query);
    let basename_stem_query = exact_match_basename_stem(&basename_query);
    let allow_extensionless_basename_match = basename_query == basename_stem_query;
    let mut hits = Vec::new();

    hits.extend(
        client
            .query(
                r#"
                SELECT
                    p.code AS project_code,
                    n.code AS namespace_code,
                    d.repo_root,
                    d.relative_path,
                    d.language,
                    d.source_kind,
                    d.git_commit_sha,
                    2000.0::real AS score,
                    LEFT(d.content, 1600) AS snippet
                FROM ami.code_documents d
                JOIN ami.projects p ON p.project_id = d.project_id
                JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
                WHERE d.project_id = $1
                  AND d.namespace_id = $2
                  AND d.relative_path = $3
                ORDER BY length(d.relative_path), d.relative_path
                LIMIT $4
                "#,
                &[&project_id, &namespace_id, &query, &limit],
            )
            .await?
            .into_iter()
            .map(document_hit_from_row),
    );

    if (hits.len() as i64) < limit {
        let remaining = limit - hits.len() as i64;
        hits.extend(
            client
                .query(
                    r#"
                    SELECT
                        p.code AS project_code,
                        n.code AS namespace_code,
                        d.repo_root,
                        d.relative_path,
                        d.language,
                        d.source_kind,
                        d.git_commit_sha,
                        1500.0::real AS score,
                        LEFT(d.content, 1600) AS snippet
                    FROM ami.code_documents d
                    JOIN ami.projects p ON p.project_id = d.project_id
                    JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
                    WHERE d.project_id = $1
                      AND d.namespace_id = $2
                      AND d.relative_basename = $3
                      AND d.relative_path <> $4
                    ORDER BY length(d.relative_path), d.relative_path
                    LIMIT $5
                    "#,
                    &[
                        &project_id,
                        &namespace_id,
                        &basename_query,
                        &query,
                        &remaining,
                    ],
                )
                .await?
                .into_iter()
                .map(document_hit_from_row),
        );
    }

    if allow_extensionless_basename_match && (hits.len() as i64) < limit {
        let remaining = limit - hits.len() as i64;
        hits.extend(
            client
                .query(
                    r#"
                    SELECT
                        p.code AS project_code,
                        n.code AS namespace_code,
                        d.repo_root,
                        d.relative_path,
                        d.language,
                        d.source_kind,
                        d.git_commit_sha,
                        1400.0::real AS score,
                        LEFT(d.content, 1600) AS snippet
                    FROM ami.code_documents d
                    JOIN ami.projects p ON p.project_id = d.project_id
                    JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
                    WHERE d.project_id = $1
                      AND d.namespace_id = $2
                      AND d.relative_basename_stem = $3
                      AND d.relative_path <> $4
                      AND d.relative_basename <> $5
                    ORDER BY length(d.relative_path), d.relative_path
                    LIMIT $6
                    "#,
                    &[
                        &project_id,
                        &namespace_id,
                        &basename_stem_query,
                        &query,
                        &basename_query,
                        &remaining,
                    ],
                )
                .await?
                .into_iter()
                .map(document_hit_from_row),
        );
    }

    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.relative_path.len().cmp(&right.relative_path.len()))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    hits.truncate(limit as usize);
    Ok(hits)
}

fn document_hit_from_row(row: Row) -> DocumentHit {
    DocumentHit {
        project_code: row.get(0),
        namespace_code: row.get(1),
        repo_root: row.get(2),
        relative_path: row.get(3),
        language: row.get(4),
        source_kind: row.get(5),
        git_commit_sha: row.get(6),
        score: row.get(7),
        snippet: row.get(8),
    }
}

fn exact_match_basename(query: &str) -> String {
    query.rsplit('/').next().unwrap_or(query).to_string()
}

fn exact_match_basename_stem(basename: &str) -> String {
    basename
        .rsplit_once('.')
        .map(|(stem, _)| stem.to_string())
        .unwrap_or_else(|| basename.to_string())
}

pub async fn search_symbols_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<SymbolHit>> {
    let rows = client
        .query(
            r#"
            SELECT
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                s.name,
                s.kind,
                s.start_line,
                s.end_line,
                s.start_byte,
                s.end_byte,
                ts_rank_cd(s.search_vector, websearch_to_tsquery('simple', $3)) AS score,
                s.metadata
            FROM ami.code_symbols s
            JOIN ami.code_documents d ON d.document_id = s.document_id
            JOIN ami.projects p ON p.project_id = s.project_id
            JOIN ami.namespaces n ON n.namespace_id = s.namespace_id
            WHERE s.project_id = $1
              AND s.namespace_id = $2
              AND s.search_vector @@ websearch_to_tsquery('simple', $3)
            ORDER BY score DESC, d.relative_path, s.start_line
            LIMIT $4
            "#,
            &[&project_id, &namespace_id, &query, &limit],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| SymbolHit {
            project_code: row.get(0),
            namespace_code: row.get(1),
            repo_root: row.get(2),
            relative_path: row.get(3),
            name: row.get(4),
            kind: row.get(5),
            start_line: row.get(6),
            end_line: row.get(7),
            start_byte: row.get(8),
            end_byte: row.get(9),
            score: row.get(10),
            metadata: row.get(11),
        })
        .collect())
}

pub async fn search_symbols_exact_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<SymbolHit>> {
    let rows = client
        .query(
            r#"
            SELECT
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                s.name,
                s.kind,
                s.start_line,
                s.end_line,
                s.start_byte,
                s.end_byte,
                2000.0::real AS score,
                s.metadata
            FROM ami.code_symbols s
            JOIN ami.code_documents d ON d.document_id = s.document_id
            JOIN ami.projects p ON p.project_id = s.project_id
            JOIN ami.namespaces n ON n.namespace_id = s.namespace_id
            WHERE s.project_id = $1
              AND s.namespace_id = $2
              AND s.name = $3
            ORDER BY d.relative_path, s.start_line
            LIMIT $4
            "#,
            &[&project_id, &namespace_id, &query, &limit],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| SymbolHit {
            project_code: row.get(0),
            namespace_code: row.get(1),
            repo_root: row.get(2),
            relative_path: row.get(3),
            name: row.get(4),
            kind: row.get(5),
            start_line: row.get(6),
            end_line: row.get(7),
            start_byte: row.get(8),
            end_byte: row.get(9),
            score: row.get(10),
            metadata: row.get(11),
        })
        .collect())
}

pub async fn search_chunks_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<ChunkHit>> {
    let rows = client
        .query(
            r#"
            SELECT
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                c.chunk_id,
                c.chunk_index,
                c.start_line,
                c.end_line,
                ts_rank_cd(c.search_vector, websearch_to_tsquery('simple', $3)) AS score,
                LEFT(c.content, 2000),
                c.metadata
            FROM ami.code_chunks c
            JOIN ami.code_documents d ON d.document_id = c.document_id
            JOIN ami.projects p ON p.project_id = c.project_id
            JOIN ami.namespaces n ON n.namespace_id = c.namespace_id
            WHERE c.project_id = $1
              AND c.namespace_id = $2
              AND c.search_vector @@ websearch_to_tsquery('simple', $3)
            ORDER BY score DESC, d.relative_path, c.chunk_index
            LIMIT $4
            "#,
            &[&project_id, &namespace_id, &query, &limit],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| ChunkHit {
            project_code: row.get(0),
            namespace_code: row.get(1),
            repo_root: row.get(2),
            relative_path: row.get(3),
            chunk_id: row.get(4),
            chunk_index: row.get(5),
            start_line: row.get(6),
            end_line: row.get(7),
            score: row.get(8),
            content: row.get(9),
            metadata: row.get(10),
        })
        .collect())
}

pub async fn list_chunks_by_qdrant_point_ids(
    client: &Client,
    point_ids: &[Uuid],
) -> Result<Vec<(Uuid, ChunkHit)>> {
    if point_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = client
        .query(
            r#"
            SELECT
                c.qdrant_point_id,
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                c.chunk_id,
                c.chunk_index,
                c.start_line,
                c.end_line,
                LEFT(c.content, 2000),
                c.metadata
            FROM ami.code_chunks c
            JOIN ami.code_documents d ON d.document_id = c.document_id
            JOIN ami.projects p ON p.project_id = c.project_id
            JOIN ami.namespaces n ON n.namespace_id = c.namespace_id
            WHERE c.qdrant_point_id = ANY($1)
            "#,
            &[&point_ids],
        )
        .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let point_id = row.get::<_, Option<Uuid>>(0)?;
            Some((
                point_id,
                ChunkHit {
                    project_code: row.get(1),
                    namespace_code: row.get(2),
                    repo_root: row.get(3),
                    relative_path: row.get(4),
                    chunk_id: row.get(5),
                    chunk_index: row.get(6),
                    start_line: row.get(7),
                    end_line: row.get(8),
                    score: 0.0,
                    content: row.get(9),
                    metadata: row.get(10),
                },
            ))
        })
        .collect())
}

pub async fn list_document_structures_for_namespace_paths(
    client: &Client,
    requests: &[(Uuid, String)],
) -> Result<Vec<DocumentStructureRecord>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let namespace_ids = requests
        .iter()
        .map(|(namespace_id, _)| *namespace_id)
        .collect::<Vec<_>>();
    let relative_paths = requests
        .iter()
        .map(|(_, relative_path)| relative_path.clone())
        .collect::<Vec<_>>();
    let rows = client
        .query(
            r#"
            WITH requested(namespace_id, relative_path) AS (
                SELECT * FROM unnest($1::uuid[], $2::text[])
            )
            SELECT
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                d.language,
                d.source_kind,
                d.git_commit_sha,
                d.structure,
                d.imports,
                d.exports,
                d.metadata
            FROM requested r
            JOIN ami.code_documents d
              ON d.namespace_id = r.namespace_id
             AND d.relative_path = r.relative_path
            JOIN ami.projects p ON p.project_id = d.project_id
            JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
            ORDER BY p.code, n.code, d.relative_path
            "#,
            &[&namespace_ids, &relative_paths],
        )
        .await
        .context("failed to list document structures for scoped paths")?;
    Ok(rows
        .into_iter()
        .map(|row| DocumentStructureRecord {
            project_code: row.get(0),
            namespace_code: row.get(1),
            repo_root: row.get(2),
            relative_path: row.get(3),
            language: row.get(4),
            source_kind: row.get(5),
            git_commit_sha: row.get(6),
            structure: row.get(7),
            imports: row.get(8),
            exports: row.get(9),
            metadata: row.get(10),
        })
        .collect())
}

pub async fn list_document_symbols_for_namespace_paths(
    client: &Client,
    requests: &[(Uuid, String)],
) -> Result<Vec<DocumentScopedSymbolRecord>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let namespace_ids = requests
        .iter()
        .map(|(namespace_id, _)| *namespace_id)
        .collect::<Vec<_>>();
    let relative_paths = requests
        .iter()
        .map(|(_, relative_path)| relative_path.clone())
        .collect::<Vec<_>>();
    let rows = client
        .query(
            r#"
            WITH requested(namespace_id, relative_path) AS (
                SELECT * FROM unnest($1::uuid[], $2::text[])
            )
            SELECT
                p.code,
                n.code,
                d.repo_root,
                d.relative_path,
                d.language,
                d.source_kind,
                d.git_commit_sha,
                s.name,
                s.kind,
                s.start_line,
                s.end_line,
                s.start_byte,
                s.end_byte,
                s.metadata
            FROM requested r
            JOIN ami.code_documents d
              ON d.namespace_id = r.namespace_id
             AND d.relative_path = r.relative_path
            JOIN ami.code_symbols s ON s.document_id = d.document_id
            JOIN ami.projects p ON p.project_id = d.project_id
            JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
            ORDER BY p.code, n.code, d.relative_path, s.start_line
            "#,
            &[&namespace_ids, &relative_paths],
        )
        .await
        .context("failed to list document symbols for scoped paths")?;
    Ok(rows
        .into_iter()
        .map(|row| DocumentScopedSymbolRecord {
            project_code: row.get(0),
            namespace_code: row.get(1),
            repo_root: row.get(2),
            relative_path: row.get(3),
            language: row.get(4),
            source_kind: row.get(5),
            git_commit_sha: row.get(6),
            name: row.get(7),
            kind: row.get(8),
            start_line: row.get(9),
            end_line: row.get(10),
            start_byte: row.get(11),
            end_byte: row.get(12),
            metadata: row.get(13),
        })
        .collect())
}

pub async fn namespace_has_vector_points(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
) -> Result<bool> {
    let row = client
        .query_one(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM ami.code_chunks
                WHERE project_id = $1
                  AND namespace_id = $2
                  AND qdrant_point_id IS NOT NULL
            )
            "#,
            &[&project_id, &namespace_id],
        )
        .await?;
    Ok(row.get(0))
}

pub async fn insert_artifact_ref(client: &Client, record: &ArtifactRefInsert<'_>) -> Result<Uuid> {
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.artifact_refs(
                project_id, namespace_id, artifact_kind, bucket, object_key, content_type, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (bucket, object_key) DO UPDATE SET
                content_type = EXCLUDED.content_type,
                metadata = EXCLUDED.metadata
            RETURNING artifact_ref_id
            "#,
            &[
                &record.project_id,
                &record.namespace_id,
                &record.artifact_kind,
                &record.bucket,
                &record.object_key,
                &record.content_type,
                record.metadata,
            ],
        )
        .await
        .context("failed to upsert artifact ref")?;
    Ok(row.get(0))
}

pub async fn insert_context_pack(client: &Client, record: &ContextPackInsert<'_>) -> Result<()> {
    client
        .execute(
            r#"
            INSERT INTO ami.context_packs(
                context_pack_id, project_id, namespace_id, retrieval_mode,
                query_text, visible_projects, payload, artifact_ref_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            &[
                &record.context_pack_id,
                &record.project_id,
                &record.namespace_id,
                &record.retrieval_mode,
                &record.query_text,
                record.visible_projects,
                record.payload,
                &record.artifact_ref_id,
            ],
        )
        .await
        .context("failed to insert context pack")?;
    Ok(())
}

pub async fn replace_document_index(
    client: &mut Client,
    document: &DocumentRecord,
    symbols: &[SymbolRecord],
    chunks: &[ChunkRecord],
) -> Result<Uuid> {
    let inserted_document_id = Uuid::new_v4();
    let transaction = client.transaction().await?;
    let document_row = transaction
        .query_one(
            r#"
            INSERT INTO ami.code_documents(
                document_id, project_id, namespace_id, repo_root, absolute_path, relative_path,
                language, source_kind, git_commit_sha, file_sha256, line_count, byte_count,
                content, metrics, structure, imports, exports, diagnostics, metadata
            )
            VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17, $18, $19
            )
            ON CONFLICT (namespace_id, relative_path) DO UPDATE
            SET project_id = EXCLUDED.project_id,
                repo_root = EXCLUDED.repo_root,
                absolute_path = EXCLUDED.absolute_path,
                language = EXCLUDED.language,
                source_kind = EXCLUDED.source_kind,
                git_commit_sha = EXCLUDED.git_commit_sha,
                file_sha256 = EXCLUDED.file_sha256,
                line_count = EXCLUDED.line_count,
                byte_count = EXCLUDED.byte_count,
                content = EXCLUDED.content,
                metrics = EXCLUDED.metrics,
                structure = EXCLUDED.structure,
                imports = EXCLUDED.imports,
                exports = EXCLUDED.exports,
                diagnostics = EXCLUDED.diagnostics,
                metadata = EXCLUDED.metadata,
                indexed_at = now()
            RETURNING document_id
            "#,
            &[
                &inserted_document_id,
                &document.project_id,
                &document.namespace_id,
                &document.repo_root,
                &document.absolute_path,
                &document.relative_path,
                &document.language,
                &document.source_kind,
                &document.git_commit_sha,
                &document.file_sha256,
                &document.line_count,
                &document.byte_count,
                &document.content,
                &document.metrics,
                &document.structure,
                &document.imports,
                &document.exports,
                &document.diagnostics,
                &document.metadata,
            ],
        )
        .await?;
    let document_id: Uuid = document_row.get(0);
    transaction
        .execute(
            "DELETE FROM ami.code_symbols WHERE document_id = $1",
            &[&document_id],
        )
        .await?;
    transaction
        .execute(
            "DELETE FROM ami.code_chunks WHERE document_id = $1",
            &[&document_id],
        )
        .await?;

    for symbol in symbols {
        transaction
            .execute(
                r#"
                INSERT INTO ami.code_symbols(
                    document_id, project_id, namespace_id, name, kind,
                    start_line, end_line, start_byte, end_byte, metadata
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                "#,
                &[
                    &document_id,
                    &document.project_id,
                    &document.namespace_id,
                    &symbol.name,
                    &symbol.kind,
                    &symbol.start_line,
                    &symbol.end_line,
                    &symbol.start_byte,
                    &symbol.end_byte,
                    &symbol.metadata,
                ],
            )
            .await?;
    }

    for chunk in chunks {
        transaction
            .execute(
                r#"
                INSERT INTO ami.code_chunks(
                    chunk_id, document_id, project_id, namespace_id,
                    qdrant_point_id, qdrant_collection_alias,
                    chunk_index, total_chunks,
                    start_line, end_line, start_byte, end_byte,
                    content, metadata
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                "#,
                &[
                    &chunk.chunk_id,
                    &document_id,
                    &document.project_id,
                    &document.namespace_id,
                    &chunk.qdrant_point_id,
                    &chunk.qdrant_collection_alias,
                    &chunk.chunk_index,
                    &chunk.total_chunks,
                    &chunk.start_line,
                    &chunk.end_line,
                    &chunk.start_byte,
                    &chunk.end_byte,
                    &chunk.content,
                    &chunk.metadata,
                ],
            )
            .await?;
    }

    transaction.commit().await?;
    Ok(document_id)
}

pub async fn delete_namespace_documents(client: &Client, namespace_id: Uuid) -> Result<u64> {
    client
        .execute(
            "DELETE FROM ami.code_documents WHERE namespace_id = $1",
            &[&namespace_id],
        )
        .await
        .context("failed to delete namespace documents")
}

pub async fn count_documents_for_project_namespace_codes(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<u64> {
    let row = client
        .query_one(
            r#"
            SELECT COUNT(*)
            FROM ami.code_documents d
            JOIN ami.projects p ON p.project_id = d.project_id
            JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
            WHERE p.code = $1
              AND n.code = $2
            "#,
            &[&project_code, &namespace_code],
        )
        .await
        .context("failed to count code documents for project/namespace")?;
    let count: i64 = row.get(0);
    Ok(count.max(0) as u64)
}

pub async fn status_counts(client: &Client) -> Result<(i64, i64, i64)> {
    let row = client
        .query_one(
            r#"
            SELECT
                (SELECT COUNT(*) FROM ami.projects),
                (SELECT COUNT(*) FROM ami.namespaces),
                (SELECT COUNT(*) FROM ami.code_documents)
            "#,
            &[],
        )
        .await?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

pub async fn touch_project_updated_at(client: &Client, project_id: Uuid) -> Result<()> {
    client
        .execute(
            "UPDATE ami.projects SET updated_at = now() WHERE project_id = $1",
            &[&project_id],
        )
        .await
        .context("failed to touch project updated_at")?;
    Ok(())
}

pub async fn upsert_stack_meta(client: &Client, key: &str, value: &Value) -> Result<()> {
    client
        .execute(
            r#"
            INSERT INTO ami.stack_meta(meta_key, meta_value, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (meta_key) DO UPDATE SET
                meta_value = EXCLUDED.meta_value,
                updated_at = now()
            "#,
            &[&key, value],
        )
        .await
        .context("failed to upsert stack meta")?;
    Ok(())
}

pub async fn get_stack_meta(client: &Client, key: &str) -> Result<Option<Value>> {
    let row = client
        .query_opt(
            "SELECT meta_value FROM ami.stack_meta WHERE meta_key = $1",
            &[&key],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn insert_observability_snapshot(
    client: &Client,
    snapshot_kind: &str,
    payload: &Value,
) -> Result<Uuid> {
    let (stored_payload, meta) = prepare_observability_payload(snapshot_kind, payload)?;
    let row = client
        .query_opt(
            r#"
            INSERT INTO ami.observability_snapshots(
                snapshot_kind,
                payload,
                event_key,
                source_event_id,
                source_kind,
                source_class,
                scope_project_code,
                scope_namespace_code,
                captured_at_epoch_ms,
                payload_sha256,
                last_seen_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
            ON CONFLICT (snapshot_kind, event_key) DO UPDATE
            SET payload = EXCLUDED.payload,
                source_event_id = EXCLUDED.source_event_id,
                source_kind = EXCLUDED.source_kind,
                source_class = EXCLUDED.source_class,
                scope_project_code = EXCLUDED.scope_project_code,
                scope_namespace_code = EXCLUDED.scope_namespace_code,
                captured_at_epoch_ms = EXCLUDED.captured_at_epoch_ms,
                payload_sha256 = EXCLUDED.payload_sha256,
                replay_count = ami.observability_snapshots.replay_count + 1,
                last_seen_at = now()
            WHERE ami.observability_snapshots.payload_sha256 = EXCLUDED.payload_sha256
               OR (
                    ami.observability_snapshots.source_event_id IS NOT NULL
                    AND EXCLUDED.source_event_id IS NOT NULL
                    AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                    AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                    AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                    AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
               )
            RETURNING snapshot_id
            "#,
            &[
                &snapshot_kind,
                &stored_payload,
                &meta.event_key,
                &meta.source_event_id,
                &meta.source_kind,
                &meta.source_class,
                &meta.scope_project_code,
                &meta.scope_namespace_code,
                &meta.captured_at_epoch_ms,
                &meta.payload_sha256,
            ],
        )
        .await
        .context("failed to insert observability snapshot")?;
    if let Some(row) = row {
        return Ok(row.get(0));
    }

    let existing = client
        .query_opt(
            r#"
            SELECT snapshot_id, source_event_id, captured_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND event_key = $2
            "#,
            &[&snapshot_kind, &meta.event_key],
        )
        .await
        .context("failed to inspect conflicting observability snapshot")?
        .ok_or_else(|| {
            anyhow!(
                "observability snapshot conflict vanished unexpectedly for {} :: {}",
                snapshot_kind,
                meta.event_key
            )
        })?;
    let existing_snapshot_id: Uuid = existing.get(0);
    let existing_source_event_id: Option<String> = existing.get(1);
    let existing_captured_at_epoch_ms: Option<i64> = existing.get(2);

    if meta.source_event_id.is_some()
        && meta.source_event_id == existing_source_event_id
        && meta
            .captured_at_epoch_ms
            .zip(existing_captured_at_epoch_ms)
            .is_some_and(|(incoming, existing)| incoming > existing)
    {
        return Err(observability_conflict_error(
            snapshot_kind,
            &meta,
            existing_snapshot_id,
            existing_source_event_id.as_deref(),
            existing_captured_at_epoch_ms,
        ));
    }

    Err(observability_conflict_error(
        snapshot_kind,
        &meta,
        existing_snapshot_id,
        existing_source_event_id.as_deref(),
        existing_captured_at_epoch_ms,
    ))
}

pub async fn insert_execctl_task_ledger_entry(
    client: &Client,
    entry: &ExecCtlTaskLedgerEntryInsert<'_>,
) -> Result<Uuid> {
    if entry.source_event_id.trim().is_empty() {
        return Err(anyhow!("execctl task ledger source_event_id must not be empty"));
    }
    if entry.agent_scope.trim().is_empty() {
        return Err(anyhow!("execctl task ledger agent_scope must not be empty"));
    }
    let inserted = client
        .query_opt(
            r#"
            INSERT INTO ami.execctl_task_ledger_entries(
                project_id,
                namespace_id,
                agent_scope,
                session_id,
                thread_id,
                source_snapshot_id,
                source_event_id,
                event_kind,
                source_kind,
                headline,
                next_step,
                summary,
                active_files,
                open_questions,
                materialized_notes,
                pending_return_queue,
                local_path,
                recorded_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT (source_event_id) DO NOTHING
            RETURNING ledger_entry_id
            "#,
            &[
                &entry.project_id,
                &entry.namespace_id,
                &entry.agent_scope,
                &entry.session_id,
                &entry.thread_id,
                &entry.source_snapshot_id,
                &entry.source_event_id,
                &entry.event_kind,
                &entry.source_kind,
                &entry.headline,
                &entry.next_step,
                &entry.summary,
                entry.active_files,
                entry.open_questions,
                entry.materialized_notes,
                entry.pending_return_queue,
                &entry.local_path,
                &entry.recorded_at_epoch_ms,
            ],
        )
        .await
        .context("failed to insert execctl task ledger entry")?;
    if let Some(row) = inserted {
        return Ok(row.get(0));
    }

    let existing = client
        .query_one(
            r#"
            SELECT ledger_entry_id
            FROM ami.execctl_task_ledger_entries
            WHERE source_event_id = $1
            "#,
            &[&entry.source_event_id],
        )
        .await
        .context("failed to resolve existing execctl task ledger entry")?;
    Ok(existing.get(0))
}

pub async fn list_execctl_task_ledger_entries(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    agent_scope: &str,
    limit: Option<i64>,
) -> Result<Vec<ExecCtlTaskLedgerEntryRecord>> {
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                ledger_entry_id,
                source_snapshot_id,
                source_event_id,
                event_kind,
                source_kind,
                agent_scope,
                session_id,
                thread_id,
                headline,
                next_step,
                summary,
                active_files,
                open_questions,
                materialized_notes,
                pending_return_queue,
                local_path,
                recorded_at_epoch_ms,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.execctl_task_ledger_entries
            WHERE project_id = $1
              AND namespace_id = $2
              AND agent_scope = $3
            ORDER BY recorded_at_epoch_ms DESC, created_at DESC
            LIMIT $4
            "#,
            &[&project_id, &namespace_id, &agent_scope, &limit],
        )
        .await
        .context("failed to list execctl task ledger entries")?;
    Ok(rows
        .into_iter()
        .map(|row| ExecCtlTaskLedgerEntryRecord {
            ledger_entry_id: row.get(0),
            source_snapshot_id: row.get(1),
            source_event_id: row.get(2),
            event_kind: row.get(3),
            source_kind: row.get(4),
            agent_scope: row.get(5),
            session_id: row.get(6),
            thread_id: row.get(7),
            headline: row.get(8),
            next_step: row.get(9),
            summary: row.get(10),
            active_files: row.get(11),
            open_questions: row.get(12),
            materialized_notes: row.get(13),
            pending_return_queue: row.get(14),
            local_path: row.get(15),
            recorded_at_epoch_ms: row.get(16),
            created_at_epoch_ms: row.get(17),
        })
        .collect())
}

pub async fn delete_observability_snapshots_by_scope(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    project_code: &str,
    namespace_code: &str,
) -> Result<u64> {
    let sql = format!(
        r#"
        DELETE FROM ami.observability_snapshots
        WHERE snapshot_kind = $1
          AND payload->'{payload_root}'->'project'->>'code' = $2
          AND payload->'{payload_root}'->'namespace'->>'code' = $3
        "#
    );
    client
        .execute(&sql, &[&snapshot_kind, &project_code, &namespace_code])
        .await
        .context("failed to delete scoped observability snapshots")
}

pub async fn latest_observability_snapshot(
    client: &Client,
    snapshot_kind: &str,
) -> Result<Option<Value>> {
    let row = client
        .query_opt(
            r#"
            SELECT payload
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            &[&snapshot_kind],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn latest_observability_snapshot_for_project(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    project_code: &str,
) -> Result<Option<Value>> {
    let row = client
        .query_opt(
            &format!(
                r#"
                SELECT payload
                FROM ami.observability_snapshots
                WHERE snapshot_kind = $1
                  AND (
                        scope_project_code = $2
                        OR payload->'{payload_root}'->'project'->>'code' = $2
                  )
                ORDER BY created_at DESC
                LIMIT 1
                "#
            ),
            &[&snapshot_kind, &project_code],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn list_observability_snapshots_by_kinds(
    client: &Client,
    kinds: &[&str],
    limit: Option<i64>,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = ANY($1::text[])
            ORDER BY created_at DESC
            LIMIT $2
            "#,
            &[&kinds, &limit],
        )
        .await
        .context("failed to list observability snapshots by kinds")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn list_observability_snapshots_older_than(
    client: &Client,
    cutoff_epoch_ms: i64,
    limit: Option<i64>,
) -> Result<Vec<ObservabilityRetentionCandidate>> {
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                COALESCE(source_kind, '') AS source_kind,
                COALESCE(source_class, '') AS source_class,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms,
                captured_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE COALESCE(
                    captured_at_epoch_ms,
                    (EXTRACT(EPOCH FROM created_at) * 1000)::bigint
                  ) <= $1
            ORDER BY COALESCE(
                    captured_at_epoch_ms,
                    (EXTRACT(EPOCH FROM created_at) * 1000)::bigint
                  ) ASC
            LIMIT $2
            "#,
            &[&cutoff_epoch_ms, &limit],
        )
        .await
        .context("failed to list aged observability snapshots")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilityRetentionCandidate {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            source_kind: row.get(3),
            source_class: row.get(4),
            created_at_epoch_ms: row.get(5),
            captured_at_epoch_ms: row.get(6),
        })
        .collect())
}

pub async fn delete_observability_snapshots_by_ids(
    client: &Client,
    snapshot_ids: &[Uuid],
) -> Result<u64> {
    if snapshot_ids.is_empty() {
        return Ok(0);
    }
    client
        .execute(
            r#"
            DELETE FROM ami.observability_snapshots
            WHERE snapshot_id = ANY($1::uuid[])
            "#,
            &[&snapshot_ids],
        )
        .await
        .context("failed to delete observability snapshots by ids")
}

pub async fn update_observability_snapshot_payload(
    client: &Client,
    snapshot_id: &Uuid,
    payload: &Value,
) -> Result<()> {
    let row = client
        .query_opt(
            r#"
            SELECT snapshot_kind, payload
            FROM ami.observability_snapshots
            WHERE snapshot_id = $1
            "#,
            &[snapshot_id],
        )
        .await
        .context("failed to load observability snapshot metadata before update")?
        .ok_or_else(|| anyhow!("observability snapshot not found: {snapshot_id}"))?;
    let snapshot_kind: String = row.get(0);
    let existing_payload: Value = row.get(1);
    let (stored_payload, meta) = prepare_observability_payload(&snapshot_kind, payload)?;
    match validate_observability_update(
        &snapshot_kind,
        snapshot_id,
        &existing_payload,
        &stored_payload,
    )? {
        false => return Ok(()),
        true => {}
    }
    client
        .execute(
            r#"
            UPDATE ami.observability_snapshots
            SET payload = $2,
                event_key = $3,
                source_event_id = $4,
                source_kind = $5,
                source_class = $6,
                scope_project_code = $7,
                scope_namespace_code = $8,
                captured_at_epoch_ms = $9,
                payload_sha256 = $10,
                last_seen_at = now()
            WHERE snapshot_id = $1
            "#,
            &[
                snapshot_id,
                &stored_payload,
                &meta.event_key,
                &meta.source_event_id,
                &meta.source_kind,
                &meta.source_class,
                &meta.scope_project_code,
                &meta.scope_namespace_code,
                &meta.captured_at_epoch_ms,
                &meta.payload_sha256,
            ],
        )
        .await
        .context("failed to update observability snapshot payload")?;
    Ok(())
}

fn observability_conflict_error(
    snapshot_kind: &str,
    meta: &ObservabilityInsertMeta,
    existing_snapshot_id: Uuid,
    existing_source_event_id: Option<&str>,
    existing_captured_at_epoch_ms: Option<i64>,
) -> anyhow::Error {
    if meta.source_event_id.is_some()
        && meta.source_event_id.as_deref() == existing_source_event_id
        && meta
            .captured_at_epoch_ms
            .zip(existing_captured_at_epoch_ms)
            .is_some_and(|(incoming, existing)| incoming > existing)
    {
        anyhow!(
            "observability anti-replay blocked newer divergent payload for immutable event {} :: {}",
            snapshot_kind,
            meta.event_key
        )
    } else {
        anyhow!(
            "observability idempotency blocked divergent payload for {} :: {} (existing snapshot {})",
            snapshot_kind,
            meta.event_key,
            existing_snapshot_id
        )
    }
}

fn validate_observability_update(
    snapshot_kind: &str,
    snapshot_id: &Uuid,
    existing_payload: &Value,
    stored_payload: &Value,
) -> Result<bool> {
    if existing_payload == stored_payload {
        return Ok(false);
    }
    if existing_payload["_observability"]["immutable_snapshot"].as_bool() == Some(true) {
        return Err(anyhow!(
            "observability snapshot is immutable and cannot be updated: {snapshot_kind} / {snapshot_id}"
        ));
    }
    Ok(true)
}

fn prepare_observability_payload(
    snapshot_kind: &str,
    payload: &Value,
) -> Result<(Value, ObservabilityInsertMeta)> {
    let payload_sha256 = hex_sha256(
        &serde_json::to_vec(payload).context("failed to serialize observability payload")?,
    );
    let source_event_id = extract_first_string(
        payload,
        &[
            &["_observability", "source_event_id"],
            &["token_budget_event", "event_id"],
            &["working_state_event", "event_id"],
            &["working_state_event", "context_pack_id"],
            &["context_pack_id"],
        ],
    );
    let source_kind = extract_first_string(
        payload,
        &[
            &["_observability", "source_kind"],
            &["token_budget_event", "source_kind"],
            &["working_state_event", "source_kind"],
            &["continuity_handoff", "source_kind"],
        ],
    )
    .unwrap_or_else(|| snapshot_kind.to_string());
    let source_class = observability_source_class(snapshot_kind, payload).to_string();
    let scope_project_code = extract_first_string(
        payload,
        &[
            &["_observability", "scope_project_code"],
            &["project", "code"],
            &["working_state_restore", "project", "code"],
            &["working_state_event", "project", "code"],
            &["continuity_import", "project", "code"],
            &["continuity_handoff", "project", "code"],
            &["token_budget_event", "project_code"],
            &["token_budget_event", "project"],
            &["benchmark", "project"],
            &["accuracy_verification", "project"],
            &["load_verification", "project"],
            &["cold_benchmark", "project"],
        ],
    );
    let scope_namespace_code = extract_first_string(
        payload,
        &[
            &["_observability", "scope_namespace_code"],
            &["namespace", "code"],
            &["working_state_restore", "namespace", "code"],
            &["working_state_event", "namespace", "code"],
            &["continuity_import", "namespace", "code"],
            &["continuity_handoff", "namespace", "code"],
            &["token_budget_event", "namespace_code"],
            &["token_budget_event", "namespace"],
            &["benchmark", "namespace"],
            &["accuracy_verification", "namespace"],
            &["load_verification", "namespace"],
        ],
    );
    let captured_at_epoch_ms = extract_first_i64(
        payload,
        &[
            &["_observability", "captured_at_epoch_ms"],
            &["captured_at_epoch_ms"],
            &["working_state_restore", "captured_at_epoch_ms"],
            &["working_state_event", "recorded_at_epoch_ms"],
            &["token_budget_event", "created_at_epoch_ms"],
            &["continuity_import", "imported_at_epoch_ms"],
            &["continuity_thread_index", "captured_at_epoch_ms"],
            &["continuity_handoff", "captured_at_epoch_ms"],
            &["benchmark", "captured_at_epoch_ms"],
            &["accuracy_verification", "captured_at_epoch_ms"],
            &["load_verification", "captured_at_epoch_ms"],
            &["cold_benchmark", "captured_at_epoch_ms"],
        ],
    );
    let event_key = source_event_id
        .clone()
        .unwrap_or_else(|| format!("sha256:{payload_sha256}"));
    let policy_meta =
        observability_policy::policy_metadata(snapshot_kind, payload, &source_kind, &source_class)?;
    let meta = ObservabilityInsertMeta {
        event_key: event_key.clone(),
        source_event_id,
        source_kind: source_kind.clone(),
        source_class: source_class.clone(),
        scope_project_code: scope_project_code.clone(),
        scope_namespace_code: scope_namespace_code.clone(),
        captured_at_epoch_ms,
        payload_sha256: payload_sha256.clone(),
    };

    let mut stored_payload = payload.clone();
    if let Some(object) = stored_payload.as_object_mut() {
        let mut observability_meta = object
            .get("_observability")
            .cloned()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| json!({}));
        let observability_meta_object = observability_meta
            .as_object_mut()
            .expect("observability meta initialized as object");
        for (key, value) in json!({
            "snapshot_kind": snapshot_kind,
            "event_key": event_key,
            "source_event_id": meta.source_event_id,
            "source_kind": source_kind,
            "source_class": source_class,
            "scope_project_code": scope_project_code,
            "scope_namespace_code": scope_namespace_code,
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "payload_sha256": payload_sha256,
            "replay_protected": meta.source_event_id.is_some(),
        })
        .as_object()
        .expect("observability base metadata object")
        {
            observability_meta_object.insert(key.clone(), value.clone());
        }
        for (key, value) in policy_meta
            .as_object()
            .expect("observability policy metadata object")
        {
            observability_meta_object.insert(key.clone(), value.clone());
        }
        object.insert("_observability".to_string(), observability_meta);
    }

    Ok((stored_payload, meta))
}

fn observability_source_class(snapshot_kind: &str, payload: &Value) -> &'static str {
    if payload["load_verification"]["record_live_context"].as_bool() == Some(true)
        || payload["load_verification"]["publish_benchmark_snapshot"].as_bool() == Some(false)
    {
        return "live_context";
    }
    match snapshot_kind {
        "retrieval_benchmark_hot"
        | "retrieval_benchmark_cold"
        | "retrieval_load_hot"
        | "retrieval_load_cold"
        | "retrieval_accuracy"
        | "continuity_verification"
        | "cold_path_benchmark"
        | "token_benchmark"
        | "token_benchmark_suite"
        | "text_compare"
        | "mcp_task_matrix"
        | "memory_task_matrix" => "benchmark",
        "system_snapshot" => "live_system",
        _ => "operational",
    }
}

fn extract_first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        value_at_path(value, path).and_then(|node| node.as_str().map(ToOwned::to_owned))
    })
}

fn extract_first_i64(value: &Value, paths: &[&[&str]]) -> Option<i64> {
    paths.iter().find_map(|path| {
        value_at_path(value, path).and_then(|node| {
            node.as_i64()
                .or_else(|| node.as_u64().and_then(|number| i64::try_from(number).ok()))
        })
    })
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn sql_ident(input: &str) -> Result<String> {
    if input.is_empty()
        || !input
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(anyhow!("unsafe SQL identifier: {input}"));
    }
    Ok(input.to_string())
}

fn sql_literal(input: &str) -> String {
    format!("'{}'", input.replace('\'', "''"))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{
        ObservabilityInsertMeta, canonical_repo_root_string, exact_match_basename,
        exact_match_basename_stem, observability_conflict_error, observability_source_class,
        prepare_observability_payload, safe_postgres_descriptor, validate_observability_update,
    };
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn observability_payload_prefers_source_event_id_for_event_key() {
        let payload = json!({
            "token_budget_event": {
                "event_id": "event-123",
                "source_kind": "live_context_pack",
                "project_code": "amai",
                "namespace_code": "continuity",
                "created_at_epoch_ms": 1234
            }
        });
        let (stored, meta) =
            prepare_observability_payload("token_budget_event", &payload).expect("payload");
        assert_eq!(meta.event_key, "event-123");
        assert_eq!(meta.source_event_id.as_deref(), Some("event-123"));
        assert_eq!(meta.scope_project_code.as_deref(), Some("amai"));
        assert_eq!(meta.scope_namespace_code.as_deref(), Some("continuity"));
        assert_eq!(meta.captured_at_epoch_ms, Some(1234));
        assert_eq!(
            stored["_observability"]["replay_protected"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn observability_payload_prefers_working_state_event_id_over_context_pack_id() {
        let payload = json!({
            "working_state_event": {
                "event_id": "working-state-event-1",
                "context_pack_id": "ctx-pack-1",
                "source_kind": "context_pack",
                "project": {
                    "code": "amai"
                },
                "namespace": {
                    "code": "default"
                },
                "recorded_at_epoch_ms": 555
            }
        });
        let (_stored, meta) =
            prepare_observability_payload("working_state_event", &payload).expect("payload");
        assert_eq!(meta.event_key, "working-state-event-1");
        assert_eq!(
            meta.source_event_id.as_deref(),
            Some("working-state-event-1")
        );
    }

    #[test]
    fn observability_payload_prefers_explicit_observability_event_over_context_pack_id() {
        let payload = json!({
            "_observability": {
                "source_event_id": "benchmark-run-1",
                "source_kind": "benchmark_run",
                "captured_at_epoch_ms": 777
            },
            "benchmark": {
                "project": "project_alpha",
                "namespace": "default",
                "captured_at_epoch_ms": 777
            },
            "context_pack_id": "ctx-pack-1"
        });
        let (_stored, meta) =
            prepare_observability_payload("retrieval_benchmark_hot", &payload).expect("payload");
        assert_eq!(meta.event_key, "benchmark-run-1");
        assert_eq!(meta.source_event_id.as_deref(), Some("benchmark-run-1"));
        assert_eq!(meta.scope_project_code.as_deref(), Some("project_alpha"));
        assert_eq!(meta.scope_namespace_code.as_deref(), Some("default"));
        assert_eq!(meta.captured_at_epoch_ms, Some(777));
    }

    #[test]
    fn observability_payload_extracts_scope_from_working_state_restore_root() {
        let payload = json!({
            "working_state_restore": {
                "project": {
                    "code": "amai"
                },
                "namespace": {
                    "code": "default"
                },
                "captured_at_epoch_ms": 999
            }
        });
        let (_stored, meta) =
            prepare_observability_payload("working_state_restore", &payload).expect("payload");
        assert_eq!(meta.scope_project_code.as_deref(), Some("amai"));
        assert_eq!(meta.scope_namespace_code.as_deref(), Some("default"));
        assert_eq!(meta.captured_at_epoch_ms, Some(999));
    }

    #[test]
    fn observability_payload_marks_live_context_benchmark_as_contaminated() {
        let payload = json!({
            "load_verification": {
                "project": "amai",
                "namespace": "default",
                "captured_at_epoch_ms": 99,
                "record_live_context": true,
                "publish_benchmark_snapshot": false
            }
        });
        let (stored, meta) =
            prepare_observability_payload("retrieval_load_hot", &payload).expect("payload");
        assert_eq!(meta.source_class, "live_context");
        assert_eq!(
            stored["_observability"]["source_class"].as_str(),
            Some("live_context")
        );
    }

    #[test]
    fn observability_source_class_defaults_to_benchmark_for_clean_load_snapshot() {
        let payload = json!({
            "load_verification": {
                "captured_at_epoch_ms": 77,
                "record_live_context": false,
                "publish_benchmark_snapshot": true
            }
        });
        assert_eq!(
            observability_source_class("retrieval_load_hot", &payload),
            "benchmark"
        );
        assert_eq!(
            observability_source_class("continuity_verification", &json!({})),
            "benchmark"
        );
    }

    #[test]
    fn observability_payload_preserves_custom_meta_and_stamps_policy_versions() {
        let payload = json!({
            "_observability": {
                "benchmark_run_id": "bench-42"
            },
            "benchmark": {
                "project": "project_alpha",
                "namespace": "default",
                "captured_at_epoch_ms": 777
            }
        });
        let (stored, _meta) =
            prepare_observability_payload("retrieval_benchmark_hot", &payload).expect("payload");
        assert_eq!(
            stored["_observability"]["benchmark_run_id"].as_str(),
            Some("bench-42")
        );
        assert!(
            stored["_observability"]["schema_version"]
                .as_u64()
                .is_some()
        );
        assert_eq!(
            stored["_observability"]["classification_rules_version"].as_str(),
            Some("observability-source-class-v2")
        );
        assert_eq!(
            stored["_observability"]["immutable_snapshot"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn observability_conflict_error_marks_newer_same_event_as_anti_replay() {
        let meta = ObservabilityInsertMeta {
            event_key: "event-1".to_string(),
            source_event_id: Some("event-1".to_string()),
            source_kind: "benchmark_run".to_string(),
            source_class: "benchmark".to_string(),
            scope_project_code: Some("project_alpha".to_string()),
            scope_namespace_code: Some("default".to_string()),
            captured_at_epoch_ms: Some(200),
            payload_sha256: "abc".to_string(),
        };
        let error = observability_conflict_error(
            "retrieval_benchmark_hot",
            &meta,
            Uuid::nil(),
            Some("event-1"),
            Some(100),
        );
        assert!(
            error
                .to_string()
                .contains("observability anti-replay blocked newer divergent payload")
        );
    }

    #[test]
    fn observability_conflict_error_marks_divergent_payload_as_idempotency_failure() {
        let meta = ObservabilityInsertMeta {
            event_key: "event-2".to_string(),
            source_event_id: Some("event-2".to_string()),
            source_kind: "benchmark_run".to_string(),
            source_class: "benchmark".to_string(),
            scope_project_code: Some("project_alpha".to_string()),
            scope_namespace_code: Some("default".to_string()),
            captured_at_epoch_ms: Some(100),
            payload_sha256: "abc".to_string(),
        };
        let error = observability_conflict_error(
            "retrieval_benchmark_hot",
            &meta,
            Uuid::parse_str("00000000-0000-0000-0000-000000000123").expect("uuid"),
            Some("event-2"),
            Some(100),
        );
        assert!(
            error
                .to_string()
                .contains("observability idempotency blocked divergent payload")
        );
    }

    #[test]
    fn immutable_observability_update_is_rejected_before_sql_write() {
        let snapshot_id = Uuid::parse_str("00000000-0000-0000-0000-000000000321").expect("uuid");
        let existing = json!({
            "_observability": {
                "immutable_snapshot": true
            },
            "benchmark": {
                "p95_ms": 1.0
            }
        });
        let incoming = json!({
            "_observability": {
                "immutable_snapshot": true
            },
            "benchmark": {
                "p95_ms": 2.0
            }
        });
        let error = validate_observability_update(
            "retrieval_benchmark_hot",
            &snapshot_id,
            &existing,
            &incoming,
        )
        .expect_err("immutable update must fail");
        assert!(
            error
                .to_string()
                .contains("observability snapshot is immutable and cannot be updated")
        );
    }

    #[test]
    fn canonical_repo_root_string_resolves_relative_segments() {
        let temp_root =
            std::env::temp_dir().join(format!("amai-postgres-canonical-{}", Uuid::new_v4()));
        let nested = temp_root.join("nested");
        std::fs::create_dir_all(&nested).expect("temp dir");
        let raw = nested.join("..").join("nested").join(".");
        let canonical = canonical_repo_root_string(&raw.display().to_string()).expect("canonical");
        assert_eq!(canonical, nested.display().to_string());
        std::fs::remove_dir_all(&temp_root).expect("cleanup");
    }

    #[test]
    fn canonical_repo_root_string_rejects_missing_paths() {
        let missing =
            std::env::temp_dir().join(format!("amai-postgres-missing-{}", Uuid::new_v4()));
        let error = canonical_repo_root_string(&missing.display().to_string())
            .expect_err("missing path must fail");
        assert!(error.to_string().contains("failed to resolve repo_root"));
    }

    #[test]
    fn exact_match_basename_strips_parent_segments() {
        assert_eq!(
            exact_match_basename("docs/source/checklists/CHECKLIST_00_MASTER_ART_REGART"),
            "CHECKLIST_00_MASTER_ART_REGART"
        );
        assert_eq!(
            exact_match_basename("scripts/tools/amai_art_continuity_startup.sh"),
            "amai_art_continuity_startup.sh"
        );
    }

    #[test]
    fn exact_match_basename_stem_strips_single_extension() {
        assert_eq!(
            exact_match_basename_stem("amai_art_continuity_startup.sh"),
            "amai_art_continuity_startup"
        );
        assert_eq!(
            exact_match_basename_stem("CHECKLIST_00_MASTER_ART_REGART"),
            "CHECKLIST_00_MASTER_ART_REGART"
        );
    }

    #[test]
    fn safe_postgres_descriptor_masks_password_for_uri_dsn() {
        let masked = safe_postgres_descriptor(
            "postgres://art_user:super-secret@example.com:5544/amai?sslmode=require",
        );
        assert_eq!(
            masked,
            "postgres://art_user:***@example.com:5544/amai?sslmode=require"
        );
        assert!(!masked.contains("super-secret"));
    }

    #[test]
    fn safe_postgres_descriptor_masks_password_for_keyword_dsn() {
        let masked = safe_postgres_descriptor(
            "host=pg.internal port=5433 user=app dbname=amai password=very-secret sslmode=prefer",
        );
        assert_eq!(
            masked,
            "postgres://app:***@pg.internal:5433/amai?sslmode=prefer"
        );
        assert!(!masked.contains("very-secret"));
    }
}
