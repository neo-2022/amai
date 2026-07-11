#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source "./scripts/stage2_fixture_roots.sh"
stage2_prepare_fixture_roots "$PWD"
source "./scripts/load_env.sh"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

assert_mcp_statistics_integrity() {
  local output="$1"
  printf '%s\n' "$output" | jq -e '
    .mcp_task_matrix.statistics.methods as $m
    | (
        $m.success_rate_confidence_interval.status == "measured"
        and $m.success_rate_confidence_interval.method == "wilson_95"
        and ($m.success_rate_confidence_interval.confidence_level | type == "number")
        and ($m.success_rate_confidence_interval.lower | type == "number")
        and ($m.success_rate_confidence_interval.upper | type == "number")
      )
    and (
        $m.score_delta_confidence_interval.status == "not_applicable"
        and ($m.score_delta_confidence_interval.reason | type == "string")
      )
    and (
        $m.mean_delta_confidence_interval.status == "measured"
        and $m.mean_delta_confidence_interval.method == "bootstrap_percentile_95"
        and ($m.mean_delta_confidence_interval.delta | type == "number")
        and ($m.mean_delta_confidence_interval.lower | type == "number")
        and ($m.mean_delta_confidence_interval.upper | type == "number")
      )
    and (
        $m.median_latency_delta_confidence_interval.status == "measured"
        and $m.median_latency_delta_confidence_interval.method == "bootstrap_percentile_95"
        and ($m.median_latency_delta_confidence_interval.delta | type == "number")
        and ($m.median_latency_delta_confidence_interval.lower | type == "number")
        and ($m.median_latency_delta_confidence_interval.upper | type == "number")
      )
    and (
        $m.p95_latency_delta_confidence_interval.status == "measured"
        and $m.p95_latency_delta_confidence_interval.method == "bootstrap_percentile_95"
        and ($m.p95_latency_delta_confidence_interval.delta | type == "number")
        and ($m.p95_latency_delta_confidence_interval.lower | type == "number")
        and ($m.p95_latency_delta_confidence_interval.upper | type == "number")
      )
    and (
        $m.verdict_distribution_drift.status == "measured"
        and $m.verdict_distribution_drift.method == "jensen_shannon_divergence"
        and ($m.verdict_distribution_drift.value | type == "number")
      )
    and (
        $m.latency_distribution_drift.status == "measured"
        and $m.latency_distribution_drift.method == "kolmogorov_smirnov"
        and ($m.latency_distribution_drift.value | type == "number")
      )
    and (
        $m.score_distribution_drift.status == "not_applicable"
        and ($m.score_distribution_drift.reason | type == "string")
      )
  ' >/dev/null
}

insert_foreign_live_probe() {
  local foreign_run_id="$1"
  local foreign_captured_at_epoch_ms="$2"
  local foreign_payload_file="${tmp_dir}/foreign-mcp-task-matrix.json"
  jq -n \
    --arg run_id "$foreign_run_id" \
    --argjson captured_at_epoch_ms "$foreign_captured_at_epoch_ms" \
    '
      {
        "_observability": {
          "source_event_id": $run_id,
          "source_kind": "mcp_task_matrix_run",
          "scope_project_code": "foreign_project",
          "scope_namespace_code": "live_mcpbench_local",
          "captured_at_epoch_ms": $captured_at_epoch_ms
        },
        "mcp_task_matrix": {
          "matrix": "live_mcpbench_local",
          "statistics": {
            "statistics_version": "benchmark-statistics-v1",
            "sample_size": 1,
            "baseline_run_id": $run_id,
            "candidate_run_id": $run_id,
            "drift_summary": {
              "status": "measured",
              "measured_methods": [],
              "not_applicable_methods": [],
              "not_measured_methods": []
            },
            "methods": {
              "score_distribution_drift": {
                "status": "not_applicable",
                "reason": "foreign_scope_guard_probe"
              }
            }
          },
          "promotion_law": {
            "state": "candidate_ready_for_measured_approval"
          },
          "measured_approval": {
            "verdict": "approved",
            "state": "approved"
          }
        }
      }
    ' >"${foreign_payload_file}"
  local foreign_payload
  local foreign_payload_sha256
  foreign_payload="$(jq -c . "${foreign_payload_file}")"
  foreign_payload_sha256="$(printf '%s' "$foreign_payload" | sha256sum | awk '{print $1}')"
  psql "$AMI_POSTGRES_DSN" \
    -v ON_ERROR_STOP=1 \
    -v payload="$foreign_payload" \
    -v source_event_id="$foreign_run_id" \
    -v captured_at_epoch_ms="$foreign_captured_at_epoch_ms" \
    -v payload_sha256="$foreign_payload_sha256" <<'SQL' >/dev/null
INSERT INTO ami.observability_snapshots(
    snapshot_kind,
    payload,
    event_key,
    source_event_id,
    source_kind,
    source_class,
    scope_project_code,
    scope_namespace_code,
    captured_at_epoch_ms,
    payload_sha256,
    last_seen_at
)
VALUES (
    'mcp_task_matrix',
    :'payload'::jsonb,
    :'source_event_id',
    :'source_event_id',
    'mcp_task_matrix_run',
    'benchmark',
    'foreign_project',
    'live_mcpbench_local',
    :captured_at_epoch_ms::bigint,
    :'payload_sha256',
    now()
)
ON CONFLICT (snapshot_kind, event_key) DO UPDATE
SET payload = EXCLUDED.payload,
    source_event_id = EXCLUDED.source_event_id,
    scope_project_code = EXCLUDED.scope_project_code,
    scope_namespace_code = EXCLUDED.scope_namespace_code,
    captured_at_epoch_ms = EXCLUDED.captured_at_epoch_ms,
    payload_sha256 = EXCLUDED.payload_sha256,
    last_seen_at = now();
SQL
}

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
  --repo-root "${AMAI_STAGE2_PROJECT_ALPHA_ROOT}"

