#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

temp_home="$(mktemp -d)"
trap 'rm -rf "${temp_home}"' EXIT

RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client codex --yes --skip-stack --skip-release-build
test -f "${temp_home}/.codex/config.toml"
grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"
test -f AGENTS.md
test -f .amai/onboarding/project-chat-startup-contract.json
grep -q '"artifact_version": "workspace-startup-contract-v1"' .amai/onboarding/project-chat-startup-contract.json
grep -q '"startup_contract_sha256": "' .amai/onboarding/project-chat-startup-contract.json
grep -q '"missing_or_unreadable_fail_closed": true' .amai/onboarding/project-chat-startup-contract.json
grep -q '"required_action_kind_when_resume_required": "resume_required_return_task"' .amai/onboarding/project-chat-startup-contract.json
grep -q '"project_task_tree"' .amai/onboarding/project-chat-startup-contract.json
grep -q '"project_task_ledger"' .amai/onboarding/project-chat-startup-contract.json
grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v1' AGENTS.md
grep -q 'project `AGENTS.md`' AGENTS.md
grep -q 'execctl_resume_contract_summary' AGENTS.md
grep -q 'execctl_resume_obligation' AGENTS.md
grep -q 'startup_next_action' AGENTS.md
grep -q 'execctl_active_lease' AGENTS.md
grep -q 'lease_owner_state' AGENTS.md
grep -q 'previous_session_owner' AGENTS.md
grep -q 'resume_required_return_task' AGENTS.md
grep -q 'required_return_task' AGENTS.md
grep -q 'startup_contract_sha256 = "' AGENTS.md
grep -q 'workspace_contract_required_before_tool_call = true' AGENTS.md
grep -q 'missing_or_unreadable_fail_closed = true' AGENTS.md
grep -q 'sha256_mismatch_fail_closed = true' AGENTS.md
grep -q 'project_task_tree' AGENTS.md
grep -q 'project_task_ledger' AGENTS.md

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/disconnect_local.sh --client codex
if [[ -f "${temp_home}/.codex/config.toml" ]] && grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"; then
  echo "proof_client_lifecycle: codex server entry still present after disconnect"
  exit 1
fi
if grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v1' AGENTS.md; then
  echo "proof_client_lifecycle: codex managed startup block still present after disconnect"
  exit 1
fi

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client cursor --yes --skip-stack --skip-release-build
test -f "${temp_home}/.cursor/mcp.json"
grep -q '"amai"' "${temp_home}/.cursor/mcp.json"
test -f .amai/onboarding/project-chat-startup-contract.json
grep -q '"artifact_version": "workspace-startup-contract-v1"' .amai/onboarding/project-chat-startup-contract.json
grep -q '"startup_contract_sha256": "' .amai/onboarding/project-chat-startup-contract.json
grep -q '"missing_or_unreadable_fail_closed": true' .amai/onboarding/project-chat-startup-contract.json
test -f .cursor/rules/amai-continuity-startup.mdc
grep -q 'amai_continuity_startup' .cursor/rules/amai-continuity-startup.mdc
grep -q 'execctl_resume_contract_summary' .cursor/rules/amai-continuity-startup.mdc
grep -q 'execctl_resume_obligation' .cursor/rules/amai-continuity-startup.mdc
grep -q 'startup_next_action' .cursor/rules/amai-continuity-startup.mdc
grep -q 'execctl_active_lease' .cursor/rules/amai-continuity-startup.mdc
grep -q 'lease_owner_state' .cursor/rules/amai-continuity-startup.mdc
grep -q 'previous_session_owner' .cursor/rules/amai-continuity-startup.mdc
grep -q 'resume_required_return_task' .cursor/rules/amai-continuity-startup.mdc
grep -q 'required_return_task' .cursor/rules/amai-continuity-startup.mdc
grep -q 'startup_contract_sha256 = "' .cursor/rules/amai-continuity-startup.mdc
grep -q 'workspace_contract_required_before_tool_call = true' .cursor/rules/amai-continuity-startup.mdc
grep -q 'missing_or_unreadable_fail_closed = true' .cursor/rules/amai-continuity-startup.mdc
grep -q 'sha256_mismatch_fail_closed = true' .cursor/rules/amai-continuity-startup.mdc
grep -q 'project_task_tree' .cursor/rules/amai-continuity-startup.mdc
grep -q 'project_task_ledger' .cursor/rules/amai-continuity-startup.mdc

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/disconnect_local.sh --client cursor
if [[ -f "${temp_home}/.cursor/mcp.json" ]] && grep -q '"amai"' "${temp_home}/.cursor/mcp.json"; then
  echo "proof_client_lifecycle: cursor server entry still present after disconnect"
  exit 1
fi
if [[ -f .cursor/rules/amai-continuity-startup.mdc ]]; then
  echo "proof_client_lifecycle: cursor startup instructions still present after disconnect"
  exit 1
fi

./scripts/onboard_local.sh --client claude-code --yes --skip-stack --skip-release-build
test -f .mcp.json
grep -q '"amai"' .mcp.json
test -f .amai/onboarding/project-chat-startup-contract.json
grep -q '"artifact_version": "workspace-startup-contract-v1"' .amai/onboarding/project-chat-startup-contract.json
grep -q '"startup_contract_sha256": "' .amai/onboarding/project-chat-startup-contract.json
grep -q '"missing_or_unreadable_fail_closed": true' .amai/onboarding/project-chat-startup-contract.json
test -f CLAUDE.md
grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v1' CLAUDE.md
grep -q 'amai_continuity_startup' CLAUDE.md
grep -q 'execctl_resume_contract_summary' CLAUDE.md
grep -q 'execctl_resume_obligation' CLAUDE.md
grep -q 'startup_next_action' CLAUDE.md
grep -q 'execctl_active_lease' CLAUDE.md
grep -q 'lease_owner_state' CLAUDE.md
grep -q 'previous_session_owner' CLAUDE.md
grep -q 'resume_required_return_task' CLAUDE.md
grep -q 'required_return_task' CLAUDE.md
grep -q 'startup_contract_sha256 = "' CLAUDE.md
grep -q 'workspace_contract_required_before_tool_call = true' CLAUDE.md
grep -q 'missing_or_unreadable_fail_closed = true' CLAUDE.md
grep -q 'sha256_mismatch_fail_closed = true' CLAUDE.md
grep -q 'project_task_tree' CLAUDE.md
grep -q 'project_task_ledger' CLAUDE.md

./scripts/disconnect_local.sh --client claude-code
if [[ -f .mcp.json ]] && grep -q '"amai"' .mcp.json; then
  echo "proof_client_lifecycle: claude-code server entry still present after disconnect"
  exit 1
fi
if [[ -f CLAUDE.md ]] && grep -q 'AMAI MANAGED STARTUP INSTRUCTIONS v1' CLAUDE.md; then
  echo "proof_client_lifecycle: claude-code managed startup block still present after disconnect"
  exit 1
fi

echo "proof_client_lifecycle: ok"
