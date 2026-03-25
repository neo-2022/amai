#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/proof-vscode-mcp.json"
human_output="tmp/onboarding/proof-vscode.out"
startup_output=".github/instructions/amai-continuity-startup.instructions.md"
rm -f "${output}" "${human_output}" "${startup_output}"

./scripts/onboard_local.sh --client vscode --yes --output "${output}" >"${human_output}"

test -f .env
test -f "${output}"
test -f "${human_output}"
test -f "${startup_output}"
grep -q '"servers"' "${output}"
grep -q 'run_mcp_stdio.sh' "${output}"
grep -q 'amai_continuity_startup' "${startup_output}"
grep -q 'execctl_resume_contract_summary' "${startup_output}"
grep -q 'execctl_resume_obligation' "${startup_output}"
grep -q 'startup_next_action' "${startup_output}"
grep -q 'execctl_active_lease' "${startup_output}"
grep -q 'lease_owner_state' "${startup_output}"
grep -q 'execctl_active_lease_summary' "${startup_output}"
grep -q 'previous_session_owner' "${startup_output}"
grep -q 'resume_required_return_task' "${startup_output}"
grep -q 'required_return_task' "${startup_output}"
grep -q 'project_task_tree' "${startup_output}"
grep -q 'project_task_ledger' "${startup_output}"
grep -q 'Auto-start readiness: instruction-backed' "${human_output}"
grep -q 'Почему такой режим:' "${human_output}"
grep -q 'Что машина реально показала после установки:' "${human_output}"

cargo run --quiet -- status >/dev/null

echo "proof_onboarding: ok"
