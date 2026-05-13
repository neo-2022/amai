#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

managed_clone_root="${AMAI_GITHUB_CLONE_DIR:-${HOME}/.local/share/amai/repo}"
if [[ "${repo_root}" == "${managed_clone_root}" ]]; then
  export AMAI_BOOTSTRAP_REMOVE_MODE=full
fi

filter_compact_remove_args() {
  local arg
  local filtered=()
  for arg in "$@"; do
    case "$arg" in
      --yes)
        continue
        ;;
    esac
    filtered+=("$arg")
  done
  printf '%s\0' "${filtered[@]}"
}

readarray -d '' REMOVE_ARGS < <(filter_compact_remove_args "$@")

compact_debug_binary_is_fresh() {
  local binary="./target/debug/amai-bootstrap"
  [[ -x "${binary}" ]] || return 1
  local candidate
  for candidate in Cargo.toml Cargo.lock; do
    if [[ -f "${candidate}" && "${candidate}" -nt "${binary}" ]]; then
      return 1
    fi
  done
  local path
  for path in src sql; do
    [[ -e "${path}" ]] || continue
    if find "${path}" -type f -newer "${binary}" -print -quit 2>/dev/null | grep -q .; then
      return 1
    fi
  done
  return 0
}

debug_binary_is_fresh() {
  local binary="./target/debug/amai"
  [[ -x "${binary}" ]] || return 1
  local candidate
  for candidate in Cargo.toml Cargo.lock; do
    if [[ -f "${candidate}" && "${candidate}" -nt "${binary}" ]]; then
      return 1
    fi
  done
  local path
  for path in src sql; do
    [[ -e "${path}" ]] || continue
    if find "${path}" -type f -newer "${binary}" -print -quit 2>/dev/null | grep -q .; then
      return 1
    fi
  done
  return 0
}

if [[ "${AMAI_BOOTSTRAP_REMOVE_MODE:-}" == "full" ]] && [[ ! -x ./target/release/amai-bootstrap ]] && compact_debug_binary_is_fresh; then
  exec ./target/debug/amai-bootstrap remove "${REMOVE_ARGS[@]}"
fi

if [[ "${AMAI_BOOTSTRAP_REMOVE_MODE:-}" == "full" ]] && [[ -x ./target/release/amai-bootstrap ]]; then
  exec ./target/release/amai-bootstrap remove "${REMOVE_ARGS[@]}"
fi

if [[ "${AMAI_BOOTSTRAP_REMOVE_MODE:-}" == "full" ]] && [[ ! -x ./target/release/amai-bootstrap ]]; then
  cargo_bin="$(./scripts/resolve_cargo.sh)"
  rustc_bin="$(./scripts/resolve_rustc.sh)"
  exec env \
    RUSTC="${rustc_bin}" \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off \
    "${cargo_bin}" run --quiet --bin amai-bootstrap -- remove "${REMOVE_ARGS[@]}"
fi

if [[ "${AMAI_BOOTSTRAP_REMOVE_MODE:-}" == "full" ]] && [[ ! -x ./target/release/amai ]] && debug_binary_is_fresh; then
  exec ./target/debug/amai bootstrap remove "$@"
fi

if [[ "${AMAI_BOOTSTRAP_REMOVE_MODE:-}" == "full" ]] && [[ ! -x ./target/release/amai ]]; then
  cargo_bin="$(./scripts/resolve_cargo.sh)"
  rustc_bin="$(./scripts/resolve_rustc.sh)"
  exec env \
    RUSTC="${rustc_bin}" \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off \
    "${cargo_bin}" run --quiet -- bootstrap remove "$@"
fi

exec ./scripts/amai_exec.sh bootstrap remove "$@"
