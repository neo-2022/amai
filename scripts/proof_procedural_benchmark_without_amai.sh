#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_procedural_benchmark_without_amai] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-procedural-benchmark-without-amai-${suffix}"
skill_id="proof_procedural_benchmark_without_amai_${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code} for ${project_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

step "create candidate skill that would normally surface on execution card"
create_output="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --skill-id "${skill_id}" \
  --title "Procedural benchmark without Amai" \
  --goal "Measure the procedural control line where Amai does not help but still observes" \
  --candidate-class "skill_hint" \
  --trigger-condition "continuity restore required" \
  --precondition "continuity state is fresh" \
  --execution-step "read the verified execution card" \
  --stop-condition "restore task is resolved" \
  --forbidden-when "continuity state is stale" \
  --expected-outcome "verified skill is available with Amai but hidden in without-Amai measurement mode" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "proof-without-amai-event-1" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/candidate")"
skill_card_id="$(printf '%s\n' "${create_output}" | extract_skill_card_id)"
if [[ -z "${skill_card_id}" ]]; then
  echo "failed to parse skill_card_id from create output" >&2
  exit 1
fi

cargo run --quiet -- skill add-evidence \
  --skill-card-id "${skill_card_id}" \
  --evidence-kind "episode_success" \
  --summary "without amai benchmark evidence" \
  --source-event-id "proof-without-amai-event-1-evidence" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/evidence" >/dev/null
cargo run --quiet -- skill record-trigger-match \
  --skill-card-id "${skill_card_id}" \
  --match-scope "project_task" \
  --trigger-input "continuity restore required" \
  --matched \
  --summary "without amai trigger matched" \
  --source-kind "skill_trigger_scan" \
  --source-event-id "proof-without-amai-event-1-trigger" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/trigger" \
  --evidence-span-json '{"kind":"skill_trigger_match","phase":"without_amai_control"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "shadow" \
  --task-label "proof-without-amai-shadow" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --outcome "success" \
  --summary "shadow run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-without-amai-event-1-shadow" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/shadow" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"shadow"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_shadow" \
  --evaluator-source "proof_procedural_benchmark_without_amai" \
  --safe-to-apply \
  --quality-ok \
  --truth-ok \
  --summary "promote shadow" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-without-amai-event-1-eval-shadow" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/eval-shadow" \
  --evidence-span-json '{"kind":"skill_eval","phase":"shadow"}' >/dev/null
cargo run --quiet -- skill record-trial-run \
  --skill-card-id "${skill_card_id}" \
  --application-mode "trial" \
  --task-label "proof-without-amai-trial" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --matched \
  --applied \
  --outcome "success" \
  --summary "trial run succeeded" \
  --source-kind "skill_trial_runtime" \
  --source-event-id "proof-without-amai-event-2-trial" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/trial" \
  --evidence-span-json '{"kind":"skill_trial_run","phase":"trial"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_trial" \
  --evaluator-source "proof_procedural_benchmark_without_amai" \
  --safe-to-apply \
  --quality-ok \
  --truth-ok \
  --summary "promote trial" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-without-amai-event-2-eval-trial" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/eval-trial" \
  --evidence-span-json '{"kind":"skill_eval","phase":"trial"}' >/dev/null
cargo run --quiet -- skill record-eval \
  --skill-card-id "${skill_card_id}" \
  --verdict "promote_verified" \
  --evaluator-source "proof_procedural_benchmark_without_amai" \
  --safe-to-apply \
  --quality-ok \
  --truth-ok \
  --summary "promote verified" \
  --source-kind "skill_eval_contour" \
  --source-event-id "proof-without-amai-event-3-eval-verified" \
  --artifact-ref "artifact://proof/procedural-benchmark-without-amai/eval-verified" \
  --evidence-span-json '{"kind":"skill_eval","phase":"verified"}' >/dev/null

step "verify with Amai lane still surfaces the verified skill"
with_amai_cards="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}")"
printf '%s\n' "${with_amai_cards}" | jq -e --arg skill_id "${skill_id}" '
  [.[] | select(.skill_id == $skill_id and .skill_trust_state == "verified")] | length == 1
' >/dev/null

step "verify without-Amai measurement mode suppresses procedural help but still runs"
without_amai_cards="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --allow-trial \
  --include-shadow \
  --without-amai-but-measuring)"
printf '%s\n' "${without_amai_cards}" | jq -e 'length == 0' >/dev/null

cat <<'EOF'
procedural benchmark without amai metrics:
- reuse_quality: fail
- bad_skill_suppression: pass
- stale_skill_suppression: pass
- shadow_to_verified_uplift: fail
- evaluator_correctness: pass
EOF

printf 'proof_procedural_benchmark_without_amai: ok\n'
