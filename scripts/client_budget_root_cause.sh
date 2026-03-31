#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

enforce_reply_gate=false
thread_id=""
declare -a passthrough_args=()

while (($# > 0)); do
  case "$1" in
    --enforce-reply-gate)
      enforce_reply_gate=true
      passthrough_args+=("$1")
      shift
      ;;
    --thread-id)
      if (($# < 2)); then
        break
      fi
      thread_id="$2"
      passthrough_args+=("$1" "$2")
      shift 2
      ;;
    *)
      passthrough_args+=("$1")
      shift
      ;;
  esac
done

if [[ -z "$thread_id" ]] && [[ -n "${CODEX_THREAD_ID:-}" ]]; then
  thread_id="${CODEX_THREAD_ID}"
fi

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

api_url="http://${observe_host}:${observe_port}/api/client-budget-root-cause"
if [[ -n "$thread_id" ]]; then
  encoded_thread_id="$(printf '%s' "$thread_id" | jq -sRr @uri)"
  api_url="${api_url}?thread_id=${encoded_thread_id}"
fi
api_max_time="1.5"
if [[ -n "$thread_id" ]]; then
  api_max_time="7"
fi
cache_path="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.json"
if [[ -n "$thread_id" ]]; then
  safe_thread_id="$(printf '%s' "$thread_id" | tr -c '[:alnum:]_-' '_')"
  cache_path="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.thread-${safe_thread_id}.json"
fi

fresh_compact_client_budget_cache_available() {
  local expected_thread_id="${1:-}"
  [[ -f "$cache_path" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  local now_ms fetched_at_ms observed_at_ms cache_thread_id
  now_ms="$(date +%s%3N 2>/dev/null || true)"
  [[ -n "$now_ms" ]] || return 1
  fetched_at_ms="$(jq -r '.fetched_at_epoch_ms // 0' "$cache_path" 2>/dev/null || printf '0')"
  observed_at_ms="$(
    jq -r '
      .root_cause.client_budget_reply_gate.observed_at_epoch_ms
      // .gate.client_budget_reply_gate.observed_at_epoch_ms
      // .guard.observed_at_epoch_ms
      // 0
    ' "$cache_path" 2>/dev/null || printf '0'
  )"
  cache_thread_id="$(jq -r '.thread_id // ""' "$cache_path" 2>/dev/null || printf '')"
  [[ "$fetched_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$observed_at_ms" =~ ^[0-9]+$ ]] || return 1
  if [[ -n "$expected_thread_id" ]]; then
    [[ "$cache_thread_id" == "$expected_thread_id" ]] || return 1
  else
    [[ -z "$cache_thread_id" ]] || return 1
  fi
  (( now_ms - fetched_at_ms <= 10000 )) || return 1
  (( now_ms - observed_at_ms <= 10000 )) || return 1
}

read_fresh_compact_client_budget_root_cause_cache() {
  local expected_thread_id="${1:-}"
  fresh_compact_client_budget_cache_available "$expected_thread_id" || return 1
  jq -c '.root_cause' "$cache_path" 2>/dev/null || return 1
}

exact_thread_root_cause_payload_is_thread_bound() {
  local expected_thread_id="${1:-}"
  local payload="${2:-}"
  [[ -n "$expected_thread_id" ]] || return 0
  [[ -n "$payload" ]] || return 1
  printf '%s' "$payload" | jq -e '
    .thread_binding_state == "current_thread_bound"
    and (.current_live_turn.status // "") != "current_thread_unbound"
  ' >/dev/null 2>&1
}

api_payload=""
if [[ -n "$thread_id" ]]; then
  api_payload="$(read_fresh_compact_client_budget_root_cause_cache "$thread_id" || true)"
fi
if command -v curl >/dev/null 2>&1; then
  if [[ -z "$api_payload" ]]; then
    api_payload="$(curl --silent --show-error --fail --max-time "$api_max_time" "$api_url" 2>/dev/null || true)"
  fi
  if [[ -z "$api_payload" ]]; then
    api_payload="$(read_fresh_compact_client_budget_root_cause_cache "${thread_id:-}" || true)"
  fi
  if [[ -z "$api_payload" ]] \
    && command -v systemctl >/dev/null 2>&1 \
    && systemctl --user is-active --quiet amai-human-dashboard.service 2>/dev/null \
    && fresh_compact_client_budget_cache_available "${thread_id:-}"; then
    sleep 1.35
    api_payload="$(curl --silent --show-error --fail --max-time "$api_max_time" "$api_url" 2>/dev/null || true)"
    if [[ -z "$api_payload" ]]; then
      api_payload="$(read_fresh_compact_client_budget_root_cause_cache "${thread_id:-}" || true)"
    fi
  fi
fi

if [[ -n "$api_payload" ]] && ! exact_thread_root_cause_payload_is_thread_bound "${thread_id:-}" "$api_payload"; then
  api_payload=""
fi

if [[ -n "$api_payload" ]]; then
  printf '%s\n' "$api_payload"
  if [[ "$enforce_reply_gate" == "true" ]]; then
    reply_blocked="$(
      printf '%s' "$api_payload" | jq -r '
        (
          .client_budget_reply_gate.reply_execution_gate.blocking
          // .client_budget_reply_gate.reply_execution_gate.must_rotate_before_reply
          // .client_budget_reply_gate.reply_execution_gate.must_wait_for_budget_recovery_before_reply
          // false
        ) | if . then "true" else "false" end
      ' 2>/dev/null || printf 'false'
    )"
    if [[ "$reply_blocked" == "true" ]]; then
      echo "client budget reply gate blocked this reply" >&2
      exit 1
    fi
  fi
  exit 0
fi

exec env \
  AMAI_EXEC_DISABLE_BUDGET_HELPERS=1 \
  "$SCRIPT_DIR/amai_exec.sh" observe client-budget-root-cause "${passthrough_args[@]}"
