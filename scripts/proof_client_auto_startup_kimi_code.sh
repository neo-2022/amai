#!/usr/bin/env bash
set -euo pipefail

# Tier-1 auto-startup proof for Kimi Code.
# Follows the OpenCode proof pattern: a clean temporary project is registered in
# Amai, runtime state is seeded via the canonical continuity startup, then
# onboarding materializes the Kimi Code client artifacts into the current repo.
# The proof verifies the MCP config, the managed startup block in AGENTS.md, and
# the SessionStart hook (~/.kimi-code/config.toml [[hooks]] +
# scripts/kimi_code_session_start_hook.sh) that runs the canonical startup with
# the exact Kimi session ID before the first model turn. KIMI_CODE_HOME is
# redirected to a temporary directory so the proof never touches the developer's
# real Kimi Code configuration.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

kimi_home_tmp="$(mktemp -d /tmp/amai-proof-kimi-home-XXXXXX)"
temp_repo="$(mktemp -d /tmp/amai-proof-kimi-repo-XXXXXX)"
client="kimi-code"
project_code="proof_auto_start_kimi_$(date +%s%N)"
export KIMI_CODE_HOME="${kimi_home_tmp}"

config_path="${repo_root}/.kimi-code/mcp.json"
agents_md="${repo_root}/AGENTS.md"
agents_backup=""

cleanup() {
  rm -rf "${kimi_home_tmp}" "${temp_repo}"
  rm -f "${config_path}"
  rmdir "${repo_root}/.kimi-code" 2>/dev/null || true
  if [ -n "${agents_backup}" ] && [ -f "${agents_backup}" ]; then
    mv "${agents_backup}" "${agents_md}"
  fi
}
trap cleanup EXIT

if [ -f "${agents_md}" ]; then
  agents_backup="$(mktemp /tmp/amai-proof-kimi-agents-XXXXXX)"
  cp "${agents_md}" "${agents_backup}"
fi

# Register a clean temporary project for the auto-start check.
cargo run --release --quiet -- project register \
  --code "${project_code}" \
  --display-name "Proof Kimi Code auto-start" \
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
  --next-step "Run Kimi Code auto-start proof" \
  --promote-active-workline >/dev/null

# Seed runtime state once via CLI so the first MCP call starts from a consistent
# runtime artifact and not from a missing or stale one.
./scripts/continuity_startup.sh \
  --repo-root "${temp_repo}" \
  --namespace continuity \
  --json >/dev/null

./scripts/onboard_local.sh --client "${client}" --yes --skip-stack --skip-release-build >/dev/null

# 1. MCP config for Kimi Code is materialized and carries the amai server.
test -f "${config_path}"
grep -q '"amai"' "${config_path}"
grep -q 'run_mcp_stdio.sh' "${config_path}"
grep -q '"mcpServers"' "${config_path}"

# 2. Managed startup block is embedded in the project AGENTS.md.
test -f "${agents_md}"
grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v2' "${agents_md}"
grep -q 'continuity_startup.sh' "${agents_md}"

# 3. Kimi Code SessionStart hook is materialized: managed [[hooks]] block in the
#    (redirected) Kimi home config + executable hook script that binds the exact
#    Kimi session ID to the canonical continuity startup.
test -f "${kimi_home_tmp}/config.toml"
grep -q 'AMAI MANAGED KIMI SESSION HOOK v1' "${kimi_home_tmp}/config.toml"
grep -q '\[\[hooks\]\]' "${kimi_home_tmp}/config.toml"
grep -q 'event = "SessionStart"' "${kimi_home_tmp}/config.toml"
grep -q 'kimi_code_session_start_hook.sh' "${kimi_home_tmp}/config.toml"
test -x scripts/kimi_code_session_start_hook.sh
grep -q 'AMAI_PLATFORM_THREAD_ID' scripts/kimi_code_session_start_hook.sh
grep -q 'continuity_startup.sh' scripts/kimi_code_session_start_hook.sh

# 4. Hook really executes the canonical startup when fed a Kimi SessionStart
#    payload (observation-only: always exits 0).
rm -f "${temp_repo}/.amai/continuity/project-chat-startup-state.json"
printf '{"session_id":"kimi-proof-session-001","source":"startup","model":"kimi-code/k3"}' \
  | ./scripts/kimi_code_session_start_hook.sh

startup_bundle="$(mktemp)"
./scripts/continuity_startup.sh \
  --repo-root "${temp_repo}" \
  --namespace continuity \
  --json >"${startup_bundle}"

jq -e '.continuity_startup.project.code == "'"${project_code}"'"' "${startup_bundle}" >/dev/null
jq -e '.continuity_startup.namespace.code == "continuity"' "${startup_bundle}" >/dev/null
jq -e '.gate_semantics_consistent == true' "${temp_repo}/.amai/continuity/project-chat-startup-state.json" >/dev/null
rm -f "${startup_bundle}"

echo "proof_client_auto_startup_kimi_code: ok"
