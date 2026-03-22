use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
struct ObservabilityPolicyFile {
    observability: ObservabilityPolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityPolicy {
    pub schema_version: u64,
    pub classification_rules_version: String,
    pub retention_profile: String,
    pub retention_live_system_hours: u64,
    pub retention_live_context_hours: u64,
    pub retention_benchmark_hours: u64,
    pub retention_verify_event_hours: u64,
    pub retention_working_state_restore_hours: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityRetentionRule {
    pub retention_class: &'static str,
    pub ttl_hours: Option<u64>,
    pub immutable_snapshot: bool,
}

static POLICY: OnceLock<Result<ObservabilityPolicy, String>> = OnceLock::new();

pub fn load_policy() -> Result<&'static ObservabilityPolicy> {
    POLICY
        .get_or_init(|| load_policy_uncached().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow!(error.clone()))
}

fn load_policy_uncached() -> Result<ObservabilityPolicy> {
    let path = observability_profile_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read observability profile {}", path.display()))?;
    let file: ObservabilityPolicyFile =
        toml::from_str(&content).context("failed to parse observability profile")?;
    Ok(file.observability)
}

pub fn observability_profile_path() -> PathBuf {
    let cwd_path = Path::new("config/observability.toml");
    if cwd_path.exists() {
        cwd_path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join("observability.toml")
    }
}

pub fn policy_json() -> Result<Value> {
    let policy = load_policy()?;
    Ok(json!({
        "schema_version": policy.schema_version,
        "classification_rules_version": policy.classification_rules_version,
        "retention_profile": policy.retention_profile,
        "retention_hours": {
            "live_system": policy.retention_live_system_hours,
            "live_context": policy.retention_live_context_hours,
            "benchmark": policy.retention_benchmark_hours,
            "verify_event": policy.retention_verify_event_hours,
            "working_state_restore": policy.retention_working_state_restore_hours,
        },
    }))
}

pub fn retention_rule(
    snapshot_kind: &str,
    payload: &Value,
    source_kind: &str,
    source_class: &str,
) -> Result<ObservabilityRetentionRule> {
    let policy = load_policy()?;
    let rule = if snapshot_kind == "working_state_restore" {
        ObservabilityRetentionRule {
            retention_class: "rolling_working_state_restore",
            ttl_hours: Some(policy.retention_working_state_restore_hours),
            immutable_snapshot: false,
        }
    } else if source_class == "live_context" {
        ObservabilityRetentionRule {
            retention_class: "ephemeral_live_context",
            ttl_hours: Some(policy.retention_live_context_hours),
            immutable_snapshot: false,
        }
    } else if source_class == "live_system" || snapshot_kind == "system_snapshot" {
        ObservabilityRetentionRule {
            retention_class: "rolling_live_system",
            ttl_hours: Some(policy.retention_live_system_hours),
            immutable_snapshot: false,
        }
    } else if snapshot_kind == "token_budget_event"
        && matches!(
            token_event_traffic_class(payload, source_kind).as_deref(),
            Some("verify" | "proof" | "benchmark")
        )
    {
        ObservabilityRetentionRule {
            retention_class: "synthetic_token_event",
            ttl_hours: Some(policy.retention_verify_event_hours),
            immutable_snapshot: false,
        }
    } else if is_benchmark_snapshot_kind(snapshot_kind) && source_class == "benchmark" {
        ObservabilityRetentionRule {
            retention_class: "benchmark_history",
            ttl_hours: Some(policy.retention_benchmark_hours),
            immutable_snapshot: true,
        }
    } else {
        ObservabilityRetentionRule {
            retention_class: "persistent",
            ttl_hours: None,
            immutable_snapshot: false,
        }
    };
    Ok(rule)
}

pub fn policy_metadata(
    snapshot_kind: &str,
    payload: &Value,
    source_kind: &str,
    source_class: &str,
) -> Result<Value> {
    let policy = load_policy()?;
    let rule = retention_rule(snapshot_kind, payload, source_kind, source_class)?;
    Ok(json!({
        "schema_version": policy.schema_version,
        "classification_rules_version": policy.classification_rules_version,
        "retention_profile": policy.retention_profile,
        "retention_class": rule.retention_class,
        "retention_ttl_hours": rule.ttl_hours,
        "immutable_snapshot": rule.immutable_snapshot,
        "recorded_by": "amai",
        "recorded_by_version": env!("CARGO_PKG_VERSION"),
    }))
}

pub fn minimum_retention_hours() -> Result<Option<u64>> {
    let policy = load_policy()?;
    Ok([
        Some(policy.retention_live_system_hours),
        Some(policy.retention_live_context_hours),
        Some(policy.retention_benchmark_hours),
        Some(policy.retention_verify_event_hours),
        Some(policy.retention_working_state_restore_hours),
    ]
    .into_iter()
    .flatten()
    .min())
}

pub fn is_benchmark_snapshot_kind(snapshot_kind: &str) -> bool {
    matches!(
        snapshot_kind,
        "retrieval_benchmark_hot"
            | "retrieval_benchmark_cold"
            | "retrieval_load_hot"
            | "retrieval_load_cold"
            | "retrieval_accuracy"
            | "cold_path_benchmark"
            | "token_benchmark"
            | "token_benchmark_suite"
            | "text_compare"
            | "mcp_task_matrix"
            | "memory_task_matrix"
    )
}

fn token_event_traffic_class(payload: &Value, source_kind: &str) -> Option<String> {
    payload["token_budget_event"]["traffic_class"]
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| Some(derive_traffic_class(source_kind)))
}

