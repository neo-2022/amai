#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

source_kind="proof_rollout_assistant_generation_$(date +%s%N)"
context_pack_raw="/tmp/amai-proof-token-rollout-context-pack.json"
rollout_path="/tmp/amai-proof-token-rollout-assistant-generation.jsonl"
attach_path="/tmp/amai-proof-token-rollout-assistant-generation-attach.json"
report_path="/tmp/amai-proof-token-rollout-assistant-generation-report.json"

./target/release/amai context pack \
  --project art \
  --query "rollout assistant generation proof" \
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

CONTEXT_PACK_ID="$context_pack_id" ROLLOUT_PATH="$rollout_path" python3 - <<'PY'
import json
import os
from pathlib import Path

context_pack_id = os.environ["CONTEXT_PACK_ID"]
rows = [
    {
        "timestamp": "2026-03-25T10:00:00Z",
        "type": "event_msg",
        "payload": {"type": "task_started", "turn_id": "turn-proof-1"},
    },
    {
        "timestamp": "2026-03-25T10:00:01Z",
        "type": "response_item",
        "payload": {
            "type": "function_call_output",
            "output": json.dumps({
                "stats": {"context_pack_id": context_pack_id},
                "nested": [{"context_pack_id": context_pack_id}],
            }),
        },
    },
    {
        "timestamp": "2026-03-25T10:00:02Z",
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {"last_token_usage": {"output_tokens": 17}},
        },
    },
    {
        "timestamp": "2026-03-25T10:00:03Z",
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {"last_token_usage": {"output_tokens": 23}},
        },
    },
    {
        "timestamp": "2026-03-25T10:00:04Z",
        "type": "event_msg",
        "payload": {"type": "task_complete", "turn_id": "turn-proof-1"},
    },
]
Path(os.environ["ROLLOUT_PATH"]).write_text(
    "\n".join(json.dumps(row, ensure_ascii=False) for row in rows) + "\n"
)
PY

./target/release/amai observe token-rollout-assistant-generation \
  --rollout-path "$rollout_path" \
  --repo-root /home/art/Art \
  --apply >"$attach_path"

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >"$report_path"

SOURCE_KIND="$source_kind" ATTACH_PATH="$attach_path" REPORT_PATH="$report_path" python3 - <<'PY'
import json
import os
from pathlib import Path

attach = json.loads(Path(os.environ["ATTACH_PATH"]).read_text())
observation = attach["rollout_assistant_generation_observation"]
candidate = observation["candidate"]

assert observation["apply_requested"] is True, observation
assert observation["applied"] is True, observation
assert candidate["assistant_generation_tokens"] == 40, candidate
assert candidate["token_count_events"] == 2, candidate
assert candidate["context_pack_id"], candidate

report = json.loads(Path(os.environ["REPORT_PATH"]).read_text())["token_budget_report"]
source_kind = os.environ["SOURCE_KIND"]
entry = next(
    item for item in report["source_breakdown"] if item["source_kind"] == source_kind
)
summary = entry["summary"]

assert summary["events_total"] >= 1, summary
assert summary["observed_assistant_generation_tokens"] == 40, summary
PY

printf 'proof_token_rollout_assistant_generation: PASS\n'