cargo run --release --quiet -- project register \
  --code project_beta \
  --display-name "Project Beta" \
  --repo-root "${AMAI_STAGE2_PROJECT_BETA_ROOT}"

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
  --path "${AMAI_STAGE2_PROJECT_ALPHA_ROOT}" \
  --namespace review \
  --limit-files 20

cargo run --release --quiet -- index project \
  --code project_beta \
  --path "${AMAI_STAGE2_PROJECT_BETA_ROOT}" \
  --namespace review \
  --limit-files 20

live_output_first="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix live_mcpbench_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile client_primary_budget)"

live_output_second="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix live_mcpbench_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile client_primary_budget)"

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
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.methods.score_distribution_drift.status == "not_applicable"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.drift_summary.status == "measured"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.promotion.fail_closed == false' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.statistics.promotion.reason == "promotion_policy_not_materialized"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.promotion_law.state == "candidate_ready_for_measured_approval"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.promotion_law.candidate_ready_for_measured_approval == true' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.promotion_law.reason == "measured_approval_policy_not_materialized"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.verdict == "approved"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.state == "approved"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.reason == "all_benchmark_gates_passed"' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.auto_promotion_allowed == true' >/dev/null
printf '%s\n' "$live_output_second" | jq -e '.mcp_task_matrix.measured_approval.explicit_human_signoff_required == false' >/dev/null
assert_mcp_statistics_integrity "$live_output_second"

foreign_run_id="$(cat /proc/sys/kernel/random/uuid)"
foreign_captured_at_epoch_ms="$(date +%s%3N)"
insert_foreign_live_probe "$foreign_run_id" "$foreign_captured_at_epoch_ms"

universe_output="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix mcp_universe_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile client_primary_budget)"

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

observe_output="$(cargo run --release --quiet -- observe snapshot)"
second_live_candidate_run_id="$(printf '%s\n' "$live_output_second" | jq -r '.mcp_task_matrix.statistics.candidate_run_id')"
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix.mcp_task_matrix.matrix == "live_mcpbench_local"' >/dev/null
printf '%s\n' "$observe_output" | jq -e '._observability.scope_project_code == "amai" and ._observability.scope_namespace_code == "observe"' >/dev/null
printf '%s\n' "$observe_output" | jq -e --arg run_id "$second_live_candidate_run_id" '.latest_mcp_task_matrix._observability.source_event_id == $run_id' >/dev/null
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix._observability.scope_project_code == "amai"' >/dev/null
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix._observability.scope_namespace_code == "live_mcpbench_local"' >/dev/null
printf '%s\n' "$observe_output" | jq -e --arg run_id "$second_live_candidate_run_id" '.latest_mcp_task_matrix.mcp_task_matrix.statistics.candidate_run_id == $run_id' >/dev/null
printf '%s\n' "$observe_output" | jq -e --arg foreign_run_id "$foreign_run_id" '.latest_mcp_task_matrix._observability.source_event_id != $foreign_run_id' >/dev/null
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix.mcp_task_matrix.statistics.drift_summary.status == "measured"' >/dev/null
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix.mcp_task_matrix.measured_approval.verdict == "approved"' >/dev/null
printf '%s\n' "$observe_output" | jq -e '
  .latest_mcp_task_matrix.mcp_task_matrix.statistics.methods as $m
  | ($m.mean_delta_confidence_interval.delta | type == "number")
  and ($m.median_latency_delta_confidence_interval.lower | type == "number")
  and ($m.p95_latency_delta_confidence_interval.upper | type == "number")
  and ($m.verdict_distribution_drift.value | type == "number")
  and ($m.latency_distribution_drift.value | type == "number")
  and ($m.score_distribution_drift.reason | type == "string")
' >/dev/null
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix_raw_latest.mcp_task_matrix.matrix == "mcp_universe_local"' >/dev/null
printf '%s\n' "$observe_output" | jq -e '.latest_mcp_task_matrix_raw_latest.mcp_task_matrix.measured_approval.state == "blocked_statistics_incomplete"' >/dev/null

mcp_verify_output="$(cargo run --release --quiet -- verify mcp \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 \
  --tokenizer o200k_base \
  --naive-limit-files 20 \
  --naive-max-bytes-per-file 32768 \
  --min-savings-factor 1.2 \
  --min-savings-percent 15 \
  --proof-scope token-ledger \
  --token-source-kind proof_mcp_task_matrix_summary)"
printf '%s\n' "$mcp_verify_output" | jq -e '.mcp_verification.latest_mcp_task_matrix_summary == "compare=measured promotion=candidate_ready_for_measured_approval approval=approved"' >/dev/null
printf '%s\n' "$mcp_verify_output" | jq -e '.mcp_verification.latest_memory_task_matrix_summary | startswith("compare=")' >/dev/null

printf 'proof_mcp_task_matrix: ok\n'
