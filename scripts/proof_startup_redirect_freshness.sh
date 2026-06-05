#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd)"

state_file=".amai/continuity/project-chat-startup-state.json"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
startup_agent_contract="AGENTS.md"
test -f "${startup_contract}"
test -f "${startup_agent_contract}"

compute_startup_contract_object_sha() {
  python3 - "${startup_contract}" <<'PY'
import hashlib
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
blob = json.dumps(
    payload["startup_contract"],
    ensure_ascii=False,
    separators=(",", ":"),
).encode("utf-8")
print(hashlib.sha256(blob).hexdigest())
PY
}

extract_agent_pinned_startup_sha() {
  grep -oE 'startup_contract_sha256 = "[0-9a-f]{64}"' "${startup_agent_contract}" \
    | head -n1 \
    | sed -E 's/.*"([0-9a-f]{64})"/\1/'
}

run_exact_test_expect_single() {
  local test_name="$1"
  local output=""
  if ! output="$(cargo test --quiet "${test_name}" -- --exact 2>&1)"; then
    printf '%s\n' "${output}" >&2
    exit 1
  fi
  if ! printf '%s\n' "${output}" | grep -Fq "running 1 test"; then
    echo "proof_startup_redirect_freshness: exact test did not run for ${test_name}" >&2
    printf '%s\n' "${output}" >&2
    exit 1
  fi
  if ! printf '%s\n' "${output}" | grep -Fq "test result: ok. 1 passed;"; then
    echo "proof_startup_redirect_freshness: exact test did not pass cleanly for ${test_name}" >&2
    printf '%s\n' "${output}" >&2
    exit 1
  fi
}

./scripts/continuity_startup.sh --repo-root "${repo_root}" --namespace continuity --json >/dev/null
test -f "${state_file}"

now_ms="$(./scripts/epoch_ms.sh)"
generated_at_ms="$(jq -r '.generated_at_epoch_ms // 0' "${state_file}")"
[[ "$generated_at_ms" =~ ^[0-9]+$ ]] || exit 1
(( now_ms - generated_at_ms <= 180000 )) || exit 1

run_exact_test_expect_single onboarding::tests::renders_vscode_startup_instructions_with_repo_root
run_exact_test_expect_single continuity::tests::startup_runtime_state_artifact_preserves_execctl_active_lease_source_event_id_alignment
run_exact_test_expect_single continuity::tests::startup_runtime_state_artifact_keeps_missing_execctl_active_lease_source_event_id_fail_closed
run_exact_test_expect_single continuity::tests::inspect_startup_runtime_state_fails_closed_on_missing_execctl_active_lease_source_event_id

jq -e '.artifact_version == "workspace-startup-runtime-state-v4"' "${state_file}" >/dev/null
jq -e '.source_tool == "amai_continuity_startup"' "${state_file}" >/dev/null
jq -e '.gate_semantics_consistent == true' "${state_file}" >/dev/null
jq -e '.working_state_restore_lineage.authoritative_event_id | type == "string" and length > 0' "${state_file}" >/dev/null
jq -e '.continuity_startup_summary.execctl_active_lease.source_event_id | type == "string" and length > 0' "${state_file}" >/dev/null
jq -e '.working_state_restore_lineage.authoritative_event_id == .continuity_startup_summary.execctl_active_lease.source_event_id' "${state_file}" >/dev/null
jq -e '.continuity_startup_summary.prompt_text_present == true' "${state_file}" >/dev/null
jq -e '.continuity_startup_summary.startup_execution_gate.no_silent_drop == true' "${state_file}" >/dev/null
startup_contract_sha="$(jq -r '.startup_contract_sha256 // empty' "${startup_contract}")"
recomputed_startup_contract_sha="$(compute_startup_contract_object_sha)"
agent_pinned_startup_sha="$(extract_agent_pinned_startup_sha)"
jq -e '.startup_contract_sha256_scope == "startup_contract object only"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.sha256_mismatch_fail_closed == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.workspace_contract_required_before_tool_call == true' "${startup_contract}" >/dev/null
[[ "${recomputed_startup_contract_sha}" == "${startup_contract_sha}" ]] || exit 1
[[ "${agent_pinned_startup_sha}" == "${startup_contract_sha}" ]] || exit 1
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.detect_after_any_success == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.local_cli_unavailable_blocks_report == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.local_cli_success_replaces_stale_mcp_success == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("missing_agent_workflow_guard") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("startup_contract_sha_mismatch") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("missing_working_state_lineage_event_id") != null' "${startup_contract}" >/dev/null
jq -e '.startup_contract.tool_runtime_reconcile.success_payload_stale_runtime_artifact.stale_conditions | index("missing_active_lease_source_event_id") != null' "${startup_contract}" >/dev/null
jq -e --arg startup_contract_sha "${startup_contract_sha}" '.startup_contract_sha256 == $startup_contract_sha' "${state_file}" >/dev/null
startup_state_output="$(./scripts/continuity_startup_state.sh --repo-root "${repo_root}" --json)"
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.startup_contract_sha_matches_current_contract == true' >/dev/null
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.agent_workflow_guard.guard_version == "agent-workflow-guard-v2"' >/dev/null
printf '%s\n' "${startup_state_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.source_event_match == true' >/dev/null

echo "proof_startup_redirect_freshness: ok"
