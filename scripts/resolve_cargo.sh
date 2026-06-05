#!/usr/bin/env bash
set -euo pipefail

rustup_timeout_seconds="${AMAI_RUSTUP_REPAIR_TIMEOUT_SECONDS:-30}"
cargo_check_timeout_seconds="${AMAI_CARGO_CHECK_TIMEOUT_SECONDS:-20}"

run_bounded() {
  local timeout_seconds="$1"
  shift
  if command -v timeout >/dev/null 2>&1; then
    timeout --preserve-status --kill-after=5s "${timeout_seconds}s" "$@"
    return $?
  fi
  "$@"
}

ensure_default_rustup_toolchain() {
  command -v rustup >/dev/null 2>&1 || return 1
  run_bounded "${rustup_timeout_seconds}" rustup show active-toolchain >/dev/null 2>&1 || true
  run_bounded "${rustup_timeout_seconds}" rustup default stable >/dev/null 2>&1 || true
  run_bounded "${rustup_timeout_seconds}" rustup toolchain install stable >/dev/null 2>&1 || true
  run_bounded "${rustup_timeout_seconds}" rustup default stable >/dev/null 2>&1
}

candidate_works() {
  local candidate="$1"
  [[ -n "${candidate}" ]] || return 1
  [[ -x "${candidate}" ]] || return 1
  if run_bounded "${cargo_check_timeout_seconds}" "${candidate}" --version >/dev/null 2>&1; then
    return 0
  fi
  ensure_default_rustup_toolchain || return 1
  run_bounded "${cargo_check_timeout_seconds}" "${candidate}" --version >/dev/null 2>&1
}

if [[ -n "${AMAI_CARGO_BIN:-}" ]] && candidate_works "${AMAI_CARGO_BIN}"; then
  printf '%s\n' "${AMAI_CARGO_BIN}"
  exit 0
fi

if command -v cargo >/dev/null 2>&1; then
  cargo_path="$(command -v cargo)"
  if candidate_works "${cargo_path}"; then
    printf '%s\n' "${cargo_path}"
    exit 0
  fi
fi

for candidate in /usr/bin/cargo /bin/cargo; do
  if candidate_works "${candidate}"; then
    printf '%s\n' "${candidate}"
    exit 0
  fi
done

printf 'Amai runner requires a working cargo binary. Install rust/cargo or set AMAI_CARGO_BIN.\n' >&2
exit 127
