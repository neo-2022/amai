#!/usr/bin/env bash
set -euo pipefail

ensure_default_rustup_toolchain() {
  command -v rustup >/dev/null 2>&1 || return 1
  rustup show active-toolchain >/dev/null 2>&1 || true
  rustup default stable >/dev/null 2>&1 || true
  rustup toolchain install stable >/dev/null 2>&1 || true
  rustup default stable >/dev/null 2>&1
}

candidate_works() {
  local candidate="$1"
  [[ -n "${candidate}" ]] || return 1
  [[ -x "${candidate}" ]] || return 1
  if "${candidate}" --version >/dev/null 2>&1; then
    return 0
  fi
  ensure_default_rustup_toolchain || return 1
  "${candidate}" --version >/dev/null 2>&1
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
