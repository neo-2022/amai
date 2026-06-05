#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_forgetting_consolidation] $*"
}

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

policy_simulate_projection_snapshot() {
  psql "${dsn}" \
    -v ON_ERROR_STOP=1 \
    -v project_code="${project_code}" \
    -v namespace_code="${namespace_code}" \
    -v suffix="${suffix}" \
    -tA <<'SQL'
WITH watched_scopes AS (
    SELECT p.project_id, n.namespace_id
    FROM ami.projects p
    JOIN ami.namespaces n ON n.project_id = p.project_id
    WHERE (p.code = :'project_code'
           AND n.code IN (:'namespace_code', 'proof-policy-simulate-foreign-ns-' || :'suffix'))
       OR (p.code = 'proof-policy-simulate-foreign-project-' || :'suffix'
           AND n.code = :'namespace_code')
),
memory_items_rows AS (
    SELECT mi.memory_item_id::text AS row_id, to_jsonb(mi)::text AS row_payload
    FROM ami.memory_items mi
    JOIN watched_scopes s ON s.project_id = mi.project_id
                         AND s.namespace_id = mi.namespace_id
),
forgetting_audit_rows AS (
    SELECT fal.audit_id::text AS row_id, to_jsonb(fal)::text AS row_payload
    FROM ami.forgetting_audit_log fal
    WHERE (fal.project_code = :'project_code'
           AND fal.namespace_code IN (:'namespace_code', 'proof-policy-simulate-foreign-ns-' || :'suffix'))
       OR (fal.project_code = 'proof-policy-simulate-foreign-project-' || :'suffix'
           AND fal.namespace_code = :'namespace_code')
),
task_node_rows AS (
    SELECT tn.task_node_id::text AS row_id, to_jsonb(tn)::text AS row_payload
    FROM ami.task_nodes tn
    JOIN watched_scopes s ON s.project_id = tn.project_id
                         AND s.namespace_id = tn.namespace_id
),
task_event_rows AS (
    SELECT te.task_event_id::text AS row_id, to_jsonb(te)::text AS row_payload
    FROM ami.task_events te
    JOIN watched_scopes s ON s.project_id = te.project_id
                         AND s.namespace_id = te.namespace_id
),
memory_link_decision_rows AS (
    SELECT mld.memory_link_decision_id::text AS row_id, to_jsonb(mld)::text AS row_payload
    FROM ami.memory_link_decisions mld
    JOIN watched_scopes s ON s.project_id = mld.project_id
                         AND s.namespace_id = mld.namespace_id
),
pending_link_proposal_rows AS (
    SELECT plp.pending_link_proposal_id::text AS row_id, to_jsonb(plp)::text AS row_payload
    FROM ami.pending_link_proposals plp
    JOIN watched_scopes s ON s.project_id = plp.project_id
                         AND s.namespace_id = plp.namespace_id
),
execctl_ledger_rows AS (
    SELECT etle.ledger_entry_id::text AS row_id, to_jsonb(etle)::text AS row_payload
    FROM ami.execctl_task_ledger_entries etle
    JOIN watched_scopes s ON s.project_id = etle.project_id
                         AND s.namespace_id = etle.namespace_id
),
execctl_lease_rows AS (
    SELECT etl.lease_id::text AS row_id, to_jsonb(etl)::text AS row_payload
    FROM ami.execctl_task_leases etl
    JOIN watched_scopes s ON s.project_id = etl.project_id
                         AND s.namespace_id = etl.namespace_id
),
snapshots AS (
    SELECT 'memory_items' AS lane, COUNT(*) AS row_count,
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex') AS row_hash
    FROM memory_items_rows
    UNION ALL
    SELECT 'forgetting_audit_log', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM forgetting_audit_rows
    UNION ALL
    SELECT 'task_nodes', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM task_node_rows
    UNION ALL
    SELECT 'task_events', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM task_event_rows
    UNION ALL
    SELECT 'memory_link_decisions', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM memory_link_decision_rows
    UNION ALL
    SELECT 'pending_link_proposals', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM pending_link_proposal_rows
    UNION ALL
    SELECT 'execctl_task_ledger_entries', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM execctl_ledger_rows
    UNION ALL
    SELECT 'execctl_task_leases', COUNT(*),
           encode(digest(COALESCE(string_agg(row_payload, E'\n' ORDER BY row_id), ''), 'sha256'), 'hex')
    FROM execctl_lease_rows
)
SELECT jsonb_object_agg(
    lane,
    jsonb_build_object('row_count', row_count, 'row_hash', row_hash)
    ORDER BY lane
)::text
FROM snapshots;
SQL
}

