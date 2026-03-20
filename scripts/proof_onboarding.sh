#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/proof-vscode-mcp.json"
rm -f "${output}"

./scripts/onboard_local.sh --client vscode --output "${output}"

test -f .env
test -f "${output}"
grep -q '"servers"' "${output}"
grep -q 'run_mcp_stdio.sh' "${output}"

cargo run --quiet -- status >/dev/null

echo "proof_onboarding: ok"
