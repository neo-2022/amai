use super::*;

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
