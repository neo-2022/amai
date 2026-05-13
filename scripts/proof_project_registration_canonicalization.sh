#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
alias_output="$(mktemp)"
alias_code="art_alias_probe_$$"

cleanup() {
  rm -f "${alias_output}"
}
trap cleanup EXIT

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

if ./target/release/amai project register \
  --code "${alias_code}" \
  --display-name "Art alias probe" \
  --repo-root ../Art >"${alias_output}" 2>&1; then
  echo "alias registration unexpectedly succeeded" >&2
  exit 1
fi

grep -q "already registered as project art" "${alias_output}"

test "$(psql "${dsn}" -Atqc "SELECT COUNT(*) FROM ami.projects WHERE code = '${alias_code}';")" = "0"

printf 'proof_project_registration_canonicalization: ok\n'
