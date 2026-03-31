#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if blocked_reply="$("$SCRIPT_DIR/client_budget_reply_gate.sh")"; then
  :
else
  status=$?
  if [[ $status -eq 10 ]]; then
    printf '%s\n' "$blocked_reply"
    exit 0
  fi
  exit $status
fi

if blocked_tool_turn="$("$SCRIPT_DIR/client_budget_tool_turn_gate.sh" --tool-name continuity_answer "$@")"; then
  :
else
  status=$?
  if [[ $status -eq 10 ]]; then
    printf '%s\n' "$blocked_tool_turn"
    exit 0
  fi
  exit $status
fi

cd "$SCRIPT_DIR/.."
exec "$SCRIPT_DIR/amai_exec.sh" continuity answer "$@"
