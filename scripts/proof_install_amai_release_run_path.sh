#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

mkdir -p "${tmpdir}/scripts"
cp "${repo_root}/scripts/install_amai.sh" "${tmpdir}/scripts/install_amai.sh"
cp "${repo_root}/scripts/ensure_verified_linux_prereqs.sh" "${tmpdir}/scripts/ensure_verified_linux_prereqs.sh"

cat > "${tmpdir}/scripts/resolve_cargo.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "${tmpdir}/fake-cargo.sh"
EOF
chmod +x "${tmpdir}/scripts/resolve_cargo.sh"

cat > "${tmpdir}/scripts/resolve_rustc.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "${tmpdir}/fake-rustc"
EOF
chmod +x "${tmpdir}/scripts/resolve_rustc.sh"

cat > "${tmpdir}/fake-cargo.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "${tmpdir}/cargo-args.txt"
exit 0
EOF
chmod +x "${tmpdir}/fake-cargo.sh"

touch "${tmpdir}/fake-rustc"

(
  cd "${tmpdir}"
  AMAI_AUTO_INSTALL_PREREQS=0 ./scripts/install_amai.sh --skip-stack --stack-profile default --client vscode --yes >/dev/null
)

grep -Fqx 'run --quiet --release --bin amai-bootstrap -- install --skip-stack --stack-profile default --client vscode --yes' "${tmpdir}/cargo-args.txt"

printf 'proof_install_amai_release_run_path: PASS\n'
