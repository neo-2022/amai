#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
output="$("${repo_root}/scripts/package_vscode_amai_bridge.sh")"
printf '%s\n' "${output}" | grep -F "package_vscode_amai_bridge: ok (" >/dev/null
vsix_path="$(printf '%s\n' "${output}" | sed -n 's/^package_vscode_amai_bridge: ok (\(.*\))$/\1/p')"
test -n "${vsix_path}"
test -f "${vsix_path}"

unzip -Z1 "${vsix_path}" | grep -F "extension/package.json" >/dev/null
unzip -Z1 "${vsix_path}" | grep -F "extension/media/amai-extension.png" >/dev/null

printf 'proof_vscode_amai_bridge_package: PASS\n'
