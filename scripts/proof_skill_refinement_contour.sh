#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_skill_refinement_contour] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-skill-refinement-${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"

base_skill_id="proof_skill_refinement_base_${suffix}"
patch_skill_id="${base_skill_id}"
merge_skill_id="proof_skill_refinement_merge_${suffix}"
new_skill_id="proof_skill_refinement_new_${suffix}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure continuity namespace for ${project_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

step "create base skill"
base_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${base_skill_id}" \
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
  --source-event-id "proof-skill-refinement-base-${suffix}" \
  --artifact-ref "artifact://proof/skill-refinement/base/${suffix}")"
base_skill_card_id="$(printf '%s\n' "${base_output}" | extract_skill_card_id)"
if [[ -z "${base_skill_card_id}" ]]; then
  echo "failed to parse base skill_card_id" >&2
  exit 1
fi

step "verify similar clone is rejected without explicit refinement decision"
if cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "proof_skill_refinement_clone_${suffix}" \
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
  --source-event-id "proof-skill-refinement-clone-${suffix}" \
  --artifact-ref "artifact://proof/skill-refinement/clone/${suffix}" \
  >/tmp/proof_skill_refinement_clone.log 2>&1; then
  echo "similar clone unexpectedly succeeded without refinement decision" >&2
  cat /tmp/proof_skill_refinement_clone.log >&2
  exit 1
fi
rg "similar skill already exists" /tmp/proof_skill_refinement_clone.log >/dev/null

step "verify patch path links to parent"
patch_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${patch_skill_id}" \
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
  --source-event-id "proof-skill-refinement-patch-${suffix}" \
  --artifact-ref "artifact://proof/skill-refinement/patch/${suffix}" \
  --refinement-action patch \
  --patch-parent-skill-card-id "${base_skill_card_id}")"
patch_skill_card_id="$(printf '%s\n' "${patch_output}" | extract_skill_card_id)"
patch_review="$(cargo run --quiet -- skill review --skill-card-id "${patch_skill_card_id}")"
printf '%s\n' "${patch_review}" | jq -e --arg parent_id "${base_skill_card_id}" '
  .skill.skill_patch_parent_id == $parent_id and
  .skill.skill_merge_group_id == $parent_id and
  .skill.skill_id != null and
  .skill.skill_version == 2
' >/dev/null

step "verify merge path groups similar skill instead of spawning ungrouped clone"
merge_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${merge_skill_id}" \
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
  --source-event-id "proof-skill-refinement-merge-${suffix}" \
  --artifact-ref "artifact://proof/skill-refinement/merge/${suffix}" \
  --refinement-action merge)"
merge_skill_card_id="$(printf '%s\n' "${merge_output}" | extract_skill_card_id)"
merge_review="$(cargo run --quiet -- skill review --skill-card-id "${merge_skill_card_id}")"
printf '%s\n' "${merge_review}" | jq -e --arg parent_id "${base_skill_card_id}" '
  .skill.skill_patch_parent_id == null and
  .skill.skill_merge_group_id == $parent_id and
  .skill.skill_evidence_span.skill_refinement_decision.action == "merge"
' >/dev/null

step "verify explicit new path is allowed but marked as conscious divergence"
new_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${new_skill_id}" \
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
  --source-event-id "proof-skill-refinement-new-${suffix}" \
  --artifact-ref "artifact://proof/skill-refinement/new/${suffix}" \
  --refinement-action new)"
new_skill_card_id="$(printf '%s\n' "${new_output}" | extract_skill_card_id)"
new_review="$(cargo run --quiet -- skill review --skill-card-id "${new_skill_card_id}")"
printf '%s\n' "${new_review}" | jq -e '
  .skill.skill_patch_parent_id == null and
  .skill.skill_merge_group_id == null and
  .skill.skill_evidence_span.skill_refinement_decision.action == "new" and
  .skill.skill_evidence_span.skill_refinement_decision.similarity_required_decision == true
' >/dev/null

step "skill refinement contour proof passed"
