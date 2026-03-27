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

cd "$SCRIPT_DIR/.."
exec cargo run --quiet -- continuity answer "$@"
