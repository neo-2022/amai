#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

payload="$("${SCRIPT_DIR}/client_budget_system_markers.sh")"

printf '%s\n' "${payload}" | jq -e '
  .marker_contract.version == "client-budget-system-markers-v3"
  and (.economic_markers.reply_execution_gate.action_kind | type) == "string"
  and (
    (.economic_markers.operator_action.primary_command_kind | type) == "string"
    or .economic_markers.operator_action.primary_command_kind == null
  )
  and (.economic_markers.current_live_turn.status | type) == "string"
  and (.economic_markers.host_context_compaction.stage | type) == "string"
  and (.economic_markers.host_current_thread_control_effect.command_id | type) == "string"
  and (.economic_markers.host_current_thread_control_effect.effect_verdict | type) == "string"
  and (.economic_markers.host_current_thread_control_effect.retry_allowed | type) == "boolean"
  and ((.economic_markers.prefix_drift_abs_percent | type) == "number" or .economic_markers.prefix_drift_abs_percent == null)
  and (.economic_markers.prefix_drift_within_tolerance | type) == "boolean"
  and (.economic_markers.root_cause_age_within_guard | type) == "boolean"
  and (.continuity_markers.startup_execution_gate.action_kind | type) == "string"
  and (.continuity_markers.execctl_resume_state | type) == "string"
  and ((.continuity_markers.startup_runtime_state_source == "runtime_artifact") or (.continuity_markers.startup_runtime_state_source == "cli_fallback"))
  and (.marker_integrity.startup_gate_ready == true)
  and (.marker_integrity.exact_prefix_drift_within_tolerance | type) == "boolean"
  and (.marker_integrity.reply_gate_present == true)
  and (.marker_integrity.reply_gate_fresh == true)
' >/dev/null

echo "proof_client_budget_system_markers: PASS"
