use super::*;

pub async fn search_documents_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<DocumentHit>> {
    let expanded_query = expanded_fts_query(query);
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
                GREATEST(
                    ts_rank_cd(d.search_vector, websearch_to_tsquery('simple', $3)),
                    CASE
                        WHEN COALESCE($4, '') = '' THEN 0.0::real
                        ELSE ts_rank_cd(d.search_vector, websearch_to_tsquery('simple', $4))
                    END
                ) AS score,
                LEFT(d.content, 1600)
            FROM ami.code_documents d
            JOIN ami.projects p ON p.project_id = d.project_id
            JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
            WHERE d.project_id = $1
              AND d.namespace_id = $2
              AND (
                    d.search_vector @@ websearch_to_tsquery('simple', $3)
                 OR (
                    COALESCE($4, '') <> ''
                    AND d.search_vector @@ websearch_to_tsquery('simple', $4)
                 )
              )
            ORDER BY score DESC, d.relative_path
            LIMIT $5
            "#,
            &[&project_id, &namespace_id, &query, &expanded_query, &limit],
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

pub async fn search_documents_exact_for_scopes(
    client: &Client,
    scopes: &[(Uuid, Uuid)],
    query: &str,
    limit_per_scope: i64,
) -> Result<Vec<DocumentHit>> {
    if limit_per_scope <= 0 || scopes.is_empty() {
        return Ok(Vec::new());
    }
    let project_ids = scopes
        .iter()
        .map(|(project_id, _)| *project_id)
        .collect::<Vec<_>>();
    let namespace_ids = scopes
        .iter()
        .map(|(_, namespace_id)| *namespace_id)
        .collect::<Vec<_>>();
    let basename_query = exact_match_basename(query);
    let basename_stem_query = exact_match_basename_stem(&basename_query);
    let allow_extensionless_basename_match = basename_query == basename_stem_query;
    let total_limit = limit_per_scope.saturating_mul(scopes.len() as i64);
    let rows = client
        .query(
            r#"
            WITH requested(project_id, namespace_id) AS (
                SELECT * FROM unnest($1::uuid[], $2::uuid[])
            ),
            candidates AS (
                SELECT
                    d.project_id,
                    d.namespace_id,
                    p.code AS project_code,
                    n.code AS namespace_code,
                    d.repo_root,
                    d.relative_path,
                    d.language,
                    d.source_kind,
                    d.git_commit_sha,
                    CASE
                        WHEN d.relative_path = $3 THEN 2000.0::real
                        WHEN d.relative_basename = $4 THEN 1500.0::real
                        WHEN $5::boolean AND d.relative_basename_stem = $6 THEN 1400.0::real
                        ELSE 0.0::real
                    END AS score,
                    LEFT(d.content, 1600) AS snippet
                FROM ami.code_documents d
                JOIN requested r
                  ON r.project_id = d.project_id
                 AND r.namespace_id = d.namespace_id
                JOIN ami.projects p ON p.project_id = d.project_id
                JOIN ami.namespaces n ON n.namespace_id = d.namespace_id
                WHERE d.relative_path = $3
                   OR d.relative_basename = $4
                   OR ($5::boolean AND d.relative_basename_stem = $6)
            ),
            dedup AS (
                SELECT DISTINCT ON (project_id, namespace_id, relative_path)
                    project_code,
                    namespace_code,
                    repo_root,
                    relative_path,
                    language,
                    source_kind,
                    git_commit_sha,
                    score,
                    snippet
                FROM candidates
                ORDER BY project_id, namespace_id, relative_path, score DESC
            )
            SELECT
                project_code,
                namespace_code,
                repo_root,
                relative_path,
                language,
                source_kind,
                git_commit_sha,
                score,
                snippet
            FROM dedup
            ORDER BY score DESC, length(relative_path), relative_path
            LIMIT $7
            "#,
            &[
                &project_ids,
                &namespace_ids,
                &query,
                &basename_query,
                &allow_extensionless_basename_match,
                &basename_stem_query,
                &total_limit,
            ],
        )
        .await?;
    Ok(rows.into_iter().map(document_hit_from_row).collect())
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

pub(super) fn exact_match_basename(query: &str) -> String {
    query.rsplit('/').next().unwrap_or(query).to_string()
}

pub(super) fn exact_match_basename_stem(basename: &str) -> String {
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
    let expanded_query = expanded_fts_query(query);
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
                GREATEST(
                    ts_rank_cd(s.search_vector, websearch_to_tsquery('simple', $3)),
                    CASE
                        WHEN COALESCE($4, '') = '' THEN 0.0::real
                        ELSE ts_rank_cd(s.search_vector, websearch_to_tsquery('simple', $4))
                    END
                ) AS score,
                s.metadata
            FROM ami.code_symbols s
            JOIN ami.code_documents d ON d.document_id = s.document_id
            JOIN ami.projects p ON p.project_id = s.project_id
            JOIN ami.namespaces n ON n.namespace_id = s.namespace_id
            WHERE s.project_id = $1
              AND s.namespace_id = $2
              AND (
                    s.search_vector @@ websearch_to_tsquery('simple', $3)
                 OR (
                    COALESCE($4, '') <> ''
                    AND s.search_vector @@ websearch_to_tsquery('simple', $4)
                 )
              )
            ORDER BY score DESC, d.relative_path, s.start_line
            LIMIT $5
            "#,
            &[&project_id, &namespace_id, &query, &expanded_query, &limit],
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

pub async fn search_symbols_exact_for_scopes(
    client: &Client,
    scopes: &[(Uuid, Uuid)],
    query: &str,
    limit_per_scope: i64,
) -> Result<Vec<SymbolHit>> {
    if limit_per_scope <= 0 || scopes.is_empty() {
        return Ok(Vec::new());
    }
    let project_ids = scopes
        .iter()
        .map(|(project_id, _)| *project_id)
        .collect::<Vec<_>>();
    let namespace_ids = scopes
        .iter()
        .map(|(_, namespace_id)| *namespace_id)
        .collect::<Vec<_>>();
    let total_limit = limit_per_scope.saturating_mul(scopes.len() as i64);
    let rows = client
        .query(
            r#"
            WITH requested(project_id, namespace_id) AS (
                SELECT * FROM unnest($1::uuid[], $2::uuid[])
            )
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
            JOIN requested r
              ON r.project_id = s.project_id
             AND r.namespace_id = s.namespace_id
            JOIN ami.code_documents d ON d.document_id = s.document_id
            JOIN ami.projects p ON p.project_id = s.project_id
            JOIN ami.namespaces n ON n.namespace_id = s.namespace_id
            WHERE s.name = $3
            ORDER BY d.relative_path, s.start_line
            LIMIT $4
            "#,
            &[&project_ids, &namespace_ids, &query, &total_limit],
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
    let expanded_query = expanded_fts_query(query);
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
                GREATEST(
                    ts_rank_cd(c.search_vector, websearch_to_tsquery('simple', $3)),
                    CASE
                        WHEN COALESCE($4, '') = '' THEN 0.0::real
                        ELSE ts_rank_cd(c.search_vector, websearch_to_tsquery('simple', $4))
                    END
                ) AS score,
                LEFT(c.content, 2000),
                c.metadata
            FROM ami.code_chunks c
            JOIN ami.code_documents d ON d.document_id = c.document_id
            JOIN ami.projects p ON p.project_id = c.project_id
            JOIN ami.namespaces n ON n.namespace_id = c.namespace_id
            WHERE c.project_id = $1
              AND c.namespace_id = $2
              AND (
                    c.search_vector @@ websearch_to_tsquery('simple', $3)
                 OR (
                    COALESCE($4, '') <> ''
                    AND c.search_vector @@ websearch_to_tsquery('simple', $4)
                 )
              )
            ORDER BY score DESC, d.relative_path, c.chunk_index
            LIMIT $5
            "#,
            &[&project_id, &namespace_id, &query, &expanded_query, &limit],
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

pub async fn search_chunks_exact_for_scopes(
    client: &Client,
    scopes: &[(Uuid, Uuid)],
    query: &str,
    limit_per_scope: i64,
) -> Result<Vec<ChunkHit>> {
    if limit_per_scope <= 0 || scopes.is_empty() {
        return Ok(Vec::new());
    }
    let project_ids = scopes
        .iter()
        .map(|(project_id, _)| *project_id)
        .collect::<Vec<_>>();
    let namespace_ids = scopes
        .iter()
        .map(|(_, namespace_id)| *namespace_id)
        .collect::<Vec<_>>();
    let total_limit = limit_per_scope.saturating_mul(scopes.len() as i64);
    let rows = client
        .query(
            r#"
            WITH requested(project_id, namespace_id) AS (
                SELECT * FROM unnest($1::uuid[], $2::uuid[])
            ),
            exact_chunk_hits AS (
                SELECT
                    c.project_id,
                    c.namespace_id,
                    p.code AS project_code,
                    n.code AS namespace_code,
                    d.repo_root,
                    d.relative_path,
                    c.chunk_id,
                    c.chunk_index,
                    c.start_line,
                    c.end_line,
                    1800.0::real AS score,
                    LEFT(c.content, 2000) AS content,
                    c.metadata
                FROM ami.code_chunks c
                JOIN requested r
                  ON r.project_id = c.project_id
                 AND r.namespace_id = c.namespace_id
                JOIN ami.code_documents d ON d.document_id = c.document_id
                JOIN ami.projects p ON p.project_id = c.project_id
                JOIN ami.namespaces n ON n.namespace_id = c.namespace_id
                WHERE strpos(lower(c.content), lower($3)) > 0
            ),
            dedup AS (
                SELECT DISTINCT ON (project_id, namespace_id, relative_path, chunk_index)
                    project_code,
                    namespace_code,
                    repo_root,
                    relative_path,
                    chunk_id,
                    chunk_index,
                    start_line,
                    end_line,
                    score,
                    content,
                    metadata
                FROM exact_chunk_hits hit
                ORDER BY project_id, namespace_id, relative_path, chunk_index, score DESC
            )
            SELECT
                project_code,
                namespace_code,
                repo_root,
                relative_path,
                chunk_id,
                chunk_index,
                start_line,
                end_line,
                score,
                content,
                metadata
            FROM dedup
            ORDER BY score DESC, relative_path, chunk_index
            LIMIT $4
            "#,
            &[&project_ids, &namespace_ids, &query, &total_limit],
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

pub async fn search_memory_cards_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
    at_epoch_ms: Option<i64>,
) -> Result<Vec<MemoryCardRecord>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let normalized_query = normalized_memory_fts_query(query);
    let expanded_query = expanded_memory_fts_query(query);
    let rows = client
        .query(
            r#"
            SELECT
                mc.memory_card_id,
                p.code,
                n.code,
                mc.title,
                mc.summary,
                LEFT(mc.body, 4000),
                mc.tags,
                mc.provenance,
                mc.fact_subject,
                mc.fact_predicate,
                mc.fact_object,
                mc.truth_state,
                mc.verification_state,
                mc.status,
                mc.derivation_kind,
                mc.candidate_class,
                mc.source_kind,
                mc.hot_path_write_eligible,
                mc.background_consolidation_recommended,
                mc.observed_at_epoch_ms,
                mc.recorded_at_epoch_ms,
                mc.valid_from_epoch_ms,
                mc.valid_to_epoch_ms,
                mc.last_verified_at_epoch_ms,
                mc.superseded_by_memory_card_id,
                to_char(mc.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            FROM ami.memory_cards mc
            JOIN ami.projects p ON p.project_id = mc.project_id
            JOIN ami.namespaces n ON n.namespace_id = mc.namespace_id
            WHERE mc.project_id = $1
              AND mc.namespace_id = $2
              AND (
                    $6::bigint IS NOT NULL
                 OR (
                        mc.truth_state NOT IN ('superseded', 'retracted')
                    AND mc.status NOT IN ('superseded', 'archived')
                 )
              )
              AND (
                    (
                        mc.search_vector
                        || to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                            )
                        )
                    ) @@ websearch_to_tsquery('simple', $3)
                 OR (
                    COALESCE($4, '') <> ''
                    AND (
                        mc.search_vector
                        || to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                            )
                        )
                    ) @@ websearch_to_tsquery('simple', $4)
                 )
                 OR (
                    COALESCE($5, '') <> ''
                    AND (
                        mc.search_vector
                        || to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                            )
                        )
                    ) @@ websearch_to_tsquery('simple', $5)
                 )
              )
              AND (
                    $6::bigint IS NULL
                 OR (
                        (mc.valid_from_epoch_ms IS NULL OR mc.valid_from_epoch_ms <= $6)
                    AND (mc.valid_to_epoch_ms IS NULL OR mc.valid_to_epoch_ms >= $6)
                 )
              )
            ORDER BY
                CASE mc.truth_state
                    WHEN 'current' THEN 4
                    WHEN 'conflicted' THEN 3
                    WHEN 'unverified' THEN 2
                    WHEN 'superseded' THEN 1
                    WHEN 'retracted' THEN 0
                    ELSE 0
                END DESC,
                CASE mc.verification_state
                    WHEN 'verified' THEN 4
                    WHEN 'proposed' THEN 3
                    WHEN 'raw' THEN 2
                    WHEN 'deprecated' THEN 1
                    WHEN 'disputed' THEN 0
                    WHEN 'quarantined' THEN -1
                    ELSE 0
                END DESC,
                CASE mc.status
                    WHEN 'active' THEN 2
                    WHEN 'inactive' THEN 1
                    WHEN 'superseded' THEN 0
                    WHEN 'archived' THEN -1
                    ELSE 0
                END DESC,
                GREATEST(
                    ts_rank_cd(
                        mc.search_vector
                        || to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                            )
                        ),
                        websearch_to_tsquery('simple', $3)
                    ),
                    CASE
                        WHEN COALESCE($4, '') = '' THEN 0.0::real
                        ELSE ts_rank_cd(
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            ),
                            websearch_to_tsquery('simple', $4)
                        )
                    END,
                    CASE
                        WHEN COALESCE($5, '') = '' THEN 0.0::real
                        ELSE ts_rank_cd(
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            ),
                            websearch_to_tsquery('simple', $5)
                        )
                    END
                ) DESC,
                mc.created_at DESC
            LIMIT $7
            "#,
            &[
                &project_id,
                &namespace_id,
                &query,
                &expanded_query,
                &normalized_query,
                &at_epoch_ms,
                &limit,
            ],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| memory_card_record_from_row(&row))
        .collect())
}

