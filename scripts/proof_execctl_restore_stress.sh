#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="execctl_restore_stress_$(date +%s%N)"
project_root="$(mktemp -d)"
tmp_dir="$(mktemp -d)"
restore_output="${tmp_dir}/restore.json"
latency_jsonl="${tmp_dir}/latency.jsonl"
namespace_code="continuity"
primary_scope="proof_execctl_restore_primary_${project_code}"
primary_thread="proof-execctl-restore-primary-${project_code}"
foreign_scope="proof_execctl_restore_foreign_${project_code}"
foreign_thread="proof-execctl-restore-foreign-${project_code}"
rounds=4

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}" "${tmp_dir}"
}
trap cleanup EXIT

record_latency() {
  local kind="$1"
  local started_ms="$2"
  local ended_ms="$3"
  printf '{"kind":"%s","ms":%s}\n' "${kind}" "$((ended_ms - started_ms))" >>"${latency_jsonl}"
}

run_release() {
  local scope="$1"
  local thread_id="$2"
  shift 2
  AMAI_AGENT_SCOPE="${scope}" CODEX_THREAD_ID="${thread_id}" \
    "${repo_root}/target/release/amai" "$@"
}

measure_release() {
  local kind="$1"
  local scope="$2"
  local thread_id="$3"
  shift 3
  local started_ms ended_ms
  started_ms="$(date +%s%3N)"
  run_release "${scope}" "${thread_id}" "$@" >/dev/null
  ended_ms="$(date +%s%3N)"
  record_latency "${kind}" "${started_ms}" "${ended_ms}"
}

fetch_restore_for_scope() {
  local scope="$1"
  psql "${dsn}" -Atqc \
    "SELECT payload::text
       FROM ami.observability_snapshots
      WHERE snapshot_kind = 'working_state_restore'
        AND scope_project_code = '${project_code}'
        AND scope_namespace_code = '${namespace_code}'
        AND payload #>> ARRAY['working_state_restore', 'agent_scope'] = '${scope}'
   ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC
      LIMIT 1"
}

measure_fetch_restore() {
  local scope="$1"
  local started_ms ended_ms
  started_ms="$(date +%s%3N)"
  fetch_restore_for_scope "${scope}" >"${restore_output}"
  ended_ms="$(date +%s%3N)"
  record_latency "restore_read" "${started_ms}" "${ended_ms}"
  test -s "${restore_output}"
}

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

./target/release/amai project register \
  --code "${project_code}" \
  --display-name "ExecCtl Restore Stress Probe" \
  --repo-root "${project_root}" >/dev/null

./target/release/amai namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

measure_release "handoff" "${primary_scope}" "${primary_thread}" \
  continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Seed active line" \
  --next-step "Seed the execctl restore burst."

measure_fetch_restore "${primary_scope}"
jq -e \
  --arg scope "${primary_scope}" \
  --arg thread_id "${primary_thread}" \
  '
  .working_state_restore.agent_scope == $scope
  and .working_state_restore.thread_id == $thread_id
  and .working_state_restore.current_goal == "Seed active line"
  and .working_state_restore.state_lineage.authoritative_event_kind == "continuity_handoff"
  and .working_state_restore.recent_actions[0].event_kind == "continuity_handoff"
  and .working_state_restore.execctl_resume_state == "clear"
  ' "${restore_output}" >/dev/null

last_primary_goal="Seed active line"
last_primary_authoritative_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"

