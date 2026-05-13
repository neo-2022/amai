#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

if [[ ! -x ./target/release/amai ]]; then
  cargo build --release --quiet
fi

proof_id="$(date +%s%N)"
temp_root="$(mktemp -d /tmp/amai-proof-live-turn-matrix.XXXXXX)"
alpha_project_code="proof_shape_alpha_${proof_id}"
beta_project_code="proof_shape_beta_${proof_id}"
declare -a cleanup_threads=()

cleanup() {
  local thread_id
  for thread_id in "${cleanup_threads[@]:-}"; do
    sqlite3 ~/.codex/state_5.sqlite "DELETE FROM threads WHERE id = '$thread_id';" >/dev/null 2>&1 || true
  done
  rm -rf "$temp_root"
}
trap cleanup EXIT

write_rollout_file() {
  local rollout_path="$1"
  local timestamp_rfc3339="$2"
  local turn_id="$3"
  local prompt_tokens="$4"
  local assistant_tokens="$5"
  local total_tokens="$6"
  local include_complete="$7"
  export ROLLOUT_PATH="$rollout_path"
  export TURN_TIMESTAMP="$timestamp_rfc3339"
  export TURN_ID="$turn_id"
  export PROMPT_TOKENS="$prompt_tokens"
  export ASSISTANT_TOKENS="$assistant_tokens"
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
                    "input_tokens": int(os.environ["PROMPT_TOKENS"]),
                    "cached_input_tokens": 0,
                    "output_tokens": int(os.environ["ASSISTANT_TOKENS"]),
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

insert_thread_row() {
  local thread_id="$1"
  local rollout_path="$2"
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
        'proof live turn savings matrix',
        'danger-full-access',
        'never',
        0,
        1,
        0,
        '',
        'proof live turn matrix',
        'enabled'
      );" >/dev/null
  cleanup_threads+=("$thread_id")
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
  row="$(fetch_token_metrics "$source_kind" "$context_pack_id")"
  if [[ -z "$row" ]]; then
    printf 'missing token metrics for source_kind=%s context_pack_id=%s\n' \
      "$source_kind" "$context_pack_id" >&2
    exit 1
  fi
  printf '%s\n' "$row"
}

assert_shape_output() {
  local output_path="$1"
  local shape="$2"
  local expected_exact="$3"
  local expected_symbol="$4"
  local expected_lexical="$5"
  local expected_semantic="$6"
  export OUTPUT_PATH="$output_path"
  export SHAPE="$shape"
  export EXPECTED_EXACT="$expected_exact"
  export EXPECTED_SYMBOL="$expected_symbol"
  export EXPECTED_LEXICAL="$expected_lexical"
  export EXPECTED_SEMANTIC="$expected_semantic"
  python3 - <<'PY'
import json
import os
from pathlib import Path

payload = json.loads(Path(os.environ["OUTPUT_PATH"]).read_text())
retrieval = payload["retrieval"]
actual = {
    "exact": len(retrieval["exact_documents"]),
    "symbol": len(retrieval["symbol_hits"]),
    "lexical": len(retrieval["lexical_chunks"]),
    "semantic": len(retrieval["semantic_chunks"]),
}
expected = {
    "exact": int(os.environ["EXPECTED_EXACT"]),
    "symbol": int(os.environ["EXPECTED_SYMBOL"]),
    "lexical": int(os.environ["EXPECTED_LEXICAL"]),
    "semantic": int(os.environ["EXPECTED_SEMANTIC"]),
}
assert actual == expected, {
    "shape": os.environ["SHAPE"],
    "expected": expected,
    "actual": actual,
}
PY
}

assert_current_live_turn_snapshot() {
  local snapshot_path="$1"
  local shape="$2"
  local thread_id="$3"
  local turn_id="$4"
  export SNAPSHOT_PATH="$snapshot_path"
  export SHAPE="$shape"
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
assert current["exact_pair"]["saved_pct"] >= 90.0, {
    "shape": os.environ["SHAPE"],
    "saved_pct": current["exact_pair"]["saved_pct"],
    "exact_pair": current["exact_pair"],
}
PY
}

