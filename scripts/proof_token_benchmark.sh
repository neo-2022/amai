#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

./scripts/bootstrap_stack.sh

cargo run --release --quiet -- verify token-benchmark \
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
  --min-savings-percent 15
