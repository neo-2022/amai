use crate::{continuity, retrieval, retrieval_science, working_state};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn build_report(captured_at_epoch_ms: u64, local_fast_cache_ttl_ms: u128) -> Result<Value> {
    let mut scenarios_by_class = BTreeMap::new();
    for scenario in working_state::degradation_proof_scenarios(captured_at_epoch_ms)? {
        if let Some(class_key) = scenario["class_key"].as_str() {
            scenarios_by_class.insert(class_key.to_string(), scenario);
        }
    }
    for scenario in continuity::degradation_proof_scenarios()? {
        if let Some(class_key) = scenario["class_key"].as_str() {
            scenarios_by_class.insert(class_key.to_string(), scenario);
        }
    }
    for scenario in retrieval::degradation_proof_scenarios(local_fast_cache_ttl_ms)? {
        if let Some(class_key) = scenario["class_key"].as_str() {
            scenarios_by_class.insert(class_key.to_string(), scenario);
        }
    }

    let mut ordered = Vec::new();
    for (class_key, _) in retrieval_science::degradation_matrix_entries()? {
        if let Some(scenario) = scenarios_by_class.remove(&class_key) {
            ordered.push(scenario);
        }
    }
    ordered.extend(scenarios_by_class.into_values());

    let pass = ordered
        .iter()
        .filter(|item| item["status"].as_str() == Some("pass"))
        .count() as u64;
    let critical = ordered
        .iter()
        .filter(|item| item["status"].as_str() == Some("critical"))
        .count() as u64;
    let unknown = ordered
        .iter()
        .filter(|item| item["status"].as_str() == Some("unknown"))
        .count() as u64;

    Ok(json!({
        "degradation_verification": {
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "summary": {
                "pass": pass,
                "critical": critical,
                "unknown": unknown,
            },
            "scenarios": ordered,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::build_report;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn build_report_covers_all_policy_classes() {
        let captured_at_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_millis() as u64;
        let report = build_report(captured_at_epoch_ms, 1).expect("report");
        let scenarios = report["degradation_verification"]["scenarios"]
            .as_array()
            .expect("scenarios");
        assert_eq!(scenarios.len(), 9);
        assert!(
            scenarios
                .iter()
                .all(|scenario| scenario["status"].as_str() == Some("pass")),
            "statuses: {:?}",
            scenarios
                .iter()
                .map(|scenario| (
                    scenario["class_key"].as_str().unwrap_or(""),
                    scenario["status"].as_str().unwrap_or(""),
                ))
                .collect::<Vec<_>>()
        );
    }
}
