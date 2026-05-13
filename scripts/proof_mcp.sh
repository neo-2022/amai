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

bootstrap_file="$(mktemp)"
handoff_file="$(mktemp)"
trap 'rm -f "$bootstrap_file" "$handoff_file"' EXIT

printf '# Synthetic continuity bootstrap\nMCP proof continuity contour for project_alpha.\n' >"$bootstrap_file"
cargo run --release --quiet -- continuity import \
  --project project_alpha \
  --display-name "Project Alpha" \
  --repo-root "$PWD/fixtures/project_alpha" \
  --namespace continuity \
  --bootstrap-file "$bootstrap_file" \
  --transcript-limit 0

printf '%s\n' '- MCP proof continuity seed.' >"$handoff_file"
cargo run --release --quiet -- continuity handoff \
  --project project_alpha \
  --namespace continuity \
  --headline "MCP continuity startup proof seed" \
  --next-step "Use amai_continuity_startup before substantive MCP work." \
  --details-file "$handoff_file"

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

cargo run --release --quiet -- verify mcp \
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
