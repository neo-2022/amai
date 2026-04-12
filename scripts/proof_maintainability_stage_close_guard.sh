#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/maintainability-stage-close-guard.json"
mkdir -p tmp/onboarding
rm -f "$output"

./scripts/maintainability_gate.sh --json >/dev/null
./scripts/maintainability_stage_close_guard.sh --json >"$output"

test -f .amai/onboarding/project-maintainability-gate-state.json
test -f "$output"

jq -e '.artifact_version == "workspace-maintainability-close-guard-v1"' "$output" >/dev/null
jq -e '.gate_trace_artifact_path == ".amai/onboarding/project-maintainability-gate-state.json"' "$output" >/dev/null
jq -e '.required_gate_artifact_version == "workspace-maintainability-gate-v1"' "$output" >/dev/null
jq -e '.checkbox_closure_allowed == true' "$output" >/dev/null
jq -e '.worktree_fingerprint_matches == true' "$output" >/dev/null
jq -e '.git_head_matches == true' "$output" >/dev/null

echo "proof_maintainability_stage_close_guard: ok"
