#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
project_code="commitment_task_graph_probe_$(date +%s%N)"
project_root="$(mktemp -d)"
namespace_code="continuity"
closed_at_epoch_ms=1775597224414
archived_at_epoch_ms=1775597225414

cleanup() {
  psql "${dsn}" -qc "DELETE FROM ami.projects WHERE code='${project_code}'" >/dev/null 2>&1 || true
  rm -rf "${project_root}"
}
trap cleanup EXIT

run_release() {
  "${repo_root}/target/release/amai" "$@"
}

cd "${repo_root}"

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null

run_release project register \
  --code "${project_code}" \
  --display-name "Commitment Task Graph Probe" \
  --repo-root "${project_root}" >/dev/null

run_release namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name Continuity >/dev/null

prefix="proof-stage3-$(date +%s%3N)"

root_out="$(run_release memory create-task-node \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --status-payload-json '{}' \
  --metadata-json '{}' \
  --task-key "${prefix}-root" \
  --task-role root \
  --headline "${prefix} root line" \
  --summary "root" \
  --next-step "continue root" \
  --execution-state active \
  --lifecycle-state hot)"
root_id="$(printf '%s' "${root_out}" | sed -E 's/^task node created: ([^ ]+) ::.*$/\1/')"

child_out="$(run_release memory create-task-node \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --status-payload-json '{}' \
  --metadata-json '{}' \
  --parent-task-node-id "${root_id}" \
  --task-key "${prefix}-child" \
  --task-role child \
  --headline "${prefix} child line" \
  --summary "child" \
  --next-step "continue child" \
  --execution-state ready \
  --lifecycle-state hot)"
child_id="$(printf '%s' "${child_out}" | sed -E 's/^task node created: ([^ ]+) ::.*$/\1/')"

closed_out="$(run_release memory create-task-node \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --status-payload-json '{}' \
  --metadata-json '{}' \
  --task-key "${prefix}-old" \
  --task-role workline \
  --headline "${prefix} old branch" \
  --summary "old branch" \
  --next-step "resume old" \
  --execution-state done \
  --lifecycle-state closed \
  --closed-at-epoch-ms "${closed_at_epoch_ms}")"
closed_id="$(printf '%s' "${closed_out}" | sed -E 's/^task node created: ([^ ]+) ::.*$/\1/')"

archived_out="$(run_release memory create-task-node \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --status-payload-json '{}' \
  --metadata-json '{}' \
  --task-key "${prefix}-archive" \
  --task-role historical \
  --headline "${prefix} archive branch" \
  --summary "archive branch" \
  --next-step "none" \
  --execution-state done \
  --lifecycle-state archived \
  --closed-at-epoch-ms "${closed_at_epoch_ms}" \
  --archived-at-epoch-ms "${archived_at_epoch_ms}")"
archived_id="$(printf '%s' "${archived_out}" | sed -E 's/^task node created: ([^ ]+) ::.*$/\1/')"

ambiguous_out="$(run_release memory create-task-node \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --status-payload-json '{}' \
  --metadata-json '{}' \
  --task-key "${prefix}-ambiguous" \
  --task-role proposal \
  --headline "${prefix} ambiguous branch" \
  --summary "ambiguous branch" \
  --next-step "collect more evidence" \
  --execution-state active \
  --lifecycle-state hot)"
ambiguous_id="$(printf '%s' "${ambiguous_out}" | sed -E 's/^task node created: ([^ ]+) ::.*$/\1/')"

run_release memory create-task-event \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --task-node-id "${root_id}" \
  --event-kind created \
  --next-execution-state active \
  --next-lifecycle-state hot \
  --event-payload-json '{"proof":"commitment_task_graph_integrity","role":"root"}' >/dev/null

run_release memory create-task-event \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --task-node-id "${child_id}" \
  --event-kind branched_child \
  --prior-execution-state proposed \
  --next-execution-state ready \
  --next-lifecycle-state hot \
  --event-payload-json '{"proof":"commitment_task_graph_integrity","role":"child"}' >/dev/null

