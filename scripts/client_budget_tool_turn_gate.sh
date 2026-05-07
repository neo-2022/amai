#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

# Tool-turn budget pressure is advisory-only. Guards still surface through
# client_budget_gate/root_cause markers, but this preflight must not hard-block
# tools in the current chat.
exit 0
