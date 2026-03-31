#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

temp_home="$(mktemp -d)"
export AMAI_INSTALL_STATE_PATH="${temp_home}/install_state.json"

RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
snapshot_dir="${temp_home}/repo-snapshots"
mkdir -p "${snapshot_dir}"

snapshot_file() {
  local path="$1"
  local key="$2"
  local state_path="${snapshot_dir}/${key}.state"
  local data_path="${snapshot_dir}/${key}.data"
  if [[ -e "${path}" ]]; then
    printf 'present\n' >"${state_path}"
    cp "${path}" "${data_path}"
  else
    printf 'absent\n' >"${state_path}"
  fi
}

assert_file_matches_snapshot() {
  local path="$1"
  local key="$2"
  local state_path="${snapshot_dir}/${key}.state"
  local data_path="${snapshot_dir}/${key}.data"
  local state
  state="$(cat "${state_path}")"
  if [[ "${state}" == "absent" ]]; then
    if [[ -e "${path}" ]]; then
      echo "proof_client_lifecycle: ${path} should have been removed"
      exit 1
    fi
    return
  fi
  if [[ ! -e "${path}" ]]; then
    echo "proof_client_lifecycle: ${path} disappeared but existed before proof"
    exit 1
  fi
  if ! cmp -s "${path}" "${data_path}"; then
    echo "proof_client_lifecycle: ${path} was not restored to its pre-proof state"
    exit 1
  fi
}

restore_file_from_snapshot() {
  local path="$1"
  local key="$2"
  local state_path="${snapshot_dir}/${key}.state"
  local data_path="${snapshot_dir}/${key}.data"
  [[ -f "${state_path}" ]] || return
  if [[ "$(cat "${state_path}")" == "absent" ]]; then
    rm -f "${path}"
    return
  fi
  mkdir -p "$(dirname "${path}")"
  cp "${data_path}" "${path}"
}

assert_startup_contract() {
  local path="$1"
  jq -e '
    .artifact_version == "workspace-startup-contract-v1" and
    (.startup_contract_sha256 | type == "string" and length > 0) and
    .startup_contract.artifact_enforcement.missing_or_unreadable_fail_closed == true and
    .startup_contract.artifact_enforcement.sha256_mismatch_fail_closed == true and
    .startup_contract.runtime_state_artifact.workspace_runtime_state_artifact_version == "workspace-startup-runtime-state-v4" and
    .startup_contract.runtime_state_artifact.workspace_runtime_state_relative_path == ".amai/continuity/project-chat-startup-state.json" and
    .startup_contract.runtime_state_artifact.startup_execution_gate_field == "startup_execution_gate" and
    .startup_contract.runtime_state_artifact.gate_semantics_consistent_field == "gate_semantics_consistent" and
    .startup_contract.runtime_state_artifact.gate_semantics_consistent_true_required == true and
    .startup_contract.startup_execution_gate_enforcement.must_follow_field == "must_follow_startup_next_action" and
    .startup_contract.startup_execution_gate_enforcement.unrelated_work_allowed_field == "unrelated_work_allowed" and
    .startup_contract.startup_execution_gate_enforcement.must_read_prompt_text_before_reply_field == "must_read_prompt_text_before_reply" and
    .startup_contract.startup_execution_gate_enforcement.required_action_kind_field == "required_action_kind_when_resume_required" and
    .startup_contract.startup_execution_gate_enforcement.no_silent_drop_field == "no_silent_drop" and
    .startup_contract.startup_execution_gate_enforcement.required_action_kind_resume_required_value == "resume_required_return_task" and
    .startup_contract.runtime_state_artifact.inspection_fallback_cli.command == "continuity startup-state" and
    .startup_contract.runtime_state_artifact.inspection_fallback_cli.shell_command == "./scripts/continuity_startup_state.sh" and
    (.startup_contract.required_summary_fields | index("project_task_tree") != null) and
    (.startup_contract.required_summary_fields | index("project_task_ledger") != null)
  ' "${path}" >/dev/null
}

assert_contains_all() {
  local path="$1"
  shift
  local needle
  for needle in "$@"; do
    grep -Fq "${needle}" "${path}"
  done
}

snapshot_file "AGENTS.md" "AGENTS.md"
snapshot_file ".cursor/rules/amai-continuity-startup.mdc" "cursor-rule"
snapshot_file "CLAUDE.md" "CLAUDE.md"
snapshot_file ".mcp.json" "repo-mcp-json"

cleanup() {
  restore_file_from_snapshot "AGENTS.md" "AGENTS.md"
  restore_file_from_snapshot ".cursor/rules/amai-continuity-startup.mdc" "cursor-rule"
  restore_file_from_snapshot "CLAUDE.md" "CLAUDE.md"
  restore_file_from_snapshot ".mcp.json" "repo-mcp-json"
  rm -rf "${temp_home}"
}

