#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
package_json="${repo_root}/tools/vscode-amai-bridge/package.json"
extension_js="${repo_root}/tools/vscode-amai-bridge/extension.js"
icon_path="${repo_root}/tools/vscode-amai-bridge/media/amai-activity.svg"
extension_icon_path="${repo_root}/tools/vscode-amai-bridge/media/amai-extension.svg"

test -f "${icon_path}"
test -f "${extension_icon_path}"
node --check "${extension_js}"

jq -e '.contributes.viewsContainers.activitybar[] | select(.id == "amai" and .icon == "media/amai-activity.svg")' "${package_json}" >/dev/null
jq -e '.icon == "media/amai-extension.png"' "${package_json}" >/dev/null
jq -e '.contributes.views.amai[] | select(.id == "amai.sidebar" and .type == "webview")' "${package_json}" >/dev/null
jq -e '.contributes.commands[] | select(.command == "amaiVscodeBridge.openWorkspaceSidebarChat")' "${package_json}" >/dev/null
jq -e '.contributes.commands[] | select(.command == "amaiVscodeBridge.openWorkspacePanelChat")' "${package_json}" >/dev/null
jq -e '.contributes.commands[] | select(.command == "amaiVscodeBridge.openManagedRepoWorkspace")' "${package_json}" >/dev/null
jq -e '.contributes.commands[] | select(.command == "amaiVscodeBridge.openOpenAiExtension")' "${package_json}" >/dev/null
jq -e '.contributes.commands[] | select(.command == "amaiVscodeBridge.reloadWindow")' "${package_json}" >/dev/null
grep -Fq 'enableCommandUris: true' "${extension_js}"
grep -Fq 'Откройте именно Amai workspace' "${extension_js}"
grep -Fq 'Открыть Amai workspace' "${extension_js}"
grep -Fq 'Открыть OpenAI extension' "${extension_js}"
grep -Fq 'Сначала закройте шаги установки ниже' "${extension_js}"
grep -Fq 'OpenAI extension с поверхностью Codex/ChatGPT' "${extension_js}"
grep -Fq 'renderStatusBadge' "${extension_js}"
grep -Fq 'showErrorMessage(`Amai launch failed:' "${extension_js}"
grep -Fq 'non_bridge_tab_labels: nonBridgeTabLabels' "${extension_js}"

printf 'proof_vscode_amai_bridge_ui_surface: PASS\n'
