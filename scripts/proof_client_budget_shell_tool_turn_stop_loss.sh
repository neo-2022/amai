#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

cat >"$tmpdir/pure-burn-gate.json" <<'EOF'
{
  "client_budget_reply_gate": {
    "reply_execution_gate": {
      "action_kind": "rotate_chat_for_client_budget",
      "reply_prefix": "5ч KPI: экономия 12.00%",
      "same_meter_pure_burn_turn_active": true,
      "must_avoid_new_tool_turn_without_specific_delta_goal": true,
      "max_tool_roundtrips_soft": 0
    }
  }
}
EOF

cat >"$tmpdir/zero-roundtrip-gate.json" <<'EOF'
{
  "client_budget_reply_gate": {
    "reply_execution_gate": {
      "action_kind": "compact_current_thread_for_client_budget",
      "reply_prefix": "5ч KPI: переплата 8.00%",
      "same_meter_pure_burn_turn_active": false,
      "must_avoid_new_tool_turn_without_specific_delta_goal": true,
      "max_tool_roundtrips_soft": 0
    }
  }
}
EOF

pure_burn_output="$(
  AMAI_CLIENT_BUDGET_GATE_FILE="$tmpdir/pure-burn-gate.json" \
    ./scripts/client_budget_tool_turn_gate.sh --tool-name continuity_answer --json
)" || pure_burn_status=$?
pure_burn_status="${pure_burn_status:-0}"
if [[ "$pure_burn_status" -ne 10 ]]; then
  echo "proof_client_budget_shell_tool_turn_stop_loss: expected pure-burn block" >&2
  exit 1
fi

printf '%s\n' "$pure_burn_output" | jq -e '
  .isError == true
  and .structuredContent.error_taxonomy.amai_error_code == "tool_blocked_by_live_client_budget_gate"
  and .structuredContent.blocked_tool == "continuity_answer"
  and .structuredContent.same_meter_pure_burn_turn_active == true
  and .structuredContent.expensive_tool_turn_stop_loss_reason == "same_meter_pure_burn_turn"
  and .structuredContent.client_budget_reply_gate.reply_execution_gate.reply_prefix == "5ч KPI: экономия 12.00%"
' >/dev/null

zero_roundtrip_output="$(
  AMAI_CLIENT_BUDGET_GATE_FILE="$tmpdir/zero-roundtrip-gate.json" \
    ./scripts/client_budget_tool_turn_gate.sh --tool-name continuity_restore --json
)" || zero_roundtrip_status=$?
zero_roundtrip_status="${zero_roundtrip_status:-0}"
if [[ "$zero_roundtrip_status" -ne 10 ]]; then
  echo "proof_client_budget_shell_tool_turn_stop_loss: expected zero-roundtrip block" >&2
  exit 1
fi

printf '%s\n' "$zero_roundtrip_output" | jq -e '
  .isError == true
  and .structuredContent.blocked_tool == "continuity_restore"
  and .structuredContent.same_meter_pure_burn_turn_active == false
  and .structuredContent.expensive_tool_turn_stop_loss_reason == "zero_tool_roundtrips_live_gate"
  and (.content[0].text | contains("wait until current-thread compaction changes the live budget gate"))
' >/dev/null

echo "proof_client_budget_shell_tool_turn_stop_loss: PASS"
