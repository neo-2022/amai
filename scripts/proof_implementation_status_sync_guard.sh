#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/implementation-status-sync-guard.json"
mkdir -p tmp/onboarding
rm -f "$output"

./scripts/maintainability_gate.sh --json >/dev/null
./scripts/implementation_status_sync_guard.sh --json >"$output"

test -f .amai/onboarding/project-maintainability-gate-state.json
test -f "$output"

jq -e '.artifact_version == "workspace-implementation-status-sync-guard-v1"' "$output" >/dev/null
jq -e '.gate_trace_artifact_path == ".amai/onboarding/project-maintainability-gate-state.json"' "$output" >/dev/null
jq -e '.implementation_status_path == "docs/IMPLEMENTATION_STATUS.md"' "$output" >/dev/null
jq -e '.required_gate_artifact_version == "workspace-maintainability-gate-v1"' "$output" >/dev/null
jq -e '.status_sync_allowed == true' "$output" >/dev/null
jq -e '.implementation_status_matches_gate_trace == true' "$output" >/dev/null
jq -e '.worktree_fingerprint_matches == true' "$output" >/dev/null
jq -e '.git_head_matches == true' "$output" >/dev/null

echo "proof_implementation_status_sync_guard: ok"
