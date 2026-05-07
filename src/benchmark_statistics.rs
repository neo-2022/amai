use serde_json::{Map, Value, json};
use uuid::Uuid;

const STATISTICS_VERSION: &str = "benchmark-statistics-v1";
const BASELINE_PAIR_REASON: &str = "baseline_candidate_pair_not_materialized";
const PROMOTION_REASON: &str = "statistics_block_incomplete_for_promotion";
const PROMOTION_POLICY_REASON: &str = "promotion_policy_not_materialized";
const BENCHMARK_GATE_FAILURES_REASON: &str = "benchmark_gate_failures_present";
const BOOTSTRAP_ITERATIONS: usize = 2000;

pub(crate) fn statistics_block_from_pair(
    payload_root: &str,
    candidate_payload: &Value,
    baseline_payload: Option<&Value>,
    candidate_run_id: Uuid,
    extra_blockers: &[String],
) -> Value {
    let candidate_root = &candidate_payload[payload_root];
    let baseline_root = baseline_payload.map(|payload| &payload[payload_root]);
    let sample_size = candidate_root["tasks_total"]
        .as_u64()
        .map(|value| value as usize)
        .unwrap_or_else(|| task_samples(candidate_root, "success").len());
    let success_count = candidate_root["tasks_passed"]
        .as_u64()
        .map(|value| value as usize)
        .unwrap_or_else(|| {
            task_samples(candidate_root, "success")
                .into_iter()
                .filter(|value| *value > 0.0)
                .count()
        });
    let success_rate_interval = if sample_size == 0 {
        json!({
            "status": "not_measured",
            "method": "wilson_95",
            "metric": "success_rate",
            "confidence_level": 0.95,
            "reason": "sample_size_is_zero",
        })
    } else {
        let (lower, upper) = wilson_interval(success_count, sample_size, 1.959_963_984_540_054);
        json!({
            "status": "measured",
            "method": "wilson_95",
            "metric": "success_rate",
            "confidence_level": 0.95,
            "lower": lower,
            "upper": upper,
        })
    };

    let baseline_run_id = baseline_payload
        .and_then(|payload| payload["_observability"]["source_event_id"].as_str())
        .map(ToOwned::to_owned);

    let score_seed = bootstrap_seed(baseline_run_id.as_deref(), candidate_run_id, 1);
    let mean_seed = bootstrap_seed(baseline_run_id.as_deref(), candidate_run_id, 2);
    let median_seed = bootstrap_seed(baseline_run_id.as_deref(), candidate_run_id, 3);
    let p95_seed = bootstrap_seed(baseline_run_id.as_deref(), candidate_run_id, 4);

    let score_delta_interval = bootstrap_metric_interval(
        baseline_root,
        candidate_root,
        "score_delta",
        "mean_score",
        "score",
        score_seed,
        sample_mean,
        MetricExpectation::OptionalPerPayload,
    );
    let mean_delta_interval = bootstrap_metric_interval(
        baseline_root,
        candidate_root,
        "mean_delta",
        "mean_ms",
        "latency_ms",
        mean_seed,
        sample_mean,
        MetricExpectation::OptionalPerPayload,
    );
    let median_latency_delta_interval = bootstrap_metric_interval(
        baseline_root,
        candidate_root,
        "median_latency_delta",
        "p50_ms",
        "latency_ms",
        median_seed,
        sample_median,
        MetricExpectation::RequiredWhenBaselineExists,
    );
    let p95_latency_delta_interval = bootstrap_metric_interval(
        baseline_root,
        candidate_root,
        "p95_latency_delta",
        "p95_ms",
        "latency_ms",
        p95_seed,
        sample_p95,
        MetricExpectation::RequiredWhenBaselineExists,
    );
    let verdict_distribution_drift = verdict_distribution_drift(baseline_root, candidate_root);
    let latency_distribution_drift = latency_distribution_drift(baseline_root, candidate_root);

    let method_entries = [
        ("success_rate_confidence_interval", &success_rate_interval),
        ("score_delta_confidence_interval", &score_delta_interval),
        ("mean_delta_confidence_interval", &mean_delta_interval),
        (
            "median_latency_delta_confidence_interval",
            &median_latency_delta_interval,
        ),
        (
            "p95_latency_delta_confidence_interval",
            &p95_latency_delta_interval,
        ),
        ("verdict_distribution_drift", &verdict_distribution_drift),
        ("latency_distribution_drift", &latency_distribution_drift),
    ];

    let measured_methods = method_entries
        .iter()
        .filter_map(|(name, value)| {
            (value["status"].as_str() == Some("measured")).then_some((*name).to_string())
        })
        .collect::<Vec<_>>();
    let not_applicable_methods = method_entries
        .iter()
        .filter_map(|(name, value)| {
            (value["status"].as_str() == Some("not_applicable")).then_some((*name).to_string())
        })
        .collect::<Vec<_>>();
    let mut not_measured_methods = method_entries
        .iter()
        .filter_map(|(name, value)| {
            (value["status"].as_str() == Some("not_measured")).then_some((*name).to_string())
        })
        .collect::<Vec<_>>();

    if success_rate_interval["status"].as_str() == Some("not_measured") {
        not_measured_methods.push("success_rate_confidence_interval".to_string());
    }

    let drift_summary = if baseline_run_id.is_none() {
        json!({
            "status": "not_measured",
            "reason": BASELINE_PAIR_REASON,
            "measured_methods": measured_methods,
            "not_applicable_methods": not_applicable_methods,
            "not_measured_methods": not_measured_methods,
        })
    } else {
        json!({
            "status": if not_measured_methods.is_empty() { "measured" } else { "partially_measured" },
            "measured_methods": measured_methods,
            "not_applicable_methods": not_applicable_methods,
            "not_measured_methods": not_measured_methods,
        })
    };

    let mut blockers = extra_blockers.to_vec();
    if baseline_run_id.is_none() {
        blockers.extend(
            [
                "baseline_run_id_missing",
                "score_delta_interval_not_measured",
                "mean_delta_interval_not_measured",
                "median_latency_delta_interval_not_measured",
                "p95_latency_delta_interval_not_measured",
                "verdict_distribution_drift_not_measured",
                "latency_distribution_drift_not_measured",
                "drift_summary_not_measured",
            ]
            .into_iter()
            .map(str::to_string),
        );
    } else {
        blockers.extend(method_blockers(
            "score_delta_interval_not_measured",
            &score_delta_interval,
        ));
        blockers.extend(method_blockers(
            "mean_delta_interval_not_measured",
            &mean_delta_interval,
        ));
        blockers.extend(method_blockers(
            "median_latency_delta_interval_not_measured",
            &median_latency_delta_interval,
        ));
        blockers.extend(method_blockers(
            "p95_latency_delta_interval_not_measured",
            &p95_latency_delta_interval,
        ));
        blockers.extend(method_blockers(
            "verdict_distribution_drift_not_measured",
            &verdict_distribution_drift,
        ));
        blockers.extend(method_blockers(
            "latency_distribution_drift_not_measured",
            &latency_distribution_drift,
        ));
        if drift_summary["status"].as_str() == Some("partially_measured") {
            blockers.push("drift_summary_partially_measured".to_string());
        }
    }
    if success_rate_interval["status"].as_str() == Some("not_measured") {
        blockers.push("success_rate_interval_not_measured".to_string());
    }

    let (promotion_fail_closed, promotion_reason) = if blockers.iter().any(|blocker| {
        blocker.ends_with("_not_measured")
            || blocker == "baseline_run_id_missing"
            || blocker == "drift_summary_not_measured"
            || blocker == "drift_summary_partially_measured"
            || blocker == "success_rate_interval_not_measured"
    }) {
        (true, PROMOTION_REASON)
    } else if !extra_blockers.is_empty() {
        (false, BENCHMARK_GATE_FAILURES_REASON)
    } else {
        (false, PROMOTION_POLICY_REASON)
    };

    json!({
        "statistics_version": STATISTICS_VERSION,
        "sample_size": sample_size,
        "baseline_run_id": baseline_run_id,
        "candidate_run_id": candidate_run_id.to_string(),
        "methods": {
            "success_rate_confidence_interval": success_rate_interval,
            "score_delta_confidence_interval": score_delta_interval,
            "mean_delta_confidence_interval": mean_delta_interval,
            "median_latency_delta_confidence_interval": median_latency_delta_interval,
            "p95_latency_delta_confidence_interval": p95_latency_delta_interval,
            "verdict_distribution_drift": verdict_distribution_drift,
            "latency_distribution_drift": latency_distribution_drift,
        },
        "drift_summary": drift_summary,
        "promotion": {
            "verdict": "not_promotable",
            "fail_closed": promotion_fail_closed,
            "reason": promotion_reason,
            "blockers": blockers,
        }
    })
}

