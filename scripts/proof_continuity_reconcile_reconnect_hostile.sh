#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

repo_root="$(pwd)"
state_file=".amai/continuity/project-chat-startup-state.json"
host_home="${HOME}"
host_rustup_home="${RUSTUP_HOME:-${host_home}/.rustup}"
host_cargo_home="${CARGO_HOME:-${host_home}/.cargo}"

mkdir -p state/locks
exec 9>state/locks/startup_runtime_state_mutation.lock
flock --exclusive 9

./scripts/continuity_startup.sh --repo-root "${repo_root}" --namespace continuity --json >/dev/null
test -f "${state_file}"

tmp_dir="$(mktemp -d)"
backup_state="${tmp_dir}/project-chat-startup-state.backup.json"
cp "${state_file}" "${backup_state}"
trap 'cp "${backup_state}" "${state_file}"; rm -rf "${tmp_dir}"' EXIT

jq 'del(.working_state_restore_lineage.authoritative_event_id) | del(.continuity_startup_summary.execctl_active_lease.source_event_id) | del(.execctl_active_lease.source_event_id) | .gate_semantics_consistent = false' \
  "${backup_state}" >"${state_file}"

drift_output="$(./scripts/continuity_startup_state.sh --repo-root "${repo_root}" --json)"
printf '%s\n' "${drift_output}" | jq -e '.startup_runtime_state.status == "startup_runtime_state_drift"' >/dev/null
printf '%s\n' "${drift_output}" | jq -e '.startup_runtime_state.working_state_restore_lineage_event_present == false' >/dev/null
printf '%s\n' "${drift_output}" | jq -e '.startup_runtime_state.execctl_active_lease_source_event_id_present == false' >/dev/null
printf '%s\n' "${drift_output}" | jq -e '.startup_runtime_state.artifact_gate_semantics_consistent_present == true' >/dev/null

reconcile_output="$(./scripts/continuity_startup.sh --repo-root "${repo_root}" --namespace continuity --json)"
printf '%s\n' "${reconcile_output}" | jq -e '.working_state_restore.state_lineage.authoritative_event_id | type == "string" and length > 0' >/dev/null

restored_output="$(./scripts/continuity_startup_state.sh --repo-root "${repo_root}" --json)"
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.gate_semantics_consistent == true' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.source_event_match == true' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.working_state_restore_lineage_event_present == true' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.execctl_active_lease_source_event_id_present == true' >/dev/null

temp_home="$(mktemp -d "${tmp_dir}/reconnect-home.XXXXXX")"
HOME="${temp_home}" RUSTUP_HOME="${host_rustup_home}" CARGO_HOME="${host_cargo_home}" ./scripts/onboard_local.sh --client codex --yes --skip-stack --skip-release-build >/dev/null
HOME="${temp_home}" RUSTUP_HOME="${host_rustup_home}" CARGO_HOME="${host_cargo_home}" ./scripts/reconnect_local.sh --client codex >/dev/null
test -f "${temp_home}/.codex/config.toml"
grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"
grep -Fq './scripts/reconnect_local.sh --client codex' AGENTS.md
grep -Fq './scripts/amai_exec.sh bootstrap reconnect --client codex --yes' AGENTS.md

echo "proof_continuity_reconcile_reconnect_hostile: ok"