pub async fn memory_card_search_temporal_stats_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    at_epoch_ms: Option<i64>,
) -> Result<MemoryCardSearchTemporalStats> {
    let normalized_query = normalized_memory_fts_query(query);
    let expanded_query = expanded_memory_fts_query(query);
    let row = client
        .query_one(
            r#"
            WITH matched AS (
                SELECT
                    mc.truth_state,
                    mc.status,
                    mc.valid_from_epoch_ms,
                    mc.valid_to_epoch_ms
                FROM ami.memory_cards mc
                WHERE mc.project_id = $1
                  AND mc.namespace_id = $2
                  AND (
                        (
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            )
                        ) @@ websearch_to_tsquery('simple', $3)
                     OR (
                            COALESCE($4, '') <> ''
                        AND (
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            )
                        ) @@ websearch_to_tsquery('simple', $4)
                     )
                     OR (
                            COALESCE($5, '') <> ''
                        AND (
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            )
                        ) @@ websearch_to_tsquery('simple', $5)
                     )
                  )
            )
            SELECT
                COUNT(*)::bigint AS prefilter_match_count,
                COUNT(*) FILTER (
                    WHERE (
                            $6::bigint IS NOT NULL
                         OR (
                                truth_state NOT IN ('superseded', 'retracted')
                            AND status NOT IN ('superseded', 'archived')
                         )
                    )
                      AND (
                            $6::bigint IS NULL
                         OR (
                                (valid_from_epoch_ms IS NULL OR valid_from_epoch_ms <= $6)
                            AND (valid_to_epoch_ms IS NULL OR valid_to_epoch_ms >= $6)
                         )
                    )
                )::bigint AS admissible_match_count,
                COUNT(*) FILTER (
                    WHERE $6::bigint IS NOT NULL
                      AND NOT (
                            (valid_from_epoch_ms IS NULL OR valid_from_epoch_ms <= $6)
                        AND (valid_to_epoch_ms IS NULL OR valid_to_epoch_ms >= $6)
                      )
                )::bigint AS excluded_by_temporal_window,
                COUNT(*) FILTER (
                    WHERE $6::bigint IS NULL
                      AND NOT (
                            truth_state NOT IN ('superseded', 'retracted')
                        AND status NOT IN ('superseded', 'archived')
                      )
                )::bigint AS excluded_by_current_truth_state
            FROM matched
            "#,
            &[
                &project_id,
                &namespace_id,
                &query,
                &expanded_query,
                &normalized_query,
                &at_epoch_ms,
            ],
        )
        .await?;

    Ok(MemoryCardSearchTemporalStats {
        prefilter_match_count: row.get(0),
        admissible_match_count: row.get(1),
        excluded_by_temporal_window: row.get(2),
        excluded_by_current_truth_state: row.get(3),
    })
}

