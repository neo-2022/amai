#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_negative_procedural_memory] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-negative-procedural-${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code} for ${project_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

create_and_promote_verified() {
  local skill_id="$1"
  local title="$2"
  local goal="$3"
  local candidate_class="$4"
  local trigger="$5"
  local step_text="$6"
  local stop_text="$7"
  local forbidden_text="$8"
  local expected_outcome="$9"

  local create_output
  create_output="$(cargo run --quiet -- skill create-candidate \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --skill-id "${skill_id}" \
    --title "${title}" \
    --goal "${goal}" \
    --candidate-class "${candidate_class}" \
    --trigger-condition "${trigger}" \
    --precondition "continuity state is fresh" \
    --execution-step "${step_text}" \
    --stop-condition "${stop_text}" \
    --forbidden-when "${forbidden_text}" \
    --expected-outcome "${expected_outcome}" \
    --runtime-constraint "${proof_runtime}" \
    --model-constraint "${proof_model}" \
    --tool-constraint "${proof_tool}" \
    --context-constraint "continuity" \
    --source-event-id "${skill_id}-event-1" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/candidate")"
  echo "${create_output}" >&2

  local skill_card_id
  skill_card_id="$(printf '%s\n' "${create_output}" | extract_skill_card_id)"
  if [[ -z "${skill_card_id}" ]]; then
    echo "failed to parse skill_card_id for ${skill_id}" >&2
    exit 1
  fi

  cargo run --quiet -- skill add-evidence \
    --skill-card-id "${skill_card_id}" \
    --evidence-kind "episode_success" \
    --summary "${title} evidence" \
    --source-event-id "${skill_id}-event-1-evidence" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/evidence" >/dev/null

  cargo run --quiet -- skill record-trigger-match \
    --skill-card-id "${skill_card_id}" \
    --match-scope "project_task" \
    --trigger-input "${trigger}" \
    --matched \
    --summary "${title} trigger matched" \
    --source-kind "skill_trigger_scan" \
    --source-event-id "${skill_id}-event-1-trigger" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/trigger" \
    --evidence-span-json "{\"kind\":\"skill_trigger_match\",\"candidate_class\":\"${candidate_class}\"}" >/dev/null

  cargo run --quiet -- skill record-trial-run \
    --skill-card-id "${skill_card_id}" \
    --application-mode "shadow" \
    --task-label "${skill_id}-shadow" \
    --context "continuity" \
    --runtime "${proof_runtime}" \
    --model "${proof_model}" \
    --tool "${proof_tool}" \
    --matched \
    --outcome "success" \
    --summary "${title} shadow run succeeded" \
    --source-kind "skill_trial_runtime" \
    --source-event-id "${skill_id}-event-1-shadow" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/shadow" \
    --evidence-span-json "{\"kind\":\"skill_trial_run\",\"phase\":\"shadow\",\"candidate_class\":\"${candidate_class}\"}" >/dev/null

  cargo run --quiet -- skill record-eval \
    --skill-card-id "${skill_card_id}" \
    --verdict "promote_shadow" \
    --evaluator-source "proof_negative_procedural_memory" \
    --summary "${title} promoted to shadow" \
    --source-kind "skill_eval_contour" \
    --source-event-id "${skill_id}-event-1-eval-shadow" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/eval-shadow" \
    --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"shadow\",\"candidate_class\":\"${candidate_class}\"}" >/dev/null

  cargo run --quiet -- skill record-trial-run \
    --skill-card-id "${skill_card_id}" \
    --application-mode "trial" \
    --task-label "${skill_id}-trial" \
    --context "continuity" \
    --runtime "${proof_runtime}" \
    --model "${proof_model}" \
    --tool "${proof_tool}" \
    --matched \
    --applied \
    --outcome "success" \
    --summary "${title} trial run succeeded" \
    --source-kind "skill_trial_runtime" \
    --source-event-id "${skill_id}-event-2-trial" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/trial" \
    --evidence-span-json "{\"kind\":\"skill_trial_run\",\"phase\":\"trial\",\"candidate_class\":\"${candidate_class}\"}" >/dev/null

  cargo run --quiet -- skill record-eval \
    --skill-card-id "${skill_card_id}" \
    --verdict "promote_trial" \
    --evaluator-source "proof_negative_procedural_memory" \
    --summary "${title} promoted to trial" \
    --source-kind "skill_eval_contour" \
    --source-event-id "${skill_id}-event-2-eval-trial" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/eval-trial" \
    --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"trial\",\"candidate_class\":\"${candidate_class}\"}" >/dev/null

  cargo run --quiet -- skill record-eval \
    --skill-card-id "${skill_card_id}" \
    --verdict "promote_verified" \
    --evaluator-source "proof_negative_procedural_memory" \
    --safe-to-apply \
    --quality-ok \
    --truth-ok \
    --summary "${title} promoted to verified" \
    --source-kind "skill_eval_contour" \
    --source-event-id "${skill_id}-event-3-eval-verified" \
    --artifact-ref "artifact://proof/negative-procedural/${skill_id}/eval-verified" \
    --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"verified\",\"candidate_class\":\"${candidate_class}\"}" >/dev/null

  printf '%s\n' "${skill_card_id}"
}

step "create verified success and negative procedural objects"
success_skill_id="proof_success_skill_${suffix}"
failure_pattern_skill_id="proof_failure_pattern_${suffix}"
failure_playbook_skill_id="proof_failure_playbook_${suffix}"
repair_sequence_skill_id="proof_repair_sequence_${suffix}"
anti_pattern_skill_id="proof_anti_pattern_${suffix}"

success_skill_card_id="$(create_and_promote_verified \
  "${success_skill_id}" \
  "Success skill proof" \
  "Surface reusable successful procedure" \
  "skill_hint" \
  "continuity resume success path needed" \
  "apply the known-good resume path" \
  "resume task restored" \
  "continuity state is stale" \
  "resume path succeeds cleanly")"
failure_pattern_skill_card_id="$(create_and_promote_verified \
  "${failure_pattern_skill_id}" \
  "Failure pattern proof" \
  "Surface a verified recurrent failure pattern" \
  "failure_pattern" \
  "continuity restore fails with repeated known signature" \
  "recognize the recurrent failure pattern early" \
  "failure signature is classified" \
  "failure signal is absent" \
  "recurrent failure is identified before unsafe retry")"
failure_playbook_skill_card_id="$(create_and_promote_verified \
  "${failure_playbook_skill_id}" \
  "Failure playbook proof" \
  "Surface a verified failure recovery playbook" \
  "failure_playbook" \
  "continuity resume path fails at restore boundary" \
  "run the failure containment playbook" \
  "failure state is contained" \
  "failure signal is absent" \
  "failure is contained and triaged")"
repair_sequence_skill_card_id="$(create_and_promote_verified \
  "${repair_sequence_skill_id}" \
  "Repair sequence proof" \
  "Surface a verified repair sequence" \
  "repair_sequence" \
  "continuity repair sequence is required" \
  "apply ordered repair steps" \
  "repair sequence completes" \
  "no damage is present" \
  "repair returns system to valid state")"
anti_pattern_skill_card_id="$(create_and_promote_verified \
  "${anti_pattern_skill_id}" \
  "Anti-pattern proof" \
  "Surface a verified anti-pattern warning" \
  "anti_pattern" \
  "continuity path attempts unsafe shortcut" \
  "block the unsafe shortcut and redirect" \
  "unsafe shortcut is avoided" \
  "operator is already on the safe path" \
  "unsafe shortcut is explicitly avoided")"

step "verify negative procedural objects live alongside success skills on execution surface"
cards_json="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}")"
printf '%s\n' "${cards_json}" | jq -e --arg success_id "${success_skill_id}" --arg failure_pattern_id "${failure_pattern_skill_id}" --arg failure_playbook_id "${failure_playbook_skill_id}" --arg repair_sequence_id "${repair_sequence_skill_id}" --arg anti_pattern_id "${anti_pattern_skill_id}" '
  [ .[] | select(
      (.skill_id == $success_id and .skill_candidate_class == "skill_hint") or
      (.skill_id == $failure_pattern_id and .skill_candidate_class == "failure_pattern") or
      (.skill_id == $failure_playbook_id and .skill_candidate_class == "failure_playbook") or
      (.skill_id == $repair_sequence_id and .skill_candidate_class == "repair_sequence") or
      (.skill_id == $anti_pattern_id and .skill_candidate_class == "anti_pattern")
    )
  ] | length == 5
' >/dev/null

step "verify review payload keeps first-class negative procedural identity"
for pair in \
  "${success_skill_card_id}:skill_hint" \
  "${failure_pattern_skill_card_id}:failure_pattern" \
  "${failure_playbook_skill_card_id}:failure_playbook" \
  "${repair_sequence_skill_card_id}:repair_sequence" \
  "${anti_pattern_skill_card_id}:anti_pattern"
do
  skill_card_id="${pair%%:*}"
  expected_class="${pair##*:}"
  review_json="$(cargo run --quiet -- skill review --skill-card-id "${skill_card_id}")"
  printf '%s\n' "${review_json}" | jq -e --arg expected_class "${expected_class}" '
    .skill.skill_trust_state == "verified" and
    .skill.skill_verification_state == "verified" and
    .skill.skill_candidate_class == $expected_class and
    .evidence_count == 1 and
    (.trigger_matches | length) == 1 and
    (.trial_runs | length) == 2 and
    (.evals | map(.verdict)) == ["promote_shadow", "promote_trial", "promote_verified"]
  ' >/dev/null
done

step "negative procedural memory proof passed"
