fn build_same_thread_cache_reuse_payload(payload: &Value, _thread_id: &str) -> Value {
    let active_files = context_pack_active_files(payload, 6);
    let default_project_code = payload["project"]["code"].as_str().unwrap_or_default();
    json!({
        "context_pack_id": payload["context_pack_id"].clone(),
        "project": {
            "code": payload["project"]["code"].clone(),
        },
        "namespace": {
            "code": payload["namespace"]["code"].clone(),
        },
        "query": payload["query"].clone(),
        "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
        "visible_projects": compact_visible_projects_for_token_budget(&payload["visible_projects"]),
        "retrieval_runtime": compact_retrieval_runtime_for_token_budget(&payload["retrieval_runtime"]),
        "whole_cycle_observed": compact_whole_cycle_observed_for_token_budget(&payload["whole_cycle_observed"]),
        "retrieval": {
            "exact_documents": metadata_only_exact_documents(
                &payload["retrieval"]["exact_documents"],
                default_project_code,
            ),
            "symbol_hits": compact_symbol_hits(&payload["retrieval"]["symbol_hits"], default_project_code),
            "lexical_chunks": metadata_only_chunk_refs(
                &payload["retrieval"]["lexical_chunks"],
                default_project_code,
            ),
            "semantic_chunks": metadata_only_chunk_refs(
                &payload["retrieval"]["semantic_chunks"],
                default_project_code,
            ),
            "memory_cards": metadata_only_memory_cards(
                &payload["retrieval"]["memory_cards"],
                default_project_code,
            ),
            "memory_relation_edges": metadata_only_memory_relation_edges(
                &payload["retrieval"]["memory_relation_edges"],
                default_project_code,
            ),
            "raw_evidence": metadata_only_raw_evidence(
                &payload["retrieval"]["raw_evidence"],
                default_project_code,
            ),
        },
        "cache_reuse_reference": {
            "state": "same_thread_context_pack_replay",
            "source_context_pack_id": payload["context_pack_id"].clone(),
            "active_files": active_files,
            "retrieval_counts": {
                "exact_documents": payload["retrieval"]["exact_documents"].as_array().map_or(0, Vec::len),
                "symbol_hits": payload["retrieval"]["symbol_hits"].as_array().map_or(0, Vec::len),
                "lexical_chunks": payload["retrieval"]["lexical_chunks"].as_array().map_or(0, Vec::len),
                "semantic_chunks": payload["retrieval"]["semantic_chunks"].as_array().map_or(0, Vec::len),
                "memory_cards": payload["retrieval"]["memory_cards"].as_array().map_or(0, Vec::len),
                "memory_relation_edges": payload["retrieval"]["memory_relation_edges"]
                    .as_array()
                    .map_or(0, Vec::len),
                "raw_evidence": payload["retrieval"]["raw_evidence"].as_array().map_or(0, Vec::len),
            }
        }
    })
}

fn compact_visible_projects_for_token_budget(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            json!({
                "project_code": item["project_code"].clone(),
                "repo_root": item["repo_root"].clone(),
            })
        })
        .collect()
}

fn compact_retrieval_runtime_for_token_budget(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "cache_hit": value["cache_hit"].clone(),
        "scope_signature": value["scope_signature"].clone(),
        "retrieval_lower_bound_ms": value["retrieval_lower_bound_ms"].clone(),
        "total_ms": value["total_ms"].clone(),
    })
}

fn compact_whole_cycle_observed_for_token_budget(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "client_prompt_tokens": value["client_prompt_tokens"].clone(),
        "assistant_generation_tokens": value["assistant_generation_tokens"].clone(),
        "tool_overhead_tokens": value["tool_overhead_tokens"].clone(),
        "continuity_restore_tokens": value["continuity_restore_tokens"].clone(),
    })
}

