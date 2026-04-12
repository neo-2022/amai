#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_procedural_shadow_review] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

repo_root="$(pwd)"
dsn="${AMI_POSTGRES_DSN}"
project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-procedural-shadow-review-${suffix}"
skill_id="proof_procedural_shadow_review_${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code} for ${project_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

step "create candidate skill"
create_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${skill_id}" \
  --title "Procedural Shadow Review" \
  --goal "Review shadow/trial evaluator lifecycle from truth tables" \
  --trigger-condition "continuity startup requests restore" \
  --precondition "continuity state is fresh" \
  --execution-step "inspect startup gate and resume task" \
  --stop-condition "resume task is restored" \
  --forbidden-when "continuity startup is stale" \
  --expected-outcome "candidate is reviewed through shadow and trial without leaking into default execution" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-shadow-review-event-1" \
  --artifact-ref "artifact://proof/procedural-shadow-review/1")"
echo "${create_output}"
skill_card_id="$(printf '%s\n' "${create_output}" | extract_skill_card_id)"
if [[ -z "${skill_card_id}" ]]; then
  echo "failed to parse skill_card_id from create output" >&2
  exit 1
fi

step "record evidence, shadow, trial and evaluator steps"
cargo run --quiet -- skill add-evidence \
  --skill-card-id "${skill_card_id}" \
  --evidence-kind "episode_success" \
  --summary "shadow review evidence" \
  --source-event-id "proof-shadow-review-event-1" \
  --artifact-ref "artifact://proof/procedural-shadow-review/evidence" >/dev/null
cargo run --quiet -- skill record-trigger-match \
  --skill-card-id "${skill_card_id}" \
  --match-scope "manual_review" \
  --trigger-input "continuity startup restore required" \
  --matched \
  --summary "manual shadow review trigger matched" \
  --source-kind "skill_trigger_scan" \
  --source-event-id "proof-shadow-review-event-1-trigger" \
  --artifact-ref "artifact://proof/procedural-shadow-review/trigger" \
  --evidence-span-json '{"kind":"skill_trigger_match","phase":"shadow_review","basis":"proof"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "shadow" \
  --task-label "proof-shadow-review-shadow" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --outcome "success" \
  --summary "shadow review run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-shadow-review-event-1-shadow" \
  --artifact-ref "artifact://proof/procedural-shadow-review/shadow-run" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"shadow","task":"proof-shadow-review-shadow"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_shadow" \
  --evaluator-source "proof_procedural_shadow_review" \
  --summary "candidate is safe for shadow review" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-shadow-review-event-1-eval-shadow" \
  --artifact-ref "artifact://proof/procedural-shadow-review/eval-shadow" \
  --evidence-span-json '{"kind":"skill_eval","phase":"shadow","verdict":"promote_shadow"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "trial" \
  --task-label "proof-shadow-review-trial" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "trial review run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-shadow-review-event-2-trial" \
  --artifact-ref "artifact://proof/procedural-shadow-review/trial-run" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"trial","task":"proof-shadow-review-trial"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_trial" \
  --evaluator-source "proof_procedural_shadow_review" \
  --summary "shadow review succeeded; trial remains explicit-only" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-shadow-review-event-2-eval-trial" \
  --artifact-ref "artifact://proof/procedural-shadow-review/eval-trial" \
  --evidence-span-json '{"kind":"skill_eval","phase":"trial","verdict":"promote_trial"}' >/dev/null
cargo run --quiet -- skill record-reuse \
  --skill-card-id "${skill_card_id}" \
  --reuse-mode "trial" \
  --task-label "proof-shadow-review-trial" \
  --context "continuity" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "trial reuse logged for review" \
  --source-event-id "proof-shadow-review-event-2" \
  --artifact-ref "artifact://proof/procedural-shadow-review/reuse" \
  --evidence-span-json "{\"kind\":\"skill_reuse_log\",\"runtime\":\"${proof_runtime}\",\"model\":\"${proof_model}\",\"tool\":\"${proof_tool}\"}" >/dev/null

step "verify execution-card visibility policy"
if cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  | jq -e --arg skill_id "${skill_id}" '.[] | select(.skill_id == $skill_id)' >/dev/null; then
  echo "trial skill leaked into default execution card during shadow review" >&2
  exit 1
fi

cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --allow-trial \
  | jq -e --arg skill_id "${skill_id}" '
      .[] | select(
        .skill_id == $skill_id and
        .skill_trust_state == "trial" and
        (.skill_trigger_conditions | index("continuity startup requests restore")) != null and
        .skill_scope_type == "project_private" and
        .skill_owner_scope == "project"
      )' >/dev/null

step "materialize truth-table review payload"
review_json="$(cargo run --quiet -- skill review --skill-card-id "${skill_card_id}")"
printf '%s\n' "${review_json}"

step "assert truth-table review matches expected lifecycle"
printf '%s\n' "${review_json}" | jq -e '
  .skill.skill_trust_state == "trial" and
  .skill.skill_verification_state == "trial_ready" and
  .evidence_count == 1 and
  .skill.skill_shadow_pass_count == 1 and
  .skill.skill_shadow_fail_count == 0 and
  .skill.skill_reuse_count == 1 and
  (.trigger_matches | length) == 1 and
  (.trial_runs | length) == 2 and
  (.evals | map(.verdict)) == ["promote_shadow", "promote_trial"] and
  (.reuse_logs | map(.reuse_mode)) == ["trial"]
' >/dev/null

step "procedural shadow review proof passed"
