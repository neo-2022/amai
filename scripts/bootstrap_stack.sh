#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

cargo_bin="$(./scripts/resolve_cargo.sh)"
rustc_bin="$(./scripts/resolve_rustc.sh)"

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
    RUSTC="${rustc_bin}" "${cargo_bin}" run -- bootstrap preflight --stack-profile "${stack_profile}"
  fi

  # Compose bind mounts fail closed if the host-side state tree is absent.
  # Clean installs and freshly synced remote repos must not depend on preexisting
  # runtime directories from an older workspace.
  mkdir -p \
    state/postgres \
    state/qdrant \
    state/minio \
    state/nats \
    tmp/postgres \
    tmp/nats

  ./scripts/render_nats_config.sh >/dev/null
  ./scripts/render_postgres_config.sh >/dev/null
  docker compose up -d --remove-orphans
  if release_binary_is_fresh; then
    ./target/release/amai bootstrap stack
  else
    RUSTC="${rustc_bin}" "${cargo_bin}" run -- bootstrap stack
  fi

  if [[ -n "${AMI_WARMUP_PROJECTS:-}" ]]; then
    ./scripts/warmup_cache.sh
  fi
}

# `docker compose up -d` may spawn long-lived rootless Podman helpers that inherit
# open file descriptors. Use `flock --close` so the bootstrap lock never leaks into
# conmon/rootlessport and future bootstrap runs do not deadlock on a stale holder.
export cargo_bin rustc_bin stack_profile
export -f release_binary_is_fresh
export -f bootstrap_main
flock --exclusive --close "${bootstrap_lock_file}" bash -lc 'bootstrap_main'
