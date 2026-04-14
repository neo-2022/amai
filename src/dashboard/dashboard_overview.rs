use super::*;

pub(super) fn build_headline(snapshot: &Value, captured_at_epoch_ms: u64) -> Value {
    let pass = snapshot["sla"]["summary"]["pass"].as_u64().unwrap_or(0);
    let alert = snapshot["sla"]["summary"]["alert"].as_u64().unwrap_or(0);
    let critical = snapshot["sla"]["summary"]["critical"].as_u64().unwrap_or(0);
    let unknown = snapshot["sla"]["summary"]["unknown"].as_u64().unwrap_or(0);
    let token_headline = &snapshot["token_budget_report"]["token_budget_report"]["headline"];
    let active_agent_headline = &snapshot["active_agent_budget"]["headline"];
    let sla_status = if critical > 0 {
        "critical"
    } else if alert > 0 {
        "alert"
    } else if unknown > 0 {
        "unknown"
    } else {
        "pass"
    };
    let live_status = live_latency_compare_status(snapshot);
    let status = combine_headline_statuses(sla_status, live_status);
    json!({
        "status": status,
        "status_label": headline_status_label(status),
        "status_reason": headline_status_reason(pass, alert, critical, unknown, live_status),
        "captured_at": human_timestamp(captured_at_epoch_ms),
        "summary": format!("SLA сейчас: pass={pass}, alert={alert}, critical={critical}, unknown={unknown}"),
        "token_title": active_agent_headline["title"]
            .as_str()
            .or_else(|| token_headline["title"].as_str())
            .unwrap_or("ещё нет данных"),
        "token_value": active_agent_headline["value_text"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format_percent(token_headline["value_percent"].as_f64())),
        "token_scope": if active_agent_headline.is_object() {
            ""
        } else {
            token_headline["scope_label"].as_str().unwrap_or("")
        },
    })
}

pub(super) fn build_top_cards(snapshot: &Value) -> Vec<Value> {
    vec![
        live_latency_compare_card(snapshot),
        working_state_live_card(snapshot),
    ]
}

fn headline_status_label(status: &str) -> &'static str {
    match status {
        "pass" => "система в норме",
        "alert" => "нужно внимание",
        "critical" => "есть критичные сигналы",
        "waiting" => "данных пока мало",
        _ => "данных пока мало",
    }
}

fn headline_status_reason(
    pass: u64,
    alert: u64,
    critical: u64,
    unknown: u64,
    live_status: &str,
) -> String {
    let mut base = if critical > 0 {
        format!("Критичных SLA-проверок: {critical}. Предупреждений: {alert}.")
    } else if alert > 0 {
        format!("SLA-предупреждений: {alert}. Критичных SLA-проверок нет.")
    } else if unknown > 0 {
        format!("Неопределённых SLA-проверок: {unknown}. Остальные зелёные: {pass}.")
    } else {
        format!("Все SLA-проверки зелёные: {pass}.")
    };

    match live_status {
        "critical" => {
            base.push_str(" Живой пользовательский поток сейчас в критичном состоянии.");
        }
        "alert" => {
            base.push_str(" Живой пользовательский поток сейчас требует внимания.");
        }
        "unknown" => {
            base.push_str(" По живому пользовательскому потоку пока недостаточно данных.");
        }
        _ => {}
    }

    base
}

