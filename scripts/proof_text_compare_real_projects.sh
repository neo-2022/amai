#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
art_repo_root="$(cd "${repo_root}/../Art" && pwd)"
cases_file="${repo_root}/fixtures/real_project_text_compare_cases.jsonl"
art_project_code="art"
amai_project_code="amai"

art_paths_file="$(mktemp)"
amai_paths_file="$(mktemp)"
runtime_cases_file="$(mktemp)"
output_file="$(mktemp)"
cleanup() {
  rm -f "${art_paths_file}" "${amai_paths_file}" "${runtime_cases_file}" "${output_file}"
}
trap cleanup EXIT

python3 - "${cases_file}" "${art_paths_file}" "${amai_paths_file}" "${runtime_cases_file}" "${art_project_code}" "${amai_project_code}" <<'PY'
import json
import sys
from pathlib import Path

cases_path = Path(sys.argv[1])
art_paths_path = Path(sys.argv[2])
amai_paths_path = Path(sys.argv[3])
runtime_cases_path = Path(sys.argv[4])
art_project_code = sys.argv[5]
amai_project_code = sys.argv[6]

grouped = {
    "art_real": set(),
    "amai_real": set(),
}
translated_cases = []

for raw_line in cases_path.read_text().splitlines():
    line = raw_line.strip()
    if not line or line.startswith("#"):
        continue
    case = json.loads(line)
    expected_projects = case.get("expected_projects", [])
    expected_paths = case.get("expected_paths", [])
    for project_code in expected_projects:
        if project_code not in grouped:
            raise SystemExit(f"unsupported real-project code in fixture: {project_code}")
        for relative_path in expected_paths:
            grouped[project_code].add(relative_path)
    translated_case = dict(case)
    translated_case["expected_projects"] = [
        art_project_code if project_code == "art_real" else amai_project_code
        for project_code in expected_projects
    ]
    translated_cases.append(translated_case)

for target_path, project_code in (
    (art_paths_path, "art_real"),
    (amai_paths_path, "amai_real"),
):
    paths = sorted(grouped[project_code])
    if not paths:
        raise SystemExit(f"fixture did not provide any expected_paths for {project_code}")
    target_path.write_text("\n".join(paths) + "\n")

runtime_cases_path.write_text(
    "\n".join(json.dumps(case, ensure_ascii=False) for case in translated_cases) + "\n"
)
PY

cd "${repo_root}"

./scripts/bootstrap_stack.sh

cargo run --release --quiet -- project register \
  --code "${art_project_code}" \
  --display-name "Art" \
  --repo-root "${art_repo_root}"

cargo run --release --quiet -- project register \
  --code "${amai_project_code}" \
  --display-name "Amai" \
  --repo-root "${repo_root}"

cargo run --release --quiet -- namespace ensure \
  --project "${art_project_code}" \
  --code review \
  --display-name Review \
  --retrieval-mode local_plus_related

cargo run --release --quiet -- namespace ensure \
  --project "${amai_project_code}" \
  --code review \
  --display-name Review \
  --retrieval-mode local_plus_related

cargo run --release --quiet -- relation add \
  --source "${art_project_code}" \
  --target "${amai_project_code}" \
  --relation-type shared_tooling \
  --shared-contour local_engineering_stack \
  --access-mode local_plus_related

cargo run --release --quiet -- index project \
  --code "${art_project_code}" \
  --path "${art_repo_root}" \
  --namespace review \
  --paths-file "${art_paths_file}" \
  --skip-embeddings

cargo run --release --quiet -- index project \
  --code "${amai_project_code}" \
  --path "${repo_root}" \
  --namespace review \
  --paths-file "${amai_paths_file}" \
  --skip-embeddings

cargo run --release --quiet -- verify text-compare \
  --project "${art_project_code}" \
  --namespace review \
  --retrieval-mode local_plus_related \
  --cases-file "${runtime_cases_file}" >"${output_file}"

grep -q '"text_compare"' "${output_file}"
grep -q '"mean_precision"' "${output_file}"
grep -q "\"${art_project_code}\"" "${output_file}"
grep -q "\"${amai_project_code}\"" "${output_file}"
jq -e '.text_compare.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' "${output_file}" >/dev/null
jq -e '.text_compare.canonical_eval.probes | length > 0' "${output_file}" >/dev/null

echo "proof_text_compare_real_projects: ok"
