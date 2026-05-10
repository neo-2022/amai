#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
docker_bin="./scripts/docker_wrapper.sh"

cleanup_conflicting_named_container() {
  local name="$1"
  local inspect_line=""
  inspect_line="$("${docker_bin}" inspect "${name}" --format '{{.State.Running}} {{range .Mounts}}{{.Type}}|{{.Source}};{{end}}' 2>/dev/null || true)"
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
    echo "prepare_stack_runtime.sh: reclaiming conflicting live container ${name} from another repo root." >&2
  fi

  "${docker_bin}" rm -f "${name}" >/dev/null
}

cleanup_same_repo_minio_credential_drift() {
  local inspect_env=""
  inspect_env="$("${docker_bin}" inspect ami-minio --format '{{range .Config.Env}}{{println .}}{{end}}' 2>/dev/null || true)"
  [[ -n "${inspect_env}" ]] || return 0

  local current_user="${AMI_S3_ACCESS_KEY:-}"
  local current_pass="${AMI_S3_SECRET_KEY:-}"
  [[ -n "${current_user}" && -n "${current_pass}" ]] || return 0

  local container_user=""
  local container_pass=""
  container_user="$(printf '%s\n' "${inspect_env}" | sed -n 's/^MINIO_ROOT_USER=//p' | head -n1)"
  container_pass="$(printf '%s\n' "${inspect_env}" | sed -n 's/^MINIO_ROOT_PASSWORD=//p' | head -n1)"

  [[ -n "${container_user}" && -n "${container_pass}" ]] || return 0
  if [[ "${container_user}" == "${current_user}" && "${container_pass}" == "${current_pass}" ]]; then
    return 0
  fi

  echo "prepare_stack_runtime.sh: reclaiming ami-minio because live container credentials drift from current .env." >&2
  "${docker_bin}" rm -f ami-minio >/dev/null
}

stack_profile="${AMI_STACK_PROFILE:-default}"

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
cleanup_same_repo_minio_credential_drift
if [[ "${stack_profile}" == "default" ]]; then
  cleanup_conflicting_named_container "ami-prometheus"
  cleanup_conflicting_named_container "ami-grafana"
fi

./scripts/render_nats_config.sh >/dev/null
./scripts/render_postgres_config.sh >/dev/null
