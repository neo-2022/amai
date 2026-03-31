#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CACHE_PATH="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.json"

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
  "cache_version": "client-budget-surfaces-cache-v1",
  "fetched_at_epoch_ms": ${now_ms},
  "root_cause": {
    "client_budget_reply_gate": {
      "max_guard_age_seconds": 10,
      "observed_at_epoch_ms": ${now_ms},
      "reply_execution_gate": {
        "action_kind": "compact_current_thread_for_client_budget",
        "blocking": false,
        "must_rotate_before_reply": false,
        "must_wait_for_budget_recovery_before_reply": false,
        "reply_budget_mode": "compact_high_signal",
        "reply_prefix": "5ч KPI: переплата 12.34%",
        "same_meter_pure_burn_turn_active": false,
        "must_require_material_delta_before_next_reply": false,
        "must_avoid_progress_reply_when_only_guard_changed": false,
        "must_avoid_new_tool_turn_without_specific_delta_goal": false,
        "max_tool_roundtrips_soft": 1
      }
    },
    "current_live_meter": {
      "client_turn_total_tokens": 12345,
      "context_used_percent": 4.78
    },
    "current_live_turn": {
      "saved_pct": null,
      "status": "exact_pair_materialized"
    },
    "exact_pair_status": {
      "state": "exact_pair_materialized"
    },
    "guard": {
      "should_rotate_chat_now": false
    },
    "host_context_compaction": {
      "growth_since_compaction_tokens": 321,
      "regrowth_of_recovered_surface_ratio": 0.02,
      "stage": "preserve"
    },
    "thread_binding_state": "current_thread_bound"
  },
  "gate": {
    "client_budget_reply_gate": {
      "observed_at_epoch_ms": ${now_ms},
      "reply_execution_gate": {
        "action_kind": "compact_current_thread_for_client_budget",
        "reply_prefix": "5ч KPI: переплата 12.34%"
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

payload="$(AMI_OBSERVE_BIND=127.0.0.1:1 "${SCRIPT_DIR}/client_budget_root_cause.sh" --enforce-reply-gate)"

printf '%s\n' "${payload}" | jq -e '
  .client_budget_reply_gate.reply_execution_gate.reply_prefix == "5ч KPI: переплата 12.34%"
  and .client_budget_reply_gate.reply_execution_gate.action_kind == "compact_current_thread_for_client_budget"
  and .thread_binding_state == "current_thread_bound"
  and .host_context_compaction.stage == "preserve"
' >/dev/null

echo "proof_client_budget_root_cause_cache_fallback: PASS"
