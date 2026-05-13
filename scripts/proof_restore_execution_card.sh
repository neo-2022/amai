#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_restore_execution_card] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-restore-card-${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"
skill_id="proof_restore_execution_card_${suffix}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

step "create compact procedural candidate"
create_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${skill_id}" \
  --title "Restore Continuity Card" \
  --goal "Restore continuity through a compact executable card" \
  --trigger-condition "restore continuity for current step" \
  --precondition "continuity startup gate is fresh" \
  --execution-step "inspect startup gate" \
  --execution-step "confirm startup next action" \
  --stop-condition "current step is restored" \
  --forbidden-when "continuity startup is stale" \
  --expected-outcome "operator receives a compact restore card instead of a long note" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-restore-card-create-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/create/${suffix}" \
  --changed-by "proof_restore_execution_card" \
  --change-reason "materialize compact restore execution card contour")"
skill_card_id="$(printf '%s\n' "${create_output}" | extract_skill_card_id)"
if [[ -z "${skill_card_id}" ]]; then
  echo "failed to parse skill_card_id from create output" >&2
  exit 1
fi

step "record evidence, trigger, shadow, trial and eval lifecycle"
cargo run --quiet -- skill add-evidence \
  --skill-card-id "${skill_card_id}" \
  --evidence-kind "episode_success" \
  --summary "restore execution card evidence" \
  --source-kind "manual_proof" \
  --source-event-id "proof-restore-card-evidence-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/evidence/${suffix}" \
  --evidence-span-json '{"kind":"bundle","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trigger-match \
  --skill-card-id "${skill_card_id}" \
  --match-scope "project_task" \
  --trigger-input "restore continuity for current step" \
  --matched \
  --summary "restore execution card trigger matched" \
  --source-kind "skill_trigger_scan" \
  --source-event-id "proof-restore-card-trigger-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/trigger/${suffix}" \
  --evidence-span-json '{"kind":"skill_trigger_match","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "shadow" \
  --task-label "proof-restore-card-shadow" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --outcome "success" \
  --summary "restore execution card shadow run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-restore-card-shadow-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/shadow/${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"shadow","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_shadow" \
  --evaluator-source "proof_restore_execution_card" \
  --summary "restore execution card promoted to shadow" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-restore-card-eval-shadow-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/eval-shadow/${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","phase":"shadow","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "trial" \
  --task-label "proof-restore-card-trial" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "restore execution card trial run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-restore-card-trial-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/trial/${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"trial","context":"continuity"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_trial" \
  --evaluator-source "proof_restore_execution_card" \
  --summary "restore execution card promoted to trial" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-restore-card-eval-trial-${suffix}" \
  --artifact-ref "artifact://proof/restore-execution-card/eval-trial/${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","phase":"trial","context":"continuity"}' >/dev/null

step "materialize restore pack via continuity handoff"
handoff_details_file="$(mktemp)"
cat >"${handoff_details_file}" <<EOF
Restore continuity safely via compact execution card.
Need a compact executable card for the current step, not a long procedural note.
Current focus: inspect startup gate, then confirm startup next action.
EOF
cargo run --quiet -- continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Restore continuity for current step" \
  --next-step "Inspect startup gate and confirm startup next action with a compact execution card." \
  --details-file "${handoff_details_file}" >/dev/null
rm -f "${handoff_details_file}"

step "run continuity restore with explicit runtime/model/tool binding"
restore_json="$(AMAI_RESTORE_EXECUTION_CARD_RUNTIME="${proof_runtime}" \
  AMAI_RESTORE_EXECUTION_CARD_MODEL="${proof_model}" \
  AMAI_RESTORE_EXECUTION_CARD_TOOL="${proof_tool}" \
  AMAI_ALLOW_EXPENSIVE_TOOL_TURN=1 \
  cargo run --quiet -- continuity restore \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --json)"

step "assert working-state restore carries compact execution card"
printf '%s\n' "${restore_json}" | jq -e --arg model "${proof_model}" --arg runtime "${proof_runtime}" --arg tool "${proof_tool}" '
  .working_state_restore.skill_execution_card.skill_title == "Restore Continuity Card" and
  .working_state_restore.skill_execution_card.skill_trust_state == "trial" and
  .working_state_restore.skill_execution_card.skill_execution_steps[0] == "inspect startup gate" and
  .working_state_restore.skill_execution_card.skill_execution_steps[1] == "confirm startup next action" and
  .working_state_restore.skill_execution_card.binding.model == $model and
  .working_state_restore.skill_execution_card.binding.runtime == $runtime and
  .working_state_restore.skill_execution_card.binding.tool == $tool and
  .working_state_restore.skill_execution_card_summary == "Restore Continuity Card [trial] -> inspect startup gate" and
  .working_state_restore.skill_execution_card_binding.model == $model and
  .working_state_restore.skill_execution_card_binding.runtime == $runtime and
  .working_state_restore.skill_execution_card_binding.tool == $tool
' >/dev/null

step "assert chat-start restore surfaces summary line, not raw procedural sheet"
printf '%s\n' "${restore_json}" | jq -e '
  .chat_start_restore.skill_execution_card_summary == "Restore Continuity Card [trial] -> inspect startup gate" and
  .chat_start_restore.skill_execution_card.skill_title == "Restore Continuity Card" and
  (.chat_start_restore.prompt_text | contains("Карточка: Restore Continuity Card [trial] -> inspect startup gate")) and
  (.chat_start_restore.prompt_text | contains("Need a compact executable card for the current step") | not) and
  (.chat_start_restore.prompt_text | contains("Current focus: inspect startup gate") | not)
' >/dev/null

step "restore execution card proof passed"
