use crate::config::AppConfig;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use tokio_postgres::{Client, NoTls};
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
pub struct ObservabilitySnapshotRecord {
    pub snapshot_kind: String,
    pub payload: Value,
    pub created_at_epoch_ms: i64,
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

pub async fn connect_admin(cfg: &AppConfig) -> Result<Client> {
    connect(&cfg.postgres_dsn).await
}

pub async fn connect_app(cfg: &AppConfig) -> Result<Client> {
    connect(&cfg.app_postgres_dsn).await
}

async fn connect(dsn: &str) -> Result<Client> {
    let (client, connection) = tokio_postgres::connect(dsn, NoTls)
        .await
        .with_context(|| format!("failed to connect to postgres via {dsn}"))?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(?error, "postgres connection task ended with error");
        }
    });
    Ok(client)
}

pub async fn bootstrap_schema(client: &Client, cfg: &AppConfig) -> Result<()> {
    client
        .batch_execute(include_str!("../sql/000_bootstrap.sql"))
        .await
        .context("failed to apply postgres schema")?;
    ensure_app_role(client, cfg).await?;
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
            &[&code, &display_name, &repo_root, &default_branch],
        )
        .await
        .context("failed to upsert project")?;

    let project = ProjectRecord {
        project_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        repo_root: row.get(3),
        updated_at: row.get(4),
    };

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
        .map(|row| ProjectRecord {
            project_id: row.get(0),
            code: row.get(1),
            display_name: row.get(2),
            repo_root: row.get(3),
            updated_at: row.get(4),
        })
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
    Ok(ProjectRecord {
        project_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        repo_root: row.get(3),
        updated_at: row.get(4),
    })
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
    let document_id = Uuid::new_v4();
    let transaction = client.transaction().await?;
    transaction
        .execute(
            "DELETE FROM ami.code_documents WHERE namespace_id = $1 AND relative_path = $2",
            &[&document.namespace_id, &document.relative_path],
        )
        .await?;
    transaction
        .execute(
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
            "#,
            &[
                &document_id,
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
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.observability_snapshots(snapshot_kind, payload)
            VALUES ($1, $2)
            RETURNING snapshot_id
            "#,
            &[&snapshot_kind, payload],
        )
        .await
        .context("failed to insert observability snapshot")?;
    Ok(row.get(0))
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
            snapshot_kind: row.get(0),
            payload: row.get(1),
            created_at_epoch_ms: row.get(2),
        })
        .collect())
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
