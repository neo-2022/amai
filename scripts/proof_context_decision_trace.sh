#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_hardening.sh >/tmp/amai-proof-context-decision-trace-setup.log

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-context-decision-trace.json

jq -e '.decision_trace.scope.effective_retrieval_mode == "local_plus_related"' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.selection_priority | length == 4' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.included | length > 0' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.semantic_guard != null' /tmp/amai-proof-context-decision-trace.json >/dev/null

printf 'proof_context_decision_trace: PASS\n'
