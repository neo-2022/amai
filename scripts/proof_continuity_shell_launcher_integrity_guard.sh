#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

launcher_path="./scripts/amai_exec.sh"
backup="$(mktemp /tmp/amai_exec.sh.integrity.XXXXXX)"
cp -p "${launcher_path}" "${backup}"
cleanup() {
  if [[ -f "${backup}" ]]; then
    cp -p "${backup}" "${launcher_path}"
    rm -f "${backup}"
  fi
}
trap cleanup EXIT

: > "${launcher_path}"
chmod 600 "${launcher_path}"

if ./scripts/proof_continuity_shell_release_fallback.sh >/tmp/proof_continuity_shell_launcher_integrity_guard.out 2>/tmp/proof_continuity_shell_launcher_integrity_guard.err; then
  echo "proof_continuity_shell_launcher_integrity_guard: expected corrupted launcher preflight to fail" >&2
  exit 1
fi

grep -Fq "proof_continuity_shell_release_fallback: corrupted ./scripts/amai_exec.sh before proof" \
  /tmp/proof_continuity_shell_launcher_integrity_guard.err

echo "proof_continuity_shell_launcher_integrity_guard: PASS"
