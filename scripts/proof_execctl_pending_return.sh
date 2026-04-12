#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="execctl_pending_return_probe_$$"
project_root="$(mktemp -d)"
restore_output="$(mktemp)"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${restore_output}"
}
trap cleanup EXIT

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

./target/release/amai project register \
  --code "${project_code}" \
  --display-name "ExecCtl Pending Return Probe" \
  --repo-root "${project_root}" >/dev/null

./target/release/amai namespace ensure \
  --project "${project_code}" \
  --code continuity \
  --display-name Continuity >/dev/null

./target/release/amai continuity handoff \
  --project "${project_code}" \
  --headline "Same-meter spend control" \
  --next-step "Materialize live assistant generation source." >/dev/null

./target/release/amai continuity handoff \
  --project "${project_code}" \
  --headline "Project relocation contour" \
  --next-step "Dovetail runtime auto-start guarantees." >/dev/null

psql "${dsn}" -Atqc \
  "SELECT payload::text
     FROM ami.observability_snapshots
    WHERE snapshot_kind = 'working_state_restore'
      AND scope_project_code = '${project_code}'
 ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
    LIMIT 1" >"${restore_output}"

jq -e '.working_state_restore.current_goal == "Project relocation contour"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.execctl_resume_state == "pending_return_queue_present"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.execctl_resume_contract.resume_state == "return_required"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.pending_return_queue[0].headline == "Same-meter spend control"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.pending_return_summary | contains("Same-meter spend control")' "${restore_output}" >/dev/null
startup_action_kind="$(jq -r '.working_state_restore.startup_next_action.action_kind' "${restore_output}")"
case "${startup_action_kind}" in
  rotate_chat_for_client_budget)
    jq -e '.working_state_restore.startup_next_action.preserves_return_obligation == true' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.startup_next_action.resume_state == "return_required"' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.startup_next_action_summary | contains("rotate_chat_for_client_budget")' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.client_budget_guard.reply_execution_gate.action_kind == "rotate_chat_for_client_budget"' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.client_budget_guard.reply_execution_gate.must_rotate_before_reply == false' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.client_budget_guard.reply_execution_gate.blocking == false' "${restore_output}" >/dev/null
    ;;
  resume_required_return_task)
    jq -e '.working_state_restore.startup_next_action.headline == "Same-meter spend control"' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.startup_next_action_summary | contains("resume_required_return_task")' "${restore_output}" >/dev/null
    jq -e '.working_state_restore.startup_next_action_summary | contains("Same-meter spend control")' "${restore_output}" >/dev/null
    ;;
  *)
    echo "proof_execctl_pending_return: unexpected startup_next_action.action_kind=${startup_action_kind}" >&2
    exit 1
    ;;
esac
jq -e '.working_state_restore.execctl_active_lease.headline == "Project relocation contour"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.execctl_active_lease.storage_lane == "ami.execctl_task_leases"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.execctl_active_lease_summary | contains("Project relocation contour")' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_tree.tree_version == "project-task-tree-v1"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_tree.open_tasks_count == 2' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_tree.nodes[0].task_role == "active"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_tree.nodes[1].task_role == "pending_return"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_tree_summary | contains("pending_return(1)")' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger.ledger_version == "project-task-ledger-v2"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger.open_tasks_count == 2' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger.persistence_state == "durable_postgres"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger.storage_lane == "ami.execctl_task_ledger_entries"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger.entries[0].task_role == "active"' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger.entries[0].ledger_entry_id | length > 0' "${restore_output}" >/dev/null
jq -e '.working_state_restore.project_task_ledger_summary | contains("historical_handoffs(0)")' "${restore_output}" >/dev/null

lease_count="$(psql "${dsn}" -Atqc \
  "SELECT COUNT(*)
     FROM ami.execctl_task_leases e
     JOIN ami.projects p ON p.project_id = e.project_id
    WHERE p.code = '${project_code}'")"
[ "${lease_count}" = "1" ]

lease_headline="$(psql "${dsn}" -Atqc \
  "SELECT headline
     FROM ami.execctl_task_leases e
     JOIN ami.projects p ON p.project_id = e.project_id
    WHERE p.code = '${project_code}'")"
[ "${lease_headline}" = "Project relocation contour" ]

ledger_count="$(psql "${dsn}" -Atqc \
  "SELECT COUNT(*)
     FROM ami.execctl_task_ledger_entries e
     JOIN ami.projects p ON p.project_id = e.project_id
    WHERE p.code = '${project_code}'")"
[ "${ledger_count}" = "2" ]

printf 'proof_execctl_pending_return: PASS\n'
