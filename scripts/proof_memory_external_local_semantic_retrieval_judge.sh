#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

: "${CASES:?CASES is required}"
: "${CASE_METRICS:?CASE_METRICS is required}"
: "${JUDGE_RESULTS:?JUDGE_RESULTS is required}"
: "${JUDGE_SUMMARY:?JUDGE_SUMMARY is required}"

OLLAMA_BASE_URL="${AMAI_LOCAL_RETRIEVAL_JUDGE_OLLAMA_BASE_URL:-http://127.0.0.1:11434}"
MODEL="${AMAI_LOCAL_RETRIEVAL_JUDGE_MODEL:-gemma4:e4b}"

cargo run --quiet -- benchmark external-memory-local-retrieval-judge \
  --cases "$CASES" \
  --case-metrics "$CASE_METRICS" \
  --judge-results "$JUDGE_RESULTS" \
  --summary "$JUDGE_SUMMARY" \
  --ollama-base-url "$OLLAMA_BASE_URL" \
  --model "$MODEL"

jq -e '
  .boundary_version == "external_memory_local_semantic_retrieval_judge_v2"
  and .judge_kind == "local_ollama_semantic_retrieval_judge"
  and .judged_input_kind == "ranked_retrieval_preview_set"
  and .status == "executed"
  and .judge_results_materialized == true
  and .blocked_cases == 0
  and .ranked_preview_cases == .retrieval_evidence_cases
  and .ranked_preview_items_total >= .judged_cases
  and .legacy_single_preview_fallback_cases == 0
  and .official_upstream_provenance_eligible == false
  and .official_upstream_scorer_parity == false
  and .semantic_precision_maturity == false
  and .benchmark_grade_maturity == false
' "$JUDGE_SUMMARY" >/dev/null
