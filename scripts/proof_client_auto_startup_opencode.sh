#!/usr/bin/env bash
set -euo pipefail

# Tier-1 auto-startup proof for OpenCode.
# Uses a clean temporary project so the proof is not blocked by the state of the
# Amai project itself. Onboards OpenCode, verifies the managed MCP config, the
# AGENTS.md startup block and the .opencode/plugins/amai-continuity.js session
# hook, then runs the canonical continuity startup and checks that the runtime
# state artifact is produced with consistent gate semantics.

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
client="opencode"
project_code="proof_auto_start_${client}_$(date +%s%N)"
export HOME="${temp_home}"
export AMAI_INSTALL_STATE_PATH="${temp_home}/install_state.json"

cleanup() {
  restore_state "opencode.json"
  restore_state "AGENTS.md"
  restore_state ".opencode/plugins/amai-continuity.js"
  rm -rf "${temp_home}" "${temp_repo}"
}
trap cleanup EXIT

snapshot_state "opencode.json"
snapshot_state "AGENTS.md"
snapshot_state ".opencode/plugins/amai-continuity.js"

# Register a clean temporary project for the auto-start check.
cargo run --release --quiet -- project register \
  --code "${project_code}" \
  --display-name "Proof OpenCode auto-start" \
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
  --next-step "Run OpenCode auto-start proof" \
  --promote-active-workline >/dev/null

# Seed runtime state once via CLI so the first MCP call starts from a consistent
# runtime artifact and not from a missing or stale one.
./scripts/continuity_startup.sh \
  --repo-root "${temp_repo}" \
  --namespace continuity \
  --json >/dev/null

./scripts/onboard_local.sh --client opencode --yes --skip-stack --skip-release-build >/dev/null

# 1. MCP config for OpenCode is materialized and carries the amai server.
test -f opencode.json
grep -q '"amai"' opencode.json
grep -q 'run_mcp_stdio.sh' opencode.json
grep -q '"mcp"' opencode.json

# 2. Managed startup block is embedded in the project AGENTS.md.
test -f AGENTS.md
grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v2' AGENTS.md
grep -q 'amai_continuity_startup' AGENTS.md

# 3. OpenCode session hook plugin is materialized and binds session.created to
#    the canonical continuity startup with the exact OpenCode session ID.
test -f .opencode/plugins/amai-continuity.js
grep -q 'session.created' .opencode/plugins/amai-continuity.js
grep -q 'AMAI_PLATFORM_THREAD_ID: sessionID' .opencode/plugins/amai-continuity.js
grep -q 'continuity_startup.sh' .opencode/plugins/amai-continuity.js

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

echo "proof_client_auto_startup_opencode: ok"
