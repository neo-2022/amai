#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

managed_clone_root="${AMAI_GITHUB_CLONE_DIR:-${HOME}/.local/share/amai/repo}"
if [[ "${repo_root}" == "${managed_clone_root}" ]]; then
  export AMAI_BOOTSTRAP_REMOVE_MODE=full
fi

exec ./scripts/amai_exec.sh bootstrap remove "$@"
