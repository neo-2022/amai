#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_file=".amai/continuity/project-chat-startup-state.json"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
startup_agent_contract="AGENTS.md"
signoff_input_file=".amai/continuity/specialist-team-signoff-input.json"
test -f "${state_file}"
test -f "${startup_contract}"
test -f "${startup_agent_contract}"
test -f "${signoff_input_file}"

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

startup_contract_sha="$(jq -r '.startup_contract_sha256 // empty' "${startup_contract}")"
recomputed_startup_contract_sha="$(compute_startup_contract_object_sha)"
agent_pinned_startup_sha="$(extract_agent_pinned_startup_sha)"
jq -e '.startup_contract_sha256_scope == "startup_contract object only"' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.sha256_mismatch_fail_closed == true' "${startup_contract}" >/dev/null
jq -e '.startup_contract.artifact_enforcement.workspace_contract_required_before_tool_call == true' "${startup_contract}" >/dev/null
[[ "${recomputed_startup_contract_sha}" == "${startup_contract_sha}" ]]
[[ "${agent_pinned_startup_sha}" == "${startup_contract_sha}" ]]
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
    and .agent_workflow_guard.guard_version == "agent-workflow-guard-v2"
    and .agent_workflow_guard.must_run_before_substantive_report == true
    and .agent_workflow_guard.workflow_cycle.ordered_stage_codes == [
      "analysis",
      "plan",
      "team_critique",
      "implementation",
      "team_verification",
      "fix",
      "reverify",
      "final_audit",
      "report"
    ]
    and .agent_workflow_guard.workflow_cycle.must_not_skip_stages == true
    and .agent_workflow_guard.workflow_cycle.must_not_advance_until_current_stage_closed == true
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
    and .workflow_promotion_state.headline_match == true
    and .workflow_promotion_state.source_kind_match == true
    and (.workflow_promotion_state.active_workline_headline | type) == "string"
    and (.workflow_promotion_state.active_workline_headline | length) > 0
    and (.workflow_promotion_state.active_lease_headline | type) == "string"
    and .workflow_promotion_state.active_workline_headline == .workflow_promotion_state.active_lease_headline
    and .workflow_promotion_state.active_workline_headline == .continuity_startup_summary.headline
    and .workflow_promotion_state.active_workline_headline == .continuity_startup_summary.execctl_active_lease.headline
    and .workflow_promotion_state.active_workline_headline == .execctl_active_lease.headline
    and .workflow_promotion_state.active_workline_headline == .working_state_restore_lineage.authoritative_headline
    and (.workflow_promotion_state.active_workline_source_kind | type) == "string"
    and (.workflow_promotion_state.active_workline_source_kind | length) > 0
    and (.workflow_promotion_state.active_lease_source_kind | type) == "string"
    and .workflow_promotion_state.active_workline_source_kind == .workflow_promotion_state.active_lease_source_kind
    and .workflow_promotion_state.active_workline_source_kind == .continuity_startup_summary.execctl_active_lease.source_kind
    and .workflow_promotion_state.active_workline_source_kind == .execctl_active_lease.source_kind
    and .workflow_promotion_state.active_workline_source_kind == .working_state_restore_lineage.authoritative_source_kind
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
    and .agent_workflow_guard.analysis.required_review_items == [
      "user_goal",
      "requirements",
      "constraints",
      "existing_code",
      "dependencies",
      "risks",
      "done_criteria",
      "verification_method"
    ]
    and .agent_workflow_guard.planning.plan_required == true
    and .agent_workflow_guard.planning.plan_items_must_be_reviewed_one_by_one == true
    and .agent_workflow_guard.planning.specialist_consensus_required_before_implementation == true
    and .agent_workflow_guard.planning.plan_item_required_fields == [
      "goal",
      "expected_result",
      "risks",
      "verification_method",
      "completion_criteria"
    ]
    and .agent_workflow_guard.team_critique.roles == [
      "architect",
      "senior_developer",
      "tester",
      "security_engineer",
      "devops_if_applicable",
      "skeptic"
    ]
    and .agent_workflow_guard.team_critique.specialist_contour_policy == "local_or_explicitly_allowed_only"
    and .agent_workflow_guard.team_critique.approval_status_code_before_implementation == "approved_no_comments"
    and .agent_workflow_guard.team_critique.approval_status_label_before_implementation == "согласовано, замечаний нет"
    and .agent_workflow_guard.team_critique.must_seek_errors_risks_complexity_and_gaps == true
    and .agent_workflow_guard.implementation.implement_after_plan_consensus == true
    and .agent_workflow_guard.implementation.per_item_specialist_bughunter_review_required == true
    and .agent_workflow_guard.implementation.local_debug_fix_retest_required == true
    and .agent_workflow_guard.implementation.prohibited_actions == [
      "unrequested_scope_change",
      "unnecessary_complexity",
      "architecture_breakage",
      "hidden_assumptions",
      "unverified_code",
      "ignored_errors"
    ]
    and .agent_workflow_guard.team_verification.required_after_each_plan_item == true
    and .agent_workflow_guard.team_verification.verification_dimensions == [
      "logic",
      "errors",
      "edge_cases",
      "regressions",
      "integration",
      "security",
      "performance",
      "readability",
      "maintainability",
      "task_fit"
    ]
    and .agent_workflow_guard.team_verification.approval_status_code_after_verification == "no_defects_found"
    and .agent_workflow_guard.team_verification.approval_status_label_after_verification == "недостатков не найдено"
    and .agent_workflow_guard.fix_loop.return_to_implementation_on_code_issue == true
    and .agent_workflow_guard.fix_loop.return_to_planning_on_plan_issue == true
    and .agent_workflow_guard.fix_loop.return_to_analysis_on_understanding_issue == true
    and .agent_workflow_guard.fix_loop.replay_downstream_checks_after_final_audit_issue == true
    and .agent_workflow_guard.fix_loop.unresolved_issues_block_progress == true
    and .agent_workflow_guard.integration_verification.required_before_final_audit == true
    and .agent_workflow_guard.integration_verification.checklist == [
      "works_together",
      "no_conflicts",
      "no_regressions",
      "architecture_preserved",
      "security_not_worse",
      "tests_build_checks_pass",
      "matches_user_request"
    ]
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
    and .agent_workflow_guard.final_audit.team_required == true
    and .agent_workflow_guard.final_audit.any_issue_blocks_ready_verdict == true
    and .agent_workflow_guard.report.required_items == [
      "done",
      "changed",
      "checks_run",
      "problems_found_and_fixed",
      "remaining_risks_or_limitations",
      "not_verified"
    ]
    and .agent_workflow_guard.completion_rule.unresolved_issue_blocks_completion == true
    and .agent_workflow_guard.completion_rule.must_loop_until_team_stops_finding_material_issues == true
    and .agent_workflow_guard.language_policy.user_reply_language == "ru"
    and .agent_workflow_guard.language_policy.user_reply_style == "simple_non_technical_when_possible"
    and .agent_workflow_guard.language_policy.avoid_anglicisms_when_possible == true
    and .agent_workflow_guard.language_policy.subagent_language == "en"
    and .gate_semantics_consistent == true
    and .startup_execution_gate.blocking == false
    and .startup_execution_gate.action_kind == "continue_active_workline"
    and .startup_execution_gate.must_follow_startup_next_action == true
    and .startup_execution_gate.unrelated_work_allowed == false
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

