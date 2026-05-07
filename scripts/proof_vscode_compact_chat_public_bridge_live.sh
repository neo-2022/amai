#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
state_path="${repo_root}/.amai/onboarding/vscode-public-bridge-live-state.json"

rm -f "${state_path}"
"${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null
"${repo_root}/scripts/verify_vscode_compact_chat_public_bridge_live.sh" --record

jq_expected_version="$(jq -r '.version' "${repo_root}/tools/vscode-amai-bridge/package.json")"

jq -e '
  .status == "live_launch_verified"
  and .public_bridge.authority == "amai.amai-vscode-bridge"
  and .public_bridge.version == $expected_version
  and .ui_cleanup.success == true
  and .ui_cleanup.uri_cleanup_requested == true
  and .ui_cleanup.matching_tabs_after == 0
  and .ui_cleanup.active_editor_matches_bridge_uri_after == false
' --arg expected_version "${jq_expected_version}" "${state_path}" >/dev/null

printf 'proof_vscode_compact_chat_public_bridge_live: ok (%s)\n' "${state_path}"
