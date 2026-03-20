#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

mkdir -p tmp/monitoring/grafana/provisioning/datasources

sed \
  -e "s|__AMI_PROMETHEUS_SCRAPE_TARGET__|${AMI_PROMETHEUS_SCRAPE_TARGET}|g" \
  -e "s|__AMI_QDRANT_SCRAPE_TARGET__|${AMI_QDRANT_SCRAPE_TARGET}|g" \
  config/prometheus/prometheus.yml > tmp/monitoring/prometheus.yml

sed \
  -e "s|__AMI_GRAFANA_PROMETHEUS_URL__|${AMI_GRAFANA_PROMETHEUS_URL}|g" \
  config/grafana/provisioning/datasources/prometheus.yaml > tmp/monitoring/grafana/provisioning/datasources/prometheus.yaml
