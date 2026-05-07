use super::*;
use anyhow::Error;
use tokio_postgres::{GenericClient, error::SqlState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplaceDocumentIndexErrorPhase {
    BeforeCommit,
    CommitRolledBackAtCommit,
    CommitOutcomeUnknown,
}

impl ReplaceDocumentIndexErrorPhase {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::BeforeCommit => "before_commit",
            Self::CommitRolledBackAtCommit => "commit_rolled_back_at_commit",
            Self::CommitOutcomeUnknown => "commit_outcome_unknown",
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReplaceDocumentIndexError {
    pub(crate) phase: ReplaceDocumentIndexErrorPhase,
    pub(crate) sqlstate_code: Option<String>,
    pub(crate) error: Error,
}

impl ReplaceDocumentIndexError {
    fn before_commit(error: Error) -> Self {
        Self {
            phase: ReplaceDocumentIndexErrorPhase::BeforeCommit,
            sqlstate_code: extract_sqlstate_code(&error),
            error,
        }
    }

    fn commit_rolled_back_at_commit(sqlstate_code: Option<String>, error: Error) -> Self {
        Self {
            phase: ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit,
            sqlstate_code,
            error,
        }
    }

    fn commit_outcome_unknown(sqlstate_code: Option<String>, error: Error) -> Self {
        Self {
            phase: ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
            sqlstate_code,
            error,
        }
    }
}

fn classify_commit_error(error: tokio_postgres::Error) -> ReplaceDocumentIndexError {
    if let Some(db_error) = error.as_db_error() {
        let sqlstate_code = Some(db_error.code().code().to_string());
        if commit_sqlstate_is_definite_rollback(db_error.code()) {
            return ReplaceDocumentIndexError::commit_rolled_back_at_commit(
                sqlstate_code,
                error.into(),
            );
        }
        return ReplaceDocumentIndexError::commit_outcome_unknown(sqlstate_code, error.into());
    }
    ReplaceDocumentIndexError::commit_outcome_unknown(None, error.into())
}

fn extract_sqlstate_code(error: &Error) -> Option<String> {
    if let Some(pg_error) = error.downcast_ref::<tokio_postgres::Error>() {
        return pg_error
            .as_db_error()
            .map(|db_error| db_error.code().code().to_string());
    }
    for cause in error.chain() {
        if let Some(pg_error) = cause.downcast_ref::<tokio_postgres::Error>() {
            return pg_error
                .as_db_error()
                .map(|db_error| db_error.code().code().to_string());
        }
    }
    None
}

fn commit_sqlstate_is_definite_rollback(code: &SqlState) -> bool {
    let code = code.code();
    if code == SqlState::T_R_STATEMENT_COMPLETION_UNKNOWN.code() {
        return false;
    }
    code.starts_with("40")
        || code.starts_with("23")
        || matches!(
            code,
            "25P01" // no_active_sql_transaction
                | "25P02" // in_failed_sql_transaction
                | "25P03" // idle_in_transaction_session_timeout
                | "25P04" // transaction_timeout
        )
}

pub async fn get_document_id_for_namespace_relative_path<C>(
    client: &C,
    namespace_id: Uuid,
    relative_path: &str,
) -> Result<Option<Uuid>>
where
    C: GenericClient + Sync,
{
    let row = client
        .query_opt(
            r#"
            SELECT document_id
            FROM ami.code_documents
            WHERE namespace_id = $1
              AND relative_path = $2
            "#,
            &[&namespace_id, &relative_path],
        )
        .await
        .context("failed to load code document id for namespace/relative_path")?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn replace_document_index(
    client: &mut Client,
    document: &DocumentRecord,
    symbols: &[SymbolRecord],
    chunks: &[ChunkRecord],
) -> Result<Uuid> {
    replace_document_index_with_document_id(client, document, symbols, chunks, Uuid::new_v4()).await
}

pub async fn replace_document_index_with_document_id(
    client: &mut Client,
    document: &DocumentRecord,
    symbols: &[SymbolRecord],
    chunks: &[ChunkRecord],
    inserted_document_id: Uuid,
) -> Result<Uuid> {
    replace_document_index_with_document_id_detailed(
        client,
        document,
        symbols,
        chunks,
        inserted_document_id,
    )
    .await
    .map_err(|error| error.error)
}

pub(crate) async fn replace_document_index_with_document_id_detailed(
    client: &mut Client,
    document: &DocumentRecord,
    symbols: &[SymbolRecord],
    chunks: &[ChunkRecord],
    inserted_document_id: Uuid,
) -> std::result::Result<Uuid, ReplaceDocumentIndexError> {
    let transaction_started = Instant::now();
    let transaction = client
        .transaction()
        .await
        .map_err(|error| ReplaceDocumentIndexError::before_commit(error.into()))?;
    super::observability_profile_log(
        "replace_document_index.transaction",
        transaction_started.elapsed().as_millis(),
        &format!("relative_path={}", document.relative_path),
    );
    let document_id = replace_document_index_with_document_id_in_client(
        &transaction,
        document,
        symbols,
        chunks,
        inserted_document_id,
    )
    .await
    .map_err(ReplaceDocumentIndexError::before_commit)?;

    #[cfg(test)]
    if let Some(forced_error) = forced_replace_document_index_error_for_tests(document) {
        return Err(forced_error);
    }

    let commit_started = Instant::now();
    transaction.commit().await.map_err(classify_commit_error)?;
    super::observability_profile_log(
        "replace_document_index.commit",
        commit_started.elapsed().as_millis(),
        &format!("relative_path={}", document.relative_path),
    );
    Ok(document_id)
}

#[cfg(test)]
fn forced_replace_document_index_error_for_tests(
    document: &DocumentRecord,
) -> Option<ReplaceDocumentIndexError> {
    let raw = std::env::var("AMAI_TEST_FORCE_REPLACE_DOCUMENT_INDEX_FAILURE").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (phase_raw, sqlstate_raw) = trimmed.split_once(':').unwrap_or((trimmed, "XX000"));
    let phase = match phase_raw.trim() {
        "before_commit" => ReplaceDocumentIndexErrorPhase::BeforeCommit,
        "commit_rolled_back_at_commit" => ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit,
        "commit_outcome_unknown" => ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
        _ => return None,
    };
    let sqlstate_code = sqlstate_raw.trim();
    Some(ReplaceDocumentIndexError {
        phase,
        sqlstate_code: Some(sqlstate_code.to_string()),
        error: anyhow!(
            "forced replace_document_index failure for tests relative_path={} phase={} sqlstate={}",
            document.relative_path,
            phase.as_str(),
            sqlstate_code
        ),
    })
}

#[cfg(test)]
mod replace_document_index_error_phase_tests {
    use super::*;

    #[test]
    fn commit_sqlstate_statement_completion_unknown_is_not_definite_rollback() {
        assert!(!commit_sqlstate_is_definite_rollback(
            &SqlState::T_R_STATEMENT_COMPLETION_UNKNOWN
        ));
    }

    #[test]
    fn commit_sqlstate_transaction_rollback_family_is_definite_rollback() {
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::T_R_SERIALIZATION_FAILURE
        ));
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::T_R_DEADLOCK_DETECTED
        ));
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::T_R_INTEGRITY_CONSTRAINT_VIOLATION
        ));
    }

    #[test]
    fn commit_sqlstate_integrity_constraint_family_is_definite_rollback() {
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::INTEGRITY_CONSTRAINT_VIOLATION
        ));
        assert!(commit_sqlstate_is_definite_rollback(&SqlState::from_code(
            "23514"
        )));
    }

    #[test]
    fn commit_sqlstate_non_rollback_family_stays_outcome_unknown() {
        assert!(!commit_sqlstate_is_definite_rollback(&SqlState::from_code(
            "08006"
        )));
        assert!(!commit_sqlstate_is_definite_rollback(&SqlState::from_code(
            "57P01"
        )));
    }

    #[test]
    fn commit_sqlstate_transaction_state_timeouts_and_failed_transaction_are_definite_rollback() {
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::NO_ACTIVE_SQL_TRANSACTION
        ));
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::IN_FAILED_SQL_TRANSACTION
        ));
        assert!(commit_sqlstate_is_definite_rollback(
            &SqlState::IDLE_IN_TRANSACTION_SESSION_TIMEOUT
        ));
        assert!(commit_sqlstate_is_definite_rollback(&SqlState::from_code(
            "25P04"
        )));
    }
}

