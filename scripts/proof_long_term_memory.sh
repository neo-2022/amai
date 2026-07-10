#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

TOKEN="crimson-fox-$(date +%s)"
VALID_FROM=$(date -d '2026-01-01' +%s)000
VALID_TO=$(date -d '2040-01-01' +%s)000
BEFORE=$(date -d '2025-01-01' +%s)000
export PGPASSWORD="ami_admin_change_me"
PG="psql -h 127.0.0.1 -p 55432 -U ami_admin -d agent_memory_index"

echo "=== 1. Create durable memory card ==="
card_line=$(./scripts/amai_exec.sh memory create-card \
  --project amai --namespace continuity \
  --title "Ten year memory empirical proof" \
  --summary "Long-term memory proof marker" \
  --body "On $(date -u +%Y-%m-%dT%H:%M:%SZ) we tested Amai long-term memory with token ${TOKEN}. This card must be retrievable years later." \
  --truth-state current \
  --valid-from-epoch-ms "${VALID_FROM}" \
  --valid-to-epoch-ms "${VALID_TO}" \
  --derivation-kind operator_write)
card_id=$(echo "$card_line" | sed 's/memory card created: //' | awk '{print $1}')
echo "created card $card_id"

echo "=== 2. Semantic lookup by meaning immediately (no manual reconcile) ==="
out1=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "memory proof marker retrievable years later" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
if ! echo "$out1" | jq -e --arg id "$card_id" '.retrieval.memory_cards[] | select(.memory_card_id == $id)' >/dev/null; then
  echo "FAIL: semantic lookup immediately after create-card"
  exit 1
fi
echo "semantic hit confirmed"

echo "=== 3. Literal lookup via full-text ==="
out2=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "$TOKEN" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
echo "$out2" | jq -e --arg id "$card_id" '.retrieval.memory_cards[] | select(.memory_card_id == $id)' >/dev/null
echo "literal hit confirmed"

echo "=== 4. Temporal lookup before valid_from must be empty ==="
out3=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "$TOKEN" --at-epoch-ms "${BEFORE}" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
card_count_before=$(echo "$out3" | jq --arg id "$card_id" '[.retrieval.memory_cards[] | select(.memory_card_id == $id)] | length')
if [[ "$card_count_before" -eq 0 ]]; then
  echo "temporal filter before valid_from works"
else
  echo "FAIL: temporal filter leaked"
  exit 1
fi

AFTER_VALID_TO=$(date -d '2041-01-01' +%s)000
echo "=== 4b. Temporal lookup after valid_to must be empty ==="
out3b=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "$TOKEN" --at-epoch-ms "${AFTER_VALID_TO}" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
card_count_after=$(echo "$out3b" | jq --arg id "$card_id" '[.retrieval.memory_cards[] | select(.memory_card_id == $id)] | length')
if [[ "$card_count_after" -eq 0 ]]; then
  echo "temporal filter after valid_to works"
else
  echo "FAIL: temporal filter leaked past valid_to"
  exit 1
fi

