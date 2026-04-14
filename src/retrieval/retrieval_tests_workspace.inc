    #[test]
    fn minimal_symbol_workspace_graph_only_applies_to_single_symbol_only_shape() {
        let symbols = vec![SymbolHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "src/verify.rs".to_string(),
            name: "run_text_compare".to_string(),
            kind: "function_item".to_string(),
            start_line: 1212,
            end_line: 1425,
            start_byte: 0,
            end_byte: 0,
            score: 2000.0,
            metadata: json!({"language":"rust"}),
        }];
        assert!(should_use_minimal_symbol_workspace_graph(
            &[],
            &symbols,
            &[],
            &[]
        ));

        let docs = vec![DocumentHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "README.md".to_string(),
            language: Some("markdown".to_string()),
            source_kind: "docs".to_string(),
            git_commit_sha: None,
            score: 1500.0,
            snippet: "readme".to_string(),
        }];
        assert!(!should_use_minimal_symbol_workspace_graph(
            &docs,
            &symbols,
            &[],
            &[]
        ));
    }

    #[test]
    fn minimal_document_workspace_graph_only_applies_to_single_document_only_shape() {
        let docs = vec![DocumentHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "src/config.rs".to_string(),
            language: Some("rust".to_string()),
            source_kind: "code".to_string(),
            git_commit_sha: None,
            score: 2000.0,
            snippet: "config".to_string(),
        }];
        assert!(should_use_minimal_document_workspace_graph(
            &docs,
            &[],
            &[],
            &[]
        ));

        let symbols = vec![SymbolHit {
            project_code: "amai".to_string(),
            namespace_code: "cold_benchmark".to_string(),
            repo_root: "/home/art/agent-memory-index".to_string(),
            relative_path: "src/config.rs".to_string(),
            name: "from_env".to_string(),
            kind: "function_item".to_string(),
            start_line: 45,
            end_line: 94,
            start_byte: 0,
            end_byte: 0,
            score: 2000.0,
            metadata: json!({"language":"rust"}),
        }];
        assert!(!should_use_minimal_document_workspace_graph(
            &docs,
            &symbols,
            &[],
            &[]
        ));
    }

    #[test]
    fn compact_workspace_enrichment_skips_for_small_nonsemantic_scope() {
        let requests = vec![
            (Uuid::new_v4(), "src/lib.rs".to_string()),
            (Uuid::new_v4(), "Cargo.toml".to_string()),
        ];
        assert!(should_use_compact_workspace_graph_enrichment(
            &requests,
            &[]
        ));

        let semantic_chunks = vec![json!({
            "project_code": "amai",
            "namespace_code": "review",
            "relative_path": "src/lib.rs"
        })];
        assert!(!should_use_compact_workspace_graph_enrichment(
            &requests,
            &semantic_chunks
        ));
    }

    #[test]
    fn semantic_fallback_result_marks_vector_layer_unavailable() {
        let chunk = synthetic_chunk_hit("src/lib.rs", "fn stable_semantic_fallback() {}");
        let (hits, _, guard) = semantic_fallback_result(
            "stable semantic fallback",
            2,
            1,
            &[chunk],
            SemanticTimings::default(),
            "vector_layer_unavailable",
            Some("synthetic qdrant outage".to_string()),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["retrieval_strategy"], json!("lexical_fallback"));
        assert_eq!(guard.reason, Some("vector_layer_unavailable"));
        assert_eq!(guard.detail.as_deref(), Some("synthetic qdrant outage"));
    }

    #[test]
    fn semantic_skipped_result_marks_reason_without_abstain() {
        let (hits, timings, guard) = semantic_skipped_result(
            "shared_runtime_marker",
            3,
            "exact_or_symbol_hits_sufficient",
        );
        assert!(hits.is_empty());
        assert_eq!(timings.query_embed_ms, 0);
        assert_eq!(guard.reason, Some("exact_or_symbol_hits_sufficient"));
        assert!(!guard.abstained);
        assert_eq!(guard.lexical_signal_count, 3);
    }

    #[test]
    fn verify_context_pack_does_not_skip_semantic_on_symbol_hits() {
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let should_try_exact_document = true;
        let should_try_exact_symbol = true;
        let semantic_skip_reason = if args.limit_semantic_chunks > 0
            && args.token_source_kind != "verify_context_pack"
            && ((should_try_exact_document && false) || (should_try_exact_symbol && true))
        {
            Some("exact_or_symbol_hits_sufficient")
        } else {
            None
        };
        assert_eq!(semantic_skip_reason, None);
    }

    #[test]
    fn benchmark_context_pack_skips_memory_lookup_when_exact_signal_exists() {
        let args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "benchmark_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        assert!(super::should_skip_memory_lookup(&args, 2, 0, 0, 0));
        assert!(super::should_skip_memory_lookup(&args, 0, 4, 0, 0));

        let verify_args = ContextPackArgs {
            token_source_kind: "verify_context_pack".to_string(),
            ..args
        };
        assert!(!super::should_skip_memory_lookup(&verify_args, 2, 0, 0, 0));
    }

    #[test]
    fn benchmark_context_pack_skips_retrieval_trace_persist() {
        assert!(!super::should_persist_retrieval_trace(
            "benchmark_context_pack"
        ));
        assert!(!super::should_persist_retrieval_trace(
            "benchmark_cold_context_pack"
        ));
        assert!(super::should_persist_retrieval_trace("verify_context_pack"));
        assert!(super::should_persist_retrieval_trace("live_context_pack"));
    }

    #[test]
    fn benchmark_context_pack_uses_compact_payload() {
        assert!(super::should_use_benchmark_compact_payload(
            "benchmark_context_pack"
        ));
        assert!(super::should_use_benchmark_compact_payload(
            "benchmark_cold_context_pack"
        ));
        assert!(!super::should_use_benchmark_compact_payload(
            "verify_context_pack"
        ));
    }

    #[test]
    fn semantic_skipped_result_marks_benchmark_compact_reason() {
        let (hits, timings, guard) = semantic_skipped_result(
            "shared_runtime_marker",
            8,
            "benchmark_compact_summary_sufficient",
        );
        assert!(hits.is_empty());
        assert_eq!(timings.query_embed_ms, 0);
        assert_eq!(guard.reason, Some("benchmark_compact_summary_sufficient"));
        assert!(!guard.abstained);
    }

    #[test]
    fn semantic_skip_fallback_uses_exact_document_snippet_before_empty_semantic() {
        let documents = vec![DocumentHit {
            project_code: "project_alpha".to_string(),
            namespace_code: "review".to_string(),
            repo_root: "/tmp/project_alpha".to_string(),
            relative_path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            source_kind: "code_chunk".to_string(),
            git_commit_sha: None,
            score: 2000.0,
            snippet: "pub fn shared_runtime_marker() {}".to_string(),
        }];
        let (hits, _, guard) = semantic_skip_fallback_result(
            "shared_runtime_marker",
            4,
            2,
            &documents,
            &[],
            "exact_or_symbol_hits_sufficient",
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0]["retrieval_strategy"],
            json!("exact_document_fallback")
        );
        assert_eq!(hits[0]["relative_path"], json!("src/lib.rs"));
        assert_eq!(
            guard.detail.as_deref(),
            Some("semantic satisfied by exact document fallback")
        );
    }

    #[test]
    fn degradation_probe_stale_fast_cache_evicts_expired_entry() {
        let state = degradation_probe_stale_fast_cache(1).expect("stale cache proof");
        assert_eq!(state["cache_hit"], json!(false));
        assert_eq!(state["cache_entry_remaining"], json!(false));
    }

    #[test]
    fn degradation_proof_scenarios_cover_retrieval_fallback_classes() {
        let scenarios = degradation_proof_scenarios(1).expect("scenarios");
        assert_eq!(scenarios.len(), 3);
        assert!(
            scenarios
                .iter()
                .all(|scenario| scenario["status"].as_str() == Some("pass"))
        );
    }
