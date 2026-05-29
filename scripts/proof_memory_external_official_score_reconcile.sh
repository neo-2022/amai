#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/tmp/external-memory-official-score-reconcile"
CASES="$OUT_DIR/cases.jsonl"
EVAL_RESULTS="$OUT_DIR/eval-results.jsonl"
SCORE="$OUT_DIR/official-score.json"
MISSING_SCORE="$OUT_DIR/missing-official-score.json"
INVALID_EVAL_RESULTS="$OUT_DIR/invalid-eval-results.jsonl"
INVALID_SCORE="$OUT_DIR/invalid-official-score.json"

echo "== Amai external memory official score reconcile proof =="
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cat >"$CASES" <<'JSONL'
{"bench":"longmemeval","dataset":"synthetic_official_score_reconcile","case_id":"case-user","question":"Q","answer":"A","metadata":{"question_type":"single-session-user"}}
{"bench":"longmemeval","dataset":"synthetic_official_score_reconcile","case_id":"case-preference","question":"Q","answer":"A","metadata":{"question_type":"single-session-preference"}}
{"bench":"longmemeval","dataset":"synthetic_official_score_reconcile","case_id":"case-assistant","question":"Q","answer":"A","metadata":{"question_type":"single-session-assistant"}}
{"bench":"longmemeval","dataset":"synthetic_official_score_reconcile","case_id":"case-multi","question":"Q","answer":"A","metadata":{"question_type":"multi-session"}}
{"bench":"longmemeval","dataset":"synthetic_official_score_reconcile","case_id":"case-temporal_abs","question":"Q","answer":"A","metadata":{"question_type":"temporal-reasoning"}}
{"bench":"longmemeval","dataset":"synthetic_official_score_reconcile","case_id":"case-knowledge","question":"Q","answer":"A","metadata":{"question_type":"knowledge-update"}}
JSONL

cat >"$EVAL_RESULTS" <<'JSONL'
{"question_id":"case-user","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":true}}
{"question_id":"case-preference","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":false}}
{"question_id":"case-assistant","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":true}}
{"question_id":"case-multi","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":true}}
{"question_id":"case-temporal_abs","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":true}}
{"question_id":"case-knowledge","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":false}}
JSONL

cargo run --quiet -- benchmark external-memory-official-score \
  --cases "$CASES" \
  --eval-results "$EVAL_RESULTS" \
  --output "$SCORE"

jq -e '
  .boundary_version == "external_memory_official_score_reconciliation_v1"
  and .bench == "longmemeval"
  and .dataset == "synthetic_official_score_reconcile"
  and .status == "reconciled"
  and .case_count == 6
  and .eval_results_present == true
  and .valid_eval_entries == 6
  and .official_eval_log_contract_valid == true
  and .official_metrics_reconciled == true
  and .official_upstream_scorer_parity == false
  and .benchmark_grade_maturity == false
  and .required_metric_model == "gpt-4o-2024-08-06"
  and .all_official_task_types_present == true
  and .metrics.overall_accuracy == 0.6667
  and .metrics.task_averaged_accuracy == 0.6667
  and .metrics.abstention_accuracy == 1
  and .metrics.by_question_type."single-session-user".total == 1
  and .metrics.by_question_type."knowledge-update".accuracy == 0
  and (.validation_blocking_reasons | length == 0)
  and (.maturity_blocking_reasons | index("live_official_llm_judge_provenance_not_verified_by_reconciler") != null)
  and (.maturity_blocking_reasons | index("official_prompt_templates_not_embedded") != null)
  and (.maturity_blocking_reasons | index("full_dataset_runtime_not_proven_by_this_score") != null)
' "$SCORE" >/dev/null

cargo run --quiet -- benchmark external-memory-official-score \
  --cases "$CASES" \
  --eval-results "$OUT_DIR/missing.eval-results.jsonl" \
  --output "$MISSING_SCORE"

jq -e '
  .status == "blocked"
  and .eval_results_present == false
  and .official_eval_log_contract_valid == false
  and .official_metrics_reconciled == false
  and .official_upstream_scorer_parity == false
  and (.validation_blocking_reasons | index("official_eval_log_not_materialized") != null)
  and (.validation_blocking_reasons | index("official_upstream_metrics_not_materialized") != null)
' "$MISSING_SCORE" >/dev/null

cat >"$INVALID_EVAL_RESULTS" <<'JSONL'
{"question_id":"case-user","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-mini-2024-07-18","label":true}}
{"question_id":"case-unknown","hypothesis":"ok","autoeval_label":{"model":"gpt-4o-2024-08-06","label":true}}
JSONL

cargo run --quiet -- benchmark external-memory-official-score \
  --cases "$CASES" \
  --eval-results "$INVALID_EVAL_RESULTS" \
  --output "$INVALID_SCORE"

jq -e '
  .status == "blocked"
  and .model_mismatch_count == 1
  and .unexpected_eval_results == 1
  and .missing_case_results == 6
  and .official_eval_log_contract_valid == false
  and .official_metrics_reconciled == false
  and (.validation_blocking_reasons | index("official_eval_log_model_mismatch") != null)
  and (.validation_blocking_reasons | index("official_eval_log_contains_unknown_question_id") != null)
  and (.validation_blocking_reasons | index("official_eval_log_missing_case_results") != null)
' "$INVALID_SCORE" >/dev/null

echo "== Done: official LongMemEval score reconciliation proof green (no live judge parity claim) =="
