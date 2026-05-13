#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="execctl_resolved_task_ids_$(date +%s%N)"
project_root="$(mktemp -d)"
restore_output="$(mktemp)"
namespace_code="continuity"
scope="proof_execctl_resolved_task_ids_${project_code}"
thread_id="proof-execctl-resolved-task-ids-${project_code}"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${restore_output}"
}
trap cleanup EXIT

run_release() {
  AMAI_AGENT_SCOPE="${scope}" CODEX_THREAD_ID="${thread_id}" \
    "${repo_root}/target/release/amai" "$@"
}

fetch_restore() {
  psql "${dsn}" -Atqc \
    "SELECT payload::text
       FROM ami.observability_snapshots
      WHERE snapshot_kind = 'working_state_restore'
        AND scope_project_code = '${project_code}'
        AND scope_namespace_code = '${namespace_code}'
        AND payload #>> ARRAY['working_state_restore', 'agent_scope'] = '${scope}'
   ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
      LIMIT 1" >"${restore_output}"
  test -s "${restore_output}"
}

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

./target/release/amai project register \
  --code "${project_code}" \
  --display-name "ExecCtl Resolved Task IDs Probe" \
  --repo-root "${project_root}" >/dev/null

./target/release/amai namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Shared headline" \
  --next-step "First incarnation."

fetch_restore
h1_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"
jq -e '
  .working_state_restore.current_goal == "Shared headline"
  and (.working_state_restore.pending_return_queue | length) == 0
' "${restore_output}" >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Interrupt one" \
  --next-step "Suspend the first shared headline."

fetch_restore
jq -e \
  --arg h1_task_id "task::${h1_event_id}" \
  '
  .working_state_restore.current_goal == "Interrupt one"
  and (.working_state_restore.pending_return_queue | length) == 1
  and .working_state_restore.pending_return_queue[0].task_id == $h1_task_id
  and .working_state_restore.pending_return_queue[0].headline == "Shared headline"
  and .working_state_restore.pending_return_queue[0].next_step == "First incarnation."
  ' "${restore_output}" >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Shared headline" \
  --next-step "Second incarnation."

fetch_restore
h3_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"
jq -e '
  .working_state_restore.current_goal == "Shared headline"
  and .working_state_restore.next_step == "Second incarnation."
  and (.working_state_restore.pending_return_queue | length) == 2
  and .working_state_restore.pending_return_queue[0].headline == "Interrupt one"
  and .working_state_restore.pending_return_queue[1].headline == "Shared headline"
' "${restore_output}" >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Interrupt two" \
  --next-step "Create duplicate pending-return headline."

fetch_restore
h1_pending_task_id="$(jq -r '
  .working_state_restore.pending_return_queue[]
  | select(.headline == "Shared headline" and .next_step == "First incarnation.")
  | .task_id
' "${restore_output}")"
h3_pending_task_id="$(jq -r '
  .working_state_restore.pending_return_queue[]
  | select(.headline == "Shared headline" and .next_step == "Second incarnation.")
  | .task_id
' "${restore_output}")"
jq -e \
  --arg h1_task_id "${h1_pending_task_id}" \
  --arg h3_task_id "${h3_pending_task_id}" \
  '
  .working_state_restore.current_goal == "Interrupt two"
  and (.working_state_restore.pending_return_queue | length) == 3
  and .working_state_restore.pending_return_queue[0].task_id == $h3_task_id
  and .working_state_restore.pending_return_queue[0].headline == "Shared headline"
  and .working_state_restore.pending_return_queue[1].headline == "Interrupt one"
  and .working_state_restore.pending_return_queue[2].task_id == $h1_task_id
  ' "${restore_output}" >/dev/null
[ "${h1_pending_task_id}" != "${h3_pending_task_id}" ]

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Resolved duplicate by task id" \
  --next-step "Keep only the older shared headline pending." \
  --resolved-task-id "${h3_event_id}"

fetch_restore
h5_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"
jq -e \
  --arg h1_task_id "${h1_pending_task_id}" \
  --arg h3_task_id "${h3_pending_task_id}" \
  '
  .working_state_restore.current_goal == "Resolved duplicate by task id"
  and (.working_state_restore.pending_return_queue | length) == 3
  and ([.working_state_restore.pending_return_queue[] | select(.headline == "Shared headline")] | length) == 1
  and ([.working_state_restore.pending_return_queue[] | select(.headline == "Shared headline")][0].task_id) == $h1_task_id
  and ([.working_state_restore.pending_return_queue[] | select(.task_id == $h3_task_id)] | length) == 0
  and .working_state_restore.pending_return_queue[0].headline == "Interrupt two"
  ' "${restore_output}" >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Switch after resolving current goal" \
  --next-step "Resolved current goal must not be requeued." \
  --resolved-task-id "${h5_event_id}"

fetch_restore
jq -e \
  --arg h1_task_id "${h1_pending_task_id}" \
  --arg h5_task_id "task::${h5_event_id}" \
  '
  .working_state_restore.current_goal == "Switch after resolving current goal"
  and (.working_state_restore.pending_return_queue | length) == 3
  and ([.working_state_restore.pending_return_queue[] | select(.headline == "Resolved duplicate by task id")] | length) == 0
  and ([.working_state_restore.pending_return_queue[] | select(.task_id == $h5_task_id)] | length) == 0
  and ([.working_state_restore.pending_return_queue[] | select(.headline == "Shared headline")] | length) == 1
  and ([.working_state_restore.pending_return_queue[] | select(.headline == "Shared headline")][0].task_id) == $h1_task_id
  ' "${restore_output}" >/dev/null

printf 'proof_execctl_resolved_task_ids: PASS\n'
