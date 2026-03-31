#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

if [[ "${AMAI_ALLOW_EXPENSIVE_TOOL_TURN:-0}" == "1" ]]; then
  exit 0
fi

tool_name="amai_shell_tool"
json_output=false

while (($# > 0)); do
  case "$1" in
    --tool-name)
      tool_name="$2"
      shift 2
      ;;
    --tool-name=*)
      tool_name="${1#*=}"
      shift
      ;;
    --json|--json=*)
      json_output=true
      shift
      ;;
    *)
      shift
      ;;
  esac
done

gate_json=""
if [[ -n "${AMAI_CLIENT_BUDGET_GATE_FILE:-}" && -f "${AMAI_CLIENT_BUDGET_GATE_FILE}" ]]; then
  gate_json="$(cat "$AMAI_CLIENT_BUDGET_GATE_FILE")"
elif [[ -n "${AMAI_CLIENT_BUDGET_GATE_JSON:-}" ]]; then
  gate_json="${AMAI_CLIENT_BUDGET_GATE_JSON}"
else
  gate_json="$("$SCRIPT_DIR/client_budget_gate.sh" 2>/dev/null || true)"
fi

if [[ -z "$gate_json" ]]; then
  echo "live client budget gate is unavailable for tool-turn preflight" >&2
  exit 11
fi

gate_fields="$(
  printf '%s' "$gate_json" | jq -r '
    [
      (
        .client_budget_reply_gate.reply_execution_gate.same_meter_pure_burn_turn_active
        // false
      ),
      (
        .client_budget_reply_gate.reply_execution_gate.must_avoid_new_tool_turn_without_specific_delta_goal
        // false
      ),
      (
        .client_budget_reply_gate.reply_execution_gate.max_tool_roundtrips_soft
        // -1
      ),
      (
        .client_budget_reply_gate.reply_execution_gate.reply_prefix
        // .client_budget_reply_gate.reply_prefix
        // empty
      ),
      (
        .client_budget_reply_gate.reply_execution_gate.action_kind
        // "continue_current_chat"
      )
    ] | @tsv
  ' 2>/dev/null || true
)"
IFS=$'\t' read -r same_meter_pure_burn_turn_active must_avoid_new_tool_turn_without_specific_delta_goal max_tool_roundtrips_soft reply_prefix action_kind <<<"$gate_fields"

if [[ "$same_meter_pure_burn_turn_active" != "true" ]] && ! {
  [[ "$must_avoid_new_tool_turn_without_specific_delta_goal" == "true" ]] &&
    [[ "$max_tool_roundtrips_soft" == "0" ]]
}; then
  exit 0
fi

stop_loss_reason="zero_tool_roundtrips_live_gate"
blocked_hint="refresh the live client budget gate before retrying this tool"
if [[ "$same_meter_pure_burn_turn_active" == "true" ]]; then
  stop_loss_reason="same_meter_pure_burn_turn"
  blocked_hint="avoid a new expensive Amai tool turn until you have a specific material delta goal or after compaction/rotation changes the live budget gate"
else
  case "$action_kind" in
    wait_for_global_client_budget_recovery)
      blocked_hint="wait for global client budget recovery before retrying this tool"
      ;;
    rotate_chat_for_client_budget)
      blocked_hint="rotate into a fresh chat before retrying this tool"
      ;;
    compact_current_thread_for_client_budget)
      blocked_hint="wait until current-thread compaction changes the live budget gate before retrying this tool"
      ;;
  esac
fi

if [[ "$json_output" == "true" ]]; then
  compact_gate_json="$(printf '%s' "$gate_json" | jq -c '.client_budget_reply_gate')"
  jq -cn \
    --arg tool_name "$tool_name" \
    --arg reply_prefix "$reply_prefix" \
    --arg blocked_hint "$blocked_hint" \
    --arg stop_loss_reason "$stop_loss_reason" \
    --argjson same_meter_pure_burn_turn_active "$([[ "$same_meter_pure_burn_turn_active" == "true" ]] && printf 'true' || printf 'false')" \
    --argjson expensive_tool_turn_stop_loss_active true \
    --argjson client_budget_reply_gate "$compact_gate_json" '
      {
        content: [
          {
            type: "text",
            text: (
              if ($reply_prefix | length) > 0
              then ($reply_prefix + "\ntool blocked by live client budget gate: " + $blocked_hint)
              else ("tool blocked by live client budget gate: " + $blocked_hint)
              end
            )
          }
        ],
        isError: true,
        structuredContent: {
          error_taxonomy: {
            amai_error_code: "tool_blocked_by_live_client_budget_gate",
            amai_error_class: "tool_budget_guard",
            retryable: true
          },
          blocked_tool: $tool_name,
          same_meter_pure_burn_turn_active: $same_meter_pure_burn_turn_active,
          expensive_tool_turn_stop_loss_active: $expensive_tool_turn_stop_loss_active,
          expensive_tool_turn_stop_loss_reason: $stop_loss_reason,
          client_budget_reply_gate: $client_budget_reply_gate
        }
      }
    '
else
  if [[ -n "$reply_prefix" ]]; then
    printf '%s\n' "$reply_prefix"
  fi
  printf 'tool blocked by live client budget gate: %s\n' "$blocked_hint"
fi

exit 10