echo "=== 5. Check memory_cards_v1 has points ==="
qdrant_count=$(curl -s http://127.0.0.1:56333/collections/memory_cards_v1 | python3 -c 'import sys,json; print(json.load(sys.stdin)["result"]["points_count"])')
if [[ "$qdrant_count" -gt 0 ]]; then
  echo "memory_cards_v1 points: $qdrant_count"
else
  echo "FAIL: memory_cards_v1 empty"
  exit 1
fi

echo "=== 6. Auto-recorded learning episode is in interaction log and searchable ==="
episode_event_id="auto-learning-episode-${card_id}"
out4=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "create memory card" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
if echo "$out4" | jq -e --arg id "$episode_event_id" '.retrieval.interaction_log_events[] | select(.event_id == $id)' >/dev/null; then
  echo "auto-recorded learning episode confirmed: $episode_event_id"
else
  echo "FAIL: auto-recorded learning episode not retrievable via context pack"
  exit 1
fi

echo "=== 7. Auto-created graph relation edges connect similar memory cards ==="
card_line_b=$(./scripts/amai_exec.sh memory create-card \
  --project amai --namespace continuity \
  --title "Decade memory proof marker second" \
  --summary "Long-term memory proof marker" \
  --body "Sibling proof marker for relation edge wiring." \
  --truth-state current \
  --valid-from-epoch-ms "${VALID_FROM}" \
  --valid-to-epoch-ms "${VALID_TO}" \
  --derivation-kind operator_write)
card_id_b=$(echo "$card_line_b" | sed 's/memory card created: //' | awk '{print $1}')
echo "created similar card $card_id_b"

# Use a temporary cache file for the relation-edge lookup so we test fresh query, not stale cache.
tmp_local_cache=$(mktemp state/proof-ltm-cache-XXXXXX.db)
trap 'rm -f "$tmp_local_cache"' EXIT
AMAISTATE="$tmp_local_cache" ./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "Long-term memory proof marker" --disable-cache --limit-documents 5 --limit-semantic-chunks 5 >/dev/null
out5=$(AMAISTATE="$tmp_local_cache" ./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "Long-term memory proof marker" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
edge_count=$(echo "$out5" | jq '[.retrieval.memory_relation_edges[]?] | length')
if [[ "$edge_count" -gt 0 ]]; then
  echo "auto-created relation edges: $edge_count"
else
  echo "FAIL: no memory relation edges auto-created between similar cards"
  exit 1
fi

echo "=== 8. Superseded card is excluded from present-time retrieval ==="
v1_line=$(./scripts/amai_exec.sh memory create-card \
  --project amai --namespace continuity \
  --title "Memory architecture v1" --summary "Initial architecture" \
  --body "Amai memory used only PostgreSQL FTS." --truth-state current \
  --valid-from-epoch-ms "${VALID_FROM}" --valid-to-epoch-ms "${VALID_TO}" --derivation-kind operator_write)
v1_id=$(echo "$v1_line" | sed 's/memory card created: //' | awk '{print $1}')
v2_line=$(./scripts/amai_exec.sh memory create-card \
  --project amai --namespace continuity \
  --title "Memory architecture v2" --summary "Semantic upgrade" \
  --body "Amai memory now uses Qdrant semantic vectors." --truth-state current \
  --valid-from-epoch-ms "${VALID_FROM}" --valid-to-epoch-ms "${VALID_TO}" --derivation-kind operator_write)
v2_id=$(echo "$v2_line" | sed 's/memory card created: //' | awk '{print $1}')
./scripts/amai_exec.sh memory supersede-card --memory-card-id "$v1_id" --superseded-by "$v2_id" >/dev/null
out6=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "architecture v1 PostgreSQL FTS" --disable-cache --limit-documents 10 --limit-semantic-chunks 10)
v1_present=$(echo "$out6" | jq --arg id "$v1_id" '[.retrieval.memory_cards[] | select(.memory_card_id == $id)] | length')
if [[ "$v1_present" -eq 0 ]]; then
  echo "superseded v1 correctly excluded"
else
  echo "FAIL: superseded v1 leaked"
  exit 1
fi

echo "=== 9. Raw events temporal lookup exists and returns ordered records ==="
now_ms=$(date +%s)000
start_window=$(date -d '2026-07-01' +%s)000
raw_count=$($PG -t -c "
  SELECT count(*) FROM ami.memory_raw_events mre
  JOIN ami.projects p ON p.project_id=mre.project_id
  JOIN ami.namespaces n ON n.namespace_id=mre.namespace_id
  JOIN ami.workspaces w ON w.workspace_id=mre.workspace_id
  WHERE p.code='amai' AND n.code='continuity' AND w.code='default'
    AND mre.server_received_at_epoch_ms BETWEEN ${start_window} AND ${now_ms}
" | xargs)
if [[ "$raw_count" -gt 0 ]]; then
  echo "raw events in window: $raw_count"
else
  echo "FAIL: no raw events in temporal window"
  exit 1
fi

echo "=== 10. Durable retention survives automated forgetting jobs ==="
durable_line=$(./scripts/amai_exec.sh memory create-item --project amai --namespace continuity \
  --item-kind fact --title "Durable retention proof $(date +%s)" \
  --summary "Must survive pruning/cold-archive" --truth-state current \
  --derivation-kind operator_write --retention-class durable)
durable_id=$(echo "$durable_line" | sed 's/memory item created: //' | awk '{print $1}')
./scripts/amai_exec.sh memory run-job --project amai --namespace continuity --job-kind pruning_job >/dev/null
./scripts/amai_exec.sh memory run-job --project amai --namespace continuity --job-kind cold_archive_job >/dev/null
durable_status=$($PG -t -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id='${durable_id}'" | xargs)
if [[ "$durable_status" == "active" ]]; then
  echo "durable item survived automated forgetting"
else
  echo "FAIL: durable item was modified by forgetting: $durable_status"
  exit 1
fi

echo "=== 11. Cache reset does not destroy retrievability ==="
rm -f state/local_cache.db
out7=$(./scripts/amai_exec.sh context pack --project amai --namespace continuity \
  --query "$TOKEN" --disable-cache --limit-documents 5 --limit-semantic-chunks 5)
if echo "$out7" | jq -e --arg id "$card_id" '.retrieval.memory_cards[] | select(.memory_card_id == $id)' >/dev/null; then
  echo "retrievable after local cache reset"
else
  echo "FAIL: card lost after cache reset"
  exit 1
fi

echo "=== ALL PASS ==="
