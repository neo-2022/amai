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

if curl --silent --show-error --fail --max-time "$curl_max_time" "$ready_url" >/dev/null 2>&1; then
  exit 0
fi

if curl --silent --show-error --fail --max-time "$curl_max_time" "$base_url" >/dev/null 2>&1; then
  exit 0
fi

if [[ ! -x "./target/release/amai" ]]; then
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