wait_for_current_live_turn_snapshot() {
  local snapshot_path="$1"
  local shape="$2"
  local thread_id="$3"
  local turn_id="$4"
  local attempt=""
  for attempt in {1..16}; do
    timeout 2s ./target/release/amai observe token-report \
      --budget-profile codex_5h \
      --include-verify-events true >/dev/null || true
    CODEX_THREAD_ID="$thread_id" AMAI_AGENT_SCOPE="proof_live_turn_matrix" \
      timeout 20s ./target/release/amai observe snapshot >"$snapshot_path" || true
    if [[ ! -s "$snapshot_path" ]]; then
      sleep 2
      continue
    fi
    if assert_current_live_turn_snapshot "$snapshot_path" "$shape" "$thread_id" "$turn_id" \
      >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  if [[ ! -s "$snapshot_path" ]]; then
    printf 'observe snapshot did not produce JSON for shape=%s thread_id=%s turn_id=%s\n' \
      "$shape" "$thread_id" "$turn_id" >&2
    return 1
  fi
  assert_current_live_turn_snapshot "$snapshot_path" "$shape" "$thread_id" "$turn_id"
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

repair_live_source_kind() {
  local live_source_kind="$1"
  local context_pack_id="$2"
  local repaired_source_kind="$3"
  local repair_reason="$4"
  ./target/release/amai observe repair-token-ledger \
    --apply \
    --limit 256 \
    --source-kind "$live_source_kind" \
    --correlation-id "$context_pack_id" \
    --rewrite-source-kind "$repaired_source_kind" \
    --repair-reason "$repair_reason" >/dev/null
}

prepare_fixture_projects() {
  local alpha_root="$temp_root/alpha"
  local beta_root="$temp_root/beta"
  mkdir -p "$alpha_root/src" "$alpha_root/docs" "$beta_root/src"
  cat >"$alpha_root/Cargo.toml" <<'EOF'
[package]
name = "proof_alpha"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
EOF
  cat >"$beta_root/Cargo.toml" <<'EOF'
[package]
name = "proof_beta"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
EOF
  export ALPHA_ROOT="$alpha_root"
  export BETA_ROOT="$beta_root"
  python3 - <<'PY'
from pathlib import Path
import os

alpha_root = Path(os.environ["ALPHA_ROOT"])
beta_root = Path(os.environ["BETA_ROOT"])
filler = "\n".join(
    f"// filler context line {index}: durable orchestration ledger and checkpoint audit trail"
    for index in range(1600)
)
alpha_root.joinpath("src/lib.rs").write_text(
    f"""pub const EXACT_NEEDLE: &str = "exact needle orbit bridge";

/// symbol only anchor for proof retrieval
pub fn symbol_only_navigation_runtime_checkpoint() -> &'static str {{
    "symbol checkpoint anchor"
}}

/* lexical anchor: lexical bridge phrase copper canyon lantern */
/* semantic anchor: operators recover continuity from a durable resume queue after restart and rebuild progress from preserved markers */
/* hybrid shared anchor: hybrid orbit shared runtime marker */
{filler}
""",
    encoding="utf-8",
)
beta_root.joinpath("src/lib.rs").write_text(
    f"""pub const HYBRID_SHARED_MARKER: &str = "hybrid orbit shared runtime marker";

/* related evidence: downstream runtime mirror tracks shared runtime marker across related project contours */
{filler}
""",
    encoding="utf-8",
)
alpha_root.joinpath("docs/exact-needle.md").write_text(
    "# Exact needle\\n\\nexact needle orbit bridge\\n",
    encoding="utf-8",
)
PY

  ./target/release/amai project register \
    --code "$alpha_project_code" \
    --display-name "Proof Shape Alpha" \
    --repo-root "$alpha_root" >/dev/null
  ./target/release/amai project register \
    --code "$beta_project_code" \
    --display-name "Proof Shape Beta" \
    --repo-root "$beta_root" >/dev/null
  ./target/release/amai namespace ensure \
    --project "$alpha_project_code" \
    --code review \
    --display-name Review \
    --retrieval-mode local_plus_related >/dev/null
  ./target/release/amai namespace ensure \
    --project "$beta_project_code" \
    --code review \
    --display-name Review \
    --retrieval-mode local_plus_related >/dev/null
  ./target/release/amai relation add \
    --source "$alpha_project_code" \
    --target "$beta_project_code" \
    --relation-type shared_runtime \
    --shared-contour live_turn_shape \
    --access-mode local_plus_related >/dev/null
  ./target/release/amai index project \
    --code "$alpha_project_code" \
    --path "$alpha_root" \
    --namespace review >/dev/null
  ./target/release/amai index project \
    --code "$beta_project_code" \
    --path "$beta_root" \
    --namespace review >/dev/null
}

run_case() {
  local shape="$1"
  local project="$2"
  local namespace="$3"
  local query="$4"
  local retrieval_mode="$5"
  local limit_documents="$6"
  local limit_symbols="$7"
  local limit_chunks="$8"
  local limit_semantic_chunks="$9"
  local expected_exact="${10}"
  local expected_symbol="${11}"
  local expected_lexical="${12}"
  local expected_semantic="${13}"
  local prompt_tokens="${14}"
  local assistant_tokens="${15}"

  local thread_id="matrix-live-turn-${proof_id}-${shape}"
  local rollout_path="$temp_root/${shape}.jsonl"
  local completed_rollout_path="$temp_root/${shape}-complete.jsonl"
  local output_path="$temp_root/${shape}.json"
  local snapshot_path="$temp_root/${shape}-snapshot.json"
  local turn_id="turn-${shape}"
  local live_source_kind="live_matrix_turn_${proof_id}_${shape}"
  local repaired_source_kind="proof_live_turn_matrix_${shape}"
  local timestamp_rfc3339
  timestamp_rfc3339="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  write_rollout_file \
    "$rollout_path" \
    "$timestamp_rfc3339" \
    "$turn_id" \
    "$prompt_tokens" \
    "$assistant_tokens" \
    "$((prompt_tokens + assistant_tokens + 512))" \
    "0"
  insert_thread_row "$thread_id" "$rollout_path"

  CODEX_THREAD_ID="$thread_id" AMAI_AGENT_SCOPE="proof_live_turn_matrix" \
    ./target/release/amai context pack \
      --project "$project" \
      --namespace "$namespace" \
      --query "$query" \
      --disable-cache \
      --retrieval-mode "$retrieval_mode" \
      --limit-documents "$limit_documents" \
      --limit-symbols "$limit_symbols" \
      --limit-chunks "$limit_chunks" \
      --limit-semantic-chunks "$limit_semantic_chunks" \
      --token-source-kind "$live_source_kind" >"$output_path"

  assert_shape_output \
    "$output_path" \
    "$shape" \
    "$expected_exact" \
    "$expected_symbol" \
    "$expected_lexical" \
    "$expected_semantic"

  local context_pack_id
  context_pack_id="$(jq -r '.context_pack_id' "$output_path")"
  local row
  row="$(wait_for_token_metrics "$live_source_kind" "$context_pack_id")"
  local naive_tokens=""
  local context_tokens=""
  local tool_overhead_tokens=""
  IFS=$'\t' read -r naive_tokens context_tokens tool_overhead_tokens <<<"$row"
  local actual_total=$((prompt_tokens + assistant_tokens + context_tokens + tool_overhead_tokens))

  write_rollout_file \
    "$completed_rollout_path" \
    "$timestamp_rfc3339" \
    "$turn_id" \
    "$prompt_tokens" \
    "$assistant_tokens" \
    "$actual_total" \
    "1"
  update_thread_rollout_path "$thread_id" "$completed_rollout_path"
  purge_thread_budget_caches "$thread_id"
  purge_shared_token_budget_caches

  ./target/release/amai observe token-report \
    --budget-profile codex_5h \
    --include-verify-events true >/dev/null

  wait_for_current_live_turn_snapshot "$snapshot_path" "$shape" "$thread_id" "$turn_id"

  repair_live_source_kind \
    "$live_source_kind" \
    "$context_pack_id" \
    "$repaired_source_kind" \
    "proof_live_turn_matrix_cleanup"

  printf '%s\t%s\t%s\t%s\t%s\n' \
    "$shape" \
    "$naive_tokens" \
    "$context_tokens" \
    "$tool_overhead_tokens" \
    "$actual_total"
}

prepare_fixture_projects

printf 'shape\tnaive_tokens\tcontext_tokens\ttool_overhead_tokens\twith_amai_total\n'
run_case exact "$alpha_project_code" review \
  "docs/exact-needle.md" \
  local_strict \
  2 0 0 0 \
  1 0 0 0 \
  20 24
run_case symbol "$alpha_project_code" review \
  "symbol_only_navigation_runtime_checkpoint" \
  local_strict \
  0 4 0 0 \
  0 1 0 0 \
  20 24
run_case lexical "$alpha_project_code" review \
  "lexical bridge phrase copper canyon lantern" \
  local_strict \
  0 0 4 0 \
  0 0 1 0 \
  20 24
run_case semantic "$alpha_project_code" review \
  "how do operators rebuild progress after a restart from preserved recovery markers" \
  local_strict \
  0 0 0 4 \
  0 0 0 1 \
  20 24
run_case hybrid "$alpha_project_code" review \
  "hybrid orbit shared runtime marker" \
  local_plus_related \
  2 0 4 0 \
  2 0 0 0 \
  24 28

printf 'proof_token_live_turn_savings_matrix: PASS\n'
