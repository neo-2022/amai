fn collect_workspace_document_requests(
    visible_scopes: &[ResolvedVisibleScope],
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
) -> Vec<(Uuid, String)> {
    let scope_ids = visible_scopes
        .iter()
        .map(|scope| {
            (
                format!("{}::{}", scope.visible.project.code, scope.namespace.code),
                scope.namespace.namespace_id,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut requests = Vec::new();
    for hit in documents {
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
        );
    }
    for hit in symbols {
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
        );
    }
    for hit in chunks {
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            &hit.project_code,
            &hit.namespace_code,
            &hit.relative_path,
        );
    }
    for hit in semantic_chunks {
        let Some(project_code) = hit["project_code"].as_str() else {
            continue;
        };
        let Some(namespace_code) = hit["namespace_code"].as_str() else {
            continue;
        };
        let Some(relative_path) = hit["relative_path"].as_str() else {
            continue;
        };
        push_workspace_request(
            &scope_ids,
            &mut seen,
            &mut requests,
            project_code,
            namespace_code,
            relative_path,
        );
    }
    requests
}

fn push_workspace_request(
    scope_ids: &HashMap<String, Uuid>,
    seen: &mut HashSet<String>,
    requests: &mut Vec<(Uuid, String)>,
    project_code: &str,
    namespace_code: &str,
    relative_path: &str,
) {
    let key = format!("{project_code}::{namespace_code}");
    let Some(namespace_id) = scope_ids.get(&key) else {
        return;
    };
    let dedupe_key = format!("{namespace_id}::{relative_path}");
    if seen.insert(dedupe_key) {
        requests.push((*namespace_id, relative_path.to_string()));
    }
}

fn should_use_minimal_document_workspace_graph(
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
) -> bool {
    documents.len() == 1 && symbols.is_empty() && chunks.is_empty() && semantic_chunks.is_empty()
}

fn should_use_minimal_symbol_workspace_graph(
    documents: &[DocumentHit],
    symbols: &[SymbolHit],
    chunks: &[ChunkHit],
    semantic_chunks: &[Value],
) -> bool {
    documents.is_empty() && chunks.is_empty() && semantic_chunks.is_empty() && symbols.len() == 1
}

fn should_use_compact_workspace_graph_enrichment(
    workspace_requests: &[(Uuid, String)],
    semantic_chunks: &[Value],
) -> bool {
    semantic_chunks.is_empty() && workspace_requests.len() <= 2
}

