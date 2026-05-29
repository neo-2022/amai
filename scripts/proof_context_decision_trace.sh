#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_stage2_setup.sh >/tmp/amai-proof-context-decision-trace-setup.log

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-context-decision-trace.json

jq -e '.decision_trace.scope.effective_retrieval_mode == "local_plus_related"' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.selection_priority | length == 7' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.intent_classifier.classification == "factual_recall"' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.evidence_ladder.cheapest_sufficient_layer != null' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.evidence_sufficiency_check.cheapest_sufficient_layer == .decision_trace.evidence_ladder.cheapest_sufficient_layer' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.scope_resolver.status == "resolved"' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.escalate_if_needed.required != null' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.abstain_if_insufficient.abstained != null' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.final_decision != null' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.included | length > 0' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.semantic_guard != null' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.candidate_generation.grouped_strategies.exact >= 0' /tmp/amai-proof-context-decision-trace.json >/dev/null
jq -e '.decision_trace.evidence_ladder.layers[] | select(.layer == "raw_evidence") | .strategies == ["raw_evidence"]' /tmp/amai-proof-context-decision-trace.json >/dev/null

printf 'proof_context_decision_trace: PASS\n'
