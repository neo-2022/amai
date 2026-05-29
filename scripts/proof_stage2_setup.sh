#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

stack_healthy() {
  local status_output
  if ! status_output="$(./scripts/status.sh 2>/dev/null)"; then
    return 1
  fi
  grep -q '^postgres: ok ' <<<"${status_output}" \
    && grep -q '^qdrant: ok ' <<<"${status_output}" \
    && grep -q '^s3: ok ' <<<"${status_output}" \
    && grep -q '^nats: ok ' <<<"${status_output}"
}

if ! stack_healthy; then
  AMAI_SKIP_STACK_PREFLIGHT=1 ./scripts/bootstrap_stack.sh >/tmp/amai-proof-stage2-bootstrap.log 2>&1
fi

cargo run --release --quiet -- compat check

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

cargo run --release --quiet -- access-policy ensure \
  --workspace default \
  --project project_alpha \
  --code proof_alpha_related_read \
  --display-name "Proof Alpha Related Read" \
  --object-class fact \
  --scope-type cross_project_linked \
  --precedence 250 \
  --can-read \
  --can-link \
  --can-import

cargo run --release --quiet -- access-policy ensure \
  --workspace default \
  --project project_beta \
  --code proof_beta_related_read \
  --display-name "Proof Beta Related Read" \
  --object-class fact \
  --scope-type cross_project_linked \
  --precedence 250 \
  --can-read \
  --can-link \
  --can-import

cargo run --release --quiet -- index project \
  --code project_alpha \
  --path "$PWD/fixtures/project_alpha" \
  --namespace review \
  --skip-embeddings

cargo run --release --quiet -- index project \
  --code project_beta \
  --path "$PWD/fixtures/project_beta" \
  --namespace review \
  --skip-embeddings