validate_workflow_trace_input() {
  local state="$1"
  local input="$2"
  cargo run --quiet -- verify workflow-trace \
    --state "${state}" \
    --startup-contract "${startup_contract}" \
    --input "${input}" \
    --fail-on-blocking-startup-gate >/dev/null
}

validate_workflow_guard "${state_file}"
validate_workflow_trace_input "${state_file}" "${signoff_input_file}"

tmp_dir="$(mktemp -d)"
proof_negative_dir=".amai/continuity/workflow-evidence/.proof-negative-dir"
proof_negative_symlink=".amai/continuity/workflow-evidence/.proof-negative-symlink.json"
trap 'rm -rf "${tmp_dir}" "${proof_negative_dir}" "${proof_negative_symlink}"' EXIT

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

jq '.agent_workflow_guard.workflow_cycle.ordered_stage_codes[0] = "plan"' \
  "${state_file}" >"${tmp_dir}/workflow-cycle-drift.json"
if validate_workflow_guard "${tmp_dir}/workflow-cycle-drift.json" 2>/dev/null; then
  echo "proof_workflow_before_report: workflow cycle drift unexpectedly passed" >&2
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

jq '.startup_execution_gate.blocking = true | .startup_execution_gate.action_kind = "resume_required_return_task"' \
  "${state_file}" >"${tmp_dir}/blocking-startup-gate.json"
if validate_workflow_guard "${tmp_dir}/blocking-startup-gate.json" 2>/dev/null; then
  echo "proof_workflow_before_report: blocking startup gate unexpectedly passed static guard" >&2
  exit 1
fi
if validate_workflow_trace_input "${tmp_dir}/blocking-startup-gate.json" "${signoff_input_file}" 2>/dev/null; then
  echo "proof_workflow_before_report: blocking startup gate unexpectedly passed workflow trace guard" >&2
  exit 1
fi

jq '.startup_execution_gate.must_follow_startup_next_action = false
    | .startup_execution_gate.unrelated_work_allowed = true' \
  "${state_file}" >"${tmp_dir}/startup-gate-unrelated-work-escape.json"
if validate_workflow_guard "${tmp_dir}/startup-gate-unrelated-work-escape.json" 2>/dev/null; then
  echo "proof_workflow_before_report: startup gate unrelated-work escape unexpectedly passed" >&2
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

jq '.workflow_promotion_state.active_workline_headline = "forged active line"
    | .workflow_promotion_state.headline_match = true' \
  "${state_file}" >"${tmp_dir}/promotion-headline-forged-same-ids.json"
