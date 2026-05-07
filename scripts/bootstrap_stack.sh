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

cleanup_conflicting_named_container() {
  local name="$1"
  local inspect_line=""
  inspect_line="$(docker inspect "${name}" --format '{{.State.Running}} {{range .Mounts}}{{.Type}}|{{.Source}};{{end}}' 2>/dev/null || true)"
  [[ -n "${inspect_line}" ]] || return 0

  local running="${inspect_line%% *}"
  local mounts_blob="${inspect_line#* }"
  local has_foreign_bind=0
  local has_missing_foreign_bind=0
  local entry=""
  IFS=';' read -r -a mount_entries <<< "${mounts_blob}"
  for entry in "${mount_entries[@]}"; do
    [[ -n "${entry}" ]] || continue
    local mount_type="${entry%%|*}"
    local mount_source="${entry#*|}"
    [[ "${mount_type}" == "bind" ]] || continue
    if [[ "${mount_source}" == "${repo_root}" || "${mount_source}" == "${repo_root}/"* ]]; then
      continue
    fi
    has_foreign_bind=1
    [[ -e "${mount_source}" ]] || has_missing_foreign_bind=1
  done

  [[ "${has_foreign_bind}" -eq 1 ]] || return 0

  if [[ "${running}" == "true" && "${has_missing_foreign_bind}" -ne 1 ]]; then
    echo "bootstrap_stack.sh: conflicting live container ${name} belongs to another repo root; stop it before installing Amai here." >&2
    exit 125
  fi

  docker rm -f "${name}" >/dev/null
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

  cleanup_conflicting_named_container "ami-postgres"
  cleanup_conflicting_named_container "ami-qdrant"
  cleanup_conflicting_named_container "ami-minio"
  cleanup_conflicting_named_container "ami-nats"
  if [[ "${stack_profile}" == "default" ]]; then
    cleanup_conflicting_named_container "ami-prometheus"
    cleanup_conflicting_named_container "ami-grafana"
  fi

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
export -f cleanup_conflicting_named_container
export -f bootstrap_main
flock --exclusive --close "${bootstrap_lock_file}" bash -lc 'bootstrap_main'
