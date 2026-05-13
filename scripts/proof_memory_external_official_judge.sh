#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/tmp/external-memory-official-judge"
CASES="$OUT_DIR/cases.jsonl"
PREDICTIONS="$OUT_DIR/predictions.jsonl"
EVAL_RESULTS="$OUT_DIR/eval-results.jsonl"
SUMMARY="$OUT_DIR/official-judge-summary.json"
MISSING_KEY_RESULTS="$OUT_DIR/missing-key-eval-results.jsonl"
MISSING_KEY_SUMMARY="$OUT_DIR/missing-key-summary.json"
MODEL_MISMATCH_RESULTS="$OUT_DIR/model-mismatch-eval-results.jsonl"
MODEL_MISMATCH_SUMMARY="$OUT_DIR/model-mismatch-summary.json"
REDACTION_MARKER="REDACTED_OFFICIAL_JUDGE_API_KEY"

echo "== Amai external memory official LongMemEval judge proof =="
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cat >"$CASES" <<'JSONL'
{"bench":"longmemeval","dataset":"synthetic_official_judge","case_id":"case-user","question":"Where did I buy coffee?","answer":"The corner shop","metadata":{"question_type":"single-session-user"}}
{"bench":"longmemeval","dataset":"synthetic_official_judge","case_id":"case-preference","question":"What style should the reply use?","answer":"Use the user's concise style preference","metadata":{"question_type":"single-session-preference"}}
{"bench":"longmemeval","dataset":"synthetic_official_judge","case_id":"case-assistant","question":"What reminder did the assistant give?","answer":"Submit the report","metadata":{"question_type":"single-session-assistant"}}
{"bench":"longmemeval","dataset":"synthetic_official_judge","case_id":"case-multi","question":"Which cafe appeared across sessions?","answer":"Riverside Cafe","metadata":{"question_type":"multi-session"}}
{"bench":"longmemeval","dataset":"synthetic_official_judge","case_id":"case-temporal_abs","question":"How many days passed?","answer":"The information is incomplete","metadata":{"question_type":"temporal-reasoning"}}
{"bench":"longmemeval","dataset":"synthetic_official_judge","case_id":"case-knowledge","question":"Which updated address should be used?","answer":"The new office address","metadata":{"question_type":"knowledge-update"}}
JSONL

cat >"$PREDICTIONS" <<'JSONL'
{"case_id":"case-user","predicted_answer":"The corner shop."}
{"case_id":"case-preference","predicted_answer":"Use the user's concise style preference."}
{"case_id":"case-assistant","predicted_answer":"Submit the report."}
{"case_id":"case-multi","predicted_answer":"Riverside Cafe."}
{"case_id":"case-temporal_abs","predicted_answer":"INSUFFICIENT_INFO"}
{"case_id":"case-knowledge","predicted_answer":"The new office address."}
JSONL

cargo run --quiet -- benchmark external-memory-official-judge \
  --cases "$CASES" \
  --predictions "$PREDICTIONS" \
  --eval-results "$EVAL_RESULTS" \
  --summary "$SUMMARY"

jq -e '
  .boundary_version == "external_memory_official_judge_execution_v1"
  and .bench == "longmemeval"
  and .dataset == "synthetic_official_judge"
  and .status == "blocked"
  and .case_count == 6
  and .prediction_count == 6
  and .eval_entries_written == 0
  and .allow_live == false
  and .live_official_llm_judge_run == false
  and .official_eval_log_materialized == false
  and .official_prompt_templates_embedded == true
  and .prompt_template_source_kind == "embedded_from_upstream_evaluate_qa_py"
  and .required_metric_model == "gpt-4o-2024-08-06"
  and .metric_model_matches_official == true
  and .official_upstream_scorer_parity == false
  and (.official_upstream_scorer_parity_reason | contains("requires successful live log reconciliation"))
  and .official_upstream_scorer_parity_boundary.score_reconciliation_run_by_this_command == false
  and .official_upstream_scorer_parity_boundary.official_metrics_compared_by_this_command == false
  and .benchmark_grade_maturity == false
  and (.validation_blocking_reasons | index("live_official_llm_judge_not_run") != null)
  and (.validation_blocking_reasons | index("official_eval_log_not_materialized") != null)
  and (.maturity_blocking_reasons | index("official_score_reconciliation_not_run_by_this_command") != null)
' "$SUMMARY" >/dev/null
test ! -e "$EVAL_RESULTS"

env -u AMAI_PROOF_MISSING_OPENAI_KEY cargo run --quiet -- benchmark external-memory-official-judge \
  --cases "$CASES" \
  --predictions "$PREDICTIONS" \
  --eval-results "$MISSING_KEY_RESULTS" \
  --summary "$MISSING_KEY_SUMMARY" \
  --allow-live \
  --api-key-env AMAI_PROOF_MISSING_OPENAI_KEY

jq -e '
  .status == "blocked"
  and .allow_live == true
  and .live_official_llm_judge_run == false
  and .official_eval_log_materialized == false
  and .official_upstream_scorer_parity == false
  and (.validation_blocking_reasons | index("official_judge_api_key_not_materialized") != null)
' "$MISSING_KEY_SUMMARY" >/dev/null
test ! -e "$MISSING_KEY_RESULTS"

cargo run --quiet -- benchmark external-memory-official-judge \
  --cases "$CASES" \
  --predictions "$PREDICTIONS" \
  --eval-results "$MODEL_MISMATCH_RESULTS" \
  --summary "$MODEL_MISMATCH_SUMMARY" \
  --model gpt-4o-mini-2024-07-18

jq -e '
  .status == "blocked"
  and .metric_model_matches_official == false
  and .official_upstream_scorer_parity == false
  and (.validation_blocking_reasons | index("official_judge_model_mismatch") != null)
' "$MODEL_MISMATCH_SUMMARY" >/dev/null
test ! -e "$MODEL_MISMATCH_RESULTS"

for no_key_summary in "$SUMMARY" "$MISSING_KEY_SUMMARY" "$MODEL_MISMATCH_SUMMARY"; do
  if grep -q "$REDACTION_MARKER" "$no_key_summary"; then
    echo "no-key/offline official judge summary must not contain redaction marker: $no_key_summary" >&2
    exit 6
  fi
done

echo "== Done: official LongMemEval judge lane proof green (fail-closed without live API secrets) =="
