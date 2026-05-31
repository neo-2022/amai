#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_index="$(mktemp)"
tmp_output="$(mktemp)"
cleanup() {
  rm -f "${tmp_index}" "${tmp_output}"
}
trap cleanup EXIT

cp .git/index "${tmp_index}"
GIT_INDEX_FILE="${tmp_index}" git update-index --force-remove scripts/epoch_ms.sh
if GIT_INDEX_FILE="${tmp_index}" ./scripts/repo_hygiene_guard.sh --json >"${tmp_output}" 2>&1; then
  echo "proof_repo_hygiene_guard: transitive helper removal unexpectedly passed" >&2
  exit 1
fi

jq -e '
  .status == "drift_detected"
  and (.issues | index("report_gate_scripts_untracked"))
  and (.untracked_report_gate_scripts | index("./scripts/epoch_ms.sh"))
' "${tmp_output}" >/dev/null

./scripts/repo_hygiene_guard.sh --json
echo "proof_repo_hygiene_guard: ok"
