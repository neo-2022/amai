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

assert contract["client_limit_meter_alignment_version"] == "client-limit-meter-alignment-v6", contract
assert agent_cycle["model_version"] == "agent-cycle-lower-bound-v4", agent_cycle
assert agent_cycle["contract"]["client_limit_meter_alignment"]["model_version"] == "client-limit-meter-alignment-v6", agent_cycle
assert agent_cycle["contract"]["client_limit_meter_alignment"]["same_meter_as_client_limit"] is False, agent_cycle
assert "client_prompt_unmeasured" in agent_cycle["contract"]["client_limit_meter_alignment"]["blocking_reasons"], agent_cycle
assert "observable_components" in agent_cycle["contract"]["client_limit_meter_alignment"], agent_cycle

for scope in ("current_session", "rolling_window", "lifetime"):
    preview = root["statement_previews"].get(scope)
    if preview is None:
        continue
    alignment = preview["client_limit_meter_alignment"]
    assert alignment["model_version"] == "client-limit-meter-alignment-v6", preview
    assert "assistant_generation_observation_source" in alignment, alignment
    assert "not_applicable_components" in alignment, alignment
    assert alignment["surface_kind"] == "statement_preview", preview
    assert alignment["same_meter_as_client_limit"] is False, preview
    assert "component_event_coverage" in alignment, preview
    assert alignment["events_total"] >= alignment["live_events_count"], preview
    assert alignment["events_total"] >= alignment["non_live_events_count"], preview
    assistant = next(item for item in alignment["component_event_coverage"] if item["code"] == "assistant_generation")
    assert assistant["target_scope_kind"] == "assistant_generation_turn_scope", alignment
    if assistant["target_scope_applicable"]:
        assert (
            "assistant_generation" in alignment["missing_components"]
            or assistant["observed_live_events"] == assistant["target_live_events_count"]
        ), preview
    else:
        assert "assistant_generation" in alignment["not_applicable_components"], preview

for scope in ("current_session", "rolling_window", "lifetime"):
    scope_payload = agent_cycle.get(scope)
    if scope_payload is None:
        continue
    alignment = scope_payload["client_limit_meter_alignment"]
    assert alignment["model_version"] == "client-limit-meter-alignment-v6", scope_payload
    assert "assistant_generation_observation_source" in alignment, alignment
    assert "not_applicable_components" in alignment, alignment
    assert alignment["surface_kind"] == "agent_cycle_scope", scope_payload
    assert alignment["same_meter_as_client_limit"] is False, scope_payload
    assert "component_event_coverage" in alignment, scope_payload
    assert "partially_measured_components" in alignment, scope_payload
    client_prompt = next(item for item in alignment["component_event_coverage"] if item["code"] == "client_prompt")
    assistant = next(item for item in alignment["component_event_coverage"] if item["code"] == "assistant_generation")
    assert assistant["target_scope_kind"] == "assistant_generation_turn_scope", alignment
    if alignment["live_events_count"] > 0:
        assert client_prompt["observed_live_events"] > 0, scope_payload
PY

printf 'proof_token_meter_alignment: PASS\n'
