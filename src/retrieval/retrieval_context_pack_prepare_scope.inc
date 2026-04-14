async fn prepare_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
) -> Result<PreparedContextPack> {
    let resolve_started = Instant::now();
    let project = postgres::get_project_by_code(db, &args.project).await?;
    let namespace =
        postgres::get_namespace_by_code(db, project.project_id, &args.namespace).await?;
    let effective_mode = args
        .retrieval_mode
        .clone()
        .unwrap_or_else(|| namespace.retrieval_mode.clone());
    let visible_projects = resolve_visible_projects(db, &project, &effective_mode).await?;
    let visible_scopes =
        resolve_visible_scopes(db, &visible_projects, project.project_id, &namespace).await?;
    let resolve_scope_ms = resolve_started.elapsed().as_millis();
    let scope_signature = scope_signature(&visible_scopes);
    let cache_key = cache_key(cfg, args, &effective_mode);
    let edge_cache_path = if args.disable_cache || !cfg.edge_cache_path.exists() {
        None
    } else {
        Some(cfg.edge_cache_path.clone())
    };

    let cache_lookup_started = Instant::now();
    if !args.disable_cache {
        if let Some(cached) = local_context_pack_cache_get(&cache_key, &scope_signature)? {
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
            return prepared_from_cached_payload(
                CacheHydrationContext {
                    project: &project,
                    namespace_id: namespace.namespace_id,
                    effective_mode: &effective_mode,
                    scope_signature,
                    cache_key,
                    resolve_scope_ms,
                    cache_lookup_ms,
                    precise_lower_bound_ms: precise_elapsed_ms(resolve_started),
                },
                cached,
            );
        }
        if let Some(edge_cache_path) = edge_cache_path.as_ref()
            && let Some(cached) = edge_cache::get_context_pack_cache_entry(
                edge_cache_path,
                &cache_key,
                &scope_signature,
            )?
        {
            let local_cached = local_entry_from_edge(cached)?;
            local_context_pack_cache_put(&cache_key, &scope_signature, &local_cached)?;
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
            return prepared_from_cached_payload(
                CacheHydrationContext {
                    project: &project,
                    namespace_id: namespace.namespace_id,
                    effective_mode: &effective_mode,
                    scope_signature,
                    cache_key,
                    resolve_scope_ms,
                    cache_lookup_ms,
                    precise_lower_bound_ms: precise_elapsed_ms(resolve_started),
                },
                local_cached,
            );
        }
    }
    let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

    let query_prefers_document_lookup = should_prefer_document_lookup_only(&args.query);
    let should_try_exact_document = should_try_exact_document_lookup(&args.query);
    let should_try_exact_symbol = should_try_exact_symbol_lookup(&args.query);
    let should_try_exact_chunk = should_try_exact_chunk_lookup(&args.query);
    let should_search_documents = args.limit_documents > 0;
    let should_search_symbols = should_try_exact_symbol;

    let mut documents = Vec::new();
    let exact_started = Instant::now();
    if args.limit_documents > 0 && should_search_documents {
        let mut exact_scope_hits = HashSet::new();
        let mut local_exact_document_hit_found = false;
        let exact_document_queries = exact_document_lookup_queries(&args.query);
        if should_try_exact_document {
            for scope in &visible_scopes {
                for query in &exact_document_queries {
                    let normalized_query_path = normalized_safe_relative_path(query)
                        .map(|value| value.to_string_lossy().into_owned());
                    let cache_lookup_path =
                        normalized_query_path.as_deref().unwrap_or(query.as_str());
                    let content = edge_cache_path
                        .as_ref()
                        .and_then(|edge_cache_path| {
                            edge_cache::get_cached_document_snippet_by_path(
                                edge_cache_path,
                                &scope.visible.project.code,
                                &scope.namespace.code,
                                cache_lookup_path,
                                2000,
                            )
                            .ok()
                            .flatten()
                        })
                        .or_else(|| {
                            edge_cache_path.as_ref().and_then(|edge_cache_path| {
                                edge_cache::get_cached_document_snippet_by_project_path(
                                    edge_cache_path,
                                    &scope.visible.project.code,
                                    cache_lookup_path,
                                    2000,
                                )
                                .ok()
                                .flatten()
                            })
                        });
                    if let Some(content) = content {
                        local_exact_document_hit_found = true;
                        exact_scope_hits.insert((
                            scope.visible.project.code.clone(),
                            scope.namespace.code.clone(),
                        ));
                        documents.push(DocumentHit {
                            project_code: scope.visible.project.code.clone(),
                            namespace_code: scope.namespace.code.clone(),
                            repo_root: scope.visible.project.repo_root.clone(),
                            relative_path: cache_lookup_path.to_string(),
                            language: None,
                            source_kind: "code_document".to_string(),
                            git_commit_sha: None,
                            score: 1.0,
                            snippet: content.chars().take(2000).collect(),
                        });
                        break;
                    }
                }
            }
            if !local_exact_document_hit_found {
                for query in &exact_document_queries {
                    let unresolved_scope_ids = visible_scopes
                        .iter()
                        .filter(|scope| {
                            !exact_scope_hits.contains(&(
                                scope.visible.project.code.clone(),
                                scope.namespace.code.clone(),
                            ))
                        })
                        .map(|scope| {
                            (
                                scope.visible.project.project_id,
                                scope.namespace.namespace_id,
                            )
                        })
                        .collect::<Vec<_>>();
                    if unresolved_scope_ids.is_empty() {
                        break;
                    }
                    let exact_hits = postgres::search_documents_exact_for_scopes(
                        db,
                        &unresolved_scope_ids,
                        query,
                        args.limit_documents as i64,
                    )
                    .await?;
                    for hit in &exact_hits {
                        exact_scope_hits
                            .insert((hit.project_code.clone(), hit.namespace_code.clone()));
                    }
                    documents.extend(exact_hits);
                    if !documents.is_empty() {
                        break;
                    }
                }
            }
            if documents.is_empty() {
                for scope in &visible_scopes {
                    for query in &exact_document_queries {
                        let normalized_query_path = normalized_safe_relative_path(query)
                            .map(|value| value.to_string_lossy().into_owned());
                        let cache_lookup_path =
                            normalized_query_path.as_deref().unwrap_or(query.as_str());
                        let content = local_repo_document_snippet(
                            &scope.visible.project.repo_root,
                            cache_lookup_path,
                        )
                        .ok()
                        .flatten()
                        .or_else(|| {
                            local_repo_document_snippet(&scope.visible.project.repo_root, query)
                                .ok()
                                .flatten()
                        });
                        if let Some(content) = content {
                            exact_scope_hits.insert((
                                scope.visible.project.code.clone(),
                                scope.namespace.code.clone(),
                            ));
                            documents.push(DocumentHit {
                                project_code: scope.visible.project.code.clone(),
                                namespace_code: scope.namespace.code.clone(),
                                repo_root: scope.visible.project.repo_root.clone(),
                                relative_path: cache_lookup_path.to_string(),
                                language: None,
                                source_kind: "code_document".to_string(),
                                git_commit_sha: None,
                                score: 1.0,
                                snippet: content.chars().take(2000).collect(),
                            });
                            break;
                        }
                    }
                }
            }
        }
        for scope in &visible_scopes {
            if exact_scope_hits.contains(&(
                scope.visible.project.code.clone(),
                scope.namespace.code.clone(),
            )) {
                continue;
            }
            documents.extend(
                postgres::search_documents_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_documents as i64,
                )
                .await?,
            );
        }
    }
    let exact_lookup_ms = exact_started.elapsed().as_millis();

    let document_exact_hit_found = should_try_exact_document && !documents.is_empty();
    let mut symbols = Vec::new();
    let symbol_started = Instant::now();
    let skip_symbol_lookup = query_prefers_document_lookup && document_exact_hit_found;
    if args.limit_symbols > 0 && should_search_symbols && !skip_symbol_lookup {
        let mut exact_scope_hits = HashSet::new();
        if should_try_exact_symbol {
            let scope_ids = visible_scopes
                .iter()
                .map(|scope| {
                    (
                        scope.visible.project.project_id,
                        scope.namespace.namespace_id,
                    )
                })
                .collect::<Vec<_>>();
            let exact_hits = postgres::search_symbols_exact_for_scopes(
                db,
                &scope_ids,
                &args.query,
                args.limit_symbols as i64,
            )
            .await?;
            for hit in &exact_hits {
                exact_scope_hits.insert((hit.project_code.clone(), hit.namespace_code.clone()));
            }
            symbols.extend(exact_hits);
        }
        for scope in &visible_scopes {
            if exact_scope_hits.contains(&(
                scope.visible.project.code.clone(),
                scope.namespace.code.clone(),
            )) {
                continue;
            }
            symbols.extend(
                postgres::search_symbols_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_symbols as i64,
                )
                .await?,
            );
        }
    }
    let symbol_lookup_ms = symbol_started.elapsed().as_millis();

    let mut chunks = Vec::new();
    let mut memory_cards = Vec::new();
    let mut memory_relation_edges = Vec::new();
    let mut memory_card_prefilter_match_count = 0_i64;
    let mut memory_card_excluded_by_temporal_window = 0_i64;
    let mut memory_card_excluded_by_current_truth_state = 0_i64;
    let mut memory_card_temporal_exclusions = Vec::new();
    let lexical_started = Instant::now();
    let skip_chunk_lookup = args.limit_chunks > 0
        && args.token_source_kind != "verify_context_pack"
        && ((should_try_exact_document && !documents.is_empty())
            || (should_try_exact_symbol && !symbols.is_empty()));
    if args.limit_chunks > 0 && !skip_chunk_lookup && should_try_exact_chunk {
        let scope_ids = visible_scopes
            .iter()
            .map(|scope| {
                (
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                )
            })
            .collect::<Vec<_>>();
        chunks.extend(
            postgres::search_chunks_exact_for_scopes(
                db,
                &scope_ids,
                &args.query,
                args.limit_chunks as i64,
            )
            .await?,
        );
    }
    let skip_lexical_lookup = skip_chunk_lookup
        || (args.limit_chunks > 0
            && args.token_source_kind != "verify_context_pack"
            && ((should_try_exact_document && !documents.is_empty())
                || (should_try_exact_symbol && !symbols.is_empty())
                || (should_try_exact_chunk && !chunks.is_empty())));
    if args.limit_chunks > 0 && !skip_lexical_lookup {
        for scope in &visible_scopes {
            chunks.extend(
                postgres::search_chunks_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    args.limit_chunks as i64,
                )
                .await?,
            );
        }
    }
    let memory_limit = args.limit_chunks.min(8);
    let skip_memory_lookup = memory_limit > 0
        && should_skip_memory_lookup(args, documents.len(), symbols.len(), chunks.len(), 0);
    if memory_limit > 0 && !skip_memory_lookup {
        let now_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before unix epoch")?
            .as_millis() as i64;
        let at_epoch_ms = args.at_epoch_ms.unwrap_or(now_epoch_ms);
        for scope in &visible_scopes {
            let scope_card_stats = postgres::memory_card_search_temporal_stats_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                Some(at_epoch_ms),
            )
            .await?;
            memory_card_prefilter_match_count += scope_card_stats.prefilter_match_count;
            memory_card_excluded_by_temporal_window += scope_card_stats.excluded_by_temporal_window;
            memory_card_excluded_by_current_truth_state +=
                scope_card_stats.excluded_by_current_truth_state;
            let mut scope_card_exclusions =
                postgres::memory_card_temporal_exclusion_diagnostics_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    Some(at_epoch_ms),
                    4,
                )
                .await?;
            memory_card_temporal_exclusions.append(&mut scope_card_exclusions);
            let mut scope_cards = postgres::search_memory_cards_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                memory_limit as i64,
                Some(at_epoch_ms),
            )
            .await?;
            memory_cards.append(&mut scope_cards);
        }
        let (summary_sufficient, _) = summary_layer_sufficiency(
            documents.len(),
            symbols.len(),
            chunks.len(),
            memory_cards.len(),
        );
        if !summary_sufficient {
            for scope in &visible_scopes {
                let scope_card_ids = memory_cards
                    .iter()
                    .filter(|card| {
                        card.project_code == scope.visible.project.code
                            && card.namespace_code == scope.namespace.code
                    })
                    .map(|card| card.memory_card_id)
                    .collect::<Vec<_>>();
                if scope_card_ids.is_empty() {
                    continue;
                }
                let relation_limit = (scope_card_ids.len() as i64 * 4).max(12);
                memory_relation_edges.extend(
                    postgres::list_memory_relation_edges_for_cards(
                        db,
                        scope.visible.project.project_id,
                        scope.namespace.namespace_id,
                        &scope_card_ids,
                        Some(at_epoch_ms),
                        relation_limit,
                    )
                    .await?,
                );
            }
        }
    }
    let lexical_lookup_ms = lexical_started.elapsed().as_millis();

    let ranking_started = Instant::now();
    sort_and_truncate_documents(&mut documents, args.limit_documents);
    sort_and_truncate_symbols(&mut symbols, args.limit_symbols);
    sort_and_truncate_chunks(&mut chunks, args.limit_chunks);
    let ranking_ms = ranking_started.elapsed().as_millis();

    let (summary_sufficient, _) = summary_layer_sufficiency(
        documents.len(),
        symbols.len(),
        chunks.len(),
        memory_cards.len(),
    );
    let (structured_sufficient, _) =
        structured_layer_sufficiency(summary_sufficient, memory_relation_edges.len());
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let at_epoch_ms = args.at_epoch_ms.unwrap_or(now_epoch_ms);
    let mut raw_evidence = Vec::new();
    let mut raw_evidence_prefilter_match_count = 0_i64;
    let mut raw_evidence_excluded_by_temporal_window = 0_i64;
    let mut raw_evidence_excluded_by_current_truth_state = 0_i64;
    let mut raw_evidence_temporal_exclusions = Vec::new();
    if !structured_sufficient {
        let raw_limit = args.limit_semantic_chunks.max(4).min(16) as i64;
        for scope in &visible_scopes {
            let scope_raw_stats = postgres::raw_evidence_search_temporal_stats_for_namespace(
                db,
                scope.visible.project.project_id,
                scope.namespace.namespace_id,
                &args.query,
                Some(at_epoch_ms),
            )
            .await?;
            raw_evidence_prefilter_match_count += scope_raw_stats.prefilter_match_count;
            raw_evidence_excluded_by_temporal_window += scope_raw_stats.excluded_by_temporal_window;
            raw_evidence_excluded_by_current_truth_state +=
                scope_raw_stats.excluded_by_current_truth_state;
            let mut scope_raw_exclusions =
                postgres::raw_evidence_temporal_exclusion_diagnostics_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    Some(at_epoch_ms),
                    4,
                )
                .await?;
            raw_evidence_temporal_exclusions.append(&mut scope_raw_exclusions);
            raw_evidence.extend(
                postgres::search_raw_evidence_for_namespace(
                    db,
                    scope.visible.project.project_id,
                    scope.namespace.namespace_id,
                    &args.query,
                    raw_limit,
                    Some(at_epoch_ms),
                )
                .await?,
            );
        }
    }
    let benchmark_compact_payload = should_use_benchmark_compact_payload(&args.token_source_kind);
    let lexical_signal_count = documents.len()
        + symbols.len()
        + chunks.len()
        + memory_cards.len()
        + memory_relation_edges.len();
    let semantic_skip_reason =
        semantic_skip_reason_for_router(args, summary_sufficient, structured_sufficient);
    let (semantic_chunks, semantic_timings, semantic_guard) =
        if benchmark_compact_payload && summary_sufficient {
            semantic_skipped_result(
                &args.query,
                args.limit_semantic_chunks,
                "benchmark_compact_summary_sufficient",
            )
        } else if let Some(reason) = semantic_skip_reason {
            semantic_skip_fallback_result(
                &args.query,
                args.limit_semantic_chunks,
                lexical_signal_count,
                &documents,
                &chunks,
                reason,
            )
        } else {
            semantic_chunks(
                cfg,
                db,
                &visible_scopes,
                &args.query,
                args.limit_semantic_chunks,
                lexical_signal_count,
                &chunks,
            )
            .await?
        };
    let provenance_started = Instant::now();
    let visible_projects_json = json!(
        visible_scopes
            .iter()
            .map(|scope| {
                json!({
                    "project_code": scope.visible.project.code,
                    "display_name": scope.visible.project.display_name,
                    "repo_root": scope.visible.project.repo_root,
                    "relation_type": scope.visible.relation_type,
                    "project_link_type": scope.visible.project_link_type,
                    "shared_contour": scope.visible.shared_contour,
                    "visibility_scope": scope.visible.visibility_scope,
                    "relation_status": scope.visible.relation_status,
                    "requires_approval": scope.visible.requires_approval,
                    "transfer_policy_code": scope.visible.transfer_policy_code,
                    "access_mode": scope.visible.access_mode,
                    "namespace_code": scope.namespace.code,
                    "namespace_display_name": scope.namespace.display_name
                })
            })
            .collect::<Vec<_>>()
    );
    let context_pack_id = Uuid::new_v4();
    let workspace_graph = if benchmark_compact_payload {
        Value::Null
    } else {
        let workspace_requests = collect_workspace_document_requests(
            &visible_scopes,
            &documents,
            &symbols,
            &chunks,
            &semantic_chunks,
        );
        let minimal_document_context = should_use_minimal_document_workspace_graph(
            &documents,
            &symbols,
            &chunks,
            &semantic_chunks,
        );
        let minimal_symbol_context = should_use_minimal_symbol_workspace_graph(
            &documents,
            &symbols,
            &chunks,
            &semantic_chunks,
        );
        let compact_workspace_enrichment =
            should_use_compact_workspace_graph_enrichment(&workspace_requests, &semantic_chunks);
        let (workspace_documents, workspace_symbols) =
            if minimal_document_context || minimal_symbol_context || compact_workspace_enrichment {
                (Vec::new(), Vec::new())
            } else {
                (
                    postgres::list_document_structures_for_namespace_paths(db, &workspace_requests)
                        .await?,
                    postgres::list_document_symbols_for_namespace_paths(db, &workspace_requests)
                        .await?,
                )
            };
        workspace_graph::build_context_pack_workspace_graph(
            &context_pack_id,
            &args.query,
            &effective_mode,
            &scope_signature,
            &visible_projects_json,
            &documents,
            &symbols,
            &chunks,
            &semantic_chunks,
            &workspace_documents,
            &workspace_symbols,
        )?
    };
    let provenance_minimum = json!([
        "source_project",
        "repo_root",
        "commit_sha",
        "path",
        "symbol",
        "chunk_id",
        "source_kind",
        "trust_level"
    ]);
    let provenance_ms = provenance_started.elapsed().as_millis();
    let generated_epoch_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs();
    let pack_assembly_started = Instant::now();
    let serialize_started = Instant::now();
    let stats = ContextPackStats {
        context_pack_id,
        exact_documents: documents.len(),
        symbol_hits: symbols.len(),
        lexical_chunks: chunks.len(),
        semantic_chunks: semantic_chunks.len(),
        cache_hit: false,
        scope_signature: scope_signature.clone(),
        timings: ContextPackTimings {
            resolve_scope_ms,
            cache_lookup_ms,
            exact_lookup_ms,
            symbol_lookup_ms,
            lexical_lookup_ms,
            query_embed_ms: semantic_timings.query_embed_ms,
            semantic_search_ms: semantic_timings.search_ms,
            semantic_hydrate_ms: semantic_timings.hydrate_ms,
            ranking_ms,
            provenance_ms,
            pack_assembly_ms: 0,
            serialize_ms: 0,
            persist_ms: 0,
        },
        retrieval_lower_bound_ms_precise: None,
    };
    let payload = json!({
        "context_pack_id": context_pack_id,
        "stack_name": cfg.stack_name,
        "tool_name": "Art-memory-agent-index",
        "tool_short_name": "Amai",
        "generated_at_epoch_s": generated_epoch_s,
        "project": {
            "code": project.code,
            "display_name": project.display_name,
            "repo_root": project.repo_root
        },
        "namespace": {
            "code": namespace.code,
            "display_name": namespace.display_name
        },
        "query": args.query,
        "temporal_query_epoch_ms": args.at_epoch_ms,
        "effective_retrieval_mode": effective_mode,
        "visible_projects": visible_projects_json,
        "retrieval": {
            "exact_documents": documents.iter().map(document_to_json).collect::<Vec<_>>(),
            "symbol_hits": symbols.iter().map(symbol_to_json).collect::<Vec<_>>(),
            "lexical_chunks": chunks.iter().map(chunk_to_json).collect::<Vec<_>>(),
            "semantic_chunks": semantic_chunks,
            "memory_cards": memory_cards.iter().map(memory_card_to_json).collect::<Vec<_>>(),
            "memory_relation_edges": memory_relation_edges
                .iter()
                .map(memory_relation_edge_to_json)
                .collect::<Vec<_>>(),
            "raw_evidence": raw_evidence
                .iter()
                .map(raw_evidence_to_json)
                .collect::<Vec<_>>()
        },
        "retrieval_temporal_stats": {
            "memory_cards": {
                "prefilter_match_count": memory_card_prefilter_match_count,
                "excluded_by_temporal_window": memory_card_excluded_by_temporal_window,
                "excluded_by_current_truth_state": memory_card_excluded_by_current_truth_state,
                "excluded_candidates": memory_card_temporal_exclusions
                    .iter()
                    .map(memory_card_temporal_exclusion_to_json)
                    .collect::<Vec<_>>()
            },
            "raw_evidence": {
                "prefilter_match_count": raw_evidence_prefilter_match_count,
                "excluded_by_temporal_window": raw_evidence_excluded_by_temporal_window,
                "excluded_by_current_truth_state": raw_evidence_excluded_by_current_truth_state,
                "excluded_candidates": raw_evidence_temporal_exclusions
                    .iter()
                    .map(raw_evidence_temporal_exclusion_to_json)
                    .collect::<Vec<_>>()
            }
        },
        "quality": {
            "semantic_guard": semantic_guard_to_json(&semantic_guard)
        },
        "workspace_graph": workspace_graph,
        "provenance_minimum": provenance_minimum,
        "retrieval_runtime": runtime_json(&stats)
    });
    let mut payload = payload;
    if !benchmark_compact_payload {
        ensure_context_pack_decision_trace(&mut payload);
    }
    let pack_assembly_ms = pack_assembly_started.elapsed().as_millis();
    let payload_json: Arc<str> = Arc::from(serde_json::to_string(&payload)?);
    let mut stats = stats;
    stats.timings.pack_assembly_ms = pack_assembly_ms;
    stats.timings.serialize_ms = serialize_started.elapsed().as_millis();

    Ok(PreparedContextPack {
        context_pack_id,
        project,
        namespace_id: namespace.namespace_id,
        effective_mode,
        visible_projects_json,
        payload: Arc::new(payload),
        payload_json,
        stats,
        cache_key,
        scope_signature,
        durably_persisted: false,
        artifact_bucket: None,
        artifact_object_key: None,
        artifact_state: None,
    })
}

