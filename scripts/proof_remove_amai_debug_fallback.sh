#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_home="$(mktemp -d)"
trap 'rm -rf "${tmp_home}"' EXIT

managed_root="${tmp_home}/.local/share/amai/repo"
mkdir -p "${managed_root}/scripts" "${managed_root}/target/debug" "${managed_root}/target/release" "${managed_root}/src" "${managed_root}/sql"
cp "${repo_root}/scripts/remove_amai.sh" "${managed_root}/scripts/remove_amai.sh"
touch "${managed_root}/Cargo.toml" "${managed_root}/Cargo.lock"

cat > "${managed_root}/target/debug/amai" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" > "${HOME}/debug-remove-args.txt"
EOF
chmod +x "${managed_root}/target/debug/amai"

cat > "${managed_root}/scripts/amai_exec.sh" <<'EOF'
#!/usr/bin/env bash
echo "proof_remove_amai_debug_fallback: unexpectedly used amai_exec" >&2
exit 99
EOF
chmod +x "${managed_root}/scripts/amai_exec.sh"

(
  export HOME="${tmp_home}"
  export AMAI_GITHUB_CLONE_DIR="${managed_root}"
  cd "${managed_root}"
  ./scripts/remove_amai.sh --client vscode >/dev/null
)

grep -Fqx 'bootstrap remove --client vscode' "${tmp_home}/debug-remove-args.txt"

printf 'proof_remove_amai_debug_fallback: PASS\n'
