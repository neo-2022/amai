#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="${repo_root}/tools/vscode-amai-bridge"

if [[ ! -f "${source_dir}/package.json" || ! -f "${source_dir}/extension.js" ]]; then
  echo "install_vscode_amai_bridge: bridge source is incomplete under ${source_dir}" >&2
  exit 1
fi

detect_vscode_extensions_root() {
  local reason="default"
  if [[ -n "${AMAI_VSCODE_EXTENSIONS_ROOT:-}" ]]; then
    reason="override_env"
    printf '%s\t%s\n' "${AMAI_VSCODE_EXTENSIONS_ROOT}" "${reason}"
    return 0
  fi
  local code_path code_realpath
  code_path="${AMAI_VSCODE_CLI_BIN:-}"
  if [[ -z "${code_path}" ]]; then
    for candidate in code codium code-oss; do
      code_path="$(command -v "${candidate}" 2>/dev/null || true)"
      [[ -n "${code_path}" ]] && break
    done
  fi
  if [[ -z "${code_path}" && -x "${HOME}/.local/bin/code" ]]; then
    code_path="${HOME}/.local/bin/code"
  fi
  if [[ -z "${code_path}" && -x "${HOME}/.local/bin/codium" ]]; then
    code_path="${HOME}/.local/bin/codium"
  fi
  code_realpath=""
  if [[ -n "${code_path}" ]]; then
    code_realpath="$(readlink -f "${code_path}" 2>/dev/null || printf '%s' "${code_path}")"
  fi

  # VS Code snap stores extensions under ~/snap/code/(common|current)/.vscode/extensions.
  # The directory may not exist until the first VS Code launch; still use the canonical snap target.
  if [[ "${code_realpath}" == *"/snap/"* ]] || ( [[ -x /usr/bin/snap ]] && snap list code >/dev/null 2>&1 ); then
    reason="vscode_snap"
    if [[ -d "${HOME}/snap/code/common/.vscode" || -d "${HOME}/snap/code/common" ]]; then
      printf '%s\t%s\n' "${HOME}/snap/code/common/.vscode/extensions" "${reason}"
      return 0
    fi
    if [[ -d "${HOME}/snap/code/current/.vscode" || -d "${HOME}/snap/code/current" ]]; then
      printf '%s\t%s\n' "${HOME}/snap/code/current/.vscode/extensions" "${reason}"
      return 0
    fi
    printf '%s\t%s\n' "${HOME}/snap/code/common/.vscode/extensions" "${reason}"
    return 0
  fi

  if [[ "${code_realpath}" == *codium* || "${code_realpath}" == *VSCodium* ]]; then
    reason="codium_or_oss"
    printf '%s\t%s\n' "${HOME}/.vscode-oss/extensions" "${reason}"
    return 0
  fi
  if [[ -z "${code_realpath}" && ( -d "${HOME}/.config/VSCodium" || -d "${HOME}/.vscode-oss" ) ]]; then
    reason="codium_or_oss_marker_only"
    printf '%s\t%s\n' "${HOME}/.vscode-oss/extensions" "${reason}"
    return 0
  fi
  printf '%s\t%s\n' "${HOME}/.vscode/extensions" "${reason}"
}

extensions_root_with_reason="$(detect_vscode_extensions_root)"
extensions_root="${extensions_root_with_reason%%$'\t'*}"
extensions_root_reason="${extensions_root_with_reason#*$'\t'}"
if [[ -n "${extensions_root_reason}" && "${extensions_root_reason}" != "${extensions_root_with_reason}" ]]; then
  printf 'install_vscode_amai_bridge: detected extensions root: %s (reason=%s, code=%s)\n' \
    "${extensions_root}" "${extensions_root_reason}" "$(command -v code 2>/dev/null || true)" >&2
else
  printf 'install_vscode_amai_bridge: detected extensions root: %s (reason=unknown, code=%s)\n' \
    "${extensions_root}" "$(command -v code 2>/dev/null || true)" >&2
fi
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
