async fn semantic_chunks(
    cfg: &AppConfig,
    db: &Client,
    visible_scopes: &[ResolvedVisibleScope],
    query: &str,
    limit: usize,
    lexical_signal_count: usize,
    lexical_fallback_chunks: &[ChunkHit],
) -> Result<(Vec<Value>, SemanticTimings, SemanticGuardSummary)> {
    if limit == 0 {
        return Ok((
            Vec::new(),
            SemanticTimings::default(),
            SemanticGuardSummary {
                query_terms: Vec::new(),
                lexical_signal_count,
                accepted_hits: 0,
                rejected_hits: 0,
                abstained: false,
                reason: None,
                detail: None,
            },
        ));
    }

    let mut any_vectors = false;
    for scope in visible_scopes {
        if postgres::namespace_has_vector_points(
            db,
            scope.visible.project.project_id,
            scope.namespace.namespace_id,
        )
        .await?
        {
            any_vectors = true;
            break;
        }
    }
    if !any_vectors {
        return Ok(semantic_fallback_result(
            query,
            limit,
            lexical_signal_count,
            lexical_fallback_chunks,
            SemanticTimings::default(),
            "no_vector_points_in_scope",
            None,
        ));
    }

    let (vector, query_embed_ms) = match embed_query(cfg, query) {
        Ok(value) => value,
        Err(error) => {
            return Ok(semantic_fallback_result(
                query,
                limit,
                lexical_signal_count,
                lexical_fallback_chunks,
                SemanticTimings::default(),
                "embedding_unavailable",
                Some(error.to_string()),
            ));
        }
    };
    if vector.len() as u64 != cfg.qdrant_code_dim {
        return Ok(semantic_fallback_result(
            query,
            limit,
            lexical_signal_count,
            lexical_fallback_chunks,
            SemanticTimings {
                query_embed_ms,
                ..SemanticTimings::default()
            },
            "embedding_unavailable",
            Some(format!(
                "query embedding size mismatch: expected {}, got {}",
                cfg.qdrant_code_dim,
                vector.len()
            )),
        ));
    }

    let search_started = Instant::now();
    let qdrant_client = match qdrant::connect(cfg) {
        Ok(client) => client,
        Err(error) => {
            return Ok(semantic_fallback_result(
                query,
                limit,
                lexical_signal_count,
                lexical_fallback_chunks,
                SemanticTimings {
                    query_embed_ms,
                    ..SemanticTimings::default()
                },
                "vector_layer_unavailable",
                Some(error.to_string()),
            ));
        }
    };
    let per_project_limit = limit.max(1);
    let mut points = Vec::new();
    for scope in visible_scopes {
        let result = match qdrant::search_namespace_points(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            vector.clone(),
            &scope.visible.project.code,
            &scope.namespace.code,
            per_project_limit,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                return Ok(semantic_fallback_result(
                    query,
                    limit,
                    lexical_signal_count,
                    lexical_fallback_chunks,
                    SemanticTimings {
                        query_embed_ms,
                        search_ms: search_started.elapsed().as_millis(),
                        ..SemanticTimings::default()
                    },
                    "vector_layer_unavailable",
                    Some(error.to_string()),
                ));
            }
        };
        points.extend(result);
    }
    let search_ms = search_started.elapsed().as_millis();

    let hydrate_started = Instant::now();
    let mut scored_points = Vec::new();
    let mut point_ids = Vec::new();
    for point in points {
        let point_id = match point.id.and_then(|id| id.point_id_options) {
            Some(PointIdOptions::Uuid(value)) => Uuid::parse_str(&value).ok(),
            _ => None,
        };
        let Some(point_id) = point_id else {
            continue;
        };
        point_ids.push(point_id);
        scored_points.push((point_id, point.score));
    }

    let hydrated = postgres::list_chunks_by_qdrant_point_ids(db, &point_ids).await?;
    let hydrated_by_point = hydrated.into_iter().collect::<HashMap<_, _>>();
    let mut hits = Vec::new();
    for (point_id, score) in scored_points {
        let Some(mut chunk) = hydrated_by_point.get(&point_id).cloned() else {
            continue;
        };
        chunk.score = score;
        hits.push(semantic_chunk_to_json(&chunk));
    }
    let hydrate_ms = hydrate_started.elapsed().as_millis();

    hits.sort_by(|left, right| {
        right["score"]
            .as_f64()
            .partial_cmp(&left["score"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    hits.retain(|hit| {
        let key = format!(
            "{}::{}",
            hit["provenance"]["chunk_id"].as_str().unwrap_or(""),
            hit["provenance"]["path"].as_str().unwrap_or("")
        );
        seen.insert(key)
    });
    if hits.is_empty() && !lexical_fallback_chunks.is_empty() {
        let fallback_hits = lexical_fallback_hits(query, lexical_fallback_chunks);
        return Ok((
            fallback_hits
                .iter()
                .take(limit)
                .map(|chunk| semantic_chunk_fallback_to_json(chunk))
                .collect(),
            SemanticTimings {
                query_embed_ms,
                search_ms,
                hydrate_ms,
            },
            SemanticGuardSummary {
                query_terms: query_terms(query),
                lexical_signal_count,
                accepted_hits: fallback_hits.len().min(limit),
                rejected_hits: 0,
                abstained: false,
                reason: None,
                detail: None,
            },
        ));
    }
    let semantic_guard = apply_semantic_relevance_guard(query, lexical_signal_count, hits);
    let mut guarded_hits = semantic_guard.0;
    guarded_hits.truncate(limit);
    Ok((
        guarded_hits,
        SemanticTimings {
            query_embed_ms,
            search_ms,
            hydrate_ms,
        },
        semantic_guard.1,
    ))
}

fn semantic_fallback_result(
    query: &str,
    limit: usize,
    lexical_signal_count: usize,
    lexical_fallback_chunks: &[ChunkHit],
    timings: SemanticTimings,
    reason: &'static str,
    detail: Option<String>,
) -> (Vec<Value>, SemanticTimings, SemanticGuardSummary) {
    let fallback_hits = lexical_fallback_hits(query, lexical_fallback_chunks);
    let accepted_hits = fallback_hits.len().min(limit);
    let abstained = accepted_hits == 0 && lexical_signal_count == 0;
    (
        fallback_hits
            .iter()
            .take(limit)
            .map(|chunk| semantic_chunk_fallback_to_json(chunk))
            .collect(),
        timings,
        SemanticGuardSummary {
            query_terms: query_terms(query),
            lexical_signal_count,
            accepted_hits,
            rejected_hits: 0,
            abstained,
            reason: Some(reason),
            detail,
        },
    )
}

fn semantic_skipped_result(
    query: &str,
    lexical_signal_count: usize,
    reason: &'static str,
) -> (Vec<Value>, SemanticTimings, SemanticGuardSummary) {
    (
        Vec::new(),
        SemanticTimings::default(),
        SemanticGuardSummary {
            query_terms: query_terms(query),
            lexical_signal_count,
            accepted_hits: 0,
            rejected_hits: 0,
            abstained: false,
            reason: Some(reason),
            detail: None,
        },
    )
}

fn semantic_skip_fallback_result(
    query: &str,
    limit: usize,
    lexical_signal_count: usize,
    documents: &[DocumentHit],
    lexical_fallback_chunks: &[ChunkHit],
    reason: &'static str,
) -> (Vec<Value>, SemanticTimings, SemanticGuardSummary) {
    let chunk_fallback_hits = lexical_fallback_hits(query, lexical_fallback_chunks);
    if !chunk_fallback_hits.is_empty() {
        let accepted_hits = chunk_fallback_hits.len().min(limit);
        return (
            chunk_fallback_hits
                .iter()
                .take(limit)
                .map(|chunk| semantic_chunk_fallback_to_json(chunk))
                .collect(),
            SemanticTimings::default(),
            SemanticGuardSummary {
                query_terms: query_terms(query),
                lexical_signal_count,
                accepted_hits,
                rejected_hits: 0,
                abstained: false,
                reason: Some(reason),
                detail: Some("semantic satisfied by lexical fallback".to_string()),
            },
        );
    }

    let document_fallback_hits = document_semantic_fallback_hits(query, documents);
    if !document_fallback_hits.is_empty() {
        let accepted_hits = document_fallback_hits.len().min(limit);
        return (
            document_fallback_hits
                .iter()
                .take(limit)
                .map(|document| semantic_document_fallback_to_json(document))
                .collect(),
            SemanticTimings::default(),
            SemanticGuardSummary {
                query_terms: query_terms(query),
                lexical_signal_count,
                accepted_hits,
                rejected_hits: 0,
                abstained: false,
                reason: Some(reason),
                detail: Some("semantic satisfied by exact document fallback".to_string()),
            },
        );
    }

    semantic_skipped_result(query, lexical_signal_count, reason)
}

fn lexical_fallback_hits<'a>(
    query: &str,
    lexical_fallback_chunks: &'a [ChunkHit],
) -> Vec<&'a ChunkHit> {
    let exact_matches = lexical_fallback_chunks
        .iter()
        .filter(|chunk| chunk.content.contains(query))
        .collect::<Vec<_>>();
    if !exact_matches.is_empty() {
        return exact_matches;
    }
    let query_lower = query.to_lowercase();
    let case_folded_matches = lexical_fallback_chunks
        .iter()
        .filter(|chunk| chunk.content.to_lowercase().contains(&query_lower))
        .collect::<Vec<_>>();
    if !case_folded_matches.is_empty() {
        return case_folded_matches;
    }
    lexical_fallback_chunks.iter().collect::<Vec<_>>()
}

fn document_semantic_fallback_hits<'a>(
    query: &str,
    documents: &'a [DocumentHit],
) -> Vec<&'a DocumentHit> {
    let exact_matches = documents
        .iter()
        .filter(|document| document.snippet.contains(query))
        .collect::<Vec<_>>();
    if !exact_matches.is_empty() {
        return exact_matches;
    }

    let query_lower = query.to_lowercase();
    let query_terms = query_terms(query)
        .into_iter()
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    documents
        .iter()
        .filter(|document| {
            let snippet_lower = document.snippet.to_lowercase();
            snippet_lower.contains(query_lower.as_str())
                || query_terms
                    .iter()
                    .all(|term| snippet_lower.contains(term.as_str()))
        })
        .collect()
}

