#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_file=".amai/continuity/project-chat-startup-state.json"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
test -f "${state_file}"
test -f "${startup_contract}"

startup_contract_sha="$(jq -r '.startup_contract_sha256 // empty' "${startup_contract}")"
test -n "${startup_contract_sha}"
current_redirect_id="$(jq -r '.workflow_promotion_state.current_user_redirect_id // empty' "${state_file}")"
promoted_redirect_id="$(jq -r '.workflow_promotion_state.promoted_user_redirect_id // empty' "${state_file}")"
state_generated_at_epoch_ms="$(jq -r '.generated_at_epoch_ms // empty' "${state_file}")"
test -n "${current_redirect_id}"
test -n "${promoted_redirect_id}"
test -n "${state_generated_at_epoch_ms}"
expected_workflow_promotion_event_id="workflow-promotion-$(printf '%s' "${current_redirect_id}:${promoted_redirect_id}:${state_generated_at_epoch_ms}" | sha256sum | awk '{print $1}')"

validate_workflow_guard() {
  local file="$1"
  jq -e \
    --arg startup_contract_sha "${startup_contract_sha}" \
    --arg expected_workflow_promotion_event_id "${expected_workflow_promotion_event_id}" '
    .artifact_version == "workspace-startup-runtime-state-v4"
    and .startup_contract_sha256 == $startup_contract_sha
    and .agent_workflow_guard.guard_version == "agent-workflow-guard-v1"
    and .agent_workflow_guard.must_run_before_substantive_report == true
    and .agent_workflow_guard.promotion_identity.runtime_state_field == "workflow_promotion_state"
    and .agent_workflow_guard.promotion_identity.current_user_redirect_id_field == "current_user_redirect_id"
    and .agent_workflow_guard.promotion_identity.promoted_user_redirect_id_field == "promoted_user_redirect_id"
    and .agent_workflow_guard.promotion_identity.workflow_promotion_event_id_field == "workflow_promotion_event_id"
    and .agent_workflow_guard.promotion_identity.workflow_promotion_event_id_required == true
    and .agent_workflow_guard.promotion_identity.workflow_promotion_event_id_fresh_runtime_nonce_required == true
    and .agent_workflow_guard.promotion_identity.source_event_match_required == true
    and .agent_workflow_guard.promotion_identity.missing_or_mismatch_blocks_report == true
    and .workflow_promotion_state.state_version == "workflow-promotion-state-v1"
    and (.workflow_promotion_state.current_user_redirect_id | type) == "string"
    and (.workflow_promotion_state.current_user_redirect_id | length) > 0
    and (.workflow_promotion_state.promoted_user_redirect_id | type) == "string"
    and (.workflow_promotion_state.promoted_user_redirect_id | length) > 0
    and .workflow_promotion_state.current_user_redirect_id == .workflow_promotion_state.promoted_user_redirect_id
    and .workflow_promotion_state.workflow_promotion_event_id == $expected_workflow_promotion_event_id
    and (.workflow_promotion_state.workflow_promotion_event_epoch_ms | tostring) == (.generated_at_epoch_ms | tostring)
    and (.working_state_restore_lineage.authoritative_event_id | type) == "string"
    and .workflow_promotion_state.current_user_redirect_id == .working_state_restore_lineage.authoritative_event_id
    and (.continuity_startup_summary.execctl_active_lease.source_event_id | type) == "string"
    and .workflow_promotion_state.promoted_user_redirect_id == .continuity_startup_summary.execctl_active_lease.source_event_id
    and .workflow_promotion_state.source_event_match == true
    and .workflow_promotion_state.missing_or_mismatch_blocks_report == true
    and .agent_workflow_guard.research_scope.documentation_required == true
    and .agent_workflow_guard.research_scope.code_required == true
    and .agent_workflow_guard.research_scope.external_references_required_when_user_or_task_requires_current_state == true
    and .agent_workflow_guard.research_scope.risk_review_required == true
    and .agent_workflow_guard.external_reference_policy.source_allowlist_kind == "official_or_primary_sources_only"
    and .agent_workflow_guard.external_reference_policy.freshness_or_version_metadata_required == true
    and .agent_workflow_guard.external_reference_policy.advisory_only == true
    and .agent_workflow_guard.external_reference_policy.raw_external_text_not_authoritative == true
    and .agent_workflow_guard.external_reference_policy.local_corroboration_required_before_truth_or_runtime_change == true
    and .agent_workflow_guard.external_reference_policy.must_not_override_local_contracts_or_gates == true
    and .agent_workflow_guard.planning.plan_required == true
    and .agent_workflow_guard.planning.plan_items_must_be_reviewed_one_by_one == true
    and .agent_workflow_guard.planning.specialist_consensus_required_before_implementation == true
    and .agent_workflow_guard.implementation.implement_after_plan_consensus == true
    and .agent_workflow_guard.implementation.per_item_specialist_bughunter_review_required == true
    and .agent_workflow_guard.implementation.local_debug_fix_retest_required == true
    and .agent_workflow_guard.report_gate.before_report_guard_required == true
    and .agent_workflow_guard.report_gate.workflow_before_report_guard_command == "./scripts/proof_workflow_before_report.sh"
    and .agent_workflow_guard.report_gate.before_report_bundle_command == "./scripts/proof_before_report.sh"
    and .agent_workflow_guard.report_gate.specialist_signoff_required == true
    and .agent_workflow_guard.report_gate.specialist_signoff_artifact == ".amai/continuity/specialist-team-signoff.json"
    and .agent_workflow_guard.report_gate.specialist_signoff_source_manifest == ".amai/continuity/specialist-team-signoff-source.json"
    and .agent_workflow_guard.report_gate.specialist_signoff_guard_command == "./scripts/proof_specialist_signoff.sh"
    and .agent_workflow_guard.report_gate.specialist_signoff_materialize_command == "./scripts/materialize_specialist_signoff.sh"
    and .agent_workflow_guard.report_gate.specialist_signoff_trust_provision_command == "./scripts/provision_specialist_signoff_trust.sh"
    and .agent_workflow_guard.report_gate.specialist_signoff_trust_root == "$HOME/.local/share/amai/signoff-trust/allowed_signers"
    and .agent_workflow_guard.report_gate.specialist_signoff_anti_replay_required == true
    and .agent_workflow_guard.report_gate.final_specialist_bughunter_pass_required == true
    and .agent_workflow_guard.report_gate.report_allowed_requires_all_green == true
    and .agent_workflow_guard.language_policy.user_reply_language == "ru"
    and .agent_workflow_guard.language_policy.subagent_language == "en"
    and .gate_semantics_consistent == true
    and .startup_execution_gate.no_silent_drop == true
    and (.required_task_set | type) == "array"
    and all(.required_task_set[]; type == "string" and length > 0)
    and .required_task_set == .continuity_startup_summary.required_task_set
    and .required_task_set == .startup_next_action.required_task_set
    and .required_task_set == .execctl_resume_obligation.required_task_set
    and (.startup_execution_gate.required_task_set_count == (.required_task_set | length))
    and (.startup_execution_gate.required_task_set_present == ((.required_task_set | length) > 0))
    and (.startup_execution_gate.must_preserve_required_task_set == ((.required_task_set | length) > 0))
  ' "${file}" >/dev/null
}