create_policy_simulate_durable_sentinels() {
  local policy_simulate_agent_scope="proof-policy-simulate-${suffix}"

  psql "${dsn}" \
    -v ON_ERROR_STOP=1 \
    -v project_code="${project_code}" \
    -v namespace_code="${namespace_code}" \
    -v suffix="${suffix}" \
    -v agent_scope="${policy_simulate_agent_scope}" \
    -tA <<'SQL'
WITH scope AS (
    SELECT w.workspace_id, p.project_id, n.namespace_id
    FROM ami.projects p
    JOIN ami.workspaces w ON w.workspace_id = p.workspace_id
    JOIN ami.namespaces n ON n.project_id = p.project_id
    WHERE p.code = :'project_code'
      AND n.code = :'namespace_code'
),
task_node AS (
    INSERT INTO ami.task_nodes(
        workspace_id,
        project_id,
        namespace_id,
        task_key,
        task_role,
        headline,
        summary,
        next_step,
        execution_state,
        lifecycle_state,
        confidence,
        current_score,
        source_event_ids,
        artifact_refs,
        evidence_span,
        candidate_class,
        derivation_kind,
        source_kind,
        hot_path_write_eligible,
        background_consolidation_recommended,
        status_payload,
        metadata,
        opened_at_epoch_ms
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        'policy-simulate-sentinel-' || :'suffix',
        'workline',
        'Policy simulate non-mutation sentinel',
        'Proof fixture that must survive advisory policy simulation unchanged.',
        'Remain unchanged before and after memory policy-simulate.',
        'active',
        'hot',
        1.0,
        1.0,
        jsonb_build_array('proof-policy-simulate-task-node-' || :'suffix'),
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/task-node'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'commitment',
        'operator_write',
        'proof_forgetting_consolidation',
        FALSE,
        FALSE,
        jsonb_build_object('proof_lane', 'policy_simulate_projection_non_mutation'),
        jsonb_build_object('sentinel', TRUE, 'agent_scope', :'agent_scope'),
        1000
    FROM scope
    RETURNING task_node_id, workspace_id, project_id, namespace_id
),
task_event AS (
    INSERT INTO ami.task_events(
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        source_event_id,
        event_kind,
        next_execution_state,
        next_lifecycle_state,
        source_kind,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind,
        schema_version,
        event_payload,
        recorded_at_epoch_ms
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        'proof-policy-simulate-task-event-' || :'suffix',
        'created',
        'active',
        'hot',
        'proof_forgetting_consolidation',
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/task-event'),
        jsonb_build_array('message:proof-policy-simulate:' || :'suffix'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'operator_write',
        'task-event-envelope-v1',
        jsonb_build_object('sentinel', TRUE, 'append_only_guard_required', TRUE),
        1001
    FROM task_node
    RETURNING task_event_id
),
memory_link_decision AS (
    INSERT INTO ami.memory_link_decisions(
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        decision_outcome,
        legality_passed,
        scope_filter_passed,
        evidence_sufficient,
        classifier_label,
        classifier_score,
        decision_reason,
        decision_payload,
        source_event_ids,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind,
        schema_version,
        recorded_at_epoch_ms
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        'abstain',
        FALSE,
        FALSE,
        FALSE,
        'policy_simulate_sentinel',
        0.0,
        'Sentinel link decision must remain unchanged by advisory policy simulation.',
        jsonb_build_object('sentinel', TRUE, 'advisory_only', TRUE),
        jsonb_build_array('proof-policy-simulate-link-decision-' || :'suffix'),
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/link-decision'),
        jsonb_build_array('message:proof-policy-simulate:' || :'suffix'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'operator_write',
        'memory-link-decision-envelope-v1',
        1002
    FROM task_node
    RETURNING memory_link_decision_id
),
pending_link_proposal AS (
    INSERT INTO ami.pending_link_proposals(
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        proposal_state,
        proposal_reason,
        evidence_request,
        evidence_payload,
        classifier_score,
        ttl_epoch_ms,
        source_event_ids,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind,
        schema_version
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        'pending',
        'Sentinel pending-link proposal must remain unchanged by advisory policy simulation.',
        'Do not resolve from policy-simulate.',
        jsonb_build_object('sentinel', TRUE, 'advisory_only', TRUE),
        0.0,
        9999999999999,
        jsonb_build_array('proof-policy-simulate-pending-link-' || :'suffix'),
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/pending-link'),
        jsonb_build_array('message:proof-policy-simulate:' || :'suffix'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'operator_write',
        'pending-link-proposal-envelope-v1'
    FROM task_node
    RETURNING pending_link_proposal_id
),
ledger_entry AS (
    INSERT INTO ami.execctl_task_ledger_entries(
        project_id,
        namespace_id,
        agent_scope,
        session_id,
        thread_id,
        source_event_id,
        event_kind,
        source_kind,
        headline,
        next_step,
        summary,
        active_files,
        open_questions,
        materialized_notes,
        pending_return_queue,
        local_path,
        recorded_at_epoch_ms
    )
    SELECT
        project_id,
        namespace_id,
        :'agent_scope',
        'proof-session-' || :'suffix',
        'proof-thread-' || :'suffix',
        'proof-policy-simulate-ledger-' || :'suffix',
        'continuity_handoff',
        'proof_forgetting_consolidation',
        'Policy simulate ExecCtl sentinel',
        'Remain unchanged before and after advisory policy simulation.',
        'Proof fixture for ExecCtl non-mutation.',
        jsonb_build_array('scripts/proof_forgetting_consolidation.sh'),
        '[]'::jsonb,
        jsonb_build_array(jsonb_build_object('sentinel', TRUE, 'suffix', :'suffix')),
        jsonb_build_array(jsonb_build_object('headline', 'pending sentinel', 'next_step', 'remain unchanged')),
        '/home/art/agent-memory-index/scripts/proof_forgetting_consolidation.sh',
        1003
    FROM scope
    RETURNING ledger_entry_id
),
lease_entry AS (
    INSERT INTO ami.execctl_task_leases(
        project_id,
        namespace_id,
        agent_scope,
        owner_session_id,
        owner_thread_id,
        source_event_id,
        source_kind,
        lease_state,
        headline,
        next_step,
        local_path,
        acquired_at_epoch_ms,
        heartbeat_at_epoch_ms,
        expires_at_epoch_ms
    )
    SELECT
        project_id,
        namespace_id,
        :'agent_scope',
        'proof-session-' || :'suffix',
        'proof-thread-' || :'suffix',
        'proof-policy-simulate-lease-' || :'suffix',
        'proof_forgetting_consolidation',
        'active',
        'Policy simulate ExecCtl lease sentinel',
        'Remain unchanged before and after advisory policy simulation.',
        '/home/art/agent-memory-index/scripts/proof_forgetting_consolidation.sh',
        1004,
        1005,
        9999999999999
    FROM scope
    RETURNING lease_id
)
SELECT
    task_node.task_node_id::text || '|' ||
    task_event.task_event_id::text || '|' ||
    memory_link_decision.memory_link_decision_id::text || '|' ||
    pending_link_proposal.pending_link_proposal_id::text || '|' ||
    ledger_entry.ledger_entry_id::text || '|' ||
    lease_entry.lease_id::text
FROM task_node, task_event, memory_link_decision, pending_link_proposal, ledger_entry, lease_entry;
SQL
}

create_policy_simulate_scope_isolation_sentinels() {
  psql "${dsn}" \
    -v ON_ERROR_STOP=1 \
    -v project_code="${project_code}" \
    -v namespace_code="${namespace_code}" \
    -v suffix="${suffix}" \
    -tA <<'SQL'
WITH workspace_scope AS (
    SELECT workspace_id
    FROM ami.workspaces
    WHERE code = 'default'
),
foreign_project AS (
    INSERT INTO ami.projects(
        workspace_id,
        code,
        display_name,
        repo_root,
        visibility_scope
    )
    SELECT
        workspace_id,
        'proof-policy-simulate-foreign-project-' || :'suffix',
        'Proof policy simulate foreign project ' || :'suffix',
        '/tmp/amai-proof-policy-simulate-foreign-project-' || :'suffix',
        'project_shared'
    FROM workspace_scope
    ON CONFLICT (code) DO UPDATE SET
        display_name = EXCLUDED.display_name,
        repo_root = EXCLUDED.repo_root,
        updated_at = now()
    RETURNING workspace_id, project_id
),
same_project_foreign_namespace AS (
    INSERT INTO ami.namespaces(
        project_id,
        code,
        display_name,
        retrieval_mode
    )
    SELECT
        p.project_id,
        'proof-policy-simulate-foreign-ns-' || :'suffix',
        'Proof policy simulate foreign namespace ' || :'suffix',
        'local_strict'
    FROM ami.projects p
    WHERE p.code = :'project_code'
    ON CONFLICT (project_id, code) DO UPDATE SET
        display_name = EXCLUDED.display_name,
        retrieval_mode = EXCLUDED.retrieval_mode,
        updated_at = now()
    RETURNING project_id, namespace_id
),
foreign_project_same_namespace AS (
    INSERT INTO ami.namespaces(
        project_id,
        code,
        display_name,
        retrieval_mode
    )
    SELECT
        fp.project_id,
        :'namespace_code',
        'Proof policy simulate same namespace code in foreign project ' || :'suffix',
        'local_strict'
    FROM foreign_project fp
    ON CONFLICT (project_id, code) DO UPDATE SET
        display_name = EXCLUDED.display_name,
        retrieval_mode = EXCLUDED.retrieval_mode,
        updated_at = now()
    RETURNING project_id, namespace_id
),
watched_scope AS (
    SELECT
        'same_project_foreign_namespace' AS scope_label,
        p.workspace_id,
        p.project_id,
        spfn.namespace_id
    FROM same_project_foreign_namespace spfn
    JOIN ami.projects p ON p.project_id = spfn.project_id
    UNION ALL
    SELECT
        'foreign_project_same_namespace' AS scope_label,
        fp.workspace_id,
        fp.project_id,
        fpsn.namespace_id
    FROM foreign_project_same_namespace fpsn
    JOIN foreign_project fp ON fp.project_id = fpsn.project_id
),
task_nodes AS (
    INSERT INTO ami.task_nodes(
        workspace_id,
        project_id,
        namespace_id,
        task_key,
        task_role,
        headline,
        summary,
        next_step,
        execution_state,
        lifecycle_state,
        confidence,
        current_score,
        source_event_ids,
        artifact_refs,
        evidence_span,
        candidate_class,
        derivation_kind,
        source_kind,
        hot_path_write_eligible,
        background_consolidation_recommended,
        status_payload,
        metadata,
        opened_at_epoch_ms
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        'policy-simulate-scope-sentinel-' || scope_label || '-' || :'suffix',
        'workline',
        'Policy simulate scope isolation sentinel',
        'Foreign-scope proof fixture that must remain unchanged.',
        'Remain unchanged when policy-simulate targets the primary proof namespace.',
        'active',
        'hot',
        1.0,
        1.0,
        jsonb_build_array('proof-policy-simulate-scope-task-node-' || scope_label || '-' || :'suffix'),
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/' || scope_label || '/task-node'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'scope_label', scope_label, 'suffix', :'suffix'),
        'commitment',
        'operator_write',
        'proof_forgetting_consolidation',
        FALSE,
        FALSE,
        jsonb_build_object('proof_lane', 'policy_simulate_scope_isolation_non_mutation'),
        jsonb_build_object('sentinel', TRUE, 'scope_label', scope_label),
        2000
    FROM watched_scope
    RETURNING task_node_id, workspace_id, project_id, namespace_id, task_key
),
task_events AS (
    INSERT INTO ami.task_events(
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        source_event_id,
        event_kind,
        next_execution_state,
        next_lifecycle_state,
        source_kind,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind,
        schema_version,
        event_payload,
        recorded_at_epoch_ms
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        'proof-policy-simulate-scope-task-event-' || task_key,
        'created',
        'active',
        'hot',
        'proof_forgetting_consolidation',
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/scope-task-event'),
        jsonb_build_array('message:proof-policy-simulate:' || :'suffix'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'operator_write',
        'task-event-envelope-v1',
        jsonb_build_object('sentinel', TRUE, 'scope_isolation', TRUE),
        2001
    FROM task_nodes
    RETURNING task_event_id
),
memory_link_decisions AS (
    INSERT INTO ami.memory_link_decisions(
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        decision_outcome,
        legality_passed,
        scope_filter_passed,
        evidence_sufficient,
        classifier_label,
        classifier_score,
        decision_reason,
        decision_payload,
        source_event_ids,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind,
        schema_version,
        recorded_at_epoch_ms
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        'abstain',
        FALSE,
        FALSE,
        FALSE,
        'policy_simulate_scope_sentinel',
        0.0,
        'Foreign-scope link decision must remain unchanged.',
        jsonb_build_object('sentinel', TRUE, 'scope_isolation', TRUE),
        jsonb_build_array('proof-policy-simulate-scope-link-decision-' || task_key),
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/scope-link-decision'),
        jsonb_build_array('message:proof-policy-simulate:' || :'suffix'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'operator_write',
        'memory-link-decision-envelope-v1',
        2002
    FROM task_nodes
    RETURNING memory_link_decision_id
),
pending_link_proposals AS (
    INSERT INTO ami.pending_link_proposals(
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        proposal_state,
        proposal_reason,
        evidence_request,
        evidence_payload,
        classifier_score,
        ttl_epoch_ms,
        source_event_ids,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind,
        schema_version
    )
    SELECT
        workspace_id,
        project_id,
        namespace_id,
        task_node_id,
        'pending',
        'Foreign-scope pending-link proposal must remain unchanged.',
        'Do not resolve from policy-simulate.',
        jsonb_build_object('sentinel', TRUE, 'scope_isolation', TRUE),
        0.0,
        9999999999999,
        jsonb_build_array('proof-policy-simulate-scope-pending-link-' || task_key),
        jsonb_build_array('artifact://proof/forgetting/policy-simulate/' || :'suffix' || '/scope-pending-link'),
        jsonb_build_array('message:proof-policy-simulate:' || :'suffix'),
        jsonb_build_object('source', 'proof_forgetting_consolidation', 'suffix', :'suffix'),
        'operator_write',
        'pending-link-proposal-envelope-v1'
    FROM task_nodes
    RETURNING pending_link_proposal_id
),
ledger_entries AS (
    INSERT INTO ami.execctl_task_ledger_entries(
        project_id,
        namespace_id,
        agent_scope,
        session_id,
        thread_id,
        source_event_id,
        event_kind,
        source_kind,
        headline,
        next_step,
        summary,
        active_files,
        open_questions,
        materialized_notes,
        pending_return_queue,
        local_path,
        recorded_at_epoch_ms
    )
    SELECT
        project_id,
        namespace_id,
        'proof-policy-simulate-scope-' || task_key,
        'proof-session-' || :'suffix',
        'proof-thread-' || :'suffix',
        'proof-policy-simulate-scope-ledger-' || task_key,
        'continuity_handoff',
        'proof_forgetting_consolidation',
        'Policy simulate scope ExecCtl sentinel',
        'Remain unchanged by policy simulation in another scope.',
        'Foreign-scope ExecCtl proof fixture.',
        jsonb_build_array('scripts/proof_forgetting_consolidation.sh'),
        '[]'::jsonb,
        jsonb_build_array(jsonb_build_object('sentinel', TRUE, 'suffix', :'suffix')),
        jsonb_build_array(jsonb_build_object('headline', 'scope pending sentinel', 'next_step', 'remain unchanged')),
        '/home/art/agent-memory-index/scripts/proof_forgetting_consolidation.sh',
        2003
    FROM task_nodes
    RETURNING ledger_entry_id
),
lease_entries AS (
    INSERT INTO ami.execctl_task_leases(
        project_id,
        namespace_id,
        agent_scope,
        owner_session_id,
        owner_thread_id,
        source_event_id,
        source_kind,
        lease_state,
        headline,
        next_step,
        local_path,
        acquired_at_epoch_ms,
        heartbeat_at_epoch_ms,
        expires_at_epoch_ms
    )
    SELECT
        project_id,
        namespace_id,
        'proof-policy-simulate-scope-' || task_key,
        'proof-session-' || :'suffix',
        'proof-thread-' || :'suffix',
        'proof-policy-simulate-scope-lease-' || task_key,
        'proof_forgetting_consolidation',
        'active',
        'Policy simulate scope ExecCtl lease sentinel',
        'Remain unchanged by policy simulation in another scope.',
        '/home/art/agent-memory-index/scripts/proof_forgetting_consolidation.sh',
        2004,
        2005,
        9999999999999
    FROM task_nodes
    RETURNING lease_id
)
SELECT jsonb_build_object(
    'task_nodes', (SELECT COUNT(*) FROM task_nodes),
    'task_events', (SELECT COUNT(*) FROM task_events),
    'memory_link_decisions', (SELECT COUNT(*) FROM memory_link_decisions),
    'pending_link_proposals', (SELECT COUNT(*) FROM pending_link_proposals),
    'execctl_task_ledger_entries', (SELECT COUNT(*) FROM ledger_entries),
    'execctl_task_leases', (SELECT COUNT(*) FROM lease_entries)
)::text;
SQL
}

assert_task_events_append_only_guard() {
  local task_event_id="$1"
  local update_error
  local delete_error

  update_error="$(psql "${dsn}" -v ON_ERROR_STOP=1 -c "UPDATE ami.task_events SET event_kind='continued' WHERE task_event_id='${task_event_id}'" 2>&1 || true)"
  [[ "${update_error}" == *"ami.task_events is append-only"* ]] || fail "task_events update was not rejected by append-only guard"

  delete_error="$(psql "${dsn}" -v ON_ERROR_STOP=1 -c "DELETE FROM ami.task_events WHERE task_event_id='${task_event_id}'" 2>&1 || true)"
  [[ "${delete_error}" == *"ami.task_events is append-only"* ]] || fail "task_events delete was not rejected by append-only guard"
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-forgetting-${suffix}"
dsn="$(grep '^AMI_POSTGRES_DSN=' "$(dirname "$0")/../.env" | cut -d= -f2-)"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

# ────────────────────────────────────────────────────────────────────────
# 1. Create test items with diverse lifecycle characteristics
# ────────────────────────────────────────────────────────────────────────
step "create ephemeral item with expired TTL"
ephemeral_expired="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Ephemeral expired fact" \
  --summary "This should be pruned" \
  --derivation-kind "summary" \
  --retention-class "ephemeral" \
  --ttl-epoch-ms 1000 \
  --utility-score 0.01 \
  --freshness-score 0.01 \
  --source-event-id "proof-forgetting-ephemeral-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/ephemeral" \
  --json)"
ephemeral_expired_id="$(printf '%s' "${ephemeral_expired}" | jq -r '.memory_item_id')"
step "ephemeral expired item: ${ephemeral_expired_id}"

step "create durable raw_capture item (must NOT be pruned)"
durable_raw="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Immutable raw evidence" \
  --summary "Raw evidence that must survive any pruning" \
  --derivation-kind "raw_capture" \
  --retention-class "durable" \
  --utility-score 0.5 \
  --freshness-score 0.5 \
  --source-event-id "proof-forgetting-durable-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/durable" \
  --json)"
durable_raw_id="$(printf '%s' "${durable_raw}" | jq -r '.memory_item_id')"
step "durable raw item: ${durable_raw_id}"

step "create standard summary item with low freshness (archivable)"
standard_stale="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Stale standard summary" \
  --summary "Should be archived to cold tier" \
  --derivation-kind "summary" \
  --retention-class "standard" \
  --utility-score 0.02 \
  --freshness-score 0.01 \
  --source-event-id "proof-forgetting-stale-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/stale" \
  --json)"
standard_stale_id="$(printf '%s' "${standard_stale}" | jq -r '.memory_item_id')"
step "standard stale item: ${standard_stale_id}"

step "create operator_write item with low freshness (must NOT be pruned or archived)"
operator_write="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "policy" \
  --title "Operator immutable policy" \
  --summary "Operator-written policy must survive all forgetting" \
  --derivation-kind "operator_write" \
  --retention-class "standard" \
  --utility-score 0.01 \
  --freshness-score 0.001 \
  --json)"
operator_write_id="$(printf '%s' "${operator_write}" | jq -r '.memory_item_id')"
step "operator_write item: ${operator_write_id}"

step "create legal_hold item with low scores (must NOT be pruned)"
legal_hold="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Legal hold evidence" \
  --summary "Under legal hold - never touch" \
  --derivation-kind "summary" \
  --retention-class "legal_hold" \
  --utility-score 0.001 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-legal-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/legal" \
  --json)"
legal_hold_id="$(printf '%s' "${legal_hold}" | jq -r '.memory_item_id')"
step "legal_hold item: ${legal_hold_id}"

step "create revalidation-target item (standard, low freshness, truth_state=current)"
reval_target="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Stale knowledge needing revalidation" \
  --summary "This fact has very low freshness and needs review" \
  --derivation-kind "extract" \
  --retention-class "standard" \
  --utility-score 0.3 \
  --freshness-score 0.02 \
  --source-event-id "proof-forgetting-reval-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/reval" \
  --json)"
reval_target_id="$(printf '%s' "${reval_target}" | jq -r '.memory_item_id')"
step "revalidation target item: ${reval_target_id}"

step "create ephemeral retain_forever item with low utility (must NOT be pruned by low-utility path)"
ephemeral_retain="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Ephemeral retain forever" \
  --summary "Protected from low-utility prune by retain_forever" \
  --derivation-kind "summary" \
  --retention-class "ephemeral" \
  --utility-score 0.001 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-ephemeral-retain-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/ephemeral-retain" \
  --json)"
ephemeral_retain_id="$(printf '%s' "${ephemeral_retain}" | jq -r '.memory_item_id')"
step "ephemeral retain_forever item: ${ephemeral_retain_id}"

step "create verified_write_back item with low freshness (must NOT be revalidated/pruned/compacted)"
verified_writeback="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --identity-key "proof-verified-writeback-${suffix}" \
  --title "Verified writeback evidence" \
  --summary "Verified writeback must survive automated forgetting" \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "proof-forgetting-writeback-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/writeback/${suffix}" \
  --message-ref "message:proof-forgetting-writeback:${suffix}" \
  --evidence-span-json '{"source":"proof","kind":"raw_log","range":"1-2"}' \
  --derivation-kind verified_write_back \
  --utility-score 0.01 \
  --freshness-score 0.001 \
  --observed-at-epoch-ms 4000 \
  --recorded-at-epoch-ms 4001 \
  --valid-from-epoch-ms 4000 \
  --last-verified-at-epoch-ms 4002 \
  --metadata-json '{"writeback_evidence":{"escalated":true,"verified":true,"confirmed_via":"raw_evidence"}}' \
  --json)"
verified_writeback_id="$(printf '%s' "${verified_writeback}" | jq -r '.memory_item_id')"
step "verified_write_back item: ${verified_writeback_id}"

step "create retain_forever duplicate pair with low freshness (must NOT be revalidated/archived/compacted)"
retain_forever_a="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --identity-key "proof-retain-forever-${suffix}" \
  --title "Retain forever duplicate" \
  --summary "Retain forever duplicate A" \
  --derivation-kind "summary" \
  --retention-class "standard" \
  --truth-state current \
  --utility-score 0.02 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-retain-a-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/retain/a" \
  --json)"
retain_forever_a_id="$(printf '%s' "${retain_forever_a}" | jq -r '.memory_item_id')"

retain_forever_b="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --identity-key "proof-retain-forever-b-${suffix}" \
  --title "Retain forever duplicate B seed" \
  --summary "Retain forever duplicate B" \
  --derivation-kind "summary" \
  --retention-class "standard" \
  --truth-state current \
  --utility-score 0.01 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-retain-b-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/retain/b" \
  --json)"
retain_forever_b_id="$(printf '%s' "${retain_forever_b}" | jq -r '.memory_item_id')"
step "retain_forever items: ${retain_forever_a_id} ${retain_forever_b_id}"

# ────────────────────────────────────────────────────────────────────────
# 1b. Simulate verified fact for revalidation target
# ────────────────────────────────────────────────────────────────────────
step "set truth_state=current on revalidation target (simulate verified fact)"
psql "${dsn}" -qc "UPDATE ami.memory_items SET truth_state='current' WHERE memory_item_id='${reval_target_id}'"

step "set decay_policy=retain_forever on duplicate pair"
psql "${dsn}" -qc "UPDATE ami.memory_items SET decay_policy='retain_forever' WHERE memory_item_id IN ('${retain_forever_a_id}', '${retain_forever_b_id}')"

step "set decay_policy=retain_forever on ephemeral protected item"
psql "${dsn}" -qc "UPDATE ami.memory_items SET decay_policy='retain_forever' WHERE memory_item_id='${ephemeral_retain_id}'"

step "reshape retain_forever pair into duplicate identity/title fixture for dedup hostile check"
psql "${dsn}" -qc "
  UPDATE ami.memory_items
  SET identity_key='proof-retain-forever-${suffix}',
      title='Retain forever duplicate'
  WHERE memory_item_id IN ('${retain_forever_a_id}', '${retain_forever_b_id}')
"

# ────────────────────────────────────────────────────────────────────────
# 2. Touch access and verify counter
# ────────────────────────────────────────────────────────────────────────
step "touch access on durable raw item"
cargo run --quiet -- memory touch-access --memory-item-id "${durable_raw_id}"

step "verify access_count incremented"
access_count="$(psql "${dsn}" -tA -c "SELECT access_count FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${access_count}" -ge 1 ]] || fail "access_count was not incremented: ${access_count}"
step "access_count = ${access_count} (pass)"

step "verify last_accessed_at is set"
last_accessed="$(psql "${dsn}" -tA -c "SELECT last_accessed_at IS NOT NULL FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${last_accessed}" == "t" ]] || fail "last_accessed_at not set after touch"
step "last_accessed_at set (pass)"

# ────────────────────────────────────────────────────────────────────────
# 3. Revalidation (BEFORE consolidation so items are still active)
# ────────────────────────────────────────────────────────────────────────
step "run revalidate on items with freshness < 0.05"
revalidate_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind revalidation_job \
  --freshness-threshold 0.05)"
revalidate_job_kind="$(printf '%s' "${revalidate_output}" | jq -r '.job_kind')"
[[ "${revalidate_job_kind}" == "revalidation_job" ]] || fail "unexpected revalidation job kind: ${revalidate_job_kind}"
revalidated_count="$(printf '%s' "${revalidate_output}" | jq '.action_count')"
step "revalidated ${revalidated_count} items"

step "verify revalidation target is now pending_review"
reval_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${reval_target_id}'")"
[[ "${reval_status}" == "pending_review" ]] || fail "revalidation target should be pending_review, got: ${reval_status}"
step "revalidation target pending_review (pass)"

step "verify operator_write NOT revalidated (immune)"
op_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${operator_write_id}'")"
[[ "${op_status}" == "active" ]] || fail "operator_write should remain active, got: ${op_status}"
step "operator_write immune to revalidation (pass)"

step "verify verified_write_back NOT revalidated (immune)"
writeback_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${verified_writeback_id}'")"
[[ "${writeback_status}" == "active" ]] || fail "verified_write_back should remain active, got: ${writeback_status}"
step "verified_write_back immune to revalidation (pass)"

step "verify retain_forever pair NOT revalidated (immune)"
retain_reval_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status != 'active'")"
[[ "${retain_reval_count}" == "0" ]] || fail "retain_forever items should remain active after revalidation, got non-active count: ${retain_reval_count}"
step "retain_forever immune to revalidation (pass)"

# ────────────────────────────────────────────────────────────────────────
# 4. Explainability: verify audit log populated from revalidation
# ────────────────────────────────────────────────────────────────────────
step "verify explainability: audit log has revalidation reason"
explain_output="$(cargo run --quiet -- memory explain-forgetting --memory-item-id "${reval_target_id}")"
explain_count="$(printf '%s' "${explain_output}" | jq 'length')"
[[ "${explain_count}" -ge 1 ]] || fail "no audit log entries for revalidated item"
explain_reason="$(printf '%s' "${explain_output}" | jq -r '.[0].reason')"
[[ "${explain_reason}" == *"freshness"* ]] || fail "audit reason should mention freshness, got: ${explain_reason}"
step "explainability: reason='${explain_reason}' (pass)"

# ────────────────────────────────────────────────────────────────────────
# 5. Prune expired items
# ────────────────────────────────────────────────────────────────────────
step "run prune (should prune ephemeral expired item)"
now_epoch_ms="$(./scripts/epoch_ms.sh)"
prune_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind pruning_job \
  --now-epoch-ms "${now_epoch_ms}" \
  --utility-threshold 0.05)"
prune_job_kind="$(printf '%s' "${prune_output}" | jq -r '.job_kind')"
[[ "${prune_job_kind}" == "pruning_job" ]] || fail "unexpected pruning job kind: ${prune_job_kind}"
pruned_count="$(printf '%s' "${prune_output}" | jq '.action_count')"
step "pruned ${pruned_count} items"
[[ "${pruned_count}" -ge 1 ]] || fail "expected at least 1 pruned item"

step "verify ephemeral item is now pruned"
consolidation_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${ephemeral_expired_id}'")"
[[ "${consolidation_status}" == "pruned" ]] || fail "ephemeral item consolidation_status should be pruned, got: ${consolidation_status}"
step "ephemeral item pruned (pass)"

# ────────────────────────────────────────────────────────────────────────
# 6. Hostile negative path: immutable items survive aggressive pruning
# ────────────────────────────────────────────────────────────────────────
step "HOSTILE: verify durable raw_capture item NOT pruned"
durable_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${durable_status}" == "active" ]] || fail "durable raw_capture must remain active, got: ${durable_status}"
step "durable raw_capture protected (pass)"

step "HOSTILE: verify operator_write item NOT pruned"
op_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${operator_write_id}'")"
[[ "${op_status2}" == "active" ]] || fail "operator_write must remain active, got: ${op_status2}"
step "operator_write protected (pass)"

step "HOSTILE: verify legal_hold item NOT pruned"
legal_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${legal_hold_id}'")"
[[ "${legal_status}" == "active" ]] || fail "legal_hold must remain active, got: ${legal_status}"
step "legal_hold protected (pass)"

step "HOSTILE: verify verified_write_back item NOT pruned"
writeback_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${verified_writeback_id}'")"
[[ "${writeback_status2}" == "active" ]] || fail "verified_write_back must remain active, got: ${writeback_status2}"
step "verified_write_back protected (pass)"

step "HOSTILE: verify retain_forever items NOT pruned"
retain_prune_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status != 'active'")"
[[ "${retain_prune_count}" == "0" ]] || fail "retain_forever items must remain active after prune, got non-active count: ${retain_prune_count}"
step "retain_forever protected from prune (pass)"

step "HOSTILE: verify low-utility ephemeral retain_forever item NOT pruned"
ephemeral_retain_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${ephemeral_retain_id}'")"
[[ "${ephemeral_retain_status}" == "active" ]] || fail "ephemeral retain_forever item must remain active after low-utility prune, got: ${ephemeral_retain_status}"
step "ephemeral retain_forever protected from low-utility prune (pass)"

# ────────────────────────────────────────────────────────────────────────
# 7. Explainability: pruned item has audit trail
# ────────────────────────────────────────────────────────────────────────
step "verify explainability: pruned item has audit log"
prune_explain="$(cargo run --quiet -- memory explain-forgetting --memory-item-id "${ephemeral_expired_id}")"
prune_explain_count="$(printf '%s' "${prune_explain}" | jq 'length')"
[[ "${prune_explain_count}" -ge 1 ]] || fail "no audit log for pruned item"
prune_action="$(printf '%s' "${prune_explain}" | jq -r '.[0].action')"
[[ "${prune_action}" == "prune_ttl_expired" || "${prune_action}" == "prune_low_utility" ]] || fail "unexpected prune action: ${prune_action}"
step "pruned item explainable: action=${prune_action} (pass)"

# ────────────────────────────────────────────────────────────────────────
# 8. Archive cold tier
# ────────────────────────────────────────────────────────────────────────
step "run cold_archive_job with stale-days=0 (aggressive: should archive stale standard item)"
archive_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind cold_archive_job \
  --stale-days 0)"
archive_job_kind="$(printf '%s' "${archive_output}" | jq -r '.job_kind')"
[[ "${archive_job_kind}" == "cold_archive_job" ]] || fail "unexpected archive job kind: ${archive_job_kind}"
archived_count="$(printf '%s' "${archive_output}" | jq '.action_count')"
step "archived ${archived_count} items"

step "verify standard stale item is now archived"
stale_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${standard_stale_id}'")"
[[ "${stale_status}" == "archived" ]] || fail "stale standard item should be archived, got: ${stale_status}"
step "stale standard item archived (pass)"

step "verify archived item retention_class changed to archive"
archive_rc="$(psql "${dsn}" -tA -c "SELECT retention_class FROM ami.memory_items WHERE memory_item_id = '${standard_stale_id}'")"
[[ "${archive_rc}" == "archive" ]] || fail "archived item retention_class should be 'archive', got: ${archive_rc}"
step "retention_class=archive (pass)"

# ────────────────────────────────────────────────────────────────────────
# 9. Hostile: immutable items survive archival
# ────────────────────────────────────────────────────────────────────────
step "HOSTILE: verify durable raw_capture NOT archived even with stale-days=0"
durable_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${durable_status2}" == "active" ]] || fail "durable raw_capture must remain active after archive, got: ${durable_status2}"
step "durable raw_capture survived archival (pass)"

step "HOSTILE: verify legal_hold NOT archived even with stale-days=0"
legal_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${legal_hold_id}'")"
[[ "${legal_status2}" == "active" ]] || fail "legal_hold must remain active after archive, got: ${legal_status2}"
step "legal_hold survived archival (pass)"

step "HOSTILE: verify verified_write_back NOT archived"
writeback_status3="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${verified_writeback_id}'")"
[[ "${writeback_status3}" == "active" ]] || fail "verified_write_back must remain active after archive, got: ${writeback_status3}"
step "verified_write_back survived archival (pass)"

step "HOSTILE: verify retain_forever pair NOT archived"
retain_archive_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status != 'active'")"
[[ "${retain_archive_count}" == "0" ]] || fail "retain_forever items must remain active after archive, got non-active count: ${retain_archive_count}"
step "retain_forever survived archival (pass)"

# ────────────────────────────────────────────────────────────────────────
# 10. Full consolidation run + immutable count
# ────────────────────────────────────────────────────────────────────────
step "run full consolidation"
consolidation_output="$(cargo run --quiet -- memory consolidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --now-epoch-ms "${now_epoch_ms}")"
immutable_protected="$(printf '%s' "${consolidation_output}" | jq '.immutable_protected_count')"
step "immutable protected items: ${immutable_protected}"
[[ "${immutable_protected}" -ge 7 ]] || fail "expected at least 7 immutable protected items (durable+operator_write+legal_hold+verified_write_back+retain_forever pair+ephemeral retain_forever), got: ${immutable_protected}"

step "verify consolidation report contract version"
contract_version="$(printf '%s' "${consolidation_output}" | jq -r '.contract_version')"
[[ "${contract_version}" == "forgetting-consolidation-v1" ]] || fail "unexpected contract version: ${contract_version}"
step "contract version verified (pass)"

step "verify safety invariant declared in report"
safety_inv="$(printf '%s' "${consolidation_output}" | jq -r '.safety_invariant')"
[[ "${safety_inv}" == *"raw_capture"* ]] || fail "safety invariant must mention raw_capture"
[[ "${safety_inv}" == *"operator_write"* ]] || fail "safety invariant must mention operator_write"
[[ "${safety_inv}" == *"verified_write_back"* ]] || fail "safety invariant must mention verified_write_back"
[[ "${safety_inv}" == *"durable"* ]] || fail "safety invariant must mention durable"
[[ "${safety_inv}" == *"legal_hold"* ]] || fail "safety invariant must mention legal_hold"
[[ "${safety_inv}" == *"retain_forever"* ]] || fail "safety invariant must mention retain_forever"
step "safety invariant complete (pass)"

step "verify retain_forever duplicate pair NOT compacted by dedup"
retain_compacted_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status = 'compacted'")"
[[ "${retain_compacted_count}" == "0" ]] || fail "retain_forever duplicate pair must not be compacted, got compacted count: ${retain_compacted_count}"
step "retain_forever pair immune to dedup (pass)"

# ────────────────────────────────────────────────────────────────────────
# 10b. Named job surface is materialized as runtime contract
# ────────────────────────────────────────────────────────────────────────
step "verify de_duplication_job surface is materialized"
dedup_job_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind de_duplication_job)"
dedup_job_kind="$(printf '%s' "${dedup_job_output}" | jq -r '.job_kind')"
[[ "${dedup_job_kind}" == "de_duplication_job" ]] || fail "unexpected de-dup job kind: ${dedup_job_kind}"
step "de_duplication_job surfaced (pass)"

step "verify compaction_job surface is materialized"
compaction_job_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind compaction_job)"
compaction_job_kind="$(printf '%s' "${compaction_job_output}" | jq -r '.job_kind')"
[[ "${compaction_job_kind}" == "compaction_job" ]] || fail "unexpected compaction job kind: ${compaction_job_kind}"
step "compaction_job surfaced (pass)"

step "verify summarization_job surface is materialized"
summarization_job_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind summarization_job)"
summarization_job_kind="$(printf '%s' "${summarization_job_output}" | jq -r '.job_kind')"
summarization_job_actions="$(printf '%s' "${summarization_job_output}" | jq '.action_count')"
[[ "${summarization_job_kind}" == "summarization_job" ]] || fail "unexpected summarization job kind: ${summarization_job_kind}"
[[ "${summarization_job_actions}" == "0" ]] || fail "summarization_job should currently be explicit no-op, got actions: ${summarization_job_actions}"
step "summarization_job surfaced as explicit no-op (pass)"