#[derive(Clone, Copy)]
enum MetricExpectation {
    RequiredWhenBaselineExists,
    OptionalPerPayload,
}

fn bootstrap_metric_interval(
    baseline_root: Option<&Value>,
    candidate_root: &Value,
    metric: &'static str,
    aggregate_field: &str,
    task_field: &str,
    bootstrap_seed: u64,
    estimator: fn(&[f64]) -> f64,
    expectation: MetricExpectation,
) -> Value {
    let Some(baseline_root) = baseline_root else {
        return not_measured_delta(metric);
    };
    let candidate_aggregate = candidate_root[aggregate_field].as_f64();
    let baseline_aggregate = baseline_root[aggregate_field].as_f64();
    let candidate_samples = task_samples(candidate_root, task_field);
    let baseline_samples = task_samples(baseline_root, task_field);

    if candidate_aggregate.is_none() || baseline_aggregate.is_none() {
        return match expectation {
            MetricExpectation::RequiredWhenBaselineExists => not_measured_delta(metric),
            MetricExpectation::OptionalPerPayload => not_applicable_delta(metric),
        };
    }
    if candidate_samples.is_empty() || baseline_samples.is_empty() {
        return not_measured_delta(metric);
    }

    let (lower, upper) = bootstrap_percentile_interval(
        &candidate_samples,
        &baseline_samples,
        bootstrap_seed,
        BOOTSTRAP_ITERATIONS,
        estimator,
    );
    let delta = candidate_aggregate.unwrap_or_default() - baseline_aggregate.unwrap_or_default();
    json!({
        "status": "measured",
        "method": "bootstrap_percentile_95",
        "metric": metric,
        "confidence_level": 0.95,
        "bootstrap_seed": bootstrap_seed,
        "candidate_value": candidate_aggregate,
        "baseline_value": baseline_aggregate,
        "delta": delta,
        "lower": lower,
        "upper": upper,
        "candidate_sample_size": candidate_samples.len(),
        "baseline_sample_size": baseline_samples.len(),
    })
}

