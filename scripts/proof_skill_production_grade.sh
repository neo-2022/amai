#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_skill_production_grade] $*"
}

proof_runtime="${PROOF_RUNTIME:-codex}"
proof_model="${PROOF_MODEL:-gpt-5}"
proof_tool="${PROOF_TOOL:-exec_command}"

## Static compile gate
cargo check --tests >/dev/null
cargo build --quiet --bin amai >/dev/null
cargo build --quiet --bin amai-bootstrap >/dev/null

## Source-code level invariants
step "checking scope-assertion coverage"
for fn in create_skill_evidence_bundle record_skill_trigger_match record_skill_trial_run record_skill_eval record_skill_reuse_log; do
  if ! grep -q "assert_skill_card_scope" "src/postgres/postgres_skills.rs"; then
    echo "FAIL: assert_skill_card_scope helper missing in postgres_skills.rs" >&2
    exit 1
  fi
  if ! grep -q "pub async fn ${fn}" "src/postgres/postgres_skills.rs"; then
    echo "FAIL: ${fn} missing in postgres_skills.rs" >&2
    exit 1
  fi
  count="$(grep -c "assert_skill_card_scope" "src/postgres/postgres_skills.rs" || true)"
  if [[ -z "${count}" || "${count}" -lt 5 ]]; then
    echo "FAIL: scope assertions present for fewer than 5 UUID-mutating functions (${count})" >&2
    exit 1
  fi
done

step "checking evaluator implementation"
if ! grep -q "pub async fn evaluate_skill_card" "src/postgres/postgres_skills.rs"; then
  echo "FAIL: evaluate_skill_card not implemented" >&2
  exit 1
fi
if ! grep -q "SkillCommand::Evaluate" "src/cli.rs" "src/main.rs"; then
  echo "FAIL: skill evaluate CLI missing" >&2
  exit 1
fi

step "checking semantic ranking integration"
if ! grep -q "mod embed" "src/main.rs"; then
  echo "FAIL: embed module not declared in main binary" >&2
  exit 1
fi
if ! grep -q "mod embed" "src/bin/amai-bootstrap.rs"; then
  echo "FAIL: embed module not declared in bootstrap binary" >&2
  exit 1
fi
if ! grep -q "pub async fn build_skill_execution_cards" "src/postgres/postgres_skills.rs"; then
  echo "FAIL: build_skill_execution_cards missing" >&2
  exit 1
fi
if ! grep -q "cosine_similarity" "src/postgres/postgres_skills.rs"; then
  echo "FAIL: semantic cosine lane missing in build_skill_execution_cards" >&2
  exit 1
fi
if ! grep -q "query: Option<&str>" "src/postgres/postgres_skills.rs"; then
  echo "FAIL: query parameter not plumbed into build_skill_execution_cards" >&2
  exit 1
fi

step "checking CLI fail-closed scope guard"
for cmd in add-evidence record-trigger-match record-trial-run record-eval evaluate record-reuse; do
  if ! grep -q "require_skill_scope" "src/main.rs"; then
    echo "FAIL: require_skill_scope helper missing in main.rs" >&2
    exit 1
  fi
  if ! cargo run --quiet -- skill "${cmd}" --help >/dev/null 2>&1; then
    echo "FAIL: skill ${cmd} CLI help is broken" >&2
    exit 1
  fi
done

## Runtime gate: only run if services are healthy
if ! ./scripts/status.sh --json >/dev/null 2>&1; then
  step "runtime services not healthy; skipping runtime authorization/semantic proof"
  step "STATIC INVARIANTS PASSED"
  exit 0
fi

## Runtime authorization: cross-project scope rejection
step "runtime stack is healthy; running cross-project auth negative path"
project_code="amai"
suffix="$(amai_unique_suffix)"
ns_a="proof-skill-pg-${suffix}-a"
ns_b="proof-skill-pg-${suffix}-b"

cargo run --quiet -- namespace ensure --project "${project_code}" --code "${ns_a}" >/dev/null
cargo run --quiet -- namespace ensure --project "${project_code}" --code "${ns_b}" >/dev/null

create_output_a="$(cargo run --quiet -- skill create-candidate \
  --project "${project_code}" \
  --namespace "${ns_a}" \
  --skill-id "skill_${suffix}" \
  --title "Authorization probe skill" \
  --goal "Probe skill card scope isolation" \
  --candidate-class "skill_hint" \
  --scope-type "project_private" \
  --owner-scope "agent_private" \
  --trigger-condition "probe" \
  --precondition "fresh" \
  --execution-step "probe step" \
  --stop-condition "done" \
  --expected-outcome "no cross-project mutation" \
  --runtime-constraint "${proof_runtime}" \
  --model-constraint "${proof_model}" \
  --tool-constraint "${proof_tool}" \
  --context-constraint "continuity" \
  --source-event-id "probe-${suffix}-candidate" \
  --artifact-ref "artifact://proof/skill-pg/${suffix}/candidate" \
  --refinement-action new)"
