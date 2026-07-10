#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd)"

set +e
output="$(./scripts/amai_exec.sh continuity with-active-workline-lock \
  --repo-root "${repo_root}" \
  --namespace continuity \
  -- false 2>&1)"
status=$?
set -e

if [[ "${status}" -eq 0 ]]; then
  echo "proof_active_workline_locked_command_failure: expected non-zero exit" >&2
  exit 1
fi

printf '%s\n' "${output}" | grep -F "locked command exited with status exit status: 1" >/dev/null
if printf '%s\n' "${output}" | grep -F "secondary advisory unlock error" >/dev/null; then
  echo "proof_active_workline_locked_command_failure: secondary unlock error leaked into output" >&2
  exit 1
fi

state_output="$(./scripts/continuity_startup_state.sh --repo-root "${repo_root}" --json)"
printf '%s\n' "${state_output}" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "${state_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.source_event_match == true' >/dev/null
printf '%s\n' "${state_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.headline_match == true' >/dev/null
printf '%s\n' "${state_output}" | jq -e '.startup_runtime_state.execctl_active_lease.headline == "Fix stale startup line-steal defect before any other work"' >/dev/null
printf '%s\n' "${state_output}" | jq -e '.startup_runtime_state.execctl_active_lease.source_event_id == "1811232b-5fd2-4d8f-a7c6-e0f6c75f5edc"' >/dev/null

echo "proof_active_workline_locked_command_failure: PASS"
