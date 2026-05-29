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
    echo "proof_vscode_compact_chat_external_bridge_boundary: missing file $path" >&2
    exit 1
  fi
}

CODE_HELP="$(code --help 2>&1 || true)"
if [[ -z "${CODE_HELP}" ]]; then
  echo "proof_vscode_compact_chat_external_bridge_boundary: unable to read code --help" >&2
  exit 1
fi

if grep -Fq -- "--command" <<<"${CODE_HELP}"; then
  echo "proof_vscode_compact_chat_external_bridge_boundary: VS Code CLI now exposes --command; boundary proof is stale and must be re-audited" >&2
  exit 1
fi

EXT_DIR="$(find_latest_extension_dir || true)"
if [[ -z "${EXT_DIR}" ]]; then
  echo "proof_vscode_compact_chat_external_bridge_boundary: openai.chatgpt extension is not installed under ${EXTENSIONS_ROOT}" >&2
  exit 1
fi

EXTENSION_JS="${EXT_DIR}/out/extension.js"
WEBVIEW_ASSET="$(find "${EXT_DIR}/webview/assets" -maxdepth 1 -type f -name 'use-start-new-conversation-*.js' | head -n1)"

require_file "${EXTENSION_JS}"
require_file "${WEBVIEW_ASSET}"

rg -F 'handleUri(e)' "${EXTENSION_JS}" >/dev/null
rg -F 'let r=e.path||"/"' "${EXTENSION_JS}" >/dev/null
rg -F 'navigateToRoute(r)' "${EXTENSION_JS}" >/dev/null

rg -F 'open-vscode-command' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'shared-object-set' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'composer_prefill' "${WEBVIEW_ASSET}" >/dev/null
rg -F 'prefillPrompt' "${WEBVIEW_ASSET}" >/dev/null

printf 'proof_vscode_compact_chat_external_bridge_boundary: ok (%s)\n' "${EXT_DIR}"
