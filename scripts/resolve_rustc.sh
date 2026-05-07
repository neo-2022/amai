#!/usr/bin/env bash
set -euo pipefail

ensure_default_rustup_toolchain() {
  command -v rustup >/dev/null 2>&1 || return 1
  rustup show active-toolchain >/dev/null 2>&1 && return 0
  rustup default stable >/dev/null 2>&1
}

candidate_works() {
  local candidate="$1"
  [[ -n "${candidate}" ]] || return 1
  [[ -x "${candidate}" ]] || return 1
  if "${candidate}" -vV >/dev/null 2>&1; then
    return 0
  fi
  ensure_default_rustup_toolchain || return 1
  "${candidate}" -vV >/dev/null 2>&1
}

if [[ -n "${AMAI_RUSTC_BIN:-}" ]] && candidate_works "${AMAI_RUSTC_BIN}"; then
  printf '%s\n' "${AMAI_RUSTC_BIN}"
  exit 0
fi

if command -v rustc >/dev/null 2>&1; then
  rustc_path="$(command -v rustc)"
  if candidate_works "${rustc_path}"; then
    printf '%s\n' "${rustc_path}"
    exit 0
  fi
fi

for candidate in /usr/bin/rustc /bin/rustc; do
  if candidate_works "${candidate}"; then
    printf '%s\n' "${candidate}"
    exit 0
  fi
done

printf 'Amai runner requires a working rustc binary. Install rustc or set AMAI_RUSTC_BIN.\n' >&2
exit 127
