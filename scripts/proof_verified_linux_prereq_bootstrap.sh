#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT

fake_bin="${temp_root}/fake-bin"
fake_home="${temp_root}/home"
proof_log="${temp_root}/proof.log"
mkdir -p "${fake_bin}" "${fake_home}"

link_real() {
  ln -s "$(command -v "$1")" "${fake_bin}/$1"
}

for binary in bash env grep cut cat mkdir chmod printf true uname rm; do
  link_real "${binary}"
done

cat >"${fake_bin}/dpkg" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exit 1
EOF
chmod +x "${fake_bin}/dpkg"

cat >"${fake_bin}/apt-cache" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exit 0
EOF
chmod +x "${fake_bin}/apt-cache"

cat >"${fake_bin}/apt-get" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf 'apt-get %s\n' "\$*" >>"${proof_log}"
exit 0
EOF
chmod +x "${fake_bin}/apt-get"

cat >"${fake_bin}/sudo" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf 'sudo %s\n' "\$*" >>"${proof_log}"
exec "\$@"
EOF
chmod +x "${fake_bin}/sudo"

cat >"${fake_bin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat <<'RUSTUP'
#!/bin/sh
set -eu
mkdir -p "${HOME}/.cargo/bin"
cat >"${HOME}/.cargo/bin/rustup" <<'INNER'
#!/bin/sh
set -eu
exit 0
INNER
cat >"${HOME}/.cargo/bin/cargo" <<'INNER'
#!/bin/sh
set -eu
if [ "${1:-}" = "--version" ]; then
  printf 'cargo proof\n'
  exit 0
fi
exit 0
INNER
cat >"${HOME}/.cargo/bin/rustc" <<'INNER'
#!/bin/sh
set -eu
if [ "${1:-}" = "-vV" ]; then
  printf 'rustc proof\n'
  exit 0
fi
exit 0
INNER
chmod +x "${HOME}/.cargo/bin/rustup" "${HOME}/.cargo/bin/cargo" "${HOME}/.cargo/bin/rustc"
RUSTUP
EOF
chmod +x "${fake_bin}/curl"

cat >"${fake_bin}/systemctl" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf 'systemctl %s\n' "\$*" >>"${proof_log}"
exit 0
EOF
chmod +x "${fake_bin}/systemctl"

cat >"${fake_bin}/getent" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "group" && "${2:-}" == "docker" ]]; then
  printf 'docker:x:999:\n'
  exit 0
fi
exit 1
EOF
chmod +x "${fake_bin}/getent"

cat >"${fake_bin}/usermod" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf 'usermod %s\n' "\$*" >>"${proof_log}"
exit 0
EOF
chmod +x "${fake_bin}/usermod"

HOME="${fake_home}" USER="proof" PATH="${fake_bin}:/usr/bin:/bin" \
  bash -c "source '${repo_root}/scripts/ensure_verified_linux_prereqs.sh'; ensure_verified_linux_prereqs 1"

test -x "${fake_home}/.cargo/bin/cargo"
test -x "${fake_home}/.cargo/bin/rustc"
rg '^sudo apt-get update$' "${proof_log}" >/dev/null
rg '^sudo apt-get install -y git curl ca-certificates build-essential pkg-config libssl-dev$' "${proof_log}" >/dev/null
rg '^sudo apt-get install -y docker.io docker-compose-v2$' "${proof_log}" >/dev/null
rg '^usermod -aG docker proof$' "${proof_log}" >/dev/null

printf 'proof_verified_linux_prereq_bootstrap: PASS\n'