run_release memory create-task-event \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --task-node-id "${archived_id}" \
  --event-kind archived \
  --prior-execution-state done \
  --next-execution-state done \
  --prior-lifecycle-state closed \
  --next-lifecycle-state archived \
  --event-payload-json '{"proof":"commitment_task_graph_integrity","role":"archived"}' >/dev/null

pending_link_decision_out="$(run_release memory create-link-decision \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --task-node-id "${ambiguous_id}" \
  --candidate-task-node-id "${ambiguous_id}" \
  --decision-outcome pending_link_proposal \
  --legality-passed \
  --scope-filter-passed \
  --decision-reason "need more evidence before hard branch link" \
  --decision-payload-json '{"pending_link_ttl_epoch_ms":1775599999999,"additional_evidence_request":"attach raw diff and latest operator note"}' \
  --classifier-label pending_link_proposal \
  --classifier-score 0.21 \
  --source-event-id "${prefix}-pending-link" \
  --artifact-ref "artifact://proof/${prefix}/pending-link" \
  --evidence-span-json '{"proof":"commitment_task_graph_integrity","kind":"pending_link_proposal"}')"
pending_link_decision_id="$(printf '%s' "${pending_link_decision_out}" | sed -E 's/^memory link decision created: ([^ ]+) ::.*$/\1/')"

abstain_decision_out="$(run_release memory create-link-decision \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --task-node-id "${ambiguous_id}" \
  --decision-outcome abstain \
  --legality-passed \
  --scope-filter-passed \
  --decision-reason "not enough evidence to choose line honestly" \
  --classifier-label abstain \
  --classifier-score 0.19 \
  --source-event-id "${prefix}-abstain" \
  --artifact-ref "artifact://proof/${prefix}/abstain" \
  --evidence-span-json '{"proof":"commitment_task_graph_integrity","kind":"abstain"}')"
abstain_decision_id="$(printf '%s' "${abstain_decision_out}" | sed -E 's/^memory link decision created: ([^ ]+) ::.*$/\1/')"

escalate_decision_out="$(run_release memory create-link-decision \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --task-node-id "${ambiguous_id}" \
  --decision-outcome escalate \
  --legality-passed \
  --scope-filter-passed \
  --decision-reason "need stronger operator evidence before branch link" \
  --decision-payload-json '{"additional_evidence_request":"attach raw trace and operator confirmation"}' \
  --classifier-label escalate \
  --classifier-score 0.23 \
  --source-event-id "${prefix}-escalate" \
  --artifact-ref "artifact://proof/${prefix}/escalate" \
  --evidence-span-json '{"proof":"commitment_task_graph_integrity","kind":"escalate"}')"
escalate_decision_id="$(printf '%s' "${escalate_decision_out}" | sed -E 's/^memory link decision created: ([^ ]+) ::.*$/\1/')"

set +e
duplicate_out="$(run_release memory create-task-node \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --status-payload-json '{}' \
  --metadata-json '{}' \
  --task-key "${prefix}-old" \
  --task-role workline \
  --headline "${prefix} old branch" \
  --summary "duplicate exact branch" \
  --next-step "duplicate exact branch" \
  --execution-state proposed \
  --lifecycle-state hot 2>&1)"
duplicate_status=$?
set -e

if [ "${duplicate_status}" -eq 0 ]; then
  echo "proof_commitment_task_graph_integrity: duplicate task_key unexpectedly accepted" >&2
  exit 1
fi
printf '%s' "${duplicate_out}" | grep -q "task node conflicts with existing task_key in the same namespace"

run_release memory create-task-event \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --derivation-kind operator_write \
  --task-node-id "${closed_id}" \
  --event-kind resumed \
  --prior-execution-state done \
  --next-execution-state active \
  --prior-lifecycle-state closed \
  --next-lifecycle-state hot \
  --event-payload-json '{"proof":"commitment_task_graph_integrity","reason":"same line resumed"}' >/dev/null

