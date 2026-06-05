#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="continuity_threadless_guard_$(date +%s%N)"
project_root="$(mktemp -d)"
namespace_code="continuity"
agent_scope="proof_continuity_threadless_guard_${project_code}"
foreign_stdout="$(mktemp)"
foreign_stderr="$(mktemp)"
startup_json="$(mktemp)"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}"
  rm -f "${foreign_stdout}" "${foreign_stderr}" "${startup_json}"
}
trap cleanup EXIT

run_release() {
  "${repo_root}/target/release/amai" "$@"
}

cd "${repo_root}"

cargo run --release --quiet -- bootstrap schema >/dev/null
cargo build --release --quiet >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "Continuity Threadless Guard Probe" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

env -u CODEX_THREAD_ID AMAI_AGENT_SCOPE="${agent_scope}" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Threadless shared line" \
    --next-step "Create a shared lease without a durable thread binding." >/dev/null

if AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-b" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Foreign overwrite attempt" \
    --next-step "Attempt overwrite after threadless shared line." \
    >"${foreign_stdout}" 2>"${foreign_stderr}"; then
  echo "foreign thread unexpectedly claimed threadless shared lease" >&2
  exit 1
fi

grep -q "continuity handoff blocked" "${foreign_stderr}"
grep -q "no durable or restorable thread binding" "${foreign_stderr}"

handoff_count="$(psql "${dsn}" -Atqc \
  "SELECT COUNT(*)
     FROM ami.observability_snapshots
    WHERE snapshot_kind = 'working_state_event'
      AND scope_project_code = '${project_code}'
      AND scope_namespace_code = '${namespace_code}'
      AND payload->'working_state_event'->>'event_kind' = 'continuity_handoff'")"
[ "${handoff_count}" = "1" ]

lease_row="$(psql "${dsn}" -Atqc \
  "SELECT COALESCE(owner_thread_id, '<null>') || '|' || headline || '|' || source_event_id
     FROM ami.execctl_task_leases l
     JOIN ami.projects p ON p.project_id = l.project_id
     JOIN ami.namespaces n ON n.namespace_id = l.namespace_id
    WHERE p.code = '${project_code}'
      AND n.code = '${namespace_code}'
      AND l.agent_scope = '${agent_scope}'
      AND l.lease_state = 'active'
 ORDER BY l.acquired_at_epoch_ms DESC
    LIMIT 1")"
IFS='|' read -r lease_owner lease_headline lease_source_event_id <<<"${lease_row}"
[ "${lease_owner}" = "<null>" ]
[ "${lease_headline}" = "Threadless shared line" ]
test -n "${lease_source_event_id}"

AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-b" \
  "${repo_root}/target/release/amai" continuity startup \
    --repo-root "${project_root}" \
    --namespace "${namespace_code}" \
    --json >"${startup_json}"

jq -e '.continuity_startup_summary.execctl_active_lease.owner_thread_id == null' "${startup_json}" >/dev/null

lease_row_after_startup="$(psql "${dsn}" -Atqc \
  "SELECT COALESCE(owner_thread_id, '<null>') || '|' || headline || '|' || source_event_id
     FROM ami.execctl_task_leases l
     JOIN ami.projects p ON p.project_id = l.project_id
     JOIN ami.namespaces n ON n.namespace_id = l.namespace_id
    WHERE p.code = '${project_code}'
      AND n.code = '${namespace_code}'
      AND l.agent_scope = '${agent_scope}'
      AND l.lease_state = 'active'
 ORDER BY l.acquired_at_epoch_ms DESC
    LIMIT 1")"
[ "${lease_row_after_startup}" = "${lease_row}" ]

printf 'proof_continuity_threadless_handoff_fail_closed: ok\n'
