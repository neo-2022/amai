#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

validate_memory_manifest() {
  local manifest="$1"
  local label="$2"
  local allow_missing_answer="${3:-false}"

  if ! jq -e --arg label "$label" --argjson allow_missing_answer "$allow_missing_answer" '
    .stats.total > 0
    and .stats.missing_question == 0
    and .stats.missing_context == 0
    and .stats.missing_id == 0
    and ($allow_missing_answer or .stats.missing_answer == 0)
    and .prep_validation.boundary_version == "external_memory_prep_validation_v2"
    and .prep_validation.written_case_count == .stats.total
    and .prep_validation.normalized_case_contract_valid == true
    and (.prep_validation.validation_blocking_reasons | length == 0)
  ' "$manifest" >/dev/null; then
    echo "External memory normalized cases are not evaluation-ready: $label" >&2
    jq '{benchmark_code, dataset_code, stats, prep_validation}' "$manifest" >&2
    exit 3
  fi
}

validate_runtime_smoke() {
  local status="$1"
  local score="$2"

  if ! jq -e '
    .stage == "done"
    and .total_requests == 1
    and .completed == 1
  ' "$status" >/dev/null; then
    echo "External memory runtime smoke did not complete exactly one request" >&2
    jq '{stage, total_requests, completed, last_case_id}' "$status" >&2
    exit 4
  fi

  if ! jq -e '
    .summary.total == 1
    and .summary.exact_match == 1
    and .summary.missing_prediction == 0
    and .capability_breakdown.longmemeval_overall_accuracy == 1
    and .evidence_boundary.boundary_version == "external_memory_score_evidence_boundary_v1"
    and .evidence_boundary.official_upstream_scorer_parity == false
    and .evidence_boundary.benchmark_grade_maturity == false
    and .official_scorer_boundary.boundary_version == "external_memory_official_scorer_boundary_v1"
    and .official_scorer_boundary.benchmark == "longmemeval"
    and .official_scorer_boundary.case_count == 1
    and .official_scorer_boundary.source_kind == "official_longmemeval_llm_judge_contract"
    and .official_scorer_boundary.metric_model == "gpt-4o-2024-08-06"
    and .official_scorer_boundary.local_contract_materialized == true
    and .official_scorer_boundary.official_upstream_scorer_parity == false
    and .official_scorer_boundary.benchmark_grade_maturity == false
    and (.official_scorer_boundary.maturity_blocking_reasons | index("live_official_llm_judge_not_run") != null)
  ' "$score" >/dev/null; then
    echo "External memory baseline scoring smoke did not produce an exact-match score" >&2
    jq '{bench, dataset, summary, capability_breakdown, evidence_boundary, official_scorer_boundary}' "$score" >&2
    exit 5
  fi
}

echo "== Amai external memory benchmarks: preflight =="
cargo run -- benchmark external-check
cargo run -- benchmark external-datasets

echo "== Download datasets (LongMemEval, MemoryAgentBench, LoCoMo) =="
cargo run -- benchmark external-download --dataset longmemeval_oracle
cargo run -- benchmark external-download --dataset longmemeval_s_cleaned
cargo run -- benchmark external-download --dataset memoryagentbench_accurate_retrieval
cargo run -- benchmark external-download --dataset memoryagentbench_conflict_resolution
cargo run -- benchmark external-download --dataset memoryagentbench_long_range_understanding
cargo run -- benchmark external-download --dataset memoryagentbench_test_time_learning
cargo run -- benchmark external-download --dataset locomo10

echo "== AMA-Bench dataset manual check =="
AMA_DATA="$REPO_ROOT/state/external-benchmarks/datasets/ama-bench.manual"
if [ ! -f "$AMA_DATA" ]; then
  echo "AMA-Bench dataset requires manual install from HF: https://huggingface.co/datasets/AMA-bench/AMA-bench" >&2
  echo "Place a marker file at $AMA_DATA after manual download." >&2
  exit 2
fi

echo "== Prepare adapter workspaces (manual-run contracts) =="
cargo run -- benchmark external-adapter --benchmark longmemeval --dataset longmemeval_s_cleaned --download-missing
cargo run -- benchmark external-adapter --benchmark memoryagentbench --dataset memoryagentbench_accurate_retrieval --download-missing
cargo run -- benchmark external-adapter --benchmark locomo --dataset locomo10 --download-missing
cargo run -- benchmark external-adapter --benchmark ama_bench --dataset ama_bench_manual --download-missing

