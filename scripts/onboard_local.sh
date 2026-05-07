#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
./scripts/cleanup_mcp_orphans.sh "${repo_root}" >/dev/null 2>&1 || true
if [[ -f "${repo_root}/.amai/onboarding/project-chat-startup-contract.json" ]]; then
  ./scripts/sync_startup_contract_sha.sh >/dev/null
fi

extra_args=()
has_skip_stack=0
has_skip_release_build=0
client=""
for arg in "$@"; do
  case "$arg" in
    --skip-stack)
      has_skip_stack=1
      ;;
    --skip-release-build)
      has_skip_release_build=1
      ;;
  esac
done

remaining_args=("$@")
for ((i=0; i<${#remaining_args[@]}; i++)); do
  case "${remaining_args[$i]}" in
    --client)
      if [[ $((i + 1)) -lt ${#remaining_args[@]} ]]; then
        client="${remaining_args[$((i + 1))]}"
      fi
      ;;
    --client=*)
      client="${remaining_args[$i]#--client=}"
      ;;
  esac
done

if [[ -f "state/install_state.json" && -x "target/release/amai" ]]; then
  if [[ ${has_skip_stack} -eq 0 ]]; then
    extra_args+=(--skip-stack)
  fi
  if [[ ${has_skip_release_build} -eq 0 ]]; then
    extra_args+=(--skip-release-build)
  fi
fi

./scripts/amai_exec.sh bootstrap onboarding "${extra_args[@]}" "$@"

if [[ "${client}" == "vscode" ]]; then
  ./scripts/install_vscode_amai_bridge.sh >/dev/null
fi
