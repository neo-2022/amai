use super::*;

pub fn build_payload(
    cfg: &AppConfig,
    snapshot: &Value,
    bind: &str,
    refresh_ms: u64,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let install_state = dashboard_context::load_install_state(&repo_root).unwrap_or(None);
    let machine = collect_machine_summary(&repo_root).ok();
    let base_url = browser_base_url(bind);
    let captured_at_epoch_ms = snapshot["captured_at_epoch_ms"]
        .as_u64()
        .unwrap_or_default();
    let observe_refresh_total_ms = snapshot["observe_refresh"]["total_ms"].as_u64();
    let (observe_refresh_slowest_stage, observe_refresh_slowest_stage_ms) =
        slowest_observe_refresh_stage(snapshot);

    let top_cards = build_top_cards(snapshot);
    let live_compare_card = top_cards
        .iter()
        .find(|card| card["kind"].as_str() == Some("live_compare"))
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "meta": {
            "stack_name": cfg.stack_name,
            "package_version": env!("CARGO_PKG_VERSION"),
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "captured_at_label": human_timestamp(captured_at_epoch_ms),
            "refresh_ms": refresh_ms,
            "refresh_seconds": refresh_ms / 1000,
            "base_url": base_url,
            "observe_refresh_total_ms": observe_refresh_total_ms,
            "observe_refresh_slowest_stage": observe_refresh_slowest_stage,
            "observe_refresh_slowest_stage_ms": observe_refresh_slowest_stage_ms,
        },
        "headline": build_headline(snapshot, captured_at_epoch_ms),
        "hero_cards": build_hero_cards(snapshot),
        "top_cards": top_cards,
        "live_compare_card": live_compare_card,
        "benchmark_cards": build_benchmark_cards(snapshot),
        "client_budget_live": client_budget_live_payload(snapshot),
        "machine_cards": build_machine_cards(snapshot, machine.as_ref(), install_state.as_ref()),
        "service_cards": build_service_cards(snapshot),
        "warnings": build_warnings(snapshot, machine.as_ref()),
        "glossary": build_glossary(),
        "links": build_links(&base_url),
    }))
}

