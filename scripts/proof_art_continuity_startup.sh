#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_art_continuity_migration.sh >/tmp/amai-art-continuity-migration-proof.log

art_repo_root="/home/art/Art"
art_startup_helper="${art_repo_root}/scripts/tools/amai_art_continuity_startup.sh"
startup_state_artifact="${art_repo_root}/.amai/continuity/project-chat-startup-state.json"
startup_output="$(./scripts/continuity_startup.sh --project art --namespace continuity --repo-root "${art_repo_root}" --json)"
helper_json_output="$("${art_startup_helper}" --json)"

reject_reserved_helper_arg() {
  local reserved="$1"
  shift
  local output
  if output="$("${art_startup_helper}" "$@" 2>&1)"; then
    echo "proof_art_continuity_startup: helper unexpectedly accepted reserved arg ${reserved}" >&2
    exit 1
  fi
  printf '%s\n' "$output" | grep -Fq \
    "reserved startup helper argument is managed by amai_art_continuity_startup.sh: ${reserved}"
}

printf '%s\n' "$startup_output" | jq -e '.retrieval_science.suite_key == "continuity_startup"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.verdict_counts.recovered_useful == 4' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[0].name == "startup_summary_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[1].name == "chat_start_restore_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[2].name == "working_state_restore_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[3].name == "workspace_restore_pack_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.prompt_text | contains("CHAT_START_RESTORE")' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.included_reasons_summary != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.excluded_reasons_summary != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.workspace_restore_pack_summary != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.working_state_restore.state_lineage.authoritative_event_id != ""' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.workspace_restore_pack.active_commitments != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.workspace_restore_pack.active_constraints != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.workspace_restore_pack.important_artifacts != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.workspace_restore_pack.procedural_restore_policy.raw_procedural_archive_forbidden == true' >/dev/null
printf '%s\n' "$helper_json_output" | jq -e '.chat_start_restore.prompt_text | contains("CHAT_START_RESTORE")' >/dev/null
printf '%s\n' "$helper_json_output" | jq -e '.chat_start_restore.project.code == "art"' >/dev/null
reject_reserved_helper_arg --project --project amai
reject_reserved_helper_arg --namespace --namespace=foreign
reject_reserved_helper_arg --repo-root --repo-root /tmp/amai-art
reject_reserved_helper_arg --token-source-kind --token-source-kind=proof_collision
test -f "${startup_state_artifact}"
jq -e '.artifact_version == "workspace-startup-runtime-state-v4"' "${startup_state_artifact}" >/dev/null
jq -e '.source_tool == "amai_continuity_startup"' "${startup_state_artifact}" >/dev/null
jq -e '.source_summary_field == "continuity_startup_summary"' "${startup_state_artifact}" >/dev/null
jq -e '.gate_semantics_consistent == true' "${startup_state_artifact}" >/dev/null
jq -e '.startup_execution_gate.gate_version == "startup-execution-gate-v1"' "${startup_state_artifact}" >/dev/null
jq -e '.startup_execution_gate.no_silent_drop == true' "${startup_state_artifact}" >/dev/null
jq -e '.startup_execution_gate.must_read_prompt_text_before_reply == true' "${startup_state_artifact}" >/dev/null
jq -e '.startup_execution_gate.required_action_kind_when_resume_required == "resume_required_return_task"' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.prompt_text_present == true' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.startup_next_action.action_kind != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary | has("required_return_task")' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.project_task_tree != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.project_task_tree_summary != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.project_task_ledger != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.project_task_ledger_summary != null' "${startup_state_artifact}" >/dev/null
startup_state_output="$(./scripts/continuity_startup_state.sh --repo-root "${art_repo_root}" --json)"
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.prompt_text_present == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.startup_next_action_present == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.startup_execution_gate_present == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.must_read_prompt_text_before_reply == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.required_action_kind_when_resume_required == "resume_required_return_task"' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.no_silent_drop == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.gate_semantics_consistent == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.startup_execution_gate.action_kind != null' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state | has("required_return_task")' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.project_task_tree_summary_field_present == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.project_task_ledger_summary_field_present == true' >/dev/null

startup_action_kind="$(printf '%s\n' "$startup_state_output" | jq -r '.startup_runtime_state_audit.action_kind // empty')"
case "$startup_action_kind" in
  rotate_chat_for_client_budget)
    printf '%s\n' "$startup_state_output" | jq -e '.reply_execution_gate.action_kind == "rotate_chat_for_client_budget"' >/dev/null
    printf '%s\n' "$startup_state_output" | jq -e '.reply_execution_gate.must_rotate_before_reply == false' >/dev/null
    printf '%s\n' "$startup_state_output" | jq -e '.reply_execution_gate.blocking == false' >/dev/null
    ;;
  resume_required_return_task|continue_active_workline)
    ;;
  *)
    echo "proof_art_continuity_startup: unexpected startup action kind: ${startup_action_kind}" >&2
    exit 1
    ;;
esac

echo "proof_art_continuity_startup: PASS"