# ────────────────────────────────────────────────────────────────────────
# 11. Stale knowledge doesn't resurface: pruned items excluded from live DB query
# ────────────────────────────────────────────────────────────────────────
step "verify stale/pruned items not in active consolidation_status"
active_count="$(psql "${dsn}" -tA -c "
  SELECT COUNT(*) FROM ami.memory_items
  WHERE namespace_id = (
    SELECT namespace_id FROM ami.namespaces WHERE code = '${namespace_code}'
  ) AND consolidation_status = 'active'
")"
total_count="$(psql "${dsn}" -tA -c "
  SELECT COUNT(*) FROM ami.memory_items
  WHERE namespace_id = (
    SELECT namespace_id FROM ami.namespaces WHERE code = '${namespace_code}'
  )
")"
step "active items: ${active_count} / total items: ${total_count}"
[[ "${active_count}" -lt "${total_count}" ]] || fail "expected some items to be non-active after forgetting operations"
step "stale items excluded from active set (pass)"

# ────────────────────────────────────────────────────────────────────────
# 12. End-to-end explainability: every non-active item has audit trail
# ────────────────────────────────────────────────────────────────────────
step "verify all non-active items have audit trail"
non_active_ids="$(psql "${dsn}" -tA -c "
  SELECT memory_item_id FROM ami.memory_items
  WHERE namespace_id = (
    SELECT namespace_id FROM ami.namespaces WHERE code = '${namespace_code}'
  ) AND consolidation_status != 'active'
