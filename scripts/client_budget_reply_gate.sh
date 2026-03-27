#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

guard_json="$(cargo run --quiet -- observe client-budget-guard)"
must_rotate="$(printf '%s' "$guard_json" | jq -r '
  .reply_execution_gate.must_rotate_before_reply
  // .reply_execution_gate.blocking
  // .should_rotate_chat_now
  // .should_rotate_chat_soon
  // false
')"

if [[ "$must_rotate" != "true" ]]; then
  exit 0
fi

blocked_reply="$(printf '%s' "$guard_json" | jq -r '
  .reply_execution_gate.blocking_reply_contract.template // empty
')"

if [[ -z "$blocked_reply" ]]; then
  echo "client budget guard blocked the reply, but blocking_reply_contract.template is missing" >&2
  exit 11
fi

printf '%s\n' "$blocked_reply"
exit 10
