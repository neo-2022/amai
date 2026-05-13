#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bridge_dir="${repo_root}/tools/vscode-amai-bridge"
package_json="${bridge_dir}/package.json"
openvsx_api_url="${AMAI_OPENVSX_API_URL:-https://open-vsx.org/api/amai/amai-vscode-bridge}"
mode="${1:-sync}"

if [[ ! -f "${package_json}" ]]; then
  echo "openvsx_bridge_sync: missing ${package_json}" >&2
  exit 1
fi

local_version="$(jq -r '.version // empty' "${package_json}")"
if [[ -z "${local_version}" ]]; then
  echo "openvsx_bridge_sync: failed to read local version from ${package_json}" >&2
  exit 1
fi

remote_payload="$(curl -fsSL "${openvsx_api_url}")"
remote_version="$(printf '%s' "${remote_payload}" | jq -r '.version // empty')"

if [[ -z "${remote_version}" ]]; then
  echo "openvsx_bridge_sync: failed to read remote OpenVSX version from ${openvsx_api_url}" >&2
  exit 1
fi

echo "openvsx_bridge_sync: local=${local_version} remote=${remote_version}"

if [[ "${local_version}" == "${remote_version}" ]]; then
  echo "openvsx_bridge_sync: versions are already in sync"
  exit 0
fi

if [[ "${mode}" == "check" || "${mode}" == "--check" || "${mode}" == "check-only" ]]; then
  echo "openvsx_bridge_sync: version drift detected (check-only mode)" >&2
  exit 2
fi

if [[ -z "${OVSX_TOKEN:-}" ]]; then
  echo "openvsx_bridge_sync: OVSX_TOKEN is required for publish mode" >&2
  exit 1
fi

if ! command -v npx >/dev/null 2>&1; then
  echo "openvsx_bridge_sync: npx is required (install Node.js/npm first)" >&2
  exit 1
fi

(
  cd "${bridge_dir}"
  npx --yes ovsx publish -p "${OVSX_TOKEN}"
)

for _ in $(seq 1 30); do
  sleep 2
  published_version="$(curl -fsSL "${openvsx_api_url}" | jq -r '.version // empty')"
  if [[ "${published_version}" == "${local_version}" ]]; then
    echo "openvsx_bridge_sync: published successfully (version=${published_version})"
    exit 0
  fi
done

echo "openvsx_bridge_sync: publish command finished, but OpenVSX still shows old version" >&2
exit 3
