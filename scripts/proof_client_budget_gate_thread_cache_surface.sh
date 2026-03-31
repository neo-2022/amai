#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
THREAD_ID="thread-proof-compact"
CACHE_PATH="${REPO_ROOT}/state/observe/client_budget_gate_cache.thread-${THREAD_ID}.json"

cd "${REPO_ROOT}"

backup_path=""
if [[ -f "${CACHE_PATH}" ]]; then
  backup_path="$(mktemp)"
  cp "${CACHE_PATH}" "${backup_path}"
fi

cleanup() {
  if [[ -n "${backup_path}" && -f "${backup_path}" ]]; then
    mkdir -p "$(dirname "${CACHE_PATH}")"
    mv "${backup_path}" "${CACHE_PATH}"
  else
    rm -f "${CACHE_PATH}"
  fi
}
trap cleanup EXIT

mkdir -p "$(dirname "${CACHE_PATH}")"
now_ms="$(date +%s%3N)"

cat >"${CACHE_PATH}" <<EOF
{
  "cache_version": "client-budget-gate-cache-v2",
  "fetched_at_epoch_ms": ${now_ms},
  "thread_id": "${THREAD_ID}",
  "gate": {
    "client_budget_reply_gate": {
      "status_label": "сожми текущий чат сейчас",
      "observed_at_epoch_ms": ${now_ms},
      "max_guard_age_seconds": 10,
      "reply_execution_gate": {
        "action_kind": "compact_current_thread_for_client_budget",
        "blocking": false,
        "reply_budget_mode": "compact_high_signal",
        "reply_prefix": "5ч KPI: переплата 12.34%",
        "same_meter_pure_burn_turn_active": true,
        "max_tool_roundtrips_soft": 0,
        "action_bundle": {
          "operator_flow": {
            "primary_command_kind": "same_thread_host_control_launch_command",
            "host_current_thread_control_launch_command": "amai observe ctl-launch --thread-id ${THREAD_ID} --compact-window"
          },
          "host_current_thread_control": {
            "command_id": "hotkey-window-open-current",
            "button_label": "Open compact window",
            "retry_allowed": true
          }
        }
      }
    }
  },
  "guard": {
    "observed_at_epoch_ms": ${now_ms},
    "reply_execution_gate": {
      "action_kind": "compact_current_thread_for_client_budget",
      "reply_prefix": "5ч KPI: переплата 12.34%"
    }
  }
}
EOF

output="$(
  CODEX_THREAD_ID="${THREAD_ID}" \
  AMI_OBSERVE_BIND=127.0.0.1:1 \
    "${SCRIPT_DIR}/client_budget_gate.sh"
)"

printf '%s\n' "${output}" | jq -e '
  .client_budget_reply_gate.reply_execution_gate.action_kind == "compact_current_thread_for_client_budget"
  and .client_budget_reply_gate.reply_execution_gate.action_bundle.operator_flow.primary_command_kind == "same_thread_host_control_launch_command"
  and .client_budget_reply_gate.reply_execution_gate.action_bundle.operator_flow.host_current_thread_control_launch_command == "amai observe ctl-launch --thread-id thread-proof-compact --compact-window"
  and .client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.command_id == "hotkey-window-open-current"
  and .client_budget_reply_gate.reply_execution_gate.action_bundle.host_current_thread_control.retry_allowed == true
' >/dev/null

echo "proof_client_budget_gate_thread_cache_surface: PASS"
