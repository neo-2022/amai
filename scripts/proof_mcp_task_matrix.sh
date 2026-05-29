#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source "./scripts/load_env.sh"

./scripts/benchmark_contamination_preflight.sh --strict-heavy
./scripts/bootstrap_stack.sh

psql "$AMI_POSTGRES_DSN" <<'SQL' >/dev/null
DELETE FROM ami.observability_snapshots
WHERE snapshot_kind = 'mcp_task_matrix'
  AND scope_project_code = 'amai'
  AND scope_namespace_code IN ('live_mcpbench_local', 'mcp_universe_local');
SQL

cargo run --release --quiet -- project register \
  --code project_alpha \
  --display-name "Project Alpha" \
  --repo-root "$PWD/fixtures/project_alpha"

cargo run --release --quiet -- project register \
  --code project_beta \
  --display-name "Project Beta" \
  --repo-root "$PWD/fixtures/project_beta"

cargo run --release --quiet -- namespace ensure \
  --project project_alpha \
  --code review \
  --display-name Review \
  --retrieval-mode local_plus_related

cargo run --release --quiet -- namespace ensure \
  --project project_beta \
  --code review \
  --display-name Review \
  --retrieval-mode local_plus_related

cargo run --release --quiet -- relation add \
  --source project_alpha \
  --target project_beta \
  --relation-type shared_runtime \
  --shared-contour common_contour \
  --access-mode local_plus_related

cargo run --release --quiet -- index project \
  --code project_alpha \
  --path "$PWD/fixtures/project_alpha" \
  --namespace review \
  --limit-files 20

cargo run --release --quiet -- index project \
  --code project_beta \
  --path "$PWD/fixtures/project_beta" \
  --namespace review \
  --limit-files 20

live_output_first="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix live_mcpbench_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile codex_5h)"

live_output_second="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix live_mcpbench_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile codex_5h)"

assert_live_common() {
  local output="$1"
  printf '%s\n' "$output" | rg '"matrix": "live_mcpbench_local"' >/dev/null
  printf '%s\n' "$output" | rg '"tasks_failed": 0' >/dev/null
  printf '%s\n' "$output" | rg '"success_rate": 1.0' >/dev/null
  printf '%s\n' "$output" | rg '"class": "hostile"' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.canonical_eval.verdict_counts.hit_correct_target == 11' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.statistics.sample_size == .mcp_task_matrix.tasks_total' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.statistics.candidate_run_id | type == "string"' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.statistics.methods.success_rate_confidence_interval.status == "measured"' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.statistics.methods.success_rate_confidence_interval.method == "wilson_95"' >/dev/null
  printf '%s\n' "$output" | jq -e '.mcp_task_matrix.statistics.promotion.verdict == "not_promotable"' >/dev/null
}

assert_live_common "$live_output_first"
printf '%s\n' "$live_output_first" | jq -e '.mcp_task_matrix.statistics.baseline_run_id == null' >/dev/null
printf '%s\n' "$live_output_first" | jq -e '.mcp_task_matrix.statistics.drift_summary.status == "not_measured"' >/dev/null
printf '%s\n' "$live_output_first" | jq -e '.mcp_task_matrix.measured_approval.state == "blocked_statistics_incomplete"' >/dev/null

first_live_candidate_run_id="$(printf '%s\n' "$live_output_first" | jq -r '.mcp_task_matrix.statistics.candidate_run_id')"
assert_live_common "$live_output_second"
printf '%s\n' "$live_output_second" | jq -e --arg baseline "$first_live_candidate_run_id" '.mcp_task_matrix.statistics.baseline_run_id == $baseline' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.score_delta_confidence_interval.status == "not_applicable"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.mean_delta_confidence_interval.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.median_latency_delta_confidence_interval.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.p95_latency_delta_confidence_interval.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.verdict_distribution_drift.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.latency_distribution_drift.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.drift_summary.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.promotion.fail_closed == false' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.promotion.reason == "promotion_policy_not_materialized"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.promotion_law.state == "candidate_ready_for_measured_approval"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.promotion_law.candidate_ready_for_measured_approval == true' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.promotion_law.reason == "measured_approval_policy_not_materialized"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.verdict == "pending_human_review"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.reason == "explicit_human_signoff_required"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.review_packet_ready == true' >/dev/null

universe_output="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix mcp_universe_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile codex_5h)"

printf '%s\n' "$universe_output" | rg '"matrix": "mcp_universe_local"' >/dev/null
printf '%s\n' "$universe_output" | rg '"tasks_failed": 0' >/dev/null
printf '%s\n' "$universe_output" | rg '"class": "isolation"' >/dev/null
printf '%s\n' "$universe_output" | rg '"status": "fail_closed"' >/dev/null
printf '%s\n' "$universe_output" | jq -e '.mcp_task_matrix.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$universe_output" | jq -e '.mcp_task_matrix.canonical_eval.verdict_counts.hit_correct_target == 8' >/dev/null
printf '%s\n' "$universe_output" | jq -e '.mcp_task_matrix.canonical_eval.verdict_counts.recovered_useful == 1' >/dev/null
printf '%s\n' "$universe_output" | jq -e '.mcp_task_matrix.statistics.baseline_run_id == null' >/dev/null
printf '%s\n' "$universe_output" | jq -e '.mcp_task_matrix.statistics.promotion.verdict == "not_promotable"' >/dev/null
printf '%s\n' "$universe_output" | jq -e '.mcp_task_matrix.measured_approval.state == "blocked_statistics_incomplete"' >/dev/null

printf 'proof_mcp_task_matrix: ok\n'
