#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
./scripts/cleanup_mcp_orphans.sh "${repo_root}" >/dev/null 2>&1 || true

exec env AMAI_EXEC_FORCE_CARGO=1 ./scripts/amai_exec.sh bootstrap onboarding "$@"
