    #[test]
    fn whole_cycle_observed_overrides_are_added_to_payload() {
        let payload = json!({
            "query": "token report"
        });
        let args = ContextPackArgs {
            project: "art".to_string(),
            namespace: "default".to_string(),
            query: "token report".to_string(),
            retrieval_mode: None,
            disable_cache: false,
            limit_documents: 5,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            token_source_kind: "live_context_pack".to_string(),
            client_prompt_tokens: Some(42),
            assistant_generation_tokens: Some(24),
            tool_overhead_tokens: Some(7),
            continuity_restore_tokens: Some(3),
        };

        let updated = with_whole_cycle_observed_overrides(&payload, &args);

        assert_eq!(updated["whole_cycle_observed"]["client_prompt_tokens"], 42);
        assert_eq!(
            updated["whole_cycle_observed"]["assistant_generation_tokens"],
            24
        );
        assert_eq!(updated["whole_cycle_observed"]["tool_overhead_tokens"], 7);
        assert_eq!(
            updated["whole_cycle_observed"]["continuity_restore_tokens"],
            3
        );
    }

    #[test]
    fn fast_cache_hit_works_without_explicit_retrieval_mode() {
        let cfg = test_config();
        let args = ContextPackArgs {
            project: "art".to_string(),
            namespace: "default".to_string(),
            query: "./src/mcp.rs".to_string(),
            retrieval_mode: None,
            disable_cache: false,
            limit_documents: 5,
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
        let payload = json!({
            "context_pack_id": Uuid::new_v4().to_string(),
            "project": {"code": "art"},
            "namespace": {"code": "default"},
            "retrieval": {
                "exact_documents": [{"relative_path": "src/mcp.rs"}],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        });
        let entry = LocalContextPackEntry {
            context_pack_id: Uuid::parse_str(
                payload["context_pack_id"]
                    .as_str()
                    .expect("context pack id"),
            )
            .expect("uuid"),
            payload: Arc::new(payload),
            exact_documents: 1,
            symbol_hits: 0,
            lexical_chunks: 0,
            semantic_chunks: 0,
            durably_persisted: true,
            cached_at_epoch_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis(),
        };
        local_fast_context_pack_cache_put(selected_fast_cache_key(&cfg, &args), &entry)
            .expect("store fast cache");

        let cached = try_execute_context_pack_fast_cached(&cfg, &args, false)
            .expect("fast cache lookup")
            .expect("fast cache hit");

        assert!(cached.stats.cache_hit);
        assert_eq!(cached.stats.timings.resolve_scope_ms, 0);
        assert_eq!(cached.stats.timings.cache_lookup_ms, 0);
        assert_eq!(
            cached.payload["retrieval"]["exact_documents"][0]["relative_path"],
            json!("src/mcp.rs")
        );
    }
    #[test]
    fn normalized_safe_relative_path_accepts_clean_relative_paths() {
        assert_eq!(
            normalized_safe_relative_path("./src/config.rs"),
            Some(PathBuf::from("src/config.rs"))
        );
        assert_eq!(
            normalized_safe_relative_path(".amai/onboarding/project-chat-startup-contract.json"),
            Some(PathBuf::from(
                ".amai/onboarding/project-chat-startup-contract.json"
            ))
        );
    }

    #[test]
    fn temporal_queries_use_distinct_cache_keys() {
        let cfg = test_config();
        let base = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "temporal fact".to_string(),
            retrieval_mode: None,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: Some(1_000),
            token_source_kind: "cli".to_string(),
            disable_cache: false,
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let other = ContextPackArgs {
            at_epoch_ms: Some(2_000),
            ..base.clone()
        };

        assert_ne!(
            cache_key(&cfg, &base, "local_strict"),
            cache_key(&cfg, &other, "local_strict")
        );
        assert_ne!(
            fast_cache_key(&cfg, &base, "local_strict"),
            fast_cache_key(&cfg, &other, "local_strict")
        );
    }

    #[test]
    fn normalized_safe_relative_path_rejects_escape_or_absolute_paths() {
        assert!(normalized_safe_relative_path("../secrets.txt").is_none());
        assert!(normalized_safe_relative_path("/etc/passwd").is_none());
    }

    #[test]
    fn model_visible_context_pack_payload_keeps_required_fields_and_drops_heavy_runtime_details() {
        let payload = json!({
            "context_pack_id": "ctx-1",
            "project": {
                "code": "art",
                "display_name": "Art",
                "repo_root": "/home/art/Art"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "included": [{"strategy": "exact_documents", "count": 1}],
                "not_included": []
            },
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": ".amai-continuity/live-handoff.md",
                    "snippet": "handoff snippet",
                    "source_kind": "docs",
                    "score": 2000.0,
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art",
                        "commit_sha": "abc"
                    }
                }],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "build_context_pack",
                    "kind": "function_item",
                    "metadata": {"language": "rust"},
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/guide.md",
                    "content": "lexical excerpt",
                    "chunk": {"chunk_id": "c1"},
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/guide.md",
                    "content": "semantic excerpt",
                    "retrieval_strategy": "semantic",
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }]
            },
            "workspace_graph": {"heavy": true},
            "retrieval_runtime": {"total_ms": 88}
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(compact["context_pack_id"].as_str(), Some("ctx-1"));
        assert_eq!(compact["project"]["code"].as_str(), Some("art"));
        assert_eq!(compact["namespace"]["code"].as_str(), Some("continuity"));
        assert!(compact["project"].get("display_name").is_none());
        assert!(compact["namespace"].get("display_name").is_none());
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["relative_path"].as_str(),
            Some(".amai-continuity/live-handoff.md")
        );
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["snippet"].as_str(),
            Some("handoff snippet")
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["content"].as_str(),
            Some("lexical excerpt")
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"][0]["content"].as_str(),
            Some("semantic excerpt")
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["name"].as_str(),
            Some("build_context_pack")
        );
        assert!(
            compact["decision_trace"]["included"][0]
                .get("reason")
                .is_none()
        );
        assert!(
            compact["decision_trace"]["semantic_guard"]
                .get("detail")
                .is_none()
        );
        assert!(compact.get("workspace_graph").is_none());
        assert!(compact.get("retrieval_runtime").is_none());
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("score")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("chunk")
                .is_none()
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("metadata")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("provenance")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            serde_json::to_string(&compact).expect("compact json").len()
                < serde_json::to_string(&payload).expect("full json").len()
        );
    }

    #[test]
    fn cli_visible_context_pack_payload_keeps_decision_trace_for_proof_context_pack() {
        let payload = json!({
            "context_pack_id": "ctx-proof",
            "project": {"code": "art"},
            "namespace": {"code": "continuity"},
            "effective_retrieval_mode": "local_strict",
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            },
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "selection_priority": ["exact_documents", "symbol_hits", "lexical_chunks", "semantic_chunks"],
                "included": [],
                "not_included": [],
                "semantic_guard": {"abstained": true}
            }
        });

        let compact = super::cli_visible_context_pack_payload(&payload, "proof_context_pack");

        assert_eq!(
            compact["decision_trace"]["scope"]["effective_retrieval_mode"].as_str(),
            Some("local_strict")
        );
        assert_eq!(
            compact["decision_trace"]["selection_priority"]
                .as_array()
                .map(Vec::len),
            Some(4)
        );
    }

    #[test]
    fn model_visible_context_pack_payload_dedupes_duplicate_semantic_chunks_only() {
        let payload = json!({
            "context_pack_id": "ctx-dup",
            "project": {
                "code": "art",
                "display_name": "Art",
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "selection_priority": ["exact_documents", "lexical_chunks", "semantic_chunks"],
                "included": [{
                    "strategy": "lexical_chunks",
                    "count": 1,
                    "reason": "verbose reason"
                }],
                "not_included": [{
                    "strategy": "symbol_hits",
                    "reason": "verbose miss reason"
                }],
                "semantic_guard": {
                    "abstained": false,
                    "accepted_hits": 1,
                    "rejected_hits": 0,
                    "lexical_signal_count": 2,
                    "query_terms": ["continuity", "snapshot"],
                    "reason": "no_vector_points_in_scope",
                    "detail": "verbose detail"
                }
            },
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "snippet": "same excerpt",
                    "source_kind": "docs"
                }],
                "symbol_hits": [],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "same excerpt",
                    "provenance": {
                        "source_project": "art",
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "same excerpt",
                    "provenance": {
                        "source_project": "art",
                    }
                }]
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
    }

    #[test]
    fn model_visible_context_pack_payload_symbol_only_shape_stays_metadata_only() {
        let payload = json!({
            "context_pack_id": "ctx-symbol-only",
            "project": {
                "code": "art",
                "display_name": "Art",
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "symbol_only_navigation_runtime_checkpoint",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "included": [{
                    "strategy": "symbol_hits",
                    "count": 1,
                    "reason": "verbose symbol reason"
                }],
                "not_included": [],
                "semantic_guard": {
                    "abstained": true,
                    "detail": "verbose semantic abstain detail"
                }
            },
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "symbol_only_navigation_runtime_checkpoint",
                    "kind": "function_item",
                    "metadata": {
                        "language": "rust",
                        "visibility": "pub"
                    },
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["name"].as_str(),
            Some("symbol_only_navigation_runtime_checkpoint")
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("metadata")
                .is_none()
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("provenance")
                .is_none()
        );
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
    }

    #[test]
    fn model_visible_context_pack_payload_preserves_unique_exact_snippet_in_hybrid_shape() {
        let payload = json!({
            "context_pack_id": "ctx-hybrid-unique-exact",
            "project": {
                "code": "art",
                "display_name": "Art",
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_plus_related",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_plus_related"},
                "included": [{
                    "strategy": "exact_documents",
                    "count": 1,
                    "reason": "verbose exact reason"
                }, {
                    "strategy": "lexical_chunks",
                    "count": 1,
                    "reason": "verbose lexical reason"
                }],
                "not_included": [],
                "semantic_guard": {
                    "abstained": false,
                    "detail": "verbose semantic detail"
                }
            },
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "snippet": "unique exact note only",
                    "source_kind": "docs"
                }],
                "symbol_hits": [],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "covered chunk text without the exact-only note",
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/related.md",
                    "content": "related semantic evidence",
                    "provenance": {
                        "source_project": "art",
                        "repo_root": "/home/art/Art"
                    }
                }]
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["snippet"].as_str(),
            Some("unique exact note only")
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["content"].as_str(),
            Some("covered chunk text without the exact-only note")
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            compact["retrieval"]["semantic_chunks"][0]["relative_path"].as_str(),
            Some("docs/related.md")
        );
        assert!(compact.get("query").is_none());
        assert_eq!(
            compact["visible_projects"][0]["project_code"].as_str(),
            Some("art")
        );
        assert!(compact.get("decision_trace").is_none());
    }

    #[test]
    fn same_thread_cache_reuse_payload_drops_text_and_keeps_reference_map() {
        let payload = json!({
            "context_pack_id": "ctx-reuse",
            "project": {
                "code": "art",
                "display_name": "Art",
                "repo_root": "/home/art/Art"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "query": "Continuity snapshot",
            "effective_retrieval_mode": "local_strict",
            "visible_projects": [{
                "project_code": "art",
                "repo_root": "/home/art/Art"
            }],
            "decision_trace": {
                "scope": {"effective_retrieval_mode": "local_strict"},
                "included": [{"strategy": "exact_documents", "count": 1}],
                "not_included": []
            },
            "retrieval_runtime": {"cache_hit": true},
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "snippet": "handoff snippet",
                    "source_kind": "docs",
                    "provenance": {
                        "source_project": "art"
                    }
                }],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "build_context_pack",
                    "kind": "function_item",
                    "provenance": {
                        "source_project": "art"
                    }
                }],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/continuity.md",
                    "content": "full lexical text",
                    "provenance": {
                        "source_project": "art"
                    }
                }],
                "semantic_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/related.md",
                    "content": "full semantic text",
                    "provenance": {
                        "source_project": "art"
                    }
                }]
            }
        });

        let compact = build_same_thread_cache_reuse_payload(&payload, "thread-1");

        assert_eq!(
            compact["cache_reuse_reference"]["state"].as_str(),
            Some("same_thread_context_pack_replay")
        );
        assert_eq!(
            compact["cache_reuse_reference"]["source_context_pack_id"].as_str(),
            Some("ctx-reuse")
        );
        assert!(compact["cache_reuse_reference"].get("thread_id").is_none());
        assert!(compact["cache_reuse_reference"].get("note").is_none());
        assert_eq!(
            compact["cache_reuse_reference"]["active_files"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(compact["project"]["code"].as_str(), Some("art"));
        assert!(compact["project"].get("display_name").is_none());
        assert_eq!(compact["namespace"]["code"].as_str(), Some("continuity"));
        assert!(compact["namespace"].get("display_name").is_none());
        assert!(compact["decision_trace"].is_null());
        assert!(compact["workspace_graph"].is_null());
        assert!(compact["visible_projects"][0].get("display_name").is_none());
        assert!(
            compact["visible_projects"][0]
                .get("relation_type")
                .is_none()
        );
        assert_eq!(
            compact["retrieval_runtime"]["cache_hit"].as_bool(),
            Some(true)
        );
        assert!(
            compact["retrieval_runtime"]
                .get("resolve_scope_ms")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("snippet")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("content")
                .is_none()
        );
        assert!(
            compact["retrieval"]["semantic_chunks"][0]
                .get("content")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("project_code")
                .is_none()
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("provenance")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["name"].as_str(),
            Some("build_context_pack")
        );
    }

    #[test]
    fn model_visible_context_pack_payload_preserves_same_thread_cache_reuse_reference() {
        let payload = build_same_thread_cache_reuse_payload(
            &json!({
                "context_pack_id": "ctx-reuse-visible",
                "project": {
                    "code": "art",
                    "display_name": "Art",
                    "repo_root": "/home/art/Art"
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                },
                "query": "Continuity snapshot",
                "effective_retrieval_mode": "local_strict",
                "visible_projects": [{
                    "project_code": "art",
                    "repo_root": "/home/art/Art"
                }],
                "decision_trace": {
                    "scope": {"effective_retrieval_mode": "local_strict"},
                    "included": [{"strategy": "lexical_chunks", "count": 1, "reason": "verbose"}],
                    "not_included": []
                },
                "retrieval": {
                    "exact_documents": [],
                    "symbol_hits": [],
                    "lexical_chunks": [{
                        "project_code": "art",
                        "relative_path": "docs/continuity.md",
                        "content": "full lexical text",
                        "provenance": {
                            "source_project": "art"
                        }
                    }],
                    "semantic_chunks": []
                }
            }),
            "thread-2",
        );

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["cache_reuse_reference"]["state"].as_str(),
            Some("same_thread_context_pack_replay")
        );
        assert_eq!(
            compact["cache_reuse_reference"]["source_context_pack_id"].as_str(),
            Some("ctx-reuse-visible")
        );
        assert_eq!(compact["project"]["code"].as_str(), Some("art"));
        assert_eq!(compact["namespace"]["code"].as_str(), Some("continuity"));
        assert!(compact.get("retrieval").is_none());
        assert!(compact.get("decision_trace").is_none());
        assert!(compact.get("query").is_none());
    }

    #[test]
    fn proof_context_pack_disables_same_thread_cache_reuse_compaction() {
        let stats = ContextPackStats {
            context_pack_id: Uuid::nil(),
            exact_documents: 1,
            symbol_hits: 0,
            lexical_chunks: 0,
            semantic_chunks: 0,
            cache_hit: true,
            scope_signature: "local_fast_cache".to_string(),
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
            retrieval_lower_bound_ms_precise: Some(0.0),
        };
        let proof_args = ContextPackArgs {
            project: "project_alpha".to_string(),
            namespace: "review".to_string(),
            query: "shared_runtime_marker".to_string(),
            retrieval_mode: Some("local_plus_related".to_string()),
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            at_epoch_ms: None,
            disable_cache: false,
            token_source_kind: "proof_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let default_args = ContextPackArgs {
            token_source_kind: "live_context_pack".to_string(),
            ..proof_args.clone()
        };
        let verify_args = ContextPackArgs {
            token_source_kind: "verify_context_pack".to_string(),
            ..proof_args.clone()
        };

        assert!(!should_emit_same_thread_cache_reuse_payload(
            &stats,
            &proof_args
        ));
        assert!(should_bypass_fast_context_pack_cache(&proof_args));
        assert!(should_bypass_fast_context_pack_cache(&verify_args));
        assert!(should_emit_same_thread_cache_reuse_payload(
            &stats,
            &default_args
        ));
        assert!(!should_bypass_fast_context_pack_cache(&default_args));
    }

    #[test]
    fn model_visible_context_pack_payload_keeps_cross_project_reference_only_when_needed() {
        let payload = json!({
            "context_pack_id": "ctx-cross-project",
            "project": {
                "code": "amai",
            },
            "namespace": {
                "code": "continuity",
            },
            "effective_retrieval_mode": "local_plus_related",
            "retrieval": {
                "exact_documents": [{
                    "project_code": "bug_bounty",
                    "relative_path": "docs/report.md",
                    "snippet": "cross project snippet",
                    "source_kind": "docs",
                    "provenance": {
                        "source_project": "bug_bounty"
                    }
                }],
                "symbol_hits": [{
                    "project_code": "bug_bounty",
                    "relative_path": "src/lib.rs",
                    "name": "run_report",
                    "kind": "function_item",
                    "provenance": {
                        "source_project": "bug_bounty"
                    }
                }],
                "lexical_chunks": [{
                    "project_code": "bug_bounty",
                    "relative_path": "docs/report.md",
                    "content": "cross project lexical",
                    "provenance": {
                        "source_project": "bug_bounty"
                    }
                }],
                "semantic_chunks": []
            }
        });

        let compact = model_visible_context_pack_payload(&payload);

        assert_eq!(
            compact["retrieval"]["exact_documents"][0]["project_code"].as_str(),
            Some("bug_bounty")
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("provenance")
                .is_none()
        );
        assert!(
            compact["retrieval"]["exact_documents"][0]
                .get("source_kind")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["symbol_hits"][0]["project_code"].as_str(),
            Some("bug_bounty")
        );
        assert!(
            compact["retrieval"]["symbol_hits"][0]
                .get("provenance")
                .is_none()
        );
        assert_eq!(
            compact["retrieval"]["lexical_chunks"][0]["project_code"].as_str(),
            Some("bug_bounty")
        );
        assert!(
            compact["retrieval"]["lexical_chunks"][0]
                .get("provenance")
                .is_none()
        );
    }
