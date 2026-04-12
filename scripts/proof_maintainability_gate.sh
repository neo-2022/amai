#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/maintainability-gate.json"
mkdir -p tmp/onboarding
rm -f "$output"

./scripts/maintainability_gate.sh --json >"$output"

test -f docs/MAINTAINABILITY_ENFORCEMENT.md
test -f "$output"
test -f .amai/onboarding/project-maintainability-gate-state.json

jq -e '.artifact_version == "workspace-maintainability-gate-v1"' "$output" >/dev/null
jq -e '.standard_source_path == "docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md"' "$output" >/dev/null
jq -e '.project_binding_path == "docs/MAINTAINABILITY_ENFORCEMENT.md"' "$output" >/dev/null
jq -e '.standard_storage_mode == "vendored_repo_copy"' "$output" >/dev/null
jq -e '.artifact_path == ".amai/onboarding/project-maintainability-gate-state.json"' "$output" >/dev/null
jq -e '.checkbox_closure_requires_fresh_trace == true' "$output" >/dev/null
jq -e '.closure_guard_command == "./scripts/maintainability_stage_close_guard.sh --json"' "$output" >/dev/null
jq -e '.implementation_status_path == "docs/IMPLEMENTATION_STATUS.md"' "$output" >/dev/null
jq -e '.implementation_status_sha256 | length > 0' "$output" >/dev/null
jq -e '.implementation_status_sync_required_for_significant_status_update == true' "$output" >/dev/null
jq -e '.implementation_status_sync_guard_command == "./scripts/implementation_status_sync_guard.sh --json"' "$output" >/dev/null
jq -e '.applies_when | length > 0' "$output" >/dev/null
jq -e '.fail_closed_questions | length > 0' "$output" >/dev/null
jq -e '.anti_hardcoding_rules | length > 0' "$output" >/dev/null
jq -e '.required_documents | index("docs/IMPLEMENTATION_GATES.md") != null' "$output" >/dev/null
jq -e '.mandatory_outputs | index("continuity handoff") != null' "$output" >/dev/null

grep -q 'Fail-closed' docs/MAINTAINABILITY_ENFORCEMENT.md || grep -q 'fail-closed' docs/MAINTAINABILITY_ENFORCEMENT.md
grep -q 'Anti-hardcoding' docs/MAINTAINABILITY_ENFORCEMENT.md
grep -q 'rollback' docs/MAINTAINABILITY_ENFORCEMENT.md
grep -q 'recovery' docs/MAINTAINABILITY_ENFORCEMENT.md

echo "proof_maintainability_gate: ok"