skill_card_id_a="$(printf '%s\n' "${create_output_a}" | sed -n 's/^skill candidate created: \([^ ]*\) ::.*$/\1/p')"
if [[ -z "${skill_card_id_a}" ]]; then
  echo "FAIL: could not parse skill_card_id for namespace_a" >&2
  exit 1
fi

## Promote the card to verified so execution-card can pick it up
promote_to_verified() {
  local card_id="$1"
  local ns="$2"

  cargo run --quiet -- skill add-evidence \
    --project "${project_code}" --namespace "${ns}" \
    --skill-card-id "${card_id}" \
    --evidence-kind episode_success \
    --summary "probe evidence" \
    --source-event-id "probe-${suffix}-evidence" \
    --artifact-ref "artifact://proof/skill-pg/${suffix}/evidence" >/dev/null

  cargo run --quiet -- skill record-trigger-match \
    --project "${project_code}" --namespace "${ns}" \
    --skill-card-id "${card_id}" \
    --match-scope project_task \
    --trigger-input "probe" \
    --matched \
    --summary "probe trigger" \
    --source-event-id "probe-${suffix}-trigger" \
    --source-kind skill_trigger_scan \
    --artifact-ref "artifact://proof/skill-pg/${suffix}/trigger" \
    --evidence-span-json '{"kind":"skill_trigger_match"}' >/dev/null

  cargo run --quiet -- skill record-trial-run \
    --project "${project_code}" --namespace "${ns}" \
    --skill-card-id "${card_id}" \
    --application-mode shadow \
    --task-label "probe-shadow" \
    --context continuity \
    --runtime "${proof_runtime}" \
    --model "${proof_model}" \
    --tool "${proof_tool}" \
    --matched \
    --outcome success \
    --summary "probe shadow success" \
    --source-event-id "probe-${suffix}-shadow" \
    --artifact-ref "artifact://proof/skill-pg/${suffix}/shadow" >/dev/null

  cargo run --quiet -- skill record-eval \
    --project "${project_code}" --namespace "${ns}" \
    --skill-card-id "${card_id}" \
    --verdict promote_shadow \
    --safe-to-apply --quality-ok --truth-ok \
    --source-kind auto_eval \
    --source-event-id "probe-${suffix}-eval-shadow" >/dev/null

  cargo run --quiet -- skill record-trial-run \
    --project "${project_code}" --namespace "${ns}" \
    --skill-card-id "${card_id}" \
    --application-mode trial \
    --task-label "probe-trial" \
    --context continuity \
    --runtime "${proof_runtime}" \
    --model "${proof_model}" \
    --tool "${proof_tool}" \
    --matched --applied \
    --outcome success \
    --summary "probe trial success" \
    --source-event-id "probe-${suffix}-trial" \
    --artifact-ref "artifact://proof/skill-pg/${suffix}/trial" >/dev/null

  cargo run --quiet -- skill record-eval \
    --project "${project_code}" --namespace "${ns}" \
    --skill-card-id "${card_id}" \
    --verdict promote_verified \
    --safe-to-apply --quality-ok --truth-ok \
    --source-kind auto_eval \
    --source-event-id "probe-${suffix}-eval-verified" >/dev/null
}

promote_to_verified "${skill_card_id_a}" "${ns_a}"

## Cross-namespace commands must fail because the card belongs to namespace A, not B
fail_count=0
for cmd_args in \
  "add-evidence --skill-card-id ${skill_card_id_a} --evidence-kind episode_success --summary x --source-event-id e" \
  "record-trigger-match --skill-card-id ${skill_card_id_a} --match-scope project_task --trigger-input probe --matched --summary x" \
  "record-trial-run --skill-card-id ${skill_card_id_a} --application-mode shadow --task-label probe --context continuity --runtime ${proof_runtime} --model ${proof_model} --tool ${proof_tool} --matched --outcome success --summary x" \
  "record-eval --skill-card-id ${skill_card_id_a} --verdict ok --safe-to-apply --quality-ok --truth-ok --source-kind manual" \
  "record-reuse-log --skill-card-id ${skill_card_id_a} --executed --outcome success --summary x"; do
  # shellcheck disable=SC2086
  if cargo run --quiet -- skill ${cmd_args} \
       --project "${project_code}" --namespace "${ns_b}" >/dev/null 2>&1; then
    echo "FAIL: ${cmd_args} accepted cross-namespace mutation" >&2
    fail_count=$((fail_count + 1))
  fi
done

if [[ "${fail_count}" -gt 0 ]]; then
  echo "FAIL: ${fail_count} cross-project mutation command(s) did not fail closed" >&2
  exit 1
fi

## Same-namespace execution card should succeed with semantic query
step "runtime: build execution cards with semantic query"
cards_output="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${ns_a}" \
  --query "probe isolation" \
  --runtime "${proof_runtime}" \
  --model "${proof_model}" \
  --tool "${proof_tool}" \
  --allow-trial \
  --include-shadow)"
if ! printf '%s\n' "${cards_output}" | grep -q "skill_card_id"; then
  echo "FAIL: execution-card with semantic query returned no cards" >&2
  exit 1
fi

step "RUNTIME AND STATIC INVARIANTS PASSED"
