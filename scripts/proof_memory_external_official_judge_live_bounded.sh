#!/usr/bin/env bash
set -euo pipefail
trap 'echo "proof_memory_external_official_judge_live_bounded.sh failed at line $LINENO" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

LIMIT="${AMAI_EXTERNAL_MEMORY_REAL_LIMIT:-3}"
DATASET="longmemeval_s_cleaned"
OUT_DIR="${AMAI_EXTERNAL_MEMORY_REAL_DIR:-$REPO_ROOT/tmp/external-memory-real-bounded/$DATASET}"
CASES="$OUT_DIR/cases.jsonl"
PREDICTIONS="$OUT_DIR/predictions.jsonl"
EVAL_RESULTS="$OUT_DIR/official-live-eval-results.jsonl"
SUMMARY="$OUT_DIR/official-live-judge-summary.json"
RECONCILE="$OUT_DIR/official-live-score-reconcile.json"
API_KEY_ENV="${AMAI_OFFICIAL_JUDGE_API_KEY_ENV:-OPENAI_API_KEY}"

echo "== Amai external memory official LongMemEval live bounded proof =="

for required_tool in cargo jq wc; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    echo "required tool not found: $required_tool" >&2
    exit 2
  fi
done

if [[ -z "${API_KEY_ENV//[[:space:]]/}" ]]; then
  echo "AMAI_OFFICIAL_JUDGE_API_KEY_ENV must name an environment variable" >&2
  exit 2
fi

if [[ ! -f "$CASES" || ! -f "$PREDICTIONS" ]]; then
  echo "bounded real LongMemEval artifacts missing; materializing via proof_memory_external_real_bounded.sh"
  AMAI_EXTERNAL_MEMORY_REAL_LIMIT="$LIMIT" ./scripts/proof_memory_external_real_bounded.sh
fi

if [[ ! -r "$CASES" || ! -r "$PREDICTIONS" ]]; then
  echo "bounded real LongMemEval artifacts are not readable: $CASES / $PREDICTIONS" >&2
  exit 3
fi

case_count="$(wc -l < "$CASES" | tr -d '[:space:]')"
prediction_count="$(wc -l < "$PREDICTIONS" | tr -d '[:space:]')"
if [[ "$case_count" -eq 0 ]]; then
  echo "bounded real LongMemEval cases are empty: $CASES" >&2
  exit 4
fi
if [[ "$case_count" -ne "$prediction_count" ]]; then
  echo "case/prediction count mismatch: cases=$case_count predictions=$prediction_count" >&2
  exit 5
fi

rm -f "$EVAL_RESULTS" "$SUMMARY" "$RECONCILE"

if [[ -z "${!API_KEY_ENV:-}" ]]; then
  cargo run --quiet -- benchmark external-memory-official-judge \
    --cases "$CASES" \
    --predictions "$PREDICTIONS" \
    --eval-results "$EVAL_RESULTS" \
    --summary "$SUMMARY" \
    --allow-live \
    --api-key-env "$API_KEY_ENV"

  jq -e --argjson case_count "$case_count" --arg api_key_env "$API_KEY_ENV" '
    .boundary_version == "external_memory_official_judge_execution_v1"
    and .bench == "longmemeval"
    and .dataset == "longmemeval_s_cleaned"
    and .status == "blocked"
    and .case_count == $case_count
    and .prediction_count == $case_count
    and .eval_entries_written == 0
    and .allow_live == true
    and .live_official_llm_judge_run == false
    and .official_eval_log_materialized == false
    and .api_key_env == $api_key_env
    and .official_upstream_scorer_parity == false
    and .official_upstream_scorer_parity_boundary.live_log_materialized_by_this_command == false
    and .official_upstream_scorer_parity_boundary.score_reconciliation_run_by_this_command == false
    and (.validation_blocking_reasons | index("official_judge_api_key_not_materialized") != null)
    and (.maturity_blocking_reasons | index("official_judge_api_key_not_materialized") != null)
  ' "$SUMMARY" >/dev/null
  if grep -q 'REDACTED_OFFICIAL_JUDGE_API_KEY' "$SUMMARY"; then
    echo "missing-key summary must not contain redaction marker when no key value was materialized" >&2
    exit 6
  fi
  test ! -e "$EVAL_RESULTS"

  echo "== Done: bounded official judge live proof is fail-closed (missing API key; no eval log materialized) =="
  exit 0
