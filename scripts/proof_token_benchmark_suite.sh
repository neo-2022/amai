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

cargo run --release --quiet -- verify token-benchmark-suite \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --queries-file "$PWD/fixtures/token_benchmark_queries.txt" \
  --tokenizer o200k_base \
  --naive-limit-files 20 \
  --naive-max-bytes-per-file 32768 \
  --min-mean-savings-factor 1.2 \
  --min-mean-savings-percent 15
