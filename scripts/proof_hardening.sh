#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source "./scripts/stage2_fixture_roots.sh"
stage2_prepare_fixture_roots "$PWD"

./scripts/bootstrap_stack.sh
./scripts/bootstrap_stack.sh
cargo run --quiet -- compat check

cargo run --quiet -- project register \
  --code project_alpha \
  --display-name "Project Alpha" \
  --repo-root "${AMAI_STAGE2_PROJECT_ALPHA_ROOT}"

cargo run --quiet -- project register \
  --code project_beta \
  --display-name "Project Beta" \
  --repo-root "${AMAI_STAGE2_PROJECT_BETA_ROOT}"

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
  --path "${AMAI_STAGE2_PROJECT_ALPHA_ROOT}" \
  --namespace review \
  --skip-embeddings

cargo run --quiet -- index project \
  --code project_beta \
  --path "${AMAI_STAGE2_PROJECT_BETA_ROOT}" \
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

wait_for_stack_ready_after_restart() {
  local attempt consecutive_ok
  consecutive_ok=0
  for attempt in $(seq 1 60); do
    if ./scripts/bootstrap_stack.sh >/tmp/amai-proof-hardening-bootstrap-after-restart.out 2>&1 \
      && cargo run --quiet -- compat check >/tmp/amai-proof-hardening-compat-after-restart.out 2>&1 \
      && ./scripts/status.sh >/tmp/amai-proof-hardening-status-after-restart.out 2>&1; then
      consecutive_ok=$((consecutive_ok + 1))
      if (( consecutive_ok >= 2 )); then
        cat /tmp/amai-proof-hardening-status-after-restart.out
        return 0
      fi
    else
      consecutive_ok=0
    fi
    sleep 2
  done

  echo "proof_hardening: stack did not become stable after restart" >&2
  for log in \
    /tmp/amai-proof-hardening-bootstrap-after-restart.out \
    /tmp/amai-proof-hardening-compat-after-restart.out \
    /tmp/amai-proof-hardening-status-after-restart.out; do
    if [[ -s "${log}" ]]; then
      echo "--- ${log} ---" >&2
      tail -n 80 "${log}" >&2
    fi
  done
  return 1
}

docker compose restart postgres qdrant minio nats
wait_for_stack_ready_after_restart
./scripts/proof_execctl_pending_return.sh
./scripts/proof_execctl_resolved_task_ids.sh
./scripts/proof_execctl_restore_stress.sh
