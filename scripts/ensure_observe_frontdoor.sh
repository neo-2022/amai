#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

path="/healthz"
max_wait_seconds="${AMAI_OBSERVE_FRONTDOOR_WAIT_SECONDS:-2}"

while (($# > 0)); do
  case "$1" in
    --path)
      path="${2:?missing value for --path}"
      shift 2
      ;;
    --max-wait-seconds)
      max_wait_seconds="${2:?missing value for --max-wait-seconds}"
      shift 2
      ;;
    *)
      echo "unsupported ensure_observe_frontdoor.sh argument: $1" >&2
      exit 1
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || exit 1

frontdoor_pid_is_stale() {
  local pid="${1:-}"
  local exe_path=""
  [[ -n "${pid}" ]] || return 1
  exe_path="$(readlink "/proc/${pid}/exe" 2>/dev/null || true)"
  [[ -n "${exe_path}" ]] && [[ "${exe_path}" == *" (deleted)" ]]
}

observe_bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
observe_host="${observe_bind%:*}"
observe_port="${observe_bind##*:}"
case "$observe_host" in
  ""|"0.0.0.0"|"::"|"[::]")
    observe_host="127.0.0.1"
    ;;
  \[*\])
    observe_host="${observe_host#[}"
    observe_host="${observe_host%]}"
    ;;
esac

ready_url="http://${observe_host}:${observe_port}${path}"
base_url="http://${observe_host}:${observe_port}/"
curl_max_time="${AMAI_OBSERVE_FRONTDOOR_CURL_MAX_TIME:-0.5}"
binary="./target/release/amai"

stale_pid=""
if command -v pgrep >/dev/null 2>&1 && command -v readlink >/dev/null 2>&1; then
  stale_pid="$(
    pgrep -fo "^${binary//\//\\/} observe serve --bind ${observe_bind}$" 2>/dev/null || true
  )"
  if [[ -n "${stale_pid}" ]]; then
    if ! kill -0 "${stale_pid}" >/dev/null 2>&1; then
      stale_pid=""
    elif ! frontdoor_pid_is_stale "${stale_pid}"; then
      stale_pid=""
    fi
  fi
fi

if [[ -z "${stale_pid}" ]]; then
  if curl --silent --show-error --fail --max-time "$curl_max_time" "$ready_url" >/dev/null 2>&1; then
    exit 0
  fi

  if curl --silent --show-error --fail --max-time "$curl_max_time" "$base_url" >/dev/null 2>&1; then
    exit 0
  fi
else
  echo "ensure_observe_frontdoor: stale dashboard process pid=${stale_pid} detected; restarting" >&2
fi

if [[ ! -x "${binary}" ]]; then
  exit 1
fi

mkdir -p ./tmp
nohup "$SCRIPT_DIR/run_human_dashboard_service.sh" </dev/null >./tmp/human_dashboard.log 2>&1 &

for _ in $(seq 1 $(( max_wait_seconds * 4 ))); do
  if curl --silent --show-error --fail --max-time "$curl_max_time" "$ready_url" >/dev/null 2>&1; then
    exit 0
  fi
  if curl --silent --show-error --fail --max-time "$curl_max_time" "$base_url" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 0.25
done

exit 1
