#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="continuity_alias_conflict_guard_$(date +%s%N)"
project_root="$(mktemp -d)"
namespace_code="continuity"
thread_scope="proof_continuity_alias_conflict_thread_${project_code}"
handoff_dir="${repo_root}/state/continuity-imports/${project_code}"
handoff_path="${handoff_dir}/live-handoff.md"
thread_stdout="$(mktemp)"
thread_stderr="$(mktemp)"
scope_stdout="$(mktemp)"
scope_stderr="$(mktemp)"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${handoff_dir}"
  rm -f "${thread_stdout}" "${thread_stderr}" "${scope_stdout}" "${scope_stderr}"
}
trap cleanup EXIT

run_release() {
  "${repo_root}/target/release/amai" "$@"
}

count_snapshot_kind() {
  local kind="$1"
  psql "${dsn}" -Atqc \
    "SELECT COUNT(*)
       FROM ami.observability_snapshots
      WHERE snapshot_kind = '${kind}'
        AND scope_project_code = '${project_code}'
        AND scope_namespace_code = '${namespace_code}'"
}

count_working_state_handoffs() {
  psql "${dsn}" -Atqc \
    "SELECT COUNT(*)
       FROM ami.observability_snapshots
      WHERE snapshot_kind = 'working_state_event'
        AND scope_project_code = '${project_code}'
        AND scope_namespace_code = '${namespace_code}'
        AND payload->'working_state_event'->>'event_kind' = 'continuity_handoff'"
}

assert_no_side_effects() {
  [ ! -e "${handoff_path}" ]
  [ "$(count_snapshot_kind continuity_handoff)" = "0" ]
  [ "$(count_snapshot_kind continuity_handoff_document_index_refresh)" = "0" ]
  [ "$(count_working_state_handoffs)" = "0" ]
}

cd "${repo_root}"

cargo run --release --quiet -- bootstrap schema >/dev/null
cargo build --release --quiet >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "Continuity Alias Conflict Guard Probe" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

if AMAI_PLATFORM_THREAD_ID="thread-a" \
  CODEX_THREAD_ID="thread-b" \
  AMAI_AGENT_SCOPE="${thread_scope}" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Thread alias conflict" \
    --next-step "This handoff must fail before side effects." \
    >"${thread_stdout}" 2>"${thread_stderr}"; then
  echo "thread alias conflict unexpectedly materialized a continuity handoff" >&2
  exit 1
fi

grep -q "continuity handoff blocked before side effects" "${thread_stderr}"
grep -q "conflicting thread identity aliases" "${thread_stderr}"
assert_no_side_effects

if AMAI_PLATFORM_THREAD_ID="thread-a" \
  CODEX_THREAD_ID="thread-a" \
  AMAI_AGENT_SCOPE="scope-a" \
  CODEX_AGENT_SCOPE="scope-b" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Scope alias conflict" \
    --next-step "This handoff must fail before side effects." \
    >"${scope_stdout}" 2>"${scope_stderr}"; then
  echo "agent scope alias conflict unexpectedly materialized a continuity handoff" >&2
  exit 1
fi

grep -q "conflicting agent scope aliases" "${scope_stderr}"
assert_no_side_effects

printf 'proof_continuity_handoff_alias_conflict_fail_closed: ok\n'
