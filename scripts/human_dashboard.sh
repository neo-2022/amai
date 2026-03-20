#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
host="${bind%:*}"
port="${bind##*:}"
browser_host="$host"
if [[ "$browser_host" == "0.0.0.0" || "$browser_host" == "::" ]]; then
  browser_host="127.0.0.1"
fi

dashboard_url="http://${browser_host}:${port}/"
health_url="http://${browser_host}:${port}/healthz"
pid_file="./state/human_dashboard.pid"
log_file="./tmp/human_dashboard.log"

mkdir -p ./state ./tmp

if curl -fsS "$health_url" >/dev/null 2>&1; then
  echo "Amai human dashboard already running"
  echo "URL: $dashboard_url"
  if [[ -f "$pid_file" ]]; then
    echo "PID: $(cat "$pid_file")"
  fi
  exit 0
fi

nohup cargo run --release --quiet -- observe serve --bind "${bind}" >"$log_file" 2>&1 &
dashboard_pid=$!
echo "$dashboard_pid" >"$pid_file"

for _ in $(seq 1 120); do
  if curl -fsS "$health_url" >/dev/null 2>&1; then
    echo "Amai human dashboard started"
    echo "URL: $dashboard_url"
    echo "PID: $dashboard_pid"
    echo "Log: $log_file"
    exit 0
  fi
  if ! kill -0 "$dashboard_pid" >/dev/null 2>&1; then
    echo "Amai human dashboard failed to start. Last log lines:" >&2
    tail -n 40 "$log_file" >&2 || true
    exit 1
  fi
  sleep 0.5
done

echo "Amai human dashboard did not become healthy in time. Last log lines:" >&2
tail -n 40 "$log_file" >&2 || true
exit 1
