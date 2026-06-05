#!/usr/bin/env bash
set -euo pipefail
trap 'echo "Proof failed at line ${LINENO}: ${BASH_COMMAND}" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Required command not found: $cmd" >&2
    exit 127
  fi
}

require_command cargo
require_command jq

cargo build --release --quiet
AMAI_BIN="./target/release/amai"
if [[ ! -x "$AMAI_BIN" ]]; then
  echo "Expected built binary at $AMAI_BIN" >&2
  exit 127
fi

LIMIT="${AMAI_EXTERNAL_MEMORY_REAL_LIMIT:-3}"
BENCH="memoryagentbench"
DATASET="memoryagentbench_accurate_retrieval"
NAMESPACE="external_memory_real_bounded_memoryagentbench_accurate_retrieval"
OUT_DIR="$REPO_ROOT/tmp/external-memory-real-bounded/$DATASET"
CASES="$OUT_DIR/cases.jsonl"
REQUESTS="$OUT_DIR/requests.jsonl"
PREDICTIONS="$OUT_DIR/predictions.jsonl"
STATUS="$OUT_DIR/status.json"
SCORE="$OUT_DIR/score.json"
METRICS="$PREDICTIONS.metrics.json"
CASE_METRICS="$PREDICTIONS.case-metrics.jsonl"
CASE_METRICS_ARRAY="$OUT_DIR/case-metrics.array.json"
JUDGE_RESULTS="$OUT_DIR/retrieval-semantic-results.jsonl"
JUDGE_SUMMARY="$OUT_DIR/retrieval-semantic-summary.json"
PROOF_CONTRACT="$OUT_DIR/bounded-proof-contract.json"

echo "== Amai external memory bounded MemoryAgentBench accurate_retrieval retrieval-backed blocked-profile proof =="
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

"$AMAI_BIN" benchmark external-memory-prepare \
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

"$AMAI_BIN" benchmark external-memory-run \
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

"$AMAI_BIN" benchmark external-memory-score \
  --cases "$CASES" \
  --predictions "$PREDICTIONS" \
  --output "$SCORE"

jq -e --argjson limit "$LIMIT" '
  .bench == "memoryagentbench"
  and .dataset == "memoryagentbench_accurate_retrieval"
  and .summary.total == $limit
  and .summary.missing_prediction == 0
  and .summary.exact_match == $limit
  and .evidence_boundary.boundary_version == "external_memory_score_evidence_boundary_v1"
  and .evidence_boundary.benchmark_grade_maturity == false
  and .official_scorer_boundary.source_kind == "official_scorer_contract_unavailable"
  and (.capability_breakdown.memoryagentbench_overall_score != null)
  and (.capability_breakdown.memoryagentbench_overall_score == 1)
' "$SCORE" >/dev/null

