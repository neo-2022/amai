#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

if [[ "${AMAI_EXEC_DISABLE_BUDGET_HELPERS:-0}" != "1" ]]; then
  case "${1:-}" in
    observe)
      case "${2:-}" in
        client-budget-gate)
          shift 2
          exec "$SCRIPT_DIR/client_budget_gate.sh" "$@"
          ;;
        client-budget-root-cause)
          shift 2
          exec "$SCRIPT_DIR/client_budget_root_cause.sh" "$@"
          ;;
      esac
      ;;
  esac
fi

if [[ "${AMAI_EXEC_FORCE_CARGO:-0}" != "1" ]] && [[ -x ./target/release/amai ]]; then
  exec ./target/release/amai "$@"
fi

if command -v cargo >/dev/null 2>&1; then
  exec cargo run --quiet -- "$@"
fi

printf 'Amai runner requires cargo or ./target/release/amai\n' >&2
exit 127
