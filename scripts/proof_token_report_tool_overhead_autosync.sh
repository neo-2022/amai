#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

proof_id="$(date +%s%N)"
source_kind="live_context_pack"
context_pack_raw="/tmp/amai-proof-token-tool-overhead-autosync.json"
report_path="/tmp/amai-proof-token-tool-overhead-autosync-report.json"

cleanup() {
  if [[ -z "${context_pack_id:-}" ]]; then
    return
  fi
  PGPASSWORD="$AMI_PG_PASSWORD" psql \
    -h "$AMI_PG_HOST" \
    -p "$AMI_PG_PORT" \
    -U "$AMI_PG_USER" \
    -d "$AMI_PG_DB" \
    -qAtc "
DELETE FROM ami.observability_snapshots
WHERE snapshot_kind = 'token_budget_event'
  AND payload->'token_budget_event'->>'context_pack_id' = '$context_pack_id';
DELETE FROM ami.context_packs
WHERE context_pack_id = '$context_pack_id'::uuid;
" >/dev/null || true
}
trap cleanup EXIT

./target/release/amai context pack \
  --project art \
  --query "tool overhead autosync proof ${proof_id}" \
  --retrieval-mode local_strict \
  --token-source-kind "$source_kind" >"$context_pack_raw"

context_pack_id="$(CONTEXT_PACK_RAW="$context_pack_raw" python3 - <<'PY'
import json
import os
from pathlib import Path

lines = [line.strip() for line in Path(os.environ["CONTEXT_PACK_RAW"]).read_text().splitlines() if line.strip()]
payload = json.loads(lines[-1])
print(payload["context_pack_id"])
PY
)"

PGPASSWORD="$AMI_PG_PASSWORD" psql \
  -h "$AMI_PG_HOST" \
  -p "$AMI_PG_PORT" \
  -U "$AMI_PG_USER" \
  -d "$AMI_PG_DB" \
  -qAtc "
WITH target AS (
  SELECT snapshot_id
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'token_budget_event'
    AND payload->'token_budget_event'->>'context_pack_id' = '$context_pack_id'
  ORDER BY captured_at_epoch_ms DESC
  LIMIT 1
)
UPDATE ami.observability_snapshots
SET payload = jsonb_set(
  payload,
  '{token_budget_event,whole_cycle_observed,tool_overhead_tokens}',
  'null'::jsonb,
  true
)
WHERE snapshot_id IN (SELECT snapshot_id FROM target);
"

update_count="$(PGPASSWORD="$AMI_PG_PASSWORD" psql \
  -h "$AMI_PG_HOST" \
  -p "$AMI_PG_PORT" \
  -U "$AMI_PG_USER" \
  -d "$AMI_PG_DB" \
  -qAtc "
SELECT COUNT(*)
FROM (
  SELECT payload
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'token_budget_event'
    AND payload->'token_budget_event'->>'context_pack_id' = '$context_pack_id'
  ORDER BY captured_at_epoch_ms DESC
  LIMIT 1
) latest
WHERE latest.payload->'token_budget_event'->'whole_cycle_observed'->>'tool_overhead_tokens' IS NOT NULL;
")"

if [[ "$update_count" != "0" ]]; then
  printf 'expected tool_overhead_tokens to be null after manual reset, got %s\n' "$update_count" >&2
  exit 1
fi

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >"$report_path"

restored_count="$(PGPASSWORD="$AMI_PG_PASSWORD" psql \
  -h "$AMI_PG_HOST" \
  -p "$AMI_PG_PORT" \
  -U "$AMI_PG_USER" \
  -d "$AMI_PG_DB" \
  -Atc "
SELECT COUNT(*)
FROM (
  SELECT payload
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'token_budget_event'
    AND payload->'token_budget_event'->>'context_pack_id' = '$context_pack_id'
  ORDER BY captured_at_epoch_ms DESC
  LIMIT 1
) latest
WHERE latest.payload->'token_budget_event'->'whole_cycle_observed'->>'tool_overhead_tokens' IS NOT NULL;
")"

RESTORED_COUNT="$restored_count" python3 - <<'PY'
import os

assert int(os.environ["RESTORED_COUNT"]) >= 1, os.environ["RESTORED_COUNT"]
PY

printf 'proof_token_report_tool_overhead_autosync: PASS\n'
