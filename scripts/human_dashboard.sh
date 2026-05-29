#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

if [[ -d "./state/tooling/cmake-venv/bin" ]]; then
  export PATH="$(pwd)/state/tooling/cmake-venv/bin:$PATH"
fi

bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
host="${bind%:*}"
port="${bind##*:}"
browser_host="$host"
if [[ "$browser_host" == "0.0.0.0" || "$browser_host" == "::" ]]; then
  browser_host="127.0.0.1"
fi

dashboard_url="http://${browser_host}:${port}/"
health_url="http://${browser_host}:${port}/healthz"
ready_url="http://${browser_host}:${port}/api/client-budget-root-cause"
unit_name="amai-human-dashboard"
unit_file="${unit_name}.service"
launcher_script="$(pwd)/scripts/run_human_dashboard_service.sh"
log_hint="journalctl --user -u ${unit_file} -f"
legacy_pid_file="./state/human_dashboard.pid"

mkdir -p ./state ./tmp

systemd_user_available() {
  systemctl --user show-environment >/dev/null 2>&1
}

stop_legacy_pid() {
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

stop_legacy_processes() {
  if [[ -f "$legacy_pid_file" ]]; then
    stop_legacy_pid "$(cat "$legacy_pid_file")"
    rm -f "$legacy_pid_file"
  fi

  while read -r orphan_pid; do
    [[ -z "$orphan_pid" ]] && continue
    stop_legacy_pid "$orphan_pid"
  done < <(pgrep -f "amai observe serve --bind ${bind}" || true)
}

if systemd_user_available && systemctl --user is-active --quiet "$unit_file" && curl -fsS "$ready_url" >/dev/null 2>&1; then
  echo "Amai human dashboard already running"
  echo "URL: $dashboard_url"
  echo "Service: $unit_file"
  echo "Logs: $log_hint"
  exit 0
fi

if curl -fsS "$ready_url" >/dev/null 2>&1; then
  echo "Existing non-systemd dashboard detected on $ready_url. Replacing it with the managed launcher." >&2
  stop_legacy_processes
fi

cargo build --release --quiet

if systemd_user_available; then
  systemctl --user stop "$unit_file" >/dev/null 2>&1 || true
  systemctl --user reset-failed "$unit_file" >/dev/null 2>&1 || true
  systemd-run \
    --user \
    --unit="$unit_name" \
    --collect \
    --description="Amai human dashboard" \
    --property=Type=notify \
    --property=NotifyAccess=all \
    --property=Restart=always \
    --property=RestartSec=2s \
    --property=WorkingDirectory="$(pwd)" \
    --setenv=AMI_OBSERVE_BIND="${bind}" \
    "$launcher_script" >/dev/null
else
  nohup "$launcher_script" </dev/null >./tmp/human_dashboard.log 2>&1 &
fi

for _ in $(seq 1 120); do
  if curl -fsS "$ready_url" >/dev/null 2>&1; then
    echo "Amai human dashboard started"
    echo "URL: $dashboard_url"
    if systemd_user_available; then
      echo "Service: $unit_file"
      echo "Logs: $log_hint"
    else
      echo "Log: ./tmp/human_dashboard.log"
    fi
    exit 0
  fi
  if systemd_user_available && ! systemctl --user is-active --quiet "$unit_file"; then
    echo "Amai human dashboard failed to start. Last journal lines:" >&2
    journalctl --user -u "$unit_file" -n 40 --no-pager >&2 || true
    exit 1
  fi
  sleep 0.5
done

if systemd_user_available; then
  echo "Amai human dashboard did not become healthy in time. Last journal lines:" >&2
  journalctl --user -u "$unit_file" -n 40 --no-pager >&2 || true
else
  echo "Amai human dashboard did not become healthy in time. Last log lines:" >&2
  tail -n 40 ./tmp/human_dashboard.log >&2 || true
fi
exit 1
