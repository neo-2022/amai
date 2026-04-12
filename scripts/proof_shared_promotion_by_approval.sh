#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_shared_promotion_by_approval] $*"
}

extract_skill_card_id() {
  sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p'
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-shared-approval-${suffix}"
proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"

create_and_promote_verified() {
  local skill_id="$1"
  local title="$2"
  local scope_type="$3"
  local source_prefix="$4"

  local create_output
  create_output="$(cargo run --quiet -- skill create-candidate \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --skill-id "${skill_id}" \
    --title "${title}" \
    --goal "Surface procedural memory only through explicit approval policy" \
    --candidate-class "skill_hint" \
    --scope-type "${scope_type}" \
    --owner-scope "project" \
    --trigger-condition "shared procedural approval required" \
    --precondition "continuity state is fresh" \
    --execution-step "inspect shared candidate before execution" \
    --stop-condition "shared candidate is classified" \
    --forbidden-when "continuity state is stale" \
    --expected-outcome "shared contour remains fail-closed until approval" \
    --runtime-constraint "${proof_runtime}" \
    --model-constraint "${proof_model}" \
    --tool-constraint "${proof_tool}" \
    --context-constraint "continuity" \
    --source-event-id "${source_prefix}-candidate" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/candidate" \
    --refinement-action new)"
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
    --source-event-id "${source_prefix}-evidence" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/evidence" >/dev/null

  cargo run --quiet -- skill record-trigger-match \
    --skill-card-id "${skill_card_id}" \
    --match-scope "project_task" \
    --trigger-input "shared procedural approval required" \
    --matched \
    --summary "${title} trigger matched" \
    --source-kind "skill_trigger_scan" \
    --source-event-id "${source_prefix}-trigger" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/trigger" \
    --evidence-span-json "{\"kind\":\"skill_trigger_match\",\"scope_type\":\"${scope_type}\"}" >/dev/null

  cargo run --quiet -- skill record-trial-run \
    --skill-card-id "${skill_card_id}" \
    --application-mode "shadow" \
    --task-label "${source_prefix}-shadow" \
    --context "continuity" \
    --runtime "${proof_runtime}" \
    --model "${proof_model}" \
    --tool "${proof_tool}" \
    --matched \
    --outcome "success" \
    --summary "${title} shadow run succeeded" \
    --source-kind "skill_trial_runtime" \
    --source-event-id "${source_prefix}-shadow" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/shadow" \
    --evidence-span-json "{\"kind\":\"skill_trial_run\",\"phase\":\"shadow\",\"scope_type\":\"${scope_type}\"}" >/dev/null

  cargo run --quiet -- skill record-eval \
    --skill-card-id "${skill_card_id}" \
    --verdict "promote_shadow" \
    --evaluator-source "proof_shared_promotion_by_approval" \
    --safe-to-apply \
    --quality-ok \
    --truth-ok \
    --summary "${title} promoted to shadow" \
    --source-kind "skill_eval_contour" \
    --source-event-id "${source_prefix}-eval-shadow" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/eval-shadow" \
    --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"shadow\",\"scope_type\":\"${scope_type}\"}" >/dev/null

  cargo run --quiet -- skill record-trial-run \
    --skill-card-id "${skill_card_id}" \
    --application-mode "trial" \
    --task-label "${source_prefix}-trial" \
    --context "continuity" \
    --runtime "${proof_runtime}" \
    --model "${proof_model}" \
    --tool "${proof_tool}" \
    --matched \
    --applied \
    --outcome "success" \
    --summary "${title} trial run succeeded" \
    --source-kind "skill_trial_runtime" \
    --source-event-id "${source_prefix}-trial" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/trial" \
    --evidence-span-json "{\"kind\":\"skill_trial_run\",\"phase\":\"trial\",\"scope_type\":\"${scope_type}\"}" >/dev/null

  cargo run --quiet -- skill record-eval \
    --skill-card-id "${skill_card_id}" \
    --verdict "promote_trial" \
    --evaluator-source "proof_shared_promotion_by_approval" \
    --safe-to-apply \
    --quality-ok \
    --truth-ok \
    --summary "${title} promoted to trial" \
    --source-kind "skill_eval_contour" \
    --source-event-id "${source_prefix}-eval-trial" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/eval-trial" \
    --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"trial\",\"scope_type\":\"${scope_type}\"}" >/dev/null

  cargo run --quiet -- skill record-eval \
    --skill-card-id "${skill_card_id}" \
    --verdict "promote_verified" \
    --evaluator-source "proof_shared_promotion_by_approval" \
    --safe-to-apply \
    --quality-ok \
    --truth-ok \
    --summary "${title} promoted to verified" \
    --source-kind "skill_eval_contour" \
    --source-event-id "${source_prefix}-eval-verified" \
    --artifact-ref "artifact://proof/shared-approval/${source_prefix}/eval-verified" \
    --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"verified\",\"scope_type\":\"${scope_type}\"}" >/dev/null

  printf '%s\n' "${skill_card_id}"
}

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

shared_skill_id="proof_shared_skill_${suffix}"
private_skill_id="proof_private_skill_${suffix}"

step "create verified shared and private skills"
shared_skill_card_id="$(create_and_promote_verified \
  "${shared_skill_id}" \
  "Shared approval proof skill" \
  "project_shared" \
  "shared-skill-${suffix}")"
private_skill_card_id="$(create_and_promote_verified \
  "${private_skill_id}" \
  "Private control proof skill" \
  "project_private" \
  "private-skill-${suffix}")"

step "verify shared skill is pending approval after verified promotion"
shared_review_before="$(cargo run --quiet -- skill review --skill-card-id "${shared_skill_card_id}")"
printf '%s\n' "${shared_review_before}" | jq -e '
  .skill.skill_trust_state == "verified" and
  .skill.skill_verification_state == "verified" and
  .skill.skill_scope_type == "project_shared" and
  .skill.skill_shared_promotion_state == "pending_approval" and
  .skill.skill_shared_approved_by == null and
  .skill.skill_shared_approval_reason == null and
  (.evals | map(.verdict)) == ["promote_shadow", "promote_trial", "promote_verified"]
' >/dev/null

step "verify shared skill is absent from execution cards before approval"
cards_before="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}")"
printf '%s\n' "${cards_before}" | jq -e --arg shared_id "${shared_skill_id}" --arg private_id "${private_skill_id}" '
  ([ .[] | select(.skill_id == $shared_id) ] | length) == 0 and
  ([ .[] | select(.skill_id == $private_id) ] | length) == 1
' >/dev/null

step "approve shared promotion explicitly"
cargo run --quiet -- skill record-eval \
  --skill-card-id "${shared_skill_card_id}" \
  --verdict "approve_shared_promotion" \
  --evaluator-source "proof_shared_promotion_by_approval" \
  --safe-to-apply \
  --quality-ok \
  --truth-ok \
  --summary "shared procedural approval granted" \
  --source-kind "skill_eval_contour" \
  --source-event-id "shared-skill-${suffix}-eval-approve" \
  --artifact-ref "artifact://proof/shared-approval/shared-skill-${suffix}/eval-approve" \
  --evidence-span-json "{\"kind\":\"skill_eval\",\"phase\":\"shared_approval\"}" >/dev/null

step "verify shared skill is approved and now surfaced"
shared_review_after="$(cargo run --quiet -- skill review --skill-card-id "${shared_skill_card_id}")"
printf '%s\n' "${shared_review_after}" | jq -e '
  .skill.skill_shared_promotion_state == "approved" and
  .skill.skill_shared_approved_by == "proof_shared_promotion_by_approval" and
  .skill.skill_shared_approval_reason == "shared procedural approval granted" and
  (.evals | map(.verdict)) == ["promote_shadow", "promote_trial", "promote_verified", "approve_shared_promotion"]
' >/dev/null

cards_after="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --context "continuity" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}")"
printf '%s\n' "${cards_after}" | jq -e --arg shared_id "${shared_skill_id}" --arg private_id "${private_skill_id}" '
  ([ .[] | select(.skill_id == $shared_id and .skill_shared_promotion_state == "approved") ] | length) == 1 and
  ([ .[] | select(.skill_id == $private_id and .skill_shared_promotion_state == "not_applicable") ] | length) == 1
' >/dev/null

step "shared promotion by approval proof passed"
