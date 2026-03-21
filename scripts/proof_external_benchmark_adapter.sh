#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

cargo run --quiet -- benchmark external-datasets
cargo run --quiet -- benchmark external-plan --benchmark vectordbbench
cargo run --quiet -- benchmark external-plan --benchmark ann_benchmarks
cargo run --quiet -- benchmark external-adapter --benchmark ann_benchmarks --dataset dbpedia_openai_1000k_angular
