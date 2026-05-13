#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

if [[ ! -x ./target/release/amai ]]; then
  cargo build --release --quiet
fi

proof_id="$(date +%s%N)"
thread_id="art-live-turn-${proof_id}"
rollout_path="/tmp/${thread_id}.jsonl"
completed_rollout_path="/tmp/${thread_id}-complete.jsonl"
output_path="/tmp/${thread_id}.json"
snapshot_path="/tmp/${thread_id}-snapshot.json"
turn_id="turn-art-continuity"
timestamp_rfc3339="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
live_source_kind="live_art_continuity_${proof_id}"
repaired_source_kind="proof_art_continuity_live_turn"

cleanup() {
  sqlite3 ~/.codex/state_5.sqlite "DELETE FROM threads WHERE id = '$thread_id';" >/dev/null 2>&1 || true
  rm -f "$rollout_path"
  rm -f "$completed_rollout_path"
}
trap cleanup EXIT

write_rollout_file() {
  local total_tokens="$1"
  local include_complete="$2"
  export ROLLOUT_PATH="$rollout_path"
  export TURN_TIMESTAMP="$timestamp_rfc3339"
  export TURN_ID="$turn_id"
  export TOTAL_TOKENS="$total_tokens"
  export INCLUDE_COMPLETE="$include_complete"
  python3 - <<'PY'
import json
import os
from pathlib import Path

rows = [
    {
        "timestamp": os.environ["TURN_TIMESTAMP"],
        "type": "event_msg",
        "payload": {"type": "task_started", "turn_id": os.environ["TURN_ID"]},
    },
    {
        "timestamp": os.environ["TURN_TIMESTAMP"],
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": 24,
                    "cached_input_tokens": 0,
                    "output_tokens": 28,
                    "reasoning_output_tokens": 0,
                    "total_tokens": int(os.environ["TOTAL_TOKENS"]),
                },
                "total_token_usage": {
                    "total_tokens": int(os.environ["TOTAL_TOKENS"]),
                },
                "model_context_window": 258400,
            },
            "rate_limits": {
                "primary": {"used_percent": 11.0},
                "secondary": {"used_percent": 7.0},
            },
        },
    },
]
if os.environ["INCLUDE_COMPLETE"] == "1":
    rows.append(
        {
            "timestamp": os.environ["TURN_TIMESTAMP"],
            "type": "event_msg",
            "payload": {"type": "task_complete", "turn_id": os.environ["TURN_ID"]},
        }
    )
Path(os.environ["ROLLOUT_PATH"]).write_text(
    "\n".join(json.dumps(row, ensure_ascii=False) for row in rows) + "\n",
    encoding="utf-8",
)
PY
}

fetch_token_metrics() {
  local source_kind="$1"
  local context_pack_id="$2"
  psql "$AMI_POSTGRES_DSN" -At -F $'\t' -c "
SELECT
  payload->'token_budget_event'->'naive_scope'->>'tokens',
  payload->'token_budget_event'->'context_pack_render'->>'tokens',
  COALESCE(payload->'token_budget_event'->'whole_cycle_observed'->>'tool_overhead_tokens', '')
FROM ami.observability_snapshots
WHERE snapshot_kind = 'token_budget_event'
  AND payload->'token_budget_event'->>'source_kind' = '$source_kind'
  AND payload->'token_budget_event'->>'context_pack_id' = '$context_pack_id'
ORDER BY created_at DESC
LIMIT 1;
"
}

wait_for_token_metrics() {
  local source_kind="$1"
  local context_pack_id="$2"
  local row=""
  local attempt=""
  for attempt in {1..5}; do
    row="$(fetch_token_metrics "$source_kind" "$context_pack_id" || true)"
    if [[ -n "$row" ]]; then
      local naive_tokens=""
      local context_tokens=""
      local tool_overhead_tokens=""
      IFS=$'\t' read -r naive_tokens context_tokens tool_overhead_tokens <<<"$row"
      if [[ -n "$naive_tokens" && -n "$context_tokens" && -n "$tool_overhead_tokens" ]]; then
        printf '%s\n' "$row"
        return 0
      fi
    fi
    sleep 1
  done
  ./target/release/amai observe token-report \
    --budget-profile codex_5h \
    --include-verify-events true >/dev/null
  fetch_token_metrics "$source_kind" "$context_pack_id"
}

assert_current_live_turn_snapshot() {
  local snapshot_path="$1"
  local thread_id="$2"
  local turn_id="$3"
  export SNAPSHOT_PATH="$snapshot_path"
  export THREAD_ID="$thread_id"
  export TURN_ID="$turn_id"
  python3 - <<'PY'
import json
import os
from pathlib import Path

payload = json.loads(Path(os.environ["SNAPSHOT_PATH"]).read_text())
report = payload["token_budget_report"]
if "token_budget_report" in report:
    report = report["token_budget_report"]
current = report["current_live_turn"]
assert current["exact_pair_available"] is True, current
assert current["status"] == "exact_pair_materialized", current
assert current["thread_id"] == os.environ["THREAD_ID"], current
assert current["turn_id"] == os.environ["TURN_ID"], current
assert current["exact_pair"]["saved_pct"] >= 90.0, current
PY
}

