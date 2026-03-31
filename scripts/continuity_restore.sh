#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if blocked_tool_turn="$("$SCRIPT_DIR/client_budget_tool_turn_gate.sh" --tool-name continuity_restore "$@")"; then
  :
else
  status=$?
  if [[ $status -eq 10 ]]; then
    printf '%s\n' "$blocked_tool_turn"
    exit 0
  fi
  exit $status
fi

exec "$SCRIPT_DIR/amai_exec.sh" continuity restore "$@"
