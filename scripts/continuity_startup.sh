#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [[ -x "./target/release/amai" ]]; then
  exec "./target/release/amai" continuity startup "$@"
fi
exec "$SCRIPT_DIR/amai_exec.sh" continuity startup "$@"
