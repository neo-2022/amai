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
./scripts/render_nats_config.sh >/dev/null
lock_dir="./state/observe"
lock_file="${lock_dir}/run_human_dashboard_service.lock"

observe_frontdoor_pid_is_stale() {
  local pid="${1:-}"
  local exe_path=""
  [[ -n "${pid}" ]] || return 1
  exe_path="$(readlink "/proc/${pid}/exe" 2>/dev/null || true)"
  [[ -n "${exe_path}" ]] && [[ "${exe_path}" == *" (deleted)" ]]
}

restart_after_stale_lock_holder() {
  local stale_pid="${1:-}"
  [[ -n "${stale_pid}" ]] || return 1
  echo "run_human_dashboard_service: stale dashboard process pid=${stale_pid} detected while lock held; restarting" >&2
  kill "${stale_pid}" >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    if ! kill -0 "${stale_pid}" >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done
  if kill -0 "${stale_pid}" >/dev/null 2>&1; then
    kill -9 "${stale_pid}" >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      if ! kill -0 "${stale_pid}" >/dev/null 2>&1; then
        break
      fi
      sleep 0.1
    done
  fi
  if kill -0 "${stale_pid}" >/dev/null 2>&1; then
    echo "run_human_dashboard_service: unable to stop stale dashboard process pid=${stale_pid}" >&2
    exit 1
  fi
  if ! flock -n 9; then
    echo "run_human_dashboard_service: stale dashboard process pid=${stale_pid} disappeared but lock did not release" >&2
    exit 1
  fi
}

mkdir -p "${lock_dir}"
exec 9>"${lock_file}"
if ! flock -n 9; then
  existing_pid="$(
    pgrep -fo "^${binary//\//\\/} observe serve --bind ${bind}$" 2>/dev/null || true
  )"
  if [[ -n "${existing_pid}" ]] && kill -0 "${existing_pid}" >/dev/null 2>&1 && observe_frontdoor_pid_is_stale "${existing_pid}"; then
    restart_after_stale_lock_holder "${existing_pid}"
  elif curl -fsS "${health_url}" >/dev/null 2>&1; then
    echo "Amai human dashboard already running at ${health_url}" >&2
    exit 0
  else
    echo "run_human_dashboard_service: another launcher instance is already active" >&2
    exit 1
  fi
fi

existing_pid="$(
  pgrep -fo "^${binary//\//\\/} observe serve --bind ${bind}$" 2>/dev/null || true
)"
if [[ -n "${existing_pid}" ]] && kill -0 "${existing_pid}" >/dev/null 2>&1; then
  if observe_frontdoor_pid_is_stale "${existing_pid}"; then
    echo "run_human_dashboard_service: stale dashboard process pid=${existing_pid} detected; restarting" >&2
    kill "${existing_pid}" >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      if ! kill -0 "${existing_pid}" >/dev/null 2>&1; then
        break
      fi
      sleep 0.1
    done
    if kill -0 "${existing_pid}" >/dev/null 2>&1; then
      kill -9 "${existing_pid}" >/dev/null 2>&1 || true
      for _ in $(seq 1 20); do
        if ! kill -0 "${existing_pid}" >/dev/null 2>&1; then
          break
        fi
        sleep 0.1
      done
    fi
    if kill -0 "${existing_pid}" >/dev/null 2>&1; then
      echo "run_human_dashboard_service: unable to stop stale dashboard process pid=${existing_pid}" >&2
      exit 1
    fi
  elif curl -fsS "${health_url}" >/dev/null 2>&1; then
    echo "Amai human dashboard already running at ${health_url} (pid=${existing_pid})" >&2
    exit 0
  fi
fi

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
