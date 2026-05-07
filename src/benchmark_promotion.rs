use serde_json::{Value, json};

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
    let statistics_complete = statistics["statistics_version"].as_str().is_some()
        && statistics["sample_size"].as_u64().unwrap_or(0) > 0
        && statistics["promotion"]["fail_closed"].as_bool() == Some(false)
        && statistics["promotion"]["reason"].as_str()
            != Some("statistics_block_incomplete_for_promotion");

    let (state, fail_closed, reason, blocking_reasons, candidate_ready_for_measured_approval) =
        if !statistics_complete {
            (
                "blocked_statistics_incomplete",
                true,
                STATISTICS_INCOMPLETE_REASON,
                collect_statistics_blockers(statistics),
                false,
            )
        } else if !gate_failures.is_empty() {
            (
                "blocked_benchmark_gates",
                false,
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
            "statistics_fail_closed": statistics["promotion"]["fail_closed"].clone(),
            "statistics_reason": statistics["promotion"]["reason"].clone(),
            "gate_failures": gate_failures,
        },
        "blocking_reasons": blocking_reasons,
    })
}

fn collect_statistics_blockers(statistics: &Value) -> Vec<String> {
    let mut blockers = statistics["promotion"]["blockers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    if blockers.is_empty() {
        blockers.push("statistics_completeness_unknown".to_string());
    }
    blockers
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
                    "baseline_run_id": "baseline",
                    "candidate_run_id": "candidate",
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
}
