#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bridge_dir="${repo_root}/tools/vscode-amai-bridge"
package_json="${bridge_dir}/package.json"

command -v jq >/dev/null 2>&1 || {
  echo "package_vscode_amai_bridge: missing jq" >&2
  exit 1
}
command -v npx >/dev/null 2>&1 || {
  echo "package_vscode_amai_bridge: missing npx" >&2
  exit 1
}

name="$(jq -r '.name // empty' "${package_json}")"
version="$(jq -r '.version // empty' "${package_json}")"
publisher="$(jq -r '.publisher // empty' "${package_json}")"
icon_path="$(jq -r '.icon // empty' "${package_json}")"

if [[ -z "${name}" || -z "${version}" || -z "${publisher}" ]]; then
  echo "package_vscode_amai_bridge: missing package metadata in ${package_json}" >&2
  exit 1
fi

if [[ -z "${icon_path}" || ! -f "${bridge_dir}/${icon_path}" ]]; then
  echo "package_vscode_amai_bridge: missing icon asset ${icon_path}" >&2
  exit 1
fi

case "${icon_path}" in
  *.png|*.jpg|*.jpeg) ;;
  *)
    echo "package_vscode_amai_bridge: extension icon must be PNG or JPEG for marketplace packaging" >&2
    exit 1
    ;;
esac

dist_dir="${repo_root}/dist"
mkdir -p "${dist_dir}"
vsix_path="${dist_dir}/${publisher}.${name}-${version}.vsix"
rm -f "${vsix_path}"

(
  cd "${bridge_dir}"
  npx --yes @vscode/vsce package --out "${vsix_path}"
)

printf 'package_vscode_amai_bridge: ok (%s)\n' "${vsix_path}"
