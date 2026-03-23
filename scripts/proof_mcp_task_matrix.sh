#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bootstrap_stack.sh

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

live_output="$(cargo run --release --quiet -- verify mcp-matrix \
  --matrix live_mcpbench_local \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --budget-profile codex_5h)"

printf '%s\n' "$live_output" | rg '"matrix": "live_mcpbench_local"' >/dev/null
printf '%s\n' "$live_output" | rg '"tasks_failed": 0' >/dev/null
printf '%s\n' "$live_output" | rg '"success_rate": 1.0' >/dev/null
printf '%s\n' "$live_output" | rg '"class": "hostile"' >/dev/null
printf '%s\n' "$live_output" | jq -e '.mcp_task_matrix.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$live_output" | jq -e '.mcp_task_matrix.canonical_eval.verdict_counts.hit_correct_target == 11' >/dev/null

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

printf 'proof_mcp_task_matrix: ok\n'
