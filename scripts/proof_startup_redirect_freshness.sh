#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_file=".amai/continuity/project-chat-startup-state.json"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
test -f "${state_file}"
test -f "${startup_contract}"

now_ms="$(./scripts/epoch_ms.sh)"
generated_at_ms="$(jq -r '.generated_at_epoch_ms // 0' "${state_file}")"
[[ "$generated_at_ms" =~ ^[0-9]+$ ]] || exit 1
(( now_ms - generated_at_ms <= 180000 )) || exit 1

cargo test --quiet onboarding::tests::renders_vscode_startup_instructions_with_repo_root -- --exact
cargo test --quiet continuity::tests::startup_runtime_state_artifact_preserves_execctl_active_lease_source_event_id_alignment -- --exact
cargo test --quiet continuity::tests::startup_runtime_state_artifact_backfills_execctl_active_lease_source_event_id_from_lineage -- --exact

jq -e '.artifact_version == "workspace-startup-runtime-state-v4"' "${state_file}" >/dev/null
jq -e '.source_tool == "amai_continuity_startup"' "${state_file}" >/dev/null
jq -e '.gate_semantics_consistent == true' "${state_file}" >/dev/null
jq -e '.working_state_restore_lineage.authoritative_event_id | type == "string" and length > 0' "${state_file}" >/dev/null
jq -e '.continuity_startup_summary.execctl_active_lease.source_event_id | type == "string" and length > 0' "${state_file}" >/dev/null
jq -e '.working_state_restore_lineage.authoritative_event_id == .continuity_startup_summary.execctl_active_lease.source_event_id' "${state_file}" >/dev/null
jq -e '.continuity_startup_summary.prompt_text_present == true' "${state_file}" >/dev/null
jq -e '.continuity_startup_summary.startup_execution_gate.no_silent_drop == true' "${state_file}" >/dev/null
startup_contract_sha="$(jq -r '.startup_contract_sha256 // empty' "${startup_contract}")"
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.detect_after_any_success == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.local_cli_unavailable_blocks_report == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.local_cli_success_replaces_stale_mcp_success == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("missing_agent_workflow_guard") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("startup_contract_sha_mismatch") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("missing_working_state_lineage_event_id") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("missing_active_lease_source_event_id") != null' "${startup_contract}" >/dev/null
jq -e --arg startup_contract_sha "${startup_contract_sha}" '.startup_contract_sha256 == $startup_contract_sha' "${state_file}" >/dev/null
startup_state_output="$(./scripts/continuity_startup_state.sh --repo-root "/home/art/agent-memory-index" --json)"
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.startup_contract_sha_matches_current_contract == true' >/dev/null
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.agent_workflow_guard.guard_version == "agent-workflow-guard-v1"' >/dev/null
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.source_event_match == true' >/dev/null

echo "proof_startup_redirect_freshness: ok"
