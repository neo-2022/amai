#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="execctl_resolved_task_identity_$(date +%s%N)"
project_root="$(mktemp -d)"
restore_output="$(mktemp)"
namespace_code="continuity"
agent_scope="proof_execctl_resolved_task_identity_${project_code}"
thread_id="proof-execctl-resolved-task-identity-${project_code}"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${restore_output}"
}
trap cleanup EXIT

run_release() {
  AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="${thread_id}" \
    "${repo_root}/target/release/amai" "$@"
}

fetch_restore() {
  psql "${dsn}" -Atqc \
    "SELECT payload::text
       FROM ami.observability_snapshots
      WHERE snapshot_kind = 'working_state_restore'
        AND scope_project_code = '${project_code}'
        AND scope_namespace_code = '${namespace_code}'
   ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
      LIMIT 1" >"${restore_output}"
  test -s "${restore_output}"
}

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "ExecCtl Resolved Task Identity Probe" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Shared headline" \
  --next-step "First incarnation."
fetch_restore
task_a_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"
task_a_id="$(jq -r '.working_state_restore.project_task_tree.nodes[0].task_id' "${restore_output}")"

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Interrupt one" \
  --next-step "Suspend the first shared line."

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Shared headline" \
  --next-step "Second incarnation."
fetch_restore
task_c_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"
task_c_id="$(jq -r '.working_state_restore.project_task_tree.nodes[0].task_id' "${restore_output}")"

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Interrupt two" \
  --next-step "Suspend the second shared line."
fetch_restore

jq -e '
  .working_state_restore.current_goal == "Interrupt two"
  and (.working_state_restore.pending_return_queue | length) == 3
  and (.working_state_restore.pending_return_queue | map(select(.headline == "Shared headline")) | length) == 2
' "${restore_output}" >/dev/null

pending_latest_shared_event_id="$(jq -r '.working_state_restore.pending_return_queue[0].authoritative_event_id' "${restore_output}")"
pending_latest_shared_task_id="$(jq -r '.working_state_restore.pending_return_queue[0].task_id' "${restore_output}")"
[ "${pending_latest_shared_event_id}" = "${task_c_event_id}" ]
[ "${pending_latest_shared_task_id}" = "${task_c_id}" ]

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Post-resolve verification" \
  --next-step "Verify only one duplicate headline entry was removed." \
  --resolved-task-id "${task_c_event_id}"
fetch_restore

jq -e \
  --arg task_a_id "${task_a_id}" \
  --arg task_c_id "${task_c_id}" \
  '
  .working_state_restore.current_goal == "Post-resolve verification"
  and (.working_state_restore.pending_return_queue | length) == 3
  and (.working_state_restore.pending_return_queue[0].headline == "Interrupt two")
  and (.working_state_restore.pending_return_queue | map(select(.headline == "Shared headline")) | length) == 1
  and (.working_state_restore.pending_return_queue | any(.task_id == $task_a_id))
  and (.working_state_restore.pending_return_queue | all(.task_id != $task_c_id))
  and .working_state_restore.required_return_task.task_id == .working_state_restore.pending_return_queue[0].task_id
' "${restore_output}" >/dev/null

post_resolve_event_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Resolved current goal identity path" \
  --next-step "Verify resolved current goal does not requeue itself." \
  --resolved-task-id "${post_resolve_event_id}"
fetch_restore

jq -e '
  .working_state_restore.current_goal == "Resolved current goal identity path"
  and (.working_state_restore.pending_return_queue | map(select(.headline == "Post-resolve verification")) | length) == 0
  and (.working_state_restore.pending_return_queue | length) == 3
' "${restore_output}" >/dev/null

printf 'proof_execctl_resolved_task_identity: PASS\n'
