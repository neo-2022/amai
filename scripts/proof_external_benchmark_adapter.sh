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
grep -F "Статус: upstream benchmark сейчас отключил этот launch path" <<<"$ann_output" >/dev/null

unsupported_output="$(cargo run --quiet -- benchmark external-adapter --benchmark ann_benchmarks --dataset sphere_10m_meta_dpr)"
printf '%s\n' "$unsupported_output"
grep -F "Статус: dataset пока не поддержан этим benchmark-ом" <<<"$unsupported_output" >/dev/null

harvest_output="$(cargo run --quiet -- benchmark external-harvest --benchmark ann_benchmarks --dataset dbpedia_openai_1000k_angular)"
printf '%s\n' "$harvest_output"
grep -F "Adapter status: blocked_upstream_disabled" <<<"$harvest_output" >/dev/null

run_script="state/external-benchmarks/runs/ann_benchmarks/dbpedia_openai_1000k_angular/latest/run_external.sh"
grep -F "disabled=true" "$run_script" >/dev/null

unsupported_script="state/external-benchmarks/runs/ann_benchmarks/sphere_10m_meta_dpr/latest/run_external.sh"
grep -F "adapter/patch слоя" "$unsupported_script" >/dev/null
