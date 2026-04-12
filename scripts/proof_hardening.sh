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

cargo run --quiet -- access-policy ensure \
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

cargo run --quiet -- access-policy ensure \
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
assert data["project"]["code"] == "project_alpha", data["project"]
assert len(data["retrieval"]["exact_documents"]) == 0, data["retrieval"]["exact_documents"]
assert len(data["retrieval"]["lexical_chunks"]) == 0, data["retrieval"]["lexical_chunks"]
assert len(data["retrieval"]["semantic_chunks"]) == 0, data["retrieval"]["semantic_chunks"]
assert len(data["retrieval"]["symbol_hits"]) == 0, data["retrieval"]["symbol_hits"]
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
visible={data["project"]["code"]}
paths=set()
for bucket in ("exact_documents", "lexical_chunks", "semantic_chunks", "symbol_hits"):
    for item in data["retrieval"][bucket]:
        visible.add(item.get("project_code", data["project"]["code"]))
        if "relative_path" in item:
            paths.add((item.get("project_code", data["project"]["code"]), item["relative_path"]))
assert "project_alpha" in visible, visible
assert "project_beta" in visible, visible
assert ("project_alpha", "src/lib.rs") in paths, paths
assert ("project_beta", "src/lib.rs") in paths, paths
PY

docker compose restart postgres qdrant minio nats
sleep 5
./scripts/bootstrap_stack.sh
cargo run --quiet -- compat check
./scripts/status.sh
./scripts/proof_execctl_pending_return.sh
./scripts/proof_execctl_resolved_task_ids.sh
./scripts/proof_execctl_restore_stress.sh
