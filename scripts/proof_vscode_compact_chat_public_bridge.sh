#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_root="$(mktemp -d)"
trap 'rm -rf "${tmp_root}"' EXIT

extensions_root="${tmp_root}/extensions"
install_output="$(
  AMAI_VSCODE_EXTENSIONS_ROOT="${extensions_root}" \
    "${repo_root}/scripts/install_vscode_amai_bridge.sh"
)"

printf '%s\n' "${install_output}" | grep -F "install_vscode_amai_bridge: ok (" >/dev/null

bridge_dir="$(find "${extensions_root}" -maxdepth 1 -type d -name 'amai.amai-vscode-bridge-*' | head -n1)"
if [[ -z "${bridge_dir}" ]]; then
  echo "proof_vscode_compact_chat_public_bridge: installed bridge directory not found" >&2
  exit 1
fi

package_json="${bridge_dir}/package.json"
extension_js="${bridge_dir}/extension.js"
expected_version="$(jq -r '.version' "${package_json}")"

[[ -f "${package_json}" ]] || { echo "proof_vscode_compact_chat_public_bridge: missing ${package_json}" >&2; exit 1; }
[[ -f "${extension_js}" ]] || { echo "proof_vscode_compact_chat_public_bridge: missing ${extension_js}" >&2; exit 1; }

jq -e '
  .publisher == "amai"
  and .name == "amai-vscode-bridge"
  and (.activationEvents | index("onUri"))
  and (.activationEvents | index("onCommand:amaiVscodeBridge.openCleanChat"))
  and (.contributes.commands | any(.command == "amaiVscodeBridge.openCleanChat"))
' "${package_json}" >/dev/null

rg -F "registerUriHandler" "${extension_js}" >/dev/null
rg -F "chatgpt.openSidebar" "${extension_js}" >/dev/null
rg -F "chatgpt.newChat" "${extension_js}" >/dev/null
rg -F "chatgpt.newCodexPanel" "${extension_js}" >/dev/null
rg -F "executeCommand(\"type\"" "${extension_js}" >/dev/null
rg -F "launch_requested" "${extension_js}" >/dev/null
rg -F "vscode://amai.amai-vscode-bridge/open-clean-chat" "${bridge_dir}/README.md" >/dev/null

code --extensions-dir "${extensions_root}" --list-extensions --show-versions \
  | grep -Fq "amai.amai-vscode-bridge@${expected_version}"

printf 'proof_vscode_compact_chat_public_bridge: ok (%s)\n' "${bridge_dir}"
