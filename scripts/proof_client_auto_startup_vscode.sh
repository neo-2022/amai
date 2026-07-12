#!/usr/bin/env bash
set -euo pipefail

# Tier-1 auto-startup proof for VS Code.
# Uses a clean temporary project so the proof is not blocked by the state of the
# Amai project itself. Onboards VS Code, verifies the managed workspace
# instruction file, starts an MCP session, calls amai_continuity_startup on the
# temporary project, and checks that the runtime state artifact is produced.

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:/usr/bin:/bin"

cargo build --release --quiet

snapshot_state() {
  local path="$1"
  if [[ -e "${path}" ]]; then
    cp -r "${path}" "${temp_home}/$(basename "${path}").snapshot" 2>/dev/null || true
  fi
}

restore_state() {
  local path="$1"
  local snap="${temp_home}/$(basename "${path}").snapshot"
  if [[ -e "${snap}" ]]; then
    rm -rf "${path}"
    cp -r "${snap}" "${path}" 2>/dev/null || true
  else
    rm -rf "${path}"
  fi
}

host_home="${HOME}"
RUSTUP_HOME="${RUSTUP_HOME:-${host_home}/.rustup}"
CARGO_HOME="${CARGO_HOME:-${host_home}/.cargo}"
export RUSTUP_HOME CARGO_HOME

temp_home="$(mktemp -d)"
temp_repo="$(mktemp -d)"
client="vscode"
project_code="proof_auto_start_${client}_$(date +%s%N)"
export HOME="${temp_home}"
export AMAI_INSTALL_STATE_PATH="${temp_home}/install_state.json"

cleanup() {
  restore_state ".vscode/mcp.json"
  restore_state ".github/instructions/amai-continuity-startup.instructions.md"
  rm -rf "${temp_home}" "${temp_repo}"
}
trap cleanup EXIT

snapshot_state ".vscode/mcp.json"
snapshot_state ".github/instructions/amai-continuity-startup.instructions.md"

# Register a clean temporary project for the auto-start check.
cargo run --release --quiet -- project register \
  --code "${project_code}" \
  --display-name "Proof VS Code auto-start" \
  --repo-root "${temp_repo}" \
  --workspace default >/dev/null
cargo run --release --quiet -- namespace ensure \
  --project "${project_code}" \
  --code continuity \
  --display-name Continuity >/dev/null
cargo run --release --quiet -- continuity handoff \
  --project "${project_code}" \
  --namespace continuity \
  --headline "Proof active line" \
  --next-step "Run VS Code auto-start proof" \
  --promote-active-workline >/dev/null

# Seed runtime state once via CLI so the first MCP call starts from a consistent
# runtime artifact and not from a missing or stale one.
./scripts/continuity_startup.sh \
  --repo-root "${temp_repo}" \
  --namespace continuity \
  --json >/dev/null

./scripts/onboard_local.sh --client vscode --yes --skip-stack --skip-release-build >/dev/null

test -f .github/instructions/amai-continuity-startup.instructions.md
grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v2' .github/instructions/amai-continuity-startup.instructions.md
grep -q 'amai_continuity_startup' .github/instructions/amai-continuity-startup.instructions.md
grep -q 'workspace-startup-runtime-state-v4' .github/instructions/amai-continuity-startup.instructions.md

test -f .vscode/mcp.json
grep -q '"amai"' .vscode/mcp.json
grep -q 'run_mcp_stdio.sh' .vscode/mcp.json

rm -f "${temp_repo}/.amai/continuity/project-chat-startup-state.json"

startup_bundle="$(mktemp)"
./scripts/continuity_startup.sh \
  --repo-root "${temp_repo}" \
  --namespace continuity \
  --json >"${startup_bundle}"

jq -e '.continuity_startup.project.code == "'"${project_code}"'"' "${startup_bundle}" >/dev/null
jq -e '.continuity_startup.namespace.code == "continuity"' "${startup_bundle}" >/dev/null
jq -e '.gate_semantics_consistent == true' "${temp_repo}/.amai/continuity/project-chat-startup-state.json" >/dev/null
rm -f "${startup_bundle}"

echo "proof_client_auto_startup_vscode: ok"
