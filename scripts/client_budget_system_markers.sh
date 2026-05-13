#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
KPI_SCRIPT_DEFAULT="${HOME}/.codex/skills/vscode-5h-kpi-prefix/scripts/read_kpi_prefix.sh"
KPI_SCRIPT="${AMAI_KPI_SCRIPT_PATH:-${KPI_SCRIPT_DEFAULT}}"
STARTUP_STATE_ARTIFACT="${REPO_ROOT}/.amai/continuity/project-chat-startup-state.json"
STARTUP_STATE_ARTIFACT_VERSION="workspace-startup-runtime-state-v4"
PREFIX_DRIFT_TOLERANCE_PERCENT="10"

cd "${REPO_ROOT}"

root_cause_freshness() {
  printf '%s\n' "$1" | jq -r '
    (.client_budget_reply_gate.observed_at_epoch_ms // 0) as $observed
    | (.client_budget_reply_gate.max_guard_age_seconds | if type == "number" then (. * 1000) else 10000 end) as $max_age
    | if $observed <= 0 then "stale"
      elif (((now * 1000) | floor) - $observed) > $max_age then "stale"
      else "fresh"
      end
  '
}

root_cause_payload_is_materialized() {
  local payload="${1:-}"
  [[ -n "${payload}" ]] || return 1
  printf '%s\n' "${payload}" | jq -e '
    (.client_budget_reply_gate | type) == "object"
    and (.client_budget_reply_gate.reply_execution_gate | type) == "object"
  ' >/dev/null 2>&1
}

startup_state_source="cli_fallback"
if [[ -r "${STARTUP_STATE_ARTIFACT}" ]] \
  && jq -e \
    --arg version "${STARTUP_STATE_ARTIFACT_VERSION}" \
    '.artifact_version == $version and .gate_semantics_consistent == true and (.startup_execution_gate | type) == "object"' \
    "${STARTUP_STATE_ARTIFACT}" >/dev/null 2>&1; then
  startup_state_json="$(cat "${STARTUP_STATE_ARTIFACT}")"
  startup_state_source="runtime_artifact"
else
  startup_state_json="$("${SCRIPT_DIR}/continuity_startup_state.sh" --repo-root "${REPO_ROOT}" --json)"
fi

toolbar_kpi="5ч KPI: н/д"
if [[ -x "${KPI_SCRIPT}" ]]; then
  toolbar_kpi="$("${KPI_SCRIPT}" 2>/dev/null || printf '5ч KPI: н/д')"
fi

fresh_root_cause_json() {
  local payload=""
  if [[ -x "${SCRIPT_DIR}/client_budget_root_cause.sh" ]]; then
    payload="$("${SCRIPT_DIR}/client_budget_root_cause.sh" --enforce-reply-gate 2>/dev/null || true)"
  fi
  if root_cause_payload_is_materialized "${payload}" \
    && [[ "$(root_cause_freshness "${payload}")" == "fresh" ]]; then
    printf '%s\n' "${payload}"
    return 0
  fi

  if [[ -x "${SCRIPT_DIR}/client_budget_root_cause.sh" ]]; then
    payload="$("${SCRIPT_DIR}/client_budget_root_cause.sh" --enforce-reply-gate 2>/dev/null || true)"
  else
    payload=""
  fi
  if root_cause_payload_is_materialized "${payload}" \
    && [[ "$(root_cause_freshness "${payload}")" == "fresh" ]]; then
    printf '%s\n' "${payload}"
    return 0
  fi

  if [[ -x "${REPO_ROOT}/target/release/amai" ]]; then
    payload="$(
      AMAI_EXEC_DISABLE_BUDGET_HELPERS=1 \
        "${REPO_ROOT}/target/release/amai" observe client-budget-root-cause --enforce-reply-gate
    )"
  elif [[ -x "${REPO_ROOT}/target/debug/amai" ]]; then
    payload="$(
      AMAI_EXEC_DISABLE_BUDGET_HELPERS=1 \
        "${REPO_ROOT}/target/debug/amai" observe client-budget-root-cause --enforce-reply-gate
    )"
  fi

  if ! root_cause_payload_is_materialized "${payload}"; then
    echo "client budget system markers: no root cause payload available" >&2
    return 12
  fi

  printf '%s\n' "${payload}"
}