")"
audit_missing=0
while IFS= read -r mid; do
  [[ -z "${mid}" ]] && continue
  audit_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.forgetting_audit_log WHERE memory_item_id = '${mid}'")"
  if [[ "${audit_count}" -lt 1 ]]; then
    echo "WARN: no audit trail for non-active item ${mid}" >&2
    audit_missing=$((audit_missing + 1))
  fi
done <<< "${non_active_ids}"
[[ "${audit_missing}" -eq 0 ]] || fail "${audit_missing} non-active items lack audit trail"
step "all non-active items have explainable audit trail (pass)"

step "verify lifecycle transition stats CLI surfaces Queue 2 derived contract"
transition_stats_output="$(cargo run --quiet -- memory transition-stats \
  --project "${project_code}" \
  --namespace "${namespace_code}")"
transition_contract="$(printf '%s' "${transition_stats_output}" | jq -r '.contract_version')"
[[ "${transition_contract}" == "lifecycle-transition-stats-v1" ]] || fail "unexpected transition stats contract: ${transition_contract}"
transition_rows="$(printf '%s' "${transition_stats_output}" | jq '.rows | length')"
[[ "${transition_rows}" -ge 3 ]] || fail "expected lifecycle transition rows, got ${transition_rows}"
for next_state in pruned archived pending_review; do
  transition_state_count="$(printf '%s' "${transition_stats_output}" | jq --arg state "${next_state}" '[.rows[] | select(.next_state == $state)] | length')"
  [[ "${transition_state_count}" -ge 1 ]] || fail "expected transition stats row for next_state=${next_state}, got ${transition_state_count}"
