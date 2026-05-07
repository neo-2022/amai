#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."
if [[ -x "./target/release/amai" ]]; then
  exec "./target/release/amai" continuity startup-state "$@"
fi
exec "$SCRIPT_DIR/amai_exec.sh" continuity startup-state "$@"
