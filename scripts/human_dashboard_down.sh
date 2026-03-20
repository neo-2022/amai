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

pid_file="./state/human_dashboard.pid"
health_url="http://${browser_host}:${port}/healthz"

stop_pid() {
  local pid="$1"
  if kill -0 "$pid" >/dev/null 2>&1; then
    kill "$pid" >/dev/null 2>&1 || true
    for _ in $(seq 1 40); do
      if ! kill -0 "$pid" >/dev/null 2>&1; then
        return 0
      fi
      sleep 0.25
    done
  fi
  return 0
}

if [[ -f "$pid_file" ]]; then
  pid="$(cat "$pid_file")"
  stop_pid "$pid"
  rm -f "$pid_file"
fi

while read -r orphan_pid; do
  [[ -z "$orphan_pid" ]] && continue
  stop_pid "$orphan_pid"
done < <(pgrep -f "amai observe serve --bind ${bind}" || true)

if curl -fsS "$health_url" >/dev/null 2>&1; then
  echo "Amai human dashboard is still responding on $health_url" >&2
  exit 1
fi

echo "Amai human dashboard stopped"
