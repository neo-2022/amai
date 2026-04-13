#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

set -a
. ./.env.example
set +a

./scripts/render_postgres_config.sh >/dev/null
compose_rendered="$(docker compose config)"
compose_monitoring_rendered="$(docker compose --profile monitoring config)"

grep -F 'AMI_STACK_BIND_HOST=127.0.0.1' .env.example >/dev/null
grep -F 'AMI_PROMETHEUS_IMAGE=prom/prometheus:v3.4.1' .env.example >/dev/null
grep -F 'AMI_GRAFANA_IMAGE=grafana/grafana:11.6.1' .env.example >/dev/null

if grep -Eq '^AMI_PROMETHEUS_IMAGE=.*:latest$' .env.example; then
  echo "prometheus image must be pinned" >&2
  exit 1
fi

if grep -Eq '^AMI_GRAFANA_IMAGE=.*:latest$' .env.example; then
  echo "grafana image must be pinned" >&2
  exit 1
fi

printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 5432\n\s+published: "55432"' >/dev/null
printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 6333\n\s+published: "56333"' >/dev/null
printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 6334\n\s+published: "56334"' >/dev/null
printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 9000\n\s+published: "59000"' >/dev/null
printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 9001\n\s+published: "59001"' >/dev/null
printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 4222\n\s+published: "54222"' >/dev/null
printf '%s\n' "$compose_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 8222\n\s+published: "58222"' >/dev/null
printf '%s\n' "$compose_monitoring_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 9090\n\s+published: "59090"' >/dev/null
printf '%s\n' "$compose_monitoring_rendered" | rg -U 'host_ip: 127\.0\.0\.1\n\s+target: 3000\n\s+published: "53000"' >/dev/null
grep -F "listen_addresses = '*'" tmp/postgres/postgresql.conf >/dev/null

echo "proof_ops_security_defaults: ok"