trap cleanup EXIT

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client codex --yes --skip-stack --skip-release-build
test -f "${temp_home}/.codex/config.toml"
grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"
test -f AGENTS.md
test -f .amai/onboarding/project-chat-startup-contract.json
assert_startup_contract .amai/onboarding/project-chat-startup-contract.json
assert_contains_all AGENTS.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'project `AGENTS.md`' \
  'execctl_resume_contract_summary' \
  'execctl_resume_obligation' \
  'startup_next_action' \
  'execctl_active_lease' \
  'lease_owner_state' \
  'previous_session_owner' \
  'resume_required_return_task' \
  'required_return_task' \
  'startup_contract_sha256 = "' \
  'workspace_contract_required_before_tool_call = true' \
  'missing_or_unreadable_fail_closed = true' \
  'sha256_mismatch_fail_closed = true' \
  '.amai/continuity/project-chat-startup-state.json' \
  'workspace-startup-runtime-state-v4' \
  'must_follow_startup_next_action' \
  'unrelated_work_allowed' \
  'must_read_prompt_text_before_reply' \
  'required_action_kind_when_resume_required' \
  'no_silent_drop' \
  'gate_semantics_consistent' \
  'gate_semantics_consistent_true_required = true' \
  './scripts/continuity_startup_state.sh --repo-root' \
  'project_task_tree' \
  'project_task_tree_summary' \
  'project_task_ledger' \
  'project_task_ledger_summary'

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/disconnect_local.sh --client codex
if [[ -f "${temp_home}/.codex/config.toml" ]] && grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"; then
  echo "proof_client_lifecycle: codex server entry still present after disconnect"
  exit 1
fi
if grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' AGENTS.md; then
  echo "proof_client_lifecycle: codex managed startup block still present after disconnect"
  exit 1
fi

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client cursor --yes --skip-stack --skip-release-build
test -f "${temp_home}/.cursor/mcp.json"
grep -q '"amai"' "${temp_home}/.cursor/mcp.json"
test -f .amai/onboarding/project-chat-startup-contract.json
assert_startup_contract .amai/onboarding/project-chat-startup-contract.json
test -f .cursor/rules/amai-continuity-startup.mdc
assert_contains_all .cursor/rules/amai-continuity-startup.mdc \
  'amai_continuity_startup' \
  'execctl_resume_contract_summary' \
  'execctl_resume_obligation' \
  'startup_next_action' \
  'execctl_active_lease' \
  'lease_owner_state' \
  'previous_session_owner' \
  'resume_required_return_task' \
  'required_return_task' \
  'startup_contract_sha256 = "' \
  'workspace_contract_required_before_tool_call = true' \
  'missing_or_unreadable_fail_closed = true' \
  'sha256_mismatch_fail_closed = true' \
  '.amai/continuity/project-chat-startup-state.json' \
  'workspace-startup-runtime-state-v4' \
  'must_follow_startup_next_action' \
  'unrelated_work_allowed' \
  'must_read_prompt_text_before_reply' \
  'required_action_kind_when_resume_required' \
  'no_silent_drop' \
  'gate_semantics_consistent' \
  'gate_semantics_consistent_true_required = true' \
  './scripts/continuity_startup_state.sh --repo-root' \
  'project_task_tree' \
  'project_task_tree_summary' \
  'project_task_ledger' \
  'project_task_ledger_summary'

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
assert_startup_contract .amai/onboarding/project-chat-startup-contract.json
test -f CLAUDE.md
assert_contains_all CLAUDE.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'amai_continuity_startup' \
  'execctl_resume_contract_summary' \
  'execctl_resume_obligation' \
  'startup_next_action' \
  'execctl_active_lease' \
  'lease_owner_state' \
  'previous_session_owner' \
  'resume_required_return_task' \
  'required_return_task' \
  'startup_contract_sha256 = "' \
  'workspace_contract_required_before_tool_call = true' \
  'missing_or_unreadable_fail_closed = true' \
  'sha256_mismatch_fail_closed = true' \
  '.amai/continuity/project-chat-startup-state.json' \
  'workspace-startup-runtime-state-v4' \
  'must_follow_startup_next_action' \
  'unrelated_work_allowed' \
  'must_read_prompt_text_before_reply' \
  'required_action_kind_when_resume_required' \
  'no_silent_drop' \
  'gate_semantics_consistent' \
  'gate_semantics_consistent_true_required = true' \
  './scripts/continuity_startup_state.sh --repo-root' \
  'project_task_tree' \
  'project_task_tree_summary' \
  'project_task_ledger' \
  'project_task_ledger_summary'

./scripts/disconnect_local.sh --client claude-code
if [[ -f .mcp.json ]] && grep -q '"amai"' .mcp.json; then
  echo "proof_client_lifecycle: claude-code server entry still present after disconnect"
  exit 1
fi
if [[ -f CLAUDE.md ]] && grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' CLAUDE.md; then
  echo "proof_client_lifecycle: claude-code managed startup block still present after disconnect"
  exit 1
fi

echo "proof_client_lifecycle: ok"
