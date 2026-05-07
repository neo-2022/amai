#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
if [[ -f "${repo_root}/.amai/onboarding/project-chat-startup-contract.json" ]]; then
  ./scripts/sync_startup_contract_sha.sh >/dev/null
fi

has_ssh_destination=0
has_remote_repo_root=0
ssh_destination=""
remote_repo_root=""
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
  case "$arg" in
    --ssh-destination)
      has_ssh_destination=1
      prev_arg="--ssh-destination"
      ;;
    --ssh-destination=*)
      has_ssh_destination=1
      ssh_destination="${arg#--ssh-destination=}"
      ;;
    --remote-repo-root)
      has_remote_repo_root=1
      prev_arg="--remote-repo-root"
      ;;
    --remote-repo-root=*)
      has_remote_repo_root=1
      remote_repo_root="${arg#--remote-repo-root=}"
      ;;
  esac
done

if [[ "$has_ssh_destination" -ne 1 || "$has_remote_repo_root" -ne 1 ]]; then
  echo "onboard_remote_client.sh requires --ssh-destination and --remote-repo-root" >&2
  exit 1
fi

if [[ -z "${ssh_destination}" || -z "${remote_repo_root}" ]]; then
  echo "onboard_remote_client.sh requires non-empty --ssh-destination and --remote-repo-root values" >&2
  exit 1
fi

./scripts/sync_remote_repo.sh \
  --ssh-destination "${ssh_destination}" \
  --remote-repo-root "${remote_repo_root}"

exec ./scripts/amai_exec.sh bootstrap onboarding --skip-stack --skip-release-build "$@"
