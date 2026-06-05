#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

LIMIT="${AMAI_EXTERNAL_MEMORY_REAL_LIMIT:-3}"
BENCH="longmemeval"
DATASET="longmemeval_s_cleaned"
NAMESPACE="external_memory_real_bounded_longmemeval"
OUT_DIR="$REPO_ROOT/tmp/external-memory-real-bounded/$DATASET"
CASES="$OUT_DIR/cases.jsonl"
REQUESTS="$OUT_DIR/requests.jsonl"
PREDICTIONS="$OUT_DIR/predictions.jsonl"
STATUS="$OUT_DIR/status.json"
SCORE="$OUT_DIR/score.json"
METRICS="$PREDICTIONS.metrics.json"
CASE_METRICS="$PREDICTIONS.case-metrics.jsonl"
JUDGE_RESULTS="$OUT_DIR/retrieval-semantic-results.jsonl"
JUDGE_SUMMARY="$OUT_DIR/retrieval-semantic-summary.json"

echo "== Amai external memory real bounded proof =="
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cargo run --quiet -- benchmark external-memory-prepare \
  --benchmark "$BENCH" \
  --dataset "$DATASET" \
  --download-missing \
  --limit "$LIMIT" \
  --output-dir "$OUT_DIR"

jq -e --argjson limit "$LIMIT" '
  .stats.total == $limit
  and .stats.missing_question == 0
  and .stats.missing_context == 0
  and .stats.missing_id == 0
  and .stats.missing_answer == 0
  and .prep_validation.boundary_version == "external_memory_prep_validation_v2"
  and .prep_validation.written_case_count == $limit
  and .prep_validation.normalized_case_contract_valid == true
  and (.prep_validation.validation_blocking_reasons | length == 0)
' "$OUT_DIR/manifest.json" >/dev/null

cargo run --quiet -- benchmark external-memory-run \
  --requests "$REQUESTS" \
  --predictions "$PREDICTIONS" \
  --project amai \
  --namespace "$NAMESPACE" \
  --status "$STATUS"

jq -e --argjson limit "$LIMIT" '
  .stage == "done"
  and .total_requests == $limit
  and .completed == $limit
' "$STATUS" >/dev/null

cargo run --quiet -- benchmark external-memory-score \
  --cases "$CASES" \
  --predictions "$PREDICTIONS" \
  --output "$SCORE"

jq -e --argjson limit "$LIMIT" '
  .bench == "longmemeval"
  and .dataset == "longmemeval_s_cleaned"
  and .summary.total == $limit
  and .summary.missing_prediction == 0
  and .evidence_boundary.boundary_version == "external_memory_score_evidence_boundary_v1"
  and .evidence_boundary.score_kind == "baseline_exact_contains_abstention"
  and .evidence_boundary.official_upstream_scorer_parity == false
  and .evidence_boundary.benchmark_grade_maturity == false
  and (.evidence_boundary.maturity_blocking_reasons | index("baseline_scorer_only") != null)
  and (.evidence_boundary.maturity_blocking_reasons | index("official_upstream_scorer_not_integrated") != null)
  and (.evidence_boundary.maturity_blocking_reasons | index("full_dataset_runtime_not_proven_by_this_score") != null)
  and .official_scorer_boundary.boundary_version == "external_memory_official_scorer_boundary_v1"
  and .official_scorer_boundary.benchmark == "longmemeval"
  and .official_scorer_boundary.case_count == $limit
  and .official_scorer_boundary.source_kind == "official_longmemeval_llm_judge_contract"
  and .official_scorer_boundary.metric_model_short == "gpt-4o"
  and .official_scorer_boundary.metric_model == "gpt-4o-2024-08-06"
  and .official_scorer_boundary.requires_live_llm_judge == true
  and .official_scorer_boundary.local_contract_materialized == true
  and .official_scorer_boundary.official_upstream_scorer_parity == false
  and .official_scorer_boundary.benchmark_grade_maturity == false
  and .official_scorer_boundary.official_prompt_templates_embedded == false
  and (.official_scorer_boundary.maturity_blocking_reasons | index("live_official_llm_judge_not_run") != null)
  and (.official_scorer_boundary.maturity_blocking_reasons | index("official_eval_log_not_materialized") != null)
  and (.official_scorer_boundary.maturity_blocking_reasons | index("official_upstream_metrics_not_materialized") != null)
' "$SCORE" >/dev/null

