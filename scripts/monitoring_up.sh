#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
./scripts/render_monitoring_config.sh

mkdir -p ./tmp

require_command() {
  local binary="$1"
  if ! command -v "${binary}" >/dev/null 2>&1; then
    echo "monitoring_up.sh requires '${binary}' in PATH" >&2
    exit 1
  fi
}

dump_monitoring_debug() {
  docker compose --profile monitoring ps || true
  docker logs ami-prometheus --tail 120 || true
  docker logs ami-grafana --tail 120 || true
}

wait_for_http_ready() {
  local service_name="$1"
  local url="$2"
  local attempts="$3"
  local output_file="$4"
  local grep_pattern="${5:-}"
  local stderr_file="${output_file}.stderr"

  rm -f "${output_file}"
  rm -f "${stderr_file}"
  for _ in $(seq 1 "${attempts}"); do
    local payload=""
    payload="$(curl --silent --show-error --max-time 3 "${url}" 2>"${stderr_file}" || true)"
    printf '%s' "${payload}" > "${output_file}" || true
    if [[ -z "${grep_pattern}" ]]; then
      if [[ -n "${payload}" ]]; then
        return 0
      fi
    elif printf '%s' "${payload}" | rg "${grep_pattern}" >/dev/null; then
      return 0
    fi
    sleep 1
  done

  echo "${service_name} did not become ready at ${url}" >&2
  if [[ -s "${output_file}" ]]; then
    echo "last ${service_name} payload:" >&2
    cat "${output_file}" >&2
  fi
  if [[ -s "${stderr_file}" ]]; then
    echo "last ${service_name} curl stderr:" >&2
    cat "${stderr_file}" >&2
  fi
  dump_monitoring_debug
  return 1
}

require_command docker
require_command curl
require_command rg

docker compose --profile monitoring rm -sf prometheus grafana >/dev/null 2>&1 || true
docker compose --profile monitoring up -d --force-recreate prometheus grafana
wait_for_http_ready \
  "Prometheus" \
  "http://127.0.0.1:${AMI_PROMETHEUS_PORT}/-/ready" \
  60 \
  "./tmp/monitoring-prometheus-ready.txt"
wait_for_http_ready \
  "Grafana" \
  "http://127.0.0.1:${AMI_GRAFANA_PORT}/api/health" \
  60 \
  "./tmp/monitoring-grafana-health.json" \
  '"database"[[:space:]]*:[[:space:]]*"ok"'

echo "Prometheus ready: http://127.0.0.1:${AMI_PROMETHEUS_PORT}"
echo "Grafana ready: http://127.0.0.1:${AMI_GRAFANA_PORT}"