fn semantic_chunk_to_json(chunk: &ChunkHit) -> Value {
    json!({
        "score": chunk.score,
        "retrieval_strategy": "vector_search",
        "project_code": chunk.project_code,
        "namespace_code": chunk.namespace_code,
        "relative_path": chunk.relative_path,
        "content": chunk.content,
        "provenance": {
            "source_project": chunk.project_code,
            "repo_root": chunk.repo_root,
            "commit_sha": Value::Null,
            "path": chunk.relative_path,
            "symbol": Value::Null,
            "chunk_id": chunk.chunk_id,
            "source_kind": "code_chunk",
            "trust_level": "local_repo"
        },
        "chunk": {
            "chunk_index": chunk.chunk_index,
            "start_line": chunk.start_line,
            "end_line": chunk.end_line,
            "metadata": chunk.metadata
        }
    })
}

fn semantic_chunk_fallback_to_json(chunk: &ChunkHit) -> Value {
    json!({
        "score": chunk.score,
        "retrieval_strategy": "lexical_fallback",
        "project_code": chunk.project_code,
        "namespace_code": chunk.namespace_code,
        "relative_path": chunk.relative_path,
        "content": chunk.content,
        "provenance": {
            "source_project": chunk.project_code,
            "repo_root": chunk.repo_root,
            "commit_sha": Value::Null,
            "path": chunk.relative_path,
            "symbol": Value::Null,
            "chunk_id": chunk.chunk_id,
            "source_kind": "code_chunk",
            "trust_level": "local_repo"
        },
        "chunk": {
            "chunk_index": chunk.chunk_index,
            "start_line": chunk.start_line,
            "end_line": chunk.end_line,
            "metadata": chunk.metadata
        }
    })
}

