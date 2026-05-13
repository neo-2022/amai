#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

temp_home="${tmpdir}/home"
fake_bin="${tmpdir}/bin"
mkdir -p "${temp_home}" "${fake_bin}"

cat > "${fake_bin}/codium" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "${fake_bin}/codium"
ln -s "${fake_bin}/codium" "${fake_bin}/code"

env \
  HOME="${temp_home}" \
  PATH="${fake_bin}:${PATH}" \
  "${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null

expected_version="$(jq -r '.version' "${repo_root}/tools/vscode-amai-bridge/package.json")"
target_dir="${temp_home}/.vscode-oss/extensions/amai.amai-vscode-bridge-${expected_version}"

test -d "${target_dir}"
test -f "${target_dir}/package.json"
test ! -e "${temp_home}/.vscode/extensions/amai.amai-vscode-bridge-${expected_version}"

printf 'proof_vscode_amai_bridge_codium_root: PASS\n'
