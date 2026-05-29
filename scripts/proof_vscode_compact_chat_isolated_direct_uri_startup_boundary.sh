#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_root="$(mktemp -d)"
cleanup() {
  if [[ -n "${user_dir:-}" ]]; then
    "${repo_root}/scripts/close_vscode_temp_host.sh" "${user_dir}" >/dev/null 2>&1 || true
  fi
  rm -rf "${tmp_root}"
}
trap cleanup EXIT

ext_dir="${tmp_root}/extensions"
user_dir="${tmp_root}/data"
mkdir -p "${ext_dir}" "${user_dir}"

chatgpt_bundle="$(find "${HOME}/.vscode/extensions" -maxdepth 1 -type d -name 'openai.chatgpt-*' | sort | tail -n1)"
if [[ -z "${chatgpt_bundle}" || ! -d "${chatgpt_bundle}" ]]; then
  echo "proof_vscode_compact_chat_isolated_direct_uri_startup_boundary: openai.chatgpt bundle not found" >&2
  exit 1
fi

bridge_version="$(jq -r '.version' "${repo_root}/tools/vscode-amai-bridge/package.json")"
cp -R "${chatgpt_bundle}" "${ext_dir}/"
cp -R "${repo_root}/tools/vscode-amai-bridge" "${ext_dir}/amai.amai-vscode-bridge-${bridge_version}"

prompt_file="${tmp_root}/prompt.txt"
result_file="${tmp_root}/result.json"
printf 'isolated direct startup uri boundary probe %s\n' "$(date -Is)" > "${prompt_file}"
uri="vscode://amai.amai-vscode-bridge/open-clean-chat?prompt_file=$(jq -rn --arg v "${prompt_file}" '$v|@uri')&result_file=$(jq -rn --arg v "${result_file}" '$v|@uri')&repo_root=$(jq -rn --arg v "${repo_root}" '$v|@uri')&target=sidebar&auto_submit=0"

code --user-data-dir "${user_dir}" --extensions-dir "${ext_dir}" --open-url "${uri}" >/dev/null 2>&1 || true
sleep 15

if [[ -f "${result_file}" ]]; then
  echo "proof_vscode_compact_chat_isolated_direct_uri_startup_boundary: isolated direct startup unexpectedly wrote bridge result file" >&2
  cat "${result_file}" >&2
  exit 1
fi

exthost_log="$(find "${user_dir}/logs" -type f -path '*window*/exthost/exthost.log' | sort | tail -n1)"
codex_log="$(find "${user_dir}/logs" -type f -path '*window*/exthost/openai.chatgpt/Codex.log' | sort | tail -n1)"

if [[ -z "${exthost_log}" || ! -f "${exthost_log}" ]]; then
  echo "proof_vscode_compact_chat_isolated_direct_uri_startup_boundary: missing exthost log" >&2
  exit 1
fi
if [[ -z "${codex_log}" || ! -f "${codex_log}" ]]; then
  echo "proof_vscode_compact_chat_isolated_direct_uri_startup_boundary: missing Codex log" >&2
  exit 1
fi

rg -Fq "ExtensionService#_doActivateExtension openai.chatgpt" "${exthost_log}"

printf 'proof_vscode_compact_chat_isolated_direct_uri_startup_boundary: ok (%s)\n' "${tmp_root}"
