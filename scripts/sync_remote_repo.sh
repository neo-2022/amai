#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

ssh_destination=""
remote_repo_root=""
list_only=0
prev_arg=""

for arg in "$@"; do
  if [[ -n "${prev_arg}" ]]; then
    case "${prev_arg}" in
      --ssh-destination)
        ssh_destination="${arg}"
        ;;
      --remote-repo-root)
        remote_repo_root="${arg}"
        ;;
    esac
    prev_arg=""
    continue
  fi

  case "${arg}" in
    --ssh-destination)
      prev_arg="--ssh-destination"
      ;;
    --ssh-destination=*)
      ssh_destination="${arg#--ssh-destination=}"
      ;;
    --remote-repo-root)
      prev_arg="--remote-repo-root"
      ;;
    --remote-repo-root=*)
      remote_repo_root="${arg#--remote-repo-root=}"
      ;;
    --list)
      list_only=1
      ;;
    *)
      echo "sync_remote_repo.sh: unsupported argument: ${arg}" >&2
      exit 1
      ;;
  esac
done

write_payload_manifest() {
  (
    cd "${repo_root}"
    find . \
      \( \
        -path './.git' -o \
        -path './.fastembed_cache' -o \
        -path './target' -o \
        -path './output' -o \
        -path './state' -o \
        -path './tmp' \
      \) -prune -o -mindepth 1 -print0
  ) | while IFS= read -r -d '' path; do
    printf '%s\0' "${path#./}"
  done
}

if [[ "${list_only}" == "1" ]]; then
  write_payload_manifest | tr '\0' '\n'
  exit 0
fi

if [[ -z "${ssh_destination}" || -z "${remote_repo_root}" ]]; then
  echo "sync_remote_repo.sh requires --ssh-destination and --remote-repo-root" >&2
  exit 1
fi

if ! command -v ssh >/dev/null 2>&1; then
  echo "sync_remote_repo.sh requires ssh" >&2
  exit 1
fi

ssh "${ssh_destination}" "bash -lc '
set -euo pipefail
mkdir -p \"${remote_repo_root}\"
cleanup_paths=(
  \"${remote_repo_root}/.fastembed_cache\"
  \"${remote_repo_root}/output\"
  \"${remote_repo_root}/target\"
  \"${remote_repo_root}/state\"
  \"${remote_repo_root}/tmp\"
)
if rm -rf \"\${cleanup_paths[@]}\" 2>/dev/null; then
  exit 0
fi
if command -v podman >/dev/null 2>&1; then
  podman unshare rm -rf \"\${cleanup_paths[@]}\"
  exit 0
fi
echo \"sync_remote_repo.sh: failed to remove live runtime directories under ${remote_repo_root}\" >&2
exit 1
'"

tmp_manifest="$(mktemp)"
trap 'rm -f "${tmp_manifest}"' EXIT
write_payload_manifest >"${tmp_manifest}"

if [[ ! -s "${tmp_manifest}" ]]; then
  echo "sync_remote_repo.sh: repo payload manifest is empty" >&2
  exit 1
fi

tar -C "${repo_root}" --null -T "${tmp_manifest}" -cf - \
  | ssh "${ssh_destination}" "cd '${remote_repo_root}' && tar -xf -"

ssh "${ssh_destination}" "test -f '${remote_repo_root}/Cargo.toml' && test -f '${remote_repo_root}/scripts/run_mcp_stdio.sh'"
