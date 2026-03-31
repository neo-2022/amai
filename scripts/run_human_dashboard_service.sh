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
health_host="$host"
if [[ "$health_host" == "0.0.0.0" || "$health_host" == "::" ]]; then
  health_host="127.0.0.1"
fi
health_url="http://${health_host}:${port}/healthz"
ready_url="http://${health_host}:${port}/api/client-budget-root-cause"
binary="./target/release/amai"
nats_http_url="${AMI_NATS_HTTP_URL:-http://127.0.0.1:58222}"
compose_file="./compose.yaml"

nats_varz_healthy() {
  curl -fsS "${nats_http_url%/}/varz" >/dev/null 2>&1
}

local_nats_self_heal_allowed() {
  [[ -f "${compose_file}" ]] || return 1
  [[ "${nats_http_url}" =~ ^http://(127\.0\.0\.1|localhost):[0-9]+/?$ ]]
}

ensure_local_nats_varz() {
  nats_varz_healthy && return 0
  local_nats_self_heal_allowed || return 0
  command -v docker >/dev/null 2>&1 || return 0

  local status=""
  local http_binding=""
  status="$(docker inspect --format '{{.State.Status}}' ami-nats 2>/dev/null || true)"
  http_binding="$(docker port ami-nats 8222/tcp 2>/dev/null || true)"

  if [[ "${status}" == "running" && -n "${http_binding}" ]]; then
    for _ in $(seq 1 20); do
      nats_varz_healthy && return 0
      sleep 0.5
    done
  fi

  if [[ "${status}" == "running" && -z "${http_binding}" ]]; then
    docker compose -f "${compose_file}" rm -sf nats >/dev/null
  fi

  docker compose -f "${compose_file}" up -d nats >/dev/null

  for _ in $(seq 1 40); do
    nats_varz_healthy && return 0
    sleep 0.5
  done

  echo "dashboard self-heal could not restore NATS /varz at ${nats_http_url%/}/varz" >&2
  return 1
}

ensure_local_nats_varz

if [[ -n "${NOTIFY_SOCKET:-}" ]] && command -v systemd-notify >/dev/null 2>&1; then
  child_pid=""
  cleanup_child() {
    [[ -n "$child_pid" ]] || return 0
    if kill -0 "$child_pid" >/dev/null 2>&1; then
      kill "$child_pid" >/dev/null 2>&1 || true
      wait "$child_pid" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup_child EXIT INT TERM

  "$binary" observe serve --bind "${bind}" &
  child_pid="$!"

  systemd-notify --status="Amai human dashboard starting: waiting for ${ready_url}" || true
  ready=0
  for _ in $(seq 1 240); do
    if curl -fsS "$ready_url" >/dev/null 2>&1; then
      ready=1
      break
    fi
    if ! kill -0 "$child_pid" >/dev/null 2>&1; then
      wait "$child_pid"
      exit $?
    fi
    sleep 0.25
  done

  if [[ "$ready" != "1" ]]; then
    echo "dashboard launcher did not observe ready ${ready_url} before notify timeout" >&2
    exit 1
  fi

  systemd-notify --ready --status="Amai human dashboard ready at ${ready_url}" || true
  wait "$child_pid"
  exit $?
fi

exec "$binary" observe serve --bind "${bind}"
