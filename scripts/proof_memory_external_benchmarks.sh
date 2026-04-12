#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

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

echo "== Done: adapters prepared (manual-run required) =="
