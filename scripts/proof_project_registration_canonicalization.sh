#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
alias_output="$(mktemp)"
child_output="$(mktemp)"
same_code_child_output="$(mktemp)"
ambiguous_parent_output="$(mktemp)"
project_code="canonical_root_probe_$$"
alias_code="${project_code}_alias"
child_code="${project_code}_child"
canonical_root="$(mktemp -d)"
child_root="${canonical_root}/nested-app"
alias_link="$(mktemp -u)"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code IN ('${project_code}', '${alias_code}', '${child_code}')" >/dev/null 2>&1 || true
  rm -rf "${canonical_root}" "${alias_link}" "${alias_output}" "${child_output}" "${same_code_child_output}" "${ambiguous_parent_output}"
}
trap cleanup EXIT

cd "${repo_root}"

cargo run --release --quiet -- bootstrap schema >/dev/null

./target/release/amai project register \
  --code "${project_code}" \
  --display-name "Canonical root probe" \
  --repo-root "${canonical_root}" >/dev/null

ln -s "${canonical_root}" "${alias_link}"

if ./target/release/amai project register \
  --code "${alias_code}" \
  --display-name "Canonical alias probe" \
  --repo-root "${alias_link}" >"${alias_output}" 2>&1; then
  echo "symlink alias registration unexpectedly succeeded" >&2
  exit 1
fi

grep -q "already registered as project ${project_code}" "${alias_output}"

test "$(psql "${dsn}" -Atqc "SELECT COUNT(*) FROM ami.projects WHERE code = '${alias_code}';")" = "0"

mkdir -p "${child_root}"

if ./target/release/amai project register \
  --code "${child_code}" \
  --display-name "Canonical child probe" \
  --repo-root "${child_root}" >"${child_output}" 2>&1; then
  echo "child repo_root registration unexpectedly succeeded" >&2
  exit 1
fi

grep -q "already registered as project ${project_code}" "${child_output}"

test "$(psql "${dsn}" -Atqc "SELECT COUNT(*) FROM ami.projects WHERE code = '${child_code}';")" = "0"

if ./target/release/amai project register \
  --code "${project_code}" \
  --display-name "Canonical root probe" \
  --repo-root "${child_root}" >"${same_code_child_output}" 2>&1; then
  echo "same-code child repo_root narrowing unexpectedly succeeded" >&2
  exit 1
fi

grep -q "narrowing the canonical project root is blocked" "${same_code_child_output}"

test "$(psql "${dsn}" -Atqc "SELECT repo_root FROM ami.projects WHERE code = '${project_code}';")" = "${canonical_root}"

cargo test --release --quiet repo_root_hint_for_parent_with_multiple_child_projects_fails_closed -- --test-threads=1 >"${ambiguous_parent_output}" 2>&1 || {
  cat "${ambiguous_parent_output}" >&2
  exit 1
}

printf 'proof_project_registration_canonicalization: ok\n'
