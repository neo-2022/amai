#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_skill_version_history] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-skill-history-${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"
skill_id="proof_skill_history_${suffix}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

step "create base skill v1 with explicit actor and reason"
base_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${skill_id}" \
  --skill-version 1 \
  --title "Continuity Restore Skill" \
  --goal "Restore continuity safely" \
  --trigger-condition "restore continuity" \
  --precondition "continuity fresh" \
  --execution-step "inspect startup gate" \
  --stop-condition "required return cleared" \
  --forbidden-when "continuity stale" \
  --expected-outcome "resume restored" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-skill-history-base-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/base/${suffix}" \
  --changed-by "seed-evaluator" \
  --change-reason "initial extraction")"
base_skill_card_id="$(printf '%s\n' "${base_output}" | extract_skill_card_id)"
[ -n "${base_skill_card_id}" ]

step "create patch skill v2 with explicit actor and reason"
patch_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${skill_id}" \
  --skill-version 2 \
  --title "Continuity Restore Skill" \
  --goal "Restore continuity safely" \
  --trigger-condition "restore continuity" \
  --precondition "continuity fresh" \
  --execution-step "inspect startup gate" \
  --execution-step "confirm startup next action" \
  --stop-condition "required return cleared" \
  --forbidden-when "continuity stale" \
  --expected-outcome "resume restored" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-skill-history-patch-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/patch/${suffix}" \
  --refinement-action patch \
  --patch-parent-skill-card-id "${base_skill_card_id}" \
  --changed-by "reviewer-1" \
  --change-reason "added explicit startup-next-action confirmation")"
patch_skill_card_id="$(printf '%s\n' "${patch_output}" | extract_skill_card_id)"
[ -n "${patch_skill_card_id}" ]

step "review history must expose actor, reason, and lineage"
review_json="$(cargo run --quiet -- skill review --skill-card-id "${patch_skill_card_id}")"
printf '%s\n' "${review_json}" | jq -e --arg parent_id "${base_skill_card_id}" '
  .history | length == 2 and
  .[0].skill_version == 1 and
  .[0].changed_by == "seed-evaluator" and
  .[0].change_reason == "initial extraction" and
  .[1].skill_version == 2 and
  .[1].changed_by == "reviewer-1" and
  .[1].change_reason == "added explicit startup-next-action confirmation" and
  .[1].refinement_action == "patch" and
  .[1].skill_patch_parent_id == $parent_id
' >/dev/null

step "create merge peer and verify merge-group history"
merge_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "proof_skill_history_merge_${suffix}" \
  --skill-version 1 \
  --title "Continuity Restore Skill" \
  --goal "Restore continuity safely" \
  --trigger-condition "restore continuity" \
  --precondition "continuity fresh" \
  --execution-step "inspect startup gate" \
  --execution-step "confirm startup next action" \
  --stop-condition "required return cleared" \
  --forbidden-when "continuity stale" \
  --expected-outcome "resume restored" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-skill-history-merge-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/merge/${suffix}" \
  --refinement-action merge \
  --changed-by "reviewer-merge" \
  --change-reason "merged overlapping restore variant")"
merge_skill_card_id="$(printf '%s\n' "${merge_output}" | extract_skill_card_id)"
[ -n "${merge_skill_card_id}" ]
merge_review_json="$(cargo run --quiet -- skill review --skill-card-id "${merge_skill_card_id}")"
printf '%s\n' "${merge_review_json}" | jq -e --arg parent_id "${base_skill_card_id}" '
  .history | length == 3 and
  any(.[]; .skill_card_id == $parent_id and .changed_by == "seed-evaluator") and
  any(.[]; .skill_card_id == $parent_id and .skill_version == 1) and
  any(.[]; .changed_by == "reviewer-merge" and .change_reason == "merged overlapping restore variant" and .refinement_action == "merge" and .skill_merge_group_id == $parent_id)
' >/dev/null

step "record lifecycle actions and verify history survives promote/eval/reuse"
cargo run --quiet -- skill add-evidence \
  --skill-card-id "${patch_skill_card_id}" \
  --evidence-kind "trace" \
  --summary "version history lifecycle evidence" \
  --source-kind "manual_proof" \
  --source-event-id "proof-skill-history-evidence-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/evidence/${suffix}" \
  --evidence-span-json '{"kind":"bundle","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trigger-match \
  --skill-card-id "${patch_skill_card_id}" \
  --match-scope "project_task" \
  --trigger-input "restore continuity" \
  --matched \
  --summary "version history trigger matched" \
  --source-kind "manual_trigger" \
  --source-event-id "proof-skill-history-trigger-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/trigger/${suffix}" \
  --evidence-span-json '{"matched":true,"context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${patch_skill_card_id}" \
  --application-mode "shadow" \
  --task-label "proof-history-shadow" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --outcome "success" \
  --summary "history shadow run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-skill-history-shadow-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/shadow/${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"shadow","task":"proof-history-shadow"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${patch_skill_card_id}" \
  --verdict "promote_shadow" \
  --evaluator-source "proof_skill_version_history" \
  --summary "history survived shadow promotion" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-skill-history-eval-shadow-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/eval-shadow/${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","phase":"shadow","verdict":"promote_shadow"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${patch_skill_card_id}" \
  --application-mode "trial" \
  --task-label "proof-history-trial" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "history trial run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-skill-history-trial-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/trial/${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"trial","task":"proof-history-trial"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${patch_skill_card_id}" \
  --verdict "promote_trial" \
  --evaluator-source "proof_skill_version_history" \
  --summary "history survived trial promotion" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-skill-history-eval-trial-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/eval-trial/${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","phase":"trial","verdict":"promote_trial"}' >/dev/null
cargo run --quiet -- skill record-reuse \
  --skill-card-id "${patch_skill_card_id}" \
  --reuse-mode "trial" \
  --task-label "proof-history-trial" \
  --context "continuity" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "history reuse succeeded" \
  --source-event-id "proof-skill-history-reuse-${suffix}" \
  --artifact-ref "artifact://proof/skill-history/reuse/${suffix}" \
  --evidence-span-json "{\"kind\":\"skill_reuse_log\",\"runtime\":\"${proof_runtime}\",\"model\":\"${proof_model}\",\"tool\":\"${proof_tool}\"}" >/dev/null

patch_lifecycle_review_json="$(cargo run --quiet -- skill review --skill-card-id "${patch_skill_card_id}")"
printf '%s\n' "${patch_lifecycle_review_json}" | jq -e --arg parent_id "${base_skill_card_id}" '
  (.history | length) == 3 and
  any(.history[]; .skill_card_id == $parent_id and .skill_version == 1 and .changed_by == "seed-evaluator" and .change_reason == "initial extraction") and
  any(.history[]; .skill_version == 2 and .changed_by == "reviewer-1" and .change_reason == "added explicit startup-next-action confirmation" and .skill_patch_parent_id == $parent_id) and
  any(.history[]; .changed_by == "reviewer-merge" and .refinement_action == "merge" and .skill_merge_group_id == $parent_id) and
  .skill.skill_trust_state == "trial" and
  (.evals | map(.verdict)) == ["promote_shadow", "promote_trial"] and
  (.reuse_logs | map(.reuse_mode)) == ["trial"]
' >/dev/null

step "skill version history proof passed"