jq -e --argjson limit "$LIMIT" '
  .total_requests == $limit
  and .completed_cases == $limit
  and .answer_source_boundary.boundary_version == "external_memory_answer_source_boundary_v1"
  and .answer_source_boundary.retrieval_hit_cases == $limit
  and .answer_source_boundary.retrieval_answer_cases >= 0
  and .answer_source_boundary.retrieval_answer_cases <= $limit
  and .answer_source_boundary.fallback_scan_cases >= 0
  and .answer_source_boundary.fallback_scan_cases <= $limit
  and (.answer_source_boundary.retrieval_answer_cases + .answer_source_boundary.fallback_scan_cases >= $limit)
  and (.answer_source_boundary.maturity_blocking_reasons | index("semantic_relevance_judge_not_integrated") != null)
  and .retrieval_relevance_boundary.boundary_version == "external_memory_retrieval_relevance_boundary_v1"
  and .retrieval_relevance_boundary.retrieval_evidence_cases == $limit
  and (.retrieval_relevance_boundary.maturity_blocking_reasons | index("semantic_relevance_judge_proxy_only") != null)
  and .gold_answer_relevance_boundary.boundary_version == "external_memory_gold_answer_relevance_boundary_v1"
  and .gold_answer_relevance_boundary.gold_labeled_cases == $limit
  and (.gold_answer_relevance_boundary.maturity_blocking_reasons | index("gold_answer_overlap_is_lexical_not_semantic") != null)
  and .structural_fact_relevance_boundary.boundary_version == "external_memory_structural_fact_relevance_boundary_v1"
  and .structural_fact_relevance_boundary.judge_kind == "anchored_fact_shape_proxy"
  and .structural_fact_relevance_boundary.proxy_applicable_cases == $limit
  and .structural_fact_relevance_boundary.top_ranked_structural_fact_supported_cases >= 0
  and .structural_fact_relevance_boundary.top_ranked_structural_fact_supported_cases <= $limit
  and (.structural_fact_relevance_boundary.maturity_blocking_reasons | index("structural_fact_proxy_not_semantic_judgment") != null)
  and (.structural_fact_relevance_boundary.maturity_blocking_reasons | index("question_shape_limited_structural_fact_proxy") != null)
  and .benchmark_specific_shaping_boundary.boundary_version == "external_memory_benchmark_specific_shaping_boundary_v1"
  and .benchmark_specific_shaping_boundary.benchmark_specific_shaping_present == false
  and .benchmark_specific_shaping_boundary.generic_runtime_maturity == true
  and .benchmark_specific_shaping_boundary.benchmark_specific_query_override_cases == 0
  and .benchmark_specific_shaping_boundary.benchmark_specific_window_override_cases == 0
  and .benchmark_specific_shaping_boundary.benchmark_specific_answer_extraction_cases == 0
  and (.benchmark_specific_shaping_boundary.maturity_blocking_reasons | length == 0)
  and (.benchmark_specific_shaping_boundary.maturity_blocking_reasons | index("benchmark_specific_runtime_window_override_present") == null)
  and (.benchmark_specific_shaping_boundary.maturity_blocking_reasons | index("benchmark_specific_context_answer_extraction_present") == null)
' "$METRICS" >/dev/null

case_metric_count="$(wc -l < "$CASE_METRICS" | tr -d '[:space:]')"
if [[ "$case_metric_count" -ne "$LIMIT" ]]; then
  echo "Expected $LIMIT case metrics, got $case_metric_count" >&2
  exit 6
fi

jq -s '.' "$CASE_METRICS" > "$CASE_METRICS_ARRAY"

jq -s -e --argjson limit "$LIMIT" '
  length == $limit
  and all(.[]; .document_hits > 0)
  and all(.[]; has("retrieval_payload_top_ranked_relative_path"))
  and all(.[]; has("retrieval_payload_top_ranked_preview"))
  and all(.[]; has("retrieval_payload_top_ranked_gold_answer_supported"))
  and all(.[]; has("retrieval_payload_top_ranked_preview_supports_gold_answer"))
  and all(.[]; has("retrieval_top_ranked_structural_fact_supported"))
  and all(.[]; has("runtime_corpus_sha256"))
  and all(.[]; has("runtime_corpus_reused_from_previous_case"))
  and all(.[]; .retrieval_payload_top_ranked_relative_path != null)
  and all(.[]; .retrieval_payload_top_ranked_preview != null)
  and all(.[]; (.retrieval_payload_top_ranked_gold_answer_supported | type) == "boolean")
  and all(.[]; (.retrieval_gold_answer_top_supported | type) == "boolean")
  and all(.[]; (.retrieval_payload_top_ranked_preview_supports_gold_answer | type) == "boolean")
  and all(.[]; (.retrieval_top_ranked_structural_fact_supported | type) == "boolean")
  and all(.[]; (.runtime_corpus_sha256 | type) == "string" and (.runtime_corpus_sha256 | length) > 0)
  and all(.[]; (.runtime_corpus_reused_from_previous_case | type) == "boolean")
  and all(.[]; .retrieval_payload_top_ranked_gold_answer_supported == .retrieval_gold_answer_top_supported)
  and all(.[]; .retrieval_payload_top_ranked_preview_supports_gold_answer != null)
  and ([.[].runtime_corpus_sha256] | unique | length) == 1
  and ([.[] | select(.runtime_corpus_reused_from_previous_case == true)] | length) == ($limit - 1)
  and all(.[] | select(.runtime_corpus_reused_from_previous_case == true); .stage_ms.index_project_ms == 0)
  and any(.[]; .case_id == "memoryagentbench_accurate_retrieval_1_1"
      and .runtime_corpus_reused_from_previous_case == false)
  and any(.[]; .case_id == "memoryagentbench_accurate_retrieval_1_2"
      and .runtime_corpus_reused_from_previous_case == true)
  and any(.[]; .case_id == "memoryagentbench_accurate_retrieval_1_3"
      and .runtime_corpus_reused_from_previous_case == true)
  and any(.[]; .relaxed_retrieval_query_used == true)
