#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

source ./scripts/load_env.sh

template_dir="config/postgres"
out_dir="tmp/postgres"
mkdir -p "${out_dir}"

profile="${AMI_SECURITY_PROFILE:-default}"
ssl_setting="off"
host_line="host"
if [[ "${profile}" == "hardened" ]]; then
  ssl_setting="on"
  host_line="hostssl"
fi

sed "s/{{SSL_SETTING}}/${ssl_setting}/g" "${template_dir}/postgresql.conf.tpl" \
  > "${out_dir}/postgresql.conf"
sed "s/{{HOST_LINE}}/${host_line}/g" "${template_dir}/pg_hba.conf.tpl" \
  > "${out_dir}/pg_hba.conf"
