#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

./scripts/cleanup_mcp_orphans.sh "${repo_root}" >/dev/null 2>&1 || true

has_flag() {
  local expected="$1"
  shift
  local arg
  for arg in "$@"; do
    if [[ "${arg}" == "${expected}" ]]; then
      return 0
    fi
  done
  return 1
}

args=("$@")

if ! has_flag "--yes" "${args[@]}"; then
  args+=("--yes")
fi

exec env AMAI_EXEC_FORCE_CARGO=1 ./scripts/amai_exec.sh bootstrap reconnect "${args[@]}"
