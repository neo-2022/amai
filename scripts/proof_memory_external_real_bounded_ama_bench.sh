#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

LIMIT="${AMAI_EXTERNAL_MEMORY_REAL_LIMIT:-3}"
BENCH="ama_bench"
DATASET="ama_bench_manual"
NAMESPACE="external_memory_real_bounded_ama_bench"
OUT_DIR="$REPO_ROOT/tmp/external-memory-real-bounded/$DATASET"
CASES="$OUT_DIR/cases.jsonl"
REQUESTS="$OUT_DIR/requests.jsonl"
PREDICTIONS="$OUT_DIR/predictions.jsonl"
STATUS="$OUT_DIR/status.json"
SCORE="$OUT_DIR/score.json"
METRICS="$PREDICTIONS.metrics.json"
CASE_METRICS="$PREDICTIONS.case-metrics.jsonl"

echo "== Amai external memory real bounded AMA-Bench proof =="
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cargo run --quiet -- benchmark external-memory-prepare \
  --benchmark "$BENCH" \
  --dataset "$DATASET" \
  --limit "$LIMIT" \
  --output-dir "$OUT_DIR"

jq -e --argjson limit "$LIMIT" '
  .limit == $limit
  and .stats.total == $limit
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
  .bench == "ama_bench"
  and .dataset == "ama_bench_manual"
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
  and .official_scorer_boundary.benchmark == "ama_bench"
  and .official_scorer_boundary.case_count == $limit
  and .official_scorer_boundary.source_kind == "official_scorer_contract_unavailable"
  and .official_scorer_boundary.requires_live_llm_judge == false
  and .official_scorer_boundary.local_contract_materialized == false
  and .official_scorer_boundary.official_upstream_scorer_parity == false
  and .official_scorer_boundary.benchmark_grade_maturity == false
  and (.official_scorer_boundary.maturity_blocking_reasons | index("official_upstream_scorer_contract_not_materialized_for_benchmark") != null)
  and (.official_scorer_boundary.maturity_blocking_reasons | index("official_upstream_metrics_not_materialized") != null)
  and (.official_scorer_boundary.maturity_blocking_reasons | index("full_dataset_runtime_not_proven_by_this_score") != null)
' "$SCORE" >/dev/null

jq -e --argjson limit "$LIMIT" '
  .total_requests == $limit
  and .completed_cases == $limit
  and .documents_materialized_avg > 0
  and .answer_source_boundary.boundary_version == "external_memory_answer_source_boundary_v1"
  and .answer_source_boundary.evidence_kind == "answer_source_accounting"
  and .answer_source_boundary.semantic_precision_maturity == false
  and .answer_source_boundary.retrieval_hit_cases == $limit
  and .answer_source_boundary.retrieval_answer_cases == $limit
  and .answer_source_boundary.fallback_scan_cases == 0
  and (.answer_source_boundary.maturity_blocking_reasons | index("semantic_relevance_judge_not_integrated") != null)
  and .retrieval_relevance_boundary.boundary_version == "external_memory_retrieval_relevance_boundary_v1"
  and .retrieval_relevance_boundary.evidence_kind == "retrieval_query_overlap_relevance_accounting"
  and .retrieval_relevance_boundary.judge_kind == "query_overlap_proxy"
  and .retrieval_relevance_boundary.semantic_precision_maturity == false
  and .retrieval_relevance_boundary.retrieval_evidence_cases == $limit
  and .retrieval_relevance_boundary.relevant_retrieval_evidence_cases > 0
  and .retrieval_relevance_boundary.relevant_retrieval_evidence_cases <= .retrieval_relevance_boundary.retrieval_evidence_cases
  and (.retrieval_relevance_boundary.no_retrieval_evidence_cases + .retrieval_relevance_boundary.retrieval_evidence_cases == .retrieval_relevance_boundary.judged_cases)
  and (.retrieval_relevance_boundary.maturity_blocking_reasons | index("semantic_relevance_judge_proxy_only") != null)
  and (.retrieval_relevance_boundary.maturity_blocking_reasons | index("gold_labeled_semantic_relevance_not_integrated") != null)
  and .gold_answer_relevance_boundary.boundary_version == "external_memory_gold_answer_relevance_boundary_v1"
  and .gold_answer_relevance_boundary.evidence_kind == "retrieval_gold_answer_support_accounting"
  and .gold_answer_relevance_boundary.judge_kind == "gold_answer_lexical_overlap"
  and .gold_answer_relevance_boundary.label_source_kind == "benchmark_answer_field"
  and .gold_answer_relevance_boundary.semantic_precision_maturity == false
  and .gold_answer_relevance_boundary.gold_labeled_cases == $limit
  and .gold_answer_relevance_boundary.retrieval_evidence_cases == $limit
  and .gold_answer_relevance_boundary.gold_answer_supported_retrieval_cases <= .gold_answer_relevance_boundary.retrieval_evidence_cases
  and (.gold_answer_relevance_boundary.no_retrieval_evidence_cases + .gold_answer_relevance_boundary.retrieval_evidence_cases == .gold_answer_relevance_boundary.judged_cases)
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("gold_answer_overlap_is_lexical_not_semantic") != null)
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("official_upstream_relevance_judge_not_integrated") != null)
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("gold_labeled_semantic_relevance_not_integrated") != null)
' "$METRICS" >/dev/null

case_metric_count="$(wc -l < "$CASE_METRICS" | tr -d '[:space:]')"
if [[ "$case_metric_count" -ne "$LIMIT" ]]; then
  echo "Expected $LIMIT case metrics, got $case_metric_count" >&2
  exit 6
fi

jq -s -e --argjson limit "$LIMIT" '
  length == $limit
  and all(.[]; .chunk_hits > 0 and .document_hits > 0)
  and any(.[]; .relaxed_retrieval_query_used == true)
' "$CASE_METRICS" >/dev/null

echo "== Done: bounded real AMA-Bench runtime+baseline-score proof green (no upstream scorer contract claim) =="
