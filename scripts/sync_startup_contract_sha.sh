#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
contract_path="${repo_root}/.amai/onboarding/project-chat-startup-contract.json"
agents_path="${repo_root}/AGENTS.md"

if [[ ! -f "${contract_path}" ]]; then
  echo "startup contract not found: ${contract_path}" >&2
  exit 1
fi
if [[ ! -f "${agents_path}" ]]; then
  echo "AGENTS.md not found: ${agents_path}" >&2
  exit 1
fi

startup_sha="$(jq -r '.startup_contract_sha256 // empty' "${contract_path}")"
if [[ ! "${startup_sha}" =~ ^[0-9a-f]{64}$ ]]; then
  echo "startup contract sha256 field is missing or invalid inside artifact" >&2
  exit 1
fi

if ! grep -Eq 'startup_contract_sha256 = "[0-9a-f]{64}"' "${agents_path}"; then
  echo "startup_contract_sha256 field not found in AGENTS.md" >&2
  exit 1
fi

perl -0pi -e "s/(startup_contract_sha256 = \")[0-9a-f]{64}(\"\\s*,?)/\\1${startup_sha}\\2/g" "${agents_path}"
