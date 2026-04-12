#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

CLIENT_BUDGET_SURFACES_CACHE_VERSION="client-budget-surfaces-cache-v7"
CLIENT_BUDGET_GATE_CACHE_VERSION="client-budget-gate-cache-v7"

startup_contract_reply_blocking_removed() {
  local startup_contract_path="${REPO_ROOT}/.amai/onboarding/project-chat-startup-contract.json"
  [[ -f "$startup_contract_path" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  jq -e '
    .startup_contract.live_client_budget_enforcement.reply_blocking_removed == true
  ' "$startup_contract_path" >/dev/null 2>&1
}

sanitize_gate_payload_for_removed_reply_blocking() {
  local payload="${1:-}"
  [[ -n "$payload" ]] || return 1
  if ! startup_contract_reply_blocking_removed; then
    printf '%s' "$payload"
    return 0
  fi
  printf '%s' "$payload" | jq -c '
    .client_budget_reply_gate.reply_execution_gate.blocking = false
    | .client_budget_reply_gate.reply_execution_gate.must_rotate_before_reply = false
    | .client_budget_reply_gate.reply_execution_gate.must_wait_for_budget_recovery_before_reply = false
    | if (.client_budget_reply_gate.reply_execution_gate.blocking_reply_contract | type) == "object"
      then .client_budget_reply_gate.reply_execution_gate.blocking_reply_contract.active = false
      else .
      end
  ' 2>/dev/null || printf '%s' "$payload"
}

normalize_front_door_gate_payload_shape() {
  local payload="${1:-}"
  [[ -n "$payload" ]] || return 1
  printf '%s' "$payload" | jq -c '
    if (.reply_prefix // null) != null then
      .
    else
      .reply_prefix = (.client_budget_reply_gate.reply_execution_gate.reply_prefix // null)
      | .global_reply_prefix = (.client_budget_reply_gate.reply_execution_gate.global_reply_prefix // null)
      | .reply_prefix_source = (.client_budget_reply_gate.reply_execution_gate.reply_prefix_source // null)
      | .status_label = (.client_budget_reply_gate.status_label // null)
      | .observed_at_epoch_ms = (.client_budget_reply_gate.observed_at_epoch_ms // null)
      | .max_guard_age_seconds = (.client_budget_reply_gate.max_guard_age_seconds // null)
    end
  ' 2>/dev/null || printf '%s' "$payload"
}

maybe_auto_launch_same_thread_host_control() {
  local payload="${1:-}"
  local effective_thread_id="${2:-}"
  [[ -n "$payload" ]] || return 1
  [[ -n "$effective_thread_id" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  local gate_fields
  gate_fields="$(
    printf '%s' "$payload" | jq -r '
      [
        (.client_budget_reply_gate.reply_execution_gate.action_kind // ""),
        (.client_budget_reply_gate.reply_execution_gate.same_meter_pure_burn_turn_active // false),
        (.client_budget_reply_gate.reply_execution_gate.must_avoid_new_tool_turn_without_specific_delta_goal // false),
        (.client_budget_reply_gate.reply_execution_gate.max_tool_roundtrips_soft // -1),
        (.client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.automation_ready // false),
        (.client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.retry_allowed // false),
        (.client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.measurement_pending // false),
        (.client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.feedback_pending // false),
        (.client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.command_id // "")
      ] | @tsv
    ' 2>/dev/null || true
  )"
  [[ -n "$gate_fields" ]] || return 1
  local action_kind same_meter_pure_burn_turn_active stop_loss_active max_tool_roundtrips_soft
  local automation_ready retry_allowed measurement_pending feedback_pending command_id
  IFS=$'\t' read -r \
    action_kind \
    same_meter_pure_burn_turn_active \
    stop_loss_active \
    max_tool_roundtrips_soft \
    automation_ready \
    retry_allowed \
    measurement_pending \
    feedback_pending \
    command_id <<<"$gate_fields"
  [[ "$action_kind" == "compact_current_thread_for_client_budget" ]] || return 1
  [[ "$same_meter_pure_burn_turn_active" == "true" ]] || return 1
  [[ "$stop_loss_active" == "true" ]] || return 1
  [[ "$max_tool_roundtrips_soft" == "0" ]] || return 1
  [[ "$automation_ready" == "true" ]] || return 1
  [[ "$retry_allowed" == "true" ]] || return 1
  [[ "$measurement_pending" != "true" ]] || return 1
  [[ "$feedback_pending" != "true" ]] || return 1
  [[ -n "$command_id" ]] || return 1

  local -a launch_args=(
    observe
    ctl-launch
    --repo-root "$REPO_ROOT"
    --namespace continuity
    --thread-id "$effective_thread_id"
  )
  if [[ "$command_id" == "hotkey-window-open-current" ]]; then
    launch_args+=(--compact-window)
  else
    launch_args+=(--command-id "$command_id")
  fi
  "$SCRIPT_DIR/amai_exec.sh" "${launch_args[@]}" >/dev/null 2>&1 || return 1
  return 0
}

enforce_reply_gate=false
enforce_online_reply_prefix=false
thread_id=""
declare -a passthrough_args=()
declare -a observe_passthrough_args=()

while (($# > 0)); do
  case "$1" in
    --enforce-reply-gate)
      enforce_reply_gate=true
      passthrough_args+=("$1")
      observe_passthrough_args+=("$1")
      shift
      ;;
    --enforce-online-reply-prefix)
      enforce_online_reply_prefix=true
      shift
      ;;
    --thread-id)
      if (($# < 2)); then
        break
      fi
      thread_id="$2"
      passthrough_args+=("$1" "$2")
      observe_passthrough_args+=("$1" "$2")
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

root_cause_api_url="http://${observe_host}:${observe_port}/api/client-budget-root-cause"
gate_api_url="http://${observe_host}:${observe_port}/api/client-budget-gate"
if [[ -n "$thread_id" ]]; then
  encoded_thread_id="$(printf '%s' "$thread_id" | jq -sRr @uri)"
  root_cause_api_url="${root_cause_api_url}?thread_id=${encoded_thread_id}"
  gate_api_url="${gate_api_url}?thread_id=${encoded_thread_id}"
fi
api_max_time="1.5"
if [[ -n "$thread_id" ]]; then
  api_max_time="7"
fi
if [[ "$enforce_online_reply_prefix" == "true" ]]; then
  # Enforcing reply prefix must stay fail-fast even when a thread id is present.
  api_max_time="1.5"
fi
gate_json=""
root_cause_cache_path="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.json"
gate_cache_path="${REPO_ROOT}/state/observe/client_budget_gate_cache.json"
if [[ -n "$thread_id" ]]; then
  safe_thread_id="$(printf '%s' "$thread_id" | tr -c '[:alnum:]_-' '_')"
  root_cause_cache_path="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.thread-${safe_thread_id}.json"
  gate_cache_path="${REPO_ROOT}/state/observe/client_budget_gate_cache.thread-${safe_thread_id}.json"
fi

extract_gate_from_root_cause() {
  jq -c '
    .client_budget_reply_gate as $gate
    | if (($gate.reply_execution_gate.action_kind // null) == null)
      then empty
      else {client_budget_reply_gate: $gate}
      end
  ' 2>/dev/null
}

fresh_compact_client_budget_root_cause_cache_available() {
  local expected_thread_id="${1:-}"
  [[ -f "$root_cause_cache_path" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  local now_ms fetched_at_ms observed_at_ms cache_thread_id
  now_ms="$(date +%s%3N 2>/dev/null || true)"
  [[ -n "$now_ms" ]] || return 1
  fetched_at_ms="$(jq -r '.fetched_at_epoch_ms // 0' "$root_cause_cache_path" 2>/dev/null || printf '0')"
  observed_at_ms="$(
    jq -r '
      .root_cause.client_budget_reply_gate.observed_at_epoch_ms
      // .gate.client_budget_reply_gate.observed_at_epoch_ms
      // .guard.observed_at_epoch_ms
      // 0
    ' "$root_cause_cache_path" 2>/dev/null || printf '0'
  )"
  cache_version="$(jq -r '.cache_version // ""' "$root_cause_cache_path" 2>/dev/null || printf '')"
  cache_thread_id="$(jq -r '.thread_id // ""' "$root_cause_cache_path" 2>/dev/null || printf '')"
  [[ "$fetched_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$observed_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$cache_version" == "$CLIENT_BUDGET_SURFACES_CACHE_VERSION" ]] || return 1
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
  fresh_compact_client_budget_root_cause_cache_available "$expected_thread_id" || return 1
  jq -c '.root_cause' "$root_cause_cache_path" 2>/dev/null || return 1
}

fresh_compact_client_budget_gate_cache_available() {
  local expected_thread_id="${1:-}"
  [[ -f "$gate_cache_path" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  local now_ms fetched_at_ms observed_at_ms cache_thread_id
  now_ms="$(date +%s%3N 2>/dev/null || true)"
  [[ -n "$now_ms" ]] || return 1
  fetched_at_ms="$(jq -r '.fetched_at_epoch_ms // 0' "$gate_cache_path" 2>/dev/null || printf '0')"
  observed_at_ms="$(
    jq -r '
      .gate.client_budget_reply_gate.observed_at_epoch_ms
      // .guard.observed_at_epoch_ms
      // 0
    ' "$gate_cache_path" 2>/dev/null || printf '0'
  )"
  cache_version="$(jq -r '.cache_version // ""' "$gate_cache_path" 2>/dev/null || printf '')"
  cache_thread_id="$(jq -r '.thread_id // ""' "$gate_cache_path" 2>/dev/null || printf '')"
  [[ "$fetched_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$observed_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$cache_version" == "$CLIENT_BUDGET_GATE_CACHE_VERSION" ]] || return 1
  if [[ -n "$expected_thread_id" ]]; then
    [[ "$cache_thread_id" == "$expected_thread_id" ]] || return 1
  else
    [[ -z "$cache_thread_id" ]] || return 1
  fi
  (( now_ms - fetched_at_ms <= 10000 )) || return 1
  (( now_ms - observed_at_ms <= 10000 )) || return 1
}

read_fresh_compact_client_budget_gate_cache() {
  local expected_thread_id="${1:-}"
  fresh_compact_client_budget_gate_cache_available "$expected_thread_id" || return 1
  jq -c '.gate' "$gate_cache_path" 2>/dev/null || return 1
}

compact_client_budget_gate_payload_is_fresh() {
  local payload="${1:-}"
  [[ -n "$payload" ]] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  local now_ms observed_at_ms max_guard_age_seconds
  now_ms="$(date +%s%3N 2>/dev/null || true)"
  [[ -n "$now_ms" ]] || return 1
  observed_at_ms="$(
    printf '%s' "$payload" | jq -r '
      .client_budget_reply_gate.observed_at_epoch_ms
      // .observed_at_epoch_ms
      // 0
    ' 2>/dev/null || printf '0'
  )"
  max_guard_age_seconds="$(
    printf '%s' "$payload" | jq -r '
      .client_budget_reply_gate.max_guard_age_seconds
      // .max_guard_age_seconds
      // 10
    ' 2>/dev/null || printf '10'
  )"
  [[ "$observed_at_ms" =~ ^[0-9]+$ ]] || return 1
  [[ "$max_guard_age_seconds" =~ ^[0-9]+$ ]] || return 1
  (( observed_at_ms > 0 )) || return 1
  (( now_ms - observed_at_ms <= max_guard_age_seconds * 1000 )) || return 1
}

fallback_gate_payload_from_startup_state() {
  command -v jq >/dev/null 2>&1 || return 1
  [[ -x "$REPO_ROOT/target/debug/amai" ]] || return 1
  local prefix now_ms
  prefix="$(
    "$REPO_ROOT/target/debug/amai" continuity startup-state --repo-root "$REPO_ROOT" --json 2>/dev/null |
      jq -r '.startup_runtime_state.reply_execution_gate.reply_prefix // empty' 2>/dev/null || true
  )"
  prefix="$(printf '%s' "$prefix" | sed -e 's/^[[:space:]]\\+//' -e 's/[[:space:]]\\+$//')"
  [[ -n "$prefix" ]] || return 1
  now_ms="$(date +%s%3N 2>/dev/null || true)"
  [[ -n "$now_ms" ]] || return 1
  jq -c -n \
    --arg prefix "$prefix" \
    --argjson observed_at_epoch_ms "$now_ms" \
    '{
      reply_prefix: $prefix,
      reply_prefix_source: "personal_agent_online_limit_contour",
      observed_at_epoch_ms: $observed_at_epoch_ms,
      max_guard_age_seconds: 10,
      client_budget_reply_gate: {
        status: "observed",
        status_label: null,
        observed_at_epoch_ms: $observed_at_epoch_ms,
        max_guard_age_seconds: 10,
        reply_execution_gate: {
          gate_version: "client-reply-budget-gate-v1",
          action_kind: "continue_current_chat",
          blocking: false,
          must_rotate_before_reply: false,
          must_wait_for_budget_recovery_before_reply: false,
          reply_budget_mode: "compact_high_signal",
          reply_prefix: $prefix,
          reply_prefix_source: "personal_agent_online_limit_contour",
          preserves_return_obligation: true
        }
      }
    }'
}

thread_bound_other_thread_feedback_gate_is_consistent() {
  local expected_thread_id="${1:-}"
  local payload="${2:-}"
  [[ -n "$expected_thread_id" ]] || return 0
  [[ -n "$payload" ]] || return 1
  printf '%s' "$payload" | jq -e '
    if (
      .client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.effect_verdict
      // null
    ) != "other_thread" then
      true
    else
      (
        .client_budget_reply_gate.reply_execution_gate.action_kind
        // empty
      ) != "confirm_same_thread_host_control_feedback"
      and (
        .client_budget_reply_gate.reply_execution_gate.must_confirm_same_thread_host_control_feedback_before_reply
        // false
      ) != true
      and (
        .client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.feedback_pending
        // false
      ) != true
    end
  ' >/dev/null 2>&1
}

gate_json="$(read_fresh_compact_client_budget_gate_cache "$thread_id" || true)"
if [[ "$enforce_online_reply_prefix" != "true" ]]; then
  if [[ -n "$gate_json" ]] && ! thread_bound_other_thread_feedback_gate_is_consistent "$thread_id" "$gate_json"; then
    gate_json=""
  fi
fi

if command -v curl >/dev/null 2>&1; then
  if [[ -z "$gate_json" ]]; then
    gate_json="$(curl --silent --show-error --fail --max-time "$api_max_time" "$gate_api_url" 2>/dev/null || true)"
    if [[ "$enforce_online_reply_prefix" != "true" ]]; then
      if [[ -n "$gate_json" ]] && ! thread_bound_other_thread_feedback_gate_is_consistent "$thread_id" "$gate_json"; then
        gate_json=""
      fi
    fi
  fi
fi

if [[ -n "$gate_json" ]] && ! compact_client_budget_gate_payload_is_fresh "$gate_json"; then
  gate_json=""
fi

if [[ -z "$gate_json" ]]; then
  if [[ "$enforce_online_reply_prefix" == "true" ]]; then
    gate_json="$(fallback_gate_payload_from_startup_state || true)"
    if [[ -z "$gate_json" ]]; then
      echo "client budget gate: no fresh payload available for online reply prefix enforcement" >&2
      exit 2
    fi
  else
    if command -v timeout >/dev/null 2>&1; then
      gate_json="$(
        timeout 12 env \
          AMAI_EXEC_DISABLE_BUDGET_HELPERS=1 \
          AMI_CLIENT_BUDGET_OBSERVE_HTTP_TIMEOUT_MS=1500 \
            "$SCRIPT_DIR/amai_exec.sh" observe client-budget-gate "${observe_passthrough_args[@]}" \
          2>/dev/null || true
      )"
    else
      gate_json="$(
        AMAI_EXEC_DISABLE_BUDGET_HELPERS=1 \
        AMI_CLIENT_BUDGET_OBSERVE_HTTP_TIMEOUT_MS=1500 \
          "$SCRIPT_DIR/amai_exec.sh" observe client-budget-gate "${observe_passthrough_args[@]}" \
          2>/dev/null || true
      )"
    fi
  fi
fi

if [[ "$enforce_online_reply_prefix" != "true" ]]; then
  if [[ -n "$gate_json" ]] && ! thread_bound_other_thread_feedback_gate_is_consistent "$thread_id" "$gate_json"; then
    gate_json=""
  fi
fi

if [[ -n "$gate_json" ]] && ! compact_client_budget_gate_payload_is_fresh "$gate_json"; then
  gate_json=""
fi

if [[ -z "$gate_json" ]]; then
  root_cause_json="$(read_fresh_compact_client_budget_root_cause_cache "$thread_id" || true)"
  if [[ -n "$root_cause_json" ]]; then
    gate_json="$(printf '%s' "$root_cause_json" | extract_gate_from_root_cause || true)"
    if [[ -n "$gate_json" ]] && ! thread_bound_other_thread_feedback_gate_is_consistent "$thread_id" "$gate_json"; then
      gate_json=""
    fi
  fi
fi

if [[ -z "$gate_json" ]] && [[ -n "$thread_id" ]]; then
  root_cause_json="$("$SCRIPT_DIR/client_budget_root_cause.sh" "${passthrough_args[@]}" 2>/dev/null || true)"
  if [[ -n "$root_cause_json" ]]; then
    gate_json="$(printf '%s' "$root_cause_json" | extract_gate_from_root_cause || true)"
    if [[ -n "$gate_json" ]] && ! thread_bound_other_thread_feedback_gate_is_consistent "$thread_id" "$gate_json"; then
      gate_json=""
    fi
  fi
fi

if [[ -z "$gate_json" ]]; then
  if command -v curl >/dev/null 2>&1; then
    root_cause_json="$(curl --silent --show-error --fail --max-time "$api_max_time" "$root_cause_api_url" 2>/dev/null || true)"
    if [[ -n "$root_cause_json" ]]; then
      gate_json="$(printf '%s' "$root_cause_json" | extract_gate_from_root_cause || true)"
    fi
  fi
fi

gate_json="$(sanitize_gate_payload_for_removed_reply_blocking "$gate_json")"
gate_json="$(normalize_front_door_gate_payload_shape "$gate_json")"
if [[ "$enforce_online_reply_prefix" != "true" ]]; then
  if maybe_auto_launch_same_thread_host_control "$gate_json" "$thread_id"; then
    sleep 0.35
    if command -v curl >/dev/null 2>&1; then
      refreshed_gate_json="$(curl --silent --show-error --fail --max-time "$api_max_time" "$gate_api_url" 2>/dev/null || true)"
      if [[ -n "$refreshed_gate_json" ]] && compact_client_budget_gate_payload_is_fresh "$refreshed_gate_json"; then
        gate_json="$(sanitize_gate_payload_for_removed_reply_blocking "$refreshed_gate_json")"
        gate_json="$(normalize_front_door_gate_payload_shape "$gate_json")"
      fi
    fi
  fi
fi

printf '%s\n' "$gate_json"

if [[ "$enforce_reply_gate" == "true" ]]; then
  reply_blocked="$(
    printf '%s' "$gate_json" | jq -r '
      (
        .client_budget_reply_gate.reply_execution_gate.blocking
        // .client_budget_reply_gate.reply_execution_gate.must_rotate_before_reply
        // .client_budget_reply_gate.reply_execution_gate.must_wait_for_budget_recovery_before_reply
        // false
      ) | if . then "true" else "false" end
    ' 2>/dev/null || printf 'false'
  )"
  if [[ "$reply_blocked" == "true" ]]; then
    echo "client budget gate blocked this reply" >&2
    exit 1
  fi
fi

if [[ "$enforce_online_reply_prefix" == "true" ]]; then
  prefix_readiness="$(
    printf '%s' "$gate_json" | jq -r '
      ((.reply_prefix // "") | tostring | gsub("^\\s+|\\s+$"; "")) as $prefix
      | ((.reply_prefix_source // "") | tostring) as $source
      | if ($prefix | length) == 0 then
          "missing_reply_prefix"
        elif $prefix == "5ч KPI: н/д" then
          "reply_prefix_not_materialized"
        elif $source != "personal_agent_online_limit_contour" then
          "wrong_reply_prefix_source:" + $source
        else
          "ready"
        end
    ' 2>/dev/null || printf 'invalid_gate_payload'
  )"
  if [[ "$prefix_readiness" != "ready" ]]; then
    echo "client budget gate missing required online reply prefix: ${prefix_readiness}" >&2
    exit 2
  fi
fi