echo "== Prepare normalized cases for Amai evaluation =="
cargo run -- benchmark external-memory-prepare --benchmark longmemeval --dataset longmemeval_s_cleaned --download-missing
cargo run -- benchmark external-memory-prepare --benchmark memoryagentbench --dataset memoryagentbench_accurate_retrieval --download-missing
cargo run -- benchmark external-memory-prepare --benchmark memoryagentbench --dataset memoryagentbench_conflict_resolution --download-missing
cargo run -- benchmark external-memory-prepare --benchmark memoryagentbench --dataset memoryagentbench_long_range_understanding --download-missing
cargo run -- benchmark external-memory-prepare --benchmark memoryagentbench --dataset memoryagentbench_test_time_learning --download-missing
cargo run -- benchmark external-memory-prepare --benchmark locomo --dataset locomo10 --download-missing
cargo run -- benchmark external-memory-prepare --benchmark ama_bench --dataset ama_bench_manual --download-missing

echo "== Validate normalized case manifests =="
validate_memory_manifest \
  "state/external-benchmarks/memory/longmemeval/longmemeval_s_cleaned/latest/manifest.json" \
  "LongMemEval / longmemeval_s_cleaned" \
  true
validate_memory_manifest \
  "state/external-benchmarks/memory/memoryagentbench/memoryagentbench_accurate_retrieval/latest/manifest.json" \
  "MemoryAgentBench / accurate retrieval"
validate_memory_manifest \
  "state/external-benchmarks/memory/memoryagentbench/memoryagentbench_conflict_resolution/latest/manifest.json" \
  "MemoryAgentBench / conflict resolution"
validate_memory_manifest \
  "state/external-benchmarks/memory/memoryagentbench/memoryagentbench_long_range_understanding/latest/manifest.json" \
  "MemoryAgentBench / long range understanding"
validate_memory_manifest \
  "state/external-benchmarks/memory/memoryagentbench/memoryagentbench_test_time_learning/latest/manifest.json" \
  "MemoryAgentBench / test-time learning"
validate_memory_manifest \
  "state/external-benchmarks/memory/locomo/locomo10/latest/manifest.json" \
  "LoCoMo / locomo10"
validate_memory_manifest \
  "state/external-benchmarks/memory/ama_bench/ama_bench_manual/latest/manifest.json" \
  "AMA-Bench / ama_bench_manual"

echo "== AMA-Bench status =="
echo "AMA-Bench normalized cases and bounded runtime+baseline-score evidence are materialized from the manual HF dataset install; full dataset runtime and benchmark-grade maturity remain separate gaps."

echo "== Synthetic runtime + baseline score smoke =="
SMOKE_DIR="$REPO_ROOT/tmp/external-memory-smoke"
SMOKE_CASES="$SMOKE_DIR/cases.jsonl"
SMOKE_REQUESTS="$SMOKE_DIR/requests.jsonl"
SMOKE_PREDICTIONS="$SMOKE_DIR/predictions.jsonl"
SMOKE_STATUS="$SMOKE_DIR/status.json"
SMOKE_SCORE="$SMOKE_DIR/score.json"
rm -rf "$SMOKE_DIR"
mkdir -p "$SMOKE_DIR"

cat >"$SMOKE_CASES" <<'JSONL'
{"bench":"longmemeval","dataset":"synthetic_external_memory_smoke","case_id":"smoke-001","question":"How long is Alice's commute to the office?","context":"Session 1\nAlice said her commute to the office usually takes 42 minutes each way.","answer":"42 minutes each way","metadata":{"proof_scope":"synthetic_runtime_score_smoke"}}
JSONL
cat >"$SMOKE_REQUESTS" <<'JSONL'
{"case_id":"smoke-001","question":"How long is Alice's commute to the office?","context":"Session 1\nAlice said her commute to the office usually takes 42 minutes each way.","prompt":"Context:\nSession 1\nAlice said her commute to the office usually takes 42 minutes each way.\n\nQuestion: How long is Alice's commute to the office?\nAnswer:"}
JSONL

cargo run -- benchmark external-memory-run \
  --requests "$SMOKE_REQUESTS" \
  --predictions "$SMOKE_PREDICTIONS" \
  --project amai \
  --namespace external_memory_smoke \
  --status "$SMOKE_STATUS"
cargo run -- benchmark external-memory-score \
  --cases "$SMOKE_CASES" \
  --predictions "$SMOKE_PREDICTIONS" \
  --output "$SMOKE_SCORE"
validate_runtime_smoke "$SMOKE_STATUS" "$SMOKE_SCORE"

echo "== Done: external memory prep proof green with synthetic runtime+score smoke (not full scored benchmark maturity) =="
