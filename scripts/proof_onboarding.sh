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
grep -q 'execctl_active_lease_summary' "${startup_output}"
grep -q 'resume_required_return_task' "${startup_output}"
grep -q 'required_return_task' "${startup_output}"
grep -q 'Auto-start readiness: instruction-backed' "${human_output}"
if ! grep -q 'Почему последний собранный контекст что-то включил:' "${human_output}"; then
  grep -q 'Почему часть слоёв ничего не добавила:' "${human_output}"
fi

cargo run --quiet -- status >/dev/null

echo "proof_onboarding: ok"