if ! root_cause_json="$(fresh_root_cause_json)"; then
  status=$?
  if [[ ${status} -eq 0 ]]; then
    status=12
  fi
  exit "${status}"
fi

jq -n \
  --arg toolbar_kpi "${toolbar_kpi}" \
  --arg startup_state_source "${startup_state_source}" \
  --argjson prefix_drift_tolerance_percent "${PREFIX_DRIFT_TOLERANCE_PERCENT}" \
  --argjson root_cause "${root_cause_json}" \
  --argjson startup_state "${startup_state_json}" '
  def prefix_family($s):
    if ($s | type) != "string" then "unknown"
    elif ($s | startswith("5ч KPI: экономия ")) then "saving"
    elif $s == "5ч KPI: 1:1" then "aligned"
    elif ($s | startswith("5ч KPI: переплата ")) then "overspend"
    elif ($s | startswith("5ч KPI: н/д")) then "missing"
    else "unknown"
    end;
  def prefix_percent($s):
    if ($s | type) != "string" then null
    elif (try ($s | capture("(?<percent>[0-9]+(?:\\.[0-9]+)?)%")) catch null) != null then
      ((try ($s | capture("(?<percent>[0-9]+(?:\\.[0-9]+)?)%")) catch null).percent | tonumber?)
    else null
    end;
  def age_ms($ts):
    if ($ts | type) == "number" then (((now * 1000) | floor) - $ts) else null end;
  def prefix_drift_abs($left; $right):
    (prefix_percent($left) as $lp | prefix_percent($right) as $rp |
      if $lp == null or $rp == null then null else (($lp - $rp) | if . < 0 then -. else . end) end);
  ($startup_state.startup_runtime_state // $startup_state) as $ss
  | ($root_cause.client_budget_reply_gate // {}) as $reply_gate
  | ($reply_gate.reply_execution_gate // {}) as $gate
  | (($gate.action_bundle // {}) | .operator_flow // {}) as $operator_flow
  | (($gate.action_bundle // {}) | .host_current_thread_control // {}) as $host_control
  | ($root_cause.host_current_thread_control_effect // {}) as $host_effect
  | (($root_cause.client_budget_reply_gate.global_reply_prefix // $root_cause.client_budget_reply_gate.reply_execution_gate.global_reply_prefix // $root_cause.client_budget_reply_gate.reply_execution_gate.reply_prefix)) as $global_reply_prefix
  | (prefix_drift_abs($toolbar_kpi; $global_reply_prefix)) as $prefix_drift_abs
  | ($reply_gate.max_guard_age_seconds | if type == "number" then (. * 1000) else 10000 end) as $max_guard_age_ms
  | (age_ms($reply_gate.observed_at_epoch_ms)) as $root_cause_age_ms
  | $max_guard_age_ms as $gate_max_age_ms
  | $root_cause_age_ms as $gate_age_ms
  |
  {
    marker_contract: {
      version: "client-budget-system-markers-v3",
      required_sources: [
        "vscode_toolbar_5h_kpi",
        "observe_client_budget_root_cause_enforced",
        "continuity_startup_runtime_state"
      ]
    },
    economic_markers: {
      toolbar_5h_prefix: $toolbar_kpi,
      internal_reply_prefix: ($gate.reply_prefix // ""),
      internal_global_reply_prefix: ($global_reply_prefix // ""),
      prefix_family_match: (prefix_family($toolbar_kpi) == prefix_family($global_reply_prefix // "")),
      prefix_drift_abs_percent: $prefix_drift_abs,
      prefix_drift_within_tolerance:
        ($prefix_drift_abs == null or $prefix_drift_abs <= $prefix_drift_tolerance_percent),
      root_cause_observed_at_epoch_ms: $reply_gate.observed_at_epoch_ms,
      root_cause_age_ms: $root_cause_age_ms,
      max_guard_age_ms: $max_guard_age_ms,
      root_cause_age_within_guard:
        ($root_cause_age_ms != null and $root_cause_age_ms <= $max_guard_age_ms),
      gate_observed_at_epoch_ms: $reply_gate.observed_at_epoch_ms,
      gate_age_ms: $gate_age_ms,
      gate_age_within_guard:
        ($gate_age_ms != null and $gate_age_ms <= $gate_max_age_ms),
      reply_execution_gate: {
        action_kind: ($gate.action_kind // ""),
        blocking: ($gate.blocking // false),
        must_rotate_before_reply: ($gate.must_rotate_before_reply // false),
        must_wait_for_budget_recovery_before_reply: ($gate.must_wait_for_budget_recovery_before_reply // false),
        must_confirm_same_thread_host_control_feedback_before_reply: ($gate.must_confirm_same_thread_host_control_feedback_before_reply // false),
        must_wait_for_same_thread_effect_measurement_before_reply: ($gate.must_wait_for_same_thread_effect_measurement_before_reply // false),
        reply_budget_mode: ($gate.reply_budget_mode // ""),
        reply_prefix: ($gate.reply_prefix // ""),
        global_reply_prefix: ($gate.global_reply_prefix // $global_reply_prefix // ""),
        reply_prefix_source: ($gate.reply_prefix_source // ""),
        host_context_compaction_inactive_target_pressure_active: ($gate.host_context_compaction_inactive_target_pressure_active // false),
        same_meter_pure_burn_turn_active: ($gate.same_meter_pure_burn_turn_active // false),
        must_prefer_short_paragraphs: ($gate.must_prefer_short_paragraphs // false),
        must_avoid_commentary_only_updates: ($gate.must_avoid_commentary_only_updates // false),
        must_batch_all_tool_reads_before_reply: ($gate.must_batch_all_tool_reads_before_reply // false),
        must_wait_for_meaningful_result_before_progress_reply: ($gate.must_wait_for_meaningful_result_before_progress_reply // false),
        must_require_material_delta_before_next_reply: ($gate.must_require_material_delta_before_next_reply // false),
        must_avoid_progress_reply_when_only_guard_changed: ($gate.must_avoid_progress_reply_when_only_guard_changed // false),
        must_avoid_new_tool_turn_without_specific_delta_goal: ($gate.must_avoid_new_tool_turn_without_specific_delta_goal // false),
        max_bullets_soft: ($gate.max_bullets_soft // 0),
        max_sentences_soft: ($gate.max_sentences_soft // 0),
        max_tool_roundtrips_soft: ($gate.max_tool_roundtrips_soft // 0)
      },
      operator_action: {
        primary_command_kind: $operator_flow.primary_command_kind,
        primary_command: $operator_flow.primary_command,
        rotate_helper_command: $operator_flow.rotate_helper_command,
        host_current_thread_control_launch_command: $operator_flow.host_current_thread_control_launch_command,
        host_current_thread_control: {
          command_id: ($host_control.command_id // ""),
          button_label: ($host_control.button_label // ""),
          automation_ready: ($host_control.automation_ready // false),
          retry_allowed: ($host_control.retry_allowed // false),
          retry_blocked_reason: $host_control.retry_blocked_reason,
          measurement_pending: ($host_control.measurement_pending // false),
          control_kind: ($host_control.control_kind // ""),
          host_surface_kind: $host_control.host_surface_kind,
          summary: $host_control.summary,
          note: $host_control.note
        }
      },
      current_live_meter: ($root_cause.current_live_meter // {}),
      current_live_turn: ($root_cause.current_live_turn // {"status": ""}),
      same_meter_economics: ($root_cause.same_meter_economics // {}),
      host_context_compaction: ($root_cause.host_context_compaction // {"stage": ""}),
      host_current_thread_control_effect: {
        command_id: ($host_effect.command_id // ""),
        effect_verdict: ($host_effect.effect_verdict // "unmeasured"),
        measurement_pending: ($host_effect.measurement_pending // false),
        measurement_sufficient: ($host_effect.measurement_sufficient // false),
        retry_allowed: ($host_effect.retry_allowed // false),
        rotate_fallback_recommended: ($host_effect.rotate_fallback_recommended // false),
        full_scale_client_burn_worsened: ($host_effect.full_scale_client_burn_worsened // false),
        verified_host_compaction_observed_after_feedback: ($host_effect.verified_host_compaction_observed_after_feedback // false),
        turn_token_delta: $host_effect.turn_token_delta,
        context_used_percent_point_delta: $host_effect.context_used_percent_point_delta,
        primary_limit_used_percent_point_delta: $host_effect.primary_limit_used_percent_point_delta,
        primary_limit_used_overrun_percent_points: $host_effect.primary_limit_used_overrun_percent_points,
        elapsed_label: $host_effect.elapsed_label,
        feedback_kind: $host_effect.feedback_kind,
        surface_label: $host_effect.surface_label
      },
      thread_binding_state: ($root_cause.thread_binding_state // ""),
      rotate_now: ($root_cause.guard.should_rotate_chat_now // false)
    },
    continuity_markers: {
      startup_runtime_state_source: $startup_state_source,
      gate_semantics_consistent: $ss.gate_semantics_consistent,
      startup_execution_gate: {
        action_kind: ($ss.startup_execution_gate.action_kind // ""),
        blocking: ($ss.startup_execution_gate.blocking // false),
        resume_state: ($ss.startup_execution_gate.resume_state // $ss.resume_state // ""),
        required_return_task_present: ($ss.startup_execution_gate.required_return_task_present // false),
        lease_owner_state: ($ss.startup_execution_gate.lease_owner_state // $ss.lease_owner_state // ""),
        must_follow_startup_next_action: ($ss.startup_execution_gate.must_follow_startup_next_action // false),
        unrelated_work_allowed: ($ss.startup_execution_gate.unrelated_work_allowed // false),
        must_read_prompt_text_before_reply: ($ss.startup_execution_gate.must_read_prompt_text_before_reply // false),
        required_action_kind_when_resume_required: ($ss.startup_execution_gate.required_action_kind_when_resume_required // ""),
        no_silent_drop: ($ss.startup_execution_gate.no_silent_drop // false)
      },
      execctl_resume_state: ($ss.execctl_resume_state // $ss.resume_state),
      startup_next_action: {
        action_kind: ($ss.startup_next_action.action_kind // ""),
        blocking: ($ss.startup_next_action.blocking // false),
        headline: $ss.startup_next_action.headline,
        next_step: $ss.startup_next_action.next_step,
        reason: $ss.startup_next_action.reason,
        resume_state: $ss.startup_next_action.resume_state,
        no_silent_drop: ($ss.startup_next_action.no_silent_drop // false)
      },
      required_return_task: {
        headline: $ss.required_return_task.headline,
        next_step: $ss.required_return_task.next_step,
        task_id: $ss.required_return_task.task_id,
        task_role: $ss.required_return_task.task_role,
        task_state: $ss.required_return_task.task_state,
        resume_state: $ss.required_return_task.resume_state
      },
      execctl_active_lease: {
        lease_owner_state: $ss.execctl_active_lease.lease_owner_state,
        lease_state: $ss.execctl_active_lease.lease_state,
        headline: $ss.execctl_active_lease.headline,
        next_step: $ss.execctl_active_lease.next_step,
        storage_lane: $ss.execctl_active_lease.storage_lane
      }
    },
    marker_integrity: {
      startup_gate_ready:
        ($ss.gate_semantics_consistent == true
         and $ss.startup_execution_gate.must_read_prompt_text_before_reply == true
         and $ss.startup_execution_gate.no_silent_drop == true
         and (
           (($ss.startup_execution_gate.action_kind // "") == "continue_active_workline"
            and ($ss.startup_execution_gate.blocking // false) == false
            and ($ss.startup_execution_gate.must_follow_startup_next_action // false) == false
            and ($ss.startup_execution_gate.unrelated_work_allowed // false) == true)
           or
           (($ss.startup_execution_gate.must_follow_startup_next_action // false) == true
            and ($ss.startup_execution_gate.unrelated_work_allowed // true) == false)
         )),
      exact_prefix_drift_within_tolerance:
        ($prefix_drift_abs == null or $prefix_drift_abs <= $prefix_drift_tolerance_percent),
      reply_gate_present:
        ($root_cause.client_budget_reply_gate.reply_execution_gate.action_kind != null),
      reply_gate_fresh:
        ($gate_age_ms != null and $gate_age_ms <= $gate_max_age_ms),
      current_thread_bound:
        ($root_cause.thread_binding_state == "current_thread_bound"),
      same_meter_live_turn_present:
        ($root_cause.current_live_turn.status != null)
    }
  }'
