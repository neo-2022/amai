#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source "./scripts/stage2_fixture_roots.sh"
stage2_prepare_fixture_roots "$PWD"

./scripts/bootstrap_stack.sh

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

for workers in 50 100 200; do
  cargo run --release --quiet -- verify load \
    --project project_alpha \
    --namespace review \
    --query "shared_runtime_marker" \
    --retrieval-mode local_plus_related \
    --limit-documents 8 \
    --limit-symbols 8 \
    --limit-chunks 8 \
    --limit-semantic-chunks 8 \
    --workers "$workers" \
    --iterations-per-worker 10 \
    --warmup-per-worker 1 \
    --min-qps 5000 \
    --max-p95-ms 10 \
    --max-error-rate 0
done
