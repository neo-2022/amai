#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
manifest_path="${repo_root}/state/cold-benchmark/generated_manifest.toml"
output_dir="${repo_root}/state/cold-benchmark/canonical"

cd "${repo_root}"
if [[ ! -f "${manifest_path}" ]]; then
  ./scripts/materialize_cold_repo_pool.sh >/tmp/amai_cold_benchmark_repo_pool_materialize.log
fi

./scripts/cold_benchmark.sh \
  --manifest "${manifest_path}" \
  --cycles 5 \
  --skip-index \
  --output-dir "${output_dir}" >/tmp/amai_cold_benchmark_canonical.json

python3 - "${manifest_path}" <<'PY'
import json
import sys
from pathlib import Path
import tomllib

manifest_path = Path(sys.argv[1])
payload = json.loads(Path("/tmp/amai_cold_benchmark_canonical.json").read_text())
manifest = tomllib.loads(manifest_path.read_text())
node = payload["cold_benchmark"]
summary = node["machine_readable_summary"]
canonical_eval = node["canonical_eval"]
dashboard_scope = node["dashboard_scope"]
case_count = len(manifest.get("cases", []))
assert "p50" in summary
assert "p95" in summary
assert "p99" in summary
assert "max" in summary
assert "sample_count" in summary
assert "precision" in summary
assert "recall" in summary
assert "hit_rate" in summary
assert "fallback_rate" in summary
assert "leakage" in summary
assert "error_rate" in summary
assert canonical_eval["eval_verdict_model_version"] == "memory-eval-verdict-v1"
assert len(canonical_eval["probes"]) == summary["sample_count"]
assert canonical_eval["verdict_counts"]
assert dashboard_scope["class"] == "canonical"
assert dashboard_scope["canonical_for_dashboard"] is True
assert node["dataset_coverage"]["repo_count"] >= 75
assert node["machine_readable_summary"]["sample_count"] >= 1000
assert case_count >= 200
PY

test -f "${output_dir}/summary.json"
test -f "${output_dir}/report.md"
test -f "${output_dir}/samples.csv"

printf 'proof_cold_benchmark_canonical: ok\n'