fn combine_headline_statuses(sla_status: &str, live_status: &str) -> &'static str {
    match live_status {
        "critical" => "critical",
        "alert" => {
            if sla_status == "critical" {
                "critical"
            } else {
                "alert"
            }
        }
        _ => match sla_status {
            "pass" => "pass",
            "alert" => "alert",
            "critical" => "critical",
            _ => "unknown",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let headline = build_headline(&snapshot, 1775039106398);
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
    fn top_cards_split_live_retrieval_from_real_workline() {
        let snapshot = json!({
            "captured_at_epoch_ms": 1774239286880u64,
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
                    "current_session": {
                        "latency_slices": [
                            {
                                "state": "hot",
                                "sample_count": 100001,
                                "p50_latency_ms": 0.4,
                                "p95_latency_ms": 0.7,
                                "p99_latency_ms": 1.2,
                                "max_latency_ms": 2.5
                            },
                            {
                                "state": "cold",
                                "sample_count": 10001,
                                "p50_latency_ms": 1.2,
                                "p95_latency_ms": 2.1,
                                "p99_latency_ms": 3.4,
                                "max_latency_ms": 5.2
                            }
                        ]
                    }
                }
            },
            "latest_repo_working_state_restore": {
                "working_state_restore": {
                    "captured_at_epoch_ms": 1774239281880u64,
                    "project": { "code": "art" },
                    "namespace": { "code": "continuity" },
                    "agent_scope": "art::continuity::default",
                    "session_age_ms": 15u64,
                    "events_count": 3u64,
                    "current_goal": "Amai observability guardrail proof materialized",
                    "next_step": "Вывести guardrail verdict в dashboard/service layer.",
                    "last_command": "continuity handoff",
                    "last_results_summary": "Зафиксирован handoff для art :: continuity",
                    "latest_decision_trace": {
                        "included": [
                            {
                                "strategy": "exact_documents",
                                "count": 1,
                                "reason": "Нашлись точные document/path совпадения внутри видимого контура."
                            }
                        ],
                        "not_included": [
                            {
                                "strategy": "semantic_chunks",
                                "reason": "Semantic layer честно abstained и не добавил фрагменты."
                            }
                        ]
                    },
                    "active_files": [
                        "/home/art/agent-memory-index/src/observe.rs",
                        "/home/art/agent-memory-index/src/dashboard.rs"
                    ],
                    "recent_queries": [],
                    "restore_confidence": "preliminary"
                }
            },
            "agent_scope_activity": {
                "client_recent_window_minutes": 30,
                "client_recent_thread_count": 1,
                "client_recent_threads": [
                    {
                        "thread_id": "019d16f2-528d-7cc0-bcfe-8984f95f05c7",
                        "cwd": "/home/art/Art",
                        "rollout_path": "/home/art/.codex/sessions/2026/03/22/rollout-2026-03-22T22-07-52-019d16f2-528d-7cc0-bcfe-8984f95f05c7.jsonl",
                        "title": "продолжай по Amai continuity",
                        "agent_nickname": "Amai",
                        "agent_role": "continuity",
                        "model_provider": "openai",
                        "model": "gpt-5.4",
                        "reasoning_effort": "xhigh",
                        "updated_at_epoch_ms": 1774239285880u64
                    }
                ],
                "active_now_count": 1,
                "active_now_scopes": [
                    {
                        "agent_scope": "art::continuity::default",
                        "owner_thread_id": "019d16f2-528d-7cc0-bcfe-8984f95f05c7",
                        "heartbeat_at_epoch_ms": 1774239285880u64
                    }
                ],
                "recent_scope_window_hours": 24,
                "recent_scope_count": 3,
                "recent_scopes": [
                    {
                        "agent_scope": "art::continuity::default",
                        "captured_at_epoch_ms": 1774239285880u64
                    },
                    {
                        "agent_scope": "bug_bounty::continuity::default",
                        "captured_at_epoch_ms": 1774239200000u64
                    }
                ]
            }
        });

        let cards = build_top_cards(&snapshot);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0]["title"].as_str(), Some("Скорость ответа"));
        assert_eq!(cards[1]["title"].as_str(), Some("Текущая работа"));
        assert!(
            cards[0]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        );
        assert!(
            cards[1]["status_tooltip"]
                .as_str()
                .unwrap_or_default()
                .contains("Уверенность в этом рабочем снимке пока")
        );
        assert!(
            cards[1]["note"]
                .as_str()
                .unwrap_or_default()
                .contains("Короткая сводка по текущей работе")
        );
        assert!(cards[1]["rows"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["label"].as_str() == Some("Что дальше"))
        }));
    }
}
