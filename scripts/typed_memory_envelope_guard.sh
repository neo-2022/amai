#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

json_mode=false
if [[ "${1:-}" == "--json" ]]; then
  json_mode=true
fi

dsn="${AMI_POSTGRES_DSN}"

required_memory_item_columns=(
  memory_item_id
  workspace_id
  project_id
  owner_agent_id
  visibility_scope
  item_kind
  sensitivity_class
  truth_state
  trust_state
  verification_state
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  observed_at_epoch_ms
  recorded_at_epoch_ms
  valid_from_epoch_ms
  valid_to_epoch_ms
  last_verified_at_epoch_ms
  ingest_seq
  object_version
  causation_id
  correlation_id
  utility_score
  freshness_score
  retention_class
  ttl_epoch_ms
  imported_from
  schema_version
  superseded_by_memory_item_id
  created_at
)

required_memory_card_columns=(
  memory_card_id
  project_id
  namespace_id
  provenance
  truth_state
  verification_state
  status
  derivation_kind
  candidate_class
  source_kind
  hot_path_write_eligible
  background_consolidation_recommended
  observed_at_epoch_ms
  recorded_at_epoch_ms
  valid_from_epoch_ms
  valid_to_epoch_ms
  last_verified_at_epoch_ms
  created_at
)

required_skill_card_columns=(
  skill_card_id
  workspace_id
  project_id
  namespace_id
  skill_source_event_ids
  skill_artifact_refs
  skill_evidence_span
  skill_candidate_class
  skill_derivation_kind
  skill_source_kind
  skill_hot_path_write_eligible
  skill_background_consolidation_recommended
  skill_trust_state
  skill_verification_state
  created_at
  updated_at
)

required_task_node_columns=(
  task_node_id
  workspace_id
  project_id
  namespace_id
  source_event_ids
  artifact_refs
  evidence_span
  candidate_class
  derivation_kind
  source_kind
  hot_path_write_eligible
  background_consolidation_recommended
  created_at
  updated_at
)

