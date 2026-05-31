#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

trust_dir="${HOME}/.local/share/amai/signoff-trust"
trust_key="${trust_dir}/signoff_ed25519"
allowed_signers="${trust_dir}/allowed_signers"

die() {
  echo "provision_specialist_signoff_trust: $*" >&2
  exit 1
}

assert_outside_worktree() {
  local path="$1"
  local workspace_canon path_canon
  workspace_canon="$(readlink -f .)"
  path_canon="$(readlink -f "${path}")"
  case "${path_canon}" in
    "${workspace_canon}" | "${workspace_canon}"/*)
      die "trust root must stay outside worktree: ${path_canon}"
      ;;
  esac
}

mkdir -p "${trust_dir}"
chmod 700 "${trust_dir}"
assert_outside_worktree "${trust_dir}"

if [[ -e "${trust_key}" || -e "${allowed_signers}" ]]; then
  test -f "${trust_key}" || die "private key missing but trust root already exists"
  test -f "${allowed_signers}" || die "allowed_signers missing but trust root already exists"
  echo "provision_specialist_signoff_trust: already provisioned"
  exit 0
fi

ssh-keygen -q -t ed25519 -N "" -C "amai-specialist-signoff" -f "${trust_key}"
chmod 600 "${trust_key}"
printf 'amai-specialist-signoff %s\n' "$(ssh-keygen -y -f "${trust_key}")" >"${allowed_signers}"
chmod 644 "${allowed_signers}"

echo "provision_specialist_signoff_trust: ok"
