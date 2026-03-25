#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

row="$(
  psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<'SQL'
SELECT ws.context_pack_id, ws.thread_id
FROM (
  SELECT DISTINCT ON (payload->'working_state_event'->>'context_pack_id')
    payload->'working_state_event'->>'context_pack_id' AS context_pack_id,
    payload->'working_state_event'->>'thread_id' AS thread_id,
    captured_at_epoch_ms
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'working_state_event'
    AND payload->'working_state_event'->>'event_kind' = 'retrieval_context_pack'
    AND COALESCE(payload->'working_state_event'->>'thread_id', '') <> ''
  ORDER BY payload->'working_state_event'->>'context_pack_id', captured_at_epoch_ms DESC
) AS ws
JOIN (
  SELECT DISTINCT ON (payload->'token_budget_event'->>'context_pack_id')
    payload->'token_budget_event'->>'context_pack_id' AS context_pack_id,
    captured_at_epoch_ms
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'token_budget_event'
    AND payload->'token_budget_event'->>'traffic_class' = 'live'
    AND payload->'token_budget_event'->>'measurement_scope' = 'retrieval_lower_bound'
    AND (payload->'token_budget_event'->>'assistant_generation_tokens') IS NULL
    AND (payload->'token_budget_event'->'whole_cycle_observed'->>'assistant_generation_tokens') IS NULL
  ORDER BY payload->'token_budget_event'->>'context_pack_id', captured_at_epoch_ms DESC
) AS tb USING (context_pack_id)
ORDER BY tb.captured_at_epoch_ms DESC
LIMIT 1;
SQL
)"

if [[ -z "$row" ]]; then
  echo "proof_token_turn_group_attach: no live retrieval_lower_bound context pack without assistant_generation found" >&2
  exit 1
fi

IFS=$'\t' read -r context_pack_id thread_id <<<"$row"
turn_id="proof-turn-attach-$(date +%s%N)"
attach_path="/tmp/amai-proof-token-turn-group-attach.json"
report_path="/tmp/amai-proof-token-turn-group-report.json"

./target/release/amai observe token-whole-cycle-turn-attach \
  --thread-id "$thread_id" \
  --turn-id "$turn_id" \
  --context-pack-id "$context_pack_id" \
  --assistant-generation-tokens 77 >"$attach_path"

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >"$report_path"

ATTACH_PATH="$attach_path" REPORT_PATH="$report_path" CONTEXT_PACK_ID="$context_pack_id" TURN_ID="$turn_id" python3 - <<'PY'
import json
import os
from pathlib import Path

attach = json.loads(Path(os.environ["ATTACH_PATH"]).read_text())
report = json.loads(Path(os.environ["REPORT_PATH"]).read_text())["token_budget_report"]
lifetime = report["agent_cycle_economics"]["lifetime"]["client_limit_meter_alignment"]
source = lifetime["assistant_generation_observation_source"]

assert attach["assistant_generation_turn_observed_attach"]["attached"] is True, attach
assert attach["assistant_generation_turn_observed_attach"]["turn_id"] == os.environ["TURN_ID"], attach
assert os.environ["CONTEXT_PACK_ID"] in attach["assistant_generation_turn_observed_attach"]["context_pack_ids"], attach
assert source["usable_direct_turns"] >= 1, source
assert source["matched_direct_turn_ids"] >= 1, source
assert source["source_kind"] in {
    "direct_turn_attach_v1",
    "direct_turn_attach_plus_rollout_turn_timeline_v1",
}, source
PY

printf 'proof_token_turn_group_attach: PASS\n'