required_task_event_columns=(
  task_event_id
  workspace_id
  project_id
  namespace_id
  task_node_id
  source_kind
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_memory_link_decision_columns=(
  memory_link_decision_id
  workspace_id
  project_id
  namespace_id
  task_node_id
  retrieval_trace_id
  candidate_task_node_id
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  recorded_at_epoch_ms
  created_at
)

required_pending_link_proposal_columns=(
  pending_link_proposal_id
  workspace_id
  project_id
  namespace_id
  task_node_id
  retrieval_trace_id
  candidate_task_node_id
  ttl_epoch_ms
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
  updated_at
)

required_artifact_ref_columns=(
  artifact_ref_id
  project_id
  namespace_id
  artifact_kind
  bucket
  object_key
  source_kind
  source_event_ids
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_skill_evidence_bundle_columns=(
  skill_evidence_bundle_id
  skill_card_id
  evidence_kind
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_memory_relation_edge_columns=(
  memory_relation_edge_id
  project_id
  namespace_id
  source_memory_card_id
  target_memory_card_id
  relation_type
  relation_state
  evidence
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  recorded_at_epoch_ms
  created_at
)

required_skill_trigger_match_columns=(
  skill_trigger_match_id
  skill_card_id
  match_scope
  trigger_input
  matched
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_skill_trial_run_columns=(
  skill_trial_run_id
  skill_card_id
  application_mode
  matched
  applied
  outcome
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_skill_eval_columns=(
  skill_eval_id
  skill_card_id
  verdict
  safe_to_apply
  quality_ok
  truth_ok
  utility_delta
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_skill_reuse_log_columns=(
  skill_reuse_log_id
  skill_card_id
  reuse_mode
  outcome
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_import_packet_columns=(
  import_packet_id
  source_project_id
  target_project_id
  artifact_refs
  source_kind
  source_event_ids
  message_refs
  evidence_span
  derivation_kind
  schema_version
  trust_state
  verification_state
  borrowed_status
  created_at
)

required_shared_asset_columns=(
  shared_asset_id
  workspace_id
  code
  asset_kind
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  visibility_scope
  status
  created_at
)

required_shared_asset_project_columns=(
  shared_asset_id
  project_id
  binding_kind
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_memory_provenance_columns=(
  memory_provenance_id
  memory_item_id
  source_kind
  source_event_id
  trust_level
  message_refs
  evidence_span
  derivation_kind
  observed_at_epoch_ms
  recorded_at_epoch_ms
  valid_from_epoch_ms
  valid_to_epoch_ms
  schema_version
  details
)

required_memory_raw_event_columns=(
  memory_raw_event_id
  workspace_id
  project_id
  namespace_id
  event_kind
  item_kind
  visibility_scope
  sensitivity_class
  derivation_kind
  truth_state
  trust_state
  verification_state
  lifecycle_state
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  server_received_at_epoch_ms
  server_order_seq
  payload
  created_at
)

required_memory_write_outbox_columns=(
  memory_write_outbox_id
  workspace_id
  project_id
  namespace_id
  memory_raw_event_id
  memory_item_id
  subject
  delivery_kind
  delivery_state
  payload
  attempt_count
  last_error
  created_at
)

required_retrieval_trace_columns=(
  retrieval_trace_id
  workspace_id
  project_id
  namespace_id
  context_pack_id
  query_text
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  final_decision
  created_at
)

required_restore_pack_columns=(
  restore_pack_id
  workspace_id
  project_id
  namespace_id
  pack_kind
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_policy_rule_columns=(
  policy_rule_id
  workspace_id
  project_id
  namespace_id
  rule_code
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_quarantine_item_columns=(
  quarantine_item_id
  workspace_id
  project_id
  namespace_id
  entity_kind
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_memory_edge_columns=(
  memory_edge_id
  workspace_id
  project_id
  namespace_id
  source_memory_item_id
  target_memory_item_id
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_memory_conflict_columns=(
  memory_conflict_id
  workspace_id
  project_id
  namespace_id
  conflict_kind
  source_kind
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  schema_version
  created_at
)

required_view_columns=(
  memory_id
  memory_type
  scope_type
  workspace_id
  project_id
  owner_agent_id
  visibility
  sensitivity_class
  truth_state
  trust_state
  verification_state
  created_at
  source_event_ids
  artifact_refs
  message_refs
  evidence_span
  derivation_kind
  valid_from_epoch_ms
  valid_to_epoch_ms
  supersedes
  conflicts_with
  utility_score
  freshness_score
  retention_class
  ttl
  imported_from
  schema_version
  ingest_seq
  object_version
  causation_id
  correlation_id
  candidate_class
  source_kind
  hot_path_write_eligible
  background_consolidation_recommended
)

check_columns() {
  local table_name="$1"
  shift
  local columns=("$@")
  local missing=()
  for column in "${columns[@]}"; do
    if ! psql "${dsn}" -Atqc "
      SELECT 1
      FROM information_schema.columns
      WHERE table_schema = 'ami'
        AND table_name = '${table_name}'
        AND column_name = '${column}'
    " | grep -qx '1'; then
      missing+=("${column}")
    fi
  done
  if [[ "${#missing[@]}" -gt 0 ]]; then
    printf '%s\n' "${missing[@]}"
  fi
}

json_array() {
  if [[ "$#" -eq 0 ]]; then
    jq -n '[]'
  else
    printf '%s\n' "$@" | jq -R . | jq -s .
  fi
}

mapfile -t missing_memory_item_columns < <(check_columns "memory_items" "${required_memory_item_columns[@]}")
mapfile -t missing_memory_card_columns < <(check_columns "memory_cards" "${required_memory_card_columns[@]}")
mapfile -t missing_skill_card_columns < <(check_columns "skill_cards" "${required_skill_card_columns[@]}")
mapfile -t missing_task_node_columns < <(check_columns "task_nodes" "${required_task_node_columns[@]}")
mapfile -t missing_task_event_columns < <(check_columns "task_events" "${required_task_event_columns[@]}")
mapfile -t missing_memory_link_decision_columns < <(check_columns "memory_link_decisions" "${required_memory_link_decision_columns[@]}")
mapfile -t missing_pending_link_proposal_columns < <(check_columns "pending_link_proposals" "${required_pending_link_proposal_columns[@]}")
mapfile -t missing_artifact_ref_columns < <(check_columns "artifact_refs" "${required_artifact_ref_columns[@]}")
mapfile -t missing_skill_evidence_bundle_columns < <(check_columns "skill_evidence_bundles" "${required_skill_evidence_bundle_columns[@]}")
mapfile -t missing_memory_relation_edge_columns < <(check_columns "memory_relation_edges" "${required_memory_relation_edge_columns[@]}")
mapfile -t missing_skill_trigger_match_columns < <(check_columns "skill_trigger_matches" "${required_skill_trigger_match_columns[@]}")
mapfile -t missing_skill_trial_run_columns < <(check_columns "skill_trial_runs" "${required_skill_trial_run_columns[@]}")
mapfile -t missing_skill_eval_columns < <(check_columns "skill_evals" "${required_skill_eval_columns[@]}")
mapfile -t missing_skill_reuse_log_columns < <(check_columns "skill_reuse_logs" "${required_skill_reuse_log_columns[@]}")
mapfile -t missing_import_packet_columns < <(check_columns "import_packets" "${required_import_packet_columns[@]}")
mapfile -t missing_shared_asset_columns < <(check_columns "shared_assets" "${required_shared_asset_columns[@]}")
mapfile -t missing_shared_asset_project_columns < <(check_columns "shared_asset_projects" "${required_shared_asset_project_columns[@]}")
mapfile -t missing_memory_provenance_columns < <(check_columns "memory_provenance" "${required_memory_provenance_columns[@]}")
mapfile -t missing_memory_raw_event_columns < <(check_columns "memory_raw_events" "${required_memory_raw_event_columns[@]}")
mapfile -t missing_memory_write_outbox_columns < <(check_columns "memory_write_outbox" "${required_memory_write_outbox_columns[@]}")
mapfile -t missing_retrieval_trace_columns < <(check_columns "retrieval_traces" "${required_retrieval_trace_columns[@]}")
mapfile -t missing_restore_pack_columns < <(check_columns "restore_packs" "${required_restore_pack_columns[@]}")
mapfile -t missing_policy_rule_columns < <(check_columns "policy_rules" "${required_policy_rule_columns[@]}")
mapfile -t missing_quarantine_item_columns < <(check_columns "quarantine_items" "${required_quarantine_item_columns[@]}")
mapfile -t missing_memory_edge_columns < <(check_columns "memory_edges" "${required_memory_edge_columns[@]}")
mapfile -t missing_memory_conflict_columns < <(check_columns "memory_conflicts" "${required_memory_conflict_columns[@]}")
mapfile -t missing_view_columns < <(check_columns "memory_envelopes" "${required_view_columns[@]}")

view_exists="$(psql "${dsn}" -Atqc "SELECT to_regclass('ami.memory_envelopes') IS NOT NULL")"
ingest_sequence_exists="$(psql "${dsn}" -Atqc "SELECT to_regclass('ami.memory_item_ingest_seq_seq') IS NOT NULL")"
raw_event_sequence_exists="$(psql "${dsn}" -Atqc "SELECT to_regclass('ami.memory_raw_event_server_order_seq_seq') IS NOT NULL")"
touch_trigger_exists="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM pg_trigger
    WHERE tgrelid = 'ami.memory_items'::regclass
      AND tgname = 'trg_ami_memory_items_touch_envelope'
  )::text
")"
import_packet_enforcer_present="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM pg_trigger
    WHERE tgrelid = 'ami.memory_items'::regclass
      AND tgname = 'trg_ami_memory_items_enforce_import_packet'
  )::text
")"
import_packet_enforcer_def="$(psql "${dsn}" -Atqc "
  SELECT pg_get_functiondef('ami.enforce_memory_item_import_packet'::regproc)
")"
raw_event_update_trigger_exists="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM pg_trigger
    WHERE tgrelid = to_regclass('ami.memory_raw_events')
      AND tgname = 'trg_ami_memory_raw_events_reject_update'
  )::text
")"
raw_event_delete_trigger_exists="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM pg_trigger
    WHERE tgrelid = to_regclass('ami.memory_raw_events')
      AND tgname = 'trg_ami_memory_raw_events_reject_delete'
  )::text
")"
task_event_update_trigger_exists="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM pg_trigger
    WHERE tgrelid = to_regclass('ami.task_events')
      AND tgname = 'trg_ami_task_events_reject_update'
  )::text
")"
task_event_delete_trigger_exists="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM pg_trigger
    WHERE tgrelid = to_regclass('ami.task_events')
      AND tgname = 'trg_ami_task_events_reject_delete'
  )::text
")"

status="pass"
reasons=()
if [[ "${view_exists}" != "t" ]]; then
  status="fail"
  reasons+=("memory_envelopes_view_missing")
fi
if [[ "${ingest_sequence_exists}" != "t" ]]; then
  status="fail"
  reasons+=("memory_item_ingest_sequence_missing")
fi
if [[ "${raw_event_sequence_exists}" != "t" ]]; then
  status="fail"
  reasons+=("memory_raw_event_sequence_missing")
fi
if [[ "${touch_trigger_exists}" != "true" ]]; then
  status="fail"
  reasons+=("memory_item_touch_trigger_missing")
fi
if [[ "${raw_event_update_trigger_exists}" != "true" || "${raw_event_delete_trigger_exists}" != "true" ]]; then
  status="fail"
  reasons+=("memory_raw_event_append_only_trigger_missing")
fi
if [[ "${task_event_update_trigger_exists}" != "true" || "${task_event_delete_trigger_exists}" != "true" ]]; then
  status="fail"
  reasons+=("task_event_append_only_trigger_missing")
fi
if [[ "${import_packet_enforcer_present}" != "true" ]]; then
  status="fail"
  reasons+=("memory_item_import_packet_trigger_missing")
fi
if ! grep -q "verified_write_back" <<<"${import_packet_enforcer_def}"; then
  status="fail"
  reasons+=("verified_write_back_enforcement_missing")
fi
if ! grep -q "writeback_evidence" <<<"${import_packet_enforcer_def}"; then
  status="fail"
  reasons+=("verified_write_back_evidence_guard_missing")
fi
if [[ "${#missing_memory_card_columns[@]}" -gt 0 && -n "${missing_memory_card_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_cards_missing_columns")
fi
if [[ "${#missing_skill_card_columns[@]}" -gt 0 && -n "${missing_skill_card_columns[0]}" ]]; then
  status="fail"
  reasons+=("skill_cards_missing_stage2_columns")
fi
if [[ "${#missing_task_node_columns[@]}" -gt 0 && -n "${missing_task_node_columns[0]}" ]]; then
  status="fail"
  reasons+=("task_nodes_missing_stage2_columns")
fi
if [[ "${#missing_task_event_columns[@]}" -gt 0 && -n "${missing_task_event_columns[0]}" ]]; then
  status="fail"
  reasons+=("task_events_missing_stage2_columns")
fi
if [[ "${#missing_memory_link_decision_columns[@]}" -gt 0 && -n "${missing_memory_link_decision_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_link_decisions_missing_stage2_columns")
fi
if [[ "${#missing_pending_link_proposal_columns[@]}" -gt 0 && -n "${missing_pending_link_proposal_columns[0]}" ]]; then
  status="fail"
  reasons+=("pending_link_proposals_missing_stage2_columns")
fi
if [[ "${#missing_artifact_ref_columns[@]}" -gt 0 && -n "${missing_artifact_ref_columns[0]}" ]]; then
  status="fail"
  reasons+=("artifact_refs_missing_stage2_columns")
fi
if [[ "${#missing_skill_evidence_bundle_columns[@]}" -gt 0 && -n "${missing_skill_evidence_bundle_columns[0]}" ]]; then
  status="fail"
  reasons+=("skill_evidence_bundles_missing_stage2_columns")
fi
if [[ "${#missing_memory_relation_edge_columns[@]}" -gt 0 && -n "${missing_memory_relation_edge_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_relation_edges_missing_stage2_columns")
fi
if [[ "${#missing_skill_trigger_match_columns[@]}" -gt 0 && -n "${missing_skill_trigger_match_columns[0]}" ]]; then
  status="fail"
  reasons+=("skill_trigger_matches_missing_stage2_columns")
fi
if [[ "${#missing_skill_trial_run_columns[@]}" -gt 0 && -n "${missing_skill_trial_run_columns[0]}" ]]; then
  status="fail"
  reasons+=("skill_trial_runs_missing_stage2_columns")
fi
if [[ "${#missing_skill_eval_columns[@]}" -gt 0 && -n "${missing_skill_eval_columns[0]}" ]]; then
  status="fail"
  reasons+=("skill_evals_missing_stage2_columns")
fi
if [[ "${#missing_skill_reuse_log_columns[@]}" -gt 0 && -n "${missing_skill_reuse_log_columns[0]}" ]]; then
  status="fail"
  reasons+=("skill_reuse_logs_missing_stage2_columns")
fi
if [[ "${#missing_import_packet_columns[@]}" -gt 0 && -n "${missing_import_packet_columns[0]}" ]]; then
  status="fail"
  reasons+=("import_packets_missing_stage2_columns")
fi
if [[ "${#missing_shared_asset_columns[@]}" -gt 0 && -n "${missing_shared_asset_columns[0]}" ]]; then
  status="fail"
  reasons+=("shared_assets_missing_stage2_columns")
fi
if [[ "${#missing_shared_asset_project_columns[@]}" -gt 0 && -n "${missing_shared_asset_project_columns[0]}" ]]; then
  status="fail"
  reasons+=("shared_asset_projects_missing_stage2_columns")
fi
if [[ "${#missing_memory_item_columns[@]}" -gt 0 && -n "${missing_memory_item_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_items_missing_columns")
fi
if [[ "${#missing_memory_provenance_columns[@]}" -gt 0 && -n "${missing_memory_provenance_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_provenance_missing_columns")
fi
if [[ "${#missing_memory_raw_event_columns[@]}" -gt 0 && -n "${missing_memory_raw_event_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_raw_events_missing_columns")
fi
if [[ "${#missing_memory_write_outbox_columns[@]}" -gt 0 && -n "${missing_memory_write_outbox_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_write_outbox_missing_columns")
fi
if [[ "${#missing_retrieval_trace_columns[@]}" -gt 0 && -n "${missing_retrieval_trace_columns[0]}" ]]; then
  status="fail"
  reasons+=("retrieval_traces_missing_stage2_columns")
fi
if [[ "${#missing_restore_pack_columns[@]}" -gt 0 && -n "${missing_restore_pack_columns[0]}" ]]; then
  status="fail"
  reasons+=("restore_packs_missing_stage2_columns")
fi
if [[ "${#missing_policy_rule_columns[@]}" -gt 0 && -n "${missing_policy_rule_columns[0]}" ]]; then
  status="fail"
  reasons+=("policy_rules_missing_stage2_columns")
fi
if [[ "${#missing_quarantine_item_columns[@]}" -gt 0 && -n "${missing_quarantine_item_columns[0]}" ]]; then
  status="fail"
  reasons+=("quarantine_items_missing_stage2_columns")
fi
if [[ "${#missing_memory_edge_columns[@]}" -gt 0 && -n "${missing_memory_edge_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_edges_missing_stage2_columns")
fi
if [[ "${#missing_memory_conflict_columns[@]}" -gt 0 && -n "${missing_memory_conflict_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_conflicts_missing_stage2_columns")
fi
if [[ "${#missing_view_columns[@]}" -gt 0 && -n "${missing_view_columns[0]}" ]]; then
  status="fail"
  reasons+=("memory_envelopes_view_missing_columns")
fi

if $json_mode; then
  jq -n \
    --arg status "${status}" \
    --argjson reasons "$(json_array "${reasons[@]}")" \
    --argjson missing_memory_item_columns "$(json_array "${missing_memory_item_columns[@]}")" \
    --argjson missing_memory_card_columns "$(json_array "${missing_memory_card_columns[@]}")" \
    --argjson missing_skill_card_columns "$(json_array "${missing_skill_card_columns[@]}")" \
    --argjson missing_task_node_columns "$(json_array "${missing_task_node_columns[@]}")" \
    --argjson missing_task_event_columns "$(json_array "${missing_task_event_columns[@]}")" \
    --argjson missing_memory_link_decision_columns "$(json_array "${missing_memory_link_decision_columns[@]}")" \
    --argjson missing_pending_link_proposal_columns "$(json_array "${missing_pending_link_proposal_columns[@]}")" \
    --argjson missing_artifact_ref_columns "$(json_array "${missing_artifact_ref_columns[@]}")" \
    --argjson missing_skill_evidence_bundle_columns "$(json_array "${missing_skill_evidence_bundle_columns[@]}")" \
    --argjson missing_memory_relation_edge_columns "$(json_array "${missing_memory_relation_edge_columns[@]}")" \
    --argjson missing_skill_trigger_match_columns "$(json_array "${missing_skill_trigger_match_columns[@]}")" \
    --argjson missing_skill_trial_run_columns "$(json_array "${missing_skill_trial_run_columns[@]}")" \
    --argjson missing_skill_eval_columns "$(json_array "${missing_skill_eval_columns[@]}")" \
    --argjson missing_skill_reuse_log_columns "$(json_array "${missing_skill_reuse_log_columns[@]}")" \
    --argjson missing_import_packet_columns "$(json_array "${missing_import_packet_columns[@]}")" \
    --argjson missing_shared_asset_columns "$(json_array "${missing_shared_asset_columns[@]}")" \
    --argjson missing_shared_asset_project_columns "$(json_array "${missing_shared_asset_project_columns[@]}")" \
    --argjson missing_memory_provenance_columns "$(json_array "${missing_memory_provenance_columns[@]}")" \
    --argjson missing_memory_raw_event_columns "$(json_array "${missing_memory_raw_event_columns[@]}")" \
    --argjson missing_memory_write_outbox_columns "$(json_array "${missing_memory_write_outbox_columns[@]}")" \
    --argjson missing_retrieval_trace_columns "$(json_array "${missing_retrieval_trace_columns[@]}")" \
    --argjson missing_restore_pack_columns "$(json_array "${missing_restore_pack_columns[@]}")" \
    --argjson missing_policy_rule_columns "$(json_array "${missing_policy_rule_columns[@]}")" \
    --argjson missing_quarantine_item_columns "$(json_array "${missing_quarantine_item_columns[@]}")" \
    --argjson missing_memory_edge_columns "$(json_array "${missing_memory_edge_columns[@]}")" \
    --argjson missing_memory_conflict_columns "$(json_array "${missing_memory_conflict_columns[@]}")" \
    --argjson missing_view_columns "$(json_array "${missing_view_columns[@]}")" \
    '{
      status: $status,
      reasons: $reasons,
      missing_memory_item_columns: $missing_memory_item_columns,
      missing_memory_card_columns: $missing_memory_card_columns,
      missing_skill_card_columns: $missing_skill_card_columns,
      missing_task_node_columns: $missing_task_node_columns,
      missing_task_event_columns: $missing_task_event_columns,
      missing_memory_link_decision_columns: $missing_memory_link_decision_columns,
      missing_pending_link_proposal_columns: $missing_pending_link_proposal_columns,
      missing_artifact_ref_columns: $missing_artifact_ref_columns,
      missing_skill_evidence_bundle_columns: $missing_skill_evidence_bundle_columns,
      missing_memory_relation_edge_columns: $missing_memory_relation_edge_columns,
      missing_skill_trigger_match_columns: $missing_skill_trigger_match_columns,
      missing_skill_trial_run_columns: $missing_skill_trial_run_columns,
      missing_skill_eval_columns: $missing_skill_eval_columns,
      missing_skill_reuse_log_columns: $missing_skill_reuse_log_columns,
      missing_import_packet_columns: $missing_import_packet_columns,
      missing_shared_asset_columns: $missing_shared_asset_columns,
      missing_shared_asset_project_columns: $missing_shared_asset_project_columns,
      missing_memory_provenance_columns: $missing_memory_provenance_columns,
      missing_memory_raw_event_columns: $missing_memory_raw_event_columns,
      missing_memory_write_outbox_columns: $missing_memory_write_outbox_columns,
      missing_retrieval_trace_columns: $missing_retrieval_trace_columns,
      missing_restore_pack_columns: $missing_restore_pack_columns,
      missing_policy_rule_columns: $missing_policy_rule_columns,
      missing_quarantine_item_columns: $missing_quarantine_item_columns,
      missing_memory_edge_columns: $missing_memory_edge_columns,
      missing_memory_conflict_columns: $missing_memory_conflict_columns,
      missing_view_columns: $missing_view_columns
    }'
else
  echo "typed memory envelope guard"
  echo "status: ${status}"
  if [[ "${#reasons[@]}" -gt 0 ]]; then
    printf 'reasons:\n'
    printf '  %s\n' "${reasons[@]}"
  fi
  if [[ "${#missing_memory_item_columns[@]}" -gt 0 && -n "${missing_memory_item_columns[0]}" ]]; then
    printf 'missing memory_items columns:\n'
    printf '  %s\n' "${missing_memory_item_columns[@]}"
  fi
  if [[ "${#missing_memory_card_columns[@]}" -gt 0 && -n "${missing_memory_card_columns[0]}" ]]; then
    printf 'missing memory_cards columns:\n'
    printf '  %s\n' "${missing_memory_card_columns[@]}"
  fi
  if [[ "${#missing_skill_card_columns[@]}" -gt 0 && -n "${missing_skill_card_columns[0]}" ]]; then
    printf 'missing skill_cards columns:\n'
    printf '  %s\n' "${missing_skill_card_columns[@]}"
  fi
  if [[ "${#missing_task_node_columns[@]}" -gt 0 && -n "${missing_task_node_columns[0]}" ]]; then
    printf 'missing task_nodes columns:\n'
    printf '  %s\n' "${missing_task_node_columns[@]}"
  fi
  if [[ "${#missing_task_event_columns[@]}" -gt 0 && -n "${missing_task_event_columns[0]}" ]]; then
    printf 'missing task_events columns:\n'
    printf '  %s\n' "${missing_task_event_columns[@]}"
  fi
  if [[ "${#missing_memory_link_decision_columns[@]}" -gt 0 && -n "${missing_memory_link_decision_columns[0]}" ]]; then
    printf 'missing memory_link_decisions columns:\n'
    printf '  %s\n' "${missing_memory_link_decision_columns[@]}"
  fi
  if [[ "${#missing_pending_link_proposal_columns[@]}" -gt 0 && -n "${missing_pending_link_proposal_columns[0]}" ]]; then
    printf 'missing pending_link_proposals columns:\n'
    printf '  %s\n' "${missing_pending_link_proposal_columns[@]}"
  fi
  if [[ "${#missing_artifact_ref_columns[@]}" -gt 0 && -n "${missing_artifact_ref_columns[0]}" ]]; then
    printf 'missing artifact_refs columns:\n'
    printf '  %s\n' "${missing_artifact_ref_columns[@]}"
  fi
  if [[ "${#missing_skill_evidence_bundle_columns[@]}" -gt 0 && -n "${missing_skill_evidence_bundle_columns[0]}" ]]; then
    printf 'missing skill_evidence_bundles columns:\n'
    printf '  %s\n' "${missing_skill_evidence_bundle_columns[@]}"
  fi
  if [[ "${#missing_memory_relation_edge_columns[@]}" -gt 0 && -n "${missing_memory_relation_edge_columns[0]}" ]]; then
    printf 'missing memory_relation_edges columns:\n'
    printf '  %s\n' "${missing_memory_relation_edge_columns[@]}"
  fi
  if [[ "${#missing_skill_trigger_match_columns[@]}" -gt 0 && -n "${missing_skill_trigger_match_columns[0]}" ]]; then
    printf 'missing skill_trigger_matches columns:\n'
    printf '  %s\n' "${missing_skill_trigger_match_columns[@]}"
  fi
  if [[ "${#missing_skill_trial_run_columns[@]}" -gt 0 && -n "${missing_skill_trial_run_columns[0]}" ]]; then
    printf 'missing skill_trial_runs columns:\n'
    printf '  %s\n' "${missing_skill_trial_run_columns[@]}"
  fi
  if [[ "${#missing_skill_eval_columns[@]}" -gt 0 && -n "${missing_skill_eval_columns[0]}" ]]; then
    printf 'missing skill_evals columns:\n'
    printf '  %s\n' "${missing_skill_eval_columns[@]}"
  fi
  if [[ "${#missing_skill_reuse_log_columns[@]}" -gt 0 && -n "${missing_skill_reuse_log_columns[0]}" ]]; then
    printf 'missing skill_reuse_logs columns:\n'
    printf '  %s\n' "${missing_skill_reuse_log_columns[@]}"
  fi
  if [[ "${#missing_memory_provenance_columns[@]}" -gt 0 && -n "${missing_memory_provenance_columns[0]}" ]]; then
    printf 'missing memory_provenance columns:\n'
    printf '  %s\n' "${missing_memory_provenance_columns[@]}"
  fi
  if [[ "${#missing_memory_raw_event_columns[@]}" -gt 0 && -n "${missing_memory_raw_event_columns[0]}" ]]; then
    printf 'missing memory_raw_events columns:\n'
    printf '  %s\n' "${missing_memory_raw_event_columns[@]}"
  fi
  if [[ "${#missing_memory_write_outbox_columns[@]}" -gt 0 && -n "${missing_memory_write_outbox_columns[0]}" ]]; then
    printf 'missing memory_write_outbox columns:\n'
    printf '  %s\n' "${missing_memory_write_outbox_columns[@]}"
  fi
  if [[ "${#missing_retrieval_trace_columns[@]}" -gt 0 && -n "${missing_retrieval_trace_columns[0]}" ]]; then
    printf 'missing retrieval_traces columns:\n'
    printf '  %s\n' "${missing_retrieval_trace_columns[@]}"
  fi
  if [[ "${#missing_restore_pack_columns[@]}" -gt 0 && -n "${missing_restore_pack_columns[0]}" ]]; then
    printf 'missing restore_packs columns:\n'
    printf '  %s\n' "${missing_restore_pack_columns[@]}"
  fi
  if [[ "${#missing_policy_rule_columns[@]}" -gt 0 && -n "${missing_policy_rule_columns[0]}" ]]; then
    printf 'missing policy_rules columns:\n'
    printf '  %s\n' "${missing_policy_rule_columns[@]}"
  fi
  if [[ "${#missing_quarantine_item_columns[@]}" -gt 0 && -n "${missing_quarantine_item_columns[0]}" ]]; then
    printf 'missing quarantine_items columns:\n'
    printf '  %s\n' "${missing_quarantine_item_columns[@]}"
  fi
  if [[ "${#missing_memory_edge_columns[@]}" -gt 0 && -n "${missing_memory_edge_columns[0]}" ]]; then
    printf 'missing memory_edges columns:\n'
    printf '  %s\n' "${missing_memory_edge_columns[@]}"
  fi
  if [[ "${#missing_memory_conflict_columns[@]}" -gt 0 && -n "${missing_memory_conflict_columns[0]}" ]]; then
    printf 'missing memory_conflicts columns:\n'
    printf '  %s\n' "${missing_memory_conflict_columns[@]}"
  fi
  if [[ "${#missing_view_columns[@]}" -gt 0 && -n "${missing_view_columns[0]}" ]]; then
    printf 'missing memory_envelopes columns:\n'
    printf '  %s\n' "${missing_view_columns[@]}"
  fi
fi

if [[ "${status}" != "pass" ]]; then
  exit 1
fi
