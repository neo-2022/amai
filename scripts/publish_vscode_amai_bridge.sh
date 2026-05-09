#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      target="${2:-}"
      shift 2
      ;;
    --target=*)
      target="${1#*=}"
      shift
      ;;
    *)
      echo "publish_vscode_amai_bridge: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "${target}" ]]; then
  echo "publish_vscode_amai_bridge: missing --target marketplace|openvsx" >&2
  exit 2
fi

command -v npx >/dev/null 2>&1 || {
  echo "publish_vscode_amai_bridge: missing npx" >&2
  exit 1
}

package_output="$("${repo_root}/scripts/package_vscode_amai_bridge.sh")"
printf '%s\n' "${package_output}"
vsix_path="$(printf '%s\n' "${package_output}" | sed -n 's/^package_vscode_amai_bridge: ok (\(.*\))$/\1/p')"

if [[ -z "${vsix_path}" || ! -f "${vsix_path}" ]]; then
  echo "publish_vscode_amai_bridge: failed to locate VSIX artifact after packaging" >&2
  exit 1
fi

case "${target}" in
  marketplace)
    token="${VSCE_PAT:-${MARKETPLACE_TOKEN:-}}"
    if [[ -z "${token}" ]]; then
      echo "publish_vscode_amai_bridge: missing marketplace publish token (set VSCE_PAT or MARKETPLACE_TOKEN)" >&2
      exit 1
    fi
    npx --yes @vscode/vsce publish --packagePath "${vsix_path}" -p "${token}"
    ;;
  openvsx)
    token="${OVSX_PAT:-${OPENVSX_TOKEN:-}}"
    if [[ -z "${token}" ]]; then
      echo "publish_vscode_amai_bridge: missing OpenVSX publish token (set OVSX_PAT or OPENVSX_TOKEN)" >&2
      exit 1
    fi
    npx --yes ovsx publish "${vsix_path}" -p "${token}"
    ;;
  *)
    echo "publish_vscode_amai_bridge: unsupported target: ${target}" >&2
    exit 2
    ;;
esac
