#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

guard_json="$(cargo run --quiet -- observe client-budget-gate)"
reply_blocked="$(printf '%s' "$guard_json" | jq -r '
  .client_budget_reply_gate.reply_execution_gate.blocking
  // .client_budget_reply_gate.reply_execution_gate.must_rotate_before_reply
  // .client_budget_reply_gate.reply_execution_gate.must_wait_for_budget_recovery_before_reply
  // false
')"

if [[ "$reply_blocked" != "true" ]]; then
  exit 0
fi

blocked_reply="$(printf '%s' "$guard_json" | jq -r '
  .client_budget_reply_gate.reply_execution_gate.blocking_reply_contract.template // empty
')"

if [[ -z "$blocked_reply" ]]; then
  blocked_reply="$(jq -r '
    .startup_contract.live_client_budget_enforcement.blocking_reply_template // empty
  ' .amai/onboarding/project-chat-startup-contract.json)"
fi

if [[ -z "$blocked_reply" ]]; then
  echo "client budget guard blocked the reply, but no blocking reply template is available" >&2
  exit 11
fi

printf '%s\n' "$blocked_reply"
exit 10