pub fn build_live_summary_payload(
    cfg: &AppConfig,
    snapshot: &Value,
    bind: &str,
    refresh_ms: u64,
) -> Result<Value> {
    let base_url = browser_base_url(bind);
    let captured_at_epoch_ms = snapshot["captured_at_epoch_ms"]
        .as_u64()
        .unwrap_or_default();
    let observe_refresh_total_ms = snapshot["observe_refresh"]["total_ms"].as_u64();
    let (observe_refresh_slowest_stage, observe_refresh_slowest_stage_ms) =
        slowest_observe_refresh_stage(snapshot);

    Ok(json!({
        "meta": {
            "stack_name": cfg.stack_name,
            "package_version": env!("CARGO_PKG_VERSION"),
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "captured_at_label": human_timestamp(captured_at_epoch_ms),
            "refresh_ms": refresh_ms,
            "refresh_seconds": refresh_ms / 1000,
            "base_url": base_url,
            "observe_refresh_total_ms": observe_refresh_total_ms,
            "observe_refresh_slowest_stage": observe_refresh_slowest_stage,
            "observe_refresh_slowest_stage_ms": observe_refresh_slowest_stage_ms,
        },
        "headline": build_headline(snapshot, captured_at_epoch_ms),
        "active_agent_card": build_active_agent_budget_session_card(snapshot),
        "top_cards": build_top_cards(snapshot),
        "service_cards": build_service_cards(snapshot),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            edge_cache_path: "/tmp/edge-cache-test.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            local_fast_cache_ttl_ms: 5_000,
        }
    }

    #[test]
    fn live_summary_payload_keeps_headline_and_active_agent_card_on_one_surface() {
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
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
                        "latency_slices": [
                            {
                                "state": "mixed",
                                "sample_count": 3,
                                "current_latency_ms": 1.7,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.4,
                                "p99_latency_ms": 2.4,
                                "max_latency_ms": 2.4
                            }
                        ]
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "amai" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "amai::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Dashboard live summary poller keeps headline and top cards fresh",
                    "next_step": "Keep headline and hero card on one live surface.",
                    "last_command": "context pack",
                    "last_results_summary": "Найдено: документов 0, символов 0.",
                    "latest_decision_trace": null,
                    "active_files": [],
                    "recent_queries": ["dashboard live summary"],
                    "restore_confidence": "preliminary"
                }
            },
            "agent_scope_activity": {
                "client_recent_window_minutes": 30,
                "client_recent_thread_count": 2,
                "client_recent_threads": [],
                "active_now_count": 2,
                "active_now_scopes": [
                    {
                        "agent_scope": "amai::continuity::default",
                        "owner_thread_id": "thread-a",
                        "heartbeat_at_epoch_ms": 1774239285880u64
                    },
                    {
                        "agent_scope": "bug_bounty::continuity::default",
                        "owner_thread_id": "thread-b",
                        "heartbeat_at_epoch_ms": 1774239200000u64
                    }
                ],
                "recent_scope_window_hours": 24,
                "recent_scope_count": 2,
                "recent_scopes": [
                    {
                        "agent_scope": "amai::continuity::default",
                        "captured_at_epoch_ms": 1774239285880u64
                    },
                    {
                        "agent_scope": "bug_bounty::continuity::default",
                        "captured_at_epoch_ms": 1774239200000u64
                    }
                ]
            },
            "active_agent_budget": {
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

        let payload = build_live_summary_payload(&test_config(), &snapshot, "127.0.0.1:9464", 1000)
            .expect("payload");
        assert_eq!(
            payload["headline"]["token_value"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(
            payload["active_agent_card"]["value"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(
            payload["active_agent_card"]["presentation_variant"].as_str(),
            Some("active_agent_budget_grouped_v3")
        );
        let top_cards = payload["top_cards"].as_array().expect("top cards");
        assert_eq!(top_cards.len(), 2);
        assert_eq!(top_cards[0]["title"].as_str(), Some("Скорость ответа"));
        assert_eq!(top_cards[1]["title"].as_str(), Some("Текущая работа"));
        assert!(payload["service_cards"].is_array());
    }

    #[test]
    fn dashboard_payload_exposes_live_compare_card_alias_from_top_cards() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
            "observe_refresh": {"total_ms": 12},
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
                        "target_p95_ms": 2.0,
                        "target_p99_ms": 3.0,
                        "target_max_ms": 5.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 100000,
                        "target_sample_count": 100000
                    },
                    "cold_live_table": {
                        "target_p50_ms": 2.0,
                        "target_p95_ms": 4.0,
                        "target_p99_ms": 6.0,
                        "target_max_ms": 10.0,
                        "live_readiness_sample_count": 100,
                        "benchmark_sample_count": 10000,
                        "target_sample_count": 10000
                    }
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "live_response_latency": {
                        "current_session": {
                            "sample_count": 0,
                            "latency_slices": []
                        },
                        "rolling_window": {
                            "sample_count": 1,
                            "latency_slices": [
                                {
                                    "state": "cold",
                                    "sample_count": 1,
                                    "p50_latency_ms": 2.0,
                                    "p95_latency_ms": 2.0,
                                    "p99_latency_ms": 2.0,
                                    "max_latency_ms": 2.0
                                }
                            ]
                        }
                    },
                    "current_live_turn": {
                        "status": "no_amai_activity_in_current_live_turn"
                    }
                }
            }
        });

        let payload =
            build_payload(&test_config(), &snapshot, "127.0.0.1:9464", 1000).expect("payload");

        assert_eq!(
            payload["live_compare_card"]["kind"].as_str(),
            Some("live_compare")
        );
        assert_eq!(
            payload["live_compare_card"]["title"].as_str(),
            Some("Скорость ответа")
        );
        assert!(payload["client_budget_live"].is_object());
    }
}