done
compaction_transition_count="$(printf '%s' "${transition_stats_output}" | jq '[.rows[] | select(.next_state == "compacted")] | length')"
compaction_job_actions="$(printf '%s' "${compaction_job_output}" | jq '.action_count')"
if [[ "${compaction_job_actions}" -gt 0 ]]; then
  [[ "${compaction_transition_count}" -ge 1 ]] || fail "expected compacted transition row when compaction_job produced actions"
fi
step "lifecycle transition stats CLI surfaces pruned/archived/pending_review and conditionally compacted rows (pass)"

step "verify lifecycle cohort-risk CLI surfaces Queue 3 advisory contract"
cohort_risk_output="$(cargo run --quiet -- memory cohort-risk \
  --project "${project_code}" \
  --namespace "${namespace_code}")"
cohort_risk_contract="$(printf '%s' "${cohort_risk_output}" | jq -r '.contract_version')"
[[ "${cohort_risk_contract}" == "lifecycle-cohort-risk-v1" ]] || fail "unexpected cohort risk contract: ${cohort_risk_contract}"
cohort_risk_rows="$(printf '%s' "${cohort_risk_output}" | jq '.rows | length')"
[[ "${cohort_risk_rows}" -ge 1 ]] || fail "expected lifecycle cohort risk rows, got ${cohort_risk_rows}"
cohort_risk_invalid_states="$(printf '%s' "${cohort_risk_output}" | jq '[.rows[] | select((.expected_next_state | IN("active_hot","active_stale","pending_review","compacted","archived","pruned","protected","quarantined")) | not)] | length')"
[[ "${cohort_risk_invalid_states}" -eq 0 ]] || fail "cohort risk surfaced invalid expected_next_state"
cohort_risk_missing_summary="$(printf '%s' "${cohort_risk_output}" | jq '[.rows[] | select((.cohort_reason_summary | type != "string") or (.cohort_reason_summary == ""))] | length')"
[[ "${cohort_risk_missing_summary}" -eq 0 ]] || fail "cohort risk rows missing cohort_reason_summary"
cohort_risk_nonzero="$(printf '%s' "${cohort_risk_output}" | jq '[.rows[] | select(.pending_review_risk_7d > 0 or .archive_risk_30d > 0 or .prune_risk_30d > 0)] | length')"
[[ "${cohort_risk_nonzero}" -ge 1 ]] || fail "expected at least one non-zero lifecycle cohort risk row"
step "lifecycle cohort-risk CLI surfaces Queue 3 advisory contract (pass)"

