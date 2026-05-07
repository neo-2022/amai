#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

source ./scripts/load_env.sh

template_path="${repo_root}/config/nats/server.conf.tpl"
output_path="${AMAI_NATS_CONFIG_OUTPUT_PATH:-${repo_root}/tmp/nats/server.conf}"
mkdir -p "$(dirname "${output_path}")"

auth_mode="${AMI_NATS_AUTH_MODE:-disabled}"
nats_user="${AMI_NATS_USER:-}"
nats_password="${AMI_NATS_PASSWORD:-}"
case "${auth_mode}" in
  disabled)
    ;;
  password)
    if [[ -z "${nats_user}" || -z "${nats_password}" ]]; then
      echo "AMI_NATS_USER and AMI_NATS_PASSWORD are required when AMI_NATS_AUTH_MODE=password" >&2
      exit 1
    fi
    ;;
  *)
    echo "unsupported AMI_NATS_AUTH_MODE: ${auth_mode}" >&2
    exit 1
    ;;
esac

python3 - <<'PY' "${template_path}" "${output_path}" "${auth_mode}" "${nats_user}" "${nats_password}"
import json
from pathlib import Path
import sys

template_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
auth_mode = sys.argv[3]
nats_user = sys.argv[4]
nats_password = sys.argv[5]

if auth_mode == "disabled":
    auth_block = "authorization {\n  timeout: 1\n}"
elif auth_mode == "password":
    auth_block = (
        "authorization {\n"
        "  timeout: 1\n"
        "  users = [\n"
        "    {\n"
        f"      user: {json.dumps(nats_user)}\n"
        f"      password: {json.dumps(nats_password)}\n"
        "    }\n"
        "  ]\n"
        "}"
    )
else:
    raise SystemExit(f"unsupported AMI_NATS_AUTH_MODE: {auth_mode}")

template = template_path.read_text()
rendered = template.replace("{{AUTH_BLOCK}}", auth_block)
output_path.write_text(rendered)
PY

printf '%s\n' "${output_path}"
