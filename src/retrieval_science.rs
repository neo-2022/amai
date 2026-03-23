use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
struct RetrievalScienceFile {
    science: RetrievalSciencePolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetrievalSciencePolicy {
    pub methodology_version: String,
    pub scoring_rules_version: String,
    pub degradation_policy_version: String,
    pub execution_state_model_version: String,
    pub lineage_model_version: String,
    pub workspace_graph_model_version: String,
    pub artifact_lineage_model_version: String,
    pub eval_verdict_model_version: String,
    pub same_input_same_verdict_required: bool,
    pub machine_variance_policy: String,
    pub truth_ranking: Vec<String>,
    pub fail_closed_classes: Vec<String>,
    pub graceful_fallback_classes: Vec<String>,
    pub execution_states: Vec<String>,
    #[serde(default)]
    pub eval_verdict_classes: BTreeMap<String, EvalVerdictClass>,
    #[serde(default)]
    pub degradation_matrix: BTreeMap<String, DegradationMatrixEntry>,
    #[serde(default)]
    pub suites: BTreeMap<String, RetrievalScienceSuite>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetrievalScienceSuite {
    pub suite_version: String,
    pub dataset_version: String,
    pub query_suite_version: String,
    pub manifest_path: String,
    pub reproducibility_contract: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DegradationMatrixEntry {
    pub title: String,
    pub mode: String,
    pub summary: String,
    pub expected_behavior: String,
    pub user_signal: String,
    pub evidence_source: String,
    pub runbook: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvalVerdictClass {
    pub title: String,
    pub summary: String,
}

static POLICY: OnceLock<Result<RetrievalSciencePolicy, String>> = OnceLock::new();

pub fn load_policy() -> Result<&'static RetrievalSciencePolicy> {
    POLICY
        .get_or_init(|| load_policy_uncached().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow!(error.clone()))
}

fn load_policy_uncached() -> Result<RetrievalSciencePolicy> {
    let path = retrieval_science_profile_path();
    let content = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read retrieval science profile {}",
            path.display()
        )
    })?;
    let file: RetrievalScienceFile =
        toml::from_str(&content).context("failed to parse retrieval science profile")?;
    Ok(file.science)
}

pub fn retrieval_science_profile_path() -> PathBuf {
    let cwd_path = Path::new("config/retrieval_science.toml");
    if cwd_path.exists() {
        cwd_path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join("retrieval_science.toml")
    }
}

pub fn suite_metadata(suite_key: &str) -> Result<Value> {
    let policy = load_policy()?;
    let suite = policy
        .suites
        .get(suite_key)
        .ok_or_else(|| anyhow!("retrieval science suite not found: {suite_key}"))?;
    Ok(json!({
        "suite_key": suite_key,
        "suite_version": suite.suite_version,
        "dataset_version": suite.dataset_version,
        "query_suite_version": suite.query_suite_version,
        "manifest_path": suite.manifest_path,
        "methodology_version": policy.methodology_version,
        "scoring_rules_version": policy.scoring_rules_version,
        "same_input_same_verdict_required": policy.same_input_same_verdict_required,
        "machine_variance_policy": policy.machine_variance_policy,
        "reproducibility_contract": suite.reproducibility_contract,
        "recorded_by": "amai",
        "recorded_by_version": env!("CARGO_PKG_VERSION"),
    }))
}

pub fn degradation_policy_json() -> Result<Value> {
    let policy = load_policy()?;
    Ok(json!({
        "policy_version": policy.degradation_policy_version,
        "fail_closed_classes": policy.fail_closed_classes,
        "graceful_fallback_classes": policy.graceful_fallback_classes,
        "truth_ranking": policy.truth_ranking,
        "machine_variance_policy": policy.machine_variance_policy,
    }))
}

pub fn degradation_matrix_entries() -> Result<Vec<(String, DegradationMatrixEntry)>> {
    let policy = load_policy()?;
    Ok(policy
        .degradation_matrix
        .iter()
        .map(|(class_key, entry)| (class_key.clone(), entry.clone()))
        .collect())
}