step "verify lifecycle policy-simulate CLI surfaces Queue 3 approval contour without authority"
step "seed policy-simulate durable lane sentinels"
policy_simulate_sentinel_ids="$(create_policy_simulate_durable_sentinels)"
IFS='|' read -r policy_simulate_task_node_id policy_simulate_task_event_id policy_simulate_link_decision_id policy_simulate_pending_link_proposal_id policy_simulate_ledger_entry_id policy_simulate_lease_id <<< "${policy_simulate_sentinel_ids}"
[[ -n "${policy_simulate_task_node_id}" ]] || fail "policy simulate task node sentinel was not created"
[[ -n "${policy_simulate_task_event_id}" ]] || fail "policy simulate task event sentinel was not created"
[[ -n "${policy_simulate_link_decision_id}" ]] || fail "policy simulate link decision sentinel was not created"
[[ -n "${policy_simulate_pending_link_proposal_id}" ]] || fail "policy simulate pending link proposal sentinel was not created"
[[ -n "${policy_simulate_ledger_entry_id}" ]] || fail "policy simulate ExecCtl ledger sentinel was not created"
[[ -n "${policy_simulate_lease_id}" ]] || fail "policy simulate ExecCtl lease sentinel was not created"
policy_simulate_scope_isolation_output="$(create_policy_simulate_scope_isolation_sentinels)"
policy_simulate_scope_isolation_ok="$(printf '%s' "${policy_simulate_scope_isolation_output}" | jq -r '
  .task_nodes == 2
  and .task_events == 2
  and .memory_link_decisions == 2
  and .pending_link_proposals == 2
  and .execctl_task_ledger_entries == 2
  and .execctl_task_leases == 2
