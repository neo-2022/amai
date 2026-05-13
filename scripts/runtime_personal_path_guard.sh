#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

json_mode=0
if [[ "${1:-}" == "--json" ]]; then
  json_mode=1
fi

declare -a scoped_paths=(
  "scripts"
)

allow_patterns='scripts/proof_|scripts/runtime_personal_path_guard\.sh'

matches="$(
  rg -n --no-heading "/home/art" "${scoped_paths[@]}" \
    | rg -v "${allow_patterns}" \
    || true
)"

status="ok"
if [[ -n "${matches}" ]]; then
  status="drift_detected"
fi

matches_json="$(printf '%s\n' "${matches}" | jq -R -s 'split("\n") | map(select(length > 0))')"
payload="$(jq -n \
  --arg status "${status}" \
  --arg scope "scripts(runtime)" \
  --argjson matches "${matches_json}" \
  '{
    status: $status,
    scope: $scope,
    issue: "personal_absolute_paths",
    matches: $matches
  }'
)"

if [[ "${json_mode}" -eq 1 ]]; then
  printf '%s\n' "${payload}"
else
  printf '%s\n' "${payload}" | jq .
fi

if [[ "${status}" != "ok" ]]; then
  exit 1
fi
