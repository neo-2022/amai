#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
state_path="${repo_root}/.amai/onboarding/vscode-public-bridge-live-state.json"

run_refresh_probe() {
  local label="$1"
  local uri="$2"
  local sleep_seconds="$3"

  rm -f "${state_path}"
  code --open-url "${uri}" >/dev/null 2>&1
  sleep "${sleep_seconds}"
  "${repo_root}/scripts/verify_vscode_compact_chat_public_bridge_live.sh" --record >/dev/null

  jq -e '
    .status == "live_launch_verified"
    and .source_bundle_capabilities.visible_surface == true
    and .runtime_capability_drift.visible_surface_missing_from_runtime_result == true
    and .bridge_result.status == "launch_requested"
  ' "${state_path}" >/dev/null

  printf 'proof_vscode_compact_chat_runtime_refresh_boundary: %s keeps visible_surface runtime drift (%s)\n' "${label}" "${uri}"
}

"${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null
run_refresh_probe "restartExtensionHost" "command:workbench.action.restartExtensionHost" 5
run_refresh_probe "reloadWindow" "command:workbench.action.reloadWindow" 8

printf 'proof_vscode_compact_chat_runtime_refresh_boundary: ok (%s)\n' "${state_path}"
