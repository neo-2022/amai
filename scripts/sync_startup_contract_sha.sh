#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
contract_path="${repo_root}/.amai/onboarding/project-chat-startup-contract.json"
startup_marker="AMAI MANAGED STARTUP INSTRUCTIONS v1"
sha_field_label='startup_contract_sha256 = "'

startup_targets=(
  "${repo_root}/AGENTS.md"
  "${repo_root}/.github/instructions/amai-continuity-startup.instructions.md"
  "${repo_root}/.cursor/rules/amai-continuity-startup.mdc"
  "${repo_root}/CLAUDE.md"
  "${repo_root}/.hermes.md"
  "${repo_root}/.openclaw/AGENTS.md"
)

existing_targets=()
for target_path in "${startup_targets[@]}"; do
  if [[ -f "${target_path}" ]]; then
    existing_targets+=("${target_path}")
  fi
done

if [[ ! -f "${contract_path}" ]]; then
  for target_path in "${existing_targets[@]}"; do
    if grep -Fq "${startup_marker}" "${target_path}" || grep -Fq "${sha_field_label}" "${target_path}"; then
      echo "startup contract artifact missing while startup surface exists: ${target_path}" >&2
      exit 1
    fi
  done
  exit 0
fi

startup_sha="$(jq -r '.startup_contract_sha256 // empty' "${contract_path}")"
if [[ ! "${startup_sha}" =~ ^[0-9a-f]{64}$ ]]; then
  echo "startup contract sha256 field is missing or invalid inside artifact" >&2
  exit 1
fi

for target_path in "${existing_targets[@]}"; do
  has_startup_surface=0
  if grep -Fq "${startup_marker}" "${target_path}" || grep -Fq "${sha_field_label}" "${target_path}"; then
    has_startup_surface=1
  fi
  if [[ "${has_startup_surface}" != "1" ]]; then
    continue
  fi
  if ! grep -Eq 'startup_contract_sha256 = "[0-9a-f]{64}"' "${target_path}"; then
    echo "startup_contract_sha256 field is missing or malformed in ${target_path}" >&2
    exit 1
  fi
  python3 - "${target_path}" "${startup_sha}" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
startup_sha = sys.argv[2]
text = path.read_text(encoding="utf-8")
updated = re.sub(
    r'(startup_contract_sha256 = ")[0-9a-f]{64}("\s*,?)',
    lambda match: f'{match.group(1)}{startup_sha}{match.group(2)}',
    text,
)
path.write_text(updated, encoding="utf-8")
PY
done