fn derive_traffic_class(source_kind: &str) -> String {
    if source_kind.starts_with("live_") {
        "live".to_string()
    } else if source_kind.starts_with("verify_") {
        "verify".to_string()
    } else if source_kind.starts_with("proof_") {
        "proof".to_string()
    } else if source_kind.starts_with("benchmark_") {
        "benchmark".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{is_benchmark_snapshot_kind, policy_metadata, retention_rule};
    use serde_json::json;

    #[test]
    fn benchmark_snapshots_are_classified_as_immutable_history() {
        let rule = retention_rule(
            "retrieval_benchmark_hot",
            &json!({"benchmark": {"project": "alpha"}}),
            "benchmark_run",
            "benchmark",
        )
        .expect("retention rule");
        assert_eq!(rule.retention_class, "benchmark_history");
        assert!(rule.immutable_snapshot);
        assert!(rule.ttl_hours.is_some());
        assert!(is_benchmark_snapshot_kind("retrieval_benchmark_hot"));
    }

    #[test]
    fn verify_token_events_are_temporary() {
        let rule = retention_rule(
            "token_budget_event",
            &json!({
                "token_budget_event": {
                    "traffic_class": "verify"
                }
            }),
            "verify_token_benchmark",
            "operational",
        )
        .expect("retention rule");
        assert_eq!(rule.retention_class, "synthetic_token_event");
        assert!(!rule.immutable_snapshot);
        assert!(rule.ttl_hours.is_some());
    }

    #[test]
    fn policy_metadata_stamps_versions_and_retention() {
        let metadata = policy_metadata(
            "retrieval_load_hot",
            &json!({
                "load_verification": {
                    "record_live_context": false,
                    "publish_benchmark_snapshot": true
                }
            }),
            "benchmark_run",
            "benchmark",
        )
        .expect("policy metadata");
        assert_eq!(metadata["recorded_by"].as_str(), Some("amai"));
        assert!(metadata["schema_version"].as_u64().is_some());
        assert_eq!(
            metadata["retention_class"].as_str(),
            Some("benchmark_history")
        );
        assert_eq!(metadata["immutable_snapshot"].as_bool(), Some(true));
    }
}
