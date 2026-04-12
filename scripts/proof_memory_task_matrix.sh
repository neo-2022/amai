#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${REPO_ROOT}/scripts/load_env.sh"

cd "${REPO_ROOT}"
./scripts/benchmark_contamination_preflight.sh

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
}

project_prefix="proof_memory_matrix_$(date +%s%N)"

first_output="$(run_matrix)"
second_output="$(run_matrix)"

assert_output "$first_output"
assert_output "$second_output"

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
