#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "${repo_root}"

source "${repo_root}/scripts/load_env.sh"

cargo build --release --quiet

state_file=".amai/continuity/project-chat-startup-state.json"
dedupe_file="state/token_budget/continuity_restore_observed_dedupe.json"
mkdir -p state/locks
exec 9>state/locks/startup_runtime_state_mutation.lock
flock --exclusive 9

tmp_dir="$(mktemp -d)"
state_backup="${tmp_dir}/project-chat-startup-state.backup.json"
dedupe_backup="${tmp_dir}/continuity_restore_observed_dedupe.backup.json"
state_had_file=0
dedupe_had_file=0

if [[ -f "${state_file}" ]]; then
  cp "${state_file}" "${state_backup}"
  state_had_file=1
fi
if [[ -f "${dedupe_file}" ]]; then
  cp "${dedupe_file}" "${dedupe_backup}"
  dedupe_had_file=1
fi

cleanup() {
  if [[ "${state_had_file}" -eq 1 ]]; then
    cp "${state_backup}" "${state_file}"
  else
    rm -f "${state_file}"
  fi
  if [[ "${dedupe_had_file}" -eq 1 ]]; then
    cp "${dedupe_backup}" "${dedupe_file}"
  else
    rm -f "${dedupe_file}"
  fi
  rm -rf "${tmp_dir}"
}
trap cleanup EXIT

agent_scope="amai::continuity::default"
query_event_count() {
  local source_kind="$1"
  psql "${AMI_POSTGRES_DSN}" -At -F $'\t' -v ON_ERROR_STOP=1 -c "
SELECT COUNT(*)
FROM ami.observability_snapshots
WHERE snapshot_kind = 'token_budget_event'
  AND scope_project_code = 'amai'
  AND scope_namespace_code = 'continuity'
  AND payload #>> '{token_budget_event,source_kind}' = '${source_kind}';
"
}

query_lease_heartbeat() {
  psql "${AMI_POSTGRES_DSN}" -At -F $'\t' -v ON_ERROR_STOP=1 -c "
SELECT COALESCE(MAX(l.heartbeat_at_epoch_ms), 0)
FROM ami.execctl_task_leases AS l
JOIN ami.projects AS p ON p.project_id = l.project_id
JOIN ami.namespaces AS n ON n.namespace_id = l.namespace_id
WHERE p.code = 'amai'
  AND n.code = 'continuity'
  AND l.agent_scope = '${agent_scope}';
"
}

run_preview_phase() {
  local phase="$1"
  local source_kind="$2"
  local output_file="$3"
  local expect_state_status="$4"
  local state_baseline="$5"
  local dedupe_baseline="$6"
  local event_count_before lease_heartbeat_before event_count_after lease_heartbeat_after

  event_count_before="$(query_event_count "${source_kind}")"
  lease_heartbeat_before="$(query_lease_heartbeat)"

  AMAI_AGENT_SCOPE="${agent_scope}" \
    ./target/release/amai continuity startup \
      --project amai \
      --repo-root "${repo_root}" \
      --namespace continuity \
      --token-source-kind "${source_kind}" \
      --internal-preview-json \
      > "${output_file}"

  event_count_after="$(query_event_count "${source_kind}")"
  lease_heartbeat_after="$(query_lease_heartbeat)"
  if [[ "${event_count_before}" != "${event_count_after}" ]]; then
    echo "${phase}: preview startup unexpectedly recorded token_budget_event snapshots" >&2
    exit 1
  fi
  if [[ "${lease_heartbeat_before}" != "${lease_heartbeat_after}" ]]; then
    echo "${phase}: preview startup unexpectedly refreshed execctl lease heartbeat" >&2
    exit 1
  fi
  if [[ "${expect_state_status}" == "missing" ]]; then
    if [[ -e "${state_file}" ]]; then
      echo "${phase}: preview startup unexpectedly created ${state_file}" >&2
      exit 1
    fi
  else
    local state_mtime_after
    state_mtime_after="$(stat -c %Y "${state_file}")"
    if [[ "${state_baseline}" != "${state_mtime_after}" ]]; then
      echo "${phase}: preview startup unexpectedly rewrote ${state_file}" >&2
      exit 1
    fi
  fi
  local dedupe_mtime_after
  dedupe_mtime_after="$(stat -c %Y "${dedupe_file}" 2>/dev/null || printf 'missing')"
  if [[ "${dedupe_baseline}" != "${dedupe_mtime_after}" ]]; then
    echo "${phase}: preview startup unexpectedly rewrote ${dedupe_file}" >&2
    exit 1
  fi

  REPO_ROOT="${repo_root}" jq -e '
    .continuity_startup.project.repo_root == env.REPO_ROOT and
    (.chat_start_restore.prompt_text | type) == "string" and
    .chat_start_restore.headline == "startup_runtime_audit_probe" and
    .chat_start_restore.next_step == "materialize_authoritative_startup_payload_after_runtime_reconcile" and
    .working_state_restore.current_goal == null and
    .working_state_restore.next_step == null
  ' "${output_file}" >/dev/null
}

dedupe_mtime_baseline="$(stat -c %Y "${dedupe_file}" 2>/dev/null || printf 'missing')"
rm -f "${state_file}"
run_preview_phase \
  "missing-state" \
  "proof_continuity_startup_preview_no_durable_side_effects_missing_$(date +%s%N)" \
  "${tmp_dir}/preview-missing-state.json" \
  "missing" \
  "missing" \
  "${dedupe_mtime_baseline}"

if [[ "${state_had_file}" -eq 1 ]]; then
  cp "${state_backup}" "${state_file}"
  state_mtime_baseline="$(stat -c %Y "${state_file}")"
  dedupe_mtime_baseline="$(stat -c %Y "${dedupe_file}" 2>/dev/null || printf 'missing')"
  run_preview_phase \
    "existing-state" \
    "proof_continuity_startup_preview_no_durable_side_effects_existing_$(date +%s%N)" \
    "${tmp_dir}/preview-existing-state.json" \
    "present" \
    "${state_mtime_baseline}" \
    "${dedupe_mtime_baseline}"
fi

echo "proof_continuity_startup_preview_no_durable_side_effects: ok"