' "$CASE_METRICS" >/dev/null

CASES="$CASES" \
CASE_METRICS="$CASE_METRICS" \
JUDGE_RESULTS="$JUDGE_RESULTS" \
JUDGE_SUMMARY="$JUDGE_SUMMARY" \
./scripts/proof_memory_external_local_semantic_retrieval_judge.sh

jq -e --argjson limit "$LIMIT" '
  .case_count == $limit
  and .runtime_case_metric_count == $limit
  and .judge_results_materialized == true
  and .retrieval_evidence_cases == $limit
  and .judged_cases == $limit
  and .question_relevant_cases >= 0
  and .question_relevant_cases <= $limit
  and .gold_labeled_cases == $limit
  and .gold_answer_supported_cases >= 0
  and .gold_answer_supported_cases <= $limit
  and (.maturity_blocking_reasons | index("non_official_local_semantic_retrieval_judge_lane") != null)
  and (.maturity_blocking_reasons | index("ranked_retrieval_preview_set_only") != null)
' "$JUDGE_SUMMARY" >/dev/null

cargo test --quiet load_requests_jsonl_rejects_missing_bench_identity
cargo test --quiet load_runtime_case_metrics_jsonl_rejects_missing_shaping_flags
cargo test --quiet load_runtime_case_metrics_jsonl_rejects_missing_top_rank_runtime_telemetry
cargo test --quiet runtime_corpus_reuse_allowed_requires_same_hash_and_paths_file

