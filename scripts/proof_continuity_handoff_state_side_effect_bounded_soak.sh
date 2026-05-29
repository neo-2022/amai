#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="handoff_state_side_effect_bounded_soak_$(date +%s%N)"
project_root="$(mktemp -d)"
namespace_code="continuity"
agent_scope="proof_handoff_state_side_effect_bounded_soak_${project_code}"
thread_id="proof-handoff-state-side-effect-bounded-soak-${project_code}"
restore_path="$(mktemp)"
latency_path="$(mktemp)"
iterations=12
max_single_ms=5000
max_total_ms=25000

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${restore_path}" "${latency_path}"
}
trap cleanup EXIT

run_release() {
  AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="${thread_id}" \
    "${repo_root}/target/release/amai" "$@"
}

query_single_value() {
  local sql="$1"
  psql "${dsn}" -Atqc "${sql}"
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

count_snapshot_kind() {
  local kind="$1"
  query_single_value "
    SELECT count(*)
      FROM ami.observability_snapshots
     WHERE snapshot_kind = '${kind}'
       AND scope_project_code = '${project_code}'
       AND scope_namespace_code = '${namespace_code}'
  "
}

cd "${repo_root}"

PGOPTIONS='-c client_min_messages=warning' psql "${dsn}" -v ON_ERROR_STOP=1 \
  -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "Continuity Handoff State Side-Effect Bounded Soak" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

total_ms=0
max_seen_ms=0
authoritative_event_id=""
previous_lease_heartbeat=0
document_index_refresh_count=""

for i in $(seq 1 "${iterations}"); do
  started_ms="$(date +%s%3N)"
  run_release continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Same line" \
    --next-step "Replay same line twice." >/dev/null
  ended_ms="$(date +%s%3N)"
  elapsed_ms="$((ended_ms - started_ms))"
  total_ms="$((total_ms + elapsed_ms))"
  if (( elapsed_ms > max_seen_ms )); then
    max_seen_ms="${elapsed_ms}"
  fi
  printf '%s\t%s\n' "${i}" "${elapsed_ms}" >>"${latency_path}"

  fetch_restore "${restore_path}"
  current_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_path}")"
  current_lease_heartbeat="$(query_single_value "
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

  if [[ -z "${authoritative_event_id}" ]]; then
    authoritative_event_id="${current_event_id}"
  fi
  test "${current_event_id}" = "${authoritative_event_id}"
  test "${current_lease_heartbeat}" -ge "${previous_lease_heartbeat}"
  previous_lease_heartbeat="${current_lease_heartbeat}"

  test "$(count_snapshot_kind continuity_handoff)" = "1"
  current_document_index_refresh_count="$(count_snapshot_kind continuity_handoff_document_index_refresh)"
  if [[ -z "${document_index_refresh_count}" ]]; then
    document_index_refresh_count="${current_document_index_refresh_count}"
  fi
  test "${current_document_index_refresh_count}" = "${document_index_refresh_count}"
  test "$(count_snapshot_kind working_state_event)" = "1"
  test "$(count_snapshot_kind working_state_restore)" = "1"

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

  jq -e '
    .working_state_restore.current_goal == "Same line"
    and .working_state_restore.next_step == "Replay same line twice."
    and (.working_state_restore.pending_return_queue | length) == 0
    and (.working_state_restore.recent_actions | length) == 1
    and .working_state_restore.recent_actions[0].event_kind == "continuity_handoff"
    and .working_state_restore.recent_actions[0].headline == "Same line"
    and (.working_state_restore.project_task_tree.nodes | length) == 1
    and .working_state_restore.project_task_tree.pending_return_count == 0
    and (.working_state_restore.project_task_ledger.entries | length) == 1
    and .working_state_restore.project_task_ledger.historical_handoffs_count == 0
    and (.working_state_restore.state_lineage.nodes | length) == 1
    and (.working_state_restore.state_lineage.edges | length) == 0
  ' "${restore_path}" >/dev/null

  if (( elapsed_ms > max_single_ms )); then
    echo "proof_continuity_handoff_state_side_effect_bounded_soak: iteration ${i} exceeded max_single_ms (${elapsed_ms} > ${max_single_ms})" >&2
    cat "${latency_path}" >&2
    exit 1
  fi
done

if (( total_ms > max_total_ms )); then
  echo "proof_continuity_handoff_state_side_effect_bounded_soak: total elapsed exceeded max_total_ms (${total_ms} > ${max_total_ms})" >&2
  cat "${latency_path}" >&2
  exit 1
fi

echo "proof_continuity_handoff_state_side_effect_bounded_soak: PASS (iterations=${iterations}, max_ms=${max_seen_ms}, total_ms=${total_ms})"
