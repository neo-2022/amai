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
)"
if [[ -n "$pure_burn_output" ]]; then
  echo "proof_client_budget_shell_tool_turn_stop_loss: continuity_answer should stay advisory-only under pure burn" >&2
  exit 1
fi

continuity_import_output="$(
  AMAI_CLIENT_BUDGET_GATE_FILE="$tmpdir/pure-burn-gate.json" \
    ./scripts/client_budget_tool_turn_gate.sh --tool-name continuity_import --json
)"
if [[ -n "$continuity_import_output" ]]; then
  echo "proof_client_budget_shell_tool_turn_stop_loss: continuity import should bypass pure-burn stop loss" >&2
  exit 1
fi

continuity_handoff_output="$(
  AMAI_CLIENT_BUDGET_GATE_FILE="$tmpdir/pure-burn-gate.json" \
    ./scripts/client_budget_tool_turn_gate.sh --tool-name continuity_handoff --json
)"
if [[ -n "$continuity_handoff_output" ]]; then
  echo "proof_client_budget_shell_tool_turn_stop_loss: continuity handoff should bypass pure-burn stop loss" >&2
  exit 1
fi

zero_roundtrip_output="$(
  AMAI_CLIENT_BUDGET_GATE_FILE="$tmpdir/zero-roundtrip-gate.json" \
    ./scripts/client_budget_tool_turn_gate.sh --tool-name continuity_restore --json
)"
if [[ -n "$zero_roundtrip_output" ]]; then
  echo "proof_client_budget_shell_tool_turn_stop_loss: continuity_restore should stay advisory-only under zero-roundtrip pressure" >&2
  exit 1
fi

echo "proof_client_budget_shell_tool_turn_stop_loss: PASS"
