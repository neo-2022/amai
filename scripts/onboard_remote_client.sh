#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

has_ssh_destination=0
has_remote_repo_root=0
for arg in "$@"; do
  case "$arg" in
    --ssh-destination)
      has_ssh_destination=1
      ;;
    --remote-repo-root)
      has_remote_repo_root=1
      ;;
  esac
done

if [[ "$has_ssh_destination" -ne 1 || "$has_remote_repo_root" -ne 1 ]]; then
  echo "onboard_remote_client.sh requires --ssh-destination and --remote-repo-root" >&2
  exit 1
fi

exec cargo run -- bootstrap onboarding --skip-stack --skip-release-build "$@"
