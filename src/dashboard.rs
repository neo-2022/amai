use crate::codex_threads;
use crate::config::{self, AppConfig};
use crate::continuity;
use crate::dashboard_format::*;
use crate::hardware_telemetry::{AcceleratorSummary, MachineSummary, collect_machine_summary};
use crate::onboarding;
use crate::working_state;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

mod dashboard_benchmark_cards;
mod dashboard_card_support;
mod dashboard_client_budget_diagnostics;
mod dashboard_client_budget_support;
mod dashboard_client_limit_alignment;
mod dashboard_context;
mod dashboard_current_session_budget_guard;
mod dashboard_hero_cards;
mod dashboard_live_latency_compare;
mod dashboard_live_response_latency_support;
mod dashboard_metric_status_support;
mod dashboard_overview;
mod dashboard_payload;
mod dashboard_renderer;
mod dashboard_runtime_support;
mod dashboard_service_cards;
mod dashboard_working_state_card;
use self::dashboard_benchmark_cards::build_benchmark_cards;
pub use self::dashboard_card_support::monitoring_url;
use self::dashboard_card_support::{
    card, card_with_rows, compact_dashboard_text, humanize_identifier, metric_row,
    metric_row_with_key, status_label, status_reason_tooltip, tcp_port_is_open, with_extra_class,
    with_status, with_status_label, with_status_tooltip, with_table_orientation,
};
use self::dashboard_client_budget_diagnostics::*;
#[allow(unused_imports)]
pub(crate) use self::dashboard_client_budget_diagnostics::{
    client_budget_root_cause_payload, client_budget_root_cause_payload_with_guard,
};
pub(crate) use self::dashboard_client_budget_support::client_budget_live_payload;
use self::dashboard_client_budget_support::*;
use self::dashboard_client_limit_alignment::*;
pub use self::dashboard_context::browser_base_url;
#[cfg(test)]
use self::dashboard_current_session_budget_guard::build_client_budget_reply_execution_gate_with_primary_command;
pub use self::dashboard_current_session_budget_guard::current_session_budget_guard;
pub(crate) use self::dashboard_hero_cards::build_active_agent_budget_session_card_from_surface;
use self::dashboard_hero_cards::{
    build_active_agent_budget_session_card, build_hero_cards,
    humanize_tracked_slice_exactness_value, humanize_tracked_slice_savings_value,
};
use self::dashboard_live_latency_compare::{
    live_latency_compare_card, live_latency_compare_status,
};
use self::dashboard_metric_status_support::*;
use self::dashboard_overview::{build_headline, build_top_cards};
pub use self::dashboard_payload::{build_live_summary_payload, build_payload};
pub use self::dashboard_renderer::render_html;
use self::dashboard_runtime_support::{
    build_glossary, build_governance_card, build_links, build_machine_cards, build_warnings,
};
use self::dashboard_service_cards::build_service_cards;
use self::dashboard_working_state_card::*;

pub use crate::dashboard_assets::{brand_lockup_svg, brand_mark_svg, favicon_ico};

#[cfg(test)]
fn compact_chat_selector_client_surface(restore_context: &Value) -> Value {
    dashboard_client_budget_support::compact_chat_selector_client_surface(restore_context)
}

const CLIENT_LIVE_CONTEXT_ROW_KEY: &str = "client_live_context";
const CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY: &str = "client_live_full_turn_savings";
const CLIENT_LIVE_LIMIT_ROW_KEY: &str = "client_live_limit";
const CLIENT_LIMIT_HOURLY_BURN_ROW_KEY: &str = "client_limit_hourly_burn";

#[cfg(test)]
mod tests {
    use super::{
        build_governance_card, build_hero_cards, build_links, build_live_summary_payload,
        build_machine_cards, format_ms, format_time_compare_pair, human_elapsed_ms, render_html,
    };
    use crate::config::AppConfig;
    use crate::working_state;
    use serde_json::{Value, json};

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
    fn dashboard_html_refresh_contract_is_live_on_focus_and_visibility() {
        let html = render_html(1000, None);
        assert!(html.contains("const TOOLTIP_HIDE_GRACE_MS = 220;"));
        assert!(html.contains("const DASHBOARD_BOOTSTRAP_PAYLOAD = null;"));
        assert!(html.contains("async function fetchWithTimeout(path, timeoutMs, init = {}) {"));
        assert!(html.contains(
            "renderDashboardPayload(chooseInitialDashboardPayload(DASHBOARD_BOOTSTRAP_PAYLOAD));"
        ));
        assert!(html.contains(
            "function chooseInitialDashboardPayload(bootstrapPayload) {\n      if (bootstrapPayload) {\n        return bootstrapPayload;\n      }\n      return null;\n    }"
        ));
        assert!(html.contains(
            "const DASHBOARD_PAYLOAD_CACHE_KEY = \"amai-human-dashboard-last-payload-v1\";"
        ));
        assert!(html.contains(
            "function scheduleHideTooltip(target = null, delayMs = TOOLTIP_HIDE_GRACE_MS) {"
        ));
        assert!(html.contains(
            "function isDocumentVisibleForRefresh() {\n      return document.visibilityState === \"visible\";\n    }"
        ));
        assert!(html.contains(
            "function scheduleForcedDashboardRefresh(reason = \"forced_refresh\", delayMs = 0) {"
        ));
        assert!(html.contains("document.addEventListener(\"visibilitychange\""));
        assert!(html.contains(
            "window.addEventListener(\"focus\", () => scheduleForcedDashboardRefresh(\"window_focus\"));"
        ));
        assert!(html.contains(
            "window.addEventListener(\"pageshow\", () => scheduleForcedDashboardRefresh(\"window_pageshow\"));"
        ));
        assert!(html.contains("const dashboardThreadId = new URLSearchParams(window.location.search).get(\"thread_id\");"));
        assert!(html.contains(
            "fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/client-budget-live\")"
        ));
        assert!(html.contains("scheduleForcedDashboardRefresh(\"initial_boot\");"));
        assert!(html.contains(
            "fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/dashboard-live-summary\")"
        ));
        assert!(html.contains("fetch(apiPathWithThreadHint(\"/api/client-budget-target\")"));
        assert!(html.contains("/api/client-budget-host-control-launch"));
        assert!(html.contains("/api/client-budget-host-control-feedback"));
        assert!(
            html.contains("fetchWithTimeout(\n          apiPathWithThreadHint(\"/api/dashboard\")")
        );
        assert!(html.contains("id=\"dashboard-toast\""));
        assert!(html.contains("tooltipLayer.addEventListener(\"mouseenter\", () => {"));
        assert!(!html.contains(
            "setInterval(() => syncDashboardLiveSummary(false), DASHBOARD_LIVE_SUMMARY_REFRESH_MS);"
        ));
        assert!(html.contains(
            "setInterval(() => syncClientBudgetLiveRows(false), CLIENT_BUDGET_LIVE_REFRESH_MS);"
        ));
        assert!(!html.contains("setInterval(() => loadDashboard(false), REFRESH_MS);"));
        assert!(!html.contains("syncActiveAgentBudgetLiveCard(false)"));
        assert!(!html.contains("fetchActiveAgentBudgetLivePayload(force = false)"));
        assert!(!html.contains(
            "async function fetchClientBudgetLivePayload(force = false) {\n      if (!force && isRefreshPaused()) {"
        ));
        assert!(!html.contains("INTERACTION_HOLD_SELECTOR"));
    }