wait_for_current_live_turn_snapshot() {
  local snapshot_path="$1"
  local thread_id="$2"
  local turn_id="$3"
  local attempt=""
  for attempt in {1..16}; do
    timeout 2s ./target/release/amai observe token-report \
      --budget-profile codex_5h \
      --include-verify-events true >/dev/null || true
    CODEX_THREAD_ID="$thread_id" AMAI_AGENT_SCOPE="proof_art_continuity_live_turn" \
      timeout 20s ./target/release/amai observe snapshot >"$snapshot_path" || true
    if [[ ! -s "$snapshot_path" ]]; then
      sleep 2
      continue
    fi
    if assert_current_live_turn_snapshot "$snapshot_path" "$thread_id" "$turn_id" \
      >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  if [[ ! -s "$snapshot_path" ]]; then
    printf 'observe snapshot did not produce JSON for thread_id=%s turn_id=%s\n' \
      "$thread_id" "$turn_id" >&2
    return 1
  fi
  assert_current_live_turn_snapshot "$snapshot_path" "$thread_id" "$turn_id"
}

update_thread_rollout_path() {
  local thread_id="$1"
  local rollout_path="$2"
  sqlite3 ~/.codex/state_5.sqlite \
    "UPDATE threads
     SET rollout_path = '$rollout_path',
         updated_at = strftime('%s','now')
     WHERE id = '$thread_id';" >/dev/null
}

purge_thread_budget_caches() {
  local thread_id="$1"
  local suffix
  suffix="$(printf '%s' "$thread_id" | tr -c '[:alnum:]_-' '_')"
  rm -f \
    "state/observe/thread_bound_budget_snapshot.thread-${suffix}.json" \
    "state/observe/thread_bound_snapshot_invalidation.thread-${suffix}.json" \
    "state/observe/client_budget_surfaces_cache.thread-${suffix}.json" \
    "state/observe/client_budget_gate_cache.thread-${suffix}.json"
}

purge_shared_token_budget_caches() {
  rm -f \
    state/token_budget/dashboard_assistant_scope_cache.json \
    state/token_budget/dashboard_assistant_scope_source_cache.json \
    state/token_budget/dashboard_current_session_events_cache.json \
    state/token_budget/dashboard_same_meter_sync_cache.json \
    state/token_budget/dashboard_token_events_cache.json \
    state/token_budget/dashboard_token_events_invalidation.json \
    state/token_budget/exact_client_limits_cache.json \
    state/token_budget/live_turn_retrieval_context_pack_cache.json \
    state/token_budget/live_turn_retrieval_context_pack_invalidation.json
}

sqlite3 ~/.codex/state_5.sqlite \
  "INSERT INTO threads (
      id,
      rollout_path,
      created_at,
      updated_at,
      source,
      model_provider,
      cwd,
      title,
      sandbox_policy,
      approval_mode,
      tokens_used,
      has_user_event,
      archived,
      cli_version,
      first_user_message,
      memory_mode
    ) VALUES (
      '$thread_id',
      '$rollout_path',
      strftime('%s','now'),
      strftime('%s','now'),
      'codex',
      'openai',
      '/home/art/agent-memory-index',
      'proof art continuity live turn',
      'danger-full-access',
      'never',
      0,
      1,
      0,
      '',
      'proof art continuity live turn',
      'enabled'
    );" >/dev/null

write_rollout_file 1200 0

CODEX_THREAD_ID="$thread_id" AMAI_AGENT_SCOPE="proof_art_continuity_live_turn" \
  ./target/release/amai context pack \
    --project art \
    --namespace continuity \
    --query "Continuity snapshot" \
    --disable-cache \
    --retrieval-mode local_strict \
    --token-source-kind "$live_source_kind" >"$output_path"

export OUTPUT_PATH="$output_path"
python3 - <<'PY'
import json
import os
from pathlib import Path

payload = json.loads(Path(os.environ["OUTPUT_PATH"]).read_text())
assert payload["project"]["code"] == "art", payload["project"]
assert payload["namespace"]["code"] == "continuity", payload["namespace"]
retrieval = payload["retrieval"]
assert (
    len(retrieval["exact_documents"]) >= 1
    or len(retrieval["lexical_chunks"]) >= 1
    or len(retrieval["semantic_chunks"]) >= 1
    or len(retrieval["symbol_hits"]) >= 1
), retrieval
PY

context_pack_id="$(jq -r '.context_pack_id' "$output_path")"
read -r naive_tokens context_tokens tool_overhead_tokens <<<"$(wait_for_token_metrics "$live_source_kind" "$context_pack_id")"
actual_total=$((24 + 28 + context_tokens + tool_overhead_tokens))

write_rollout_file "$actual_total" 1
mv "$rollout_path" "$completed_rollout_path"
update_thread_rollout_path "$thread_id" "$completed_rollout_path"
purge_thread_budget_caches "$thread_id"
purge_shared_token_budget_caches

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/dev/null

wait_for_current_live_turn_snapshot "$snapshot_path" "$thread_id" "$turn_id"

./target/release/amai observe repair-token-ledger \
  --apply \
  --limit 256 \
  --source-kind "$live_source_kind" \
  --correlation-id "$context_pack_id" \
  --rewrite-source-kind "$repaired_source_kind" \
  --repair-reason "proof_art_continuity_live_turn_cleanup" >/dev/null

printf 'art_continuity\t%s\t%s\t%s\t%s\n' \
  "$naive_tokens" \
  "$context_tokens" \
  "$tool_overhead_tokens" \
  "$actual_total"
printf 'proof_token_art_live_turn_savings: PASS\n'
