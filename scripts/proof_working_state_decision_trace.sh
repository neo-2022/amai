#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_hardening.sh >/tmp/amai-proof-working-state-decision-trace-setup.log

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

jq -e '.latest_working_state_restore.working_state_restore.latest_decision_trace.scope.project_code == "project_alpha"' /tmp/amai-proof-working-state-snapshot.json >/dev/null
jq -e '.latest_working_state_restore.working_state_restore.latest_decision_trace.included | length > 0' /tmp/amai-proof-working-state-snapshot.json >/dev/null
jq -e '.latest_working_state_restore.working_state_restore.included_reasons_summary != null' /tmp/amai-proof-working-state-snapshot.json >/dev/null
jq -e '.latest_working_state_restore.working_state_restore | has("excluded_reasons_summary")' /tmp/amai-proof-working-state-snapshot.json >/dev/null
jq -e '.latest_working_state_restore.working_state_restore.recent_decision_traces | length > 0' /tmp/amai-proof-working-state-snapshot.json >/dev/null

printf 'proof_working_state_decision_trace: PASS\n'
