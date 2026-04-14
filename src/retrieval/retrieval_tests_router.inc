    fn test_config() -> AppConfig {
        AppConfig {
            stack_name: "amai".to_string(),
            pg_db: "amai".to_string(),
            app_db_user: "amai".to_string(),
            app_db_password: "amai".to_string(),
            postgres_dsn: "postgres://localhost/unused".to_string(),
            app_postgres_dsn: "postgres://localhost/unused".to_string(),
            qdrant_url: "http://127.0.0.1:6334".to_string(),
            qdrant_http_url: "http://127.0.0.1:6334".to_string(),
            qdrant_collection_code: "test".to_string(),
            benchmark_qdrant_http_url: None,
            benchmark_qdrant_collection_code: None,
            qdrant_alias_code: "test".to_string(),
            qdrant_collection_memory: "memory".to_string(),
            qdrant_alias_memory: "memory".to_string(),
            qdrant_code_dim: 384,
            qdrant_memory_dim: 384,
            qdrant_distance: "Cosine".to_string(),
            s3_endpoint: "http://127.0.0.1:9000".to_string(),
            s3_region: "us-east-1".to_string(),
            s3_access_key: "test".to_string(),
            s3_secret_key: "test".to_string(),
            s3_bucket_artifacts: "artifacts".to_string(),
            s3_bucket_transcripts: "transcripts".to_string(),
            s3_bucket_context: "context".to_string(),
            nats_url: "nats://127.0.0.1:4222".to_string(),
            nats_http_url: "http://127.0.0.1:8222".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            edge_cache_path: "/tmp/edge-cache-test.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            local_fast_cache_ttl_ms: 5_000,
        }
    }

    #[test]
    fn query_terms_extracts_unique_alphanumeric_tokens() {
        assert_eq!(
            query_terms("Amai onboarding MCP VS Code integration"),
            vec![
                "amai".to_string(),
                "onboarding".to_string(),
                "mcp".to_string(),
                "code".to_string(),
                "integration".to_string()
            ]
        );
    }
    #[test]
    fn semantic_guard_abstains_without_lexical_signals_and_overlap() {
        let hit = json!({
            "relative_path": "packages/art-i18n/src/lib.rs",
            "content": "m.insert(\"shell.title\", \"Art Console\");"
        });
        let (accepted, summary) =
            apply_semantic_relevance_guard("Amai onboarding MCP VS Code integration", 0, vec![hit]);
        assert!(accepted.is_empty());
        assert!(summary.abstained);
        assert_eq!(summary.rejected_hits, 1);
        assert_eq!(summary.reason, Some("semantic_hits_missing_query_overlap"));
    }

    #[test]
    fn semantic_guard_keeps_hits_with_query_overlap() {
        let hit = json!({
            "relative_path": "docs/MCP_INTEGRATION.md",
            "content": "VS Code MCP onboarding guide"
        });
        assert!(semantic_hit_has_query_overlap(
            &hit,
            &query_terms("Amai onboarding MCP VS Code integration")
        ));
        let (accepted, summary) =
            apply_semantic_relevance_guard("Amai onboarding MCP VS Code integration", 0, vec![hit]);
        assert_eq!(accepted.len(), 1);
        assert!(!summary.abstained);
        assert_eq!(summary.accepted_hits, 1);
    }

    #[test]
    fn document_lookup_only_prefers_file_like_queries() {
        assert!(should_prefer_document_lookup_only("README.md"));
        assert!(should_prefer_document_lookup_only(
            "scripts/benchmark_matrix.sh"
        ));
        assert!(should_prefer_document_lookup_only("Makefile"));
        assert!(should_prefer_document_lookup_only("Dockerfile"));
        assert!(should_prefer_document_lookup_only("justfile"));
        assert!(!should_prefer_document_lookup_only("build_context_pack"));
        assert!(!should_prefer_document_lookup_only("ContextPackBuilder"));
    }

    #[test]
    fn exact_document_lookup_covers_common_config_basenames() {
        assert!(should_try_exact_document_lookup("Makefile"));
        assert!(should_try_exact_document_lookup("Dockerfile"));
        assert!(should_try_exact_document_lookup("justfile"));
        assert!(should_try_exact_document_lookup("WORKSPACE"));
        assert!(should_try_exact_document_lookup("package.json"));
        assert!(should_try_exact_document_lookup("README.md"));
        assert!(should_try_exact_document_lookup("Continuity snapshot"));
        assert!(!should_try_exact_document_lookup("build_context_pack"));
    }

    #[test]
    fn exact_document_lookup_queries_add_slugified_markdown_aliases() {
        assert_eq!(
            exact_document_lookup_queries("Continuity snapshot"),
            vec![
                "Continuity snapshot".to_string(),
                "continuity-snapshot".to_string(),
                "continuity-snapshot.md".to_string()
            ]
        );
        assert_eq!(
            exact_document_lookup_queries("src/config.rs"),
            vec!["src/config.rs".to_string()]
        );
    }

    #[test]
    fn exact_symbol_lookup_skips_hyphenated_cold_probe_queries() {
        assert!(should_try_exact_symbol_lookup("ContextPackBuilder"));
        assert!(should_try_exact_symbol_lookup("build_context_pack"));
        assert!(!should_try_exact_symbol_lookup("etalon-cold-push-87"));
    }

    #[test]
    fn context_pack_decision_trace_explains_included_and_missing_layers() {
        let payload = json!({
            "project": {"code": "art"},
            "namespace": {"code": "continuity"},
            "query": "restore previous chat context",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{"project_code": "art"}],
            "retrieval": {
                "exact_documents": [{"relative_path": "README.md"}],
                "symbol_hits": [],
                "lexical_chunks": [{"relative_path": "docs/guide.md"}],
                "semantic_chunks": [],
                "memory_cards": [],
                "memory_relation_edges": [],
                "raw_evidence": []
            },
            "quality": {
                "semantic_guard": {
                    "abstained": true,
                    "reason": "semantic_hits_missing_query_overlap",
                    "detail": "semantic layer abstained"
                }
            }
        });
        let trace = build_context_pack_decision_trace(&payload);
        assert_eq!(trace["included"].as_array().map(Vec::len), Some(2));
        assert_eq!(trace["not_included"].as_array().map(Vec::len), Some(5));
        assert_eq!(
            trace["evidence_ladder"]["cheapest_sufficient_layer"].as_str(),
            Some("summary_compact")
        );
        assert_eq!(trace["final_decision"].as_str(), Some("continue"));
        assert_eq!(trace["scope_resolver"]["status"].as_str(), Some("resolved"));
        assert_eq!(
            trace["escalate_if_needed"]["required"].as_bool(),
            Some(false)
        );
        assert_eq!(
            trace["abstain_if_insufficient"]["abstained"].as_bool(),
            Some(false)
        );
        let semantic_not_included = trace["not_included"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["strategy"].as_str() == Some("semantic_chunks"))
            })
            .expect("semantic not_included entry");
        assert_eq!(
            semantic_not_included["reason"].as_str(),
            Some("semantic layer abstained")
        );
        assert_eq!(
            trace["scope"]["effective_retrieval_mode"].as_str(),
            Some("local_strict")
        );
        assert_eq!(
            trace["intent_classifier"]["classification"].as_str(),
            Some("continuity")
        );
        assert_eq!(
            trace["evidence_sufficiency_check"]["cheapest_sufficient_layer"].as_str(),
            Some("summary_compact")
        );
        let raw_not_included = trace["not_included"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["strategy"].as_str() == Some("raw_evidence"))
            })
            .expect("raw not_included entry");
        assert_eq!(
            raw_not_included["reason"].as_str(),
            Some(
                "Raw evidence не поднимался, потому что более дешёвый summary/structured слой уже был достаточен."
            )
        );
    }

    #[test]
    fn context_pack_decision_trace_classifies_policy_queries_and_groups_candidates() {
        let payload = json!({
            "project": {"code": "art"},
            "namespace": {"code": "review"},
            "query": "what policy allows read access to this file path",
            "effective_retrieval_mode": "local_plus_related",
            "visible_projects": [{"project_code": "art"}],
            "temporal_query_epoch_ms": 1_735_689_600_000_i64,
            "retrieval": {
                "exact_documents": [{"relative_path": "docs/policy.md"}],
                "symbol_hits": [],
                "lexical_chunks": [{"relative_path": "docs/policy.md"}],
                "semantic_chunks": [],
                "memory_cards": [],
                "memory_relation_edges": [],
                "raw_evidence": []
            },
            "quality": {
                "semantic_guard": {
                    "abstained": true,
                    "reason": "semantic_hits_missing_query_overlap",
                    "detail": "semantic layer abstained"
                }
            }
        });

        let trace = build_context_pack_decision_trace(&payload);

        assert_eq!(
            trace["intent_classifier"]["classification"].as_str(),
            Some("policy_check")
        );
        assert_eq!(
            trace["candidate_generation"]["grouped_strategies"]["exact"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["candidate_generation"]["grouped_strategies"]["lexical"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["candidate_generation"]["temporal_query_requested"].as_bool(),
            Some(true)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_query_requested"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn context_pack_decision_trace_explains_historical_temporal_hits() {
        let payload = json!({
            "project": {"code": "project_alpha"},
            "namespace": {"code": "review"},
            "query": "historical semantic fact",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{"project_code": "project_alpha"}],
            "temporal_query_epoch_ms": 11_500_i64,
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": [],
                "memory_cards": [{
                    "memory_card_id": Uuid::new_v4(),
                    "truth_state": "superseded",
                    "valid_from_epoch_ms": 11_000,
                    "valid_to_epoch_ms": 11_999,
                    "summary": "historical semantic fact"
                }],
                "memory_relation_edges": [],
                "raw_evidence": []
            },
            "retrieval_temporal_stats": {
                "memory_cards": {
                    "prefilter_match_count": 3,
                    "excluded_by_temporal_window": 2,
                    "excluded_by_current_truth_state": 0,
                    "excluded_candidates": [{
                        "memory_card_id": Uuid::new_v4(),
                        "title": "newer fact",
                        "truth_state": "current",
                        "status": "active",
                        "valid_from_epoch_ms": 12_000,
                        "valid_to_epoch_ms": null,
                        "exclusion_reason": "outside_requested_time_slice"
                    }]
                },
                "raw_evidence": {
                    "prefilter_match_count": 1,
                    "excluded_by_temporal_window": 1,
                    "excluded_by_current_truth_state": 0,
                    "excluded_candidates": [{
                        "memory_item_id": Uuid::new_v4(),
                        "title": "newer raw evidence",
                        "truth_state": "current",
                        "verification_state": "verified",
                        "valid_from_epoch_ms": 12_000,
                        "valid_to_epoch_ms": null,
                        "exclusion_reason": "outside_requested_time_slice"
                    }]
                }
            },
            "quality": {
                "semantic_guard": {
                    "abstained": false,
                    "reason": "cheapest_sufficient_evidence_already_found",
                    "detail": null
                }
            }
        });

        let trace = build_context_pack_decision_trace(&payload);

        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["status"].as_str(),
            Some("applied_exact_time_slice")
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["historical_memory_cards_after_filter"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["prefilter_memory_cards"]
                .as_u64(),
            Some(3)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_cards_by_temporal_window"].as_u64(),
            Some(2)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_raw_evidence_by_temporal_window"].as_u64(),
            Some(1)
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_card_candidates"][0]["title"].as_str(),
            Some("newer fact")
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_card_candidates"][0]["exclusion_reason"].as_str(),
            Some("outside_requested_time_slice")
        );
        assert_eq!(
            trace["rerank_legality_relevance"]["temporal_legality"]["explanation"].as_str(),
            Some(
                "Historical-but-valid candidates remained admissible at the requested timestamp while out-of-window candidates were excluded."
            )
        );
    }

    #[test]
    fn retrieval_trace_insert_keeps_cheapest_sufficient_layer_in_evidence_span() {
        let context_pack_id = Uuid::new_v4();
        let payload = json!({
            "project": {"code": "project_alpha"},
            "namespace": {"code": "review"},
            "decision_trace": {
                "scope": {
                    "project_code": "project_alpha",
                    "namespace_code": "review",
                    "effective_retrieval_mode": "local_plus_related",
                    "visible_projects_total": 1
                },
                "candidate_generation": {"exact_documents": 1},
                "selection_priority": ["exact_documents", "lexical_chunks", "semantic_chunks"],
                "rerank_legality_relevance": {"scope_filtered": true},
                "included": [],
                "not_included": [],
                "evidence_ladder": {"cheapest_sufficient_layer": "structured_graph"},
                "evidence_sufficiency_check": {
                    "status": "sufficient",
                    "next_action": "answer_on_cheapest_sufficient_evidence"
                },
                "escalate_if_needed": {"required": false},
                "abstain_if_insufficient": {"abstained": false},
                "final_decision": "continue"
            }
        });
        let prepared = PreparedContextPack {
            context_pack_id,
            project: ProjectRecord {
                project_id: Uuid::new_v4(),
                code: "project_alpha".to_string(),
                display_name: "Project Alpha".to_string(),
                repo_root: "/tmp/project_alpha".to_string(),
                visibility_scope: "private".to_string(),
                updated_at: "2026-04-07T00:00:00Z".to_string(),
            },
            namespace_id: Uuid::new_v4(),
            effective_mode: "local_plus_related".to_string(),
            visible_projects_json: json!([{"project_code": "project_alpha"}]),
            payload: Arc::new(payload),
            payload_json: Arc::from("{}"),
            stats: ContextPackStats {
                context_pack_id,
                exact_documents: 1,
                symbol_hits: 0,
                lexical_chunks: 0,
                semantic_chunks: 0,
                cache_hit: false,
                scope_signature: "scope".to_string(),
                timings: ContextPackTimings {
                    resolve_scope_ms: 0,
                    cache_lookup_ms: 0,
                    exact_lookup_ms: 0,
                    symbol_lookup_ms: 0,
                    lexical_lookup_ms: 0,
                    query_embed_ms: 0,
                    semantic_search_ms: 0,
                    semantic_hydrate_ms: 0,
                    ranking_ms: 0,
                    provenance_ms: 0,
                    pack_assembly_ms: 0,
                    serialize_ms: 0,
                    persist_ms: 0,
                },
                retrieval_lower_bound_ms_precise: None,
            },
            cache_key: "cache-key".to_string(),
            scope_signature: "scope".to_string(),
            durably_persisted: true,
            artifact_bucket: None,
            artifact_object_key: None,
            artifact_state: None,
        };
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: false,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "proof_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };

        let trace = build_retrieval_trace_insert(Uuid::new_v4(), &prepared, &args);

        assert_eq!(
            trace.evidence_span["cheapest_sufficient_layer"].as_str(),
            Some("structured_graph")
        );
    }

    #[test]
    fn sufficiency_router_prefers_cheapest_available_layer() {
        assert_eq!(
            summary_layer_sufficiency(1, 0, 0, 0),
            (true, "exact_documents_sufficient")
        );
        assert_eq!(
            summary_layer_sufficiency(0, 0, 0, 1),
            (false, "single_memory_card_needs_structured_neighborhood")
        );
        assert_eq!(
            structured_layer_sufficiency(false, 2),
            (true, "structured_graph_neighborhood_sufficient")
        );
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "router".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: false,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "live_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        assert_eq!(
            semantic_skip_reason_for_router(&args, true, false),
            Some("cheapest_sufficient_evidence_already_found")
        );
        assert_eq!(semantic_skip_reason_for_router(&args, false, false), None);
    }