    #[test]
    fn dashboard_html_contains_agent_rename_endpoint_and_inline_tooltip_trigger() {
        let html = render_html(1000, None);
        assert!(html.contains("/api/agent-display-name"));
        assert!(html.contains("content.className = \"tooltip-inline-trigger has-tooltip\";"));
    }

    #[test]
    fn elapsed_label_is_compact() {
        assert_eq!(human_elapsed_ms(30_000), "меньше минуты");
        assert_eq!(human_elapsed_ms(61_000), "1 мин.");
        assert_eq!(human_elapsed_ms(3_720_000), "1 ч. 2 мин.");
    }

    #[test]
    fn format_ms_uses_dashboard_timing_policy_from_snapshot() {
        let snapshot = json!({
            "thresholds": {
                "dashboard": {
                    "timing_format": {
                        "switch_to_nanoseconds_below_ms": 0.0005,
                        "switch_to_microseconds_below_ms": 2.0,
                        "switch_to_seconds_at_or_above_ms": 1000.0,
                        "non_positive_floor_label": "below timer floor",
                        "seconds_suffix": "secs",
                        "milliseconds_suffix": "millis",
                        "microseconds_suffix": "micros",
                        "nanoseconds_suffix": "nanos",
                        "seconds_decimals": 2,
                        "milliseconds_decimals": 2,
                        "microseconds_decimals": 1,
                        "nanoseconds_decimals": 0
                    }
                }
            }
        });

        assert_eq!(format_ms(&snapshot, Some(0.0)), "below timer floor");
        assert_eq!(format_ms(&snapshot, Some(0.0004)), "400 nanos");
        assert_eq!(format_ms(&snapshot, Some(0.0015)), "1.5 micros");
        assert_eq!(format_ms(&snapshot, Some(2.3456)), "2.35 millis");
        assert_eq!(format_ms(&snapshot, Some(2345.6)), "2.35 secs");
    }

    #[test]
    fn format_ms_falls_back_to_default_dashboard_timing_policy_when_missing() {
        let snapshot = json!({});

        assert_eq!(format_ms(&snapshot, Some(0.0)), "0 ns");
        assert_eq!(format_ms(&snapshot, Some(0.0004)), "400 ns");
        assert_eq!(format_ms(&snapshot, Some(0.0015)), "1.5 µs");
        assert_eq!(format_ms(&snapshot, Some(2.3456)), "2.346 ms");
        assert_eq!(format_ms(&snapshot, Some(2345.6)), "2.346 s");
    }

