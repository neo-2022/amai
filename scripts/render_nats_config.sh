#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

source ./scripts/load_env.sh

template_path="${repo_root}/config/nats/server.conf.tpl"
output_path="${repo_root}/tmp/nats/server.conf"
mkdir -p "$(dirname "${output_path}")"

auth_mode="${AMI_NATS_AUTH_MODE:-disabled}"
case "${auth_mode}" in
  disabled)
    auth_block=$'authorization {\n  timeout: 1\n}'
    ;;
  password)
    nats_user="${AMI_NATS_USER:-}"
    nats_password="${AMI_NATS_PASSWORD:-}"
    if [[ -z "${nats_user}" || -z "${nats_password}" ]]; then
      echo "AMI_NATS_USER and AMI_NATS_PASSWORD are required when AMI_NATS_AUTH_MODE=password" >&2
      exit 1
    fi
    auth_block=$'authorization {\n  timeout: 1\n  users = [\n    {\n      user: "'"${nats_user}"'"\n      password: "'"${nats_password}"'"\n    }\n  ]\n}'
    ;;
  *)
    echo "unsupported AMI_NATS_AUTH_MODE: ${auth_mode}" >&2
    exit 1
    ;;
esac

python3 - <<'PY' "${template_path}" "${output_path}" "${auth_block}"
from pathlib import Path
import sys

template_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
auth_block = sys.argv[3]

template = template_path.read_text()
rendered = template.replace("{{AUTH_BLOCK}}", auth_block)
output_path.write_text(rendered)
PY

printf '%s\n' "${output_path}"