validate_workflow_guard "${state_file}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

jq 'del(.agent_workflow_guard)' "${state_file}" >"${tmp_dir}/missing-guard.json"
if validate_workflow_guard "${tmp_dir}/missing-guard.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing guard unexpectedly passed" >&2
  exit 1
fi

jq '.agent_workflow_guard.planning.specialist_consensus_required_before_implementation = false' \
  "${state_file}" >"${tmp_dir}/no-consensus.json"
if validate_workflow_guard "${tmp_dir}/no-consensus.json" 2>/dev/null; then
  echo "proof_workflow_before_report: disabled consensus unexpectedly passed" >&2
  exit 1
fi

jq '.agent_workflow_guard.report_gate.report_allowed_requires_all_green = false' \
  "${state_file}" >"${tmp_dir}/report-not-blocked.json"
if validate_workflow_guard "${tmp_dir}/report-not-blocked.json" 2>/dev/null; then
  echo "proof_workflow_before_report: disabled report gate unexpectedly passed" >&2
  exit 1
fi

jq '.agent_workflow_guard.report_gate.specialist_signoff_required = false' \
  "${state_file}" >"${tmp_dir}/signoff-not-required.json"
if validate_workflow_guard "${tmp_dir}/signoff-not-required.json" 2>/dev/null; then
  echo "proof_workflow_before_report: disabled specialist signoff unexpectedly passed" >&2
  exit 1
