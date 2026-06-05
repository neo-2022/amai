#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="same_thread_same_scope_competing_handoff_$(date +%s%N)"
project_root="$(mktemp -d)"
namespace_code="continuity"
agent_scope="${project_code}::${namespace_code}::default"
critic_agent_scope="${project_code}::${namespace_code}::critic"
first_restore="$(mktemp)"
foreign_stdout="$(mktemp)"
foreign_stderr="$(mktemp)"
promotion_stdout="$(mktemp)"
promotion_stderr="$(mktemp)"
promotion_details="$(mktemp)"
startup_json="$(mktemp)"

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}"
  rm -f "${first_restore}" "${foreign_stdout}" "${foreign_stderr}" "${promotion_stdout}" "${promotion_stderr}" "${promotion_details}" "${startup_json}"
}
trap cleanup EXIT

run_release() {
  AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
    "${repo_root}/target/release/amai" "$@"
}

cd "${repo_root}"

cargo run --release --quiet -- bootstrap schema >/dev/null
cargo build --release --quiet >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "Same Thread Same Scope Competing Handoff Probe" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

run_release continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Active shared line" \
  --next-step "Keep the active shared line stable." \
  --promote-active-workline >/dev/null

psql "${dsn}" -Atqc \
  "SELECT payload::text
     FROM ami.observability_snapshots
    WHERE snapshot_kind = 'working_state_restore'
      AND scope_project_code = '${project_code}'
      AND scope_namespace_code = '${namespace_code}'
 ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
    LIMIT 1" >"${first_restore}"

jq -e '.working_state_restore.current_goal == "Active shared line"' "${first_restore}" >/dev/null
jq -e '.working_state_restore.execctl_resume_state == "clear"' "${first_restore}" >/dev/null

if AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Competing side line" \
    --next-step "Keep the active shared line stable." \
    >"${foreign_stdout}" 2>"${foreign_stderr}"; then
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: competing handoff unexpectedly succeeded" >&2
  exit 1
fi

grep -q "same-thread same-scope write would replace active line" "${foreign_stderr}"
grep -Fq -- "--promote-active-workline" "${foreign_stderr}"

if AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Promoted side line" \
    --next-step "Advance the active line with boolean-only promotion." \
    --promote-active-workline \
    >"${promotion_stdout}" 2>"${promotion_stderr}"; then
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: boolean-only explicit promotion unexpectedly succeeded" >&2
  exit 1
fi

grep -q "missing a valid promotion_contract" "${promotion_stderr}"
grep -q "distinct AMAI_AGENT_SCOPE" "${promotion_stderr}"

printf '%s\n%s\n' \
  "promotion_contract: typo" \
  "Unsupported promotion contracts must stay blocked." \
  >"${promotion_details}"
if AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Invalid contract side line" \
    --next-step "Advance the active line with an invalid promotion contract." \
    --details-file "${promotion_details}" \
    --promote-active-workline \
    >"${promotion_stdout}" 2>"${promotion_stderr}"; then
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: invalid promotion_contract unexpectedly succeeded" >&2
  exit 1
fi

grep -q "missing a valid promotion_contract" "${promotion_stderr}"
grep -q "operator_redirect" "${promotion_stderr}"

printf '%s\n%s\n' \
  "promotion_contract: operator_redirect" \
  "Synthetic operator redirects must carry trusted provenance." \
  >"${promotion_details}"
if env -u AMAI_OPERATOR_REDIRECT_PROVENANCE \
  AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Operator redirect without provenance" \
    --next-step "Attempt a synthetic mainline redirect without provenance." \
    --details-file "${promotion_details}" \
    --promote-active-workline \
    >"${promotion_stdout}" 2>"${promotion_stderr}"; then
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: operator_redirect without provenance unexpectedly succeeded" >&2
  exit 1
fi

grep -q "missing a valid promotion_contract" "${promotion_stderr}"
grep -q "AMAI_OPERATOR_REDIRECT_PROVENANCE" "${promotion_stderr}"