pub async fn memory_card_temporal_exclusion_diagnostics_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    at_epoch_ms: Option<i64>,
    limit: i64,
) -> Result<Vec<MemoryCardTemporalExclusionDiagnostic>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let normalized_query = normalized_memory_fts_query(query);
    let expanded_query = expanded_memory_fts_query(query);
    let rows = client
        .query(
            r#"
            WITH matched AS (
                SELECT
                    mc.memory_card_id,
                    mc.title,
                    mc.truth_state,
                    mc.status,
                    mc.valid_from_epoch_ms,
                    mc.valid_to_epoch_ms,
                    CASE
                        WHEN $6::bigint IS NOT NULL
                             AND NOT (
                                    (mc.valid_from_epoch_ms IS NULL OR mc.valid_from_epoch_ms <= $6)
                                AND (mc.valid_to_epoch_ms IS NULL OR mc.valid_to_epoch_ms >= $6)
                             )
                            THEN 'outside_requested_time_slice'
                        WHEN $6::bigint IS NULL
                             AND (
                                    mc.truth_state IN ('superseded', 'retracted')
                                 OR mc.status IN ('superseded', 'archived')
                             )
                            THEN 'excluded_by_latest_truth_window'
                        ELSE NULL
                    END AS exclusion_reason
                FROM ami.memory_cards mc
                WHERE mc.project_id = $1
                  AND mc.namespace_id = $2
                  AND (
                        (
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            )
                        ) @@ websearch_to_tsquery('simple', $3)
                     OR (
                            COALESCE($4, '') <> ''
                        AND (
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            )
                        ) @@ websearch_to_tsquery('simple', $4)
                     )
                     OR (
                            COALESCE($5, '') <> ''
                        AND (
                            mc.search_vector
                            || to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    regexp_replace(COALESCE(mc.fact_subject, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_predicate, ''), '[[:punct:]_]+', ' ', 'g'),
                                    regexp_replace(COALESCE(mc.fact_object, ''), '[[:punct:]_]+', ' ', 'g')
                                )
                            )
                        ) @@ websearch_to_tsquery('simple', $5)
                     )
                  )
            )
            SELECT
                memory_card_id,
                title,
                truth_state,
                status,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                exclusion_reason
            FROM matched
            WHERE exclusion_reason IS NOT NULL
            ORDER BY
                CASE exclusion_reason
                    WHEN 'outside_requested_time_slice' THEN 0
                    WHEN 'excluded_by_latest_truth_window' THEN 1
                    ELSE 2
                END,
                valid_to_epoch_ms DESC NULLS LAST,
                title ASC
            LIMIT $7
            "#,
            &[
                &project_id,
                &namespace_id,
                &query,
                &expanded_query,
                &normalized_query,
                &at_epoch_ms,
                &limit,
            ],
        )
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| MemoryCardTemporalExclusionDiagnostic {
            memory_card_id: row.get(0),
            title: row.get(1),
            truth_state: row.get(2),
            status: row.get(3),
            valid_from_epoch_ms: row.get(4),
            valid_to_epoch_ms: row.get(5),
            exclusion_reason: row.get(6),
        })
        .collect())
}

