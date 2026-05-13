#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
extensions_root="${HOME}/.vscode/extensions"
expected_version="$(jq -r '.version' "${repo_root}/tools/vscode-amai-bridge/package.json")"
target_dir="${extensions_root}/amai.amai-vscode-bridge-${expected_version}"

latest_sharedprocess_log() {
  find "${HOME}/.config/Code/logs" -type f -path '*sharedprocess.log' | sort | tail -n1
}

sharedprocess_log="$(latest_sharedprocess_log)"
if [[ -z "${sharedprocess_log}" || ! -f "${sharedprocess_log}" ]]; then
  echo "proof_vscode_amai_bridge_install_live_safe: sharedprocess.log not found" >&2
  exit 1
fi

before_lines="$(wc -l < "${sharedprocess_log}" | tr -d ' ')"
"${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null

if [[ ! -f "${target_dir}/package.json" ]]; then
  echo "proof_vscode_amai_bridge_install_live_safe: missing ${target_dir}/package.json after install" >&2
  exit 1
fi

after_lines="$(wc -l < "${sharedprocess_log}" | tr -d ' ')"
if (( after_lines > before_lines )); then
  if tail -n +"$((before_lines + 1))" "${sharedprocess_log}" \
      | rg -Fq "Unable to read file '${target_dir}/package.json'"; then
    echo "proof_vscode_amai_bridge_install_live_safe: install still caused missing bridge package.json in running VS Code host" >&2
    exit 1
  fi
fi

printf 'proof_vscode_amai_bridge_install_live_safe: ok (%s)\n' "${target_dir}"
