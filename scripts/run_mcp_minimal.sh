#!/usr/bin/env bash
set -euo pipefail
printf '%s wrapper-start cwd=%s argv0=%s\n' "$(date -Is)" "$PWD" "$0" >> /tmp/amai_mcp_wrapper.log
export AMAI_MCP_DEBUG_LOG=/tmp/amai_mcp_server_wire.log
exec /home/art/agent-memory-index/target/release/amai mcp serve