pub async fn search_raw_evidence_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    limit: i64,
    at_epoch_ms: Option<i64>,
) -> Result<Vec<RawEvidenceRecord>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let expanded_query = expanded_fts_query(query);
    let rows = client
        .query(
            r#"
            SELECT
                mi.memory_item_id,
                mp.memory_provenance_id,
                p.code,
                n.code,
                mi.title,
                mi.summary,
                COALESCE(
                    NULLIF(mp.details ->> 'raw_excerpt', ''),
                    NULLIF(mp.details ->> 'excerpt', ''),
                    NULLIF(mi.summary, ''),
                    LEFT(COALESCE(mi.body, ''), 4000),
                    COALESCE(mp.evidence_span, '{}'::jsonb)::text,
                    COALESCE(mp.details, '{}'::jsonb)::text
                ) AS content,
                COALESCE(mp.source_kind, 'memory_item') AS source_kind,
                mp.source_event_id,
                mi.artifact_refs,
                COALESCE(mp.message_refs, mi.message_refs) AS message_refs,
                COALESCE(mp.evidence_span, mi.evidence_span) AS evidence_span,
                COALESCE(mp.details, '{}'::jsonb) AS details,
                mi.derivation_kind,
                mi.truth_state,
                mi.trust_state,
                mi.verification_state,
                mi.observed_at_epoch_ms,
                mi.recorded_at_epoch_ms,
                mi.valid_from_epoch_ms,
                mi.valid_to_epoch_ms,
                mi.last_verified_at_epoch_ms
            FROM ami.memory_items mi
            JOIN ami.projects p ON p.project_id = mi.project_id
            JOIN ami.namespaces n ON n.namespace_id = mi.namespace_id
            LEFT JOIN LATERAL (
                SELECT
                    mp.memory_provenance_id,
                    mp.source_kind,
                    mp.source_event_id,
                    mp.message_refs,
                    mp.evidence_span,
                    mp.details
                FROM ami.memory_provenance mp
                WHERE mp.memory_item_id = mi.memory_item_id
                  AND mp.source_kind <> 'memory_item_envelope'
                ORDER BY mp.created_at DESC
                LIMIT 1
            ) mp ON TRUE
            WHERE mi.project_id = $1
              AND mi.namespace_id = $2
              AND (
                    $5::bigint IS NOT NULL
                 OR mi.truth_state NOT IN ('superseded', 'retracted')
              )
              AND (
                    mp.memory_provenance_id IS NOT NULL
                 OR mi.derivation_kind IN ('raw_capture', 'import', 'verified_write_back', 'operator_write')
              )
              AND (
                    to_tsvector(
                        'simple',
                        concat_ws(
                            ' ',
                            COALESCE(mi.title, ''),
                            COALESCE(mi.summary, ''),
                            LEFT(COALESCE(mi.body, ''), 4000),
                            COALESCE(mp.source_kind, ''),
                            COALESCE(mp.source_event_id, ''),
                            COALESCE(mi.artifact_refs::text, ''),
                            COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                            COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                            COALESCE(mp.details::text, ''),
                            COALESCE(mi.derivation_kind, '')
                        )
                    ) @@ websearch_to_tsquery('simple', $3)
                 OR (
                    COALESCE($4, '') <> ''
                    AND to_tsvector(
                        'simple',
                        concat_ws(
                            ' ',
                            COALESCE(mi.title, ''),
                            COALESCE(mi.summary, ''),
                            LEFT(COALESCE(mi.body, ''), 4000),
                            COALESCE(mp.source_kind, ''),
                            COALESCE(mp.source_event_id, ''),
                            COALESCE(mi.artifact_refs::text, ''),
                            COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                            COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                            COALESCE(mp.details::text, ''),
                            COALESCE(mi.derivation_kind, '')
                        )
                    ) @@ websearch_to_tsquery('simple', $4)
                 )
              )
              AND (
                    $5::bigint IS NULL
                 OR (
                        (mi.valid_from_epoch_ms IS NULL OR mi.valid_from_epoch_ms <= $5)
                    AND (mi.valid_to_epoch_ms IS NULL OR mi.valid_to_epoch_ms >= $5)
                 )
              )
            ORDER BY
                GREATEST(
                    ts_rank_cd(
                        to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                COALESCE(mi.title, ''),
                                COALESCE(mi.summary, ''),
                                LEFT(COALESCE(mi.body, ''), 4000),
                                COALESCE(mp.source_kind, ''),
                                COALESCE(mp.source_event_id, ''),
                                COALESCE(mi.artifact_refs::text, ''),
                                COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                                COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                                COALESCE(mp.details::text, ''),
                                COALESCE(mi.derivation_kind, '')
                            )
                        ),
                        websearch_to_tsquery('simple', $3)
                    ),
                    CASE
                        WHEN COALESCE($4, '') = '' THEN 0.0::real
                        ELSE ts_rank_cd(
                            to_tsvector(
                                'simple',
                                concat_ws(
                                    ' ',
                                    COALESCE(mi.title, ''),
                                    COALESCE(mi.summary, ''),
                                    LEFT(COALESCE(mi.body, ''), 4000),
                                    COALESCE(mp.source_kind, ''),
                                    COALESCE(mp.source_event_id, ''),
                                    COALESCE(mi.artifact_refs::text, ''),
                                    COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                                    COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                                    COALESCE(mp.details::text, ''),
                                    COALESCE(mi.derivation_kind, '')
                                )
                            ),
                            websearch_to_tsquery('simple', $4)
                        )
                    END
                ) DESC,
                mi.created_at DESC
            LIMIT $6
            "#,
            &[&project_id, &namespace_id, &query, &expanded_query, &at_epoch_ms, &limit],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| raw_evidence_record_from_row(&row))
        .collect())
}

