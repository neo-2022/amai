    use super::{
        COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS, ObserveCache, ObserveState,
        benchmark_contamination_value, build_continuity_correctness_model, build_degradation_model,
        cache_snapshot_age_ms, cached_client_live_meter_state, client_live_meter_refresh_needed,
        compact_client_budget_snapshot_cache_too_old, dashboard_live_summary_api_handler,
        dashboard_page_handler, evaluate_sla, expired_retention_candidates, load_profile,
        merge_thread_bound_client_budget_snapshot_into_base_snapshot, normalized_thread_id_hint,
        profile_thresholds_json, render_prometheus_metrics, select_latest_clean_benchmark_snapshot,
    };
    use crate::codex_threads::{
        RecentClientThreadRecord, RolloutClientMeterObservation, RolloutJsonlToleranceSummary,
    };
    use crate::config::AppConfig;
    use crate::postgres::ObservabilityRetentionCandidate;
    use crate::working_state;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use reqwest::Client;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn test_config() -> AppConfig {
        AppConfig {
            stack_name: "amai".to_string(),
            pg_db: "amai".to_string(),
            app_db_user: "amai".to_string(),
            app_db_password: "amai".to_string(),
            postgres_dsn: "postgres://127.0.0.1:1/unused".to_string(),
            app_postgres_dsn: "postgres://127.0.0.1:1/unused".to_string(),
            qdrant_url: "http://127.0.0.1:1".to_string(),
            qdrant_http_url: "http://127.0.0.1:1".to_string(),
            qdrant_collection_code: "test".to_string(),
            benchmark_qdrant_http_url: None,
            benchmark_qdrant_collection_code: None,
            qdrant_alias_code: "test".to_string(),
            qdrant_collection_memory: "memory".to_string(),
            qdrant_alias_memory: "memory".to_string(),
            qdrant_code_dim: 384,
            qdrant_memory_dim: 384,
            qdrant_distance: "Cosine".to_string(),
            s3_endpoint: "http://127.0.0.1:1".to_string(),
            s3_region: "us-east-1".to_string(),
            s3_access_key: "test".to_string(),
            s3_secret_key: "test".to_string(),
            s3_bucket_artifacts: "artifacts".to_string(),
            s3_bucket_transcripts: "transcripts".to_string(),
            s3_bucket_context: "context".to_string(),
            nats_url: "nats://127.0.0.1:1".to_string(),
            nats_http_url: "http://127.0.0.1:1".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            edge_cache_path: "/tmp/observe-test-edge-cache.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            local_fast_cache_ttl_ms: 5_000,
        }
    }

    #[tokio::test]
    async fn collect_qdrant_live_falls_back_to_single_discovered_collection() {
        async fn metrics_handler() -> &'static str {
            "# HELP collections_total number of collections\n# TYPE collections_total gauge\ncollections_total 1\n# HELP collections_vector_total total number of vectors in all collections\n# TYPE collections_vector_total gauge\ncollections_vector_total 860484\n# HELP memory_resident_bytes resident memory\n# TYPE memory_resident_bytes gauge\nmemory_resident_bytes 12345\n"
        }

        async fn collections_handler() -> axum::Json<Value> {
            axum::Json(json!({
                "result": {
                    "collections": [
                        {"name": "ann_benchmarks_test"}
                    ]
                },
                "status": "ok"
            }))
        }

        async fn missing_collection_handler() -> (StatusCode, axum::Json<Value>) {
            (
                StatusCode::NOT_FOUND,
                axum::Json(json!({
                    "status": "error",
                    "result": null
                })),
            )
        }

        async fn discovered_collection_handler() -> axum::Json<Value> {
            axum::Json(json!({
                "result": {
                    "optimizer_status": "ok",
                    "indexed_vectors_count": 0,
                    "points_count": 859520,
                    "segments_count": 30
                },
                "status": "ok"
            }))
        }

        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/collections", get(collections_handler))
            .route(
                "/collections/QdrantLocalCollection",
                get(missing_collection_handler),
            )
            .route(
                "/collections/ann_benchmarks_test",
                get(discovered_collection_handler),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let http = Client::new();
        let live = super::collect_qdrant_live_from(
            &format!("http://{}", addr),
            "QdrantLocalCollection",
            &http,
        )
        .await
        .unwrap();

        assert_eq!(
            live["effective_collection_code"].as_str(),
            Some("ann_benchmarks_test")
        );
        assert_eq!(live["points_count"].as_u64(), Some(859520));
        assert_eq!(live["segments_count"].as_u64(), Some(30));
        assert_eq!(live["collections_vector_total"].as_f64(), Some(860484.0));
        server.abort();
    }

    #[test]
    fn merge_keeps_richer_base_live_response_latency_when_thread_surface_is_empty() {
        let base_snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "live_response_latency": {
                        "current_session": {
                            "sample_count": 91,
                            "latency_slices": [
                                {"state": "hot", "sample_count": 47},
                                {"state": "cold", "sample_count": 44}
                            ]
                        },
                        "rolling_window": {
                            "sample_count": 92,
                            "latency_slices": [
                                {"state": "hot", "sample_count": 48},
                                {"state": "cold", "sample_count": 44}
                            ]
                        }
                    }
                }
            }
        });
        let thread_bound_snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "live_response_latency": {
                        "current_session": {
                            "sample_count": 0,
                            "latency_slices": []
                        },
                        "rolling_window": {
                            "sample_count": 0,
                            "latency_slices": []
                        }
                    }
                }
            }
        });

        let merged = merge_thread_bound_client_budget_snapshot_into_base_snapshot(
            &base_snapshot,
            &thread_bound_snapshot,
        );

        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["live_response_latency"]
                ["current_session"]["sample_count"]
                .as_u64(),
            Some(91)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["live_response_latency"]
                ["rolling_window"]["sample_count"]
                .as_u64(),
            Some(92)
        );
    }

    fn test_observe_state(cache: ObserveCache) -> ObserveState {
        ObserveState {
            dashboard_refresh_ms: 1000,
            cfg: test_config(),
            bind: "127.0.0.1:9464".to_string(),
            cache: Arc::new(RwLock::new(cache)),
        }
    }

    #[test]
    fn host_current_thread_control_window_targeting_allows_single_visible_target_thread() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let threads = vec![RecentClientThreadRecord {
            thread_id: "thread-amai".to_string(),
            cwd: "/home/art/agent-memory-index".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            title: "Amai".to_string(),
            agent_nickname: None,
            agent_role: None,
            model_provider: Some("openai".to_string()),
            model: Some("gpt-5.4".to_string()),
            reasoning_effort: Some("medium".to_string()),
            updated_at_epoch_s: 1_775_118_000,
        }];

        let summary = super::evaluate_host_current_thread_control_window_targeting(
            repo_root,
            "thread-amai",
            &threads,
        );

        assert_eq!(summary["status"], json!("allowed"));
        assert_eq!(summary["allowed"], json!(true));
        assert_eq!(summary["visible_model_bound_thread_count"], json!(1));
        assert_eq!(summary["target_thread_visible"], json!(true));
        assert_eq!(summary["target_thread_repo_root_match"], json!(true));
        assert!(summary["denial_reason"].is_null());
    }

    #[tokio::test]
    async fn dashboard_page_route_keeps_live_summary_contract_without_cached_bootstrap_payload() {
        let payload = json!({
            "meta": {
                "package_version": "0.1.0"
            },
            "headline": {
                "status": "pass",
                "status_label": "ok",
                "status_reason": "cached payload",
                "token_value": "5ч KPI: 1:1",
                "token_scope": ""
            },
            "hero_cards": [],
            "top_cards": [],
            "benchmark_cards": [],
            "machine_cards": [],
            "service_cards": [],
            "warnings": [],
            "glossary": [],
            "links": []
        });
        let mut cache = ObserveCache::default();
        cache.dashboard_payload = Some(payload);
        let app = Router::new()
            .route("/", get(dashboard_page_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let html = String::from_utf8(body.to_vec()).expect("html");
        assert!(html.contains("/api/dashboard-live-summary"));
        assert!(html.contains("syncDashboardLiveSummary"));
        assert!(!html.contains("/api/active-agent-budget-live"));
        assert!(!html.contains("syncActiveAgentBudgetLiveCard"));
        assert!(!html.contains("fetchActiveAgentBudgetLivePayload"));
        assert!(html.contains("const DASHBOARD_BOOTSTRAP_PAYLOAD = null;"));
        assert!(!html.contains("cached payload"));
    }

    #[tokio::test]
    async fn dashboard_live_summary_route_is_mounted_and_not_404() {
        let app = Router::new()
            .route(
                "/api/dashboard-live-summary",
                get(dashboard_live_summary_api_handler),
            )
            .with_state(test_observe_state(ObserveCache::default()));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard-live-summary")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let text = String::from_utf8(body.to_vec()).expect("body text");
        assert!(text.contains("\"status\":\"down\""));
    }

    #[tokio::test]
    async fn client_budget_live_route_returns_warmup_envelope_while_refresh_in_progress() {
        let mut cache = ObserveCache::default();
        cache.refresh_in_progress = true;

        let app = Router::new()
            .route(
                "/api/client-budget-live",
                get(super::client_budget_live_api_handler),
            )
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/client-budget-live")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("warmup payload");
        assert_eq!(payload["status"], json!("warming_up"));
        assert_eq!(payload["warmup_pending"], json!(true));
        assert_eq!(payload["rows"].as_array().map(Vec::len), Some(0));
    }

    #[tokio::test]
    async fn dashboard_live_summary_route_returns_warmup_envelope_while_refresh_in_progress() {
        let mut cache = ObserveCache::default();
        cache.refresh_in_progress = true;

        let app = Router::new()
            .route(
                "/api/dashboard-live-summary",
                get(super::dashboard_live_summary_api_handler),
            )
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard-live-summary")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("warmup payload");
        assert_eq!(payload["headline"]["status"], json!("waiting"));
        assert_eq!(payload["warmup_pending"], json!(true));
        assert_eq!(payload["top_cards"], json!([]));
    }

    #[tokio::test]
    async fn dashboard_route_returns_warmup_envelope_while_refresh_in_progress() {
        let mut cache = ObserveCache::default();
        cache.refresh_in_progress = true;

        let app = Router::new()
            .route("/api/dashboard", get(super::dashboard_api_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("warmup payload");
        assert_eq!(payload["headline"]["status"], json!("waiting"));
        assert_eq!(payload["service_cards"], json!([]));
        assert_eq!(
            payload["warnings"][0],
            json!("Панель ещё прогревается: первый live snapshot не готов.")
        );
    }

    #[tokio::test]
    async fn healthz_route_returns_up_envelope_for_ready_cache() {
        let mut cache = ObserveCache::default();
        cache.snapshot = Some(json!({
            "sla": {
                "summary": {
                    "critical": 0,
                    "unknown": 0
                }
            }
        }));
        cache.last_refresh_completed_epoch_ms = Some(super::now_epoch_ms());

        let app = Router::new()
            .route("/healthz", get(super::healthz_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("health payload");
        assert_eq!(payload["status"], json!("up"));
        assert_eq!(payload["critical"], json!(0));
        assert_eq!(payload["unknown"], json!(0));
        assert_eq!(payload["cache_stale"], json!(false));
    }

    #[tokio::test]
    async fn healthz_route_returns_down_envelope_when_cache_is_stale() {
        let mut cache = ObserveCache::default();
        cache.snapshot = Some(json!({
            "sla": {
                "summary": {
                    "critical": 0,
                    "unknown": 0
                }
            }
        }));

        let app = Router::new()
            .route("/healthz", get(super::healthz_handler))
            .with_state(test_observe_state(cache));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("health payload");
        assert_eq!(payload["status"], json!("down"));
        assert_eq!(payload["cache_stale"], json!(true));
    }

    #[tokio::test]
    async fn agent_display_name_route_rejects_empty_scope_before_db_lookup() {
        let app = Router::new()
            .route(
                "/api/agent-display-name",
                post(super::agent_display_name_update_api_handler),
            )
            .with_state(test_observe_state(ObserveCache::default()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agent-display-name")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"agent_scope":"   ","display_name":"Новый агент"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let text = String::from_utf8(body.to_vec()).expect("body text");
        assert!(text.contains("agent_scope is required"));
    }

    #[test]
    fn legacy_active_agent_payload_matches_live_summary_bundle_on_same_snapshot() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "observe_refresh": {
                "total_ms": 321u64,
                "stage_ms": {
                    "active_agent_budget": 44u64
                }
            },
            "sla": {
                "summary": {
                    "pass": 19,
                    "alert": 0,
                    "critical": 0,
                    "unknown": 0
                }
            },
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001,
                        "switch_to_microseconds_below_ms": 1.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "0 ns",
                        "seconds_suffix": "s",
                        "milliseconds_suffix": "ms",
                        "microseconds_suffix": "µs",
                        "nanoseconds_suffix": "ns",
                        "seconds_decimals": 3,
                        "milliseconds_decimals": 3,
                        "microseconds_decimals": 3,
                        "nanoseconds_decimals": 0
                    }
                },
                "retrieval": {
                    "hot_live_table": {
                        "target_p50_ms": 1.0,
                        "target_p95_ms": 1.0,
                        "target_p99_ms": 2.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 12.0,
                        "target_p99_ms": 13.0,
                        "target_max_ms": 15.0,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "headline": {
                        "title": "global fallback",
                        "value_percent": 12.0,
                        "scope_label": "fallback"
                    },
                    "current_session": {
                        "latency_slices": []
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": null
            },
            "agent_scope_activity": {
                "client_recent_window_minutes": 30,
                "client_recent_thread_count": 2,
                "client_recent_threads": [],
                "active_now_count": 2,
                "active_now_scopes": [],
                "recent_scope_window_hours": 24,
                "recent_scope_count": 2,
                "recent_scopes": []
            },
            "active_agent_budget": {
                "captured_at_epoch_ms": 1774239286000u64,
                "source": "live_active_agent_budget_surface",
                "headline": {
                    "title": "Средний KPI активных агентов",
                    "value_text": "5ч KPI: экономия 40.00%",
                    "scope_label": "среднее по 2 активным агентам"
                },
                "aggregate": {
                    "status": "observed",
                    "classification": "saving",
                    "reply_prefix": "5ч KPI: экономия 40.00%"
                },
                "agents": [
                    {
                        "agent_label": "Amai",
                        "agent_scope": "amai::continuity::default",
                        "thread_title": "Amai dashboard",
                        "cwd": "/home/art/agent-memory-index",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 60.00%",
                            "summary": "agent one"
                        },
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 43.00%, 7д остаётся 72.00%",
                            "tooltip": "personal limit one"
                        }
                    },
                    {
                        "agent_label": "Hunter",
                        "agent_scope": "bug_bounty::continuity::default",
                        "thread_title": "Bug bounty",
                        "cwd": "/home/art/Bug-Bounty",
                        "personal_agent_kpi": {
                            "reply_prefix": "5ч KPI: экономия 20.00%",
                            "summary": "agent two"
                        },
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 88.00%, 7д остаётся 91.00%",
                            "tooltip": "personal limit two"
                        }
                    }
                ]
            }
        });

        let legacy_payload = super::active_agent_budget_card_payload_from_snapshot(&snapshot)
            .expect("legacy active agent payload");
        let live_summary = crate::dashboard::build_live_summary_payload(
            &test_config(),
            &snapshot,
            "127.0.0.1:9464",
            1000,
        )
        .expect("live summary payload");

        assert_eq!(
            legacy_payload["card"]["value"].as_str(),
            live_summary["active_agent_card"]["value"].as_str()
        );
        assert_eq!(
            legacy_payload["card"]["agent_blocks"],
            live_summary["active_agent_card"]["agent_blocks"]
        );
    }

    #[test]
    fn host_current_thread_control_window_targeting_denies_ambiguous_multi_window_state() {
        let repo_root = Path::new("/home/art/agent-memory-index");
        let threads = vec![
            RecentClientThreadRecord {
                thread_id: "thread-amai".to_string(),
                cwd: "/home/art/agent-memory-index".to_string(),
                rollout_path: "/tmp/rollout-amai.jsonl".to_string(),
                title: "Amai".to_string(),
                agent_nickname: None,
                agent_role: None,
                model_provider: Some("openai".to_string()),
                model: Some("gpt-5.4".to_string()),
                reasoning_effort: Some("medium".to_string()),
                updated_at_epoch_s: 1_775_118_000,
            },
            RecentClientThreadRecord {
                thread_id: "thread-bounty".to_string(),
                cwd: "/home/art/Bug-Bounty".to_string(),
                rollout_path: "/tmp/rollout-bounty.jsonl".to_string(),
                title: "Bug Bounty".to_string(),
                agent_nickname: None,
                agent_role: None,
                model_provider: Some("openai".to_string()),
                model: Some("gpt-5.4".to_string()),
                reasoning_effort: Some("medium".to_string()),
                updated_at_epoch_s: 1_775_118_001,
            },
        ];

        let summary = super::evaluate_host_current_thread_control_window_targeting(
            repo_root,
            "thread-amai",
            &threads,
        );

        assert_eq!(summary["status"], json!("denied"));
        assert_eq!(summary["allowed"], json!(false));
        assert_eq!(summary["visible_model_bound_thread_count"], json!(2));
        assert_eq!(
            summary["denial_reason"],
            json!("ambiguous_multi_window_recent_threads")
        );
    }

    #[test]
    fn prometheus_token_rollups_export_verified_values_by_default() {
        let snapshot = json!({
            "latest_token_benchmark": {
                "token_benchmark": {
                    "naive_scope": { "token_count": 100.0 },
                    "context_pack": { "token_count": 20.0 },
                    "savings": {
                        "saved_tokens": 80.0,
                        "savings_factor": 5.0,
                        "savings_percent": 80.0
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "verified_effective_saved_tokens": 60.0,
                        "verified_effective_savings_pct": 75.0,
                        "total_saved_tokens": 90.0,
                        "savings_percent": 90.0,
                        "quality_ok_rate": 70.0,
                        "fallback_rate": 20.0,
                        "answer_like_rate": 50.0
                    },
                    "rolling_window": {
                        "verified_effective_saved_tokens": 120.0,
                        "verified_effective_savings_pct": 66.0,
                        "total_saved_tokens": 180.0,
                        "savings_percent": 88.0,
                        "quality_ok_rate": 80.0,
                        "fallback_rate": 10.0,
                        "answer_like_rate": 40.0
                    },
                    "lifetime": {
                        "verified_effective_saved_tokens": 240.0,
                        "verified_effective_savings_pct": 55.0,
                        "total_saved_tokens": 300.0,
                        "savings_percent": 77.0,
                        "quality_ok_rate": 90.0,
                        "fallback_rate": 5.0,
                        "answer_like_rate": 35.0
                    }
                }
            },
            "sla": {
                "summary": {
                    "pass": 1.0,
                    "alert": 0.0,
                    "critical": 0.0,
                    "unknown": 0.0
                }
            },
            "degradation_model": {
                "summary": {
                    "pass": 2.0,
                    "critical": 0.0,
                    "unknown": 9.0,
                    "fail_closed_total": 5.0,
                    "graceful_fallback_total": 6.0,
                    "evidence_gaps": 9.0
                }
            },
            "continuity_correctness_model": {
                "summary": {
                    "verified_probes": 9.0,
                    "failed_probes": 0.0,
                    "recovered_useful": 7.0,
                    "fail_closed": 2.0
                }
            }
        });

        let output = render_prometheus_metrics(&snapshot);
        assert!(output.contains("amai_tokens_saved_session_total 60"));
        assert!(output.contains("amai_tokens_savings_percent_window 66"));
        assert!(output.contains("amai_tokens_raw_saved_session_total 90"));
        assert!(output.contains("amai_tokens_quality_ok_rate_lifetime 90"));
        assert!(output.contains("amai_tokens_answer_like_rate_window 40"));
        assert!(output.contains("amai_degradation_unknown_total 9"));
        assert!(output.contains("amai_degradation_fail_closed_total 5"));
        assert!(output.contains("amai_degradation_graceful_fallback_total 6"));
        assert!(output.contains("amai_continuity_verified_probes_total 9"));
        assert!(output.contains("amai_continuity_recovered_useful_total 7"));
        assert!(output.contains("amai_continuity_fail_closed_total 2"));
    }

    #[test]
    fn merge_thread_bound_snapshot_preserves_full_dashboard_thresholds() {
        let base_snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.001
                    }
                }
            },
            "sla": {
                "summary": {
                    "critical": 0
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "headline": {
                        "title": "5ч KPI: экономия",
                        "value_percent": 42.0
                    },
                    "current_session": {
                        "savings_percent": 25.0
                    },
                    "rolling_window": {
                        "verified_effective_saved_tokens": 7154
                    },
                    "lifetime": {
                        "verified_effective_saved_tokens": 13844711
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "same_meter_equivalent"
                            }
                        },
                        "rolling_window": {
                            "scope_label": "окно"
                        },
                        "lifetime": {
                            "scope_label": "всё время"
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "current_goal": "full snapshot"
                }
            }
        });
        let thread_bound_snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "current_session": {
                        "savings_percent": 70.0
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "same_meter_equivalent"
                            }
                        }
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "current_goal": "thread bound snapshot"
                }
            }
        });

        let merged = merge_thread_bound_client_budget_snapshot_into_base_snapshot(
            &base_snapshot,
            &thread_bound_snapshot,
        );

        assert_eq!(
            merged["thresholds"]["dashboard"]["timing_format"]["switch_to_nanoseconds_below_ms"]
                .as_f64(),
            Some(0.001)
        );
        assert_eq!(merged["sla"]["summary"]["critical"].as_u64(), Some(0));
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["current_session"]
                ["savings_percent"]
                .as_f64(),
            Some(70.0)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["headline"]["value_percent"]
                .as_f64(),
            Some(42.0)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["rolling_window"]
                ["verified_effective_saved_tokens"]
                .as_i64(),
            Some(7154)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["lifetime"]
                ["verified_effective_saved_tokens"]
                .as_i64(),
            Some(13844711)
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["statement_previews"]
                ["rolling_window"]["scope_label"]
                .as_str(),
            Some("окно")
        );
        assert_eq!(
            merged["token_budget_report"]["token_budget_report"]["statement_previews"]["lifetime"]
                ["scope_label"]
                .as_str(),
            Some("всё время")
        );
        assert_eq!(
            merged["latest_repo_working_state_restore"]["working_state_restore"]["current_goal"]
                .as_str(),
            Some("thread bound snapshot")
        );
    }

    #[test]
    fn overlay_live_active_agent_surfaces_replaces_cached_agent_card_data() {
        let snapshot = json!({
            "agent_scope_activity": {
                "active_now_scopes": []
            },
            "active_agent_budget": {
                "agents": [
                    {
                        "agent_label": "Amai",
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 44.00%, 7д остаётся 23.00%"
                        }
                    }
                ]
            }
        });

        let merged = super::overlay_live_active_agent_surfaces(
            snapshot,
            json!({
                "active_now_scopes": [
                    { "owner_thread_id": "thread-amai" }
                ]
            }),
            json!({
                "agents": [
                    {
                        "agent_label": "Amai",
                        "personal_client_limit": {
                            "value_text": "5ч остаётся 42.00%, 7д остаётся 22.00%"
                        }
                    }
                ]
            }),
        );

        assert_eq!(
            merged["active_agent_budget"]["agents"][0]["personal_client_limit"]["value_text"]
                .as_str(),
            Some("5ч остаётся 42.00%, 7д остаётся 22.00%")
        );
        assert_eq!(
            merged["agent_scope_activity"]["active_now_scopes"][0]["owner_thread_id"].as_str(),
            Some("thread-amai")
        );
    }

    #[test]
    fn normalized_thread_id_hint_rejects_empty_values() {
        assert_eq!(normalized_thread_id_hint(None), None);
        assert_eq!(normalized_thread_id_hint(Some("")), None);
        assert_eq!(normalized_thread_id_hint(Some("   ")), None);
    }

    #[test]
    fn normalized_thread_id_hint_trims_value() {
        assert_eq!(
            normalized_thread_id_hint(Some("  thread-123  ")),
            Some("thread-123")
        );
    }

    #[test]
    fn strict_auto_thread_binding_hint_uses_unique_active_thread() {
        let snapshot = json!({
            "agent_scope_activity": {
                "client_recent_threads": [
                    { "thread_id": "thread-live" },
                    { "thread_id": "thread-other" }
                ],
                "active_now_scopes": [
                    { "owner_thread_id": "thread-live" },
                    { "owner_thread_id": "thread-live" }
                ]
            }
        });

        assert_eq!(
            super::strict_auto_thread_binding_hint_from_snapshot(snapshot).as_deref(),
            Some("thread-live")
        );
    }

    #[test]
    fn strict_auto_thread_binding_hint_rejects_ambiguous_active_threads() {
        let snapshot = json!({
            "agent_scope_activity": {
                "client_recent_threads": [
                    { "thread_id": "thread-a" },
                    { "thread_id": "thread-b" }
                ],
                "active_now_scopes": [
                    { "owner_thread_id": "thread-a" },
                    { "owner_thread_id": "thread-b" }
                ]
            }
        });

        assert_eq!(
            super::strict_auto_thread_binding_hint_from_snapshot(snapshot),
            None
        );
    }

    #[test]
    fn strict_auto_thread_binding_hint_requires_matching_raw_client_thread() {
        let snapshot = json!({
            "agent_scope_activity": {
                "client_recent_threads": [
                    { "thread_id": "thread-other" }
                ],
                "active_now_scopes": [
                    { "owner_thread_id": "thread-live" }
                ]
            }
        });

        assert_eq!(
            super::strict_auto_thread_binding_hint_from_snapshot(snapshot),
            None
        );
    }

    #[test]
    fn continuity_correctness_model_reports_passing_probe_counts() {
        let payload = json!({
            "latest_continuity_verification": {
                "continuity_verification": {
                    "captured_at_epoch_ms": 4242,
                    "verification_status": "pass",
                    "probe_count": 9,
                    "verified_probes": 9,
                    "failed_probes": [],
                    "canonical_eval": {
                        "verdict_counts": {
                            "recovered_useful": 7,
                            "hit_correct_target": 2
                        }
                    }
                }
            }
        });

        let model = build_continuity_correctness_model(&payload).expect("continuity model");
        assert_eq!(model["summary"]["status"], json!("pass"));
        assert_eq!(model["summary"]["probe_count"], json!(9));
        assert_eq!(model["summary"]["verified_probes"], json!(9));
        assert_eq!(model["summary"]["failed_probes"], json!(0));
        assert_eq!(model["summary"]["recovered_useful"], json!(7));
        assert_eq!(model["summary"]["fail_closed"], json!(2));
        assert_eq!(model["summary"]["evidence_gap"], json!(false));
    }

    #[test]
    fn continuity_correctness_model_reports_gap_without_snapshot() {
        let payload = json!({
            "latest_continuity_verification": null
        });

        let model = build_continuity_correctness_model(&payload).expect("continuity model");
        assert_eq!(model["summary"]["status"], json!("unknown"));
        assert_eq!(model["summary"]["probe_count"], json!(0));
        assert_eq!(model["summary"]["evidence_gap"], json!(true));
    }

    #[test]
    fn client_live_meter_refresh_needed_when_rollout_is_newer_than_cache() {
        let snapshot = json!({
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "thread_id": "thread-1"
                }
            },
            "token_budget_report": {
                "client_live_meter": {
                    "thread_id": "thread-1",
                    "turn_id": "turn-1",
                    "ended_at_epoch_ms": 1000,
                    "client_turn_total_tokens": 120000,
                    "primary_limit_used_percent": 91,
                    "secondary_limit_used_percent": 28
                }
            }
        });
        let cached = cached_client_live_meter_state(&snapshot);
        let rollout = RolloutClientMeterObservation {
            thread_id: "thread-1".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_epoch_ms: 900,
            ended_at_epoch_ms: 1100,
            first_assistant_response_at_epoch_ms: None,
            client_turn_total_tokens: 122000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 93,
            latest_secondary_limit_used_percent: 29,
            latest_primary_window_duration_mins: None,
            latest_primary_resets_at_epoch_seconds: None,
            latest_secondary_window_duration_mins: None,
            latest_secondary_resets_at_epoch_seconds: None,
            rollout_jsonl_tolerance_summary: RolloutJsonlToleranceSummary::default(),
            observation_source: "codex_rollout_client_meter_v1".to_string(),
        };

        assert!(client_live_meter_refresh_needed(&cached, Some(&rollout)));
    }

    #[test]
    fn client_live_meter_refresh_needed_when_cached_thread_drifted_from_working_state() {
        let snapshot = json!({
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "thread_id": "thread-2"
                }
            },
            "token_budget_report": {
                "client_live_meter": {
                    "thread_id": "thread-1",
                    "turn_id": "turn-1",
                    "ended_at_epoch_ms": 1000,
                    "client_turn_total_tokens": 120000,
                    "primary_limit_used_percent": 91,
                    "secondary_limit_used_percent": 28
                }
            }
        });
        let cached = cached_client_live_meter_state(&snapshot);

        assert!(client_live_meter_refresh_needed(&cached, None));
    }

    #[test]
    fn active_agent_card_refresh_needed_when_rollout_drift_exceeds_10_seconds() {
        let rollout = RolloutClientMeterObservation {
            thread_id: "thread-bounty".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            turn_id: "turn-2".to_string(),
            started_at_epoch_ms: 1_000,
            ended_at_epoch_ms: 25_001,
            first_assistant_response_at_epoch_ms: None,
            client_turn_total_tokens: 150_000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200_000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 74,
            latest_secondary_limit_used_percent: 22,
            latest_primary_window_duration_mins: Some(300),
            latest_primary_resets_at_epoch_seconds: Some(123),
            latest_secondary_window_duration_mins: Some(10080),
            latest_secondary_resets_at_epoch_seconds: Some(456),
            rollout_jsonl_tolerance_summary: RolloutJsonlToleranceSummary::default(),
            observation_source: "codex_rollout_client_meter_v2".to_string(),
        };

        assert!(super::active_agent_card_refresh_needed_against_rollout(
            15_000, &rollout, 10_000,
        ));
    }

    #[test]
    fn active_agent_card_refresh_not_needed_within_10_second_drift_budget() {
        let rollout = RolloutClientMeterObservation {
            thread_id: "thread-amai".to_string(),
            rollout_path: "/tmp/rollout.jsonl".to_string(),
            turn_id: "turn-2".to_string(),
            started_at_epoch_ms: 1_000,
            ended_at_epoch_ms: 25_000,
            first_assistant_response_at_epoch_ms: None,
            client_turn_total_tokens: 150_000,
            client_turn_input_tokens: 0,
            client_turn_cached_input_tokens: 0,
            client_turn_output_tokens: 0,
            client_turn_reasoning_output_tokens: 0,
            latest_cumulative_total_tokens: 200_000,
            latest_model_context_window: 258400,
            latest_primary_limit_used_percent: 55,
            latest_secondary_limit_used_percent: 77,
            latest_primary_window_duration_mins: Some(300),
            latest_primary_resets_at_epoch_seconds: Some(123),
            latest_secondary_window_duration_mins: Some(10080),
            latest_secondary_resets_at_epoch_seconds: Some(456),
            rollout_jsonl_tolerance_summary: RolloutJsonlToleranceSummary::default(),
            observation_source: "codex_rollout_client_meter_v2".to_string(),
        };

        assert!(!super::active_agent_card_refresh_needed_against_rollout(
            15_000, &rollout, 10_000,
        ));
    }

    #[test]
    fn degradation_model_reports_proven_and_gap_classes_honestly() {
        let payload = json!({
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 100,
                    "cross_project_leakage": 0,
                    "cross_namespace_leakage": 0,
                    "formal_invariants": [
                        { "name": "strict_local_visible_projects_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_projects", "pass": true },
                        { "name": "hostile_mixed_query_fail_closed", "pass": true },
                        { "name": "hostile_mixed_query_visible_projects_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_projects", "pass": true },
                        { "name": "strict_local_visible_namespaces_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "hostile_mixed_query_visible_namespaces_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_visible_projects_only", "pass": true },
                        { "name": "namespace_strict_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_fail_closed", "pass": true }
                    ]
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 200,
                    "restore_freshness_state": "fresh",
                    "restore_confidence": "medium"
                }
            }
        });

        let model = build_degradation_model(&payload).expect("degradation model");
        assert_eq!(model["summary"]["pass"], json!(2));
        assert_eq!(model["summary"]["unknown"], json!(9));
        assert_eq!(model["summary"]["evidence_gaps"], json!(9));

        let classes = model["classes"].as_array().expect("classes");
        let cross_project = classes
            .iter()
            .find(|item| item["class_key"].as_str() == Some("cross_project_scope"))
            .expect("cross_project_scope");
        assert_eq!(cross_project["status"], json!("pass"));

        let stale_handoff = classes
            .iter()
            .find(|item| item["class_key"].as_str() == Some("stale_handoff"))
            .expect("stale_handoff");
        assert_eq!(stale_handoff["status"], json!("unknown"));
        assert_eq!(
            stale_handoff["last_evidence_kind"],
            json!("working_state_restore")
        );
        assert_eq!(stale_handoff["evidence_gap"], json!(true));
    }

    #[test]
    fn degradation_model_promotes_classes_after_degradation_verification() {
        let payload = json!({
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "captured_at_epoch_ms": 100,
                    "cross_project_leakage": 0,
                    "cross_namespace_leakage": 0,
                    "formal_invariants": [
                        { "name": "strict_local_visible_projects_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_projects", "pass": true },
                        { "name": "hostile_mixed_query_fail_closed", "pass": true },
                        { "name": "hostile_mixed_query_visible_projects_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_projects", "pass": true },
                        { "name": "strict_local_visible_namespaces_only", "pass": true },
                        { "name": "strict_local_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "hostile_mixed_query_visible_namespaces_only", "pass": true },
                        { "name": "hostile_mixed_query_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_visible_projects_only", "pass": true },
                        { "name": "namespace_strict_hits_do_not_leak_namespaces", "pass": true },
                        { "name": "namespace_strict_fail_closed", "pass": true }
                    ]
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 200,
                    "restore_freshness_state": "fresh",
                    "restore_confidence": "medium"
                }
            },
            "latest_degradation_verification": {
                "degradation_verification": {
                    "captured_at_epoch_ms": 300,
                    "scenarios": [
                        {
                            "class_key": "cross_agent_scope",
                            "status": "pass",
                            "reason": "cross agent proof passed"
                        },
                        {
                            "class_key": "stale_handoff",
                            "status": "pass",
                            "reason": "stale handoff proof passed"
                        },
                        {
                            "class_key": "working_state_conflict",
                            "status": "pass",
                            "reason": "conflict proof passed"
                        }
                    ]
                }
            }
        });

        let model = build_degradation_model(&payload).expect("degradation model");
        assert_eq!(model["summary"]["pass"], json!(5));
        assert_eq!(model["summary"]["unknown"], json!(6));

        let classes = model["classes"].as_array().expect("classes");
        let cross_agent = classes
            .iter()
            .find(|item| item["class_key"].as_str() == Some("cross_agent_scope"))
            .expect("cross_agent_scope");
        assert_eq!(cross_agent["status"], json!("pass"));
        assert_eq!(
            cross_agent["last_evidence_kind"],
            json!("degradation_verification")
        );
        assert_eq!(cross_agent["evidence_gap"], json!(false));
    }

    #[test]
    fn contamination_check_flags_live_context_payload_in_benchmark_lane() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "_observability": {
                    "source_class": "live_context"
                },
                "load_verification": {
                    "record_live_context": true,
                    "publish_benchmark_snapshot": false
                }
            },
            "latest_retrieval_load_cold": null,
            "latest_retrieval_hot": null,
            "latest_retrieval_cold": null,
            "latest_cold_path_benchmark": null
        });
        assert_eq!(benchmark_contamination_value(&snapshot), Some(1.0));
    }

    #[test]
    fn contamination_check_stays_clean_for_benchmark_payload() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "_observability": {
                    "source_class": "benchmark"
                },
                "load_verification": {
                    "record_live_context": false,
                    "publish_benchmark_snapshot": true
                }
            },
            "latest_retrieval_load_cold": null,
            "latest_retrieval_hot": null,
            "latest_retrieval_cold": null,
            "latest_cold_path_benchmark": null
        });
        assert_eq!(benchmark_contamination_value(&snapshot), Some(0.0));
    }

    #[test]
    fn contamination_check_prefers_raw_lane_when_dashboard_falls_back_to_clean_snapshot() {
        let snapshot = json!({
            "latest_retrieval_load_hot": {
                "_observability": {
                    "source_class": "benchmark"
                },
                "load_verification": {
                    "record_live_context": false,
                    "publish_benchmark_snapshot": true
                }
            },
            "latest_retrieval_load_hot_raw": {
                "_observability": {
                    "source_class": "live_context"
                },
                "load_verification": {
                    "record_live_context": true,
                    "publish_benchmark_snapshot": false
                }
            },
            "latest_retrieval_load_cold": null,
            "latest_retrieval_load_cold_raw": null,
            "latest_retrieval_hot": null,
            "latest_retrieval_cold": null,
            "latest_cold_path_benchmark": null
        });
        assert_eq!(benchmark_contamination_value(&snapshot), Some(1.0));
    }

    #[test]
    fn select_latest_clean_benchmark_snapshot_skips_contaminated_latest_payload() {
        let contaminated = json!({
            "_observability": { "source_class": "live_context" },
            "load_verification": {
                "record_live_context": true,
                "publish_benchmark_snapshot": false,
                "qps": 2.0
            }
        });
        let clean = json!({
            "_observability": { "source_class": "benchmark" },
            "load_verification": {
                "record_live_context": false,
                "publish_benchmark_snapshot": true,
                "qps": 62000.0
            }
        });

        let selected = select_latest_clean_benchmark_snapshot(
            &[contaminated.clone(), clean.clone()],
            "load_verification",
        );
        assert_eq!(selected, Some(clean));
    }

    #[test]
    fn select_latest_dashboard_cold_benchmark_prefers_canonical_over_newer_proof() {
        let proof = json!({
            "_observability": { "source_class": "benchmark" },
            "cold_benchmark": {
                "profile": {
                    "display_name": "Local Proof Cold Contour",
                    "min_sample_count": 1000,
                    "min_repo_count": 75,
                    "min_query_slice_count": 200
                },
                "machine_readable_summary": {
                    "sample_count": 9,
                    "repo_count": 4,
                    "query_slice_count": 9
                }
            }
        });
        let canonical = json!({
            "_observability": { "source_class": "benchmark" },
            "cold_benchmark": {
                "profile": {
                    "display_name": "Large Real-Repos Cold Contour",
                    "min_sample_count": 1000,
                    "min_repo_count": 75,
                    "min_query_slice_count": 200
                },
                "machine_readable_summary": {
                    "sample_count": 1105,
                    "repo_count": 75,
                    "query_slice_count": 221
                }
            }
        });

        let selected =
            super::select_latest_dashboard_cold_benchmark_snapshot(&[proof, canonical.clone()]);
        assert_eq!(selected, Some(canonical));
    }

    #[test]
    fn retrieval_hot_sla_uses_configured_critical_threshold() {
        let profile = load_profile().expect("profile");
        let snapshot = json!({
            "postgres": {
                "connection_usage_ratio": 0.1,
                "query_probe_p95_ms": 1.0,
                "replica_lag_seconds": 0.0,
                "deadlocks_total": 0.0,
                "deadlocks_delta": 0.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            },
            "nats": {
                "publish_probe_p95_ms": 0.1,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.01
            },
            "latest_retrieval_cold": {
                "benchmark": {
                    "p95_ms": 2.0
                },
                "retrieval_runtime": {
                    "stage_p95_ms": {
                        "semantic_search_ms": 0.0
                    }
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "p95_ms": 7.0
                }
            },
            "latest_index_project": {
                "index_project": {
                    "parser_coverage_ratio": 1.0
                }
            }
        });

        let sla = evaluate_sla(&snapshot, &profile);
        let hot_check = sla["checks"]
            .as_array()
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|check| check["metric"].as_str() == Some("retrieval.hot_p95_ms"))
            })
            .expect("hot retrieval SLA check");
        assert_eq!(hot_check["target"].as_f64(), Some(1.0));
        assert_eq!(hot_check["alert"].as_f64(), Some(13.0));
        assert_eq!(hot_check["critical"].as_f64(), Some(15.0));
        assert_eq!(hot_check["status"].as_str(), Some("alert"));
    }

    #[test]
    fn postgres_historical_deadlock_total_without_fresh_delta_does_not_fail_sla() {
        let profile = load_profile().expect("profile");
        let snapshot = json!({
            "postgres": {
                "connection_usage_ratio": 0.1,
                "query_probe_p95_ms": 1.0,
                "replica_lag_seconds": 0.0,
                "deadlocks_total": 1.0,
                "deadlocks_delta": 0.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            },
            "nats": {
                "publish_probe_p95_ms": 0.1,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.01
            },
            "latest_retrieval_cold": {
                "benchmark": {
                    "p95_ms": 2.0
                },
                "retrieval_runtime": {
                    "stage_p95_ms": {
                        "semantic_search_ms": 0.0
                    }
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "p95_ms": 2.0
                }
            },
            "latest_index_project": {
                "index_project": {
                    "parser_coverage_ratio": 1.0
                }
            }
        });

        let sla = evaluate_sla(&snapshot, &profile);
        let deadlock_check = sla["checks"]
            .as_array()
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|check| check["metric"].as_str() == Some("postgres.deadlocks_delta"))
            })
            .expect("deadlock delta check");
        assert_eq!(deadlock_check["value"].as_f64(), Some(0.0));
        assert_eq!(deadlock_check["status"].as_str(), Some("pass"));
        assert_eq!(sla["summary"]["critical"].as_u64(), Some(0));
    }

    #[test]
    fn shared_client_budget_surfaces_cache_reuses_fresh_bundle() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-surfaces-cache-{}",
            Uuid::new_v4()
        ));
        let root_cause = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 10.00%"
                }
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500,
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "compact_current_thread_for_client_budget"
                }
            }
        });
        let cache =
            super::build_compact_client_budget_surfaces_cache(&root_cause, &gate, &guard, None);
        super::write_shared_compact_client_budget_surfaces(&temp_root, None, &cache)
            .expect("write shared cache");
        let loaded = super::load_shared_compact_client_budget_surfaces(&temp_root, 10_500, None)
            .expect("fresh shared cache");
        assert_eq!(
            loaded.cache_version,
            super::CLIENT_BUDGET_SURFACES_SHARED_CACHE_VERSION
        );
        assert_eq!(
            loaded.root_cause["client_budget_reply_gate"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 10.00%")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn try_load_fast_thread_bound_materialized_compact_client_budget_surfaces_uses_cached_surfaces()
    {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-fast-thread-bound-surfaces-cache-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let root_cause = json!({
            "thread_binding_state": "current_thread_bound",
            "current_live_turn": {
                "status": "observed"
            },
            "host_current_thread_control_effect": {
                "effect_verdict": "same_thread_host_control_not_requested"
            },
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": observed_at_epoch_ms,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 10.00%"
                }
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": observed_at_epoch_ms,
                "reply_execution_gate": {
                    "action_kind": "rotate_chat_for_client_budget"
                }
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": observed_at_epoch_ms,
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "rotate_chat_for_client_budget"
                }
            }
        });
        let cache = super::build_compact_client_budget_surfaces_cache(
            &root_cause,
            &gate,
            &guard,
            Some("thread-current"),
        );
        super::write_shared_compact_client_budget_surfaces(
            &temp_root,
            Some("thread-current"),
            &cache,
        )
        .expect("write shared cache");
        let loaded = super::try_load_fast_thread_bound_materialized_compact_client_budget_surfaces(
            &temp_root,
            "thread-current",
        )
        .expect("fast thread-bound surfaces");
        assert_eq!(
            loaded.surfaces.root_cause_payload["client_budget_reply_gate"]["reply_execution_gate"]
                ["reply_prefix"],
            json!("5ч KPI: переплата 10.00%")
        );
        assert_eq!(
            loaded.gate.gate_payload["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_client_budget_surfaces_cache_fail_closed_when_stale() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-surfaces-cache-stale-{}",
            Uuid::new_v4()
        ));
        let root_cause = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache =
            super::build_compact_client_budget_surfaces_cache(&root_cause, &gate, &guard, None);
        super::write_shared_compact_client_budget_surfaces(&temp_root, None, &cache)
            .expect("write shared cache");
        let cache_path = super::client_budget_surfaces_shared_cache_path(&temp_root, None);
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(1u64);
        persisted["root_cause"]["client_budget_reply_gate"]["observed_at_epoch_ms"] = json!(1u64);
        persisted["gate"]["client_budget_reply_gate"]["observed_at_epoch_ms"] = json!(1u64);
        persisted["guard"]["observed_at_epoch_ms"] = json!(1u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite stale cache");
        assert!(
            super::load_shared_compact_client_budget_surfaces(
                &temp_root,
                super::CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS + 2,
                None,
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_snapshot_invalidation_roundtrips() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-snapshot-invalidation-{}",
            Uuid::new_v4()
        ));
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidated_at =
            super::load_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
                .expect("load invalidation");
        assert!(invalidated_at > 0);
        assert!(
            super::load_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-other")
                .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_roundtrips_when_fresh() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 0,
                            "with_amai_tokens": 0,
                            "saved_tokens": 0,
                            "saved_pct": 0.0
                        },
                        "matched_events_count": 0,
                        "retrieval_context_pack_count": 0
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let loaded = super::load_shared_thread_bound_budget_snapshot(
            &temp_root,
            super::current_epoch_ms_u64(),
            "thread-current",
        )
        .expect("load thread-bound snapshot");
        assert_eq!(
            loaded["token_budget_report"]["token_budget_report"]["surface"],
            json!("dashboard_current_session_budget_only")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_respects_invalidation() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-invalidated-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 0,
                            "with_amai_tokens": 0,
                            "saved_tokens": 0,
                            "saved_pct": 0.0
                        },
                        "matched_events_count": 0,
                        "retrieval_context_pack_count": 0
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let cache_path =
            super::thread_bound_budget_snapshot_shared_cache_path(&temp_root, "thread-current");
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(9_600u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidation_path = super::thread_bound_snapshot_invalidation_shared_cache_path(
            &temp_root,
            "thread-current",
        );
        let mut invalidation: serde_json::Value =
            serde_json::from_slice(&fs::read(&invalidation_path).expect("read invalidation"))
                .expect("parse invalidation");
        invalidation["invalidated_at_epoch_ms"] = json!(9_700u64);
        fs::write(
            &invalidation_path,
            serde_json::to_vec(&invalidation).expect("serialize invalidation"),
        )
        .expect("rewrite invalidation");
        assert!(
            super::load_shared_thread_bound_budget_snapshot(&temp_root, 10_000, "thread-current")
                .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn load_shared_budget_snapshot_preview_uses_thread_bound_snapshot_cache() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-budget-snapshot-preview-cache-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let loaded = super::load_shared_budget_snapshot_preview(&temp_root, Some("thread-current"))
            .expect("load shared budget snapshot preview");
        assert_eq!(
            loaded["token_budget_report"]["token_budget_report"]["surface"],
            json!("dashboard_current_session_budget_only")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_rejects_stale_exact_limit_surfaces() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-stale-exact-limits-{}",
            Uuid::new_v4()
        ));
        let stale_observed_at_epoch_ms =
            20_000u64.saturating_sub(super::COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS + 1);
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": stale_observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": stale_observed_at_epoch_ms
                        }
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let cache_path =
            super::thread_bound_budget_snapshot_shared_cache_path(&temp_root, "thread-current");
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(20_000u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");

        assert!(
            super::load_shared_thread_bound_budget_snapshot(&temp_root, 20_000, "thread-current")
                .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_rejects_unmaterialized_current_live_turn_exact_pair() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-exact-pair-gap-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "activity_observed_exact_pair_unavailable",
                        "exact_pair_available": false,
                        "matched_events_count": 1,
                        "retrieval_context_pack_count": 1
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        assert!(
            super::load_shared_thread_bound_budget_snapshot(
                &temp_root,
                super::current_epoch_ms_u64(),
                "thread-current",
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_budget_snapshot_accepts_no_amai_activity_exact_pair() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-thread-bound-budget-snapshot-no-amai-activity-{}",
            Uuid::new_v4()
        ));
        let observed_at_epoch_ms = super::current_epoch_ms_u64();
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": observed_at_epoch_ms
                    },
                    "client_live_meter": {
                        "status_bar_rate_limits": {
                            "status": "observed",
                            "observed_at_epoch_ms": observed_at_epoch_ms
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "exact_pair_available": true,
                        "exact_pair": {
                            "without_amai_tokens": 0,
                            "with_amai_tokens": 0,
                            "saved_tokens": 0,
                            "saved_pct": 0.0
                        },
                        "matched_events_count": 0,
                        "retrieval_context_pack_count": 0
                    }
                }
            }
        });
        super::write_shared_thread_bound_budget_snapshot(&temp_root, "thread-current", &snapshot)
            .expect("write thread-bound snapshot");
        let loaded = super::load_shared_thread_bound_budget_snapshot(
            &temp_root,
            super::current_epoch_ms_u64(),
            "thread-current",
        )
        .expect("load thread-bound snapshot");
        assert_eq!(
            loaded["token_budget_report"]["token_budget_report"]["current_live_turn"]["status"],
            json!("no_amai_activity_in_current_live_turn")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_client_budget_surfaces_cache_respects_invalidation() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-surfaces-cache-invalidated-{}",
            Uuid::new_v4()
        ));
        let root_cause = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache = super::build_compact_client_budget_surfaces_cache(
            &root_cause,
            &gate,
            &guard,
            Some("thread-current"),
        );
        super::write_shared_compact_client_budget_surfaces(
            &temp_root,
            Some("thread-current"),
            &cache,
        )
        .expect("write thread-bound shared cache");
        let cache_path =
            super::client_budget_surfaces_shared_cache_path(&temp_root, Some("thread-current"));
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(9_600u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidation_path = super::thread_bound_snapshot_invalidation_shared_cache_path(
            &temp_root,
            "thread-current",
        );
        let mut invalidation: serde_json::Value =
            serde_json::from_slice(&fs::read(&invalidation_path).expect("read invalidation"))
                .expect("parse invalidation");
        invalidation["invalidated_at_epoch_ms"] = json!(9_700u64);
        fs::write(
            &invalidation_path,
            serde_json::to_vec(&invalidation).expect("serialize invalidation"),
        )
        .expect("rewrite invalidation");
        assert!(
            super::load_shared_compact_client_budget_surfaces(
                &temp_root,
                10_000,
                Some("thread-current"),
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_active_thread_hint_roundtrips_when_fresh() {
        let temp_root =
            std::env::temp_dir().join(format!("amai-active-thread-hint-cache-{}", Uuid::new_v4()));
        super::write_shared_active_thread_hint(&temp_root, "thread-current")
            .expect("write active thread hint");
        let loaded =
            super::load_shared_active_thread_hint(&temp_root, super::current_epoch_ms_u64())
                .expect("fresh active thread hint");
        assert_eq!(loaded, "thread-current");
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_active_thread_hint_fails_closed_when_stale() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-active-thread-hint-cache-stale-{}",
            Uuid::new_v4()
        ));
        super::write_shared_active_thread_hint(&temp_root, "thread-current")
            .expect("write active thread hint");
        let cache_path = super::active_thread_hint_shared_cache_path(&temp_root);
        let mut persisted: super::PersistedActiveThreadHint =
            serde_json::from_slice(&fs::read(&cache_path).expect("read active thread hint"))
                .expect("decode active thread hint");
        persisted.updated_at_epoch_ms = 1;
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("encode active thread hint"),
        )
        .expect("rewrite active thread hint");
        assert!(
            super::load_shared_active_thread_hint(
                &temp_root,
                super::ACTIVE_THREAD_HINT_MAX_AGE_MS + 2,
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_client_budget_gate_cache_reuses_fresh_bundle() {
        let temp_root =
            std::env::temp_dir().join(format!("amai-client-budget-gate-cache-{}", Uuid::new_v4()));
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 10.00%"
                }
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget"
            }
        });
        let cache = super::build_compact_client_budget_gate_cache(&gate, &guard, None);
        super::write_shared_compact_client_budget_gate(&temp_root, None, &cache)
            .expect("write gate cache");
        let loaded = super::load_shared_compact_client_budget_gate(&temp_root, 10_500, None)
            .expect("gate cache");
        assert_eq!(
            loaded.cache_version,
            super::CLIENT_BUDGET_GATE_SHARED_CACHE_VERSION
        );
        assert_eq!(
            loaded.gate["client_budget_reply_gate"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 10.00%")
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn thread_bound_gate_payload_rejects_other_thread_feedback_confirmation() {
        let payload = json!({
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "confirm_same_thread_host_control_feedback",
                    "must_confirm_same_thread_host_control_feedback_before_reply": true,
                    "action_bundle": {
                        "host_current_thread_control": {
                            "effect_verdict": "other_thread",
                            "feedback_pending": true
                        }
                    }
                }
            }
        });

        assert!(
            !super::compact_thread_bound_client_budget_gate_payload_is_consistent(
                Some("thread-current"),
                &payload,
            )
        );
        assert!(
            super::compact_thread_bound_client_budget_gate_payload_is_consistent(None, &payload,)
        );
    }

    #[test]
    fn thread_bound_root_cause_payload_rejects_other_thread_feedback_confirmation() {
        let payload = json!({
            "thread_binding_state": "current_thread_bound",
            "current_live_turn": {
                "status": "no_amai_activity_in_current_live_turn"
            },
            "host_current_thread_control_effect": {
                "effect_verdict": "other_thread"
            },
            "client_budget_reply_gate": {
                "reply_execution_gate": {
                    "action_kind": "confirm_same_thread_host_control_feedback",
                    "must_confirm_same_thread_host_control_feedback_before_reply": true,
                    "action_bundle": {
                        "host_current_thread_control": {
                            "feedback_pending": true
                        }
                    }
                }
            }
        });

        assert!(
            !super::compact_thread_bound_client_budget_root_cause_payload_is_consistent(
                Some("thread-current"),
                &payload,
            )
        );
        assert!(
            super::compact_thread_bound_client_budget_root_cause_payload_is_consistent(
                None, &payload,
            )
        );
    }

    #[test]
    fn shared_client_budget_gate_cache_fail_closed_when_stale() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-gate-cache-stale-{}",
            Uuid::new_v4()
        ));
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache = super::build_compact_client_budget_gate_cache(&gate, &guard, None);
        super::write_shared_compact_client_budget_gate(&temp_root, None, &cache)
            .expect("write gate cache");
        let cache_path = super::client_budget_gate_shared_cache_path(&temp_root, None);
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(1u64);
        persisted["gate"]["client_budget_reply_gate"]["observed_at_epoch_ms"] = json!(1u64);
        persisted["guard"]["observed_at_epoch_ms"] = json!(1u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite stale cache");
        assert!(
            super::load_shared_compact_client_budget_gate(
                &temp_root,
                super::CLIENT_BUDGET_SURFACES_SHARED_CACHE_TTL_MS + 2,
                None,
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn shared_thread_bound_client_budget_gate_cache_respects_invalidation() {
        let temp_root = std::env::temp_dir().join(format!(
            "amai-client-budget-gate-cache-invalidated-{}",
            Uuid::new_v4()
        ));
        let gate = json!({
            "client_budget_reply_gate": {
                "observed_at_epoch_ms": 9_500
            }
        });
        let guard = json!({
            "observed_at_epoch_ms": 9_500
        });
        let cache =
            super::build_compact_client_budget_gate_cache(&gate, &guard, Some("thread-current"));
        super::write_shared_compact_client_budget_gate(&temp_root, Some("thread-current"), &cache)
            .expect("write gate cache");
        let cache_path =
            super::client_budget_gate_shared_cache_path(&temp_root, Some("thread-current"));
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");
        persisted["fetched_at_epoch_ms"] = json!(9_600u64);
        fs::write(
            &cache_path,
            serde_json::to_vec(&persisted).expect("serialize cache"),
        )
        .expect("rewrite cache");
        super::write_shared_thread_bound_snapshot_invalidation(&temp_root, "thread-current")
            .expect("write invalidation");
        let invalidation_path = super::thread_bound_snapshot_invalidation_shared_cache_path(
            &temp_root,
            "thread-current",
        );
        let mut invalidation: serde_json::Value =
            serde_json::from_slice(&fs::read(&invalidation_path).expect("read invalidation"))
                .expect("parse invalidation");
        invalidation["invalidated_at_epoch_ms"] = json!(9_700u64);
        fs::write(
            &invalidation_path,
            serde_json::to_vec(&invalidation).expect("serialize invalidation"),
        )
        .expect("rewrite invalidation");
        assert!(
            super::load_shared_compact_client_budget_gate(
                &temp_root,
                10_000,
                Some("thread-current"),
            )
            .is_none()
        );
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn postgres_fresh_deadlock_delta_trips_sla() {
        let profile = load_profile().expect("profile");
        let snapshot = json!({
            "postgres": {
                "connection_usage_ratio": 0.1,
                "query_probe_p95_ms": 1.0,
                "replica_lag_seconds": 0.0,
                "deadlocks_total": 2.0,
                "deadlocks_delta": 1.0
            },
            "qdrant": {
                "index_optimize_queue": 0.0,
                "update_queue_length": 0.0
            },
            "nats": {
                "publish_probe_p95_ms": 0.1,
                "consumer_lag_msgs": 0.0,
                "jetstream_disk_usage_ratio": 0.01
            },
            "latest_retrieval_cold": {
                "benchmark": {
                    "p95_ms": 2.0
                },
                "retrieval_runtime": {
                    "stage_p95_ms": {
                        "semantic_search_ms": 0.0
                    }
                }
            },
            "latest_retrieval_hot": {
                "benchmark": {
                    "p95_ms": 2.0
                }
            },
            "latest_index_project": {
                "index_project": {
                    "parser_coverage_ratio": 1.0
                }
            }
        });

        let sla = evaluate_sla(&snapshot, &profile);
        let deadlock_check = sla["checks"]
            .as_array()
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|check| check["metric"].as_str() == Some("postgres.deadlocks_delta"))
            })
            .expect("deadlock delta check");
        assert_eq!(deadlock_check["value"].as_f64(), Some(1.0));
        assert_eq!(deadlock_check["status"].as_str(), Some("critical"));
        assert_eq!(sla["summary"]["critical"].as_u64(), Some(1));
    }

    #[test]
    fn retention_cleanup_marks_old_verify_token_events_as_expired() {
        let snapshot_id = Uuid::parse_str("00000000-0000-0000-0000-000000000042").expect("uuid");
        let candidates = vec![ObservabilityRetentionCandidate {
            snapshot_id,
            snapshot_kind: "token_budget_event".to_string(),
            payload: json!({
                "token_budget_event": {
                    "traffic_class": "verify"
                }
            }),
            source_kind: "verify_token_benchmark".to_string(),
            source_class: "operational".to_string(),
            created_at_epoch_ms: 0,
            captured_at_epoch_ms: Some(0),
        }];
        let expired =
            expired_retention_candidates(&candidates, 720_u64.saturating_mul(3_600_000) + 1)
                .expect("expired candidates");
        assert_eq!(expired.len(), 1);
        assert_eq!(
            expired[0]["retention_class"].as_str(),
            Some("synthetic_token_event")
        );
    }

    #[test]
    fn thresholds_surface_observability_policy_versions() {
        let profile = load_profile().expect("profile");
        let thresholds = profile_thresholds_json(&profile);
        assert_eq!(
            thresholds["observability"]["classification_rules_version"].as_str(),
            Some("observability-source-class-v2")
        );
        assert_eq!(
            thresholds["observability"]["schema_version"].as_u64(),
            Some(2)
        );
    }

    #[test]
    fn client_budget_guard_keeps_rotate_requirements_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "must_rotate_before_reply": true
            }
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_allows_reply_when_rotate_flags_are_clear() {
        let guard = json!({
            "reply_execution_gate": {
                "must_rotate_before_reply": false,
                "blocking": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": false
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_allows_reply_for_rotate_soon_advisory_only() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "must_rotate_before_reply": false,
                "blocking": false
            },
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": true
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn client_budget_guard_keeps_global_budget_wait_advisory() {
        let guard = json!({
            "reply_execution_gate": {
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true,
                "must_wait_for_budget_recovery_before_reply": true
            },
            "requires_global_budget_recovery_before_reply": true
        });
        assert!(!super::client_budget_guard_blocks_reply(&guard));
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_only_gate_fields() {
        let guard = json!({
            "status": "critical",
            "status_label": "глобальный лимит клиента почти исчерпан",
            "reply_prefix": "5ч KPI: переплата 2.46%",
            "reason": "heavy human explanation",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": false,
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "reason": "client_budget_guard_global_exhaustion",
                "action_kind": "wait_for_global_client_budget_recovery",
                "blocking": true,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 2.46%",
                "reply_budget_contract": {
                    "host_context_compaction_inactive_target_pressure_active": true,
                    "same_meter_pure_burn_turn_active": true,
                    "must_prefer_short_paragraphs": true,
                    "must_avoid_commentary_only_updates": true,
                    "must_batch_all_tool_reads_before_reply": true,
                    "must_wait_for_meaningful_result_before_progress_reply": true,
                    "must_require_material_delta_before_next_reply": true,
                    "must_avoid_progress_reply_when_only_guard_changed": true,
                    "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                    "max_bullets_soft": 0,
                    "max_sentences_soft": 1,
                    "max_tool_roundtrips_soft": 0
                },
                "rotate_now": false,
                "rotate_soon": false,
                "blocking_reply_contract": {
                    "active": true,
                    "contract_version": working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION,
                    "response_kind": working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND,
                    "max_sentences": working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES,
                    "template": working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE,
                },
                "action_bundle": {
                    "preserves_return_obligation": false,
                }
            },
            "last_request": "heavy row",
            "tracked_slice": "heavy row",
            "client_limits": "heavy row"
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["status_label"].as_str(),
            Some("глобальный лимит клиента почти исчерпан")
        );
        assert_eq!(
            payload["reply_execution_gate"]["reply_budget_mode"].as_str(),
            Some("compact_high_signal")
        );
        assert_eq!(
            payload["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 2.46%")
        );
        assert_eq!(
            payload["reply_execution_gate"]["reply_prefix"].as_str(),
            Some("5ч KPI: переплата 2.46%")
        );
        assert_eq!(
            payload["reply_execution_gate"]["preserves_return_obligation"].as_bool(),
            Some(false)
        );
        assert_eq!(
            payload["reply_execution_gate"]["same_meter_pure_burn_turn_active"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_prefer_short_paragraphs"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_avoid_commentary_only_updates"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_bullets_soft"],
            json!(0)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_sentences_soft"],
            json!(1)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_tool_roundtrips_soft"],
            json!(0)
        );
        assert!(payload.get("last_request").is_none());
        assert!(payload.get("tracked_slice").is_none());
        assert!(payload.get("client_limits").is_none());
        assert!(payload.get("reason").is_none());
        assert!(
            payload
                .get("requires_global_budget_recovery_before_reply")
                .is_none()
        );
        assert!(payload["reply_execution_gate"]["reply_budget_contract"].is_null());
        assert!(payload["reply_execution_gate"]["action_bundle"].is_null());
        assert!(payload["reply_execution_gate"]["blocking_reply_contract"].is_null());
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_blocking_contract_null_when_not_blocking() {
        let guard = json!({
            "status": "alert",
            "status_label": "цель >90% не достигнута",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "should_rotate_chat_now": false,
            "should_rotate_chat_soon": false,
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "reason": "client_budget_guard_clear",
                "action_kind": "continue_current_chat",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": "compact_high_signal",
                "rotate_now": false,
                "rotate_soon": false,
                "preserves_return_obligation": true,
                "blocking_reply_contract": {
                    "active": false,
                    "template": "should not leak"
                }
            }
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert!(payload["reply_execution_gate"]["blocking_reply_contract"].is_null());
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_compact_rotate_action_bundle() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: экономия 39.49%",
            "observed_at_epoch_ms": 1774789858128u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "critical_regrowth",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": true,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: экономия 39.49%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "host_current_thread_control": {
                        "available": true,
                        "control_kind": "hotkey_window_open_current",
                        "command_id": "hotkey-window-open-current",
                        "automation_ready": false
                    },
                    "operator_flow": {
                        "primary_command_kind": "same_thread_host_control_launch_command",
                        "primary_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--compact-window' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--compact-window' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'",
                        "rotate_helper_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'",
                        "startup_command": "'amai' 'continuity' 'startup' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index' '--token-source-kind' 'live_continuity_startup' '--json'"
                    }
                }
            }
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["bundle_version"],
            json!("rotate-chat-action-bundle-v1")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("same_thread_host_control_launch_command")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            payload["reply_execution_gate"]["host_context_compaction_stage"],
            json!("critical_regrowth")
        );
        assert_eq!(
            payload["reply_execution_gate"]["host_context_compaction_critical_regrowth_active"],
            json!(true)
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command"]
                .as_str()
                .unwrap_or_default()
                .contains("ctl-launch")
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]
                .get("host_current_thread_control_launch_command")
                .is_none()
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["startup_command"]
                .as_str()
                .unwrap_or_default()
                .contains("live_continuity_startup")
        );
    }

    #[test]
    fn compact_client_budget_gate_payload_keeps_same_thread_wait_metadata() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 32.34%",
            "observed_at_epoch_ms": 1774831991851u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "critical_regrowth",
                "must_confirm_same_thread_host_control_feedback_before_reply": true,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": true,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 32.34%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "feedback_confirmation_before_retry_required": true,
                    "order": [
                        "confirm_same_thread_host_control_feedback",
                        "measure_existing_same_thread_effect",
                        "fallback_rotate_chat"
                    ],
                    "host_current_thread_control": {
                        "available": true,
                        "control_kind": "thread_overlay_open_current",
                        "command_id": "thread-overlay-open-current",
                        "retry_allowed": false
                    },
                    "operator_flow": {
                        "primary_command_kind": "confirm_same_thread_host_control_feedback",
                        "primary_command": null,
                        "same_thread_feedback_confirmation_required": true,
                        "same_thread_feedback_confirmation_summary": "Requested same-thread overlay launch via host current-thread control.",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--project' 'amai' '--namespace' 'continuity' '--repo-root' '/home/art/agent-memory-index'"
                    }
                }
            }
        });
        let payload = super::compact_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["feedback_confirmation_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["order"][0],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_confirm_same_thread_host_control_feedback_before_reply"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"],
            json!(true)
        );
    }

    #[test]
    fn compact_host_control_client_budget_reply_gate_stays_small_and_machine_readable() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 12.34%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true
            },
            "observed_at_epoch_ms": 1774850926868u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 12.34%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "host_current_thread_control": {
                        "available": true,
                        "command_id": "hotkey-window-open-current"
                    },
                    "operator_flow": {
                        "primary_command_kind": "same_thread_host_control_launch_command",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current' '--compact-window'"
                    }
                }
            },
            "long_prose_field": "this should not survive"
        });
        let payload = super::compact_host_control_client_budget_reply_gate(&guard);
        assert_eq!(payload["reply_prefix"], json!("5ч KPI: переплата 12.34%"));
        assert_eq!(payload["status_label"], json!("сожми текущий чат сейчас"));
        assert_eq!(
            payload["reply_execution_gate"]["action_kind"],
            json!("compact_current_thread_for_client_budget")
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert!(payload.get("observed_at_epoch_ms").is_none());
        assert!(payload.get("max_guard_age_seconds").is_none());
        assert!(payload.get("long_prose_field").is_none());
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_trims_same_thread_surface_but_keeps_commands() {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 11.11%",
            "host_context_compaction": {
                "stage": "critical_regrowth",
                "current_thread_bound": true,
                "current_turn_total_tokens": 120000,
                "growth_since_compaction_tokens": 70000,
                "regrowth_of_recovered_surface_ratio": 0.42,
                "critical_regrowth_active": true,
                "preserve_active": true,
                "long_note": "drop me"
            },
            "observed_at_epoch_ms": 1774852000000u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "critical_regrowth",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": true,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 11.11%",
                "reply_budget_contract": {
                    "must_prefer_short_paragraphs": true,
                    "must_avoid_commentary_only_updates": true,
                    "max_bullets_soft": 0,
                    "max_sentences_soft": 1,
                    "max_tool_roundtrips_soft": 0
                },
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "feedback_confirmation_before_retry_required": true,
                    "order": [
                        "confirm_same_thread_host_control_feedback",
                        "measure_existing_same_thread_effect",
                        "fallback_rotate_chat"
                    ],
                    "host_current_thread_control": {
                        "available": true,
                        "automation_ready": true,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "critical_regrowth",
                        "feedback_pending": true,
                        "measurement_pending": true,
                        "retry_allowed": false,
                        "retry_blocked_reason": "pending feedback",
                        "effect_verdict": "measurement_pending",
                        "effect_summary": "summary",
                        "selection_reason": "protect_recent_host_compaction_gain",
                        "external_uri_launch": {
                            "uri": "vscode://too-big"
                        },
                        "alternate_controls": [
                            {
                                "button_label": "Open thread overlay",
                                "command_id": "thread-overlay-open-current",
                                "control_kind": "thread_overlay_open_current",
                                "note": "drop me"
                            }
                        ]
                    },
                    "operator_flow": {
                        "primary_command_kind": "confirm_same_thread_host_control_feedback",
                        "host_current_thread_control_launch_command": "launch",
                        "rotate_helper_command": "rotate"
                    }
                }
            }
        });
        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        assert_eq!(payload["status_label"], json!("сожми текущий чат сейчас"));
        assert!(payload.get("reply_prefix").is_none());
        assert!(payload.get("host_context_compaction").is_none());
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["operator_flow"]["rotate_helper_command"],
            json!("rotate")
        );
        assert_eq!(
            payload["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 11.11%")
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_prefer_short_paragraphs"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["must_avoid_commentary_only_updates"],
            json!(true)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_bullets_soft"],
            json!(0)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_sentences_soft"],
            json!(1)
        );
        assert_eq!(
            payload["reply_execution_gate"]["max_tool_roundtrips_soft"],
            json!(0)
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                ["host_current_thread_control"]["command_id"]
                .as_str()
                == Some("hotkey-window-open-current")
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                .get("order")
                .is_none()
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_drops_long_same_thread_details_after_verified_failure()
     {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 204.20%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true,
                "current_turn_total_tokens": 77335,
                "growth_since_compaction_tokens": 28228,
                "regrowth_of_recovered_surface_ratio": 0.15,
                "critical_regrowth_active": false,
                "preserve_active": true
            },
            "observed_at_epoch_ms": 1774854192111u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 204.20%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "order": [
                        "run_rotate_helper",
                        "open_fresh_chat",
                        "run_continuity_startup"
                    ],
                    "host_current_thread_control": {
                        "available": false,
                        "automation_ready": false,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "preserve",
                        "feedback_pending": false,
                        "measurement_pending": false,
                        "retry_allowed": false,
                        "retry_blocked_reason": "Same-thread compact window локально меняет thread, но полный 5ч burn всё ещё идёт хуже идеального темпа на +12.68 п.п.; rotate fallback should become primary.",
                        "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                        "effect_summary": "Очень длинный exhausted summary, который больше не должен тащиться в CLI payload после verified failure.",
                        "selection_reason": "protect_recent_host_compaction_gain",
                        "availability_state": "exhausted_after_verified_failure",
                        "surface_exhausted_after_verified_failure": true,
                        "alternate_controls": [
                            {
                                "button_label": "Open thread overlay",
                                "command_id": "thread-overlay-open-current",
                                "control_kind": "thread_overlay_open_current"
                            }
                        ]
                    },
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command",
                        "rotate_helper_command": "rotate"
                    }
                }
            }
        });
        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["availability_state"],
            json!("exhausted_after_verified_failure")
        );
        assert!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
                .get("effect_summary")
                .is_none()
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_normalizes_commands_and_deduplicates_primary_rotate_command()
     {
        let guard = json!({
            "status": "critical",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 50.00%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true,
                "current_turn_total_tokens": 50000,
                "growth_since_compaction_tokens": 12000,
                "regrowth_of_recovered_surface_ratio": 0.2,
                "critical_regrowth_active": false,
                "preserve_active": true
            },
            "observed_at_epoch_ms": 1774854192111u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 50.00%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "order": [
                        "run_rotate_helper",
                        "open_fresh_chat",
                        "run_continuity_startup"
                    ],
                    "host_current_thread_control": {
                        "available": false,
                        "automation_ready": false,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "preserve",
                        "feedback_pending": false,
                        "measurement_pending": false,
                        "retry_allowed": false,
                        "retry_blocked_reason": "rotate fallback already primary",
                        "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                        "selection_reason": "protect_recent_host_compaction_gain"
                    },
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command",
                        "primary_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai'",
                        "rotate_helper_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai'",
                        "startup_command": "'amai' 'continuity' 'startup' '--project' 'amai' '--runtime-state-json'"
                    }
                }
            }
        });

        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        let operator_flow = &payload["reply_execution_gate"]["action_bundle"]["operator_flow"];
        assert!(operator_flow.get("primary_command").is_none());
        assert!(operator_flow.get("handoff_command").is_none());
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                .get("order")
                .is_none()
        );
        assert_eq!(
            operator_flow["rotate_helper_command"],
            json!("amai continuity rotate-chat --project amai")
        );
        assert!(operator_flow.get("startup_command").is_none());
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_deduplicates_same_thread_primary_command() {
        let guard = json!({
            "status": "alert",
            "status_label": "сожми текущий чат сейчас",
            "reply_prefix": "5ч KPI: переплата 10.00%",
            "host_context_compaction": {
                "stage": "preserve",
                "current_thread_bound": true,
                "current_turn_total_tokens": 50000,
                "growth_since_compaction_tokens": 12000,
                "regrowth_of_recovered_surface_ratio": 0.2,
                "critical_regrowth_active": false,
                "preserve_active": true
            },
            "observed_at_epoch_ms": 1774854192111u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "compact_current_thread_for_client_budget",
                "blocking": false,
                "host_context_compaction_stage": "preserve",
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "host_context_compaction_critical_regrowth_active": false,
                "host_context_compaction_preserve_active": true,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: переплата 10.00%",
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "ready_for_automation": true,
                    "preserves_return_obligation": true,
                    "order": [
                        "run_same_thread_host_control",
                        "confirm_surface_effect",
                        "fallback_rotate_chat"
                    ],
                    "host_current_thread_control": {
                        "available": true,
                        "automation_ready": true,
                        "button_label": "Open compact window",
                        "command_id": "hotkey-window-open-current",
                        "control_kind": "hotkey_window_open_current",
                        "thread_id": "thread-current",
                        "host_context_compaction_stage": "preserve",
                        "feedback_pending": false,
                        "measurement_pending": false,
                        "retry_allowed": true,
                        "effect_verdict": "measurement_pending",
                        "selection_reason": "protect_recent_host_compaction_gain"
                    },
                    "operator_flow": {
                        "primary_command_kind": "same_thread_host_control_launch_command",
                        "primary_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current'",
                        "host_current_thread_control_launch_command": "'amai' 'observe' 'ctl-launch' '--thread-id' 'thread-current'",
                        "rotate_helper_command": "'amai' 'continuity' 'rotate-chat' '--project' 'amai'",
                        "startup_command": "'amai' 'continuity' 'startup' '--project' 'amai' '--runtime-state-json'"
                    }
                }
            }
        });

        let payload = super::compact_cli_client_budget_gate_payload(&guard);
        let operator_flow = &payload["reply_execution_gate"]["action_bundle"]["operator_flow"];
        assert_eq!(
            operator_flow["primary_command"],
            json!("amai observe ctl-launch --thread-id thread-current")
        );
        assert!(
            payload["reply_execution_gate"]
                .get("must_confirm_same_thread_host_control_feedback_before_reply")
                .is_none()
        );
        assert!(
            operator_flow
                .get("host_current_thread_control_launch_command")
                .is_none()
        );
        assert!(operator_flow.get("handoff_command").is_none());
        assert!(
            payload["reply_execution_gate"]["action_bundle"]
                .get("order")
                .is_none()
        );
        assert_eq!(
            payload["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
    }

    #[test]
    fn compact_current_session_budget_guard_payload_drops_prose_and_operator_flow() {
        let guard = json!({
            "status_label": "сожми текущий чат сейчас",
            "status_tooltip": "long prose",
            "reason": "long prose",
            "note": "long prose",
            "full_turn_savings_proven": true,
            "full_turn_savings_percent": "0.00%",
            "should_rotate_chat_now": true,
            "should_rotate_chat_soon": true,
            "requires_global_budget_recovery_before_reply": false,
            "next_action": "rotate now",
            "last_request": "154048 из 258400",
            "client_limits": "5ч остаётся 69%",
            "tracked_slice": "экономия 490",
            "tracked_slice_truth": "учтённая часть",
            "client_live_meter_current_thread_bound": true,
            "client_live_meter_thread_binding_state": "current_thread_bound",
            "observed_at_epoch_ms": 1774622949000u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "gate_version": "client-reply-budget-gate-v1",
                "reason": "client_budget_guard_pressure",
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": true,
                "must_rotate_before_reply": true,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": "compact_high_signal",
                "rotate_now": true,
                "rotate_soon": true,
                "reply_budget_contract": {
                    "active": true
                },
                "action_bundle": {
                    "operator_flow": {
                        "copy_paste_ready": true
                    },
                    "preserves_return_obligation": false
                }
            }
        });
        let payload = super::compact_current_session_budget_guard_payload(&guard);
        assert_eq!(
            payload["status_label"].as_str(),
            Some("сожми текущий чат сейчас")
        );
        assert_eq!(payload["full_turn_savings_percent"].as_str(), Some("0.00%"));
        assert_eq!(payload["last_request"].as_str(), Some("154048 из 258400"));
        assert_eq!(payload["client_limits"].as_str(), Some("5ч остаётся 69%"));
        assert!(payload.get("status_tooltip").is_none());
        assert!(payload.get("reason").is_none());
        assert!(payload.get("note").is_none());
        assert!(payload["reply_execution_gate"]["reply_budget_contract"].is_null());
        assert!(payload["reply_execution_gate"]["action_bundle"].is_null());
    }

    #[test]
    fn compact_client_budget_root_cause_payload_trims_long_same_thread_effect_fields() {
        let payload = json!({
            "status": "observed",
            "reply_prefix": "5ч KPI: переплата 182.29%",
            "thread_binding_state": "current_thread_bound",
            "current_thread_bound": true,
            "current_live_meter": {
                "client_turn_total_tokens": 169873,
                "context_used_percent": 65.74
            },
            "current_live_turn": {
                "status": "no_amai_activity_in_current_live_turn",
                "saved_pct": 0.0
            },
            "same_meter_economics": {
                "strict_lower_bound_tokens": 182,
                "same_meter_without_amai_tokens": 182,
                "same_meter_with_amai_tokens": 72,
                "same_meter_saved_tokens": 110,
                "same_meter_saved_pct": 60.43956043956044,
                "continuity_restore_baseline_tokens": 178,
                "continuity_restore_observed_tokens": 68,
                "continuity_restore_delta_tokens": -110,
                "full_turn_overhang_tokens": 169691,
                "full_turn_vs_strict_ratio": 933.3681318681319,
                "dominant_cost_surface": "giant_thread_context_outside_same_meter_slice"
            },
            "exact_pair_status": {
                "state": "not_applicable_current_live_turn_has_no_amai_activity"
            },
            "guard": {
                "status_label": "сожми текущий чат сейчас",
                "action_kind": "rotate_chat_for_client_budget"
            },
            "host_context_compaction": {
                "stage": "critical_regrowth",
                "current_thread_bound": true,
                "current_turn_total_tokens": 169873,
                "growth_since_compaction_tokens": 120766,
                "regrowth_of_recovered_surface_ratio": 0.65,
                "critical_regrowth_active": true,
                "preserve_active": true,
                "long_note": "drop me"
            },
            "host_current_thread_control_effect": {
                "command_id": "hotkey-window-open-current",
                "surface_label": "compact window",
                "current_stage": "critical_regrowth",
                "thread_id": "thread-current",
                "measurement_pending": false,
                "measurement_sufficient": true,
                "retry_allowed": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "full_scale_client_burn_worsened": true,
                "rotate_fallback_recommended": true,
                "verified_host_compaction_observed_after_feedback": true,
                "compaction_count_delta": 2,
                "primary_limit_used_overrun_percent_points": 12.68,
                "turn_token_delta": -58073,
                "context_used_percent_point_delta": -22.47,
                "regrowth_since_feedback_tokens": -59781,
                "summary": "Long prose summary that should not survive compact CLI root-cause output.",
                "note": "Long prose note that should also be dropped."
            },
            "measured_components": ["client_prompt"],
            "missing_components": [],
            "blocking_reasons": []
        });

        let guard = json!({
            "status_label": "сожми текущий чат сейчас",
            "observed_at_epoch_ms": 1774862518426u64,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "rotate_chat_for_client_budget",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL,
                "reply_prefix": "5ч KPI: переплата 182.29%",
                "host_context_compaction_stage": "critical_regrowth",
                "host_context_compaction_preserve_active": true,
                "host_context_compaction_critical_regrowth_active": true,
                "reply_budget_contract": {
                    "host_context_compaction_inactive_target_pressure_active": true,
                    "same_meter_pure_burn_turn_active": true,
                    "must_prefer_short_paragraphs": true,
                    "must_avoid_commentary_only_updates": true,
                    "must_batch_all_tool_reads_before_reply": true,
                    "must_wait_for_meaningful_result_before_progress_reply": true,
                    "must_require_material_delta_before_next_reply": true,
                    "must_avoid_progress_reply_when_only_guard_changed": true,
                    "must_avoid_new_tool_turn_without_specific_delta_goal": true,
                    "max_bullets_soft": 0,
                    "max_sentences_soft": 1,
                    "max_tool_roundtrips_soft": 0
                },
                "preserves_return_obligation": true,
                "action_bundle": {
                    "bundle_version": "rotate-chat-action-bundle-v1",
                    "host_current_thread_control": {
                        "command_id": "hotkey-window-open-current",
                        "button_label": "Open compact window",
                        "automation_ready": true,
                        "retry_allowed": false,
                        "summary": "Same-thread compact window",
                        "note": "internal host surface"
                    },
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command",
                        "rotate_helper_command": "amai continuity rotate-chat",
                        "startup_command": "./scripts/continuity_startup.sh --json"
                    }
                }
            }
        });

        let compact = super::compact_client_budget_root_cause_payload(&payload, Some(&guard));

        assert_eq!(
            compact["host_current_thread_control_effect"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            compact["host_context_compaction"]["stage"],
            json!("critical_regrowth")
        );
        assert_eq!(
            compact["host_context_compaction"]["regrowth_of_recovered_surface_ratio"],
            json!(0.65)
        );
        assert!(
            compact["host_context_compaction"]
                .get("current_turn_total_tokens")
                .is_none()
        );
        assert_eq!(
            compact["current_live_meter"]["context_used_percent"],
            json!(65.74)
        );
        assert!(
            compact["current_live_turn"]
                .get("exact_pair_available")
                .is_none()
        );
        assert_eq!(
            compact["same_meter_economics"]["strict_lower_bound_tokens"],
            json!(182)
        );
        assert_eq!(
            compact["same_meter_economics"]["same_meter_saved_pct"],
            json!(60.44)
        );
        assert_eq!(
            compact["same_meter_economics"]["full_turn_vs_strict_ratio"],
            json!(933.37)
        );
        assert_eq!(
            compact["same_meter_economics"]["dominant_cost_surface"],
            json!("giant_thread_context_outside_same_meter_slice")
        );
        assert!(compact.get("exact_pair_status").is_none());
        assert!(
            compact["current_live_meter"]
                .get("ended_at_epoch_ms")
                .is_none()
        );
        assert!(compact["guard"].get("action_kind").is_none());
        assert_eq!(compact["guard"]["should_rotate_chat_now"], json!(null));
        assert!(
            compact["host_current_thread_control_effect"]
                .get("summary")
                .is_none()
        );
        assert!(
            compact["host_current_thread_control_effect"]
                .get("note")
                .is_none()
        );
        assert!(
            compact["host_context_compaction"]
                .get("long_note")
                .is_none()
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert!(
            compact["client_budget_reply_gate"]["status_label"].is_null()
                || compact["client_budget_reply_gate"]
                    .get("status_label")
                    .is_none()
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["operator_flow"]
                ["primary_command_kind"],
            json!("rotate_helper_command")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["operator_flow"]
                ["rotate_helper_command"],
            json!("amai continuity rotate-chat")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
                ["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["host_current_thread_control"]
                ["button_label"],
            json!("Open compact window")
        );
        assert!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_bundle"]["operator_flow"]
                .get("startup_command")
                .is_none()
        );
        assert!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]
                .get("host_context_compaction_stage")
                .is_none()
        );
        assert!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]
                .get("preserves_return_obligation")
                .is_none()
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["same_meter_pure_burn_turn_active"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["must_prefer_short_paragraphs"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["must_avoid_commentary_only_updates"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["must_require_material_delta_before_next_reply"],
            json!(true)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["max_bullets_soft"],
            json!(0)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["max_sentences_soft"],
            json!(1)
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["max_tool_roundtrips_soft"],
            json!(0)
        );
    }

    #[test]
    fn compact_working_state_restore_for_budget_keeps_only_budget_critical_fields() {
        let restore = json!({
            "client_budget_target_percent": 50,
            "thread_id": "thread-1",
            "current_goal": "goal",
            "next_step": "next",
            "execctl_resume_state": "pending_return_queue_present",
            "project": {
                "code": "amai",
                "repo_root": "/home/art/agent-memory-index",
                "display_name": "Amai"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "state_lineage": {
                "authoritative_event_id": "handoff-1",
                "authoritative_source_kind": "continuity_handoff",
                "authoritative_local_path": "/tmp/HANDOFF.md",
                "drop_me": "nope"
            },
            "recent_actions": [
                {
                    "source_kind": working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_SOURCE_KIND,
                    "summary": "feedback summary",
                    "recorded_at_epoch_ms": 123,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": working_state::HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED,
                        "command_id": working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID,
                        "feedback_snapshot": {
                            "thread_id": "thread-1",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100,
                                "context_used_percent": 12.5,
                                "primary_limit_used_percent": 7,
                                "secondary_limit_used_percent": 9
                            },
                            "host_context_compaction": {
                                "compaction_count": 2,
                                "growth_since_compaction_tokens": 30,
                                "compacted_at_epoch_ms": 456,
                                "stage": "preserve",
                                "note": "drop me"
                            },
                            "unrelated": "drop me"
                        }
                    },
                    "query": "drop me"
                },
                {
                    "source_kind": "continuity_handoff",
                    "summary": "unrelated"
                }
            ],
            "open_questions": ["drop me"],
            "materialized_notes": ["drop me"]
        });

        let compact = super::compact_working_state_restore_for_budget(&restore);

        assert_eq!(compact["client_budget_target_percent"], json!(50));
        assert_eq!(compact["thread_id"], json!("thread-1"));
        assert_eq!(compact["project"]["code"], json!("amai"));
        assert_eq!(
            compact["project"]["repo_root"],
            json!("/home/art/agent-memory-index")
        );
        assert!(compact["project"].get("display_name").is_none());
        assert_eq!(compact["namespace"]["code"], json!("continuity"));
        assert!(compact["namespace"].get("display_name").is_none());
        assert_eq!(
            compact["state_lineage"]["authoritative_event_id"],
            json!("handoff-1")
        );
        assert_eq!(
            compact["state_lineage"]["authoritative_source_kind"],
            json!("continuity_handoff")
        );
        assert_eq!(
            compact["state_lineage"]["authoritative_local_path"],
            json!("/tmp/HANDOFF.md")
        );
        assert!(compact["state_lineage"].get("drop_me").is_none());
        assert_eq!(compact["recent_actions"].as_array().unwrap().len(), 1);
        assert_eq!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]["feedback_snapshot"]
                ["client_live_meter"]["client_turn_total_tokens"],
            json!(100)
        );
        assert!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]["feedback_snapshot"]
                .get("unrelated")
                .is_none()
        );
        assert!(
            compact["recent_actions"][0]["host_current_thread_control_feedback"]["feedback_snapshot"]["client_live_meter"]
                .get("secondary_limit_used_percent")
                .is_none()
        );
        assert!(compact.get("open_questions").is_none());
        assert!(compact.get("materialized_notes").is_none());
    }

    #[test]
    fn compact_budget_snapshot_preview_payload_trims_hourly_burn_and_alignment_to_essential_fields()
    {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "surface": "dashboard_current_session_budget_only",
                    "client_budget_target_percent": 50,
                    "current_session": {
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "counted_events": 3,
                        "live_events_count": 4,
                        "total_saved_tokens": 5,
                        "savings_percent": 12.3456,
                        "effective_savings_pct": 23.4567,
                        "observed_client_prompt_tokens": 10,
                        "observed_tool_overhead_tokens": 11,
                        "observed_continuity_restore_tokens": 12,
                        "observed_assistant_generation_tokens": 13,
                        "age_ms_since_latest": 14,
                        "model_version": "drop me"
                    },
                    "statement_previews": {
                        "current_session": {
                            "client_limit_meter_alignment": {
                                "alignment_state": "same_meter_equivalent",
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": "exact_pair_materialized",
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 182
                                },
                                "baseline_equivalence": {
                                    "measured_baseline_tokens_lower_bound": 182,
                                    "component_semantics": [
                                        {
                                            "code": "client_prompt",
                                            "baseline_measured_tokens": 4,
                                            "observed_tokens": 4,
                                            "whole_cycle_observed_complete": true,
                                            "note": "drop me"
                                        },
                                        {
                                            "code": "continuity_restore_outside_retrieval",
                                            "baseline_measured_tokens": 178,
                                            "observed_tokens": 68,
                                            "whole_cycle_observed_complete": true,
                                            "note": "drop me"
                                        }
                                    ]
                                },
                                "blocking_reasons": ["x"],
                                "measured_components": ["client_prompt"],
                                "missing_components": ["tool_overhead"],
                                "not_applicable_components": ["retrieval_quality"],
                                "note": "drop me",
                                "model_version": "drop me",
                                "surface_kind": "drop me"
                            },
                            "observed_whole_cycle_with_amai_tokens": 72,
                            "verified_observed_whole_cycle_with_amai_tokens": 70,
                            "with_amai_measured_tokens": 72,
                            "verified_with_amai_measured_tokens": 70
                        }
                    },
                    "client_limit_hourly_burn": {
                        "status": "overspend",
                        "reply_prefix": "5ч KPI: переплата 12.34%",
                        "kpi_percent": 12.3456,
                        "actual_used_percent": 67.891,
                        "actual_remaining_percent": 32.109,
                        "ideal_used_percent": 44.444,
                        "ideal_remaining_percent": 55.556,
                        "projected_primary_used_per_hour_percent": 14.789,
                        "ideal_primary_used_per_hour_percent": 10.111,
                        "latest_observed_at_epoch_ms": 100,
                        "latest_live_age_seconds": 1.987,
                        "summary": "drop me",
                        "status_bar_correlated": true,
                        "window_minutes": 123
                    },
                    "client_live_meter": {
                        "status": "live",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "thread-1",
                        "turn_id": "turn-1",
                        "started_at_epoch_ms": 10,
                        "ended_at_epoch_ms": 11,
                        "client_turn_total_tokens": 12,
                        "context_used_percent": 34.567,
                        "primary_limit_used_percent": 35,
                        "secondary_limit_used_percent": 36,
                        "note": "drop me"
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn",
                        "scope_code": "same_meter",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "thread-1",
                        "turn_id": "turn-1",
                        "started_at_epoch_ms": 20,
                        "ended_at_epoch_ms": 21,
                        "exact_pair_available": false,
                        "exact_pair": null,
                        "matched_events_count": 0,
                        "matched_context_pack_ids_count": 0,
                        "retrieval_context_pack_count": 0,
                        "note": "drop me"
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "client_budget_target_percent": 50,
                    "thread_id": "thread-1",
                    "current_goal": "goal",
                    "next_step": "next",
                    "execctl_resume_state": "pending_return_queue_present",
                    "project": {
                        "code": "amai",
                        "repo_root": "/home/art/agent-memory-index"
                    },
                    "namespace": {
                        "code": "continuity"
                    },
                    "recent_actions": []
                }
            }
        });

        let compact = super::compact_budget_snapshot_preview_payload(&snapshot);
        let report = &compact["token_budget_report"]["token_budget_report"];
        assert_eq!(
            report["client_limit_hourly_burn"]["reply_prefix"],
            json!("5ч KPI: переплата 12.34%")
        );
        assert_eq!(
            report["client_limit_hourly_burn"]["kpi_percent"],
            json!(12.35)
        );
        assert!(report["client_limit_hourly_burn"].get("summary").is_none());
        assert!(
            report["client_limit_hourly_burn"]
                .get("status_bar_correlated")
                .is_none()
        );
        assert!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                .get("note")
                .is_none()
        );
        assert!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]
                .get("model_version")
                .is_none()
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["alignment_state"],
            json!("same_meter_equivalent")
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["strict_client_meter_slice"]
                ["lower_bound_tokens"],
            json!(182)
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]
                ["measured_baseline_tokens_lower_bound"],
            json!(182)
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]
                ["component_semantics"][1]["code"],
            json!("continuity_restore_outside_retrieval")
        );
        assert!(
            report["statement_previews"]["current_session"]["client_limit_meter_alignment"]["baseline_equivalence"]["component_semantics"][1]
                .get("note")
                .is_none()
        );
        assert_eq!(
            report["statement_previews"]["current_session"]["observed_whole_cycle_with_amai_tokens"],
            json!(72)
        );
    }

    #[test]
    fn cached_exact_client_limit_refresh_needed_when_hourly_burn_is_missing() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_limit_hourly_burn": {
                        "status": "missing"
                    }
                }
            }
        });

        assert!(super::cached_exact_client_limit_refresh_needed(
            &snapshot, 10_000, 3_000
        ));
    }

    #[test]
    fn compact_cli_client_budget_gate_from_root_cause_payload_extracts_gate() {
        let payload = json!({
            "client_budget_reply_gate": {
                "status_label": "ok",
                "observed_at_epoch_ms": 1234,
                "max_guard_age_seconds": 10,
                "global_reply_prefix": "5ч KPI: экономия 8.00%",
                "reply_prefix_source": "personal_agent_5h_kpi",
                "reply_execution_gate": {
                    "action_kind": "rotate_chat_for_client_budget",
                    "reply_prefix": "5ч KPI: переплата 1.00%",
                    "global_reply_prefix": "5ч KPI: экономия 8.00%",
                    "reply_prefix_source": "personal_agent_5h_kpi"
                }
            }
        });
        let compact = super::compact_cli_client_budget_gate_from_root_cause_payload(&payload)
            .expect("gate payload");
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["action_kind"],
            json!("rotate_chat_for_client_budget")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_execution_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert_eq!(compact["reply_prefix"], json!("5ч KPI: переплата 1.00%"));
        assert_eq!(
            compact["global_reply_prefix"],
            json!("5ч KPI: экономия 8.00%")
        );
        assert_eq!(
            compact["reply_prefix_source"],
            json!("personal_agent_5h_kpi")
        );
    }

    #[test]
    fn front_door_client_budget_gate_payload_surfaces_top_level_reply_prefix_fields() {
        let gate = json!({
            "status_label": "ok",
            "observed_at_epoch_ms": 1234,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "reply_prefix": "5ч KPI: переплата 7.00%",
                "global_reply_prefix": "5ч KPI: переплата 7.00%",
                "reply_prefix_source": "global_client_limit_hourly_burn"
            }
        });

        let payload = super::front_door_client_budget_gate_payload(gate.clone());
        assert_eq!(payload["reply_prefix"], json!("5ч KPI: переплата 7.00%"));
        assert_eq!(
            payload["global_reply_prefix"],
            json!("5ч KPI: переплата 7.00%")
        );
        assert_eq!(
            payload["reply_prefix_source"],
            json!("global_client_limit_hourly_burn")
        );
        assert_eq!(payload["client_budget_reply_gate"], gate);
    }

    #[test]
    fn normalize_front_door_client_budget_gate_payload_shape_backfills_top_level_fields() {
        let payload = json!({
            "client_budget_reply_gate": {
                "status_label": "ok",
                "observed_at_epoch_ms": 1234,
                "max_guard_age_seconds": 10,
                "reply_execution_gate": {
                    "reply_prefix": "5ч KPI: переплата 7.00%",
                    "global_reply_prefix": "5ч KPI: переплата 7.00%",
                    "reply_prefix_source": "global_client_limit_hourly_burn"
                }
            }
        });

        let normalized = super::normalize_front_door_client_budget_gate_payload_shape(payload);
        assert_eq!(normalized["reply_prefix"], json!("5ч KPI: переплата 7.00%"));
        assert_eq!(
            normalized["global_reply_prefix"],
            json!("5ч KPI: переплата 7.00%")
        );
        assert_eq!(
            normalized["reply_prefix_source"],
            json!("global_client_limit_hourly_burn")
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_from_root_cause_payload_fails_closed_without_action_kind() {
        let payload = json!({
            "client_budget_reply_gate": {
                "reply_execution_gate": {}
            }
        });
        assert!(super::compact_cli_client_budget_gate_from_root_cause_payload(&payload).is_none());
    }

    #[test]
    fn compact_chat_api_summary_drops_heavy_runtime_fields() {
        let payload = json!({
            "project": {
                "code": "amai",
                "display_name": "Amai",
                "repo_root": "/home/art/agent-memory-index"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "chat_start_restore": {
                "headline": "headline",
                "next_step": "next",
                "prompt_text": "PROMPT",
                "project_task_tree": {"drop_me": true},
                "pending_return_queue": [{"drop_me": true}]
            },
            "operator_notice": {
                "message_text": "message",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "exact_chat_command": "компакт_чат",
                "prompt_file": "/tmp/prompt.txt",
                "required_host_action": "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
                "note": "note",
                "launch_clean_chat_command": "code chat ...",
                "launch_clean_chat_fallback_command": "code chat --reuse-window ...",
                "launch_clean_chat_command_kind": "vscode_code_chat_cli",
                "manual_fallback_steps": ["Codex: New Thread in Codex Sidebar (chatgpt.newChat)"]
            },
            "client_surface": {
                "client_key": "vscode"
            },
            "host_launch": {
                "status": "available_not_requested"
            },
            "host_current_thread_control": {
                "command_id": "thread-overlay-open-current"
            },
            "client_budget_guard": {
                "drop_me": true
            }
        });

        let compact = super::compact_chat_api_summary(&payload);

        assert_eq!(compact["project"]["code"], json!("amai"));
        assert_eq!(
            compact["chat_start_restore"]["prompt_text"],
            json!("PROMPT")
        );
        assert_eq!(
            compact["operator_notice"]["exact_chat_command"],
            json!("компакт_чат")
        );
        assert_eq!(compact["client_surface"]["client_key"], json!("vscode"));
        assert_eq!(
            compact["host_launch"]["status"],
            json!("available_not_requested")
        );
        assert!(compact.get("host_current_thread_control").is_none());
        assert!(compact.get("client_budget_guard").is_none());
        assert_eq!(
            compact["operator_notice"]["launch_clean_chat_command"],
            json!("code chat ...")
        );
        assert_eq!(
            compact["operator_notice"]["launch_clean_chat_fallback_command"],
            json!("code chat --reuse-window ...")
        );
        assert_eq!(
            compact["operator_notice"]["launch_clean_chat_command_kind"],
            json!("vscode_code_chat_cli")
        );
        assert!(
            compact["operator_notice"]["manual_fallback_steps"]
                .as_array()
                .is_some()
        );
        assert!(
            compact["chat_start_restore"]
                .get("project_task_tree")
                .is_none()
        );
        assert!(
            compact["chat_start_restore"]
                .get("pending_return_queue")
                .is_none()
        );
    }

    #[test]
    fn client_budget_host_control_launch_api_summary_drops_launch_detail_echo() {
        let payload = json!({
            "project": {
                "code": "amai",
                "display_name": "Amai",
                "repo_root": "/home/art/agent-memory-index"
            },
            "namespace": {
                "code": "continuity",
                "display_name": "Continuity"
            },
            "thread_id": "thread-current",
            "command_id": "hotkey-window-open-current",
            "launch_targeting": {
                "status": "ok",
                "summary": "single target",
                "window_count": 1
            },
            "host_current_thread_control": {
                "command_id": "hotkey-window-open-current"
            },
            "launch": {
                "status": "opened"
            },
            "client_budget_reply_gate": {
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "blocking": false
            },
            "operator_notice": {
                "kind": "host_current_thread_control_launch_opened",
                "message_text": "opened",
                "feedback_kind": "opened",
                "command_id": "hotkey-window-open-current",
                "thread_id": "thread-current"
            }
        });

        let compact = super::client_budget_host_control_launch_api_summary(&payload);

        assert_eq!(
            compact["launch_targeting"]["summary"],
            json!("single target")
        );
        assert_eq!(
            compact["client_budget_reply_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert_eq!(
            compact["operator_notice"]["kind"],
            json!("host_current_thread_control_launch_opened")
        );
        assert!(compact["launch_targeting"].get("window_count").is_none());
        assert!(compact.get("host_current_thread_control").is_none());
        assert!(compact.get("launch").is_none());
        assert!(
            compact["client_budget_reply_gate"]
                .get("blocking")
                .is_none()
        );
    }

    #[test]
    fn thread_bound_host_control_feedback_payload_stays_summary_first() {
        let payload = json!({
            "status": "ok",
            "client_budget_host_control_feedback": {
                "project": {
                    "code": "amai",
                    "display_name": "Amai",
                    "repo_root": "/home/art/agent-memory-index"
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                },
                "thread_id": "thread-current",
                "command_id": "hotkey-window-open-current",
                "feedback_kind": "opened",
                "message_text": "Подтверждено: same-thread host control открылся.",
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 1.00%"
                }
            }
        });

        let feedback = &payload["client_budget_host_control_feedback"];
        assert_eq!(feedback["thread_id"], json!("thread-current"));
        assert_eq!(feedback["feedback_kind"], json!("opened"));
        assert_eq!(
            feedback["client_budget_reply_gate"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert!(payload.get("chat_notice").is_none());
        assert!(feedback.get("operator_notice").is_none());
        assert!(feedback.get("host_current_thread_control").is_none());
        assert!(feedback.get("launch").is_none());
        assert!(feedback.get("launch_targeting").is_none());
    }

    #[test]
    fn non_thread_host_control_feedback_payload_stays_summary_first() {
        let payload = json!({
            "status": "ok",
            "client_budget_host_control_feedback": {
                "project": {
                    "code": "amai",
                    "display_name": "Amai",
                    "repo_root": "/home/art/agent-memory-index"
                },
                "namespace": {
                    "code": "continuity",
                    "display_name": "Continuity"
                },
                "feedback_kind": "failed",
                "command_id": "hotkey-window-open-current",
                "client_budget_reply_gate": {
                    "reply_prefix": "5ч KPI: переплата 1.00%"
                },
                "operator_notice": {
                    "kind": "host_current_thread_control_feedback_failed",
                    "message_text": "Зафиксировано: same-thread host control не открылся.",
                    "feedback_kind": "failed",
                    "command_id": "hotkey-window-open-current"
                }
            },
            "chat_notice": {
                "kind": "host_current_thread_control_feedback_failed",
                "thread_id": null,
                "message_text": "Зафиксировано: same-thread host control не открылся.",
                "reply_prefix": "5ч KPI: переплата 1.00%",
                "feedback_kind": "failed",
                "command_id": "hotkey-window-open-current"
            }
        });

        let feedback = &payload["client_budget_host_control_feedback"];
        assert_eq!(feedback["feedback_kind"], json!("failed"));
        assert_eq!(
            feedback["operator_notice"]["kind"],
            json!("host_current_thread_control_feedback_failed")
        );
        assert_eq!(
            payload["chat_notice"]["reply_prefix"],
            json!("5ч KPI: переплата 1.00%")
        );
        assert!(feedback.get("message_text").is_none());
        assert!(feedback.get("host_current_thread_control").is_none());
        assert!(feedback.get("launch").is_none());
        assert!(feedback.get("launch_targeting").is_none());
        assert!(payload["chat_notice"].get("host_launch").is_none());
        assert!(
            payload["chat_notice"]
                .get("host_current_thread_control")
                .is_none()
        );
    }

    #[test]
    fn compact_cli_client_budget_gate_payload_keeps_personal_kpi_metadata() {
        let guard = json!({
            "status_label": "ok",
            "reply_prefix": "5ч KPI: н/д",
            "global_reply_prefix": "5ч KPI: экономия 8.00%",
            "reply_prefix_source": "personal_agent_5h_kpi",
            "observed_at_epoch_ms": 1_000,
            "max_guard_age_seconds": 10,
            "reply_execution_gate": {
                "action_kind": "continue_current_chat",
                "blocking": false,
                "must_rotate_before_reply": false,
                "must_wait_for_budget_recovery_before_reply": false,
                "reply_budget_mode": "compact_high_signal",
                "reply_prefix": "5ч KPI: н/д",
                "global_reply_prefix": "5ч KPI: экономия 8.00%",
                "reply_prefix_source": "personal_agent_5h_kpi",
                "host_context_compaction_stage": "preserve",
                "host_context_compaction_preserve_active": true,
                "host_context_compaction_critical_regrowth_active": false,
                "preserves_return_obligation": false,
                "same_meter_pure_burn_turn_active": true,
                "action_bundle": {
                    "operator_flow": {
                        "primary_command_kind": "rotate_helper_command"
                    },
                    "host_current_thread_control": {
                        "command_id": "thread-overlay-open-current",
                        "button_label": "Open thread overlay"
                    }
                }
            }
        });

        let compact = super::compact_cli_client_budget_gate_payload(&guard);
        assert_eq!(compact["reply_prefix"], json!("5ч KPI: н/д"));
        assert_eq!(
            compact["global_reply_prefix"],
            json!("5ч KPI: экономия 8.00%")
        );
        assert_eq!(
            compact["reply_prefix_source"],
            json!("personal_agent_5h_kpi")
        );
        assert_eq!(
            compact["reply_execution_gate"]["global_reply_prefix"],
            json!("5ч KPI: экономия 8.00%")
        );
        assert_eq!(
            compact["reply_execution_gate"]["reply_prefix_source"],
            json!("personal_agent_5h_kpi")
        );
    }

    #[test]
    fn cached_exact_client_limit_refresh_needed_when_hourly_burn_is_stale() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "client_limit_hourly_burn": {
                        "status": "observed",
                        "latest_observed_at_epoch_ms": 1_000
                    }
                }
            }
        });

        assert!(super::cached_exact_client_limit_refresh_needed(
            &snapshot, 5_001, 3_000
        ));
        assert!(!super::cached_exact_client_limit_refresh_needed(
            &snapshot, 4_000, 3_000
        ));
    }

    #[test]
    fn compact_client_budget_request_uses_fresh_cached_snapshot() {
        let cache = ObserveCache {
            snapshot: Some(json!({"status": "ok"})),
            last_refresh_completed_epoch_ms: Some(
                super::now_epoch_ms()
                    .saturating_sub(COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS / 2),
            ),
            ..ObserveCache::default()
        };

        assert!(
            cache_snapshot_age_ms(&cache)
                .is_some_and(|age| age < COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS)
        );
        assert!(!compact_client_budget_snapshot_cache_too_old(
            cache_snapshot_age_ms(&cache)
        ));
    }

    #[test]
    fn compact_client_budget_request_rejects_stale_cached_snapshot() {
        let cache = ObserveCache {
            snapshot: Some(json!({"status": "ok"})),
            last_refresh_completed_epoch_ms: Some(
                super::now_epoch_ms()
                    .saturating_sub(COMPACT_CLIENT_BUDGET_REQUEST_MAX_CACHE_AGE_MS + 1),
            ),
            ..ObserveCache::default()
        };

        assert!(compact_client_budget_snapshot_cache_too_old(
            cache_snapshot_age_ms(&cache)
        ));
    }

    #[test]
    fn observe_refresh_stuck_when_refresh_in_progress_exceeds_timeout() {
        let cache = ObserveCache {
            refresh_in_progress: true,
            last_refresh_started_epoch_ms: Some(super::now_epoch_ms().saturating_sub(
                super::OBSERVE_REFRESH_TIMEOUT_MS + super::OBSERVE_REFRESH_STUCK_GRACE_MS + 1,
            )),
            ..ObserveCache::default()
        };

        assert!(super::observe_refresh_stuck(&cache));
    }

    #[test]
    fn observe_refresh_not_stuck_when_not_in_progress() {
        let cache = ObserveCache {
            last_refresh_started_epoch_ms: Some(super::now_epoch_ms().saturating_sub(
                super::OBSERVE_REFRESH_TIMEOUT_MS + super::OBSERVE_REFRESH_STUCK_GRACE_MS + 1,
            )),
            ..ObserveCache::default()
        };

        assert!(!super::observe_refresh_stuck(&cache));
    }

    #[test]
    fn artifact_cleanup_summary_is_fresh_within_interval() {
        let summary = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 9_500
            }
        });
        assert!(super::artifact_cleanup_summary_is_fresh(
            &summary, 10_000, 1_000
        ));
        assert!(!super::artifact_cleanup_summary_is_fresh(
            &summary, 10_501, 1_000
        ));
    }

    #[test]
    fn artifact_cleanup_summary_is_not_fresh_without_capture_timestamp() {
        let summary = json!({
            "artifact_cleanup": {}
        });
        assert!(!super::artifact_cleanup_summary_is_fresh(
            &summary, 10_000, 1_000
        ));
    }
