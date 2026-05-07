use anyhow::Result;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

const REGRESSION_EXPLAIN_VERSION: &str = "regression-explain-v1";
const MIN_BINARY_SAMPLE_SIZE: usize = 6;
const LOGISTIC_ITERATIONS: usize = 600;
const LOGISTIC_LEARNING_RATE: f64 = 0.1;
const LOGISTIC_L2: f64 = 0.01;

#[derive(Debug, Clone)]
struct ExplainSample {
    source_surface: &'static str,
    item_class: String,
    item_kind: String,
    item_layer: String,
    latency_ms: Option<f64>,
    score: Option<f64>,
    expected_operation_count: Option<f64>,
    actual_operation_count: Option<f64>,
    penalty_points: Option<f64>,
    expected_present: Option<bool>,
    unexpected_present: Option<bool>,
    boundary_clean: Option<bool>,
    recovered_state: Option<bool>,
    answer_ok: Option<bool>,
    success: Option<bool>,
    eval_verdict_class: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum OutcomeKind {
    BenchmarkPass,
    StaleError,
    RetrievalHelpful,
}

impl OutcomeKind {
    fn key(self) -> &'static str {
        match self {
            Self::BenchmarkPass => "benchmark_pass",
            Self::StaleError => "stale_error",
            Self::RetrievalHelpful => "retrieval_helpful",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::BenchmarkPass => "Benchmark pass",
            Self::StaleError => "Stale error",
            Self::RetrievalHelpful => "Retrieval helpful",
        }
    }

    fn label_positive_summary(self) -> &'static str {
        match self {
            Self::BenchmarkPass => "task.success == true",
            Self::StaleError => "eval_verdict_class == stale_target",
            Self::RetrievalHelpful => {
                "eval_verdict_class in {hit_correct_target, recovered_useful}"
            }
        }
    }

    fn label_negative_summary(self) -> &'static str {
        match self {
            Self::BenchmarkPass => "task.success == false",
            Self::StaleError => "eval_verdict_class != stale_target",
            Self::RetrievalHelpful => {
                "eval_verdict_class outside {hit_correct_target, recovered_useful}"
            }
        }
    }
}

#[derive(Debug, Clone)]
struct BinaryDataset {
    samples: Vec<ExplainSample>,
    labels: Vec<f64>,
    positive_count: usize,
    negative_count: usize,
}

#[derive(Debug, Clone)]
struct FeatureSpace {
    names: Vec<String>,
    numeric_features: Vec<NumericFeature>,
    bool_features: Vec<BoolFeature>,
    categorical_features: Vec<CategoricalFeature>,
}

#[derive(Debug, Clone, Copy)]
enum NumericFeature {
    LatencyMs,
    Score,
    ExpectedOperationCount,
    ActualOperationCount,
    PenaltyPoints,
}

