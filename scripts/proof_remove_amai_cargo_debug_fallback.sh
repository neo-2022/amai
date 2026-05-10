#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_home="$(mktemp -d)"
trap 'rm -rf "${tmp_home}"' EXIT

managed_root="${tmp_home}/.local/share/amai/repo"
mkdir -p "${managed_root}/scripts" "${managed_root}/target/debug" "${managed_root}/target/release"
cp "${repo_root}/scripts/remove_amai.sh" "${managed_root}/scripts/remove_amai.sh"
touch "${managed_root}/Cargo.toml" "${managed_root}/Cargo.lock"

cat > "${managed_root}/scripts/resolve_cargo.sh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${HOME}/fake-cargo"
EOF
chmod +x "${managed_root}/scripts/resolve_cargo.sh"

cat > "${managed_root}/scripts/resolve_rustc.sh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${HOME}/fake-rustc"
EOF
chmod +x "${managed_root}/scripts/resolve_rustc.sh"

cat > "${tmp_home}/fake-cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" > "${HOME}/cargo-remove-args.txt"
EOF
chmod +x "${tmp_home}/fake-cargo"

cat > "${tmp_home}/fake-rustc" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "${tmp_home}/fake-rustc"

cat > "${managed_root}/scripts/amai_exec.sh" <<'EOF'
#!/usr/bin/env bash
echo "proof_remove_amai_cargo_debug_fallback: unexpectedly used amai_exec" >&2
exit 99
EOF
chmod +x "${managed_root}/scripts/amai_exec.sh"

(
  export HOME="${tmp_home}"
  export AMAI_GITHUB_CLONE_DIR="${managed_root}"
  cd "${managed_root}"
  ./scripts/remove_amai.sh --client vscode >/dev/null
)

grep -Fqx 'run --quiet -- bootstrap remove --client vscode' "${tmp_home}/cargo-remove-args.txt"

printf 'proof_remove_amai_cargo_debug_fallback: PASS\n'
