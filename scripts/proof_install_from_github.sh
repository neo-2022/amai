#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

clone_dir="${tmp_dir}/clone"
state_file="${tmp_dir}/install-state.json"
target_file="${tmp_dir}/mcp.json"
install_out="${tmp_dir}/install.out"

AMAI_INSTALL_STATE_PATH="${state_file}" ./scripts/install_from_github.sh \
  --repo-url "${repo_root}" \
  --clone-dir "${clone_dir}" \
  --client vscode \
  --stack-profile default \
  --yes \
  --skip-stack \
  --output "${target_file}" \
  >"${install_out}"

test -d "${clone_dir}/.git"
test -f "${clone_dir}/scripts/install_amai.sh"
test -f "${target_file}"
test -f "${state_file}"
rg '^Amai готов$' "${install_out}" >/dev/null
rg '^Результат: Amai установлен впервые\.$' "${install_out}" >/dev/null
rg '^Клиент: VS Code$' "${install_out}" >/dev/null
rg '^Machine-readable startup contract:' "${install_out}" >/dev/null

AMAI_INSTALL_STATE_PATH="${state_file}" ./scripts/install_from_github.sh \
  --repo-url "${repo_root}" \
  --clone-dir "${clone_dir}" \
  --client vscode \
  --stack-profile default \
  --yes \
  --skip-stack \
  --skip-release-build \
  --output "${target_file}" \
  >>"${install_out}"

test "$(rg -o '"amai"' "${target_file}" | wc -l | tr -d ' ')" = "1"
rg '^Результат: Amai уже был установлен\. Обновление не требовалось; текущая версия уже актуальна\.$' "${install_out}" >/dev/null

echo "proof_install_from_github: ok"
