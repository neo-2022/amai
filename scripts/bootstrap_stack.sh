#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
docker_bin="./scripts/docker_wrapper.sh"

cargo_bin="$(./scripts/resolve_cargo.sh)"
rustc_bin="$(./scripts/resolve_rustc.sh)"

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

run_bootstrap_command() {
  local command_name="$1"
  shift
  if compact_release_binary_is_fresh; then
    ./target/release/amai-bootstrap "${command_name}" "$@"
    return 0
  fi
  if compact_debug_binary_is_fresh; then
    ./target/debug/amai-bootstrap "${command_name}" "$@"
    return 0
  fi
  if release_binary_is_fresh; then
    ./target/release/amai bootstrap "${command_name}" "$@"
    return 0
  fi
  if debug_binary_is_fresh; then
    ./target/debug/amai bootstrap "${command_name}" "$@"
    return 0
  fi
  if [[ "${command_name}" == "stack" || "${command_name}" == "preflight" ]]; then
    RUSTC="${rustc_bin}" "${cargo_bin}" run --quiet --bin amai-bootstrap -- "${command_name}" "$@"
    return 0
  fi
  RUSTC="${rustc_bin}" "${cargo_bin}" run -- bootstrap "${command_name}" "$@"
}

bootstrap_lock_dir="state/locks"
bootstrap_lock_file="${bootstrap_lock_dir}/bootstrap_stack.lock"
mkdir -p "${bootstrap_lock_dir}"

stack_profile="${AMI_STACK_PROFILE:-default}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --stack-profile)
      stack_profile="${2:?missing value for --stack-profile}"
      shift 2
      ;;
    *)
      echo "unsupported bootstrap_stack.sh argument: $1" >&2
      exit 1
      ;;
  esac
done

bootstrap_main() {
  export AMI_STACK_PROFILE="${stack_profile}"

  if [[ "${AMAI_SKIP_STACK_PREFLIGHT:-0}" != "1" ]]; then
    run_bootstrap_command preflight --stack-profile "${stack_profile}"
  fi

  ./scripts/prepare_stack_runtime.sh
  "${docker_bin}" compose up -d --remove-orphans
  run_bootstrap_command stack

  if [[ -n "${AMI_WARMUP_PROJECTS:-}" ]]; then
    ./scripts/warmup_cache.sh
  fi
}

# `docker compose up -d` may spawn long-lived rootless Podman helpers that inherit
# open file descriptors. Use `flock --close` so the bootstrap lock never leaks into
# conmon/rootlessport and future bootstrap runs do not deadlock on a stale holder.
export cargo_bin rustc_bin stack_profile
export docker_bin
export -f compact_release_binary_is_fresh
export -f compact_debug_binary_is_fresh
export -f release_binary_is_fresh
export -f debug_binary_is_fresh
export -f binary_is_fresh
export -f run_bootstrap_command
export -f bootstrap_main
flock --exclusive --close "${bootstrap_lock_file}" bash -lc 'bootstrap_main'
