#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

binary_is_fresh() {
  local binary="$1"
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

release_binary_is_fresh() {
  binary_is_fresh "./target/release/amai"
}

debug_binary_is_fresh() {
  binary_is_fresh "./target/debug/amai"
}

build_binary() {
  local profile="$1"
  local cargo_args=(build --quiet)
  local binary="./target/debug/amai"
  if [[ "${profile}" == "release" ]]; then
    cargo_args=(build --release --quiet)
    binary="./target/release/amai"
  fi
  if [[ "${AMAI_EXEC_SUPPRESS_BUILD_NOISE:-1}" == "1" ]]; then
    local log_dir="state/logs"
    mkdir -p "$log_dir"
    local log_path="${log_dir}/amai_${profile}_build_$(date +%s).log"
    if ! cargo "${cargo_args[@]}" >"$log_path" 2>&1; then
      echo "Amai build failed. See ${log_path}" >&2
      sed -n '1,200p' "$log_path" >&2
      exit 1
    fi
    printf '%s\n' "$binary"
    return 0
  fi

  cargo "${cargo_args[@]}"
  printf '%s\n' "$binary"
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

if [[ "${AMAI_EXEC_FORCE_CARGO:-0}" != "1" ]] && debug_binary_is_fresh; then
  exec ./target/debug/amai "$@"
fi

if command -v cargo >/dev/null 2>&1; then
  selected_binary="./target/debug/amai"
  selected_profile="debug"
  if [[ -x "./target/release/amai" ]]; then
    selected_binary="./target/release/amai"
    selected_profile="release"
  fi
  if ! binary_is_fresh "${selected_binary}"; then
    selected_binary="$(build_binary "${selected_profile}")"
  fi
  exec "${selected_binary}" "$@"
fi

printf 'Amai runner requires cargo or ./target/release/amai\n' >&2
exit 127
