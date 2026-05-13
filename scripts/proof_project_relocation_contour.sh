#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="relocation_probe_$$"
display_name="Relocation Probe"
old_root="$(mktemp -d)"
new_root="$(mktemp -d)"
conflict_root="$(mktemp -d)"
conflict_output="$(mktemp)"

canonical_path() {
  local path="$1"
  (
    cd "${path}"
    pwd -P
  )
}

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code IN ('${project_code}', '${project_code}_conflict')" >/dev/null 2>&1 || true
  rm -rf "${old_root}" "${new_root}" "${conflict_root}" "${conflict_output}"
}
trap cleanup EXIT

old_root="$(canonical_path "${old_root}")"
new_root="$(canonical_path "${new_root}")"
conflict_root="$(canonical_path "${conflict_root}")"

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

./target/release/amai project register \
  --code "${project_code}" \
  --display-name "${display_name}" \
  --repo-root "${old_root}" >/dev/null

./target/release/amai project register \
  --code "${project_code}" \
  --display-name "${display_name}" \
  --repo-root "${new_root}" >/dev/null

test "$(psql "${dsn}" -Atqc "SELECT repo_root FROM ami.projects WHERE code='${project_code}'")" = "${new_root}"
test "$(psql "${dsn}" -Atqc "SELECT COUNT(*) FROM ami.project_repo_roots r INNER JOIN ami.projects p ON p.project_id = r.project_id WHERE p.code='${project_code}'")" = "2"
test "$(psql "${dsn}" -Atqc "SELECT root_kind FROM ami.project_repo_roots r INNER JOIN ami.projects p ON p.project_id = r.project_id WHERE p.code='${project_code}' AND r.repo_root='${new_root}'")" = "primary"
test "$(psql "${dsn}" -Atqc "SELECT root_kind FROM ami.project_repo_roots r INNER JOIN ami.projects p ON p.project_id = r.project_id WHERE p.code='${project_code}' AND r.repo_root='${old_root}'")" = "relocated_from"
test "$(psql "${dsn}" -Atqc "SELECT p.code FROM ami.project_repo_roots r INNER JOIN ami.projects p ON p.project_id = r.project_id WHERE r.repo_root='${old_root}'")" = "${project_code}"

./target/release/amai project register \
  --code "${project_code}_conflict" \
  --display-name "Conflict Probe" \
  --repo-root "${conflict_root}" >/dev/null

if ./target/release/amai project register \
  --code "${project_code}_conflict" \
  --display-name "Conflict Probe" \
  --repo-root "${old_root}" >"${conflict_output}" 2>&1; then
  echo "cross-project alias steal unexpectedly succeeded" >&2
  exit 1
fi

grep -q "already registered as project ${project_code}" "${conflict_output}"

printf 'proof_project_relocation_contour: PASS\n'
