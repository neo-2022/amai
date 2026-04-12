#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

cargo run --quiet -- benchmark external-datasets
cargo run --quiet -- benchmark external-plan --benchmark vectordbbench
cargo run --quiet -- benchmark external-plan --benchmark ann_benchmarks

vectordbbench_output="$(cargo run --quiet -- benchmark external-adapter --benchmark vectordbbench --dataset dbpedia_openai_1000k_angular)"
printf '%s\n' "$vectordbbench_output"
grep -F "Статус: готов к следующему запуску" <<<"$vectordbbench_output" >/dev/null
grep -F "Adapter kind: custom_parquet_bundle" <<<"$vectordbbench_output" >/dev/null
grep -F "Conversion bundle:" <<<"$vectordbbench_output" >/dev/null

ann_output="$(cargo run --quiet -- benchmark external-adapter --benchmark ann_benchmarks --dataset dbpedia_openai_1000k_angular)"
printf '%s\n' "$ann_output"
grep -F "Статус: готов к следующему запуску" <<<"$ann_output" >/dev/null
grep -F "Adapter kind: direct_hdf5" <<<"$ann_output" >/dev/null

unsupported_output="$(cargo run --quiet -- benchmark external-adapter --benchmark ann_benchmarks --dataset sphere_10m_meta_dpr)"
printf '%s\n' "$unsupported_output"
grep -F "Статус: dataset пока не поддержан этим benchmark-ом" <<<"$unsupported_output" >/dev/null

vectordbbench_harvest_output="$(cargo run --quiet -- benchmark external-harvest --benchmark vectordbbench --dataset dbpedia_openai_1000k_angular)"
printf '%s\n' "$vectordbbench_harvest_output"
grep -F "Adapter status: prepared" <<<"$vectordbbench_harvest_output" >/dev/null
if grep -F "Run state: running" <<<"$vectordbbench_harvest_output" >/dev/null; then
  grep -F "External result files: ещё не появились" <<<"$vectordbbench_harvest_output" >/dev/null
else
  grep -F "Run state: finished_ok" <<<"$vectordbbench_harvest_output" >/dev/null
  grep -F "Result verdict: benchmark_ok" <<<"$vectordbbench_harvest_output" >/dev/null
  grep -F "External result files: 1" <<<"$vectordbbench_harvest_output" >/dev/null
  grep -F "db=QdrantLocal" <<<"$vectordbbench_harvest_output" >/dev/null
fi
grep -F "Benchmark Qdrant URL is env-driven via AMAI_BENCHMARK_QDRANT_HTTP_URL" <<<"$vectordbbench_harvest_output" >/dev/null

harvest_output="$(cargo run --quiet -- benchmark external-harvest --benchmark ann_benchmarks --dataset dbpedia_openai_1000k_angular)"
printf '%s\n' "$harvest_output"
grep -F "Adapter status: prepared" <<<"$harvest_output" >/dev/null
if grep -F "Run state: running" <<<"$harvest_output" >/dev/null; then
  grep -F "Message: upstream launch running" <<<"$harvest_output" >/dev/null
  grep -F "Heartbeat at epoch_s:" <<<"$harvest_output" >/dev/null
  grep -F "External result files:" <<<"$harvest_output" >/dev/null
  if grep -F "Live progress: group " <<<"$harvest_output" >/dev/null; then
    :
  fi
elif grep -F "Run state: finished_error" <<<"$harvest_output" >/dev/null; then
  grep -F "Result verdict: partial_results" <<<"$harvest_output" >/dev/null
  grep -F "External result files:" <<<"$harvest_output" >/dev/null
  grep -F "db=qdrant" <<<"$harvest_output" >/dev/null
else
  grep -F "Run state: finished_ok" <<<"$harvest_output" >/dev/null
  grep -F "Result verdict: benchmark_ok" <<<"$harvest_output" >/dev/null
  grep -F "External result files:" <<<"$harvest_output" >/dev/null
  grep -F "db=qdrant" <<<"$harvest_output" >/dev/null
fi

run_script="state/external-benchmarks/runs/ann_benchmarks/dbpedia_openai_1000k_angular/latest/run_external.sh"
grep -F "ann-benchmarks" "$run_script" >/dev/null
grep -F ".amai-ann-qdrant-launch-v5" "$run_script" >/dev/null
grep -F 'disabled: true' "$run_script" >/dev/null
grep -F 'disabled: false' "$run_script" >/dev/null
grep -F 'network_mode="host"' "$run_script" >/dev/null
grep -F 'network_mode="bridge"' "$run_script" >/dev/null
grep -F 'ports=_amai_bridge_port_bindings()' "$run_script" >/dev/null
grep -F 'AMAI_BENCHMARK_QDRANT_HTTP_URL_EFFECTIVE' "$run_script" >/dev/null
grep -F 'Ignoring ann-benchmarks docker log race' "$run_script" >/dev/null
grep -F 'Ignoring ann-benchmarks docker error-log race' "$run_script" >/dev/null
grep -F 'Ignoring ann-benchmarks docker remove race' "$run_script" >/dev/null
grep -F 'heartbeat_at_epoch_s' "$run_script" >/dev/null
grep -F 'write_running_status_heartbeat()' "$run_script" >/dev/null
grep -F 'upstream launch running' "$run_script" >/dev/null
grep -F 'STATUS_HEARTBEAT_PID=$!' "$run_script" >/dev/null
grep -F "print('partial_results')" "$run_script" >/dev/null
grep -F "./.venv/bin/python run.py --dataset dbpedia-openai-1000k-angular --algorithm qdrant --runs 1 --parallelism 1 --timeout 21600 --force" "$run_script" >/dev/null

vectordbbench_run_script="state/external-benchmarks/runs/vectordbbench/dbpedia_openai_1000k_angular/latest/run_external.sh"
grep -F ".amai-vdbbench-qdrant-timeout-600-v2" "$vectordbbench_run_script" >/dev/null
grep -F "export NUM_PER_BATCH=16" "$vectordbbench_run_script" >/dev/null
grep -F "export LOAD_CONCURRENCY=1" "$vectordbbench_run_script" >/dev/null
grep -F "export AMAI_VDBBENCH_QDRANT_BATCH_SIZE=16" "$vectordbbench_run_script" >/dev/null
grep -F "export AMAI_VDBBENCH_QDRANT_SKIP_INDEX_TOGGLE=1" "$vectordbbench_run_script" >/dev/null
grep -F "qdrant-client==1.12.2" "$vectordbbench_run_script" >/dev/null
grep -F "qdrant/qdrant:v1.12.5" "$vectordbbench_run_script" >/dev/null
grep -F 'amaiexternal/dbpedia_openai_1000k_angular' state/external-benchmarks/runs/vectordbbench/dbpedia_openai_1000k_angular/latest/summary.json >/dev/null

unsupported_script="state/external-benchmarks/runs/ann_benchmarks/sphere_10m_meta_dpr/latest/run_external.sh"
grep -F "adapter/patch слоя" "$unsupported_script" >/dev/null