root_json="$(run_release memory get-task-node --task-node-id "${root_id}")"
child_json="$(run_release memory get-task-node --task-node-id "${child_id}")"
closed_json="$(run_release memory get-task-node --task-node-id "${closed_id}")"
archived_json="$(run_release memory get-task-node --task-node-id "${archived_id}")"

jq -e --arg root_id "${root_id}" '
  .task_role == "child"
  and .parent_task_node_id == $root_id
  and .lifecycle_state == "hot"
' <<<"${child_json}" >/dev/null

jq -e '
  .task_role == "root"
  and .execution_state == "active"
  and .lifecycle_state == "hot"
' <<<"${root_json}" >/dev/null

jq -e '
  .execution_state == "active"
  and .lifecycle_state == "hot"
  and .reopened_count == 1
  and .closed_at_epoch_ms == null
  and .archived_at_epoch_ms == null
' <<<"${closed_json}" >/dev/null

jq -e --argjson closed_at "${closed_at_epoch_ms}" '
  .execution_state == "done"
  and .lifecycle_state == "archived"
  and .closed_at_epoch_ms == $closed_at
  and (.archived_at_epoch_ms != null)
  and (.archived_at_epoch_ms >= $closed_at)
' <<<"${archived_json}" >/dev/null

lineage_count="$(psql "${dsn}" -Atqc \
  "SELECT COUNT(*)
     FROM ami.task_events
    WHERE task_node_id = '${child_id}'
      AND event_kind = 'branched_child'")"
[ "${lineage_count}" = "1" ]

pending_link_event_row="$(psql "${dsn}" -At -F $'\t' -qc \
  "SELECT event_kind,
          event_payload->>'decision_outcome',
          event_payload->>'additional_evidence_request',
          event_payload->>'pending_link_ttl_epoch_ms'
     FROM ami.task_events
    WHERE task_node_id = '${ambiguous_id}'
      AND source_event_id = 'memory_link_decision:${pending_link_decision_id}'")"
[ -n "${pending_link_event_row}" ]
IFS=$'\t' read -r pending_event_kind pending_decision_outcome pending_request pending_ttl <<<"${pending_link_event_row}"
[ "${pending_event_kind}" = "evidence_request" ]
[ "${pending_decision_outcome}" = "pending_link_proposal" ]
[ "${pending_request}" = "attach raw diff and latest operator note" ]
[ "${pending_ttl}" = "1775599999999" ]

abstain_event_row="$(psql "${dsn}" -At -F $'\t' -qc \
  "SELECT event_kind,
          event_payload->>'decision_outcome',
          event_payload->>'decision_reason'
     FROM ami.task_events
    WHERE task_node_id = '${ambiguous_id}'
      AND source_event_id = 'memory_link_decision:${abstain_decision_id}'")"
[ -n "${abstain_event_row}" ]
IFS=$'\t' read -r abstain_event_kind abstain_decision_outcome abstain_reason <<<"${abstain_event_row}"
[ "${abstain_event_kind}" = "state_change" ]
[ "${abstain_decision_outcome}" = "abstain" ]
[ "${abstain_reason}" = "not enough evidence to choose line honestly" ]

escalate_event_row="$(psql "${dsn}" -At -F $'\t' -qc \
  "SELECT event_kind,
          event_payload->>'decision_outcome',
          event_payload->>'additional_evidence_request'
     FROM ami.task_events
    WHERE task_node_id = '${ambiguous_id}'
      AND source_event_id = 'memory_link_decision:${escalate_decision_id}'")"
[ -n "${escalate_event_row}" ]
IFS=$'\t' read -r escalate_event_kind escalate_decision_outcome escalate_request <<<"${escalate_event_row}"
[ "${escalate_event_kind}" = "evidence_request" ]
[ "${escalate_decision_outcome}" = "escalate" ]
[ "${escalate_request}" = "attach raw trace and operator confirmation" ]

printf 'proof_commitment_task_graph_integrity: PASS\n'
