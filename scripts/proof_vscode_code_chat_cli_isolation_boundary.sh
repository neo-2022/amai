#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_root="$(mktemp -d)"
trap 'rm -rf "${tmp_root}"' EXIT

user_dir="${tmp_root}/data"
ext_dir="${tmp_root}/extensions"
mkdir -p "${user_dir}" "${ext_dir}"

chatgpt_bundle="$(find "${HOME}/.vscode/extensions" -maxdepth 1 -type d -name 'openai.chatgpt-*' | sort | tail -n1)"
if [[ -z "${chatgpt_bundle}" || ! -d "${chatgpt_bundle}" ]]; then
  echo "proof_vscode_code_chat_cli_isolation_boundary: openai.chatgpt bundle not found" >&2
  exit 1
fi

bridge_version="$(jq -r '.version' "${repo_root}/tools/vscode-amai-bridge/package.json")"
cp -R "${chatgpt_bundle}" "${ext_dir}/"
cp -R "${repo_root}/tools/vscode-amai-bridge" "${ext_dir}/amai.amai-vscode-bridge-${bridge_version}"

profile_name="$(
  python3 - <<'PY'
import hashlib
repo='/home/art/agent-memory-index'
label='agent-memory-index'
print(f"Amai-{label}-{hashlib.sha256(repo.encode()).hexdigest()[:12]}")
PY
)"

prompt_file="${tmp_root}/prompt.txt"
stdout_file="${tmp_root}/stdout.txt"
stderr_file="${tmp_root}/stderr.txt"
printf 'isolated code chat boundary probe %s\n' "$(date -Is)" > "${prompt_file}"

code --user-data-dir "${user_dir}" --extensions-dir "${ext_dir}" chat --mode agent --new-window --profile "${profile_name}" - < "${prompt_file}" >"${stdout_file}" 2>"${stderr_file}" || true
sleep 8

grep -Fq "Warning: 'user-data-dir' is not in the list of known options for subcommand 'chat'" "${stderr_file}"
grep -Fq "Warning: 'extensions-dir' is not in the list of known options for subcommand 'chat'" "${stderr_file}"

if find "${user_dir}" -type f -path '*window*/exthost/exthost.log' | grep -q .; then
  echo "proof_vscode_code_chat_cli_isolation_boundary: isolated user-data-dir unexpectedly received exthost logs" >&2
  exit 1
fi

printf 'proof_vscode_code_chat_cli_isolation_boundary: ok (%s)\n' "${tmp_root}"
