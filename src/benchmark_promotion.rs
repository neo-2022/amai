use serde_json::{Value, json};
use uuid::Uuid;

const PROMOTION_LAW_VERSION: &str = "benchmark-promotion-law-v1";
const STATISTICS_INCOMPLETE_REASON: &str = "statistics_incomplete";
const BENCHMARK_GATES_NOT_MET_REASON: &str = "benchmark_gates_not_met";
const MEASURED_APPROVAL_PENDING_REASON: &str = "measured_approval_policy_not_materialized";

pub(crate) fn promotion_law_block(payload_root: &str, payload: &Value) -> Value {
    let root = &payload[payload_root];
    let statistics = &root["statistics"];
    let gate_failures = root["gate_failures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let statistics_version_supported =
        statistics["statistics_version"].as_str() == Some("benchmark-statistics-v1");
    let statistics_methods_complete =
        expected_statistics_methods(payload_root)
            .iter()
            .all(|(method_name, allowed_statuses)| {
                statistics_method_status_allowed(statistics, method_name, allowed_statuses)
            });
    let statistics_complete = statistics_version_supported
        && statistics["sample_size"].as_u64().unwrap_or(0) > 0
        && valid_run_id(&statistics["baseline_run_id"])
        && valid_run_id(&statistics["candidate_run_id"])
        && statistics["drift_summary"]["status"].as_str() == Some("measured")
        && statistics_methods_complete
        && statistics["promotion"]["fail_closed"].as_bool() == Some(false)
        && statistics["promotion"]["reason"].as_str()
            != Some("statistics_block_incomplete_for_promotion");

    let (state, fail_closed, reason, blocking_reasons, candidate_ready_for_measured_approval) =
        if !statistics_complete {
            (
                "blocked_statistics_incomplete",
                true,
                STATISTICS_INCOMPLETE_REASON,
                collect_statistics_blockers(payload_root, statistics),
                false,
            )
        } else if !gate_failures.is_empty() {
            (
                "blocked_benchmark_gates",
                true,
                BENCHMARK_GATES_NOT_MET_REASON,
                gate_failures.clone(),
                false,
            )
        } else {
            (
                "candidate_ready_for_measured_approval",
                false,
                MEASURED_APPROVAL_PENDING_REASON,
                Vec::new(),
                true,
            )
        };

    json!({
        "policy_version": PROMOTION_LAW_VERSION,
        "verdict": "not_promotable",
        "state": state,
        "fail_closed": fail_closed,
        "reason": reason,
        "candidate_ready_for_measured_approval": candidate_ready_for_measured_approval,
        "inputs": {
            "statistics_complete": statistics_complete,
            "statistics_version": statistics["statistics_version"].clone(),
            "sample_size": statistics["sample_size"].clone(),
            "baseline_run_id": statistics["baseline_run_id"].clone(),
            "candidate_run_id": statistics["candidate_run_id"].clone(),
            "drift_summary_status": statistics["drift_summary"]["status"].clone(),
            "statistics_methods_complete": statistics_methods_complete,
            "statistics_fail_closed": statistics["promotion"]["fail_closed"].clone(),
            "statistics_reason": statistics["promotion"]["reason"].clone(),
            "gate_failures": gate_failures,
        },
        "blocking_reasons": blocking_reasons,
    })
}

fn collect_statistics_blockers(payload_root: &str, statistics: &Value) -> Vec<String> {
    let mut blockers = statistics["promotion"]["blockers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    if statistics["statistics_version"].as_str() != Some("benchmark-statistics-v1") {
        blockers.push("statistics_version_missing_or_unsupported".to_string());
    }
    if statistics["sample_size"].as_u64().unwrap_or(0) == 0 {
        blockers.push("sample_size_missing_or_zero".to_string());
    }
    if !valid_run_id(&statistics["baseline_run_id"]) {
        blockers.push("baseline_run_id_missing_or_invalid".to_string());
    }
    if !valid_run_id(&statistics["candidate_run_id"]) {
        blockers.push("candidate_run_id_missing_or_invalid".to_string());
    }
    if statistics["drift_summary"]["status"].as_str() != Some("measured") {
        blockers.push("drift_summary_not_measured".to_string());
    }
    for (method_name, allowed_statuses) in expected_statistics_methods(payload_root) {
        if !statistics_method_status_allowed(statistics, method_name, allowed_statuses) {
            blockers.push(format!("{method_name}_missing_or_unaccepted_status"));
        }
    }
    if statistics["promotion"]["fail_closed"].as_bool() != Some(false) {
        blockers.push("statistics_fail_closed".to_string());
    }
    if statistics["promotion"]["reason"].as_str()
        == Some("statistics_block_incomplete_for_promotion")
    {
        blockers.push("statistics_incomplete_reason".to_string());
    }
    blockers.sort();
    blockers.dedup();
    if blockers.is_empty() {
        blockers.push("statistics_completeness_unknown".to_string());
    }
    blockers
}

fn expected_statistics_methods(payload_root: &str) -> Vec<(&'static str, &'static [&'static str])> {
    const MEASURED: &[&str] = &["measured"];
    const NOT_APPLICABLE: &[&str] = &["not_applicable"];
    const MEASURED_OR_NOT_APPLICABLE: &[&str] = &["measured", "not_applicable"];

    let mut methods = vec![
        ("success_rate_confidence_interval", MEASURED),
        ("median_latency_delta_confidence_interval", MEASURED),
        ("p95_latency_delta_confidence_interval", MEASURED),
        ("verdict_distribution_drift", MEASURED),
        ("latency_distribution_drift", MEASURED),
    ];

    match payload_root {
        "memory_task_matrix" => {
            methods.push(("score_delta_confidence_interval", MEASURED));
            methods.push(("mean_delta_confidence_interval", NOT_APPLICABLE));
            methods.push(("score_distribution_drift", MEASURED));
        }
        "mcp_task_matrix" => {
            methods.push(("score_delta_confidence_interval", NOT_APPLICABLE));
            methods.push(("mean_delta_confidence_interval", MEASURED));
            methods.push(("score_distribution_drift", NOT_APPLICABLE));
        }
        _ => {
            methods.push((
                "score_delta_confidence_interval",
                MEASURED_OR_NOT_APPLICABLE,
            ));
            methods.push(("mean_delta_confidence_interval", MEASURED_OR_NOT_APPLICABLE));
            methods.push(("score_distribution_drift", MEASURED_OR_NOT_APPLICABLE));
        }
    }

    methods
}

fn statistics_method_status_allowed(
    statistics: &Value,
    method_name: &str,
    allowed_statuses: &[&str],
) -> bool {
    statistics["methods"][method_name]
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| allowed_statuses.contains(&status))
}

fn valid_run_id(value: &Value) -> bool {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| Uuid::parse_str(value).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn promotion_law_blocks_on_incomplete_statistics() {
        let payload = json!({
            "memory_task_matrix": {
                "gate_failures": [],
                "statistics": {
                    "statistics_version": "benchmark-statistics-v1",
                    "sample_size": 4,
                    "baseline_run_id": null,
                    "candidate_run_id": "candidate",
                    "drift_summary": { "status": "not_measured" },
                    "promotion": {
                        "fail_closed": true,
                        "reason": "statistics_block_incomplete_for_promotion",
                        "blockers": ["baseline_run_id_missing"]
                    }
                }
            }
        });
        let block = promotion_law_block("memory_task_matrix", &payload);
        assert_eq!(block["state"], json!("blocked_statistics_incomplete"));
        assert_eq!(block["fail_closed"], json!(true));
        assert_eq!(block["reason"], json!(STATISTICS_INCOMPLETE_REASON));
    }

    #[test]
    fn promotion_law_exposes_candidate_ready_state_without_auto_promotion() {
        let payload = json!({
            "mcp_task_matrix": {
                "statistics": {
                    "statistics_version": "benchmark-statistics-v1",
                    "sample_size": 6,
                    "baseline_run_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                    "candidate_run_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                    "methods": {
                        "success_rate_confidence_interval": {"status": "measured"},
                        "score_delta_confidence_interval": {"status": "not_applicable"},
                        "mean_delta_confidence_interval": {"status": "measured"},
                        "median_latency_delta_confidence_interval": {"status": "measured"},
                        "p95_latency_delta_confidence_interval": {"status": "measured"},
                        "verdict_distribution_drift": {"status": "measured"},
                        "latency_distribution_drift": {"status": "measured"},
                        "score_distribution_drift": {"status": "not_applicable"}
                    },
                    "drift_summary": { "status": "measured" },
                    "promotion": {
                        "fail_closed": false,
                        "reason": "promotion_policy_not_materialized",
                        "blockers": []
                    }
                }
            }
        });
        let block = promotion_law_block("mcp_task_matrix", &payload);
        assert_eq!(
            block["state"],
            json!("candidate_ready_for_measured_approval")
        );
        assert_eq!(block["verdict"], json!("not_promotable"));
        assert_eq!(block["reason"], json!(MEASURED_APPROVAL_PENDING_REASON));
        assert_eq!(block["candidate_ready_for_measured_approval"], json!(true));
    }

    #[test]
    fn promotion_law_fail_closes_when_benchmark_gates_block() {
        let payload = json!({
            "mcp_task_matrix": {
                "gate_failures": ["p95_latency_sla_failed"],
                "statistics": {
                    "statistics_version": "benchmark-statistics-v1",
                    "sample_size": 6,
                    "baseline_run_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                    "candidate_run_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                    "methods": {
                        "success_rate_confidence_interval": {"status": "measured"},
                        "score_delta_confidence_interval": {"status": "not_applicable"},
                        "mean_delta_confidence_interval": {"status": "measured"},
                        "median_latency_delta_confidence_interval": {"status": "measured"},
                        "p95_latency_delta_confidence_interval": {"status": "measured"},
                        "verdict_distribution_drift": {"status": "measured"},
                        "latency_distribution_drift": {"status": "measured"},
                        "score_distribution_drift": {"status": "not_applicable"}
                    },
                    "drift_summary": { "status": "measured" },
                    "promotion": {
                        "fail_closed": false,
                        "reason": "promotion_policy_not_materialized",
                        "blockers": []
                    }
                }
            }
        });
        let block = promotion_law_block("mcp_task_matrix", &payload);

        assert_eq!(block["state"], json!("blocked_benchmark_gates"));
        assert_eq!(block["fail_closed"], json!(true));
        assert_eq!(block["reason"], json!(BENCHMARK_GATES_NOT_MET_REASON));
        assert_eq!(block["candidate_ready_for_measured_approval"], json!(false));
        assert_eq!(block["blocking_reasons"], json!(["p95_latency_sla_failed"]));
    }

    #[test]
    fn promotion_law_blocks_forged_ready_statistics_without_score_distribution_method() {
        let payload = json!({
            "memory_task_matrix": {
                "gate_failures": [],
                "statistics": {
                    "statistics_version": "benchmark-statistics-v1",
                    "sample_size": 6,
                    "baseline_run_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                    "candidate_run_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                    "methods": {
                        "success_rate_confidence_interval": {"status": "measured"},
                        "score_delta_confidence_interval": {"status": "measured"},
                        "mean_delta_confidence_interval": {"status": "not_applicable"},
                        "median_latency_delta_confidence_interval": {"status": "measured"},
                        "p95_latency_delta_confidence_interval": {"status": "measured"},
                        "verdict_distribution_drift": {"status": "measured"},
                        "latency_distribution_drift": {"status": "measured"}
                    },
                    "drift_summary": { "status": "measured" },
                    "promotion": {
                        "fail_closed": false,
                        "reason": "promotion_policy_not_materialized",
                        "blockers": []
                    }
                }
            }
        });
        let block = promotion_law_block("memory_task_matrix", &payload);

        assert_eq!(block["state"], json!("blocked_statistics_incomplete"));
        assert_eq!(block["fail_closed"], json!(true));
        assert_eq!(block["candidate_ready_for_measured_approval"], json!(false));
        assert!(
            block["blocking_reasons"]
                .as_array()
                .expect("blocking reasons")
                .contains(&json!(
                    "score_distribution_drift_missing_or_unaccepted_status"
                ))
        );
    }

    #[test]
    fn promotion_law_blocks_memory_statistics_with_wrong_score_distribution_status() {
        let payload = json!({
            "memory_task_matrix": {
                "gate_failures": [],
                "statistics": {
                    "statistics_version": "benchmark-statistics-v1",
                    "sample_size": 6,
                    "baseline_run_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                    "candidate_run_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                    "methods": {
                        "success_rate_confidence_interval": {"status": "measured"},
                        "score_delta_confidence_interval": {"status": "measured"},
                        "mean_delta_confidence_interval": {"status": "not_applicable"},
                        "median_latency_delta_confidence_interval": {"status": "measured"},
                        "p95_latency_delta_confidence_interval": {"status": "measured"},
                        "verdict_distribution_drift": {"status": "measured"},
                        "latency_distribution_drift": {"status": "measured"},
                        "score_distribution_drift": {"status": "not_applicable"}
                    },
                    "drift_summary": { "status": "measured" },
                    "promotion": {
                        "fail_closed": false,
                        "reason": "promotion_policy_not_materialized",
                        "blockers": []
                    }
                }
            }
        });
        let block = promotion_law_block("memory_task_matrix", &payload);

        assert_eq!(block["state"], json!("blocked_statistics_incomplete"));
        assert_eq!(block["inputs"]["statistics_methods_complete"], json!(false));
        assert!(
            block["blocking_reasons"]
                .as_array()
                .expect("blocking reasons")
                .contains(&json!(
                    "score_distribution_drift_missing_or_unaccepted_status"
                ))
        );
    }

    #[test]
    fn promotion_law_blocks_forged_ready_statistics_without_valid_run_identity() {
        let payload = json!({
            "memory_task_matrix": {
                "gate_failures": [],
                "statistics": {
                    "statistics_version": "benchmark-statistics-v1",
                    "sample_size": 6,
                    "baseline_run_id": "not-a-uuid",
                    "candidate_run_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                    "drift_summary": { "status": "measured" },
                    "promotion": {
                        "fail_closed": false,
                        "reason": "promotion_policy_not_materialized",
                        "blockers": []
                    }
                }
            }
        });
        let block = promotion_law_block("memory_task_matrix", &payload);

        assert_eq!(block["state"], json!("blocked_statistics_incomplete"));
        assert_eq!(block["fail_closed"], json!(true));
        assert_eq!(block["reason"], json!(STATISTICS_INCOMPLETE_REASON));
        assert_eq!(block["candidate_ready_for_measured_approval"], json!(false));
        assert!(
            block["blocking_reasons"]
                .as_array()
                .expect("blocking reasons")
                .contains(&json!("baseline_run_id_missing_or_invalid"))
        );
    }
}