printf '%s\n%s\n' \
  "promotion_contract: user_redirect" \
  "User redirects must carry trusted provenance." \
  >"${promotion_details}"
if env -u AMAI_USER_REDIRECT_PROVENANCE \
  AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "User redirect without provenance" \
    --next-step "Attempt a user redirect without provenance." \
    --details-file "${promotion_details}" \
    --promote-active-workline \
    >"${promotion_stdout}" 2>"${promotion_stderr}"; then
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: user_redirect without provenance unexpectedly succeeded" >&2
  exit 1
fi

grep -q "missing a valid promotion_contract" "${promotion_stderr}"
grep -q "AMAI_USER_REDIRECT_PROVENANCE" "${promotion_stderr}"

psql "${dsn}" -Atqc \
  "SELECT payload::text
     FROM ami.observability_snapshots
    WHERE snapshot_kind = 'working_state_restore'
      AND scope_project_code = '${project_code}'
      AND scope_namespace_code = '${namespace_code}'
 ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
    LIMIT 1" >"${startup_json}"

jq -e '
  .working_state_restore.current_goal == "Active shared line"
  and .working_state_restore.execctl_resume_state == "clear"
  and .working_state_restore.execctl_active_lease.headline == "Active shared line"
  and .working_state_restore.execctl_active_lease.owner_thread_id == "thread-a"
  and (.working_state_restore.pending_return_queue | length) == 0
' "${startup_json}" >/dev/null

if AMAI_AGENT_SCOPE="${critic_agent_scope}" CODEX_THREAD_ID="thread-critic" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Critic private line" \
    --next-step "Review without taking over the primary startup line." \
    >"${promotion_stdout}" 2>"${promotion_stderr}"; then
  :
else
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: distinct critic scope handoff should succeed" >&2
  cat "${promotion_stderr}" >&2
  exit 1
fi

run_release continuity startup \
  --repo-root "${project_root}" \
  --namespace "${namespace_code}" \
  --json >"${startup_json}"

if ! jq -e --arg agent_scope "${agent_scope}" '
  .working_state_restore.current_goal == "Active shared line"
  and .working_state_restore.agent_scope == $agent_scope
' "${startup_json}" >/dev/null; then
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: critic scope handoff contaminated primary startup selection" >&2
  jq '{continuity_startup_summary, working_state_restore}' "${startup_json}" >&2
  exit 1
fi

printf '%s\n%s\n' \
  "promotion_contract: user_redirect" \
  "The operator explicitly promotes this competing line as the new main workline." \
  >"${promotion_details}"

if AMAI_AGENT_SCOPE="${agent_scope}" CODEX_THREAD_ID="thread-a" \
  AMAI_USER_REDIRECT_PROVENANCE="proof_harness:$(basename "$0")" \
  "${repo_root}/target/release/amai" continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "Promoted side line" \
    --next-step "Advance the active line with explicit promotion." \
    --details-file "${promotion_details}" \
    --promote-active-workline >/dev/null; then
  :
else
  echo "proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: explicit promotion should succeed" >&2
  exit 1
fi

psql "${dsn}" -Atqc \
  "SELECT payload::text
     FROM ami.observability_snapshots
    WHERE snapshot_kind = 'working_state_restore'
      AND scope_project_code = '${project_code}'
      AND scope_namespace_code = '${namespace_code}'
 ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
    LIMIT 1" >"${startup_json}"

jq -e '
  .working_state_restore.current_goal == "Promoted side line"
  and .working_state_restore.execctl_active_lease.headline == "Promoted side line"
  and .working_state_restore.execctl_active_lease.owner_thread_id == "thread-a"
  and (.working_state_restore.pending_return_queue | length) == 1
  and .working_state_restore.pending_return_queue[0].headline == "Active shared line"
' "${startup_json}" >/dev/null

printf 'proof_continuity_same_thread_same_scope_competing_handoff_fail_closed: PASS\n'
