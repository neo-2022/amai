#!/usr/bin/env bash
set -euo pipefail

EXTENSIONS_ROOT="${HOME}/.vscode/extensions"

find_latest_extension_dir() {
  shopt -s nullglob
  local matches=("${EXTENSIONS_ROOT}"/openai.chatgpt-*)
  shopt -u nullglob
  if [[ ${#matches[@]} -eq 0 ]]; then
    return 1
  fi
  ls -dt "${matches[@]}" 2>/dev/null | head -n1
}

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "proof_vscode_compact_chat_extension_bridge: missing file $path" >&2
    exit 1
  fi
}

EXT_DIR="$(find_latest_extension_dir || true)"
if [[ -z "${EXT_DIR}" ]]; then
  echo "proof_vscode_compact_chat_extension_bridge: openai.chatgpt extension is not installed under ${EXTENSIONS_ROOT}" >&2
  exit 1
fi

PACKAGE_JSON="${EXT_DIR}/package.json"
EXTENSION_JS="${EXT_DIR}/out/extension.js"
WEBVIEW_ASSET="$(find "${EXT_DIR}/webview/assets" -maxdepth 1 -type f -name 'use-start-new-conversation-*.js' | head -n1)"

require_file "${PACKAGE_JSON}"
require_file "${EXTENSION_JS}"
require_file "${WEBVIEW_ASSET}"

rg -F '"command": "chatgpt.newChat"' "${PACKAGE_JSON}" >/dev/null
rg -F '"command": "chatgpt.newCodexPanel"' "${PACKAGE_JSON}" >/dev/null

rg -F 'Y2e="chatgpt.newChat"' "${EXTENSION_JS}" >/dev/null
rg -F 'X2e="chatgpt.newCodexPanel"' "${EXTENSION_JS}" >/dev/null
rg -F 'triggerNewChatViaWebview()' "${EXTENSION_JS}" >/dev/null
rg -F 'createNewPanel()' "${EXTENSION_JS}" >/dev/null

rg -F 'open-vscode-command' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'chatgpt.newChat' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'chatgpt.newCodexPanel' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'shared-object-set' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'composer_prefill' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'prefillPrompt' "${WEBVIEW_ASSET}" >/dev/null

printf 'proof_vscode_compact_chat_extension_bridge: ok (%s)\n' "${EXT_DIR}"
