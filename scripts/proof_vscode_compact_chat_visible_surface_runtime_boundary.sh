#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
state_path="${repo_root}/.amai/onboarding/vscode-public-bridge-live-state.json"

rm -f "${state_path}"
"${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null
"${repo_root}/scripts/verify_vscode_compact_chat_public_bridge_live.sh" --record

jq -e '
  .status == "live_launch_verified"
  and .source_bundle_capabilities.visible_surface == true
  and .runtime_capability_drift.visible_surface_missing_from_runtime_result == true
  and .bridge_result.status == "launch_requested"
  and .bridge_result.public_bridge.authority == "amai.amai-vscode-bridge"
' "${state_path}" >/dev/null

printf 'proof_vscode_compact_chat_visible_surface_runtime_boundary: ok (%s)\n' "${state_path}"
