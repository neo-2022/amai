#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${REPO_ROOT}/scripts/load_env.sh"

cd "${REPO_ROOT}"
./scripts/benchmark_contamination_preflight.sh

psql "$AMI_POSTGRES_DSN" <<'SQL' >/dev/null
DELETE FROM ami.observability_snapshots
WHERE snapshot_kind = 'memory_task_matrix'
  AND scope_project_code = 'amai'
  AND scope_namespace_code = 'letta_memory_local';
SQL

run_matrix() {
    cargo run --release --quiet -- verify memory-matrix --matrix letta_memory_local --project-prefix "${project_prefix}"
}

assert_output() {
    local output="$1"
    printf '%s\n' "$output" | rg '"matrix": "letta_memory_local"' >/dev/null
    printf '%s\n' "$output" | rg '"tasks_failed": 0' >/dev/null
    printf '%s\n' "$output" | rg '"success_rate": 1\.0' >/dev/null
    printf '%s\n' "$output" | rg '"mean_score": 1\.0' >/dev/null
    printf '%s\n' "$output" | rg '"class": "read"' >/dev/null
    printf '%s\n' "$output" | rg '"class": "write"' >/dev/null
    printf '%s\n' "$output" | rg '"class": "update"' >/dev/null
    printf '%s\n' "$output" | rg '"class": "isolation"' >/dev/null
    printf '%s\n' "$output" | rg '"layer": "core"' >/dev/null
    printf '%s\n' "$output" | rg '"layer": "archival"' >/dev/null
    printf '%s\n' "$output" | rg '"eval_verdict_model_version": "memory-eval-verdict-v1"' >/dev/null
    printf '%s\n' "$output" | rg '"eval_verdict_class": "hit_correct_target"' >/dev/null
    printf '%s\n' "$output" | rg '"eval_verdict_class": "recovered_useful"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.sample_size == .memory_task_matrix.tasks_total' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.candidate_run_id | type == "string"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.success_rate_confidence_interval.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.success_rate_confidence_interval.method == "wilson_95"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.promotion.verdict == "not_promotable"' >/dev/null
}

assert_first_output() {
    local output="$1"
    assert_output "$output"
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.baseline_run_id == null' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.drift_summary.status == "not_measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.measured_approval.state == "blocked_statistics_incomplete"' >/dev/null
}

assert_second_output() {
    local output="$1"
    local expected_baseline_run_id="$2"
    assert_output "$output"
    printf '%s\n' "$output" | jq -e --arg baseline "$expected_baseline_run_id" '.memory_task_matrix.statistics.baseline_run_id == $baseline' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.score_delta_confidence_interval.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.mean_delta_confidence_interval.status == "not_applicable"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.median_latency_delta_confidence_interval.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.p95_latency_delta_confidence_interval.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.verdict_distribution_drift.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.methods.latency_distribution_drift.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.drift_summary.status == "measured"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.promotion.fail_closed == false' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.statistics.promotion.reason == "promotion_policy_not_materialized"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.promotion_law.state == "candidate_ready_for_measured_approval"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.promotion_law.candidate_ready_for_measured_approval == true' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.promotion_law.reason == "measured_approval_policy_not_materialized"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.measured_approval.verdict == "pending_human_review"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.measured_approval.reason == "explicit_human_signoff_required"' >/dev/null
    printf '%s\n' "$output" | jq -e '.memory_task_matrix.measured_approval.review_packet_ready == true' >/dev/null
}

project_prefix="proof_memory_matrix_$(date +%s%N)"

first_output="$(run_matrix)"
second_output="$(run_matrix)"

first_candidate_run_id="$(printf '%s\n' "$first_output" | jq -r '.memory_task_matrix.statistics.candidate_run_id')"

assert_first_output "$first_output"
assert_second_output "$second_output" "$first_candidate_run_id"

recent_sources="$(
  psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<SQL
SELECT
  payload->'token_budget_event'->>'source_kind' AS source_kind,
  payload->'token_budget_event'->>'traffic_class' AS traffic_class,
  COUNT(*) AS events
FROM ami.observability_snapshots
WHERE snapshot_kind = 'token_budget_event'
  AND payload->'_observability'->>'scope_project_code' LIKE '${project_prefix}_%'
GROUP BY
  payload->'token_budget_event'->>'source_kind',
  payload->'token_budget_event'->>'traffic_class'
ORDER BY
  payload->'token_budget_event'->>'source_kind';
SQL
)"

printf '%s\n' "$recent_sources" | rg '^verify_memory_matrix_context_pack\tverify\t[1-9][0-9]*$' >/dev/null
if printf '%s\n' "$recent_sources" | rg '^live_context_pack\tlive\t' >/dev/null; then
  echo "proof_memory_task_matrix: memory matrix leaked live_context_pack into token ledger" >&2
  exit 1
fi

printf 'proof_memory_task_matrix: ok\n'