pub async fn raw_evidence_search_temporal_stats_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    at_epoch_ms: Option<i64>,
) -> Result<RawEvidenceSearchTemporalStats> {
    let expanded_query = expanded_fts_query(query);
    let row = client
        .query_one(
            r#"
            WITH matched AS (
                SELECT
                    mi.truth_state,
                    mi.valid_from_epoch_ms,
                    mi.valid_to_epoch_ms
                FROM ami.memory_items mi
                LEFT JOIN LATERAL (
                    SELECT
                        mp.memory_provenance_id,
                        mp.source_kind,
                        mp.source_event_id,
                        mp.message_refs,
                        mp.evidence_span,
                        mp.details
                    FROM ami.memory_provenance mp
                    WHERE mp.memory_item_id = mi.memory_item_id
                      AND mp.source_kind <> 'memory_item_envelope'
                    ORDER BY mp.created_at DESC
                    LIMIT 1
                ) mp ON TRUE
                WHERE mi.project_id = $1
                  AND mi.namespace_id = $2
                  AND (
                        mp.memory_provenance_id IS NOT NULL
                     OR mi.derivation_kind IN ('raw_capture', 'import', 'verified_write_back', 'operator_write')
                  )
                  AND (
                        to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                COALESCE(mi.title, ''),
                                COALESCE(mi.summary, ''),
                                LEFT(COALESCE(mi.body, ''), 4000),
                                COALESCE(mp.source_kind, ''),
                                COALESCE(mp.source_event_id, ''),
                                COALESCE(mi.artifact_refs::text, ''),
                                COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                                COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                                COALESCE(mp.details::text, ''),
                                COALESCE(mi.derivation_kind, '')
                            )
                        ) @@ websearch_to_tsquery('simple', $3)
                     OR (
                            COALESCE($4, '') <> ''
                        AND to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                COALESCE(mi.title, ''),
                                COALESCE(mi.summary, ''),
                                LEFT(COALESCE(mi.body, ''), 4000),
                                COALESCE(mp.source_kind, ''),
                                COALESCE(mp.source_event_id, ''),
                                COALESCE(mi.artifact_refs::text, ''),
                                COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                                COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                                COALESCE(mp.details::text, ''),
                                COALESCE(mi.derivation_kind, '')
                            )
                        ) @@ websearch_to_tsquery('simple', $4)
                     )
                  )
            )
            SELECT
                COUNT(*)::bigint AS prefilter_match_count,
                COUNT(*) FILTER (
                    WHERE (
                            $5::bigint IS NOT NULL
                         OR truth_state NOT IN ('superseded', 'retracted')
                    )
                      AND (
                            $5::bigint IS NULL
                         OR (
                                (valid_from_epoch_ms IS NULL OR valid_from_epoch_ms <= $5)
                            AND (valid_to_epoch_ms IS NULL OR valid_to_epoch_ms >= $5)
                         )
                    )
                )::bigint AS admissible_match_count,
                COUNT(*) FILTER (
                    WHERE $5::bigint IS NOT NULL
                      AND NOT (
                            (valid_from_epoch_ms IS NULL OR valid_from_epoch_ms <= $5)
                        AND (valid_to_epoch_ms IS NULL OR valid_to_epoch_ms >= $5)
                      )
                )::bigint AS excluded_by_temporal_window,
                COUNT(*) FILTER (
                    WHERE $5::bigint IS NULL
                      AND truth_state IN ('superseded', 'retracted')
                )::bigint AS excluded_by_current_truth_state
            FROM matched
            "#,
            &[&project_id, &namespace_id, &query, &expanded_query, &at_epoch_ms],
        )
        .await?;

    Ok(RawEvidenceSearchTemporalStats {
        prefilter_match_count: row.get(0),
        admissible_match_count: row.get(1),
        excluded_by_temporal_window: row.get(2),
        excluded_by_current_truth_state: row.get(3),
    })
}

