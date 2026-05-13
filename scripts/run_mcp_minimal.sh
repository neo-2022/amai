#!/usr/bin/env bash
set -euo pipefail
printf '%s wrapper-start cwd=%s argv0=%s\n' "$(date -Is)" "$PWD" "$0" >> /tmp/amai_mcp_wrapper.log
export AMAI_MCP_DEBUG_LOG=/tmp/amai_mcp_server_wire.log
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -x "${REPO_ROOT}/target/release/amai" ]]; then
  exec "${REPO_ROOT}/target/release/amai" mcp serve
fi
if [[ -x "${REPO_ROOT}/target/debug/amai" ]]; then
  exec "${REPO_ROOT}/target/debug/amai" mcp serve
fi
exec cargo run --manifest-path "${REPO_ROOT}/Cargo.toml" --release --quiet -- mcp serve
