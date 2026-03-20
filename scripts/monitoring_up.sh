#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
./scripts/render_monitoring_config.sh

docker compose --profile monitoring rm -sf prometheus grafana >/dev/null 2>&1 || true
docker compose --profile monitoring up -d --force-recreate prometheus grafana
echo "Prometheus: http://127.0.0.1:${AMI_PROMETHEUS_PORT}"
echo "Grafana: http://127.0.0.1:${AMI_GRAFANA_PORT}"
