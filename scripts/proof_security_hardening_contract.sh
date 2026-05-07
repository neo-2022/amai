#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

source ./scripts/load_env.sh

profile="${AMI_SECURITY_PROFILE:-default}"
if [[ "${profile}" != "hardened" ]]; then
  printf 'proof_security_hardening_contract: ok (profile=%s)\n' "${profile}"
  exit 0
fi

require_sslmode() {
  local dsn_name="$1"
  local dsn_value="$2"
  if [[ "${dsn_value}" != *"sslmode="* ]]; then
    echo "${dsn_name} must include sslmode when AMI_SECURITY_PROFILE=hardened" >&2
    exit 1
  fi
  if [[ "${dsn_value}" == *"sslmode=disable"* ]]; then
    echo "${dsn_name} must not use sslmode=disable when AMI_SECURITY_PROFILE=hardened" >&2
    exit 1
  fi
}

require_sslmode "AMI_POSTGRES_DSN" "${AMI_POSTGRES_DSN}"
require_sslmode "AMI_APP_POSTGRES_DSN" "${AMI_APP_POSTGRES_DSN}"

if [[ "${AMI_S3_ENDPOINT}" != https://* ]]; then
  echo "AMI_S3_ENDPOINT must be https:// when AMI_SECURITY_PROFILE=hardened" >&2
  exit 1
fi

if [[ "${AMI_MINIO_SCHEME}" != "https" ]]; then
  echo "AMI_MINIO_SCHEME must be https when AMI_SECURITY_PROFILE=hardened" >&2
  exit 1
fi

if [[ -z "${AMI_MINIO_CERTS_DIR:-}" ]]; then
  echo "AMI_MINIO_CERTS_DIR must be set when AMI_SECURITY_PROFILE=hardened" >&2
  exit 1
fi

if [[ ! -f "${AMI_MINIO_CERTS_DIR}/public.crt" || ! -f "${AMI_MINIO_CERTS_DIR}/private.key" ]]; then
  echo "AMI_MINIO_CERTS_DIR must contain public.crt and private.key when hardened" >&2
  exit 1
fi

if [[ -z "${AMI_POSTGRES_CERTS_DIR:-}" ]]; then
  echo "AMI_POSTGRES_CERTS_DIR must be set when AMI_SECURITY_PROFILE=hardened" >&2
  exit 1
fi

if [[ ! -f "${AMI_POSTGRES_CERTS_DIR}/server.crt" || ! -f "${AMI_POSTGRES_CERTS_DIR}/server.key" ]]; then
  echo "AMI_POSTGRES_CERTS_DIR must contain server.crt and server.key when hardened" >&2
  exit 1
fi

if [[ "${AMI_S3_ACCESS_KEY}" == "minioadmin" || "${AMI_S3_SECRET_KEY}" == "minioadmin" ]]; then
  echo "AMI_S3_ACCESS_KEY/AMI_S3_SECRET_KEY must not use minioadmin when hardened" >&2
  exit 1
fi

if [[ "${AMI_S3_ACCESS_KEY}" == *"change_me"* || "${AMI_S3_SECRET_KEY}" == *"change_me"* ]]; then
  echo "AMI_S3_ACCESS_KEY/AMI_S3_SECRET_KEY must be rotated from change_me when hardened" >&2
  exit 1
fi

if [[ "${AMI_NATS_AUTH_MODE}" != "password" ]]; then
  echo "AMI_NATS_AUTH_MODE must be password when AMI_SECURITY_PROFILE=hardened" >&2
  exit 1
fi

if [[ "${AMI_NATS_URL}" != *"://"* || "${AMI_NATS_URL}" != *"@"* ]]; then
  echo "AMI_NATS_URL must embed credentials (user:pass@host) when hardened" >&2
  exit 1
fi

printf 'proof_security_hardening_contract: ok (profile=%s)\n' "${profile}"
