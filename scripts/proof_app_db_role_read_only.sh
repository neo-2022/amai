#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

source ./scripts/load_env.sh

cargo run --quiet -- bootstrap stack >/dev/null

psql "${AMI_APP_POSTGRES_DSN}" -At -F $'\t' -c "SELECT count(*) FROM ami.projects;" >/dev/null

probe_key="proof_app_role_read_only_$(date +%s)_$$"
psql "${AMI_POSTGRES_DSN}" -v ON_ERROR_STOP=1 -c \
  "DELETE FROM ami.stack_meta WHERE meta_key = '${probe_key}';" >/dev/null

set +e
insert_output="$(
  psql "${AMI_APP_POSTGRES_DSN}" -v ON_ERROR_STOP=1 \
    -c "INSERT INTO ami.stack_meta(meta_key, meta_value) VALUES ('${probe_key}', '{}'::jsonb);" \
    2>&1
)"
insert_status=$?
set -e

if [[ ${insert_status} -eq 0 ]]; then
  echo "app db role unexpectedly allowed INSERT into ami.stack_meta" >&2
  exit 1
fi

grep -E "permission denied|must be owner of relation|permission denied for table" <<<"${insert_output}" >/dev/null

printf 'proof_app_db_role_read_only: ok\n'