')"
[[ "${policy_simulate_scope_isolation_ok}" == "true" ]] || fail "policy simulate scope-isolation sentinels were not fully created"
assert_task_events_append_only_guard "${policy_simulate_task_event_id}"
policy_simulate_projection_snapshot_before="$(policy_simulate_projection_snapshot)"
memory_count_before_policy_simulate="$(psql "${dsn}" -tA -c "
  SELECT COUNT(*)
  FROM ami.memory_items mi
  JOIN ami.projects p ON p.project_id = mi.project_id
  JOIN ami.namespaces n ON n.namespace_id = mi.namespace_id
  WHERE p.code = '${project_code}'
    AND n.code = '${namespace_code}'
")"
audit_count_before_policy_simulate="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.forgetting_audit_log WHERE project_code = '${project_code}' AND namespace_code = '${namespace_code}'")"
policy_simulate_output="$(cargo run --quiet -- memory policy-simulate \
  --project "${project_code}" \
  --namespace "${namespace_code}")"
memory_count_after_policy_simulate="$(psql "${dsn}" -tA -c "
  SELECT COUNT(*)
  FROM ami.memory_items mi
  JOIN ami.projects p ON p.project_id = mi.project_id
  JOIN ami.namespaces n ON n.namespace_id = mi.namespace_id
  WHERE p.code = '${project_code}'
    AND n.code = '${namespace_code}'