fi

cargo run --quiet -- benchmark external-memory-official-judge \
  --cases "$CASES" \
  --predictions "$PREDICTIONS" \
  --eval-results "$EVAL_RESULTS" \
  --summary "$SUMMARY" \
  --allow-live \
  --api-key-env "$API_KEY_ENV"

jq -e --argjson case_count "$case_count" --arg api_key_env "$API_KEY_ENV" '
  .boundary_version == "external_memory_official_judge_execution_v1"
  and .bench == "longmemeval"
  and .dataset == "longmemeval_s_cleaned"
  and .status == "executed"
  and .case_count == $case_count
  and .prediction_count == $case_count
  and .eval_entries_written == $case_count
  and .allow_live == true
  and .live_official_llm_judge_run == true
  and .official_eval_log_materialized == true
  and .official_prompt_templates_embedded == true
  and .api_key_env == $api_key_env
  and .official_upstream_scorer_parity == false
  and .official_upstream_scorer_parity_boundary.live_log_materialized_by_this_command == true
  and .official_upstream_scorer_parity_boundary.score_reconciliation_run_by_this_command == false
  and (.maturity_blocking_reasons | index("official_upstream_scorer_parity_requires_reconciliation") != null)
' "$SUMMARY" >/dev/null

test -s "$EVAL_RESULTS"
eval_line_count="$(wc -l < "$EVAL_RESULTS" | tr -d '[:space:]')"
if [[ "$eval_line_count" -ne "$case_count" ]]; then
  echo "expected $case_count official eval entries, got $eval_line_count" >&2
  exit 6
fi

jq -s -e --argjson case_count "$case_count" --arg api_key_env "$API_KEY_ENV" '
  length == $case_count
  and all(.[]; (
    (.question_id | type) == "string"
    and (.hypothesis | type) == "string"
    and .autoeval_label.model == "gpt-4o-2024-08-06"
    and ((.autoeval_label.label | type) == "boolean")
    and .official_judge_provenance.provenance_version == "external_memory_official_judge_provenance_v1"
    and .official_judge_provenance.source_kind == "official_longmemeval_llm_judge_execution"
    and .official_judge_provenance.api_key_env == $api_key_env
    and .official_judge_provenance.api_key_value_persisted == false
    and .official_judge_provenance.prompt_template_source_kind == "embedded_from_upstream_evaluate_qa_py"
    and (.official_judge_provenance.prompt_sha256 | test("^[0-9a-f]{64}$"))
  ))
' "$EVAL_RESULTS" >/dev/null

cargo run --quiet -- benchmark external-memory-official-score \
  --cases "$CASES" \
  --eval-results "$EVAL_RESULTS" \
  --output "$RECONCILE"

jq -e --argjson case_count "$case_count" '
  .boundary_version == "external_memory_official_score_reconciliation_v1"
  and .bench == "longmemeval"
  and .dataset == "longmemeval_s_cleaned"
  and .case_count == $case_count
  and .eval_results_present == true
  and .eval_entries_total == $case_count
  and .valid_eval_entries == $case_count
  and .invalid_eval_entries == 0
  and .missing_required_fields == 0
  and .duplicate_question_ids == 0
  and .unexpected_eval_results == 0
  and .missing_case_results == 0
  and .model_mismatch_count == 0
  and .official_upstream_scorer_parity == false
  and .benchmark_grade_maturity == false
  and (
    (
      .all_official_task_types_present == true
      and .status == "reconciled"
      and .official_eval_log_contract_valid == true
      and .official_metrics_reconciled == true
    )
    or
    (
      .all_official_task_types_present == false
      and .status == "blocked"
      and .official_eval_log_contract_valid == false
      and .official_metrics_reconciled == false
      and (.validation_blocking_reasons | index("not_all_official_question_types_present") != null)
    )
  )
' "$RECONCILE" >/dev/null

cargo run --quiet -- benchmark external-memory-secret-scan \
  --output-dir "$OUT_DIR" \
  --secret-env "$API_KEY_ENV" >/dev/null

echo "== Done: bounded official judge live proof produced eval log and reconciled its current bounded contract =="
