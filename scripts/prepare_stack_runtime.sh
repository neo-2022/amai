#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

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
    echo "prepare_stack_runtime.sh: conflicting live container ${name} belongs to another repo root; stop it before installing Amai here." >&2
    exit 125
  fi

  docker rm -f "${name}" >/dev/null
}

stack_profile="${AMI_STACK_PROFILE:-default}"

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
