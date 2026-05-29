#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bootstrap_stack.sh
./scripts/benchmark_contamination_preflight.sh

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

cargo run --release --quiet -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 \
  --warmup 3 \
  --iterations 20 \
  --persist \
  --max-mean-ms 11 \
  --max-p95-ms 12 \
  --max-p99-ms 13 \
  --max-max-ms 15

cargo run --release --quiet -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --disable-cache \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 \
  --warmup 3 \
  --iterations 20 \
  --persist \
  --max-mean-ms 11 \
  --max-p95-ms 12 \
  --max-p99-ms 13 \
  --max-max-ms 15
