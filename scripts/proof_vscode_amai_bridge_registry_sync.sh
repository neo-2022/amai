#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_root="$(mktemp -d)"
trap 'rm -rf "${tmp_root}"' EXIT

extensions_root="${tmp_root}/extensions"
mkdir -p "${extensions_root}"

cat > "${extensions_root}/extensions.json" <<'JSON'
[
  {
    "identifier": {
      "id": "amai.amai-vscode-bridge"
    },
    "version": "0.0.2",
    "location": {
      "$mid": 1,
      "fsPath": "/tmp/stale/art-local.amai-vscode-bridge-0.0.3",
      "external": "file:///tmp/stale/art-local.amai-vscode-bridge-0.0.3",
      "path": "/tmp/stale/art-local.amai-vscode-bridge-0.0.3",
      "scheme": "file"
    },
    "relativeLocation": "art-local.amai-vscode-bridge-0.0.3"
  }
]
JSON

ln -s /tmp/stale "${extensions_root}/art-local.amai-vscode-bridge-0.0.3"

AMAI_VSCODE_EXTENSIONS_ROOT="${extensions_root}" \
  "${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null

expected_version="$(jq -r '.version' "${repo_root}/tools/vscode-amai-bridge/package.json")"
target_dir="${extensions_root}/amai.amai-vscode-bridge-${expected_version}"

[[ -f "${target_dir}/package.json" ]] || {
  echo "proof_vscode_amai_bridge_registry_sync: missing ${target_dir}/package.json" >&2
  exit 1
}

[[ ! -e "${extensions_root}/art-local.amai-vscode-bridge-0.0.3" ]] || {
  echo "proof_vscode_amai_bridge_registry_sync: stale art-local bridge alias still exists" >&2
  exit 1
}

jq -e --arg version "${expected_version}" --arg path "${target_dir}" --arg rel "amai.amai-vscode-bridge-${expected_version}" '
  length == 1
  and .[0].identifier.id == "amai.amai-vscode-bridge"
  and .[0].version == $version
  and .[0].relativeLocation == $rel
  and .[0].location.path == $path
  and .[0].location.fsPath == $path
' "${extensions_root}/extensions.json" >/dev/null

printf 'proof_vscode_amai_bridge_registry_sync: PASS (%s)\n' "${target_dir}"