pub fn degradation_proof_scenarios(local_fast_cache_ttl_ms: u128) -> Result<Vec<Value>> {
    let lexical_chunk = synthetic_chunk_hit(
        "src/degradation.rs",
        "fn stable_semantic_fallback() { /* lexical safety net */ }",
    );
    let (qdrant_hits, _, qdrant_guard) = semantic_fallback_result(
        "stable semantic fallback",
        4,
        1,
        &[lexical_chunk.clone()],
        SemanticTimings::default(),
        "vector_layer_unavailable",
        Some("synthetic Qdrant outage".to_string()),
    );
    let qdrant_unavailable_pass = qdrant_hits
        .first()
        .and_then(|item| item["retrieval_strategy"].as_str())
        == Some("lexical_fallback")
        && qdrant_guard.reason == Some("vector_layer_unavailable")
        && qdrant_guard.accepted_hits == 1;

    let (empty_embedding_hits, _, empty_embedding_guard) = semantic_fallback_result(
        "stable semantic fallback",
        4,
        1,
        &[lexical_chunk],
        SemanticTimings::default(),
        "embedding_unavailable",
        Some("synthetic empty embedding vector".to_string()),
    );
    let empty_embeddings_pass = empty_embedding_hits
        .first()
        .and_then(|item| item["retrieval_strategy"].as_str())
        == Some("lexical_fallback")
        && empty_embedding_guard.reason == Some("embedding_unavailable")
        && empty_embedding_guard.accepted_hits == 1;

    let stale_cache_state = degradation_probe_stale_fast_cache(local_fast_cache_ttl_ms)?;
    let stale_cache_pass = stale_cache_state["cache_hit"].as_bool() == Some(false)
        && stale_cache_state["cache_entry_remaining"].as_bool() == Some(false);

    Ok(vec![
        json!({
            "class_key": "qdrant_unavailable",
            "title": "Qdrant недоступен",
            "status": if qdrant_unavailable_pass { "pass" } else { "critical" },
            "reason": if qdrant_unavailable_pass {
                "retrieval держит безопасный lexical fallback, когда vector layer недоступен."
            } else {
                "retrieval не удержал безопасный lexical fallback при недоступном vector layer."
            },
            "details": {
                "retrieval_strategy": qdrant_hits.first().and_then(|item| item["retrieval_strategy"].as_str()).unwrap_or("none"),
                "semantic_guard_reason": qdrant_guard.reason,
                "semantic_guard_detail": qdrant_guard.detail,
            }
        }),
        json!({
            "class_key": "stale_cache",
            "title": "Устаревший cache",
            "status": if stale_cache_pass { "pass" } else { "critical" },
            "reason": if stale_cache_pass {
                "expired local fast cache не выдаётся как свежий hit и честно вычищается перед reuse."
            } else {
                "expired local fast cache продолжил выглядеть как свежий hit."
            },
            "details": stale_cache_state,
        }),
        json!({
            "class_key": "empty_embeddings",
            "title": "Пустые embeddings",
            "status": if empty_embeddings_pass { "pass" } else { "critical" },
            "reason": if empty_embeddings_pass {
                "retrieval честно уходит в lexical fallback, когда semantic embedding layer не даёт usable vector."
            } else {
                "retrieval потерял безопасный fallback при пустом semantic embedding layer."
            },
            "details": {
                "retrieval_strategy": empty_embedding_hits.first().and_then(|item| item["retrieval_strategy"].as_str()).unwrap_or("none"),
                "semantic_guard_reason": empty_embedding_guard.reason,
                "semantic_guard_detail": empty_embedding_guard.detail,
            }
        }),
    ])
}

fn degradation_probe_stale_fast_cache(local_fast_cache_ttl_ms: u128) -> Result<Value> {
    let proof_key = 0xfeed_u128;
    let expired_at = now_epoch_ms().saturating_sub(local_fast_cache_ttl_ms.saturating_add(1));
    let entry = LocalContextPackEntry {
        context_pack_id: Uuid::new_v4(),
        payload: Arc::new(json!({
            "context_pack_id": Uuid::nil(),
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        })),
        exact_documents: 0,
        symbol_hits: 0,
        lexical_chunks: 0,
        semantic_chunks: 0,
        durably_persisted: true,
        cached_at_epoch_ms: expired_at,
    };
    local_fast_context_pack_cache_put(proof_key, &entry)?;
    let cache_hit =
        local_fast_context_pack_cache_contains(proof_key, local_fast_cache_ttl_ms, true)?;
    let cache_entry_remaining =
        local_fast_context_pack_cache_get(proof_key, local_fast_cache_ttl_ms)?.is_some();
    Ok(json!({
        "cache_hit": cache_hit,
        "cache_entry_remaining": cache_entry_remaining,
        "expired_by_ms": now_epoch_ms().saturating_sub(expired_at).saturating_sub(local_fast_cache_ttl_ms),
    }))
}

fn synthetic_chunk_hit(relative_path: &str, content: &str) -> ChunkHit {
    ChunkHit {
        project_code: "art".to_string(),
        namespace_code: "continuity".to_string(),
        repo_root: "/tmp/degradation-proof".to_string(),
        relative_path: relative_path.to_string(),
        chunk_id: Uuid::new_v4(),
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        score: 1.0,
        content: content.to_string(),
        metadata: json!({}),
    }
}