if validate_workflow_guard "${tmp_dir}/promotion-headline-forged-same-ids.json" 2>/dev/null; then
  echo "proof_workflow_before_report: forged headline with same ids unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_promotion_state.active_workline_source_kind = "context_pack"
    | .workflow_promotion_state.source_kind_match = true' \
  "${state_file}" >"${tmp_dir}/promotion-source-kind-forged-same-ids.json"
if validate_workflow_guard "${tmp_dir}/promotion-source-kind-forged-same-ids.json" 2>/dev/null; then
  echo "proof_workflow_before_report: forged source_kind with same ids unexpectedly passed" >&2
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

jq 'del(.workflow_execution_trace)' \
  "${signoff_input_file}" >"${tmp_dir}/missing-workflow-trace-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/missing-workflow-trace-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing workflow trace unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_execution_trace.stage_records[2].stage = "implementation"' \
  "${signoff_input_file}" >"${tmp_dir}/stage-order-drift-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/stage-order-drift-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: workflow trace stage order drift unexpectedly passed" >&2
  exit 1
fi

jq 'del(.plan_items[0].goal)' \
  "${signoff_input_file}" >"${tmp_dir}/missing-plan-field-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/missing-plan-field-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing plan field unexpectedly passed" >&2
  exit 1
fi

jq '.required_roles += ["legacy_verification"]' \
  "${signoff_input_file}" >"${tmp_dir}/extra-role-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/extra-role-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: extra role unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_execution_trace.plan_item_reviews[0].team_verification.dimensions = (.workflow_execution_trace.plan_item_reviews[0].team_verification.dimensions[0:9])' \
  "${signoff_input_file}" >"${tmp_dir}/missing-verification-dimension-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/missing-verification-dimension-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing verification dimension unexpectedly passed" >&2
  exit 1
fi

jq '.workflow_execution_trace.final_audit.open_issues = [{"id":"open"}]' \
  "${signoff_input_file}" >"${tmp_dir}/open-final-audit-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/open-final-audit-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: open final audit issue unexpectedly passed" >&2
  exit 1
fi

jq '.proof_bundle.commands[0].evidence_sha256 = "1111111111111111111111111111111111111111111111111111111111111111"' \
  "${signoff_input_file}" >"${tmp_dir}/placeholder-proof-evidence-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/placeholder-proof-evidence-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: placeholder proof evidence unexpectedly passed" >&2
  exit 1
fi

jq '.proof_bundle.commands[0].evidence_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"' \
  "${signoff_input_file}" >"${tmp_dir}/unbacked-proof-evidence-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/unbacked-proof-evidence-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: unbacked proof evidence unexpectedly passed" >&2
  exit 1
fi

jq '.proof_bundle.commands[0].evidence_sha256 = .final_bughunter_pass.evidence_sha256
    | .evidence_manifest.items = [.evidence_manifest.items[] | select(.id != "proof:1")]' \
  "${signoff_input_file}" >"${tmp_dir}/cross-kind-proof-evidence-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/cross-kind-proof-evidence-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: cross-kind proof evidence unexpectedly passed" >&2
  exit 1
fi

jq '.proof_bundle.commands[0].command = "forged command"' \
  "${signoff_input_file}" >"${tmp_dir}/proof-command-metadata-mismatch-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/proof-command-metadata-mismatch-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: proof command metadata mismatch unexpectedly passed" >&2
  exit 1
fi

jq '.evidence_manifest.items[0].sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"' \
  "${signoff_input_file}" >"${tmp_dir}/evidence-file-hash-mismatch-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/evidence-file-hash-mismatch-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: evidence file hash mismatch unexpectedly passed" >&2
  exit 1
fi

jq '.evidence_manifest.items[0].path = ".amai/continuity/workflow-evidence/missing-evidence.json"' \
  "${signoff_input_file}" >"${tmp_dir}/missing-evidence-file-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/missing-evidence-file-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: missing evidence file unexpectedly passed" >&2
  exit 1
fi

mkdir -p "${proof_negative_dir}"
jq '.evidence_manifest.items[0].path = ".amai/continuity/workflow-evidence/.proof-negative-dir"' \
  "${signoff_input_file}" >"${tmp_dir}/directory-evidence-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/directory-evidence-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: directory evidence path unexpectedly passed" >&2
  exit 1
fi

ln -s "$(pwd)/.amai/continuity/specialist-team-signoff-input.json" "${proof_negative_symlink}"
jq '.evidence_manifest.items[0].path = ".amai/continuity/workflow-evidence/.proof-negative-symlink.json"' \
  "${signoff_input_file}" >"${tmp_dir}/symlink-evidence-input.json"
if validate_workflow_trace_input "${state_file}" "${tmp_dir}/symlink-evidence-input.json" 2>/dev/null; then
  echo "proof_workflow_before_report: symlink evidence path unexpectedly passed" >&2
  exit 1
fi

echo "proof_workflow_before_report: ok"