for round in $(seq 1 "${rounds}"); do
  headline="Burst handoff ${round}"
  next_step="Verify restore freshness after burst handoff ${round}."
  percent="$(case "${round}" in
    1|4|7) echo 70 ;;
    2|5|8) echo 50 ;;
    *) echo 80 ;;
  esac)"

  measure_release "handoff" "${primary_scope}" "${primary_thread}" \
    continuity handoff \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --headline "${headline}" \
    --next-step "${next_step}"

  measure_fetch_restore "${primary_scope}"
  jq -e \
    --arg scope "${primary_scope}" \
    --arg thread_id "${primary_thread}" \
    --arg headline "${headline}" \
    --argjson pending_count "${round}" \
    '
    .working_state_restore.agent_scope == $scope
    and .working_state_restore.thread_id == $thread_id
    and .working_state_restore.current_goal == $headline
    and .working_state_restore.state_lineage.authoritative_event_kind == "continuity_handoff"
    and .working_state_restore.recent_actions[0].event_kind == "continuity_handoff"
    and .working_state_restore.recent_actions[0].headline == $headline
    and (.working_state_restore.pending_return_queue | length) == $pending_count
    and .working_state_restore.project_task_tree.pending_return_count == $pending_count
    and .working_state_restore.restore_freshness_state == "fresh"
    ' "${restore_output}" >/dev/null

  authoritative_id="$(jq -r '.working_state_restore.state_lineage.authoritative_event_id' "${restore_output}")"

  measure_release "client_budget_target" "${primary_scope}" "${primary_thread}" \
    continuity client-budget-target \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --percent "${percent}"

  measure_fetch_restore "${primary_scope}"
  jq -e \
    --arg scope "${primary_scope}" \
    --arg thread_id "${primary_thread}" \
    --arg headline "${headline}" \
    --arg authoritative_id "${authoritative_id}" \
    --argjson pending_count "${round}" \
    --argjson percent "${percent}" \
    '
    .working_state_restore.agent_scope == $scope
    and .working_state_restore.thread_id == $thread_id
    and .working_state_restore.current_goal == $headline
    and .working_state_restore.client_budget_target_percent == $percent
    and .working_state_restore.state_lineage.authoritative_event_id == $authoritative_id
    and .working_state_restore.state_lineage.authoritative_event_kind == "continuity_handoff"
    and .working_state_restore.recent_actions[0].event_kind == "client_budget_target_update"
    and .working_state_restore.recent_actions[0].headline == $headline
    and (.working_state_restore.pending_return_queue | length) == $pending_count
    ' "${restore_output}" >/dev/null

  last_primary_goal="${headline}"
  last_primary_authoritative_id="${authoritative_id}"
done

measure_release "handoff" "${foreign_scope}" "${foreign_thread}" \
  continuity handoff \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --headline "Foreign scope line" \
  --next-step "This line must stay isolated from the primary scope."

measure_fetch_restore "${foreign_scope}"
jq -e \
  --arg scope "${foreign_scope}" \
  --arg thread_id "${foreign_thread}" \
  '
  .working_state_restore.agent_scope == $scope
  and .working_state_restore.thread_id == $thread_id
  and .working_state_restore.current_goal == "Foreign scope line"
  and .working_state_restore.execctl_resume_state == "clear"
  and (.working_state_restore.pending_return_queue | length) == 0
  and .working_state_restore.state_lineage.authoritative_event_kind == "continuity_handoff"
  ' "${restore_output}" >/dev/null

measure_fetch_restore "${primary_scope}"
jq -e \
  --arg scope "${primary_scope}" \
  --arg thread_id "${primary_thread}" \
  --arg headline "${last_primary_goal}" \
  --arg authoritative_id "${last_primary_authoritative_id}" \
  '
  .working_state_restore.agent_scope == $scope
  and .working_state_restore.thread_id == $thread_id
  and .working_state_restore.current_goal == $headline
  and .working_state_restore.state_lineage.authoritative_event_id == $authoritative_id
  and .working_state_restore.current_goal != "Foreign scope line"
  ' "${restore_output}" >/dev/null

LATENCY_JSONL="${latency_jsonl}" python3 - <<'PY'
import json
import math
import os
from collections import defaultdict

path = os.environ["LATENCY_JSONL"]
buckets = defaultdict(list)
with open(path, "r", encoding="utf-8") as fh:
    for line in fh:
        row = json.loads(line)
        buckets[row["kind"]].append(float(row["ms"]))

required = {"handoff", "restore_read"}
missing = required.difference(buckets)
if missing:
    raise SystemExit(f"proof_execctl_restore_stress: missing latency buckets: {sorted(missing)}")

def percentile(values, pct):
    ordered = sorted(values)
    index = max(0, math.ceil((pct / 100.0) * len(ordered)) - 1)
    return ordered[index]

limits = {
    "handoff": 2000.0,
    "restore_read": 250.0,
}
max_limits = {
    "handoff": 3000.0,
    "restore_read": 1000.0,
}

for kind in sorted(required):
    values = buckets[kind]
    p95 = percentile(values, 95)
    max_v = max(values)
    if p95 > limits[kind]:
        raise SystemExit(
            f"proof_execctl_restore_stress: {kind} p95_ms={p95:.2f} exceeds {limits[kind]:.2f}"
        )
    if max_v > max_limits[kind]:
        raise SystemExit(
            f"proof_execctl_restore_stress: {kind} max_ms={max_v:.2f} exceeds {max_limits[kind]:.2f}"
        )

if "client_budget_target" not in buckets:
    raise SystemExit("proof_execctl_restore_stress: missing client_budget_target latency bucket")
client_target_max = max(buckets["client_budget_target"])
if client_target_max > 15000.0:
    raise SystemExit(
        f"proof_execctl_restore_stress: client_budget_target max_ms={client_target_max:.2f} exceeds 15000.00"
    )

print("proof_execctl_restore_stress: latency ok")
PY

printf 'proof_execctl_restore_stress: PASS\n'