pub async fn raw_evidence_temporal_exclusion_diagnostics_for_namespace(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    query: &str,
    at_epoch_ms: Option<i64>,
    limit: i64,
) -> Result<Vec<RawEvidenceTemporalExclusionDiagnostic>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let expanded_query = expanded_fts_query(query);
    let rows = client
        .query(
            r#"
            WITH matched AS (
                SELECT
                    mi.memory_item_id,
                    mi.title,
                    mi.truth_state,
                    mi.verification_state,
                    mi.valid_from_epoch_ms,
                    mi.valid_to_epoch_ms,
                    CASE
                        WHEN $5::bigint IS NOT NULL
                             AND NOT (
                                    (mi.valid_from_epoch_ms IS NULL OR mi.valid_from_epoch_ms <= $5)
                                AND (mi.valid_to_epoch_ms IS NULL OR mi.valid_to_epoch_ms >= $5)
                             )
                            THEN 'outside_requested_time_slice'
                        WHEN $5::bigint IS NULL
                             AND mi.truth_state IN ('superseded', 'retracted')
                            THEN 'excluded_by_latest_truth_window'
                        ELSE NULL
                    END AS exclusion_reason
                FROM ami.memory_items mi
                LEFT JOIN LATERAL (
                    SELECT
                        mp.memory_provenance_id,
                        mp.source_kind,
                        mp.source_event_id,
                        mp.message_refs,
                        mp.evidence_span,
                        mp.details
                    FROM ami.memory_provenance mp
                    WHERE mp.memory_item_id = mi.memory_item_id
                      AND mp.source_kind <> 'memory_item_envelope'
                    ORDER BY mp.created_at DESC
                    LIMIT 1
                ) mp ON TRUE
                WHERE mi.project_id = $1
                  AND mi.namespace_id = $2
                  AND (
                        mp.memory_provenance_id IS NOT NULL
                     OR mi.derivation_kind IN ('raw_capture', 'import', 'verified_write_back', 'operator_write')
                  )
                  AND (
                        to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                COALESCE(mi.title, ''),
                                COALESCE(mi.summary, ''),
                                LEFT(COALESCE(mi.body, ''), 4000),
                                COALESCE(mp.source_kind, ''),
                                COALESCE(mp.source_event_id, ''),
                                COALESCE(mi.artifact_refs::text, ''),
                                COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                                COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                                COALESCE(mp.details::text, ''),
                                COALESCE(mi.derivation_kind, '')
                            )
                        ) @@ websearch_to_tsquery('simple', $3)
                     OR (
                            COALESCE($4, '') <> ''
                        AND to_tsvector(
                            'simple',
                            concat_ws(
                                ' ',
                                COALESCE(mi.title, ''),
                                COALESCE(mi.summary, ''),
                                LEFT(COALESCE(mi.body, ''), 4000),
                                COALESCE(mp.source_kind, ''),
                                COALESCE(mp.source_event_id, ''),
                                COALESCE(mi.artifact_refs::text, ''),
                                COALESCE(mp.message_refs::text, mi.message_refs::text, ''),
                                COALESCE(mp.evidence_span::text, mi.evidence_span::text, ''),
                                COALESCE(mp.details::text, ''),
                                COALESCE(mi.derivation_kind, '')
                            )
                        ) @@ websearch_to_tsquery('simple', $4)
                     )
                  )
            )
            SELECT
                memory_item_id,
                title,
                truth_state,
                verification_state,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                exclusion_reason
            FROM matched
            WHERE exclusion_reason IS NOT NULL
            ORDER BY
                CASE exclusion_reason
                    WHEN 'outside_requested_time_slice' THEN 0
                    WHEN 'excluded_by_latest_truth_window' THEN 1
                    ELSE 2
                END,
                valid_to_epoch_ms DESC NULLS LAST,
                title ASC
            LIMIT $6
            "#,
            &[
                &project_id,
                &namespace_id,
                &query,
                &expanded_query,
                &at_epoch_ms,
                &limit,
            ],
        )
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| RawEvidenceTemporalExclusionDiagnostic {
            memory_item_id: row.get(0),
            title: row.get(1),
            truth_state: row.get(2),
            verification_state: row.get(3),
            valid_from_epoch_ms: row.get(4),
            valid_to_epoch_ms: row.get(5),
            exclusion_reason: row.get(6),
        })
        .collect())
}

