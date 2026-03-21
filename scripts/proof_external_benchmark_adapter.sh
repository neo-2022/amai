#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

cargo run --quiet -- benchmark external-datasets
cargo run --quiet -- benchmark external-plan --benchmark vectordbbench
cargo run --quiet -- benchmark external-plan --benchmark ann_benchmarks

ann_output="$(cargo run --quiet -- benchmark external-adapter --benchmark ann_benchmarks --dataset dbpedia_openai_1000k_angular)"
printf '%s\n' "$ann_output"
grep -F "Статус: готов к следующему запуску" <<<"$ann_output" >/dev/null

unsupported_output="$(cargo run --quiet -- benchmark external-adapter --benchmark ann_benchmarks --dataset sphere_10m_meta_dpr)"
printf '%s\n' "$unsupported_output"
grep -F "Статус: dataset пока не поддержан этим benchmark-ом" <<<"$unsupported_output" >/dev/null

run_script="state/external-benchmarks/runs/ann_benchmarks/dbpedia_openai_1000k_angular/latest/run_external.sh"
grep -F "python3 -m venv .venv" "$run_script" >/dev/null
grep -F "./.venv/bin/python install.py --algorithm qdrant" "$run_script" >/dev/null
grep -F "./.venv/bin/python run.py --dataset dbpedia-openai-1000k-angular --algorithm qdrant --runs 1 --parallelism 1 --force" "$run_script" >/dev/null
if grep -F "docker compose up" "$run_script" >/dev/null; then
  echo "unsafe ann-benchmarks launch path drifted back to docker compose"
  exit 1
fi
