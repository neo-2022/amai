#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_workspace_restore_pack_acceptance] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
repo_root="$(pwd)"
suffix="$(amai_unique_suffix)"
namespace_code="proof-workspace-restore-pack-${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"
skill_id="proof_workspace_restore_pack_${suffix}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

step "create procedural card for restore surface"
create_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${skill_id}" \
  --title "Restore Continuity Card" \
  --goal "Surface compact restore execution guidance" \
  --trigger-condition "restore continuity for current step" \
  --precondition "continuity startup gate is fresh" \
  --execution-step "inspect startup gate" \
  --execution-step "confirm startup next action" \
  --stop-condition "current step is restored" \
  --forbidden-when "continuity startup is stale" \
  --expected-outcome "workspace restore pack surfaces compact execution card only" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --context-constraint "restore" \
  --source-event-id "proof-workspace-restore-pack-create-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/create/${suffix}" \
  --changed-by "proof_workspace_restore_pack_acceptance" \
  --change-reason "materialize compact execution card for workspace restore acceptance")"
skill_card_id="$(printf '%s\n' "${create_output}" | extract_skill_card_id)"
if [[ -z "${skill_card_id}" ]]; then
  echo "failed to parse skill_card_id from create output" >&2
  exit 1
fi

step "promote procedural card to trial"
cargo run --quiet -- skill add-evidence \
  --skill-card-id "${skill_card_id}" \
  --evidence-kind "episode_success" \
  --summary "workspace restore acceptance evidence" \
  --source-kind "manual_proof" \
  --source-event-id "proof-workspace-restore-pack-evidence-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/evidence/${suffix}" \
  --evidence-span-json '{"kind":"bundle","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trigger-match \
  --skill-card-id "${skill_card_id}" \
  --match-scope "project_task" \
  --trigger-input "restore continuity for current step" \
  --matched \
  --summary "workspace restore pack trigger matched" \
  --source-kind "skill_trigger_scan" \
  --source-event-id "proof-workspace-restore-pack-trigger-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/trigger/${suffix}" \
  --evidence-span-json '{"kind":"skill_trigger_match","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "shadow" \
  --task-label "proof-workspace-restore-pack-shadow" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --outcome "success" \
  --summary "workspace restore pack shadow run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-workspace-restore-pack-shadow-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/shadow/${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"shadow","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_shadow" \
  --evaluator-source "proof_workspace_restore_pack_acceptance" \
  --summary "workspace restore pack promoted to shadow" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-workspace-restore-pack-eval-shadow-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/eval-shadow/${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","phase":"shadow","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "trial" \
  --task-label "proof-workspace-restore-pack-trial" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "workspace restore pack trial run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-workspace-restore-pack-trial-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/trial/${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"trial","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_trial" \
  --evaluator-source "proof_workspace_restore_pack_acceptance" \
  --summary "workspace restore pack promoted to trial" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-workspace-restore-pack-eval-trial-${suffix}" \
  --artifact-ref "artifact://proof/workspace-restore-pack/eval-trial/${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","phase":"trial","context":"continuity"}' >/dev/null

step "create paused branch and active line"
first_handoff_details="$(mktemp)"
cat >"${first_handoff_details}" <<EOF
Paused branch for workspace restore acceptance.
Waiting for the next live line to preempt this branch.
EOF
cargo run --quiet -- continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Paused branch for workspace restore pack" \
  --next-step "Resume this branch after the active acceptance line is verified." \
  --details-file "${first_handoff_details}" >/dev/null
rm -f "${first_handoff_details}"

second_handoff_details="$(mktemp)"
cat >"${second_handoff_details}" <<EOF
Acceptance line for workspace restore pack.
Need startup and restore to surface a compact execution card, recent episodic traces, constraints, permissions and artifacts.
EOF
cargo run --quiet -- continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Workspace restore pack acceptance line" \
  --next-step "Verify startup and restore surfaces with compact procedural card." \
  --details-file "${second_handoff_details}" >/dev/null
rm -f "${second_handoff_details}"

step "verify live startup and restore surfaces relevant procedures"
startup_json="$(AMAI_RESTORE_EXECUTION_CARD_RUNTIME="${proof_runtime}" \
  AMAI_RESTORE_EXECUTION_CARD_MODEL="${proof_model}" \
  AMAI_RESTORE_EXECUTION_CARD_TOOL="${proof_tool}" \
  cargo run --quiet -- continuity startup \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --repo-root "${repo_root}" \
    --json)"
restore_json="$(AMAI_RESTORE_EXECUTION_CARD_RUNTIME="${proof_runtime}" \
  AMAI_RESTORE_EXECUTION_CARD_MODEL="${proof_model}" \
  AMAI_RESTORE_EXECUTION_CARD_TOOL="${proof_tool}" \
  AMAI_ALLOW_EXPENSIVE_TOOL_TURN=1 \
  cargo run --quiet -- continuity restore \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --json)"

printf '%s\n' "${startup_json}" | jq -e '
  .chat_start_restore.prompt_text | contains("Workspace:")
' >/dev/null
printf '%s\n' "${startup_json}" | jq -e '
  .workspace_restore_pack.active_commitments[0].headline == "Workspace restore pack acceptance line" and
  (.workspace_restore_pack.paused_branches | length) >= 1 and
  (.workspace_restore_pack.recent_episodic_traces | length) >= 2 and
  (.workspace_restore_pack.active_constraints | length) >= 3 and
  .workspace_restore_pack.permission_summary.visible_projects_count == 1 and
  (.workspace_restore_pack.important_artifacts | length) >= 1 and
  (.workspace_restore_pack.relevant_procedures | length) == 1 and
  .workspace_restore_pack.relevant_procedures[0].procedure_kind == "compact_execution_card" and
  .workspace_restore_pack.relevant_procedures[0].raw_procedural_archive_included == false and
  .workspace_restore_pack.procedural_restore_policy.materialized_surface == "compact_execution_card" and
  .workspace_restore_pack.procedural_restore_policy.raw_procedural_archive_forbidden == true and
  (.chat_start_restore.workspace_restore_pack_summary | contains("procedures(1)"))
' >/dev/null

printf '%s\n' "${restore_json}" | jq -e '
  .chat_start_restore.prompt_text | contains("Workspace:")
' >/dev/null
printf '%s\n' "${restore_json}" | jq -e '
  .workspace_restore_pack.active_commitments[0].headline == "Workspace restore pack acceptance line" and
  (.workspace_restore_pack.paused_branches | length) >= 1 and
  (.workspace_restore_pack.recent_episodic_traces | length) >= 2 and
  (.workspace_restore_pack.active_constraints | length) >= 4 and
  .workspace_restore_pack.permission_summary.visible_projects_count == 1 and
  (.workspace_restore_pack.important_artifacts | length) >= 1 and
  (.workspace_restore_pack.relevant_procedures | length) == 1 and
  .workspace_restore_pack.relevant_procedures[0].card.skill_title == "Restore Continuity Card" and
  .workspace_restore_pack.relevant_procedures[0].binding.tool == "'"${proof_tool}"'" and
  .workspace_restore_pack.procedural_restore_policy.materialized_surface == "compact_execution_card" and
  .workspace_restore_pack.procedural_restore_policy.raw_procedural_archive_forbidden == true and
  (.chat_start_restore.workspace_restore_pack_summary | contains("procedures(1)"))
' >/dev/null

step "verify synthetic blocked/waiting acceptance bucket"
cargo test --quiet working_state::tests::build_workspace_restore_pack_surfaces_full_acceptance_buckets -- --exact

step "workspace restore pack acceptance proof passed"
