#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

missing_cargo_out="${tmp_dir}/missing-cargo.out"
missing_rustc_out="${tmp_dir}/missing-rustc.out"
missing_docker_out="${tmp_dir}/missing-docker.out"
missing_compose_out="${tmp_dir}/missing-compose.out"
skip_stack_out="${tmp_dir}/skip-stack.out"
stub_bin="${tmp_dir}/stub-bin"
mkdir -p "${stub_bin}"
ln -s /bin/bash "${stub_bin}/bash"
ln -s /usr/bin/env "${stub_bin}/env"
ln -s /usr/bin/dirname "${stub_bin}/dirname"

if PATH="/usr/bin:/bin" ./scripts/install_amai.sh --client vscode --skip-stack --stack-profile default --yes >"${missing_cargo_out}" 2>&1; then
  echo "proof_install_prereq_frontdoor: missing cargo unexpectedly succeeded" >&2
  exit 1
fi
rg '^Amai runner requires a working cargo binary\. Install rust/cargo or set AMAI_CARGO_BIN\.$' "${missing_cargo_out}" >/dev/null

ln -s "$(command -v cargo)" "${stub_bin}/cargo"
if PATH="${stub_bin}:/usr/bin:/bin" ./scripts/install_amai.sh --client vscode --skip-stack --stack-profile default --yes >"${missing_rustc_out}" 2>&1; then
  echo "proof_install_prereq_frontdoor: missing rustc unexpectedly succeeded" >&2
  exit 1
fi
rg '^Amai runner requires a working rustc binary\. Install rustc or set AMAI_RUSTC_BIN\.$' "${missing_rustc_out}" >/dev/null

cat >"${stub_bin}/cargo-stub" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  printf 'cargo 1.0.0-stub\n'
  exit 0
fi
if [[ "${1:-}" == "run" ]]; then
  printf 'stub cargo run reached\n'
  exit 0
fi
printf 'cargo-stub: unexpected args: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${stub_bin}/cargo-stub"

cat >"${stub_bin}/rustc-stub" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "-vV" ]]; then
  printf 'rustc 1.0.0-stub\n'
  exit 0
fi
printf 'rustc-stub: unexpected args: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${stub_bin}/rustc-stub"

if PATH="${stub_bin}" AMAI_CARGO_BIN="${stub_bin}/cargo-stub" AMAI_RUSTC_BIN="${stub_bin}/rustc-stub" \
  bash ./scripts/install_amai.sh --client vscode --stack-profile default --yes >"${missing_docker_out}" 2>&1; then
  echo "proof_install_prereq_frontdoor: missing docker unexpectedly succeeded" >&2
  exit 1
fi
rg '^Amai install requires docker for local stack bootstrap\. Install Docker or rerun with --skip-stack / --ssh-destination\.$' "${missing_docker_out}" >/dev/null

cat >"${stub_bin}/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  printf 'Docker version 99.0.0-stub\n'
  exit 0
fi
if [[ "${1:-}" == "compose" ]]; then
  printf 'docker compose missing\n' >&2
  exit 127
fi
printf 'docker-stub: unexpected args: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${stub_bin}/docker"

if PATH="${stub_bin}" AMAI_CARGO_BIN="${stub_bin}/cargo-stub" AMAI_RUSTC_BIN="${stub_bin}/rustc-stub" \
  bash ./scripts/install_amai.sh --client vscode --stack-profile default --yes >"${missing_compose_out}" 2>&1; then
  echo "proof_install_prereq_frontdoor: missing docker compose unexpectedly succeeded" >&2
  exit 1
fi
rg '^Amai install requires docker compose v2 for local stack bootstrap\. Install the docker compose plugin or rerun with --skip-stack / --ssh-destination\.$' "${missing_compose_out}" >/dev/null

PATH="${stub_bin}" AMAI_CARGO_BIN="${stub_bin}/cargo-stub" AMAI_RUSTC_BIN="${stub_bin}/rustc-stub" \
  bash ./scripts/install_amai.sh --client vscode --skip-stack --stack-profile default --yes >"${skip_stack_out}" 2>&1
rg '^stub cargo run reached$' "${skip_stack_out}" >/dev/null

echo "proof_install_prereq_frontdoor: ok"