fn verdict_distribution_drift(baseline_root: Option<&Value>, candidate_root: &Value) -> Value {
    let Some(baseline_root) = baseline_root else {
        return not_measured_drift("verdict_distribution", "jensen_shannon_divergence");
    };
    let candidate = task_strings(candidate_root, "eval_verdict_class");
    let baseline = task_strings(baseline_root, "eval_verdict_class");
    if candidate.is_empty() || baseline.is_empty() {
        return not_measured_drift("verdict_distribution", "jensen_shannon_divergence");
    }
    let divergence = jensen_shannon_divergence(&candidate, &baseline);
    json!({
        "status": "measured",
        "method": "jensen_shannon_divergence",
        "metric": "verdict_distribution",
        "value": divergence,
        "candidate_sample_size": candidate.len(),
        "baseline_sample_size": baseline.len(),
    })
}

fn latency_distribution_drift(baseline_root: Option<&Value>, candidate_root: &Value) -> Value {
    let Some(baseline_root) = baseline_root else {
        return not_measured_drift("latency_distribution", "kolmogorov_smirnov");
    };
    let candidate = task_samples(candidate_root, "latency_ms");
    let baseline = task_samples(baseline_root, "latency_ms");
    if candidate.is_empty() || baseline.is_empty() {
        return not_measured_drift("latency_distribution", "kolmogorov_smirnov");
    }
    let statistic = kolmogorov_smirnov_statistic(&candidate, &baseline);
    json!({
        "status": "measured",
        "method": "kolmogorov_smirnov",
        "metric": "latency_distribution",
        "value": statistic,
        "candidate_sample_size": candidate.len(),
        "baseline_sample_size": baseline.len(),
    })
}