jq -e --argjson limit "$LIMIT" '
  .total_requests == $limit
  and .completed_cases == $limit
  and .documents_materialized_avg > 0
  and .answer_source_boundary.boundary_version == "external_memory_answer_source_boundary_v1"
  and .answer_source_boundary.evidence_kind == "answer_source_accounting"
  and .answer_source_boundary.semantic_precision_maturity == false
  and .answer_source_boundary.retrieval_hit_cases == $limit
  and .answer_source_boundary.retrieval_answer_cases > 0
  and .answer_source_boundary.fallback_scan_cases < $limit
  and (.answer_source_boundary.maturity_blocking_reasons | index("semantic_relevance_judge_not_integrated") != null)
  and .retrieval_relevance_boundary.boundary_version == "external_memory_retrieval_relevance_boundary_v1"
  and .retrieval_relevance_boundary.evidence_kind == "retrieval_query_overlap_relevance_accounting"
  and .retrieval_relevance_boundary.judge_kind == "query_overlap_proxy"
  and .retrieval_relevance_boundary.semantic_precision_maturity == false
  and (.retrieval_relevance_boundary.no_retrieval_evidence_cases + .retrieval_relevance_boundary.retrieval_evidence_cases == .retrieval_relevance_boundary.judged_cases)
  and (.retrieval_relevance_boundary.maturity_blocking_reasons | index("semantic_relevance_judge_proxy_only") != null)
  and (.retrieval_relevance_boundary.maturity_blocking_reasons | index("gold_labeled_semantic_relevance_not_integrated") != null)
  and .gold_answer_relevance_boundary.boundary_version == "external_memory_gold_answer_relevance_boundary_v1"
  and .gold_answer_relevance_boundary.evidence_kind == "retrieval_gold_answer_support_accounting"
  and .gold_answer_relevance_boundary.judge_kind == "gold_answer_lexical_overlap"
  and .gold_answer_relevance_boundary.label_source_kind == "benchmark_answer_field"
  and .gold_answer_relevance_boundary.semantic_precision_maturity == false
  and .gold_answer_relevance_boundary.gold_labeled_cases > 0
  and (.gold_answer_relevance_boundary.no_retrieval_evidence_cases + .gold_answer_relevance_boundary.retrieval_evidence_cases == .gold_answer_relevance_boundary.judged_cases)
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("gold_answer_overlap_is_lexical_not_semantic") != null)
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("official_upstream_relevance_judge_not_integrated") != null)
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("gold_labeled_semantic_relevance_not_integrated") != null)
' "$METRICS" >/dev/null

if ! jq -e '(.chunk_hits_avg + .document_hits_avg) > 0' "$METRICS" >/dev/null; then
  echo "no_retrieval_evidence: bounded real proof requires chunk_hits_avg + document_hits_avg > 0" >&2
  exit 7
fi

case_metric_count="$(wc -l < "$CASE_METRICS" | tr -d '[:space:]')"
if [[ "$case_metric_count" -ne "$LIMIT" ]]; then
  echo "Expected $LIMIT case metrics, got $case_metric_count" >&2
  exit 6
fi

CASES="$CASES" \
CASE_METRICS="$CASE_METRICS" \
JUDGE_RESULTS="$JUDGE_RESULTS" \
JUDGE_SUMMARY="$JUDGE_SUMMARY" \
./scripts/proof_memory_external_local_semantic_retrieval_judge.sh

jq -e --argjson limit "$LIMIT" '
  .case_count == $limit
  and .runtime_case_metric_count == $limit
  and .judge_results_materialized == true
  and .retrieval_evidence_cases > 0
  and .blocked_cases == 0
  and .judged_cases == .retrieval_evidence_cases
  and .live_local_llm_judge_attempts == .retrieval_evidence_cases
  and .gold_labeled_cases > 0
  and .gold_answer_supported_cases >= 0
  and .gold_answer_supported_cases <= .gold_labeled_judged_cases
  and (.maturity_blocking_reasons | index("non_official_local_semantic_retrieval_judge_lane") != null)
  and (.maturity_blocking_reasons | index("ranked_retrieval_preview_set_only") != null)
' "$JUDGE_SUMMARY" >/dev/null

jq -s -e '
  [ .[] | select((.chunk_hits + .document_hits) > 0 and .relaxed_retrieval_query_used == true)]
  | length > 0
' "$CASE_METRICS" >/dev/null

echo "== Done: bounded real LongMemEval runtime+baseline-score proof green (semantic retrieval evidence now checked through the local non-official judge lane) =="
