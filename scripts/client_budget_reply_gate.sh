#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

startup_contract_path=".amai/onboarding/project-chat-startup-contract.json"
if [[ -f "$startup_contract_path" ]] && command -v jq >/dev/null 2>&1; then
  reply_blocking_removed="$(
    jq -r '
      .startup_contract.live_client_budget_enforcement.reply_blocking_removed
      // false
    ' "$startup_contract_path" 2>/dev/null || printf 'false'
  )"
  if [[ "$reply_blocking_removed" == "true" ]]; then
    exit 0
  fi
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

api_url="http://${observe_host}:${observe_port}/api/client-budget-gate"
cache_path="${REPO_ROOT}/state/observe/client_budget_gate_cache.json"
CLIENT_BUDGET_GATE_CACHE_VERSION="client-budget-gate-cache-v7"

fresh_compact_client_budget_gate_cache_available() {
  [[ -f "$cache_path" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  local now_ms fetched_at_ms observed_at_ms
  now_ms="$(date +%s%3N 2>/dev/null || true)"
  [[ -n "$now_ms" ]] || return 1
  fetched_at_ms="$(jq -r '.fetched_at_epoch_ms // 0' "$cache_path" 2>/dev/null || printf '0')"
  observed_at_ms="$(
    jq -r '
      .gate.client_budget_reply_gate.observed_at_epoch_ms
      // .guard.observed_at_epoch_ms
      // 0
    ' "$cache_path" 2>/dev/null || printf '0'
  )"
  cache_version="$(jq -r '.cache_version // ""' "$cache_path" 2>/dev/null || printf '')"
  [[ "$fetched_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$observed_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$cache_version" == "$CLIENT_BUDGET_GATE_CACHE_VERSION" ]] || return 1
  (( now_ms - fetched_at_ms <= 10000 )) || return 1
  (( now_ms - observed_at_ms <= 10000 )) || return 1
}

read_fresh_compact_client_budget_gate_cache() {
  fresh_compact_client_budget_gate_cache_available || return 1
  jq -c '.gate' "$cache_path" 2>/dev/null || return 1
}

gate_json=""
if command -v curl >/dev/null 2>&1; then
  gate_json="$(curl --silent --show-error --fail --max-time 1.5 "$api_url" 2>/dev/null || true)"
  if [[ -z "$gate_json" ]]; then
    gate_json="$(read_fresh_compact_client_budget_gate_cache || true)"
  fi
fi

if [[ -z "$gate_json" ]]; then
  gate_json="$("$SCRIPT_DIR/client_budget_gate.sh" 2>/dev/null || true)"
fi

if [[ -z "$gate_json" ]]; then
  echo "client budget reply gate: no gate payload available" >&2
  exit 12
fi

guard_fields="$(
  printf '%s' "$gate_json" | jq -r '
    [
      (
        .client_budget_reply_gate.reply_execution_gate.blocking
        // .client_budget_reply_gate.reply_execution_gate.must_rotate_before_reply
        // .client_budget_reply_gate.reply_execution_gate.must_wait_for_budget_recovery_before_reply
        // false
      ),
      (
        .client_budget_reply_gate.reply_execution_gate.blocking_reply_contract.template
        // empty
      ),
      (
        .client_budget_reply_gate.reply_execution_gate.reply_prefix
        // .client_budget_reply_gate.reply_prefix
        // empty
      )
    ] | @tsv
  ' 2>/dev/null || true
)"
if [[ -z "$guard_fields" ]]; then
  echo "client budget reply gate: invalid gate payload" >&2
  exit 12
fi
IFS=$'\t' read -r reply_blocked blocked_reply reply_prefix <<<"$guard_fields"

if [[ "$reply_blocked" != "true" ]]; then
  exit 0
fi

if [[ -z "$blocked_reply" ]]; then
  blocked_reply="$(jq -r '
    .startup_contract.live_client_budget_enforcement.blocking_reply_template // empty
  ' .amai/onboarding/project-chat-startup-contract.json)"
fi

if [[ -z "$blocked_reply" ]]; then
  echo "client budget guard blocked the reply, but no blocking reply template is available" >&2
  exit 11
fi

if [[ -n "$reply_prefix" ]]; then
  printf '%s\n' "$reply_prefix"
fi
printf '%s\n' "$blocked_reply"
exit 10
