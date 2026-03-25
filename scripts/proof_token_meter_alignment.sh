#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-meter-alignment.json

python3 - <<'PY'
import json
from pathlib import Path

root = json.loads(Path("/tmp/amai-proof-token-meter-alignment.json").read_text())["token_budget_report"]
contract = root["contract"]
agent_cycle = root["agent_cycle_economics"]

assert contract["client_limit_meter_alignment_version"] == "client-limit-meter-alignment-v2", contract
assert agent_cycle["model_version"] == "agent-cycle-lower-bound-v2", agent_cycle
assert agent_cycle["contract"]["client_limit_meter_alignment"]["model_version"] == "client-limit-meter-alignment-v2", agent_cycle
assert agent_cycle["contract"]["client_limit_meter_alignment"]["same_meter_as_client_limit"] is False, agent_cycle
assert "client_prompt_unmeasured" in agent_cycle["contract"]["client_limit_meter_alignment"]["blocking_reasons"], agent_cycle
assert "observable_components" in agent_cycle["contract"]["client_limit_meter_alignment"], agent_cycle

for scope in ("current_session", "rolling_window", "lifetime"):
    preview = root["statement_previews"].get(scope)
    if preview is None:
        continue
    alignment = preview["client_limit_meter_alignment"]
    assert alignment["model_version"] == "client-limit-meter-alignment-v2", preview
    assert alignment["surface_kind"] == "statement_preview", preview
    assert alignment["same_meter_as_client_limit"] is False, preview
    assert "assistant_generation" in alignment["missing_components"], preview
    assert "component_event_coverage" in alignment, preview
    assert alignment["events_total"] >= alignment["live_events_count"], preview
    assert alignment["events_total"] >= alignment["non_live_events_count"], preview

for scope in ("current_session", "rolling_window", "lifetime"):
    scope_payload = agent_cycle.get(scope)
    if scope_payload is None:
        continue
    alignment = scope_payload["client_limit_meter_alignment"]
    assert alignment["model_version"] == "client-limit-meter-alignment-v2", scope_payload
    assert alignment["surface_kind"] == "agent_cycle_scope", scope_payload
    assert alignment["same_meter_as_client_limit"] is False, scope_payload
    assert "component_event_coverage" in alignment, scope_payload
    assert "partially_measured_components" in alignment, scope_payload
PY

printf 'proof_token_meter_alignment: PASS\n'
