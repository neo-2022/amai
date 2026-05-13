#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workspace_mcp="${repo_root}/.vscode/mcp.json"
user_code_mcp="${HOME}/.config/Code/User/mcp.json"
user_codium_mcp="${HOME}/.config/VSCodium/User/mcp.json"

has_amai_server() {
  local file="$1"
  [[ -f "${file}" ]] || return 1
  rg -n '"amai"\s*:' "${file}" >/dev/null 2>&1
}

user_has_amai() {
  has_amai_server "${user_code_mcp}" || has_amai_server "${user_codium_mcp}"
}

remove_amai_from_workspace_mcp() {
  [[ -f "${workspace_mcp}" ]] || return 0
  has_amai_server "${workspace_mcp}" || return 0

  if ! command -v jq >/dev/null 2>&1; then
    return 0
  fi

  local tmp_file
  tmp_file="$(mktemp)"
  jq 'if (.servers|type)=="object" then .servers |= with_entries(select(.key != "amai")) else . end' \
    "${workspace_mcp}" > "${tmp_file}"

  if jq -e '.servers|type=="object" and (keys|length)==0' "${tmp_file}" >/dev/null 2>&1; then
    rm -f "${workspace_mcp}"
    rm -f "${tmp_file}"
    return 0
  fi

  mv "${tmp_file}" "${workspace_mcp}"
}

main() {
  user_has_amai || exit 0
  remove_amai_from_workspace_mcp
}

main "$@"