impl NumericFeature {
    fn name(self) -> &'static str {
        match self {
            Self::LatencyMs => "latency_ms",
            Self::Score => "score",
            Self::ExpectedOperationCount => "expected_operation_count",
            Self::ActualOperationCount => "actual_operation_count",
            Self::PenaltyPoints => "penalty_points",
        }
    }

    fn value(self, sample: &ExplainSample) -> Option<f64> {
        match self {
            Self::LatencyMs => sample.latency_ms,
            Self::Score => sample.score,
            Self::ExpectedOperationCount => sample.expected_operation_count,
            Self::ActualOperationCount => sample.actual_operation_count,
            Self::PenaltyPoints => sample.penalty_points,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BoolFeature {
    ExpectedPresent,
    UnexpectedPresent,
    BoundaryClean,
    RecoveredState,
    AnswerOk,
}

impl BoolFeature {
    fn name(self) -> &'static str {
        match self {
            Self::ExpectedPresent => "expected_present",
            Self::UnexpectedPresent => "unexpected_present",
            Self::BoundaryClean => "boundary_clean",
            Self::RecoveredState => "recovered_state",
            Self::AnswerOk => "answer_ok",
        }
    }

    fn value(self, sample: &ExplainSample) -> Option<bool> {
        match self {
            Self::ExpectedPresent => sample.expected_present,
            Self::UnexpectedPresent => sample.unexpected_present,
            Self::BoundaryClean => sample.boundary_clean,
            Self::RecoveredState => sample.recovered_state,
            Self::AnswerOk => sample.answer_ok,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CategoricalField {
    SourceSurface,
    ItemClass,
    ItemKind,
    ItemLayer,
}

#[derive(Debug, Clone)]
struct CategoricalFeature {
    field: CategoricalField,
    categories: Vec<String>,
}

impl CategoricalField {
    fn prefix(self) -> &'static str {
        match self {
            Self::SourceSurface => "source_surface",
            Self::ItemClass => "item_class",
            Self::ItemKind => "item_kind",
            Self::ItemLayer => "item_layer",
        }
    }

    fn value<'a>(self, sample: &'a ExplainSample) -> &'a str {
        match self {
            Self::SourceSurface => sample.source_surface,
            Self::ItemClass => &sample.item_class,
            Self::ItemKind => &sample.item_kind,
            Self::ItemLayer => &sample.item_layer,
        }
    }
}

pub fn build_regression_explain(snapshot: &Value) -> Result<Value> {
    let sample_pool = collect_samples(snapshot);
    let outcomes = [
        OutcomeKind::BenchmarkPass,
        OutcomeKind::StaleError,
        OutcomeKind::RetrievalHelpful,
    ]
    .into_iter()
    .map(|outcome| build_outcome_report(outcome, &sample_pool))
    .collect::<Vec<_>>();
    let measured = outcomes
        .iter()
        .filter(|item| item["status"].as_str() == Some("measured"))
        .count();
    let insufficient = outcomes
        .iter()
        .filter(|item| item["status"].as_str() == Some("insufficient_sample"))
        .count();
    let not_materialized = outcomes
        .iter()
        .filter(|item| item["status"].as_str() == Some("not_materialized"))
        .count();
    Ok(json!({
        "model_version": REGRESSION_EXPLAIN_VERSION,
        "surface_role": "read_only_explainability",
        "summary": {
            "status": if measured > 0 { "pass" } else { "unknown" },
            "measured_outcomes": measured,
            "insufficient_sample_outcomes": insufficient,
            "not_materialized_outcomes": not_materialized,
            "sample_pool_size": sample_pool.len(),
        },
        "sample_pool_summary": summarize_sample_pool(&sample_pool),
        "outcomes": outcomes,
        "guardrails": {
            "routing_authority": false,
            "truth_authority": false,
            "forgetting_authority": false,
        }
    }))
}

fn collect_samples(snapshot: &Value) -> Vec<ExplainSample> {
    let mut rows = Vec::new();
    if let Some(tasks) =
        snapshot["latest_memory_task_matrix"]["memory_task_matrix"]["tasks"].as_array()
    {
        for task in tasks {
            rows.push(sample_from_task("memory_task_matrix", task));
        }
    }
    if let Some(tasks) = snapshot["latest_mcp_task_matrix"]["mcp_task_matrix"]["tasks"].as_array() {
        for task in tasks {
            rows.push(sample_from_task("mcp_task_matrix", task));
        }
    }
    if let Some(probes) =
        snapshot["latest_retrieval_accuracy"]["accuracy_verification"]["canonical_eval"]["probes"]
            .as_array()
    {
        for probe in probes {
            rows.push(sample_from_probe("retrieval_accuracy", probe));
        }
    }
    if let Some(probes) = snapshot["latest_continuity_verification"]["continuity_verification"]["canonical_eval"]["probes"]
        .as_array()
    {
        for probe in probes {
            rows.push(sample_from_probe("continuity_verification", probe));
        }
    }
    rows
}

fn sample_from_task(source_surface: &'static str, task: &Value) -> ExplainSample {
    let details = &task["details"];
    ExplainSample {
        source_surface,
        item_class: task["class"].as_str().unwrap_or("unknown").to_string(),
        item_kind: task["kind"].as_str().unwrap_or("unknown").to_string(),
        item_layer: task["layer"].as_str().unwrap_or("none").to_string(),
        latency_ms: task["latency_ms"].as_f64(),
        score: task["score"].as_f64(),
        expected_operation_count: task["expected_operation_count"].as_f64(),
        actual_operation_count: task["actual_operation_count"].as_f64(),
        penalty_points: task["penalty_points"].as_f64(),
        expected_present: details["expected_present"].as_bool(),
        unexpected_present: details["unexpected_present"].as_bool(),
        boundary_clean: details["boundary_clean"].as_bool(),
        recovered_state: details["recovered_state"].as_bool(),
        answer_ok: details["answer_ok"].as_bool(),
        success: task["success"].as_bool(),
        eval_verdict_class: task["eval_verdict_class"].as_str().map(ToOwned::to_owned),
    }
}

fn sample_from_probe(source_surface: &'static str, probe: &Value) -> ExplainSample {
    let details = &probe["details"];
    ExplainSample {
        source_surface,
        item_class: "probe".to_string(),
        item_kind: probe["name"].as_str().unwrap_or("probe").to_string(),
        item_layer: if source_surface == "continuity_verification" {
            "continuity".to_string()
        } else {
            "retrieval".to_string()
        },
        latency_ms: None,
        score: None,
        expected_operation_count: None,
        actual_operation_count: None,
        penalty_points: None,
        expected_present: details["expected_present"].as_bool(),
        unexpected_present: details["unexpected_present"].as_bool(),
        boundary_clean: details["boundary_clean"].as_bool(),
        recovered_state: details["recovered_state"].as_bool(),
        answer_ok: None,
        success: None,
        eval_verdict_class: probe["eval_verdict_class"].as_str().map(ToOwned::to_owned),
    }
}

fn summarize_sample_pool(samples: &[ExplainSample]) -> Value {
    let mut source_counts = BTreeMap::<String, usize>::new();
    for sample in samples {
        *source_counts
            .entry(sample.source_surface.to_string())
            .or_default() += 1;
    }
    json!({
        "source_counts": source_counts,
        "sample_count": samples.len(),
    })
}

fn build_outcome_report(outcome: OutcomeKind, sample_pool: &[ExplainSample]) -> Value {
    let dataset = dataset_for_outcome(outcome, sample_pool);
    if dataset.samples.is_empty() {
        return json!({
            "outcome_key": outcome.key(),
            "title": outcome.title(),
            "status": "not_materialized",
            "sample_size": 0,
            "positive_count": 0,
            "negative_count": 0,
            "notes": ["В текущем observe snapshot нет подходящих rows для этого outcome."],
            "label_mapping": {
                "positive": outcome.label_positive_summary(),
                "negative": outcome.label_negative_summary(),
            },
        });
    }
    if dataset.samples.len() < MIN_BINARY_SAMPLE_SIZE
        || dataset.positive_count == 0
        || dataset.negative_count == 0
    {
        return json!({
            "outcome_key": outcome.key(),
            "title": outcome.title(),
            "status": "insufficient_sample",
            "sample_size": dataset.samples.len(),
            "positive_count": dataset.positive_count,
            "negative_count": dataset.negative_count,
            "notes": [format!(
                "Для logistic regression нужны обе binary стороны и не меньше {MIN_BINARY_SAMPLE_SIZE} samples."
            )],
            "label_mapping": {
                "positive": outcome.label_positive_summary(),
                "negative": outcome.label_negative_summary(),
            },
            "source_counts": summarize_sample_pool(&dataset.samples)["source_counts"].clone(),
        });
    }

    let feature_space = build_feature_space(&dataset.samples);
    if feature_space.names.is_empty() {
        return json!({
            "outcome_key": outcome.key(),
            "title": outcome.title(),
            "status": "insufficient_sample",
            "sample_size": dataset.samples.len(),
            "positive_count": dataset.positive_count,
            "negative_count": dataset.negative_count,
            "notes": ["После feature extraction не осталось ни одного usable explanatory feature."],
            "label_mapping": {
                "positive": outcome.label_positive_summary(),
                "negative": outcome.label_negative_summary(),
            },
        });
    }

    let matrix = build_feature_matrix(&dataset.samples, &feature_space);
    let weights = fit_logistic_regression(&matrix, &dataset.labels);
    let probabilities = matrix
        .iter()
        .map(|row| sigmoid(dot(&weights, row)))
        .collect::<Vec<_>>();
    let auc = auc_score(&dataset.labels, &probabilities);
    let brier_score = mean_squared_error(&dataset.labels, &probabilities);
    let coefficient_table = coefficient_table(&feature_space.names, &weights);

    json!({
        "outcome_key": outcome.key(),
        "title": outcome.title(),
        "status": "measured",
        "model_kind": "logistic_regression",
        "sample_size": dataset.samples.len(),
        "positive_count": dataset.positive_count,
        "negative_count": dataset.negative_count,
        "feature_count": feature_space.names.len(),
        "auc": auc,
        "brier_score": brier_score,
        "r_squared": Value::Null,
        "label_mapping": {
            "positive": outcome.label_positive_summary(),
            "negative": outcome.label_negative_summary(),
        },
        "source_counts": summarize_sample_pool(&dataset.samples)["source_counts"].clone(),
        "coefficient_table": coefficient_table,
        "feature_sign_summary": feature_sign_summary(&feature_space.names, &weights),
    })
}

fn dataset_for_outcome(outcome: OutcomeKind, sample_pool: &[ExplainSample]) -> BinaryDataset {
    let mut samples = Vec::new();
    let mut labels = Vec::new();
    for sample in sample_pool {
        let label = match outcome {
            OutcomeKind::BenchmarkPass => sample.success.map(|value| if value { 1.0 } else { 0.0 }),
            OutcomeKind::StaleError => sample
                .eval_verdict_class
                .as_deref()
                .map(|value| if value == "stale_target" { 1.0 } else { 0.0 }),
            OutcomeKind::RetrievalHelpful => sample.eval_verdict_class.as_deref().map(|value| {
                if matches!(value, "hit_correct_target" | "recovered_useful") {
                    1.0
                } else {
                    0.0
                }
            }),
        };
        if let Some(label) = label {
            samples.push(sample.clone());
            labels.push(label);
        }
    }
    let positive_count = labels.iter().filter(|value| **value >= 0.5).count();
    let negative_count = labels.len().saturating_sub(positive_count);
    BinaryDataset {
        samples,
        labels,
        positive_count,
        negative_count,
    }
}

fn build_feature_space(samples: &[ExplainSample]) -> FeatureSpace {
    let mut names = Vec::new();
    let mut numeric_features = Vec::new();
    let mut bool_features = Vec::new();
    let mut categorical_features = Vec::new();

    for feature in [
        NumericFeature::LatencyMs,
        NumericFeature::Score,
        NumericFeature::ExpectedOperationCount,
        NumericFeature::ActualOperationCount,
        NumericFeature::PenaltyPoints,
    ] {
        if samples.iter().any(|sample| feature.value(sample).is_some()) {
            names.push(feature.name().to_string());
            numeric_features.push(feature);
        }
    }

    for feature in [
        BoolFeature::ExpectedPresent,
        BoolFeature::UnexpectedPresent,
        BoolFeature::BoundaryClean,
        BoolFeature::RecoveredState,
        BoolFeature::AnswerOk,
    ] {
        if samples.iter().any(|sample| feature.value(sample).is_some()) {
            names.push(feature.name().to_string());
            bool_features.push(feature);
        }
    }

    for field in [
        CategoricalField::SourceSurface,
        CategoricalField::ItemClass,
        CategoricalField::ItemKind,
        CategoricalField::ItemLayer,
    ] {
        let categories = samples
            .iter()
            .map(|sample| field.value(sample).to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if categories.len() > 1 {
            for category in &categories {
                names.push(format!("{}={}", field.prefix(), category));
            }
            categorical_features.push(CategoricalFeature { field, categories });
        }
    }

    FeatureSpace {
        names,
        numeric_features,
        bool_features,
        categorical_features,
    }
}

fn build_feature_matrix(samples: &[ExplainSample], feature_space: &FeatureSpace) -> Vec<Vec<f64>> {
    let mut rows = samples
        .iter()
        .map(|sample| {
            let mut row = vec![1.0];
            for feature in &feature_space.numeric_features {
                row.push(feature.value(sample).unwrap_or(0.0));
            }
            for feature in &feature_space.bool_features {
                row.push(if feature.value(sample).unwrap_or(false) {
                    1.0
                } else {
                    0.0
                });
            }
            for feature in &feature_space.categorical_features {
                let value = feature.field.value(sample);
                for category in &feature.categories {
                    row.push(if value == category { 1.0 } else { 0.0 });
                }
            }
            row
        })
        .collect::<Vec<_>>();

    if rows.is_empty() {
        return rows;
    }

    let columns = rows[0].len();
    for column in 1..columns {
        let mean = rows.iter().map(|row| row[column]).sum::<f64>() / rows.len() as f64;
        let variance = rows
            .iter()
            .map(|row| {
                let delta = row[column] - mean;
                delta * delta
            })
            .sum::<f64>()
            / rows.len() as f64;
        let stddev = variance.sqrt();
        if stddev > 1e-9 {
            for row in &mut rows {
                row[column] = (row[column] - mean) / stddev;
            }
        } else {
            for row in &mut rows {
                row[column] = 0.0;
            }
        }
    }

    rows
}

fn fit_logistic_regression(matrix: &[Vec<f64>], labels: &[f64]) -> Vec<f64> {
    if matrix.is_empty() {
        return Vec::new();
    }
    let feature_count = matrix[0].len();
    let mut weights = vec![0.0; feature_count];
    for _ in 0..LOGISTIC_ITERATIONS {
        let mut gradient = vec![0.0; feature_count];
        for (row, label) in matrix.iter().zip(labels) {
            let prediction = sigmoid(dot(&weights, row));
            let error = prediction - label;
            for (index, value) in row.iter().enumerate() {
                gradient[index] += error * value;
            }
        }
        for index in 0..feature_count {
            let regularizer = if index == 0 {
                0.0
            } else {
                LOGISTIC_L2 * weights[index]
            };
            weights[index] -=
                LOGISTIC_LEARNING_RATE * ((gradient[index] / matrix.len() as f64) + regularizer);
        }
    }
    weights
}

fn coefficient_table(feature_names: &[String], weights: &[f64]) -> Vec<Value> {
    let mut rows = feature_names
        .iter()
        .zip(weights.iter().skip(1))
        .map(|(feature, coefficient)| {
            json!({
                "feature": feature,
                "coefficient": coefficient,
                "sign": if *coefficient > 0.0 {
                    "positive"
                } else if *coefficient < 0.0 {
                    "negative"
                } else {
                    "neutral"
                },
                "magnitude": coefficient.abs(),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        let left_value = left["magnitude"].as_f64().unwrap_or(0.0);
        let right_value = right["magnitude"].as_f64().unwrap_or(0.0);
        right_value
            .partial_cmp(&left_value)
            .unwrap_or(Ordering::Equal)
    });
    rows
}

fn feature_sign_summary(feature_names: &[String], weights: &[f64]) -> Value {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for (name, coefficient) in feature_names.iter().zip(weights.iter().skip(1)) {
        if *coefficient > 0.0 {
            positive.push((name.clone(), coefficient.abs()));
        } else if *coefficient < 0.0 {
            negative.push((name.clone(), coefficient.abs()));
        }
    }
    positive.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
    negative.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
    json!({
        "positive": positive.into_iter().take(5).map(|(feature, magnitude)| {
            json!({"feature": feature, "magnitude": magnitude})
        }).collect::<Vec<_>>(),
        "negative": negative.into_iter().take(5).map(|(feature, magnitude)| {
            json!({"feature": feature, "magnitude": magnitude})
        }).collect::<Vec<_>>(),
    })
}

fn auc_score(labels: &[f64], probabilities: &[f64]) -> Value {
    let mut positives = Vec::new();
    let mut negatives = Vec::new();
    for (label, probability) in labels.iter().zip(probabilities) {
        if *label >= 0.5 {
            positives.push(*probability);
        } else {
            negatives.push(*probability);
        }
    }
    if positives.is_empty() || negatives.is_empty() {
        return Value::Null;
    }
    let mut wins = 0.0;
    let mut comparisons = 0.0;
    for positive in &positives {
        for negative in &negatives {
            comparisons += 1.0;
            if positive > negative {
                wins += 1.0;
            } else if (*positive - *negative).abs() < 1e-12 {
                wins += 0.5;
            }
        }
    }
    Value::from(wins / comparisons)
}

fn mean_squared_error(labels: &[f64], probabilities: &[f64]) -> f64 {
    labels
        .iter()
        .zip(probabilities)
        .map(|(label, probability)| {
            let delta = probability - label;
            delta * delta
        })
        .sum::<f64>()
        / labels.len() as f64
}

fn dot(weights: &[f64], row: &[f64]) -> f64 {
    weights
        .iter()
        .zip(row)
        .map(|(weight, value)| weight * value)
        .sum()
}

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        let exp = (-value).exp();
        1.0 / (1.0 + exp)
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    }
}

#[cfg(test)]
mod tests {
    use super::build_regression_explain;
    use serde_json::json;

    #[test]
    fn regression_explain_reports_measured_and_insufficient_outcomes() {
        let snapshot = json!({
            "latest_memory_task_matrix": {
                "memory_task_matrix": {
                    "tasks": [
                        {
                            "class": "read",
                            "kind": "CoreRead",
                            "layer": "core",
                            "latency_ms": 10.0,
                            "score": 1.0,
                            "expected_operation_count": 3.0,
                            "actual_operation_count": 3.0,
                            "penalty_points": 0.0,
                            "success": true,
                            "eval_verdict_class": "recovered_useful",
                            "details": {
                                "expected_present": true,
                                "unexpected_present": false,
                                "recovered_state": true,
                                "answer_ok": true
                            }
                        },
                        {
                            "class": "read",
                            "kind": "CoreRead",
                            "layer": "core",
                            "latency_ms": 25.0,
                            "score": 0.0,
                            "expected_operation_count": 3.0,
                            "actual_operation_count": 4.0,
                            "penalty_points": 1.0,
                            "success": false,
                            "eval_verdict_class": "under_retrieved",
                            "details": {
                                "expected_present": false,
                                "unexpected_present": false,
                                "recovered_state": false,
                                "answer_ok": false
                            }
                        }
                    ]
                }
            },
            "latest_mcp_task_matrix": {
                "mcp_task_matrix": {
                    "tasks": [
                        {
                            "class": "happy_path",
                            "kind": "ToolCatalog",
                            "latency_ms": 2.0,
                            "success": true,
                            "eval_verdict_class": "hit_correct_target",
                            "details": {
                                "expected_present": true,
                                "unexpected_present": false
                            }
                        },
                        {
                            "class": "fail_closed",
                            "kind": "ContinuityRestoreFailClosed",
                            "latency_ms": 5.0,
                            "success": false,
                            "eval_verdict_class": "stale_target",
                            "details": {
                                "expected_present": false,
                                "unexpected_present": true
                            }
                        },
                        {
                            "class": "happy_path",
                            "kind": "ContinuityRestoreSuccess",
                            "latency_ms": 4.0,
                            "success": true,
                            "eval_verdict_class": "recovered_useful",
                            "details": {
                                "expected_present": true,
                                "unexpected_present": false
                            }
                        },
                        {
                            "class": "happy_path",
                            "kind": "ToolCatalog",
                            "latency_ms": 3.0,
                            "success": true,
                            "eval_verdict_class": "hit_correct_target",
                            "details": {
                                "expected_present": true,
                                "unexpected_present": false
                            }
                        }
                    ]
                }
            },
            "latest_retrieval_accuracy": {
                "accuracy_verification": {
                    "canonical_eval": {
                        "probes": [
                            {
                                "name": "related_retrieval_target",
                                "eval_verdict_class": "hit_correct_target",
                                "details": {
                                    "expected_present": true,
                                    "unexpected_present": false
                                }
                            },
                            {
                                "name": "strict_local_fail_closed",
                                "eval_verdict_class": "under_retrieved",
                                "details": {
                                    "expected_present": false,
                                    "unexpected_present": false,
                                    "boundary_clean": true
                                }
                            }
                        ]
                    }
                }
            },
            "latest_continuity_verification": {
                "continuity_verification": {
                    "canonical_eval": {
                        "probes": [
                            {
                                "name": "startup_summary_recovered_useful",
                                "eval_verdict_class": "recovered_useful",
                                "details": {
                                    "expected_present": true,
                                    "unexpected_present": false,
                                    "recovered_state": true
                                }
                            }
                        ]
                    }
                }
            }
        });

        let report = build_regression_explain(&snapshot).expect("report");
        assert_eq!(report["model_version"], json!("regression-explain-v1"));
        assert_eq!(
            report["outcomes"].as_array().map(|items| items.len()),
            Some(3)
        );
        assert_eq!(
            report["outcomes"][0]["outcome_key"].as_str(),
            Some("benchmark_pass")
        );
        assert_eq!(report["outcomes"][0]["status"].as_str(), Some("measured"));
        assert_eq!(
            report["outcomes"][1]["outcome_key"].as_str(),
            Some("stale_error")
        );
        assert_eq!(report["outcomes"][1]["status"].as_str(), Some("measured"));
        assert_eq!(
            report["outcomes"][2]["outcome_key"].as_str(),
            Some("retrieval_helpful")
        );
    }

    #[test]
    fn regression_explain_marks_sparse_outcome_as_insufficient() {
        let snapshot = json!({
            "latest_memory_task_matrix": {
                "memory_task_matrix": {
                    "tasks": [
                        {
                            "class": "read",
                            "kind": "CoreRead",
                            "layer": "core",
                            "latency_ms": 10.0,
                            "score": 1.0,
                            "expected_operation_count": 3.0,
                            "actual_operation_count": 3.0,
                            "penalty_points": 0.0,
                            "success": true,
                            "eval_verdict_class": "recovered_useful",
                            "details": {
                                "expected_present": true,
                                "unexpected_present": false,
                                "recovered_state": true,
                                "answer_ok": true
                            }
                        },
                        {
                            "class": "read",
                            "kind": "CoreRead",
                            "layer": "core",
                            "latency_ms": 12.0,
                            "score": 1.0,
                            "expected_operation_count": 3.0,
                            "actual_operation_count": 3.0,
                            "penalty_points": 0.0,
                            "success": true,
                            "eval_verdict_class": "recovered_useful",
                            "details": {
                                "expected_present": true,
                                "unexpected_present": false,
                                "recovered_state": true,
                                "answer_ok": true
                            }
                        }
                    ]
                }
            },
            "latest_mcp_task_matrix": { "mcp_task_matrix": { "tasks": [] } },
            "latest_retrieval_accuracy": { "accuracy_verification": { "canonical_eval": { "probes": [] } } },
            "latest_continuity_verification": { "continuity_verification": { "canonical_eval": { "probes": [] } } }
        });

        let report = build_regression_explain(&snapshot).expect("report");
        let stale = report["outcomes"]
            .as_array()
            .expect("outcomes")
            .iter()
            .find(|item| item["outcome_key"].as_str() == Some("stale_error"))
            .expect("stale outcome");
        assert_eq!(stale["status"].as_str(), Some("insufficient_sample"));
    }
}