jq -n \
  --arg proof_script "scripts/proof_memory_external_real_bounded_memoryagentbench_accurate_retrieval_blocked.sh" \
  --arg bench "$BENCH" \
  --arg dataset "$DATASET" \
  --argjson limit "$LIMIT" \
  --slurpfile metrics "$METRICS" \
  --slurpfile case_metrics "$CASE_METRICS_ARRAY" \
  --slurpfile semantic "$JUDGE_SUMMARY" \
  --slurpfile score "$SCORE" \
  '{
    artifact_version: "external_memory_bounded_proof_contract_v1",
    proof_script: $proof_script,
    proof_kind: "bounded_real_runtime_score_blocked_profile",
    bench: $bench,
    dataset: $dataset,
    bounded_case_limit: $limit,
    completed_cases: $metrics[0].completed_cases,
    baseline_exact_match: $score[0].summary.exact_match,
    baseline_total: $score[0].summary.total,
    retrieval_answer_cases: $metrics[0].answer_source_boundary.retrieval_answer_cases,
    retrieval_answer_rate: $metrics[0].answer_source_boundary.retrieval_answer_rate,
    fallback_scan_cases: $metrics[0].answer_source_boundary.fallback_scan_cases,
    semantic_judged_cases: $semantic[0].judged_cases,
    semantic_question_relevant_cases: $semantic[0].question_relevant_cases,
    semantic_gold_answer_supported_cases: $semantic[0].gold_answer_supported_cases,
    semantic_all_judged_cases_semantically_relevant: $semantic[0].all_judged_cases_semantically_relevant,
    semantic_all_gold_labeled_judged_cases_answer_supporting: $semantic[0].all_gold_labeled_judged_cases_answer_supporting,
    top_ranked_structural_fact_supported_cases: $metrics[0].structural_fact_relevance_boundary.top_ranked_structural_fact_supported_cases,
    runtime_corpus_unique_sha_count: ([$case_metrics[0][] | .runtime_corpus_sha256] | unique | length),
    runtime_corpus_reused_cases: ([$case_metrics[0][] | select(.runtime_corpus_reused_from_previous_case == true)] | length),
    benchmark_specific_shaping_present: $metrics[0].benchmark_specific_shaping_boundary.benchmark_specific_shaping_present,
    generic_runtime_maturity: $metrics[0].benchmark_specific_shaping_boundary.generic_runtime_maturity,
    semantic_relevance_maturity: $semantic[0].semantic_precision_maturity,
    benchmark_grade_maturity: $score[0].evidence_boundary.benchmark_grade_maturity,
    official_upstream_scorer_parity: $score[0].official_scorer_boundary.official_upstream_scorer_parity,
    interpretation_boundary: {
      strict_bounded_slice_contract: true,
      bounded_slice_limit: $limit,
      bounded_slice_contract_scope: "exact_fixed_case_slice_not_probabilistic_threshold",
      answer_source_rate_interpretation: "retrieval_presence_not_semantic_correctness",
      answer_source_rate_semantic_proof: false,
      semantic_relevance_interpretation: "local_ranked_preview_set_semantic_judge_not_full_retrieval_distribution",
      semantic_judge_boundary_version: $semantic[0].boundary_version,
      semantic_judge_kind: $semantic[0].judge_kind,
      semantic_judge_input_kind: $semantic[0].judged_input_kind,
      structural_fact_proxy_boundary_version: $metrics[0].structural_fact_relevance_boundary.boundary_version,
      structural_fact_proxy_judge_kind: $metrics[0].structural_fact_relevance_boundary.judge_kind,
      structural_fact_proxy_interpretation: "anchored_fact_shape_proxy_not_semantic_judgment",
      runtime_corpus_identity_interpretation: "byte_identical_materialized_runtime_corpus_only_not_full_environment_or_dependency_equivalence",
      bounded_score_interpretation: "baseline_exact_match_only",
      bounded_score_benchmark_grade_proof: false
    },
    latency_boundary: {
      index_project_ms_avg: $metrics[0].index_project_ms.avg,
      total_case_ms_avg: $metrics[0].total_case_ms.avg,
      index_project_share_of_total_case_ms: (
        if ($metrics[0].total_case_ms.avg // 0) == 0 then 0
        else ($metrics[0].index_project_ms.avg / $metrics[0].total_case_ms.avg)
        end
      ),
      runtime_corpus_reused_cases: ([$case_metrics[0][] | select(.runtime_corpus_reused_from_previous_case == true)] | length),
      runtime_cold_index_cases: ([$case_metrics[0][] | select(.runtime_corpus_reused_from_previous_case != true)] | length),
      latency_maturity: false,
      maturity_blocking_reasons: (
        [
          "bounded_profile_not_latency_grade"
        ]
        + (
          if (
            ($metrics[0].total_case_ms.avg // 0) > 0
            and (($metrics[0].index_project_ms.avg / $metrics[0].total_case_ms.avg) > 0.9)
          )
          then ["index_project_ms_dominates_total_case_ms"]
          else []
          end
        )
      )
    },
    maturity_disclaimer: {
      status: "blocked_profile_not_fully_trusted",
      reasons: ([
        $semantic[0].maturity_blocking_reasons,
        $metrics[0].structural_fact_relevance_boundary.maturity_blocking_reasons,
        $metrics[0].benchmark_specific_shaping_boundary.maturity_blocking_reasons,
        $score[0].evidence_boundary.maturity_blocking_reasons
      ] | add | unique)
    }
  }' > "$PROOF_CONTRACT"

jq -e --argjson limit "$LIMIT" '
  .artifact_version == "external_memory_bounded_proof_contract_v1"
  and .proof_kind == "bounded_real_runtime_score_blocked_profile"
  and .bounded_case_limit == $limit
  and .completed_cases == $limit
  and .baseline_exact_match == $limit
  and .baseline_total == $limit
  and .retrieval_answer_cases >= 0
  and .retrieval_answer_cases <= $limit
  and .fallback_scan_cases >= 0
  and .fallback_scan_cases <= $limit
  and .semantic_judged_cases == $limit
  and .semantic_question_relevant_cases >= 0
  and .semantic_question_relevant_cases <= $limit
  and .semantic_gold_answer_supported_cases >= 0
  and .semantic_gold_answer_supported_cases <= $limit
  and .top_ranked_structural_fact_supported_cases >= 0
  and .top_ranked_structural_fact_supported_cases <= $limit
  and .runtime_corpus_unique_sha_count == 1
  and .runtime_corpus_reused_cases == ($limit - 1)
  and .benchmark_specific_shaping_present == false
  and .generic_runtime_maturity == true
  and .semantic_relevance_maturity == false
  and .benchmark_grade_maturity == false
  and .official_upstream_scorer_parity == false
  and .interpretation_boundary.strict_bounded_slice_contract == true
  and .interpretation_boundary.bounded_slice_contract_scope == "exact_fixed_case_slice_not_probabilistic_threshold"
  and .interpretation_boundary.answer_source_rate_semantic_proof == false
  and .interpretation_boundary.semantic_relevance_interpretation == "local_ranked_preview_set_semantic_judge_not_full_retrieval_distribution"
  and .interpretation_boundary.semantic_judge_boundary_version == "external_memory_local_semantic_retrieval_judge_v2"
  and .interpretation_boundary.semantic_judge_kind == "local_ollama_semantic_retrieval_judge"
  and .interpretation_boundary.semantic_judge_input_kind == "ranked_retrieval_preview_set"
  and .interpretation_boundary.structural_fact_proxy_boundary_version == "external_memory_structural_fact_relevance_boundary_v1"
  and .interpretation_boundary.structural_fact_proxy_judge_kind == "anchored_fact_shape_proxy"
  and .interpretation_boundary.structural_fact_proxy_interpretation == "anchored_fact_shape_proxy_not_semantic_judgment"
  and .interpretation_boundary.runtime_corpus_identity_interpretation == "byte_identical_materialized_runtime_corpus_only_not_full_environment_or_dependency_equivalence"
  and .interpretation_boundary.bounded_score_benchmark_grade_proof == false
  and .latency_boundary.latency_maturity == false
  and .latency_boundary.runtime_corpus_reused_cases == ($limit - 1)
  and .latency_boundary.runtime_cold_index_cases == 1
  and .latency_boundary.index_project_share_of_total_case_ms < 0.5
  and (.latency_boundary.maturity_blocking_reasons | index("bounded_profile_not_latency_grade") != null)
  and (.latency_boundary.maturity_blocking_reasons | index("index_project_ms_dominates_total_case_ms") == null)
  and (.maturity_disclaimer.reasons | index("non_official_local_semantic_retrieval_judge_lane") != null)
  and (.maturity_disclaimer.reasons | index("ranked_retrieval_preview_set_only") != null)
  and (.maturity_disclaimer.reasons | index("structural_fact_proxy_not_semantic_judgment") != null)
  and (.maturity_disclaimer.reasons | index("official_upstream_scorer_not_integrated") != null)
' "$PROOF_CONTRACT" >/dev/null

echo "== Done: accurate_retrieval bounded slice stays blocker-visible with local semantic retrieval evidence on the primary proof lane =="
