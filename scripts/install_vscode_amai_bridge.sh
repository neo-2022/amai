#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="${repo_root}/tools/vscode-amai-bridge"

if [[ ! -f "${source_dir}/package.json" || ! -f "${source_dir}/extension.js" ]]; then
  echo "install_vscode_amai_bridge: bridge source is incomplete under ${source_dir}" >&2
  exit 1
fi

extensions_root="${AMAI_VSCODE_EXTENSIONS_ROOT:-${HOME}/.vscode/extensions}"
package_json="${source_dir}/package.json"
publisher="$(jq -r '.publisher' "${package_json}")"
name="$(jq -r '.name' "${package_json}")"
version="$(jq -r '.version' "${package_json}")"

if [[ -z "${publisher}" || -z "${name}" || -z "${version}" || "${publisher}" == "null" || "${name}" == "null" || "${version}" == "null" ]]; then
  echo "install_vscode_amai_bridge: failed to read publisher/name/version from ${package_json}" >&2
  exit 1
fi

target_dir="${extensions_root}/${publisher}.${name}-${version}"
live_state_path="${repo_root}/.amai/onboarding/vscode-public-bridge-live-state.json"
extensions_registry_path="${extensions_root}/extensions.json"
mkdir -p "${extensions_root}"
lock_path="${extensions_root}/.amai-vscode-bridge.install.lock"
exec 9>"${lock_path}"
if command -v flock >/dev/null 2>&1; then
  flock 9
fi
if command -v rsync >/dev/null 2>&1; then
  mkdir -p "${target_dir}"
  rsync -a --delete "${source_dir}/" "${target_dir}/"
else
  mkdir -p "${target_dir}"
  cp -R "${source_dir}/." "${target_dir}/"
fi
find "${extensions_root}" -maxdepth 1 -type d -name "${publisher}.${name}-*" ! -path "${target_dir}" -exec rm -rf {} +
find "${extensions_root}" -maxdepth 1 \( -type d -o -type l \) -name "art-local.amai-vscode-bridge-*" -exec rm -rf {} +
if [[ -f "${extensions_registry_path}" ]]; then
  python3 - "${extensions_registry_path}" "${target_dir}" "${publisher}.${name}" "${version}" <<'PY'
import json
import pathlib
import sys

registry_path = pathlib.Path(sys.argv[1])
target_dir = pathlib.Path(sys.argv[2])
extension_id = sys.argv[3]
version = sys.argv[4]

raw = registry_path.read_text()
entries = json.loads(raw)
changed = False
target_name = target_dir.name
target_path = str(target_dir)

for entry in entries:
    identifier = entry.get("identifier", {})
    entry_id = identifier.get("id")
    if entry_id not in {"amai.amai-vscode-bridge", "art-local.amai-vscode-bridge"}:
        continue
    entry["identifier"] = {"id": extension_id}
    entry["version"] = version
    entry["relativeLocation"] = target_name
    location = dict(entry.get("location") or {})
    location["path"] = target_path
    location["fsPath"] = target_path
    location["external"] = target_dir.as_uri()
    location["scheme"] = "file"
    entry["location"] = location
    changed = True

if changed:
    registry_path.write_text(json.dumps(entries, ensure_ascii=False, indent=2) + "\n")
PY
fi
rm -f "${live_state_path}"

printf 'install_vscode_amai_bridge: ok (%s)\n' "${target_dir}"