    #[test]
    fn compare_time_pair_uses_one_row_unit_for_target_and_current() {
        let snapshot = json!({
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
                }
            }
        });

        assert_eq!(
            format_time_compare_pair(&snapshot, Some(1.0), Some(0.674), "<="),
            vec!["<= 1 ms".to_string(), "0.674 ms".to_string()]
        );
        assert_eq!(
            format_time_compare_pair(&snapshot, Some(0.015), Some(0.003226), "<="),
            vec!["<= 15 µs".to_string(), "3.226 µs".to_string()]
        );
        assert_eq!(
            format_time_compare_pair(&snapshot, Some(1.0), Some(0.000271), "<="),
            vec!["<= 1 ms".to_string(), "271 ns".to_string()]
        );
    }

    #[test]
    fn client_turn_pressure_guard_triggers_on_large_thread_with_weak_full_turn_share() {
        let snapshot = json!({
            "token_budget_report": {
                "token_budget_report": {
                    "current_session": {
                        "events_total": 4,
                        "counted_events": 2,
                        "verified_effective_saved_tokens": 445,
                        "verified_effective_savings_pct": 78.76,
                        "started_at_epoch_ms": 1,
                        "ended_at_epoch_ms": 2,
                        "median_recovery_tokens": 0.0,
                        "answer_like_rate": 50.0,
                        "answer_like_counted_events": 2,
                        "verified_answer_like_savings_pct": 78.76,
                        "verified_baseline_tokens": 565,
                        "verified_delivered_tokens": 120,
                        "verified_recovery_tokens": 0,
                        "excluded_events_count": 2,
                        "excluded_effective_saved_tokens": 0,
                        "total_naive_tokens": 565,
                        "total_context_tokens": 120,
                        "effective_savings_pct": 78.76,
                        "total_effective_saved_tokens": 445,
                        "total_recovery_tokens": 0
                    },
                    "rolling_window": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "lifetime": {
                        "events_total": 0,
                        "counted_events": 0
                    },
                    "statement_previews": {
                        "current_session": {
                            "verified_observed_whole_cycle_with_amai_tokens": 206274,
                            "client_limit_meter_alignment": {
                                "same_meter_as_client_limit": true,
                                "exact_pair_status": {
                                    "exact_pair_available": true
                                },
                                "strict_client_meter_slice": {
                                    "lower_bound_tokens": 206719
                                },
                                "explicit_boundary_surface": {
                                    "blocks_full_same_meter_equivalence": false
                                }
                            }
                        }
                    },
                    "statement_export_previews": {
                        "lifetime": {}
                    },
                    "client_live_meter": {
                        "status": "observed",
                        "thread_binding_state": "current_thread_bound",
                        "current_thread_bound": true,
                        "thread_id": "019d4eb1-3e92-75e3-b22b-2bdf21f13885",
                        "client_turn_total_tokens": 206274,
                        "latest_model_context_window": 258400,
                        "context_used_percent": 79.82739938080495,
                        "primary_limit_remaining_percent": 28.0,
                        "secondary_limit_remaining_percent": 78.0
                    },
                    "profile": {
                        "display_name": "Обычная рабочая машина"
                    }
                }
            }
        });

        let cards = build_hero_cards(&snapshot);
        assert_eq!(cards[0]["status"].as_str(), Some("critical"));
        assert_eq!(
            cards[0]["status_label"].as_str(),
            Some("сожми текущий чат сейчас")
        );
        assert!(
            cards[0]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("внешний лимит клиента уже горит быстрее")
        );
        let row = cards[0]["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["label"].as_str() == Some("Следующее действие"))
            .expect("next action row");
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
        assert!(
            row["value"]
                .as_str()
                .unwrap_or_default()
                .contains("проверь effect")
        );
    }

    #[test]
    fn client_turn_pressure_guard_stays_off_for_light_live_turn() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 95.0
        });
        let current_live_turn = json!({
            "status": "observed",
            "retrieval_context_pack_count": 1
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 22000,
            "latest_model_context_window": 258400,
            "context_used_percent": 8.51,
            "primary_limit_remaining_percent": 74.0,
            "secondary_limit_remaining_percent": 91.0
        });
        assert!(
            super::client_turn_pressure_guard(
                &meter,
                Some((22420, 22000, 420, 1.87)),
                &hourly_burn,
                &current_live_turn
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_triggers_on_nearly_exhausted_primary_limit_even_below_70pct() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 178971,
            "latest_model_context_window": 258400,
            "context_used_percent": 69.26,
            "primary_limit_remaining_percent": 3.0,
            "secondary_limit_remaining_percent": 71.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((179416, 178971, 445, 0.25)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_triggers_early_when_exact_full_turn_pair_is_missing() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 118116,
            "latest_model_context_window": 258400,
            "context_used_percent": 45.71,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard =
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert!(
            super::client_turn_pressure_tooltip(guard, None, false,).contains("слишком раздут")
        );
    }

    #[test]
    fn client_turn_pressure_tooltip_surfaces_same_thread_host_control_when_present() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 118116,
            "latest_model_context_window": 258400,
            "context_used_percent": 45.71,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard =
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .expect("pressure guard");
        let bundle = json!({
            "host_current_thread_control": working_state::build_host_current_thread_control_surface()
        });
        let tooltip = super::client_turn_pressure_tooltip(guard, Some(&bundle), false);
        assert!(tooltip.contains("same-thread host surface"));
        assert!(tooltip.contains("thread-overlay-open-current"));
    }

    #[test]
    fn client_turn_pressure_guard_triggers_when_exact_full_turn_savings_are_tiny() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 140921,
            "latest_model_context_window": 258400,
            "context_used_percent": 54.54,
            "primary_limit_remaining_percent": 61.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((141366, 140921, 445, 0.31)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_blocks_earlier_for_negligible_exact_savings() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 90000,
            "latest_model_context_window": 258400,
            "context_used_percent": 34.83,
            "primary_limit_remaining_percent": 82.0,
            "secondary_limit_remaining_percent": 95.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((90452, 90000, 452, 0.50)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_even_earlier_for_small_negligible_gain() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 65000,
            "latest_model_context_window": 258400,
            "context_used_percent": 25.15,
            "primary_limit_remaining_percent": 94.0,
            "secondary_limit_remaining_percent": 97.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((65320, 65000, 320, 0.49)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
    }

    #[test]
    fn client_turn_pressure_guard_escalates_to_critical_when_primary_budget_is_nearly_burned() {
        let hourly_burn = json!({});
        let current_live_turn = json!({});
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 88241,
            "latest_model_context_window": 258400,
            "context_used_percent": 34.15,
            "primary_limit_remaining_percent": 8.0,
            "secondary_limit_remaining_percent": 72.0
        });
        let guard =
            super::client_turn_pressure_guard(&meter, None, &hourly_burn, &current_live_turn)
                .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_early_when_5h_kpi_overspends_without_amai_activity() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 36.59
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 18200,
            "latest_model_context_window": 258400,
            "context_used_percent": 7.04,
            "primary_limit_remaining_percent": 64.0,
            "secondary_limit_remaining_percent": 82.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((0, 0, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_when_5h_kpi_overspends_with_weak_live_gain() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 111.87
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 85681,
                "with_amai_tokens": 84456,
                "saved_tokens": 1225,
                "saved_pct": 1.4297218753282526
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 84456,
            "latest_model_context_window": 258400,
            "context_used_percent": 32.68421052631579,
            "primary_limit_remaining_percent": 75.0,
            "secondary_limit_remaining_percent": 88.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((85681, 84456, 1225, 1.4297218753282526)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_overspend_large_thread_even_with_fresh_budget() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 48.53
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 128709,
                "with_amai_tokens": 127509,
                "saved_tokens": 1200,
                "saved_pct": 0.9323357284223408
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 127509,
            "latest_model_context_window": 258400,
            "context_used_percent": 50.65,
            "primary_limit_remaining_percent": 93.0,
            "secondary_limit_remaining_percent": 51.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((128709, 127509, 1200, 0.9323357284223408)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_huge_overspend_thread_before_primary_limit_softens()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 47.59
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 150842,
                "with_amai_tokens": 150104,
                "saved_tokens": 738,
                "saved_pct": 0.4892536561435144
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 150104,
            "latest_model_context_window": 258400,
            "context_used_percent": 58.09,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 29.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((150842, 150104, 738, 0.4892536561435144)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_now_for_huge_no_amai_thread_without_hourly_burn_surface()
    {
        let hourly_burn = json!({});
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 130000,
            "latest_model_context_window": 258400,
            "context_used_percent": 50.31,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((0, 0, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert!(guard.no_amai_activity_in_current_live_turn);
        assert_eq!(guard.hourly_burn_classification, None);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_large_no_amai_thread_without_hourly_burn_surface()
     {
        let hourly_burn = json!({});
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 76000,
            "latest_model_context_window": 258400,
            "context_used_percent": 30.44,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((0, 0, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert!(guard.no_amai_activity_in_current_live_turn);
        assert_eq!(guard.hourly_burn_classification, None);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_huge_no_amai_thread_when_exact_5h_kpi_is_below_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 14.07
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 85.0,
            "secondary_limit_remaining_percent": 5.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((196009, 196009, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_weak_exact_pair_thread_when_5h_kpi_is_below_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 42.89
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 87076,
                "with_amai_tokens": 85435,
                "saved_tokens": 1641,
                "saved_pct": 1.8845606137167532
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 85435,
            "latest_model_context_window": 258400,
            "context_used_percent": 33.06308049535604,
            "primary_limit_remaining_percent": 82.0,
            "secondary_limit_remaining_percent": 91.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((87076, 85435, 1641, 1.8845606137167532)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_high_context_thread_when_exact_pair_is_far_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "overspend",
            "kpi_percent": 20.19
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 507742,
                "with_amai_tokens": 177981,
                "saved_tokens": 329761,
                "saved_pct": 64.94656735113503
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 177981,
            "latest_model_context_window": 258400,
            "context_used_percent": 68.8780959752322,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((507742, 177981, 329761, 64.94656735113503)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("overspend"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_large_high_context_thread_when_exact_pair_is_below_90_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 88.4
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 180000,
                "with_amai_tokens": 25000,
                "saved_tokens": 155000,
                "saved_pct": 86.11111111111111
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 25000,
            "latest_model_context_window": 258400,
            "context_used_percent": 55.0,
            "primary_limit_remaining_percent": 92.0,
            "secondary_limit_remaining_percent": 98.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((180000, 25000, 155000, 86.11111111111111)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_early_when_exact_pair_is_below_90_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 82.5
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 90000,
                "with_amai_tokens": 18000,
                "saved_tokens": 72000,
                "saved_pct": 80.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 45000,
            "latest_model_context_window": 258400,
            "context_used_percent": 18.2,
            "primary_limit_remaining_percent": 94.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((90000, 18000, 72000, 80.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_on_small_thread_when_exact_pair_and_5h_kpi_are_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 87.1
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 62000,
                "with_amai_tokens": 10200,
                "saved_tokens": 51800,
                "saved_pct": 83.54838709677419
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 10200,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.05,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((62000, 10200, 51800, 83.54838709677419)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_on_moderate_thread_when_exact_pair_and_5h_kpi_are_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 84.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 110000,
                "with_amai_tokens": 19000,
                "saved_tokens": 91000,
                "saved_pct": 82.72727272727273
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 19000,
            "latest_model_context_window": 258400,
            "context_used_percent": 7.35,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((110000, 19000, 91000, 82.72727272727273)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(!guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_respects_custom_50_percent_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 62.0
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 30000,
                "with_amai_tokens": 12000,
                "saved_tokens": 18000,
                "saved_pct": 60.0
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 12000,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.65,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((30000, 12000, 18000, 60.0)),
                &hourly_burn,
                &current_live_turn,
                50,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_zero_target_disables_target_only_pressure() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 87.1
        });
        let current_live_turn = json!({
            "status": "exact_pair_materialized",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 62000,
                "with_amai_tokens": 10200,
                "saved_tokens": 51800,
                "saved_pct": 83.54838709677419
            }
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 10200,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.05,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        assert!(
            super::client_turn_pressure_guard_with_target(
                &meter,
                Some((62000, 10200, 51800, 83.54838709677419)),
                &hourly_burn,
                &current_live_turn,
                0,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_small_no_amai_thread_when_5h_kpi_is_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 88.2
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            },
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 10500,
            "latest_model_context_window": 258400,
            "context_used_percent": 4.1,
            "primary_limit_remaining_percent": 97.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((10500, 10500, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_stays_off_for_huge_no_amai_thread_when_exact_5h_kpi_is_target_saving()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 94.07
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 85.0,
            "secondary_limit_remaining_percent": 5.0
        });
        assert!(
            super::client_turn_pressure_guard(
                &meter,
                Some((196009, 196009, 0, 0.0)),
                &hourly_burn,
                &current_live_turn,
            )
            .is_none()
        );
    }

    #[test]
    fn client_turn_pressure_guard_recommends_rotate_for_moderate_no_amai_thread_when_5h_kpi_is_below_target()
     {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 84.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            },
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 19000,
            "latest_model_context_window": 258400,
            "context_used_percent": 7.4,
            "primary_limit_remaining_percent": 96.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((19000, 19000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "alert");
        assert_eq!(guard.status_label, "новый чат рекомендован");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_early_no_amai_thread_when_5h_kpi_is_below_target() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 86.2
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "exact_pair_available": true,
            "exact_pair": {
                "without_amai_tokens": 0,
                "with_amai_tokens": 0,
                "saved_tokens": 0,
                "saved_pct": 0.0
            },
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 47000,
            "latest_model_context_window": 258400,
            "context_used_percent": 18.6,
            "primary_limit_remaining_percent": 94.0,
            "secondary_limit_remaining_percent": 99.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((47000, 47000, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
        assert_eq!(guard.hourly_burn_classification, Some("saving"));
        assert!(guard.no_amai_activity_in_current_live_turn);
    }

    #[test]
    fn client_turn_pressure_guard_rotates_for_huge_no_amai_thread_when_exact_5h_kpi_is_one_to_one()
    {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "one_to_one",
            "kpi_percent": 0.0
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 85.0,
            "secondary_limit_remaining_percent": 5.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((196009, 196009, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn client_turn_pressure_guard_keeps_critical_primary_limit_even_when_exact_5h_kpi_is_saving() {
        let hourly_burn = json!({
            "status": "observed",
            "classification": "saving",
            "kpi_percent": 14.07
        });
        let current_live_turn = json!({
            "status": "no_amai_activity_in_current_live_turn",
            "retrieval_context_pack_count": 0
        });
        let meter = json!({
            "status": "observed",
            "client_turn_total_tokens": 196009,
            "latest_model_context_window": 258400,
            "context_used_percent": 75.85487616099071,
            "primary_limit_remaining_percent": 18.0,
            "secondary_limit_remaining_percent": 5.0
        });
        let guard = super::client_turn_pressure_guard(
            &meter,
            Some((196009, 196009, 0, 0.0)),
            &hourly_burn,
            &current_live_turn,
        )
        .expect("pressure guard");
        assert_eq!(guard.severity, "critical");
        assert_eq!(guard.status_label, "новый чат нужен сейчас");
    }

    #[test]
    fn build_headline_prefers_active_agent_budget_average() {
        let snapshot = json!({
            "sla": {
                "summary": {
                    "pass": 19,
                    "alert": 0,
                    "critical": 0,
                    "unknown": 0
                }
            },
            "active_agent_budget": {
                "headline": {
                    "title": "Средний KPI активных агентов",
                    "value_text": "5ч KPI: экономия 40.00%",
                    "scope_label": "среднее по 2 активным агентам"
                }
            },
            "token_budget_report": {
                "token_budget_report": {
                    "headline": {
                        "title": "global fallback",
                        "value_percent": 12.0,
                        "scope_label": "fallback"
                    }
                }
            }
        });
        let headline = super::build_headline(&snapshot, 1775039106398);
        assert_eq!(
            headline["token_title"].as_str(),
            Some("Средний KPI активных агентов")
        );
        assert_eq!(
            headline["token_value"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(headline["token_scope"].as_str(), Some(""));
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
    }

    #[test]
    fn build_links_groups_api_and_monitoring_entries() {
        let links = build_links("http://127.0.0.1:9464");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0]["label"].as_str(), Some(""));
        assert_eq!(
            links[0]["items"].as_array().map(|items| items.len()),
            Some(4)
        );
        assert_eq!(links[1]["label"].as_str(), Some(""));
        assert_eq!(links[1]["note"].as_str(), Some(""));
        assert_eq!(
            links[1]["items"].as_array().map(|items| items.len()),
            Some(2)
        );
    }

    #[test]
    fn machine_cards_include_artifact_cleanup_visibility() {
        let snapshot = json!({
            "artifact_cleanup": {
                "captured_at_epoch_ms": 42,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "policy_retained_reclaimable_bytes": 0,
                "policy_retained_targets": [],
                "manual_only_reclaimable_bytes": 0,
                "manual_only_reclaimable_targets": [],
                "expired": 0,
                "kept_latest": 3,
                "protected": 1,
                "targets_scanned": 7,
                "aggressive_preview_selected": 4,
                "aggressive_preview_reclaimable_bytes": 35_604_527_338u64,
                "last_apply": {
                    "captured_at_epoch_ms": 41,
                    "mode": "aggressive",
                    "deleted": 30,
                    "reclaimed_bytes": 50_424_092_586u64
                },
                "repo_inventory": {
                    "repo_total_bytes": 230_200_000_000u64,
                    "cleanup_scope_bytes": 29_960_520_424u64,
                    "out_of_policy_bytes": 200_239_479_576u64,
                    "unmanaged_alert_triggered": true,
                    "large_unmanaged_roots": [
                        {
                            "path": "output/windows-vm-lab",
                            "unmanaged_bytes": 199_715_979_264u64
                        }
                    ],
                    "manual_only_targets": [
                        {
                            "path": "output/windows-vm-lab",
                            "ttl_hours": 168,
                            "keep_latest": 2,
                            "total_bytes": 199_715_979_264u64
                        }
                    ],
                    "unreadable_paths_count": 1
                }
            }
        });
        let cards = build_machine_cards(&snapshot, None, None);
        let cleanup_card = cards
            .iter()
            .find(|card| card["title"].as_str() == Some("Локальный мусор и retention"))
            .expect("cleanup card");
        assert_eq!(cleanup_card["status"].as_str(), Some("alert"));
        assert_eq!(
            cleanup_card["value"].as_str(),
            Some("186.49 GiB вне policy")
        );
        assert_eq!(
            cleanup_card["rows"][0]["value"].as_str(),
            Some("214.39 GiB")
        );
        assert_eq!(cleanup_card["rows"][1]["value"].as_str(), Some("27.90 GiB"));
        assert_eq!(
            cleanup_card["rows"][2]["value"].as_str(),
            Some("186.49 GiB")
        );
        assert_eq!(cleanup_card["rows"][4]["value"].as_str(), Some("33.16 GiB"));
        assert_eq!(
            cleanup_card["rows"][7]["value"].as_str(),
            Some("46.96 GiB (30, aggressive)")
        );
        assert_eq!(
            cleanup_card["rows"][11]["value"].as_str(),
            Some("output/windows-vm-lab (186.00 GiB)")
        );
        assert_eq!(
            cleanup_card["rows"][12]["value"].as_str(),
            Some("output/windows-vm-lab (186.00 GiB, ttl 168h, keep_latest 2)")
        );
    }

    #[test]
    fn governance_card_surfaces_forgetting_job_breakdown() {
        let snapshot = json!({
            "governance_surface": {
                "human_override_audit": {
                    "scope_override_events_total": 2,
                    "forgetting_audit_log_entries_total": 17
                },
                "wrong_link_rate": {
                    "open_conflict_count": 0
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 0
                },
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.125
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 7,
                    "cold_archive_job": 3,
                    "revalidation_job": 4,
                    "de_duplication_job": 2,
                    "summarization_job": 0
                }
            }
        });

        let card = build_governance_card(&snapshot);
        assert_eq!(card["title"], json!("Жизненный цикл памяти"));
        assert_eq!(card["status"], json!("pass"));
        assert_eq!(card["rows"][0]["value"], json!("7"));
        assert_eq!(card["rows"][1]["value"], json!("3"));
        assert_eq!(card["rows"][2]["value"], json!("4"));
        assert_eq!(card["rows"][3]["value"], json!("2"));
        assert_eq!(card["rows"][4]["value"], json!("0"));
    }

    #[test]
    fn governance_card_alert_headline_surfaces_quarantine_and_conflicts() {
        let snapshot = json!({
            "governance_surface": {
                "human_override_audit": {
                    "forgetting_audit_log_entries_total": 18
                },
                "wrong_link_rate": {
                    "open_conflict_count": 135
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 66,
                    "active_quarantine_breakdown": [
                        {
                            "quarantine_reason": "proof quarantine",
                            "entity_kind": "import_packet",
                            "source_kind": "import_packet_override",
                            "item_count": 60
                        }
                    ]
                },
                "open_conflict_breakdown": [
                    {
                        "summary": "truth conflict detected",
                        "source_kind": "verification_conflict_runtime",
                        "item_count": 120
                    }
                ],
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.0095
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 6,
                    "cold_archive_job": 6,
                    "revalidation_job": 6,
                    "de_duplication_job": 0,
                    "summarization_job": 0
                }
            }
        });

        let card = build_governance_card(&snapshot);
        assert_eq!(card["status"], json!("alert"));
        assert_eq!(card["value"], json!("66 в quarantine • 135 конфликтов"));
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("главный quarantine-класс")
        );
        assert!(
            card["note"]
                .as_str()
                .unwrap_or_default()
                .contains("главный конфликт")
        );
        assert_eq!(card["rows"][6]["label"], json!("Quarantine"));
        assert_eq!(card["rows"][6]["value"], json!("66"));
        assert_eq!(card["rows"][7]["label"], json!("Спорные"));
        assert_eq!(card["rows"][8]["label"], json!("Открытые конфликты"));
    }

    #[test]
    fn governance_card_uses_correct_russian_count_forms_in_alert_headline() {
        let snapshot = json!({
            "governance_surface": {
                "human_override_audit": {
                    "forgetting_audit_log_entries_total": 1
                },
                "wrong_link_rate": {
                    "open_conflict_count": 1
                },
                "poisoning_alert_count": {
                    "active_quarantine_items": 0,
                    "active_quarantine_breakdown": []
                },
                "open_conflict_breakdown": [
                    {
                        "summary": "cli get conflict 1",
                        "source_kind": "runtime_cli",
                        "item_count": 1
                    }
                ],
                "trust_state_distribution": {
                    "disputed_memory_items": 0
                },
                "stale_memory_error_rate": {
                    "rate": 0.0
                },
                "forgetting_job_breakdown": {
                    "pruning_job": 0,
                    "cold_archive_job": 0,
                    "revalidation_job": 0,
                    "de_duplication_job": 0,
                    "summarization_job": 0
                }
            }
        });

        let card = build_governance_card(&snapshot);
        assert_eq!(card["value"], json!("1 конфликт"));
    }

    #[test]
    fn client_limit_hourly_burn_row_embeds_same_thread_host_control_in_selector() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "saving",
                "reply_prefix": "5ч KPI: экономия 50.00%",
                "projected_primary_used_per_hour_percent": 10.0,
                "kpi_percent": 50.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 75.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 2000,
                "projected_reset_delta_minutes": 30.0
            }),
            50,
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "summary": "Operator confirmed same-thread overlay opened.",
                    "recorded_at_epoch_ms": 3000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "opened",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 12000,
                                "context_used_percent": 4.65,
                                "primary_limit_used_percent": 21
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 2000
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 6000,
                "client_turn_total_tokens": 14500,
                "context_used_percent": 5.61,
                "primary_limit_used_percent": 23
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 4500
            }),
            &working_state::build_host_current_thread_control_surface_for_thread(Some(
                "thread-current",
            )),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["host_current_thread_control"]["command_id"],
            json!("thread-overlay-open-current")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_button_label"],
            json!("Open thread overlay")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control"]["external_uri_launch"]["uri"],
            json!("vscode://openai.chatgpt/thread-overlay/thread-current")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_last_feedback_kind"],
            json!("opened")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_last_feedback_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("Operator confirmed same-thread overlay opened.")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_effect_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("thread overlay")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_uses_surface_driven_compact_window_text() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "saving",
                "reply_prefix": "5ч KPI: экономия 50.00%",
                "projected_primary_used_per_hour_percent": 10.0,
                "kpi_percent": 50.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 75.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 2000,
                "projected_reset_delta_minutes": 30.0
            }),
            50,
            &json!({}),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 6000,
                "client_turn_total_tokens": 15000,
                "context_used_percent": 5.8,
                "primary_limit_used_percent": 24
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 4800
            }),
            &working_state::build_host_current_thread_control_surface_for_thread_and_stage(
                Some("thread-current"),
                working_state::HostContextCompactionStage::Preserve,
            ),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["host_current_thread_control"]["command_id"],
            json!("hotkey-window-open-current")
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_button_label"],
            json!("Open compact window")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_intro"]
                .as_str()
                .unwrap_or_default()
                .contains("compact-window")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_notice_message"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
        assert!(
            row["target_selector"]["host_current_thread_control_ack_intro"]
                .as_str()
                .unwrap_or_default()
                .contains("compact window")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_embeds_compact_chat_client_surface_assist() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "saving",
                "reply_prefix": "5ч KPI: экономия 50.00%",
                "projected_primary_used_per_hour_percent": 10.0,
                "kpi_percent": 50.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 75.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 2000,
                "projected_reset_delta_minutes": 30.0
            }),
            50,
            &json!({
                "project": {
                    "repo_root": env!("CARGO_MANIFEST_DIR")
                }
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 6000,
                "client_turn_total_tokens": 15000,
                "context_used_percent": 5.8,
                "primary_limit_used_percent": 24
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 4800
            }),
            &working_state::build_host_current_thread_control_surface_for_thread_and_stage(
                Some("thread-current"),
                working_state::HostContextCompactionStage::Preserve,
            ),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["compact_chat_required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            row["target_selector"]["compact_chat_note"]
                .as_str()
                .unwrap_or_default()
                .contains("clean")
        );
        assert!(
            row["target_selector"]["compact_chat_assist_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("./scripts/reconnect_local.sh --client")
        );
        assert!(
            row["target_selector"]["compact_chat_reconnect_bootstrap_command"]
                .as_str()
                .unwrap_or_default()
                .contains("./scripts/amai_exec.sh bootstrap reconnect --client")
        );
    }

    #[test]
    fn compact_chat_selector_client_surface_falls_back_to_discovered_repo_root() {
        let surface = super::compact_chat_selector_client_surface(&json!({}));
        assert!(
            surface["display_name"]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty())
        );
        assert!(
            surface["reconnect_shell_command"]
                .as_str()
                .unwrap_or_default()
                .contains("./scripts/reconnect_local.sh --client")
        );
    }

    #[test]
    fn host_current_thread_control_effect_recommends_rotate_fallback_after_failed_compact_window() {
        let effect = super::host_current_thread_control_effect_payload(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 3000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9000,
                "client_turn_total_tokens": 160500,
                "context_used_percent": 53.5,
                "primary_limit_used_percent": 66
            }),
            &json!({
                "stage": "critical_regrowth",
                "growth_since_compaction_tokens": 52000
            }),
        );
        assert_eq!(
            effect["effect_verdict"],
            json!("full_scale_client_burn_worsened_rotate_fallback_recommended")
        );
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(false));
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("полный 5ч burn")
        );
    }

    #[test]
    fn host_current_thread_control_effect_recommends_overlay_trial_during_critical_regrowth() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 3000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9000,
                "client_turn_total_tokens": 106500,
                "context_used_percent": 42.6,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "critical_regrowth",
                "growth_since_compaction_tokens": 26530
            }),
            Some("hotkey-window-open-current"),
        );
        assert_eq!(
            effect["effect_verdict"],
            json!("critical_regrowth_overlay_trial_recommended")
        );
        assert_eq!(effect["overlay_trial_recommended"], json!(true));
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("overlay")
        );
    }

    #[test]
    fn host_current_thread_control_effect_marks_recent_baseline_as_measurement_pending() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 8_500,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9_000,
                "client_turn_total_tokens": 101000,
                "context_used_percent": 40.2,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 20800
            }),
            Some("hotkey-window-open-current"),
        );
        assert_eq!(effect["measurement_pending"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(false));
        assert_eq!(effect["effect_verdict"], json!("measurement_pending"));
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("дождись измеримого effect")
        );
    }

    #[test]
    fn host_current_thread_control_effect_clears_measurement_pending_after_short_idle_window() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "opened",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "inactive"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 31_500,
                "client_turn_total_tokens": 100000,
                "context_used_percent": 40.0,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "inactive",
                "growth_since_compaction_tokens": 20000
            }),
            Some("thread-overlay-open-current"),
        );
        assert_eq!(effect["measurement_pending"], json!(false));
        assert_eq!(effect["measurement_sufficient"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(true));
        assert_eq!(
            effect["effect_verdict"],
            json!("opened_overlay_surface_observed")
        );
    }

    #[test]
    fn host_current_thread_control_effect_marks_verified_compaction_after_requested_feedback() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "compaction_count": 2,
                                "compacted_at_epoch_ms": 900,
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 300_000,
                "client_turn_total_tokens": 101000,
                "context_used_percent": 40.2,
                "primary_limit_used_percent": 61
            }),
            &json!({
                "stage": "preserve",
                "compaction_count": 3,
                "compacted_at_epoch_ms": 200_000,
                "growth_since_compaction_tokens": 20800
            }),
            Some("thread-overlay-open-current"),
        );
        assert_eq!(
            effect["verified_host_compaction_observed_after_feedback"],
            json!(true)
        );
        assert_eq!(effect["compaction_count_delta"], json!(1));
    }

    #[test]
    fn host_current_thread_control_effect_recommends_rotate_when_full_scale_burn_worsens() {
        let effect = super::host_current_thread_control_effect_payload_for_command(
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 165_000,
                                "context_used_percent": 63.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 601_000,
                "client_turn_total_tokens": 93_000,
                "context_used_percent": 36.0,
                "primary_limit_used_percent": 40
            }),
            &json!({
                "stage": "critical_regrowth",
                "growth_since_compaction_tokens": 0
            }),
            Some("thread-overlay-open-current"),
        );
        assert_eq!(
            effect["effect_verdict"],
            json!("full_scale_client_burn_worsened_rotate_fallback_recommended")
        );
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["material_compaction_gain_observed"], json!(false));
        assert_eq!(effect["retry_allowed"], json!(false));
        assert!(
            effect["summary"]
                .as_str()
                .unwrap_or_default()
                .contains("против идеального темпа")
        );
        assert!(
            effect["verdict_summary"]
                .as_str()
                .unwrap_or_default()
                .contains("полный 5ч burn")
        );
    }

    #[test]
    fn client_limit_hourly_burn_row_blocks_same_thread_retry_while_measurement_pending() {
        let row = super::client_limit_hourly_burn_metric_row(
            &json!({
                "status": "observed",
                "classification": "overspend",
                "reply_prefix": "5ч KPI: переплата 10.00%",
                "projected_primary_used_per_hour_percent": 12.0,
                "kpi_percent": 10.0,
                "remaining_window_minutes": 30.0,
                "actual_remaining_percent": 40.0,
                "ideal_remaining_percent": 50.0,
                "latest_observed_at_epoch_ms": 9_000,
                "projected_reset_delta_minutes": -10.0
            }),
            90,
            &json!({
                "recent_actions": [{
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 8_500,
                    "summary": "Requested compact window launch via host current-thread control.",
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "preserve"
                            }
                        }
                    }
                }]
            }),
            &json!({
                "thread_id": "thread-current",
                "ended_at_epoch_ms": 9_000,
                "client_turn_total_tokens": 101000,
                "context_used_percent": 40.2,
                "primary_limit_used_percent": 60
            }),
            &json!({
                "stage": "preserve",
                "growth_since_compaction_tokens": 20800
            }),
            &working_state::build_host_current_thread_control_surface_for_thread_and_stage(
                Some("thread-current"),
                working_state::HostContextCompactionStage::Preserve,
            ),
        )
        .expect("hourly burn row");
        assert_eq!(
            row["target_selector"]["host_current_thread_control_retry_allowed"],
            json!(false)
        );
        assert_eq!(
            row["target_selector"]["host_current_thread_control_measurement_pending"],
            json!(true)
        );
        assert!(
            row["target_selector"]["host_current_thread_control_retry_blocked_reason"]
                .as_str()
                .unwrap_or_default()
                .contains("Requested compact window launch")
        );
    }

    #[test]
    fn reply_execution_gate_waits_for_same_thread_effect_when_retry_is_blocked() {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "сожми текущий чат сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
            false,
            Some("thread-current"),
            Some("thread-overlay-open-current"),
            &json!({
                "retry_allowed": false,
                "retry_blocked_reason": "Requested same-thread overlay launch via host current-thread control.",
                "measurement_pending": false,
                "effect_verdict": "requested_overlay_surface_observed",
                "summary": "Overlay request is still active."
            }),
            false,
            Some("Requested same-thread overlay launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_bundle"]["host_current_thread_control"]["retry_allowed"],
            json!(false)
        );
        assert_eq!(
            gate["action_kind"],
            json!("wait_for_same_thread_effect_measurement")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("wait_for_same_thread_effect_measurement")
        );
        assert_eq!(
            gate["must_wait_for_same_thread_effect_measurement_before_reply"],
            json!(true)
        );
        assert!(gate["action_bundle"]["operator_flow"]["primary_command"].is_null());
        assert_eq!(
            gate["action_bundle"]["measurement_before_retry_required"],
            json!(true)
        );
    }

    #[test]
    fn reply_execution_gate_requests_feedback_confirmation_when_retry_is_blocked_by_pending_feedback()
     {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "сожми текущий чат сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
            false,
            Some("thread-current"),
            Some("thread-overlay-open-current"),
            &json!({
                "retry_allowed": false,
                "retry_blocked_reason": "Requested same-thread overlay launch via host current-thread control.",
                "measurement_pending": false,
                "effect_verdict": "requested_overlay_surface_observed",
                "summary": "Overlay request is still active."
            }),
            true,
            Some("Requested same-thread overlay launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["must_confirm_same_thread_host_control_feedback_before_reply"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["feedback_confirmation_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"],
            json!(true)
        );
    }

    #[test]
    fn reply_execution_gate_skips_feedback_confirmation_after_verified_host_compaction() {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "сожми текущий чат сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            true,
            true,
            false,
            Some("thread-current"),
            Some("thread-overlay-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "verified_host_compaction_observed_after_feedback": true,
                "summary": "Real host compaction already observed after baseline."
            }),
            false,
            Some("Requested same-thread overlay launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_bundle"]["feedback_confirmation_before_retry_required"],
            Value::Null
        );
        assert_eq!(
            gate["action_bundle"]["measurement_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("wait_for_same_thread_effect_measurement")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["same_thread_feedback_confirmation_required"],
            Value::Null
        );
    }

    #[test]
    fn reply_execution_gate_keeps_rotate_order_when_same_thread_retry_is_disallowed_after_rotate_selection()
     {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "новый чат нужен сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            false,
            true,
            false,
            Some("thread-current"),
            Some("hotkey-window-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "verified_host_compaction_observed_after_feedback": true,
                "summary": "Surface already failed; rotate is primary."
            }),
            false,
            Some("Requested same-thread compact window launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("rotate_helper_command")
        );
        assert_eq!(
            gate["action_bundle"]["measurement_before_retry_required"],
            Value::Null
        );
        assert_eq!(
            gate["action_bundle"]["order"],
            json!([
                "run_rotate_helper",
                "open_fresh_chat",
                "run_continuity_startup"
            ])
        );
    }

    #[test]
    fn reply_execution_gate_requests_feedback_confirmation_before_rotate_when_same_thread_feedback_is_pending()
     {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "новый чат нужен сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            false,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            false,
            true,
            false,
            Some("thread-current"),
            Some("hotkey-window-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "requested_compact_surface_observed",
                "summary": "Requested compact window launch via host current-thread control."
            }),
            true,
            Some("Requested same-thread compact window launch via host current-thread control."),
        );
        assert_eq!(
            gate["action_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["action_bundle"]["operator_flow"]["primary_command_kind"],
            json!("confirm_same_thread_host_control_feedback")
        );
        assert_eq!(
            gate["must_confirm_same_thread_host_control_feedback_before_reply"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["feedback_confirmation_before_retry_required"],
            json!(true)
        );
        assert_eq!(
            gate["action_bundle"]["order"],
            json!([
                "confirm_same_thread_host_control_feedback",
                "run_rotate_helper",
                "open_fresh_chat",
                "run_continuity_startup"
            ])
        );
    }

    #[test]
    fn reply_execution_gate_hard_blocks_rotate_now_for_pure_burn_critical_regrowth() {
        let gate = super::build_client_budget_reply_execution_gate_with_primary_command(
            "critical",
            "новый чат нужен сейчас",
            Some("5ч KPI: переплата 10.00%"),
            Some("5ч KPI: переплата 10.00%"),
            "personal_agent_5h_kpi",
            Some(9_000),
            10,
            true,
            true,
            true,
            false,
            true,
            Some("amai"),
            Some("continuity"),
            Some("/home/art/agent-memory-index"),
            Some("headline"),
            Some("next step"),
            90,
            working_state::HostContextCompactionStage::CriticalRegrowth,
            false,
            true,
            true,
            Some("thread-current"),
            Some("hotkey-window-open-current"),
            &json!({
                "retry_allowed": false,
                "measurement_pending": false,
                "effect_verdict": "full_scale_client_burn_worsened_rotate_fallback_recommended",
                "verified_host_compaction_observed_after_feedback": true,
                "summary": "Surface already failed; rotate is primary."
            }),
            false,
            None,
        );
        assert_eq!(gate["action_kind"], json!("rotate_chat_for_client_budget"));
        assert_eq!(gate["blocking"], json!(true));
        assert_eq!(gate["must_rotate_before_reply"], json!(true));
        assert_eq!(
            gate["blocking_reply_contract"]["response_kind"],
            json!(working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND)
        );
        assert_eq!(
            gate["reason"],
            json!("client_budget_guard_pure_burn_rotate_now")
        );
    }

    #[test]
    fn selected_host_current_thread_control_state_prefers_lower_regrowth_rate_in_critical_stage() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 100000,
                                "context_used_percent": 40.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 20000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 120000,
                                "context_used_percent": 48.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 32000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9000,
            "client_turn_total_tokens": 124000,
            "context_used_percent": 49.2,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 36000
        });
        let (surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(surface["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(
            surface["selection_reason"],
            json!("critical_regrowth_try_overlay")
        );
        assert_eq!(effect["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(same_thread_preferred, true);
    }

    #[test]
    fn selected_host_current_thread_control_state_drops_same_thread_preference_only_after_verified_failure_on_both_surfaces()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 170_000,
                                "context_used_percent": 65.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 80_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 1_500
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 2_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 160_000,
                                "context_used_percent": 62.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 2_500
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 602_000,
            "client_turn_total_tokens": 188_000,
            "context_used_percent": 72.5,
            "primary_limit_used_percent": 40
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 95_000,
            "compaction_count": 2,
            "compacted_at_epoch_ms": 3_000
        });
        let (_surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, false);
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(false));
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_same_thread_preference_when_verified_compaction_has_material_gain()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 170_000,
                                "context_used_percent": 65.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 80_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 1_500
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 2_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 160_000,
                                "context_used_percent": 62.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth",
                                "compaction_count": 1,
                                "compacted_at_epoch_ms": 2_500
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 602_000,
            "client_turn_total_tokens": 92_000,
            "context_used_percent": 35.5,
            "primary_limit_used_percent": 40
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 0,
            "compaction_count": 2,
            "compacted_at_epoch_ms": 3_000
        });
        let (_surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, true);
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(effect["full_scale_client_burn_worsened"], json!(true));
        assert_eq!(effect["material_compaction_gain_observed"], json!(true));
        assert_eq!(effect["retry_allowed"], json!(true));
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_same_thread_preference_when_both_surfaces_only_have_observational_rotate_fallback()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 1_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 170_000,
                                "context_used_percent": 65.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 80_000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 2_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 160_000,
                                "context_used_percent": 62.0,
                                "primary_limit_used_percent": 20
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 60_000,
                                "stage": "critical_regrowth"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 602_000,
            "client_turn_total_tokens": 92_000,
            "context_used_percent": 35.5,
            "primary_limit_used_percent": 40
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 0
        });
        let (_surface, effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, true);
        assert_eq!(effect["rotate_fallback_recommended"], json!(true));
        assert_eq!(
            effect["verified_host_compaction_observed_after_feedback"],
            json!(false)
        );
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_same_thread_preference_for_oversized_critical_regrowth_without_verified_failure()
     {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": []
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9_000,
            "client_turn_total_tokens": 190_000,
            "context_used_percent": 81.0,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "critical_regrowth",
            "growth_since_compaction_tokens": 80_000,
            "regrowth_of_recovered_surface_ratio": 0.8
        });
        let (_surface, _effect, same_thread_preferred) =
            super::selected_host_current_thread_control_state(
                &report,
                &restore,
                &client_live_meter,
                &host_context_compaction,
            );
        assert_eq!(same_thread_preferred, true);
    }

    #[test]
    fn selected_host_current_thread_control_state_keeps_pending_feedback_command_selected() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 110000,
                                "context_used_percent": 46.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 26000,
                                "stage": "inactive"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9_000,
            "client_turn_total_tokens": 111000,
            "context_used_percent": 46.2,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "inactive",
            "growth_since_compaction_tokens": 26200
        });

        let (surface, effect, _) = super::selected_host_current_thread_control_state(
            &report,
            &restore,
            &client_live_meter,
            &host_context_compaction,
        );

        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID)
        );
        assert_eq!(effect["command_id"], json!("hotkey-window-open-current"));
        assert_eq!(effect["effect_verdict"], json!("measurement_pending"));
    }

    #[test]
    fn selected_host_current_thread_control_state_ignores_pending_feedback_from_other_thread() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-old",
                            "client_live_meter": {
                                "client_turn_total_tokens": 110000,
                                "context_used_percent": 46.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 26000,
                                "stage": "inactive"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 9_000,
            "client_turn_total_tokens": 111000,
            "context_used_percent": 46.2,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "inactive",
            "growth_since_compaction_tokens": 26200
        });

        let (surface, effect, _) = super::selected_host_current_thread_control_state(
            &report,
            &restore,
            &client_live_meter,
            &host_context_compaction,
        );

        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
        );
        assert_eq!(effect["command_id"], Value::Null);
    }

    #[test]
    fn selected_host_current_thread_control_state_prefers_newer_pending_feedback_command() {
        let report = json!({
            "client_live_meter": {
                "thread_id": "thread-current",
                "current_thread_bound": true
            }
        });
        let restore = json!({
            "thread_id": "thread-current",
            "recent_actions": [
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 9_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "thread-overlay-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 120000,
                                "context_used_percent": 48.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 30000,
                                "stage": "inactive"
                            }
                        }
                    }
                },
                {
                    "source_kind": "host_current_thread_control_feedback",
                    "recorded_at_epoch_ms": 5_000,
                    "host_current_thread_control_feedback": {
                        "feedback_kind": "requested",
                        "command_id": "hotkey-window-open-current",
                        "feedback_snapshot": {
                            "thread_id": "thread-current",
                            "client_live_meter": {
                                "client_turn_total_tokens": 110000,
                                "context_used_percent": 46.0,
                                "primary_limit_used_percent": 60
                            },
                            "host_context_compaction": {
                                "growth_since_compaction_tokens": 26000,
                                "stage": "inactive"
                            }
                        }
                    }
                }
            ]
        });
        let client_live_meter = json!({
            "thread_id": "thread-current",
            "ended_at_epoch_ms": 10_000,
            "client_turn_total_tokens": 120400,
            "context_used_percent": 48.1,
            "primary_limit_used_percent": 60
        });
        let host_context_compaction = json!({
            "stage": "inactive",
            "growth_since_compaction_tokens": 30100
        });

        let (surface, effect, _) = super::selected_host_current_thread_control_state(
            &report,
            &restore,
            &client_live_meter,
            &host_context_compaction,
        );

        assert_eq!(
            surface["command_id"],
            json!(working_state::HOST_CURRENT_THREAD_CONTROL_COMMAND_ID)
        );
        assert_eq!(effect["command_id"], json!("thread-overlay-open-current"));
        assert_eq!(effect["effect_verdict"], json!("measurement_pending"));
    }
}
