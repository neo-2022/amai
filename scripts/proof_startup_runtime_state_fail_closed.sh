#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mkdir -p state/locks
exec 9>state/locks/startup_runtime_state_mutation.lock
flock --exclusive 9

./scripts/continuity_startup.sh --repo-root "/home/art/agent-memory-index" --namespace continuity --json >/dev/null

state_file=".amai/continuity/project-chat-startup-state.json"
test -f "${state_file}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

validate_runtime_state() {
  local file="$1"
  ./scripts/continuity_startup_state.sh --repo-root "/home/art/agent-memory-index" --json \
    | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
  python3 - "$file" <<'PY'
import json
import pathlib
import shutil
import subprocess
import sys

source = pathlib.Path(sys.argv[1])
repo = pathlib.Path("/home/art/agent-memory-index")
target = repo / ".amai/continuity/project-chat-startup-state.json"
backup = repo / ".amai/continuity/project-chat-startup-state.json.proof-backup"

shutil.copy2(target, backup)
try:
    shutil.copy2(source, target)
    result = subprocess.run(
        ["./scripts/continuity_startup_state.sh", "--repo-root", str(repo), "--json"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    print(json.dumps(payload))
finally:
    shutil.move(str(backup), str(target))
PY
}

jq '.workflow_promotion_state.source_event_match = false' \
  "${state_file}" >"${tmp_dir}/source-event-mismatch.json"
mismatch_output="$(validate_runtime_state "${tmp_dir}/source-event-mismatch.json")"
printf '%s\n' "${mismatch_output}" | jq -e '.startup_runtime_state.status == "startup_runtime_state_drift"' >/dev/null
printf '%s\n' "${mismatch_output}" | jq -e '.startup_runtime_state.gate_semantics_consistent == false' >/dev/null
printf '%s\n' "${mismatch_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.source_event_match == false' >/dev/null
printf '%s\n' "${mismatch_output}" | jq -e '.startup_runtime_state.artifact_gate_semantics_consistent_matches_recomputed == false' >/dev/null

jq 'del(.working_state_restore_lineage.authoritative_event_id)' \
  "${state_file}" >"${tmp_dir}/missing-lineage-event-id.json"
missing_lineage_output="$(validate_runtime_state "${tmp_dir}/missing-lineage-event-id.json")"
printf '%s\n' "${missing_lineage_output}" | jq -e '.startup_runtime_state.status == "startup_runtime_state_drift"' >/dev/null
printf '%s\n' "${missing_lineage_output}" | jq -e '.startup_runtime_state.working_state_restore_lineage_event_present == false' >/dev/null
printf '%s\n' "${missing_lineage_output}" | jq -e '.startup_runtime_state_audit.working_state_restore_lineage_event_present == false' >/dev/null

jq 'del(.continuity_startup_summary.execctl_active_lease.source_event_id) | del(.execctl_active_lease.source_event_id)' \
  "${state_file}" >"${tmp_dir}/missing-lease-source-event-id.json"
missing_lease_output="$(validate_runtime_state "${tmp_dir}/missing-lease-source-event-id.json")"
printf '%s\n' "${missing_lease_output}" | jq -e '.startup_runtime_state.status == "startup_runtime_state_drift"' >/dev/null
printf '%s\n' "${missing_lease_output}" | jq -e '.startup_runtime_state.execctl_active_lease_source_event_id_present == false' >/dev/null
printf '%s\n' "${missing_lease_output}" | jq -e '.startup_runtime_state_audit.execctl_active_lease_source_event_id_present == false' >/dev/null

./scripts/continuity_startup.sh --repo-root "/home/art/agent-memory-index" --namespace continuity --json >/dev/null
restored_output="$(./scripts/continuity_startup_state.sh --repo-root "/home/art/agent-memory-index" --json)"
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.gate_semantics_consistent == true' >/dev/null

echo "proof_startup_runtime_state_fail_closed: ok"
