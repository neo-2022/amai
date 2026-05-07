use serde_json::{Value, json};

const APPROVAL_POLICY_VERSION: &str = "benchmark-measured-approval-v1";
const REVIEW_READY_REASON: &str = "explicit_human_signoff_required";
const EVIDENCE_INCOMPLETE_REASON: &str = "measured_approval_evidence_incomplete";
const PROMOTION_LAW_MISSING_REASON: &str = "promotion_law_missing";
const PROMOTION_LAW_UNEXPECTED_STATE_REASON: &str = "promotion_law_unexpected_state";

pub(crate) fn measured_approval_block(payload_root: &str, payload: &Value) -> Value {
    let root = &payload[payload_root];
    let statistics = &root["statistics"];
    let promotion_law = &root["promotion_law"];
    let promotion_law_state = promotion_law["state"].as_str().unwrap_or("state_missing");
    let promotion_law_ready = promotion_law_state == "candidate_ready_for_measured_approval";
    let baseline_pair_materialized = statistics["baseline_run_id"]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty())
        && statistics["candidate_run_id"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty());
    let drift_summary_measured = statistics["drift_summary"]["status"].as_str() == Some("measured");
    let success_rate_ci_measured =
        method_status(statistics, "success_rate_confidence_interval") == Some("measured");
    let score_delta_status = method_status(statistics, "score_delta_confidence_interval");
    let mean_delta_status = method_status(statistics, "mean_delta_confidence_interval");
    let primary_delta_metric_ready =
        score_delta_status == Some("measured") || mean_delta_status == Some("measured");
    let median_latency_ci_measured =
        method_status(statistics, "median_latency_delta_confidence_interval") == Some("measured");
    let p95_latency_ci_measured =
        method_status(statistics, "p95_latency_delta_confidence_interval") == Some("measured");
    let verdict_distribution_drift_measured =
        method_status(statistics, "verdict_distribution_drift") == Some("measured");
    let latency_distribution_drift_measured =
        method_status(statistics, "latency_distribution_drift") == Some("measured");
    let gate_failures = promotion_law["inputs"]["gate_failures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let gate_failures_absent = gate_failures.is_empty();

    let evidence_ready = baseline_pair_materialized
        && drift_summary_measured
        && success_rate_ci_measured
        && primary_delta_metric_ready
        && median_latency_ci_measured
        && p95_latency_ci_measured
        && verdict_distribution_drift_measured
        && latency_distribution_drift_measured
        && gate_failures_absent;

    let measured_methods = statistics["drift_summary"]["measured_methods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let not_measured_methods = statistics["drift_summary"]["not_measured_methods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let not_applicable_methods = statistics["drift_summary"]["not_applicable_methods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();

    let (verdict, state, fail_closed, reason, required_action, blocking_reasons) =
        if promotion_law_state == "state_missing" {
            (
                "blocked",
                "blocked_promotion_law_missing",
                true,
                PROMOTION_LAW_MISSING_REASON,
                "materialize_promotion_law_before_human_review",
                vec!["promotion_law_state_missing".to_string()],
            )
        } else if promotion_law_state == "blocked_statistics_incomplete" {
            (
                "blocked",
                "blocked_statistics_incomplete",
                true,
                "statistics_incomplete",
                "materialize_complete_statistics_before_human_review",
                promotion_law["blocking_reasons"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect::<Vec<_>>(),
            )
        } else if promotion_law_state == "blocked_benchmark_gates" {
            (
                "blocked",
                "blocked_benchmark_gates",
                false,
                "benchmark_gates_not_met",
                "resolve_gate_failures_before_human_review",
                gate_failures.clone(),
            )
        } else if !promotion_law_ready {
            (
                "blocked",
                "blocked_promotion_law_unexpected_state",
                true,
                PROMOTION_LAW_UNEXPECTED_STATE_REASON,
                "reconcile_promotion_law_state_before_human_review",
                vec![format!(
                    "promotion_law_state_unexpected:{promotion_law_state}"
                )],
            )
        } else if !evidence_ready {
            let mut blocking_reasons = Vec::new();
            if !baseline_pair_materialized {
                blocking_reasons.push("baseline_pair_missing".to_string());
            }
            if !drift_summary_measured {
                blocking_reasons.push("drift_summary_not_measured".to_string());
            }
            if !success_rate_ci_measured {
                blocking_reasons.push("success_rate_ci_not_measured".to_string());
            }
            if !primary_delta_metric_ready {
                blocking_reasons.push("primary_delta_metric_not_measured".to_string());
            }
            if !median_latency_ci_measured {
                blocking_reasons.push("median_latency_ci_not_measured".to_string());
            }
            if !p95_latency_ci_measured {
                blocking_reasons.push("p95_latency_ci_not_measured".to_string());
            }
            if !verdict_distribution_drift_measured {
                blocking_reasons.push("verdict_distribution_drift_not_measured".to_string());
            }
            if !latency_distribution_drift_measured {
                blocking_reasons.push("latency_distribution_drift_not_measured".to_string());
            }
            if !gate_failures_absent {
                blocking_reasons.extend(gate_failures.clone());
            }
            (
                "blocked",
                "blocked_evidence_incomplete",
                true,
                EVIDENCE_INCOMPLETE_REASON,
                "complete_measured_approval_evidence_before_human_review",
                blocking_reasons,
            )
        } else {
            (
                "pending_human_review",
                "pending_human_review",
                false,
                REVIEW_READY_REASON,
                "explicit_human_signoff_required_before_promotion",
                Vec::new(),
            )
        };

    json!({
        "policy_version": APPROVAL_POLICY_VERSION,
        "verdict": verdict,
        "state": state,
        "fail_closed": fail_closed,
        "reason": reason,
        "review_packet_ready": verdict == "pending_human_review",
        "auto_promotion_allowed": false,
        "explicit_human_signoff_required": true,
        "required_action": required_action,
        "future_terminal_state": "approved_by_explicit_human_signal_only",
        "inputs": {
            "promotion_law_state": promotion_law_state,
            "baseline_run_id": statistics["baseline_run_id"].clone(),
            "candidate_run_id": statistics["candidate_run_id"].clone(),
            "drift_summary_status": statistics["drift_summary"]["status"].clone(),
            "measured_methods": measured_methods,
            "not_measured_methods": not_measured_methods,
            "not_applicable_methods": not_applicable_methods,
            "gate_failures": gate_failures,
        },
        "checks": {
            "promotion_law_ready": promotion_law_ready,
            "baseline_pair_materialized": baseline_pair_materialized,
            "drift_summary_measured": drift_summary_measured,
            "success_rate_ci_measured": success_rate_ci_measured,
            "primary_delta_metric_ready": primary_delta_metric_ready,
            "median_latency_ci_measured": median_latency_ci_measured,
            "p95_latency_ci_measured": p95_latency_ci_measured,
            "verdict_distribution_drift_measured": verdict_distribution_drift_measured,
            "latency_distribution_drift_measured": latency_distribution_drift_measured,
            "gate_failures_absent": gate_failures_absent,
        },
        "blocking_reasons": blocking_reasons,
    })
}

fn method_status<'a>(statistics: &'a Value, key: &str) -> Option<&'a str> {
    statistics["methods"][key]["status"].as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn measured_approval_blocks_when_statistics_are_incomplete() {
        let payload = json!({
            "memory_task_matrix": {
                "statistics": {
                    "baseline_run_id": null,
                    "candidate_run_id": "candidate",
                    "drift_summary": {
                        "status": "not_measured",
                        "measured_methods": [],
                        "not_measured_methods": ["score_delta_confidence_interval"],
                        "not_applicable_methods": [],
                    },
                    "methods": {}
                },
                "promotion_law": {
                    "state": "blocked_statistics_incomplete",
                    "blocking_reasons": ["baseline_run_id_missing"],
                    "inputs": { "gate_failures": [] }
                }
            }
        });
        let block = measured_approval_block("memory_task_matrix", &payload);
        assert_eq!(block["verdict"], json!("blocked"));
        assert_eq!(block["state"], json!("blocked_statistics_incomplete"));
        assert_eq!(block["fail_closed"], json!(true));
    }

    #[test]
    fn measured_approval_surfaces_pending_human_review_when_ready() {
        let payload = json!({
            "mcp_task_matrix": {
                "statistics": {
                    "baseline_run_id": "baseline",
                    "candidate_run_id": "candidate",
                    "drift_summary": {
                        "status": "measured",
                        "measured_methods": [
                            "success_rate_confidence_interval",
                            "mean_delta_confidence_interval",
                            "median_latency_delta_confidence_interval",
                            "p95_latency_delta_confidence_interval",
                            "verdict_distribution_drift",
                            "latency_distribution_drift"
                        ],
                        "not_measured_methods": [],
                        "not_applicable_methods": ["score_delta_confidence_interval"],
                    },
                    "methods": {
                        "success_rate_confidence_interval": { "status": "measured" },
                        "score_delta_confidence_interval": { "status": "not_applicable" },
                        "mean_delta_confidence_interval": { "status": "measured" },
                        "median_latency_delta_confidence_interval": { "status": "measured" },
                        "p95_latency_delta_confidence_interval": { "status": "measured" },
                        "verdict_distribution_drift": { "status": "measured" },
                        "latency_distribution_drift": { "status": "measured" }
                    }
                },
                "promotion_law": {
                    "state": "candidate_ready_for_measured_approval",
                    "inputs": { "gate_failures": [] }
                }
            }
        });
        let block = measured_approval_block("mcp_task_matrix", &payload);
        assert_eq!(block["verdict"], json!("pending_human_review"));
        assert_eq!(block["state"], json!("pending_human_review"));
        assert_eq!(block["review_packet_ready"], json!(true));
        assert_eq!(block["auto_promotion_allowed"], json!(false));
    }

    #[test]
    fn measured_approval_fail_closes_when_promotion_law_is_missing() {
        let payload = json!({
            "memory_task_matrix": {
                "statistics": {
                    "baseline_run_id": "baseline",
                    "candidate_run_id": "candidate",
                    "drift_summary": {
                        "status": "measured",
                        "measured_methods": [
                            "success_rate_confidence_interval",
                            "score_delta_confidence_interval",
                            "median_latency_delta_confidence_interval",
                            "p95_latency_delta_confidence_interval",
                            "verdict_distribution_drift",
                            "latency_distribution_drift"
                        ],
                        "not_measured_methods": [],
                        "not_applicable_methods": [],
                    },
                    "methods": {
                        "success_rate_confidence_interval": { "status": "measured" },
                        "score_delta_confidence_interval": { "status": "measured" },
                        "mean_delta_confidence_interval": { "status": "not_measured" },
                        "median_latency_delta_confidence_interval": { "status": "measured" },
                        "p95_latency_delta_confidence_interval": { "status": "measured" },
                        "verdict_distribution_drift": { "status": "measured" },
                        "latency_distribution_drift": { "status": "measured" }
                    }
                }
            }
        });

        let block = measured_approval_block("memory_task_matrix", &payload);
        assert_eq!(block["verdict"], json!("blocked"));
        assert_eq!(block["state"], json!("blocked_promotion_law_missing"));
        assert_eq!(block["fail_closed"], json!(true));
        assert_eq!(block["reason"], json!(PROMOTION_LAW_MISSING_REASON));
        assert_eq!(
            block["inputs"]["promotion_law_state"],
            json!("state_missing")
        );
        assert_eq!(block["checks"]["promotion_law_ready"], json!(false));
        assert_eq!(
            block["blocking_reasons"],
            json!(["promotion_law_state_missing"])
        );
    }

    #[test]
    fn measured_approval_fail_closes_when_promotion_law_state_is_unexpected() {
        let payload = json!({
            "mcp_task_matrix": {
                "statistics": {
                    "baseline_run_id": "baseline",
                    "candidate_run_id": "candidate",
                    "drift_summary": {
                        "status": "measured",
                        "measured_methods": [
                            "success_rate_confidence_interval",
                            "score_delta_confidence_interval",
                            "median_latency_delta_confidence_interval",
                            "p95_latency_delta_confidence_interval",
                            "verdict_distribution_drift",
                            "latency_distribution_drift"
                        ],
                        "not_measured_methods": [],
                        "not_applicable_methods": [],
                    },
                    "methods": {
                        "success_rate_confidence_interval": { "status": "measured" },
                        "score_delta_confidence_interval": { "status": "measured" },
                        "mean_delta_confidence_interval": { "status": "not_measured" },
                        "median_latency_delta_confidence_interval": { "status": "measured" },
                        "p95_latency_delta_confidence_interval": { "status": "measured" },
                        "verdict_distribution_drift": { "status": "measured" },
                        "latency_distribution_drift": { "status": "measured" }
                    }
                },
                "promotion_law": {
                    "state": "unexpected_future_state",
                    "inputs": { "gate_failures": [] }
                }
            }
        });

        let block = measured_approval_block("mcp_task_matrix", &payload);
        assert_eq!(block["verdict"], json!("blocked"));
        assert_eq!(
            block["state"],
            json!("blocked_promotion_law_unexpected_state")
        );
        assert_eq!(block["fail_closed"], json!(true));
        assert_eq!(
            block["reason"],
            json!(PROMOTION_LAW_UNEXPECTED_STATE_REASON)
        );
        assert_eq!(
            block["inputs"]["promotion_law_state"],
            json!("unexpected_future_state")
        );
        assert_eq!(block["checks"]["promotion_law_ready"], json!(false));
        assert_eq!(
            block["blocking_reasons"],
            json!(["promotion_law_state_unexpected:unexpected_future_state"])
        );
    }
}
