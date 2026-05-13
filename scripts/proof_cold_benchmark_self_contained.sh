#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
output_dir="${repo_root}/state/cold-benchmark/self-contained-proof"

cd "${repo_root}"
./scripts/cold_benchmark.sh \
  --manifest config/cold_benchmark_self_contained.toml \
  --cycles 1 \
  --output-dir "${output_dir}" >/tmp/amai_cold_benchmark_self_contained.json

python3 - <<'PY'
import json
from pathlib import Path

payload = json.loads(Path("/tmp/amai_cold_benchmark_self_contained.json").read_text())
node = payload["cold_benchmark"]
summary = node["machine_readable_summary"]
coverage = node["dataset_coverage"]
query_coverage = node["query_coverage"]
canonical_eval = node["canonical_eval"]
dashboard_scope = node["dashboard_scope"]

assert summary["sample_count"] >= 8
assert summary["precision"] >= 0.997
assert summary["recall"] >= 0.997
assert summary["hit_rate"] >= 0.997
assert summary["leakage"] == 0
assert summary["error_rate"] == 0.0
assert coverage["repo_count"] == 1
assert query_coverage["query_slice_count"] >= 8
assert dashboard_scope["class"] == "proof"
assert dashboard_scope["canonical_for_dashboard"] is False
assert canonical_eval["eval_verdict_model_version"] == "memory-eval-verdict-v1"
assert len(canonical_eval["probes"]) == summary["sample_count"]
assert canonical_eval["verdict_counts"]
PY

test -f "${output_dir}/summary.json"
test -f "${output_dir}/report.md"
test -f "${output_dir}/samples.csv"

printf 'proof_cold_benchmark_self_contained: ok\n'
