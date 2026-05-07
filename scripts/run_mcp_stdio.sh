#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
log_file="${AMAI_MCP_DEBUG_LOG:-/tmp/amai_mcp_debug.log}"
printf '[%s] --- MCP STARTUP DEBUG ---\n' "$(date)" >>"${log_file}" 2>/dev/null || true
source ./scripts/load_env.sh
./scripts/cleanup_mcp_orphans.sh "${repo_root}" >/dev/null 2>&1 || true

if command -v cargo >/dev/null 2>&1; then
  exec cargo run --release --quiet -- mcp serve
fi

if [[ -x ./target/release/amai ]]; then
  exec ./target/release/amai mcp serve
fi

printf 'Amai MCP runner requires cargo or ./target/release/amai binary\n' >&2
exit 1
