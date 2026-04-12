#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_stage2_setup.sh >/tmp/amai-proof-working-state-decision-trace-setup.log

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-working-state-context-pack.json

cargo run --release --quiet -- observe snapshot >/tmp/amai-proof-working-state-snapshot.json

python3 - <<'PY'
import json

with open('/tmp/amai-proof-working-state-snapshot.json', 'r', encoding='utf-8') as fh:
    payload = json.load(fh)

restore = payload["latest_working_state_restore"]["working_state_restore"]
recent = restore.get("recent_decision_traces") or []
match = None
for item in recent:
    scope = item.get("scope") or {}
    if (
        scope.get("project_code") == "project_alpha"
        and scope.get("namespace_code") == "review"
        and scope.get("effective_retrieval_mode") == "local_plus_related"
    ):
        match = item
        break

assert match is not None, recent
assert len(match.get("included") or []) > 0, match
assert restore.get("included_reasons_summary"), restore
assert "excluded_reasons_summary" in restore, restore
assert len(recent) > 0, restore
PY

printf 'proof_working_state_decision_trace: PASS\n'
