#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
output_dir="${repo_root}/state/cold-benchmark/proof"

cd "${repo_root}"
./scripts/cold_benchmark.sh \
  --manifest config/cold_benchmark_proof.toml \
  --cycles 1 \
  --output-dir "${output_dir}" >/tmp/amai_cold_benchmark_proof.json

python3 - <<'PY'
import json
from pathlib import Path

payload = json.loads(Path("/tmp/amai_cold_benchmark_proof.json").read_text())
node = payload["cold_benchmark"]
summary = node["machine_readable_summary"]
assert "p50" in summary
assert "p95" in summary
assert "p99" in summary
assert "max" in summary
assert "sample_count" in summary
assert "precision" in summary
assert "recall" in summary
assert "hit_rate" in summary
assert "fallback_rate" in summary
assert node["dataset_coverage"]["repo_count"] >= 1
PY

test -f "${output_dir}/summary.json"
test -f "${output_dir}/report.md"
test -f "${output_dir}/samples.csv"

printf 'proof_cold_benchmark: ok\n'
