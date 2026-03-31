#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="tmp/onboarding/proof-vscode-mcp.json"
human_output="tmp/onboarding/proof-vscode.out"
startup_output=".github/instructions/amai-continuity-startup.instructions.md"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
rm -f "${output}" "${human_output}" "${startup_output}" "${startup_contract}"

cargo build --release --quiet
./scripts/onboard_local.sh --client vscode --yes --output "${output}" >"${human_output}"

test -f .env
test -f "${output}"
test -f "${human_output}"
test -f "${startup_output}"
test -f "${startup_contract}"
grep -q '"servers"' "${output}"
grep -q 'run_mcp_stdio.sh' "${output}"
jq -e '.artifact_version == "workspace-startup-contract-v1"' "${startup_contract}" >/dev/null
jq -e '.startup_contract_sha256 | type == "string" and length > 0' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.workspace_contract_relative_path == ".amai/onboarding/project-chat-startup-contract.json"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.missing_or_unreadable_fail_closed == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.sha256_mismatch_fail_closed == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.workspace_runtime_state_artifact_version == "workspace-startup-runtime-state-v4"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.resume_enforcement.required_action_kind_when_resume_required == "resume_required_return_task"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.blocking_action_kinds == ["wait_for_global_client_budget_recovery"]' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.blocking_reply_response_kind == "wait_for_budget_only"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.reply_prefix_field == "reply_prefix"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.target_control.exact_chat_command_pattern == "^экономия_(0|10|20|30|40|50|60|70|80|90)%$"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.target_control.cli_command == "continuity client-budget-target"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.target_control.shell_command == "./scripts/continuity_client_budget_target.sh"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.compact_chat_control.exact_chat_command == "компакт_чат"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.compact_chat_control.cli_command == "continuity compact-chat"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.compact_chat_control.shell_command == "./scripts/continuity_compact_chat.sh"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.live_client_budget_enforcement.compact_chat_control.required_host_action == "open_clean_chat_surface_and_inject_prompt_text"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.workspace_runtime_state_relative_path == ".amai/continuity/project-chat-startup-state.json"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.startup_execution_gate_field == "startup_execution_gate"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.gate_semantics_consistent_field == "gate_semantics_consistent"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.gate_semantics_consistent_true_required == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.inspection_fallback_cli.shell_command == "./scripts/continuity_startup_state.sh"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.error_class == "tool_execution_failed"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.error_detail_contains == "no continuity import found for"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.transport_error_detail_contains == "Transport closed"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.transport_error_detail_case_insensitive == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli.command == "continuity startup"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli.shell_command == "./scripts/continuity_startup.sh"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli.requires_repo_root_argument == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli.requires_namespace_argument == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli.json_required == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli_success_classification == "stale_embedded_mcp_session"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.local_cli_success_replaces_transport_failure == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.must_request_mcp_reconnect_after_local_success == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.must_continue_from_local_startup_payload == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.reconnect_helper.shell_helper_relative_path == "./scripts/reconnect_local.sh"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.reconnect_helper.bootstrap_command == "bootstrap reconnect"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.must_follow_field == "must_follow_startup_next_action"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.unrelated_work_allowed_field == "unrelated_work_allowed"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.must_read_prompt_text_before_reply_field == "must_read_prompt_text_before_reply"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.required_action_kind_field == "required_action_kind_when_resume_required"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.no_silent_drop_field == "no_silent_drop"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.must_follow_true_blocks_unrelated_work == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.startup_execution_gate_enforcement.required_action_kind_resume_required_value == "resume_required_return_task"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.inspection_fallback_cli.command == "continuity startup-state"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.runtime_state_artifact.startup_execution_gate_field == "startup_execution_gate"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.required_summary_fields | index("project_task_tree") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.required_summary_fields | index("project_task_ledger") != null' "${startup_contract}" >/dev/null
grep -q 'amai_continuity_startup' "${startup_output}"
grep -q '.amai/onboarding/project-chat-startup-contract.json' "${startup_output}"
grep -q 'startup_contract_sha256 = "' "${startup_output}"
grep -q 'workspace_contract_required_before_tool_call = true' "${startup_output}"
grep -q 'missing_or_unreadable_fail_closed = true' "${startup_output}"
grep -q 'sha256_mismatch_fail_closed = true' "${startup_output}"
grep -q '.amai/continuity/project-chat-startup-state.json' "${startup_output}"
grep -q 'workspace-startup-runtime-state-v4' "${startup_output}"
grep -q 'startup_execution_gate' "${startup_output}"
grep -q 'must_follow_startup_next_action' "${startup_output}"
grep -q 'unrelated_work_allowed' "${startup_output}"
grep -q 'must_read_prompt_text_before_reply' "${startup_output}"
grep -q 'required_action_kind_when_resume_required' "${startup_output}"
grep -q 'no_silent_drop' "${startup_output}"
grep -q 'gate_semantics_consistent' "${startup_output}"
grep -q 'gate_semantics_consistent_true_required' "${startup_output}"
grep -q './scripts/continuity_startup_state.sh --repo-root' "${startup_output}"
grep -q 'tool_execution_failed' "${startup_output}"
grep -q 'no continuity import found for' "${startup_output}"
grep -q 'Transport closed' "${startup_output}"
grep -q './scripts/continuity_startup.sh --repo-root' "${startup_output}"
grep -q 'requires_namespace_argument = true' "${startup_output}"
grep -q 'stale_embedded_mcp_session' "${startup_output}"
grep -q 'local_cli_success_replaces_transport_failure = true' "${startup_output}"
grep -q 'must_request_mcp_reconnect_after_local_success = true' "${startup_output}"
grep -q 'must_continue_from_local_startup_payload = true' "${startup_output}"
grep -q './scripts/reconnect_local.sh --client vscode' "${startup_output}"
grep -q './scripts/amai_exec.sh bootstrap reconnect --client vscode --yes' "${startup_output}"
grep -q 'execctl_resume_contract_summary' "${startup_output}"
grep -q 'execctl_resume_obligation' "${startup_output}"
grep -q 'startup_next_action' "${startup_output}"
grep -q 'execctl_active_lease' "${startup_output}"
grep -q 'lease_owner_state' "${startup_output}"
grep -q 'execctl_active_lease_summary' "${startup_output}"
grep -q 'previous_session_owner' "${startup_output}"
grep -q 'resume_required_return_task' "${startup_output}"
grep -q 'reply_execution_gate.reply_prefix' "${startup_output}"
grep -q 'точную команду `компакт_чат`' "${startup_output}"
grep -q './scripts/continuity_client_budget_target.sh --repo-root' "${startup_output}"
grep -q './scripts/continuity_compact_chat.sh --repo-root' "${startup_output}"
grep -q 'open_clean_chat_surface_and_inject_prompt_text' "${startup_output}"
grep -q 'warning/advisory pressure signal' "${startup_output}"
grep -q 'response_kind = "wait_for_budget_only"' "${startup_output}"
grep -q 'required_return_task' "${startup_output}"
grep -q 'project_task_tree' "${startup_output}"
grep -q 'project_task_tree_summary' "${startup_output}"
grep -q 'project_task_ledger' "${startup_output}"
grep -q 'project_task_ledger_summary' "${startup_output}"
grep -q 'Auto-start readiness: instruction-backed' "${human_output}"
grep -q 'Machine-readable startup contract:' "${human_output}"
grep -q 'Где лежит startup contract JSON:' "${human_output}"
grep -q 'Startup contract SHA-256:' "${human_output}"
grep -q 'Почему такой режим:' "${human_output}"
grep -q 'Что машина реально показала после установки:' "${human_output}"

cargo run --quiet -- status >/dev/null

echo "proof_onboarding: ok"