")"
audit_count_after_policy_simulate="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.forgetting_audit_log WHERE project_code = '${project_code}' AND namespace_code = '${namespace_code}'")"
policy_simulate_projection_snapshot_after="$(policy_simulate_projection_snapshot)"
[[ "${memory_count_after_policy_simulate}" == "${memory_count_before_policy_simulate}" ]] || fail "policy simulate must not mutate memory item count"
[[ "${audit_count_after_policy_simulate}" == "${audit_count_before_policy_simulate}" ]] || fail "policy simulate must not write forgetting audit actions"
[[ "${policy_simulate_projection_snapshot_after}" == "${policy_simulate_projection_snapshot_before}" ]] || fail "policy simulate mutated memory/task-memory/ExecCtl projection lanes"
policy_simulate_contract="$(printf '%s' "${policy_simulate_output}" | jq -r '.contract_version')"
[[ "${policy_simulate_contract}" == "lifecycle-policy-simulate-v1" ]] || fail "unexpected policy simulate contract: ${policy_simulate_contract}"
policy_simulate_authority="$(printf '%s' "${policy_simulate_output}" | jq -r '.authority_mode')"
[[ "${policy_simulate_authority}" == "advisory_only_no_runtime_authority" ]] || fail "policy simulate surfaced unexpected authority mode: ${policy_simulate_authority}"
policy_simulate_rows="$(printf '%s' "${policy_simulate_output}" | jq '.rows | length')"
[[ "${policy_simulate_rows}" -ge 1 ]] || fail "expected lifecycle policy simulation rows, got ${policy_simulate_rows}"
policy_simulate_invalid_actions="$(printf '%s' "${policy_simulate_output}" | jq '[.rows[] | select((.recommended_review_action | IN("hold_current_policy","review_revalidation_queue","review_archive_candidate","review_prune_candidate","observe_only")) | not)] | length')"
[[ "${policy_simulate_invalid_actions}" -eq 0 ]] || fail "policy simulate surfaced invalid recommended_review_action"
policy_simulate_invalid_urgency="$(printf '%s' "${policy_simulate_output}" | jq '[.rows[] | select((.urgency | IN("manual_only","high","medium","low")) | not)] | length')"
[[ "${policy_simulate_invalid_urgency}" -eq 0 ]] || fail "policy simulate surfaced invalid urgency"
policy_simulate_missing_blocker="$(printf '%s' "${policy_simulate_output}" | jq '[.rows[] | select((.blocking_reasons | index("advisory_only_no_runtime_authority")) == null)] | length')"
[[ "${policy_simulate_missing_blocker}" -eq 0 ]] || fail "policy simulate rows lost advisory-only blocker"
policy_simulate_guardrails_ok="$(printf '%s' "${policy_simulate_output}" | jq -r '
  .guardrails.truth_authority == false
  and .guardrails.routing_authority == false
  and .guardrails.forgetting_authority == false
  and .guardrails.promotion_authority == false
  and .guardrails.destructive_authority == false
  and .guardrails.runtime_authority == false
  and .guardrails.auto_apply_allowed == false
')"
[[ "${policy_simulate_guardrails_ok}" == "true" ]] || fail "policy simulate guardrails gained forbidden authority"
policy_simulate_validation_contract="$(printf '%s' "${policy_simulate_output}" | jq -r '.measured_validation.contract_version')"
[[ "${policy_simulate_validation_contract}" == "lifecycle-policy-simulate-validation-v1" ]] || fail "unexpected policy simulate validation contract: ${policy_simulate_validation_contract}"
policy_simulate_review_packet_state="$(printf '%s' "${policy_simulate_output}" | jq -r '.measured_validation.review_packet_state')"
[[ "${policy_simulate_review_packet_state}" == "review_packet_ready" ]] || fail "policy simulate review packet not ready on controlled fixture: ${policy_simulate_review_packet_state}"
policy_simulate_approval_state="$(printf '%s' "${policy_simulate_output}" | jq -r '.measured_validation.approval_state')"
[[ "${policy_simulate_approval_state}" == "pending_human_review" ]] || fail "policy simulate approval state must stay human-gated, got: ${policy_simulate_approval_state}"
policy_simulate_validation_flags_ok="$(printf '%s' "${policy_simulate_output}" | jq -r '
  .measured_validation.human_review_required == true
  and .measured_validation.auto_promotion_allowed == false
  and .measured_validation.improvement_measured == false
  and .measured_validation.measured_improvement_state == "not_measured_requires_holdout_or_post_action_outcomes"
  and .measured_validation.missing_advisory_blocker_count == 0
  and .measured_validation.invalid_recommendation_count == 0
  and .measured_validation.invalid_urgency_count == 0
')"
[[ "${policy_simulate_validation_flags_ok}" == "true" ]] || fail "policy simulate validation flags lost fail-closed/human-gated contract"
policy_simulate_validation_sample="$(printf '%s' "${policy_simulate_output}" | jq '.measured_validation.sample_size')"
[[ "${policy_simulate_validation_sample}" -ge 3 ]] || fail "policy simulate validation sample too small on controlled fixture: ${policy_simulate_validation_sample}"
policy_simulate_validation_cohorts="$(printf '%s' "${policy_simulate_output}" | jq '.measured_validation.cohort_count')"
[[ "${policy_simulate_validation_cohorts}" -ge 1 ]] || fail "policy simulate validation cohort count missing"
policy_simulate_authority_blocked_rows="$(printf '%s' "${policy_simulate_output}" | jq '.measured_validation.authority_blocked_row_count')"
[[ "${policy_simulate_authority_blocked_rows}" == "${policy_simulate_rows}" ]] || fail "policy simulate validation did not count every row as authority-blocked"
step "lifecycle policy-simulate CLI stays advisory-only and surfaces approval contour (pass)"

# ────────────────────────────────────────────────────────────────────────
# 13. Final integrity: immutable items untouched throughout entire proof
# ────────────────────────────────────────────────────────────────────────
step "FINAL: verify all immutable items still active"
for item_label_id in \
  "durable_raw:${durable_raw_id}" \
  "operator_write:${operator_write_id}" \
  "legal_hold:${legal_hold_id}" \
  "verified_write_back:${verified_writeback_id}" \
  "ephemeral_retain_forever:${ephemeral_retain_id}" \
  "retain_forever_a:${retain_forever_a_id}" \
  "retain_forever_b:${retain_forever_b_id}"; do
  label="${item_label_id%%:*}"
  iid="${item_label_id##*:}"
  status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${iid}'")"
  [[ "${status}" == "active" ]] || fail "FINAL: ${label} item ${iid} should be active, got: ${status}"
done
step "all immutable items survived full proof lifecycle (pass)"

echo ""
echo "==========================================="
echo " forgetting/consolidation proof: ALL PASS"
echo "==========================================="