fn metadata_only_exact_documents(value: &Value, default_project_code: &str) -> Vec<Value> {
    let mut seen_paths = HashSet::new();
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let signature = compact_text_path_signature(
                item["project_code"]
                    .as_str()
                    .or_else(|| item["provenance"]["source_project"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
            )?;
            if !seen_paths.insert(signature) {
                return None;
            }
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            Some(compact)
        })
        .collect()
}

fn metadata_only_chunk_refs(value: &Value, default_project_code: &str) -> Vec<Value> {
    let mut seen_paths = HashSet::new();
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let signature = compact_text_path_signature(
                item["project_code"]
                    .as_str()
                    .or_else(|| item["provenance"]["source_project"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
            )?;
            if !seen_paths.insert(signature) {
                return None;
            }
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            Some(compact)
        })
        .collect()
}

fn metadata_only_memory_cards(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_card_id": item["memory_card_id"].clone(),
                "title": item["title"].clone(),
                "truth_state": item["truth_state"].clone(),
                "verification_state": item["verification_state"].clone(),
                "status": item["status"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_memory_cards(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_card_id": item["memory_card_id"].clone(),
                "title": item["title"].clone(),
                "summary": item["summary"].clone(),
                "fact_subject": item["fact_subject"].clone(),
                "fact_predicate": item["fact_predicate"].clone(),
                "fact_object": item["fact_object"].clone(),
                "truth_state": item["truth_state"].clone(),
                "verification_state": item["verification_state"].clone(),
                "status": item["status"].clone(),
                "observed_at_epoch_ms": item["observed_at_epoch_ms"].clone(),
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"].clone(),
                "valid_from_epoch_ms": item["valid_from_epoch_ms"].clone(),
                "valid_to_epoch_ms": item["valid_to_epoch_ms"].clone(),
                "last_verified_at_epoch_ms": item["last_verified_at_epoch_ms"].clone(),
                "superseded_by_memory_card_id": item["superseded_by_memory_card_id"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn metadata_only_memory_relation_edges(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_relation_edge_id": item["memory_relation_edge_id"].clone(),
                "source_memory_card_id": item["source_memory_card_id"].clone(),
                "target_memory_card_id": item["target_memory_card_id"].clone(),
                "relation_type": item["relation_type"].clone(),
                "relation_state": item["relation_state"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn metadata_only_raw_evidence(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_item_id": item["memory_item_id"].clone(),
                "memory_provenance_id": item["memory_provenance_id"].clone(),
                "title": item["title"].clone(),
                "source_kind": item["source_kind"].clone(),
                "verification_state": item["verification_state"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_memory_relation_edges(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_relation_edge_id": item["memory_relation_edge_id"].clone(),
                "source_memory_card_id": item["source_memory_card_id"].clone(),
                "target_memory_card_id": item["target_memory_card_id"].clone(),
                "relation_type": item["relation_type"].clone(),
                "relation_state": item["relation_state"].clone(),
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"].clone(),
                "valid_from_epoch_ms": item["valid_from_epoch_ms"].clone(),
                "valid_to_epoch_ms": item["valid_to_epoch_ms"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_raw_evidence(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "memory_item_id": item["memory_item_id"].clone(),
                "memory_provenance_id": item["memory_provenance_id"].clone(),
                "title": item["title"].clone(),
                "content": item["content"].clone(),
                "source_kind": item["source_kind"].clone(),
                "source_event_id": item["source_event_id"].clone(),
                "artifact_refs": item["artifact_refs"].clone(),
                "message_refs": item["message_refs"].clone(),
                "evidence_span": item["evidence_span"].clone(),
                "derivation_kind": item["derivation_kind"].clone(),
                "truth_state": item["truth_state"].clone(),
                "trust_state": item["trust_state"].clone(),
                "verification_state": item["verification_state"].clone(),
                "observed_at_epoch_ms": item["observed_at_epoch_ms"].clone(),
                "recorded_at_epoch_ms": item["recorded_at_epoch_ms"].clone(),
                "valid_from_epoch_ms": item["valid_from_epoch_ms"].clone(),
                "valid_to_epoch_ms": item["valid_to_epoch_ms"].clone(),
                "last_verified_at_epoch_ms": item["last_verified_at_epoch_ms"].clone(),
                "relative_path": item["relative_path"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn context_pack_active_files(payload: &Value, limit: usize) -> Vec<String> {
    let mut active_files = Vec::new();
    for key in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
        "raw_evidence",
    ] {
        for item in payload["retrieval"][key].as_array().into_iter().flatten() {
            let Some(path) = item["relative_path"]
                .as_str()
                .or_else(|| item["provenance"]["path"].as_str())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if !active_files.iter().any(|existing| existing == path) {
                active_files.push(path.to_string());
            }
            if active_files.len() >= limit {
                return active_files;
            }
        }
    }
    active_files
}

fn thread_context_pack_delivery_seen(thread_id: &str, context_pack_id: &str) -> Result<bool> {
    let cache = THREAD_CONTEXT_PACK_DELIVERY_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let guard = cache
        .read()
        .map_err(|_| anyhow!("thread context-pack delivery cache lock poisoned"))?;
    Ok(guard
        .get(thread_id)
        .is_some_and(|entries| entries.contains(context_pack_id)))
}

fn remember_thread_context_pack_delivery(payload: &Value) -> Result<()> {
    let Some(thread_id) = codex_threads::current_thread_id() else {
        return Ok(());
    };
    let Some(context_pack_id) = payload["context_pack_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    remember_thread_context_pack_delivery_pair(&thread_id, context_pack_id)
}

fn remember_thread_context_pack_delivery_pair(
    thread_id: &str,
    context_pack_id: &str,
) -> Result<()> {
    let cache = THREAD_CONTEXT_PACK_DELIVERY_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut guard = cache
        .write()
        .map_err(|_| anyhow!("thread context-pack delivery cache lock poisoned"))?;
    guard
        .entry(thread_id.to_string())
        .or_insert_with(HashSet::new)
        .insert(context_pack_id.to_string());
    Ok(())
}

fn with_whole_cycle_observed_overrides(payload: &Value, args: &ContextPackArgs) -> Value {
    if args.client_prompt_tokens.is_none()
        && args.assistant_generation_tokens.is_none()
        && args.tool_overhead_tokens.is_none()
        && args.continuity_restore_tokens.is_none()
    {
        return payload.clone();
    }
    let mut augmented = payload.clone();
    let Some(root) = augmented.as_object_mut() else {
        return payload.clone();
    };
    let whole_cycle = root
        .entry("whole_cycle_observed".to_string())
        .or_insert_with(|| json!({}));
    if !whole_cycle.is_object() {
        *whole_cycle = json!({});
    }
    let Some(whole_cycle_object) = whole_cycle.as_object_mut() else {
        return payload.clone();
    };
    if let Some(tokens) = args.client_prompt_tokens {
        whole_cycle_object.insert("client_prompt_tokens".to_string(), Value::from(tokens));
    }
    if let Some(tokens) = args.assistant_generation_tokens {
        whole_cycle_object.insert(
            "assistant_generation_tokens".to_string(),
            Value::from(tokens),
        );
    }
    if let Some(tokens) = args.tool_overhead_tokens {
        whole_cycle_object.insert("tool_overhead_tokens".to_string(), Value::from(tokens));
    }
    if let Some(tokens) = args.continuity_restore_tokens {
        whole_cycle_object.insert("continuity_restore_tokens".to_string(), Value::from(tokens));
    }
    augmented
}

pub(crate) fn model_visible_context_pack_payload(payload: &Value) -> Value {
    if is_same_thread_cache_reuse_payload(payload) {
        return model_visible_same_thread_cache_reuse_payload(payload);
    }
    let default_project_code = payload["project"]["code"].as_str().unwrap_or_default();
    let (lexical_chunks, semantic_chunks) =
        compact_retrieval_chunks(&payload["retrieval"], default_project_code);
    let chunk_contents_by_path = compact_chunk_contents_by_path(&lexical_chunks, &semantic_chunks);
    json!({
        "context_pack_id": payload["context_pack_id"].clone(),
        "project": {
            "code": payload["project"]["code"].clone(),
        },
        "namespace": {
            "code": payload["namespace"]["code"].clone(),
        },
        "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
        "visible_projects": compact_visible_projects_for_token_budget(&payload["visible_projects"]),
        "retrieval": {
            "exact_documents": compact_exact_documents(
                &payload["retrieval"]["exact_documents"],
                &chunk_contents_by_path,
                default_project_code,
            ),
            "symbol_hits": compact_symbol_hits(&payload["retrieval"]["symbol_hits"], default_project_code),
            "lexical_chunks": lexical_chunks,
            "semantic_chunks": semantic_chunks,
            "memory_cards": compact_memory_cards(
                &payload["retrieval"]["memory_cards"],
                default_project_code,
            ),
            "memory_relation_edges": compact_memory_relation_edges(
                &payload["retrieval"]["memory_relation_edges"],
                default_project_code,
            ),
            "raw_evidence": compact_raw_evidence(
                &payload["retrieval"]["raw_evidence"],
                default_project_code,
            ),
        }
    })
}

fn is_same_thread_cache_reuse_payload(payload: &Value) -> bool {
    payload["cache_reuse_reference"]["state"].as_str() == Some("same_thread_context_pack_replay")
}

fn model_visible_same_thread_cache_reuse_payload(payload: &Value) -> Value {
    json!({
        "context_pack_id": payload["context_pack_id"].clone(),
        "project": {
            "code": payload["project"]["code"].clone(),
        },
        "namespace": {
            "code": payload["namespace"]["code"].clone(),
        },
        "cache_reuse_reference": compact_cache_reuse_reference(&payload["cache_reuse_reference"]),
    })
}

fn compact_cache_reuse_reference(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    let active_files = value["active_files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let retrieval_counts = json!({
        "exact_documents": value["retrieval_counts"]["exact_documents"].as_u64().unwrap_or(0),
        "symbol_hits": value["retrieval_counts"]["symbol_hits"].as_u64().unwrap_or(0),
        "lexical_chunks": value["retrieval_counts"]["lexical_chunks"].as_u64().unwrap_or(0),
        "semantic_chunks": value["retrieval_counts"]["semantic_chunks"].as_u64().unwrap_or(0),
        "memory_cards": value["retrieval_counts"]["memory_cards"].as_u64().unwrap_or(0),
        "memory_relation_edges": value["retrieval_counts"]["memory_relation_edges"]
            .as_u64()
            .unwrap_or(0),
        "raw_evidence": value["retrieval_counts"]["raw_evidence"].as_u64().unwrap_or(0),
    });
    let has_non_zero_counts = retrieval_counts
        .as_object()
        .is_some_and(|items| items.values().any(|item| item.as_u64().unwrap_or(0) > 0));
    let mut compact = json!({
        "state": value["state"].clone(),
        "source_context_pack_id": value["source_context_pack_id"].clone(),
    });
    if let Some(object) = compact.as_object_mut() {
        if !active_files.is_empty() {
            object.insert("active_files".to_string(), json!(active_files));
        }
        if has_non_zero_counts {
            object.insert("retrieval_counts".to_string(), retrieval_counts);
        }
    }
    compact
}

fn compact_exact_documents(
    value: &Value,
    chunk_contents_by_path: &HashMap<String, Vec<String>>,
    default_project_code: &str,
) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let duplicate_snippet =
                exact_document_snippet_covered_by_chunks(item, chunk_contents_by_path);
            if duplicate_snippet {
                return None;
            }
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
                "snippet": item["snippet"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            Some(compact)
        })
        .collect()
}

fn compact_retrieval_chunks(
    retrieval: &Value,
    default_project_code: &str,
) -> (Vec<Value>, Vec<Value>) {
    let mut seen_signatures = HashSet::new();
    let lexical_chunks = compact_chunks(
        &retrieval["lexical_chunks"],
        &mut seen_signatures,
        default_project_code,
    );
    let semantic_chunks = compact_chunks(
        &retrieval["semantic_chunks"],
        &mut seen_signatures,
        default_project_code,
    );
    (lexical_chunks, semantic_chunks)
}

fn compact_chunk_contents_by_path(
    lexical_chunks: &[Value],
    semantic_chunks: &[Value],
) -> HashMap<String, Vec<String>> {
    let mut contents_by_path = HashMap::new();
    for item in lexical_chunks.iter().chain(semantic_chunks.iter()) {
        let Some(path_signature) = compact_text_path_signature(
            item["project_code"]
                .as_str()
                .or_else(|| item["provenance"]["source_project"].as_str())
                .unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
        ) else {
            continue;
        };
        let Some(content) = item["content"].as_str() else {
            continue;
        };
        let entry = contents_by_path
            .entry(path_signature)
            .or_insert_with(Vec::new);
        if !entry.iter().any(|existing| existing == content) {
            entry.push(content.to_string());
        }
    }
    contents_by_path
}

fn compact_symbol_hits(value: &Value, default_project_code: &str) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
                "name": item["name"].clone(),
                "kind": item["kind"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_chunks(
    value: &Value,
    seen_signatures: &mut HashSet<String>,
    default_project_code: &str,
) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let signature = compact_chunk_signature(item)?;
            if !seen_signatures.insert(signature) {
                return None;
            }
            Some(item)
        })
        .map(|item| {
            let mut compact = json!({
                "relative_path": item["relative_path"].clone(),
                "content": item["content"].clone(),
            });
            insert_compact_project_reference(&mut compact, item, default_project_code);
            compact
        })
        .collect()
}

fn compact_item_project_code<'a>(item: &'a Value, default_project_code: &str) -> Option<&'a str> {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .filter(|value| !value.is_empty() && *value != default_project_code)
}

fn insert_compact_project_reference(compact: &mut Value, item: &Value, default_project_code: &str) {
    let Some(project_code) = compact_item_project_code(item, default_project_code) else {
        return;
    };
    if let Some(object) = compact.as_object_mut() {
        object.insert("project_code".to_string(), json!(project_code));
    }
}

fn compact_chunk_signature(item: &Value) -> Option<String> {
    let project_code = item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .unwrap_or_default();
    let relative_path = item["relative_path"].as_str().unwrap_or_default();
    let content = item["content"].as_str().unwrap_or_default();
    compact_text_signature(project_code, relative_path, content)
}

fn exact_document_snippet_covered_by_chunks(
    item: &Value,
    chunk_contents_by_path: &HashMap<String, Vec<String>>,
) -> bool {
    let relative_path = item["relative_path"].as_str().unwrap_or_default();
    let project_code = item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .unwrap_or_default();
    let candidate_signatures = [
        compact_text_path_signature(project_code, relative_path),
        compact_text_path_signature("", relative_path),
    ];
    if candidate_signatures.iter().all(Option::is_none) {
        return false;
    }
    let Some(snippet) = item["snippet"].as_str() else {
        return false;
    };
    if snippet.is_empty() {
        return false;
    }
    candidate_signatures.iter().flatten().any(|path_signature| {
        chunk_contents_by_path
            .get(path_signature)
            .is_some_and(|contents| contents.iter().any(|content| content.contains(snippet)))
    })
}

fn compact_text_path_signature(project_code: &str, relative_path: &str) -> Option<String> {
    if project_code.is_empty() && relative_path.is_empty() {
        return None;
    }
    Some(format!("{project_code}\u{1f}{relative_path}"))
}

fn compact_text_signature(project_code: &str, relative_path: &str, text: &str) -> Option<String> {
    if project_code.is_empty() && relative_path.is_empty() && text.is_empty() {
        return None;
    }
    Some(format!("{project_code}\u{1f}{relative_path}\u{1f}{text}"))
}

