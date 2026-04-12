#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

default_output="$(./scripts/render_nats_config.sh)"
grep -F 'authorization {' "${default_output}" >/dev/null
if grep -F 'users = [' "${default_output}" >/dev/null; then
  echo "default disabled NATS auth mode must not render credential users" >&2
  exit 1
fi

AMI_NATS_AUTH_MODE=password \
AMI_NATS_USER=proof_nats_user \
AMI_NATS_PASSWORD=proof_nats_secret \
  ./scripts/render_nats_config.sh >/tmp/amai_render_nats_auth_path.txt

password_output="$(cat /tmp/amai_render_nats_auth_path.txt)"
grep -F 'user: "proof_nats_user"' "${password_output}" >/dev/null
grep -F 'password: "proof_nats_secret"' "${password_output}" >/dev/null
grep -F 'users = [' "${password_output}" >/dev/null

printf 'proof_nats_auth_render: ok\n'
