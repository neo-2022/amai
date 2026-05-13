#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_procedural_seed] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
seed_suffix="$(amai_unique_suffix)"
namespace_code="proof-procedural-seed-${seed_suffix}"
skill_id="proof_procedural_seed_${seed_suffix}"
skill_id_fail="${skill_id}_fail"
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
  --title "Procedural Seed Proof" \
  --goal "Prove candidate shadow trial lifecycle" \
  --trigger-condition "resume gate detected" \
  --precondition "continuity startup is fresh" \
  --execution-step "read startup next action" \
  --stop-condition "required return task restored" \
  --forbidden-when "continuity state is stale" \
  --expected-outcome "resume path restored without drift" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-seed-event-1" \
  --artifact-ref "artifact://proof/procedural-seed/1")"
echo "${create_output}"
skill_card_id="$(printf '%s\n' "${create_output}" | extract_skill_card_id)"
if [[ -z "${skill_card_id}" ]]; then
  echo "failed to parse skill_card_id from create output" >&2
  exit 1
fi

step "verify candidate starts unverified and is hidden from default execution card"
cargo run --quiet -- skill list --project "${project_code}" --namespace "${namespace_code}" \
  | rg "${skill_id}@v1 :: trust=candidate :: verify=unverified" >/dev/null
if cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  | jq -e --arg skill_id "${skill_id}" '.[] | select(.skill_id == $skill_id)' >/dev/null; then
  echo "candidate skill leaked into default execution card" >&2
  exit 1
fi

step "attach evidence and record shadow-mode match/run"
cargo run --quiet -- skill add-evidence \
  --skill-card-id "${skill_card_id}" \
  --evidence-kind "episode_success" \
  --summary "seed evidence" \
  --source-event-id "proof-seed-event-1" \
  --artifact-ref "artifact://proof/procedural-seed/evidence" >/dev/null
cargo run --quiet -- skill record-trigger-match \
  --skill-card-id "${skill_card_id}" \
  --match-scope "project_task" \
  --trigger-input "resume required after continuity startup" \
  --matched \
  --summary "shadow trigger matched" \
  --source-kind "skill_trigger_scan" \
  --source-event-id "proof-seed-event-1-trigger" \
  --artifact-ref "artifact://proof/procedural-seed/trigger" \
  --evidence-span-json '{"kind":"skill_trigger_match","phase":"shadow","basis":"proof"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "shadow" \
  --task-label "proof-shadow" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --outcome "success" \
  --summary "shadow run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-seed-event-1-shadow" \
  --artifact-ref "artifact://proof/procedural-seed/shadow-run" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"shadow","task":"proof-shadow"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_shadow" \
  --evaluator-source "proof_procedural_seed" \
  --summary "evidence present; safe to try in shadow" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-seed-event-1-eval-shadow" \
  --artifact-ref "artifact://proof/procedural-seed/eval-shadow" \
  --evidence-span-json '{"kind":"skill_eval","phase":"shadow","verdict":"promote_shadow"}' >/dev/null

step "verify shadow skill is still hidden by default and visible only with include-shadow"
if cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "codex" \
  --tool "exec_command" \
  | jq -e --arg skill_id "${skill_id}" '.[] | select(.skill_id == $skill_id)' >/dev/null; then
  echo "shadow skill leaked into default execution card" >&2
  exit 1
fi
cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "codex" \
  --model "gpt-5" \
  --tool "exec_command" \
  --include-shadow \
  | jq -e --arg skill_id "${skill_id}" '.[] | select(.skill_id == $skill_id and .skill_trust_state == "shadow")' >/dev/null

step "record trial success and allow trial execution card only when explicitly requested"
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "trial" \
  --task-label "proof-trial" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "trial run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-seed-event-2-trial" \
  --artifact-ref "artifact://proof/procedural-seed/trial-run" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"trial","task":"proof-trial"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_trial" \
  --evaluator-source "proof_procedural_seed" \
  --summary "shadow success observed; trial allowed" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-seed-event-2-eval-trial" \
  --artifact-ref "artifact://proof/procedural-seed/eval-trial" \
  --evidence-span-json '{"kind":"skill_eval","phase":"trial","verdict":"promote_trial"}' >/dev/null
cargo run --quiet -- skill record-reuse \
  --skill-card-id "${skill_card_id}" \
  --reuse-mode "trial" \
  --task-label "proof-trial" \
  --context "continuity" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "trial reuse succeeded" \
  --source-event-id "proof-seed-event-2" \
  --artifact-ref "artifact://proof/procedural-seed/reuse" \
  --evidence-span-json "{\"kind\":\"skill_reuse_log\",\"runtime\":\"${proof_runtime}\",\"model\":\"${proof_model}\",\"tool\":\"${proof_tool}\"}" >/dev/null
if cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  | jq -e --arg skill_id "${skill_id}" '.[] | select(.skill_id == $skill_id)' >/dev/null; then
  echo "trial skill leaked into default execution card without allow-trial" >&2
  exit 1
fi
cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --allow-trial \
  | jq -e --arg skill_id "${skill_id}" '.[] | select(.skill_id == $skill_id and .skill_trust_state == "trial")' >/dev/null

step "verify direct promotion to verified is fail-closed without evidence and successful trial"
fail_output="$(
  cargo run --quiet -- skill create-candidate \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --skill-id "${skill_id_fail}" \
    --title "Premature Verified" \
    --goal "must fail closed" \
    --trigger-condition "manual operator force" \
    --execution-step "do not promote blindly" \
    --source-event-id "proof-seed-fail-event-1" \
    --artifact-ref "artifact://proof/procedural-seed/fail-candidate" \
    2>/dev/null
)"
fail_skill_card_id="$(printf '%s\n' "${fail_output}" | extract_skill_card_id)"
if [[ -z "${fail_skill_card_id}" ]]; then
  echo "failed to parse fail-closed skill_card_id" >&2
  exit 1
fi
if cargo run --quiet -- skill record-eval \
  --skill-card-id "${fail_skill_card_id}" \
  --verdict "promote_verified" \
  --evaluator-source "proof_procedural_seed" \
  --safe-to-apply \
  --quality-ok \
  --truth-ok \
  --summary "should fail without evidence and trial" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-seed-fail-event-1-eval" \
  --artifact-ref "artifact://proof/procedural-seed/fail-eval" \
  --evidence-span-json '{"kind":"skill_eval","phase":"negative","verdict":"promote_verified"}' \
  >/tmp/proof_procedural_seed_fail.log 2>&1; then
  echo "premature verified promotion unexpectedly succeeded" >&2
  cat /tmp/proof_procedural_seed_fail.log >&2
  exit 1
fi
rg "cannot promote to verified without evidence and successful trial run" /tmp/proof_procedural_seed_fail.log >/dev/null

step "procedural seed proof passed"
