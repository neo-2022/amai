#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

if [[ ! -f .env ]]; then
  cp .env.example .env
fi

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  [[ "${line}" == \#* ]] && continue
  [[ "${line}" != *=* ]] && continue
  key="${line%%=*}"
  if ! grep -q "^${key}=" .env; then
    printf '%s\n' "${line}" >> .env
  fi
done < .env.example

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  [[ "${line}" == \#* ]] && continue
  [[ "${line}" != *=* ]] && continue
  key="${line%%=*}"
  value="${line#*=}"
  if [[ -z "${!key+x}" ]]; then
    export "${key}=${value}"
  fi
done < .env
