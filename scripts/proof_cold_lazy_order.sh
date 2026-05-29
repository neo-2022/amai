#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
progress_file="${repo_root}/state/cold-benchmark/live_progress.json"
output_file="/tmp/amai_cold_lazy_order_proof.json"

cd "${repo_root}"
./target/release/amai verify cold-path \
  --manifest config/cold_benchmark_lazy_order_proof.toml \
  --cycles 1 >"${output_file}" &
runner_pid=$!

cleanup() {
  if kill -0 "${runner_pid}" 2>/dev/null; then
    kill "${runner_pid}" 2>/dev/null || true
    wait "${runner_pid}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

lazy_transition_seen=0
for _ in $(seq 1 60); do
  if [[ -f "${progress_file}" ]] && jq -e '
    .cold_benchmark_progress.progress.completed_case_count == 1 and
    .cold_benchmark_progress.progress.repo_count == 1 and
    .cold_benchmark_progress.current_repo_code == "amai"
  ' "${progress_file}" >/dev/null; then
    lazy_transition_seen=1
    break
  fi
  if ! kill -0 "${runner_pid}" 2>/dev/null; then
    break
  fi
  sleep 1
done

wait "${runner_pid}"
trap - EXIT

test "${lazy_transition_seen}" -eq 1
jq -e '.cold_benchmark.machine_readable_summary.sample_count == 2' "${output_file}" >/dev/null
jq -e '[.cold_benchmark.indexed_repos[].repo_code] == ["art", "amai"]' "${output_file}" >/dev/null

printf 'proof_cold_lazy_order: ok\n'
