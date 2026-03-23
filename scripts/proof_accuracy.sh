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

output="$(cargo run --release --quiet -- verify accuracy \
  --project project_alpha \
  --related-project project_beta \
  --namespace review)"

printf '%s\n' "$output" | rg '"accuracy_verification"' >/dev/null
printf '%s\n' "$output" | rg '"canonical_eval"' >/dev/null
printf '%s\n' "$output" | rg '"eval_verdict_model_version": "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$output" | rg '"name": "strict_local_fail_closed"' >/dev/null
printf '%s\n' "$output" | rg '"name": "hostile_fail_closed"' >/dev/null

printf 'proof_accuracy: ok\n'