pub fn degradation_matrix_json() -> Result<Value> {
    let policy = load_policy()?;
    let classes = policy
        .degradation_matrix
        .iter()
        .map(|(class_key, entry)| {
            json!({
                "class_key": class_key,
                "title": entry.title,
                "mode": entry.mode,
                "summary": entry.summary,
                "expected_behavior": entry.expected_behavior,
                "user_signal": entry.user_signal,
                "evidence_source": entry.evidence_source,
                "runbook": entry.runbook,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "policy_version": policy.degradation_policy_version,
        "truth_ranking": policy.truth_ranking,
        "classes": classes,
    }))
}

pub fn execution_state_catalog_json() -> Result<Value> {
    let policy = load_policy()?;
    Ok(json!({
        "execution_state_model_version": policy.execution_state_model_version,
        "lineage_model_version": policy.lineage_model_version,
        "workspace_graph_model_version": policy.workspace_graph_model_version,
        "artifact_lineage_model_version": policy.artifact_lineage_model_version,
        "states": policy.execution_states,
        "truth_ranking": policy.truth_ranking,
    }))
}

pub fn eval_verdict_catalog_json() -> Result<Value> {
    let policy = load_policy()?;
    let classes = policy
        .eval_verdict_classes
        .iter()
        .map(|(class_key, entry)| {
            json!({
                "class_key": class_key,
                "title": entry.title,
                "summary": entry.summary,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "eval_verdict_model_version": policy.eval_verdict_model_version,
        "classes": classes,
    }))
}

pub fn validate_eval_verdict_class(class_key: &str) -> Result<()> {
    let policy = load_policy()?;
    if policy.eval_verdict_classes.contains_key(class_key) {
        Ok(())
    } else {
        Err(anyhow!("unknown eval verdict class: {class_key}"))
    }
}

pub fn workspace_graph_catalog_json() -> Result<Value> {
    let policy = load_policy()?;
    Ok(json!({
        "workspace_graph_model_version": policy.workspace_graph_model_version,
        "artifact_lineage_model_version": policy.artifact_lineage_model_version,
        "lineage_model_version": policy.lineage_model_version,
        "truth_ranking": policy.truth_ranking,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        degradation_matrix_json, degradation_policy_json, eval_verdict_catalog_json,
        execution_state_catalog_json, suite_metadata, validate_eval_verdict_class,
        workspace_graph_catalog_json,
    };

    #[test]
    fn suite_metadata_loads_known_suite() {
        let suite = suite_metadata("retrieval_accuracy").expect("suite metadata");
        assert_eq!(
            suite["query_suite_version"].as_str(),
            Some("red-team-retrieval-isolation-v1")
        );
        assert_eq!(
            suite["manifest_path"].as_str(),
            Some("config/red_team_retrieval_isolation.toml")
        );
    }

    #[test]
    fn suite_metadata_loads_degradation_verification_suite() {
        let suite = suite_metadata("degradation_verification").expect("suite metadata");
        assert_eq!(
            suite["dataset_version"].as_str(),
            Some("synthetic-degradation-matrix-v2")
        );
    }

    #[test]
    fn suite_metadata_loads_continuity_verification_suite() {
        let suite = suite_metadata("continuity_verification").expect("suite metadata");
        assert_eq!(
            suite["manifest_path"].as_str(),
            Some("scripts/proof_art_continuity_migration.sh")
        );
        assert_eq!(
            suite["query_suite_version"].as_str(),
            Some("continuity-verification-v2")
        );
    }

    #[test]
    fn suite_metadata_loads_continuity_answer_suite() {
        let suite = suite_metadata("continuity_answer").expect("suite metadata");
        assert_eq!(
            suite["manifest_path"].as_str(),
            Some("scripts/proof_art_continuity_answer.sh")
        );
        assert_eq!(
            suite["query_suite_version"].as_str(),
            Some("continuity-answer-v1")
        );
    }

    #[test]
    fn suite_metadata_loads_continuity_restore_suite() {
        let suite = suite_metadata("continuity_restore").expect("suite metadata");
        assert_eq!(
            suite["manifest_path"].as_str(),
            Some("scripts/proof_art_continuity_restore.sh")
        );
        assert_eq!(
            suite["query_suite_version"].as_str(),
            Some("continuity-restore-v1")
        );
    }

    #[test]
    fn suite_metadata_loads_continuity_startup_suite() {
        let suite = suite_metadata("continuity_startup").expect("suite metadata");
        assert_eq!(
            suite["manifest_path"].as_str(),
            Some("scripts/proof_art_continuity_startup.sh")
        );
        assert_eq!(
            suite["query_suite_version"].as_str(),
            Some("continuity-startup-v1")
        );
    }

    #[test]
    fn degradation_policy_includes_fail_closed_classes() {
        let policy = degradation_policy_json().expect("degradation policy");
        assert!(
            policy["fail_closed_classes"]
                .as_array()
                .expect("array")
                .iter()
                .any(|item| item.as_str() == Some("cross_project_scope"))
        );
    }

    #[test]
    fn execution_state_catalog_exposes_lineage_versions() {
        let catalog = execution_state_catalog_json().expect("execution state catalog");
        assert_eq!(
            catalog["execution_state_model_version"].as_str(),
            Some("execution-state-v1")
        );
        assert_eq!(
            catalog["lineage_model_version"].as_str(),
            Some("lineage-v2")
        );
        assert!(
            catalog["states"]
                .as_array()
                .expect("array")
                .iter()
                .any(|item| item.as_str() == Some("superseded"))
        );
    }

    #[test]
    fn workspace_graph_catalog_exposes_graph_versions() {
        let catalog = workspace_graph_catalog_json().expect("workspace graph catalog");
        assert_eq!(
            catalog["workspace_graph_model_version"].as_str(),
            Some("workspace-graph-v10")
        );
        assert_eq!(
            catalog["artifact_lineage_model_version"].as_str(),
            Some("artifact-lineage-v1")
        );
    }

    #[test]
    fn eval_verdict_catalog_exposes_requested_classes() {
        let catalog = eval_verdict_catalog_json().expect("eval verdict catalog");
        assert_eq!(
            catalog["eval_verdict_model_version"].as_str(),
            Some("memory-eval-verdict-v1")
        );
        let classes = catalog["classes"].as_array().expect("classes");
        assert!(
            classes
                .iter()
                .any(|entry| entry["class_key"].as_str() == Some("hit_correct_target"))
        );
        assert!(
            classes
                .iter()
                .any(|entry| entry["class_key"].as_str() == Some("recovered_useful"))
        );
    }

    #[test]
    fn validate_eval_verdict_class_rejects_unknown_class() {
        validate_eval_verdict_class("hit_correct_target").expect("known class");
        assert!(validate_eval_verdict_class("made_up").is_err());
    }

    #[test]
    fn degradation_matrix_exposes_cross_project_contract() {
        let matrix = degradation_matrix_json().expect("degradation matrix");
        let classes = matrix["classes"].as_array().expect("classes");
        let cross_project = classes
            .iter()
            .find(|entry| entry["class_key"].as_str() == Some("cross_project_scope"))
            .expect("cross project entry");
        assert_eq!(cross_project["mode"].as_str(), Some("fail_closed"));
        assert!(
            cross_project["summary"]
                .as_str()
                .is_some_and(|value| value.contains("чужого проекта"))
        );
    }
}
