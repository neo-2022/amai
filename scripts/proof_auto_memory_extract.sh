#!/usr/bin/env bash
# ponytail: smallest proof that auto-extract turns raw events into memory_items without manual tags.
set -euo pipefail

cd "$(dirname "$0")/.."

export PGPASSWORD=ami_admin_change_me
PG="psql -h 127.0.0.1 -p 55432 -U ami_admin -d agent_memory_index -t -A"
RAW_ID=$(uuidgen)
NOW_MS=$(python3 -c 'import time; print(int(time.time()*1000))')
PAYLOAD='{"note":"user said remember Amai auto-extracts memory"}'

# Insert a raw memory event directly; no human tags touch the pipeline.
$PG -c "INSERT INTO ami.memory_raw_events (
    memory_raw_event_id, workspace_id, project_id, namespace_id,
    event_kind, item_kind, visibility_scope, sensitivity_class, derivation_kind,
    truth_state, trust_state, verification_state, lifecycle_state,
    title, summary, body,
    source_event_ids, artifact_refs, message_refs, evidence_span,
    server_received_at_epoch_ms, payload, server_order_seq
)
SELECT
    '$RAW_ID', w.workspace_id, p.project_id, n.namespace_id,
    'memory_candidate_write', 'raw_fact', 'project_shared', 'internal', 'auto_extract',
    'raw', 'raw', 'unverified', 'hot',
    'Auto memory requirement', 'User wants auto-extract.', 'User said Amai does everything automatically and never uses manual tags.',
    '[\"$RAW_ID\"]'::jsonb, '[]'::jsonb, '[]'::jsonb, '{}'::jsonb,
    $NOW_MS, '$PAYLOAD'::jsonb,
    (SELECT COALESCE(MIN(server_order_seq), 0) - 1 FROM ami.memory_raw_events)
FROM ami.workspaces w
JOIN ami.projects p ON p.workspace_id = w.workspace_id
JOIN ami.namespaces n ON n.project_id = p.project_id
WHERE p.code = 'amai' AND n.code = 'continuity' AND w.code = 'default';" >/dev/null

# Run auto-extraction via CLI; this is the only action under test.
RESULT=$(./scripts/amai_exec.sh memory auto-extract --project amai --namespace continuity --limit 10)
echo "extract result: $RESULT"

CREATED=$(printf '%s' "$RESULT" | python3 -c 'import sys,json; print(json.load(sys.stdin)["created"])')
if [[ "$CREATED" -lt "1" ]]; then
    echo "proof_auto_memory_extract: expected created>=1, got $RESULT"
    exit 1
fi

# Verify the memory item exists and links back to the raw event.
ITEM_ID=$($PG -c "SELECT m.memory_item_id FROM ami.memory_items m JOIN ami.projects p ON p.project_id = m.project_id JOIN ami.namespaces n ON n.namespace_id = m.namespace_id WHERE p.code='amai' AND n.code='continuity' AND m.source_event_ids @> '[\"$RAW_ID\"]'::jsonb LIMIT 1;")
if [[ -z "$ITEM_ID" ]]; then
    echo "proof_auto_memory_extract: memory_item tied to raw event not found"
    exit 1
fi

# Verify derivation was automatic, not operator_write.
DERIVATION=$($PG -c "SELECT derivation_kind FROM ami.memory_items WHERE memory_item_id='$ITEM_ID';")
if [[ "$DERIVATION" != "extract" ]]; then
    echo "proof_auto_memory_extract: expected derivation_kind=extract, got $DERIVATION"
    exit 1
fi

echo "proof_auto_memory_extract: OK (memory_item=$ITEM_ID from raw event without tags)"