pub(crate) async fn replace_document_index_with_document_id_in_client<C>(
    client: &C,
    document: &DocumentRecord,
    symbols: &[SymbolRecord],
    chunks: &[ChunkRecord],
    inserted_document_id: Uuid,
) -> Result<Uuid>
where
    C: GenericClient + Sync,
{
    let document_upsert_started = Instant::now();
    let document_row = client
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
    super::observability_profile_log(
        "replace_document_index.upsert_document",
        document_upsert_started.elapsed().as_millis(),
        &format!(
            "relative_path={} byte_count={} line_count={}",
            document.relative_path, document.byte_count, document.line_count
        ),
    );
    let document_id: Uuid = document_row.get(0);
    let delete_symbols_started = Instant::now();
    client
        .execute(
            "DELETE FROM ami.code_symbols WHERE document_id = $1",
            &[&document_id],
        )
        .await?;
    super::observability_profile_log(
        "replace_document_index.delete_symbols",
        delete_symbols_started.elapsed().as_millis(),
        &format!("relative_path={}", document.relative_path),
    );
    let delete_chunks_started = Instant::now();
    client
        .execute(
            "DELETE FROM ami.code_chunks WHERE document_id = $1",
            &[&document_id],
        )
        .await?;
    super::observability_profile_log(
        "replace_document_index.delete_chunks",
        delete_chunks_started.elapsed().as_millis(),
        &format!("relative_path={}", document.relative_path),
    );

    let insert_symbols_started = Instant::now();
    for symbol in symbols {
        client
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
    super::observability_profile_log(
        "replace_document_index.insert_symbols",
        insert_symbols_started.elapsed().as_millis(),
        &format!(
            "relative_path={} symbol_count={}",
            document.relative_path,
            symbols.len()
        ),
    );

    let insert_chunks_started = Instant::now();
    for chunk in chunks {
        client
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
    super::observability_profile_log(
        "replace_document_index.insert_chunks",
        insert_chunks_started.elapsed().as_millis(),
        &format!(
            "relative_path={} chunk_count={}",
            document.relative_path,
            chunks.len()
        ),
    );

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

pub async fn delete_namespace_documents_except_paths(
    client: &Client,
    namespace_id: Uuid,
    keep_paths: &[String],
) -> Result<u64> {
    client
        .execute(
            r#"
            DELETE FROM ami.code_documents
            WHERE namespace_id = $1
              AND NOT (relative_path = ANY($2::text[]))
            "#,
            &[&namespace_id, &keep_paths],
        )
        .await
        .context("failed to delete namespace documents except paths")
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