fi

jq '.agent_workflow_guard.external_reference_policy.advisory_only = false' \
  "${state_file}" >"${tmp_dir}/external-refs-authoritative.json"
if validate_workflow_guard "${tmp_dir}/external-refs-authoritative.json" 2>/dev/null; then
  echo "proof_workflow_before_report: authoritative external refs unexpectedly passed" >&2
  exit 1
fi

jq '.startup_contract_sha256 = "218c603815692422ef3fd648b7672acae69eea587e3ba23cf5c75d6fb481f1da"' \
  "${state_file}" >"${tmp_dir}/stale-startup-contract-sha.json"
if validate_workflow_guard "${tmp_dir}/stale-startup-contract-sha.json" 2>/dev/null; then
  echo "proof_workflow_before_report: stale startup contract sha unexpectedly passed" >&2
  exit 1
fi

jq '.startup_execution_gate.required_task_set_count = 999' \
  "${state_file}" >"${tmp_dir}/task-set-drift.json"
if validate_workflow_guard "${tmp_dir}/task-set-drift.json" 2>/dev/null; then
  echo "proof_workflow_before_report: task-set drift unexpectedly passed" >&2
  exit 1
fi

jq '.required_task_set[0] = "forged same-length task set item"' \
  "${state_file}" >"${tmp_dir}/task-set-content-drift.json"
if validate_workflow_guard "${tmp_dir}/task-set-content-drift.json" 2>/dev/null; then
  echo "proof_workflow_before_report: task-set content drift unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_promotion_state.promoted_user_redirect_id = "different-event"' \
  "${state_file}" >"${tmp_dir}/promotion-mismatch.json"
if validate_workflow_guard "${tmp_dir}/promotion-mismatch.json" 2>/dev/null; then
  echo "proof_workflow_before_report: promotion mismatch unexpectedly passed" >&2
  exit 1
fi

jq 'del(.working_state_restore_lineage.authoritative_event_id)' \
  "${state_file}" >"${tmp_dir}/missing-lineage-event-id.json"
if validate_workflow_guard "${tmp_dir}/missing-lineage-event-id.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing lineage event id unexpectedly passed" >&2
  exit 1
fi

jq 'del(.continuity_startup_summary.execctl_active_lease.source_event_id)' \
  "${state_file}" >"${tmp_dir}/missing-active-lease-source-event-id.json"
if validate_workflow_guard "${tmp_dir}/missing-active-lease-source-event-id.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing active lease source event id unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_promotion_state.source_event_match = false' \
  "${state_file}" >"${tmp_dir}/promotion-source-event-match-disabled.json"
if validate_workflow_guard "${tmp_dir}/promotion-source-event-match-disabled.json" 2>/dev/null; then
  echo "proof_workflow_before_report: disabled source event match unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_promotion_state.workflow_promotion_event_id = "replayed-event"' \
  "${state_file}" >"${tmp_dir}/promotion-event-id-replay.json"
if validate_workflow_guard "${tmp_dir}/promotion-event-id-replay.json" 2>/dev/null; then
  echo "proof_workflow_before_report: replayed workflow promotion event unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_promotion_state.current_user_redirect_id = "same-forged-event" | .workflow_promotion_state.promoted_user_redirect_id = "same-forged-event"' \
  "${state_file}" >"${tmp_dir}/promotion-forged-equal.json"
if validate_workflow_guard "${tmp_dir}/promotion-forged-equal.json" 2>/dev/null; then
  echo "proof_workflow_before_report: forged equal promotion ids unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_promotion_state.state_version = "workflow-promotion-state-v0"' \
  "${state_file}" >"${tmp_dir}/promotion-state-version-drift.json"
if validate_workflow_guard "${tmp_dir}/promotion-state-version-drift.json" 2>/dev/null; then
  echo "proof_workflow_before_report: promotion state version drift unexpectedly passed" >&2
  exit 1
fi

echo "proof_workflow_before_report: ok"
