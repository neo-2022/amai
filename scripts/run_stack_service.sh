#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

export PATH="${HOME}/.local/bin:${HOME}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:${PATH:-}"

resolve_cargo_bin() {
  if [[ -x "./scripts/resolve_cargo.sh" ]]; then
    ./scripts/resolve_cargo.sh
    return 0
  fi
  printf 'cargo\n'
}

resolve_rustc_bin() {
  if [[ -x "./scripts/resolve_rustc.sh" ]]; then
    ./scripts/resolve_rustc.sh
    return 0
  fi
  printf 'rustc\n'
}

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

compact_release_binary_is_fresh() {
  binary_is_fresh "./target/release/amai-bootstrap"
}

compact_debug_binary_is_fresh() {
  binary_is_fresh "./target/debug/amai-bootstrap"
}

release_binary_is_fresh() {
  binary_is_fresh "./target/release/amai"
}

debug_binary_is_fresh() {
  binary_is_fresh "./target/debug/amai"
}

run_bootstrap_stack() {
  local cargo_bin
  cargo_bin="$(resolve_cargo_bin)"
  local rustc_bin
  rustc_bin="$(resolve_rustc_bin)"

  if compact_release_binary_is_fresh; then
    exec ./target/release/amai-bootstrap stack
  fi
  if compact_debug_binary_is_fresh; then
    exec ./target/debug/amai-bootstrap stack
  fi
  if release_binary_is_fresh; then
    exec ./target/release/amai bootstrap stack
  fi
  if debug_binary_is_fresh; then
    exec ./target/debug/amai bootstrap stack
  fi

  exec env RUSTC="${rustc_bin}" "${cargo_bin}" run --quiet --bin amai-bootstrap -- stack
}

./scripts/prepare_stack_runtime.sh
docker compose up -d --remove-orphans
run_bootstrap_stack
