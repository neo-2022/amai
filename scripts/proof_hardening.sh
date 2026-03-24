#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bootstrap_stack.sh
./scripts/bootstrap_stack.sh
cargo run --quiet -- compat check

cargo run --quiet -- project register \
  --code project_alpha \
  --display-name "Project Alpha" \
  --repo-root "$PWD/fixtures/project_alpha"

cargo run --quiet -- project register \
  --code project_beta \
  --display-name "Project Beta" \
  --repo-root "$PWD/fixtures/project_beta"

cargo run --quiet -- namespace ensure \
  --project project_alpha \
  --code review \
  --display-name Review \
  --retrieval-mode local_plus_related

cargo run --quiet -- namespace ensure \
  --project project_beta \
  --code review \
  --display-name Review \
  --retrieval-mode local_plus_related

cargo run --quiet -- relation add \
  --source project_alpha \
  --target project_beta \
  --relation-type shared_runtime \
  --shared-contour common_contour \
  --access-mode local_plus_related

cargo run --quiet -- index project \
  --code project_alpha \
  --path "$PWD/fixtures/project_alpha" \
  --namespace review \
  --skip-embeddings

cargo run --quiet -- index project \
  --code project_beta \
  --path "$PWD/fixtures/project_beta" \
  --namespace review \
  --skip-embeddings

cargo run --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "beta_only_token" \
  --retrieval-mode local_strict \
  --token-source-kind proof_context_pack > /tmp/amai-proof-local-strict.json

python3 - <<'PY'
import json
with open('/tmp/amai-proof-local-strict.json','r',encoding='utf-8') as f:
    data=json.load(f)
assert len(data["visible_projects"]) == 1, data["visible_projects"]
assert len(data["retrieval"]["exact_documents"]) == 0, data["retrieval"]["exact_documents"]
assert len(data["retrieval"]["lexical_chunks"]) == 0, data["retrieval"]["lexical_chunks"]
PY

cargo run --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack > /tmp/amai-proof-related.json

python3 - <<'PY'
import json
with open('/tmp/amai-proof-related.json','r',encoding='utf-8') as f:
    data=json.load(f)
visible={item["project_code"] for item in data["visible_projects"]}
assert "project_alpha" in visible, visible
assert "project_beta" in visible, visible
paths={item["relative_path"] for item in data["retrieval"]["exact_documents"]}
assert "src/lib.rs" in paths, paths
assert len(data["retrieval"]["exact_documents"]) >= 2, data["retrieval"]["exact_documents"]
PY

docker compose restart postgres qdrant minio nats
sleep 5
./scripts/bootstrap_stack.sh
cargo run --quiet -- compat check
./scripts/status.sh