fn semantic_document_fallback_to_json(hit: &DocumentHit) -> Value {
    json!({
        "score": hit.score,
        "retrieval_strategy": "exact_document_fallback",
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "relative_path": hit.relative_path,
        "content": hit.snippet,
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": hit.git_commit_sha,
            "path": hit.relative_path,
            "symbol": Value::Null,
            "chunk_id": Value::Null,
            "source_kind": hit.source_kind,
            "trust_level": "local_repo"
        },
        "chunk": {
            "chunk_id": Value::Null,
            "chunk_index": Value::Null,
            "start_line": Value::Null,
            "end_line": Value::Null,
            "metadata": {
                "fallback_source": "exact_document"
            }
        }
    })
}

fn apply_semantic_relevance_guard(
    query: &str,
    lexical_signal_count: usize,
    hits: Vec<Value>,
) -> (Vec<Value>, SemanticGuardSummary) {
    let query_terms = query_terms(query);
    let require_overlap = lexical_signal_count == 0 && !query_terms.is_empty();
    let mut accepted_hits = Vec::new();
    let mut rejected_hits = 0usize;

    for hit in hits {
        if !require_overlap || semantic_hit_has_query_overlap(&hit, &query_terms) {
            accepted_hits.push(hit);
        } else {
            rejected_hits += 1;
        }
    }

    let accepted_count = accepted_hits.len();
    let abstained = require_overlap && accepted_count == 0 && rejected_hits > 0;
    (
        accepted_hits,
        SemanticGuardSummary {
            query_terms,
            lexical_signal_count,
            accepted_hits: accepted_count,
            rejected_hits,
            abstained,
            reason: if abstained {
                Some("semantic_hits_missing_query_overlap")
            } else {
                None
            },
            detail: None,
        },
    )
}

fn semantic_hit_has_query_overlap(hit: &Value, query_terms: &[String]) -> bool {
    if query_terms.is_empty() {
        return true;
    }

    let relative_path = hit["relative_path"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let content = hit["content"].as_str().unwrap_or_default().to_lowercase();

    query_terms
        .iter()
        .any(|term| relative_path.contains(term) || content.contains(term))
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let mut current = String::new();

    let push_current =
        |current: &mut String, terms: &mut Vec<String>, seen: &mut HashSet<String>| {
            if current.chars().count() >= 3 {
                let lowered = current.to_lowercase();
                if seen.insert(lowered.clone()) {
                    terms.push(lowered);
                }
            }
            current.clear();
        };

    for ch in query.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            push_current(&mut current, &mut terms, &mut seen);
        }
    }
    if !current.is_empty() {
        push_current(&mut current, &mut terms, &mut seen);
    }

    terms
}

fn semantic_guard_to_json(guard: &SemanticGuardSummary) -> Value {
    json!({
        "query_terms": guard.query_terms,
        "lexical_signal_count": guard.lexical_signal_count,
        "accepted_hits": guard.accepted_hits,
        "rejected_hits": guard.rejected_hits,
        "abstained": guard.abstained,
        "reason": guard.reason,
        "detail": guard.detail
    })
}

