#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

release_binary_is_fresh() {
  local binary="./target/release/amai"
  [[ -x "$binary" ]] || return 1
  local candidate
  for candidate in Cargo.toml Cargo.lock; do
    if [[ -f "$candidate" && "$candidate" -nt "$binary" ]]; then
      return 1
    fi
  done
  local path
  for path in src sql; do
    [[ -e "$path" ]] || continue
    if find "$path" -type f -newer "$binary" -print -quit 2>/dev/null | grep -q .; then
      return 1
    fi
  done
  return 0
}

build_release_binary() {
  if [[ "${AMAI_EXEC_SUPPRESS_BUILD_NOISE:-1}" == "1" ]]; then
    local log_dir="state/logs"
    mkdir -p "$log_dir"
    local log_path="${log_dir}/amai_build_$(date +%s).log"
    if ! cargo build --release --quiet >"$log_path" 2>&1; then
      echo "Amai build failed. See ${log_path}" >&2
      sed -n '1,200p' "$log_path" >&2
      exit 1
    fi
    return 0
  fi

  cargo build --release --quiet
}

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

if [[ "${AMAI_EXEC_FORCE_CARGO:-0}" != "1" ]] && release_binary_is_fresh; then
  exec ./target/release/amai "$@"
fi

if command -v cargo >/dev/null 2>&1; then
  if ! release_binary_is_fresh; then
    build_release_binary
  fi
  exec ./target/release/amai "$@"
fi

printf 'Amai runner requires cargo or ./target/release/amai\n' >&2
exit 127
