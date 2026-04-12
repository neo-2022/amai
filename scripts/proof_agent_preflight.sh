#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/agent-preflight.json"
contract=".amai/onboarding/project-agent-preflight-contract.json"
agent_contract=".amai/onboarding/project-agent-preflight-agent-contract.json"
state_artifact=".amai/onboarding/project-agent-preflight-state.json"

mkdir -p tmp/onboarding
rm -f "$output"

./scripts/agent_preflight.sh --json >"$output"

test -f "$output"
test -f "$contract"
test -f "$agent_contract"
test -f "$state_artifact"

jq -e '.artifact_version == "workspace-agent-preflight-state-v1"' "$output" >/dev/null
jq -e '.agent_preflight_summary.status_snapshot_path == "docs/IMPLEMENTATION_STATUS.md"' "$output" >/dev/null
jq -e '.agent_preflight_summary.stage_checklist | length > 0' "$output" >/dev/null
jq -e '.agent_preflight_summary.next_required_stage.label | type == "string" and length > 0' "$output" >/dev/null
jq -e '.agent_preflight_summary.next_stage_ready_mechanisms | length > 0' "$output" >/dev/null
jq -e '.source_documents | map(select(.path == "AGENTS.md" and .exists == true)) | length == 1' "$output" >/dev/null

jq -e '.artifact_version == "workspace-agent-preflight-contract-v1"' "$contract" >/dev/null
jq -e '.preflight_contract.contract_version == "agent-preflight-contract-v1"' "$contract" >/dev/null
jq -e '.preflight_contract.refresh_commands.shell_command == "./scripts/agent_preflight.sh"' "$contract" >/dev/null
jq -e '.preflight_contract.status_sources.gates_path == "docs/IMPLEMENTATION_GATES.md"' "$contract" >/dev/null

jq -e '.artifact_version == "workspace-agent-preflight-agent-contract-v1"' "$agent_contract" >/dev/null
jq -e '.runtime_state_artifact.workspace_runtime_state_relative_path == ".amai/onboarding/project-agent-preflight-state.json"' "$agent_contract" >/dev/null

echo "proof_agent_preflight: ok"
