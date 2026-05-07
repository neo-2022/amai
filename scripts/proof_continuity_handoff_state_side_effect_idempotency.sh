#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="handoff_state_side_effect_idempotency_$(date +%s%N)"
project_root="$(mktemp -d)"
namespace_code="continuity"
agent_scope="proof_handoff_state_side_effect_idempotency_${project_code}"
thread_id="proof-handoff-state-side-effect-idempotency-${project_code}"
first_restore="$(mktemp)"
second_restore="$(mktemp)"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${first_restore}" "${second_restore}"
}
trap cleanup EXIT

run_release() {
  AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="${thread_id}" \
    "${repo_root}/target/release/amai" "$@"
}

fetch_restore() {
  local output_path="$1"
  psql "${dsn}" -Atqc \
    "SELECT payload::text
       FROM ami.observability_snapshots
      WHERE snapshot_kind = 'working_state_restore'
        AND scope_project_code = '${project_code}'
        AND scope_namespace_code = '${namespace_code}'
   ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
      LIMIT 1" >"${output_path}"
  test -s "${output_path}"
}

query_single_value() {
  local sql="$1"
  psql "${dsn}" -Atqc "${sql}"
}

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "Continuity Handoff State Side-Effect Idempotency Probe" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Same line" \
  --next-step "Replay same line twice." >/dev/null

fetch_restore "${first_restore}"
first_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${first_restore}")"
first_lease_heartbeat="$(query_single_value "
  SELECT heartbeat_at_epoch_ms
    FROM ami.execctl_task_leases l
    JOIN ami.projects p ON p.project_id = l.project_id
    JOIN ami.namespaces n ON n.namespace_id = l.namespace_id
   WHERE p.code = '${project_code}'
     AND n.code = '${namespace_code}'
     AND l.agent_scope = '${agent_scope}'
   ORDER BY l.updated_at DESC, l.lease_id DESC
   LIMIT 1
")"

sleep 0.05

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Same line" \
  --next-step "Replay same line twice." >/dev/null

fetch_restore "${second_restore}"
second_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${second_restore}")"
second_lease_heartbeat="$(query_single_value "
  SELECT heartbeat_at_epoch_ms
    FROM ami.execctl_task_leases l
    JOIN ami.projects p ON p.project_id = l.project_id
    JOIN ami.namespaces n ON n.namespace_id = l.namespace_id
   WHERE p.code = '${project_code}'
     AND n.code = '${namespace_code}'
     AND l.agent_scope = '${agent_scope}'
   ORDER BY l.updated_at DESC, l.lease_id DESC
   LIMIT 1
")"

test "${first_event_id}" = "${second_event_id}"
test "${second_lease_heartbeat}" -ge "${first_lease_heartbeat}"

jq -e '
  .working_state_restore.current_goal == "Same line"
  and .working_state_restore.next_step == "Replay same line twice."
  and (.working_state_restore.pending_return_queue | length) == 0
  and (.working_state_restore.recent_actions | length) == 1
  and .working_state_restore.recent_actions[0].headline == "Same line"
  and .working_state_restore.recent_actions[0].event_kind == "continuity_handoff"
  and (.working_state_restore.project_task_tree.nodes | length) == 1
  and .working_state_restore.project_task_tree.pending_return_count == 0
  and (.working_state_restore.project_task_ledger.entries | length) == 1
  and .working_state_restore.project_task_ledger.historical_handoffs_count == 0
  and (.working_state_restore.state_lineage.nodes | length) == 1
  and (.working_state_restore.state_lineage.edges | length) == 0
' "${second_restore}" >/dev/null

continuity_handoff_snapshot_count="$(query_single_value "
  SELECT count(*)
    FROM ami.observability_snapshots
   WHERE snapshot_kind = 'continuity_handoff'
     AND scope_project_code = '${project_code}'
     AND scope_namespace_code = '${namespace_code}'
")"
test "${continuity_handoff_snapshot_count}" = "1"

working_state_handoff_count="$(query_single_value "
  SELECT count(*)
    FROM ami.observability_snapshots
   WHERE snapshot_kind = 'working_state_event'
     AND scope_project_code = '${project_code}'
     AND scope_namespace_code = '${namespace_code}'
     AND payload #>> ARRAY['working_state_event', 'event_kind'] = 'continuity_handoff'
")"
test "${working_state_handoff_count}" = "1"

ledger_entry_count="$(query_single_value "
  SELECT count(*)
    FROM ami.execctl_task_ledger_entries e
    JOIN ami.projects p ON p.project_id = e.project_id
    JOIN ami.namespaces n ON n.namespace_id = e.namespace_id
   WHERE p.code = '${project_code}'
     AND n.code = '${namespace_code}'
     AND e.agent_scope = '${agent_scope}'
")"
test "${ledger_entry_count}" = "1"

lease_source_event_id="$(query_single_value "
  SELECT source_event_id
    FROM ami.execctl_task_leases l
    JOIN ami.projects p ON p.project_id = l.project_id
    JOIN ami.namespaces n ON n.namespace_id = l.namespace_id
   WHERE p.code = '${project_code}'
     AND n.code = '${namespace_code}'
     AND l.agent_scope = '${agent_scope}'
   ORDER BY l.updated_at DESC, l.lease_id DESC
   LIMIT 1
")"
test "${lease_source_event_id}" = "${first_event_id}"

printf 'proof_continuity_handoff_state_side_effect_idempotency: PASS\n'