fn normalized_memory_fts_query(query: &str) -> Option<String> {
    const MEMORY_QUERY_STOPWORDS: &[&str] = &[
        "a", "an", "and", "are", "at", "be", "does", "for", "how", "in", "is", "of", "on", "or",
        "the", "to", "what", "when", "where", "which", "who", "why",
    ];
    let normalized = query
        .replace(['_', '-', '.', ':', '/', '?'], " ")
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|token| token.len() >= 2 && !MEMORY_QUERY_STOPWORDS.contains(&token.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() || normalized == query {
        None
    } else {
        Some(normalized)
    }
}

fn expanded_fts_query(query: &str) -> Option<String> {
    let expanded = query.replace(['_', '-'], " ");
    let normalized = expanded.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut variants = Vec::new();
    for token in normalized.split_whitespace() {
        let cleaned = token
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
            .to_ascii_lowercase();
        if cleaned.len() < 4 {
            continue;
        }
        variants.push(cleaned.clone());
        if cleaned.ends_with('e') {
            variants.push(format!("{cleaned}d"));
            variants.push(format!("{}ing", &cleaned[..cleaned.len() - 1]));
        } else {
            variants.push(format!("{cleaned}ed"));
            variants.push(format!("{cleaned}ing"));
        }
        if cleaned.ends_with('y') && cleaned.len() > 4 {
            variants.push(format!("{}ies", &cleaned[..cleaned.len() - 1]));
        } else {
            variants.push(format!("{cleaned}s"));
        }
    }
    variants.sort();
    variants.dedup();
    let expanded_query = if variants.is_empty() {
        normalized.clone()
    } else {
        format!("{normalized} {}", variants.join(" "))
    };
    if expanded_query.is_empty() || expanded_query == query {
        None
    } else {
        Some(expanded_query)
    }
}

fn expanded_memory_fts_query(query: &str) -> Option<String> {
    let normalized = normalized_memory_fts_query(query)
        .unwrap_or_else(|| query.replace(['_', '-', '.', ':', '/', '?'], " "));
    let mut variants = Vec::new();
    for token in normalized.split_whitespace() {
        let cleaned = token
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
            .to_ascii_lowercase();
        if cleaned.len() < 4 {
            continue;
        }
        variants.push(cleaned.clone());
        if cleaned.ends_with('e') {
            variants.push(format!("{cleaned}d"));
            variants.push(format!("{}ing", &cleaned[..cleaned.len() - 1]));
        } else {
            variants.push(format!("{cleaned}ed"));
            variants.push(format!("{cleaned}ing"));
        }
        if cleaned.ends_with('y') && cleaned.len() > 4 {
            variants.push(format!("{}ies", &cleaned[..cleaned.len() - 1]));
        } else {
            variants.push(format!("{cleaned}s"));
        }
    }
    variants.sort();
    variants.dedup();
    let expanded_query = if variants.is_empty() {
        normalized.clone()
    } else {
        format!("{normalized} {}", variants.join(" "))
    };
    if expanded_query.is_empty() || expanded_query == query {
        None
    } else {
        Some(expanded_query)
    }
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