fn not_measured_delta(metric: &'static str) -> Value {
    json!({
        "status": "not_measured",
        "method": "bootstrap_percentile_95",
        "metric": metric,
        "confidence_level": 0.95,
        "bootstrap_seed": Value::Null,
        "reason": BASELINE_PAIR_REASON,
    })
}

fn not_applicable_delta(metric: &'static str) -> Value {
    json!({
        "status": "not_applicable",
        "method": "bootstrap_percentile_95",
        "metric": metric,
        "confidence_level": 0.95,
        "bootstrap_seed": Value::Null,
        "reason": "metric_not_available_for_payload_kind",
    })
}

fn not_measured_drift(metric: &'static str, method: &'static str) -> Value {
    json!({
        "status": "not_measured",
        "method": method,
        "metric": metric,
        "reason": BASELINE_PAIR_REASON,
    })
}

fn method_blockers(blocker: &str, method: &Value) -> Vec<String> {
    (method["status"].as_str() == Some("not_measured"))
        .then_some(vec![blocker.to_string()])
        .unwrap_or_default()
}

fn task_samples(root: &Value, field_name: &str) -> Vec<f64> {
    root["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|task| task[field_name].as_f64())
        .collect()
}

fn task_strings(root: &Value, field_name: &str) -> Vec<String> {
    root["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|task| task[field_name].as_str().map(ToOwned::to_owned))
        .collect()
}

fn bootstrap_seed(baseline_run_id: Option<&str>, candidate_run_id: Uuid, lane: u64) -> u64 {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ lane;
    for chunk in candidate_run_id.as_bytes().chunks(8) {
        seed ^= fold_chunk(chunk).rotate_left(13);
    }
    if let Some(run_id) = baseline_run_id.and_then(|value| Uuid::parse_str(value).ok()) {
        for chunk in run_id.as_bytes().chunks(8) {
            seed ^= fold_chunk(chunk).rotate_left(29);
        }
    }
    if seed == 0 {
        0xfeed_face_cafe_babe
    } else {
        seed
    }
}

fn fold_chunk(chunk: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    for (index, byte) in chunk.iter().enumerate() {
        bytes[index] = *byte;
    }
    u64::from_le_bytes(bytes)
}

fn bootstrap_percentile_interval(
    candidate_samples: &[f64],
    baseline_samples: &[f64],
    seed: u64,
    iterations: usize,
    estimator: fn(&[f64]) -> f64,
) -> (f64, f64) {
    let mut rng = DeterministicRng::new(seed);
    let mut deltas = Vec::with_capacity(iterations.max(1));
    for _ in 0..iterations.max(1) {
        let candidate = sample_with_replacement(candidate_samples, &mut rng);
        let baseline = sample_with_replacement(baseline_samples, &mut rng);
        deltas.push(estimator(&candidate) - estimator(&baseline));
    }
    deltas.sort_by(f64::total_cmp);
    let lower_index = percentile_index(deltas.len(), 0.025);
    let upper_index = percentile_index(deltas.len(), 0.975);
    (deltas[lower_index], deltas[upper_index])
}

fn percentile_index(len: usize, percentile: f64) -> usize {
    if len <= 1 {
        return 0;
    }
    let index = ((len - 1) as f64 * percentile).round() as usize;
    index.min(len - 1)
}

fn sample_with_replacement(samples: &[f64], rng: &mut DeterministicRng) -> Vec<f64> {
    let mut picked = Vec::with_capacity(samples.len());
    for _ in 0..samples.len() {
        picked.push(samples[rng.gen_index(samples.len())]);
    }
    picked
}

fn sample_mean(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f64>() / samples.len() as f64
}

fn sample_median(samples: &[f64]) -> f64 {
    percentile_from_sorted(samples, 0.5)
}

fn sample_p95(samples: &[f64]) -> f64 {
    percentile_from_sorted(samples, 0.95)
}

fn percentile_from_sorted(samples: &[f64], percentile: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = percentile_index(sorted.len(), percentile);
    sorted[index]
}

fn jensen_shannon_divergence(candidate: &[String], baseline: &[String]) -> f64 {
    let mut counts = Map::new();
    for value in candidate.iter().chain(baseline.iter()) {
        counts.entry(value.clone()).or_insert(Value::Null);
    }
    let candidate_total = candidate.len() as f64;
    let baseline_total = baseline.len() as f64;
    let mut p = Vec::with_capacity(counts.len());
    let mut q = Vec::with_capacity(counts.len());
    for key in counts.keys() {
        let candidate_count = candidate.iter().filter(|value| *value == key).count() as f64;
        let baseline_count = baseline.iter().filter(|value| *value == key).count() as f64;
        p.push(candidate_count / candidate_total);
        q.push(baseline_count / baseline_total);
    }
    let m = p
        .iter()
        .zip(q.iter())
        .map(|(a, b)| 0.5 * (a + b))
        .collect::<Vec<_>>();
    0.5 * kl_divergence(&p, &m) + 0.5 * kl_divergence(&q, &m)
}

fn kl_divergence(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right.iter())
        .filter(|(l, r)| **l > 0.0 && **r > 0.0)
        .map(|(l, r)| l * (l / r).ln())
        .sum()
}

fn kolmogorov_smirnov_statistic(candidate: &[f64], baseline: &[f64]) -> f64 {
    let mut candidate_sorted = candidate.to_vec();
    candidate_sorted.sort_by(f64::total_cmp);
    let mut baseline_sorted = baseline.to_vec();
    baseline_sorted.sort_by(f64::total_cmp);
    let mut i = 0usize;
    let mut j = 0usize;
    let mut d = 0.0f64;
    while i < candidate_sorted.len() || j < baseline_sorted.len() {
        let next = match (candidate_sorted.get(i), baseline_sorted.get(j)) {
            (Some(left), Some(right)) => left.min(*right),
            (Some(left), None) => *left,
            (None, Some(right)) => *right,
            (None, None) => break,
        };
        while i < candidate_sorted.len() && candidate_sorted[i] <= next {
            i += 1;
        }
        while j < baseline_sorted.len() && baseline_sorted[j] <= next {
            j += 1;
        }
        let cdf_candidate = i as f64 / candidate_sorted.len() as f64;
        let cdf_baseline = j as f64 / baseline_sorted.len() as f64;
        d = d.max((cdf_candidate - cdf_baseline).abs());
    }
    d
}

fn wilson_interval(success_count: usize, sample_size: usize, z: f64) -> (f64, f64) {
    let n = sample_size as f64;
    let p_hat = success_count as f64 / n;
    let z_sq = z * z;
    let denominator = 1.0 + z_sq / n;
    let center = (p_hat + z_sq / (2.0 * n)) / denominator;
    let radius = ((p_hat * (1.0 - p_hat) + z_sq / (4.0 * n)) / n).sqrt() * z / denominator;
    ((center - radius).max(0.0), (center + radius).min(1.0))
}

struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn gen_index(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            0
        } else {
            (self.next_u64() % upper as u64) as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_block_stays_honest_for_missing_baseline_pair() {
        let candidate = json!({
            "memory_task_matrix": {
                "tasks_total": 2,
                "tasks_passed": 2,
                "mean_score": 1.0,
                "p50_ms": 10.0,
                "p95_ms": 12.0,
                "tasks": [
                    {"score": 1.0, "latency_ms": 10.0, "eval_verdict_class": "hit_correct_target", "success": true},
                    {"score": 1.0, "latency_ms": 12.0, "eval_verdict_class": "hit_correct_target", "success": true}
                ]
            }
        });
        let payload =
            statistics_block_from_pair("memory_task_matrix", &candidate, None, Uuid::nil(), &[]);
        assert_eq!(payload["baseline_run_id"], Value::Null);
        assert_eq!(
            payload["drift_summary"]["status"].as_str(),
            Some("not_measured")
        );
        assert_eq!(payload["promotion"]["fail_closed"].as_bool(), Some(true));
    }

    #[test]
    fn statistics_block_materializes_pairwise_methods_for_memory_payload() {
        let baseline = json!({
            "_observability": {"source_event_id": Uuid::new_v4().to_string()},
            "memory_task_matrix": {
                "tasks_total": 3,
                "tasks_passed": 2,
                "mean_score": 0.8,
                "p50_ms": 13.0,
                "p95_ms": 19.0,
                "tasks": [
                    {"score": 1.0, "latency_ms": 11.0, "eval_verdict_class": "hit_correct_target", "success": true},
                    {"score": 0.4, "latency_ms": 13.0, "eval_verdict_class": "recovered_useful", "success": false},
                    {"score": 1.0, "latency_ms": 19.0, "eval_verdict_class": "hit_correct_target", "success": true}
                ]
            }
        });
        let candidate = json!({
            "memory_task_matrix": {
                "tasks_total": 3,
                "tasks_passed": 3,
                "mean_score": 1.0,
                "p50_ms": 9.0,
                "p95_ms": 14.0,
                "tasks": [
                    {"score": 1.0, "latency_ms": 8.0, "eval_verdict_class": "hit_correct_target", "success": true},
                    {"score": 1.0, "latency_ms": 9.0, "eval_verdict_class": "hit_correct_target", "success": true},
                    {"score": 1.0, "latency_ms": 14.0, "eval_verdict_class": "hit_correct_target", "success": true}
                ]
            }
        });
        let payload = statistics_block_from_pair(
            "memory_task_matrix",
            &candidate,
            Some(&baseline),
            Uuid::new_v4(),
            &[],
        );
        assert!(payload["baseline_run_id"].as_str().is_some());
        assert_eq!(
            payload["methods"]["score_delta_confidence_interval"]["status"].as_str(),
            Some("measured")
        );
        assert_eq!(
            payload["methods"]["mean_delta_confidence_interval"]["status"].as_str(),
            Some("not_applicable")
        );
        assert_eq!(
            payload["methods"]["median_latency_delta_confidence_interval"]["status"].as_str(),
            Some("measured")
        );
        assert_eq!(
            payload["methods"]["p95_latency_delta_confidence_interval"]["status"].as_str(),
            Some("measured")
        );
        assert_eq!(
            payload["methods"]["verdict_distribution_drift"]["status"].as_str(),
            Some("measured")
        );
        assert_eq!(
            payload["methods"]["latency_distribution_drift"]["status"].as_str(),
            Some("measured")
        );
        assert_eq!(
            payload["drift_summary"]["status"].as_str(),
            Some("measured")
        );
        assert_eq!(payload["promotion"]["fail_closed"].as_bool(), Some(false));
        assert_eq!(
            payload["promotion"]["reason"].as_str(),
            Some(PROMOTION_POLICY_REASON)
        );
    }
}
