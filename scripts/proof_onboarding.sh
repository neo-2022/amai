#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/proof-vscode-mcp.json"
human_output="tmp/onboarding/proof-vscode.out"
rm -f "${output}" "${human_output}"

./scripts/onboard_local.sh --client vscode --yes --output "${output}" >"${human_output}"

test -f .env
test -f "${output}"
test -f "${human_output}"
grep -q '"servers"' "${output}"
grep -q 'run_mcp_stdio.sh' "${output}"
grep -q 'Почему последний собранный контекст что-то включил:' "${human_output}"

cargo run --quiet -- status >/dev/null

echo "proof_onboarding: ok"
