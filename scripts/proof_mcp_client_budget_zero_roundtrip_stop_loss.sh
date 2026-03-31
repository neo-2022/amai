#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CACHE_PATH="${REPO_ROOT}/state/observe/client_budget_gate_cache.json"

tmp_dir="$(mktemp -d)"
restore_cache() {
  if [[ -f "${tmp_dir}/client_budget_gate_cache.json.bak" ]]; then
    mv "${tmp_dir}/client_budget_gate_cache.json.bak" "${CACHE_PATH}"
  else
    rm -f "${CACHE_PATH}"
  fi
  rm -rf "${tmp_dir}"
}
trap restore_cache EXIT

mkdir -p "$(dirname "${CACHE_PATH}")"
if [[ -f "${CACHE_PATH}" ]]; then
  cp "${CACHE_PATH}" "${tmp_dir}/client_budget_gate_cache.json.bak"
fi

now_epoch_ms="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"

cat >"${CACHE_PATH}" <<EOF
{
  "cache_version": "client-budget-gate-cache-v1",
  "fetched_at_epoch_ms": ${now_epoch_ms},
  "gate": {
    "client_budget_reply_gate": {
      "status_label": "новый чат нужен сейчас",
      "reply_execution_gate": {
        "action_kind": "rotate_chat_for_client_budget",
        "blocking": false,
        "must_rotate_before_reply": false,
        "must_wait_for_budget_recovery_before_reply": false,
        "reply_budget_mode": "compact_high_signal",
        "reply_prefix": "5ч KPI: переплата 8.00%",
        "same_meter_pure_burn_turn_active": false,
        "must_avoid_new_tool_turn_without_specific_delta_goal": true,
        "max_tool_roundtrips_soft": 0
      }
    }
  },
  "guard": {
    "status_label": "новый чат нужен сейчас",
    "reply_prefix": "5ч KPI: переплата 8.00%",
    "observed_at_epoch_ms": ${now_epoch_ms},
    "max_guard_age_seconds": 10,
    "last_request": "88234 из 258400",
    "client_limits": "5ч остаётся 65.85%, 7д остаётся 70.00%",
    "reply_execution_gate": {
      "action_kind": "rotate_chat_for_client_budget",
      "blocking": false,
      "must_rotate_before_reply": false,
      "must_wait_for_budget_recovery_before_reply": false,
      "reply_budget_mode": "compact_high_signal",
      "reply_prefix": "5ч KPI: переплата 8.00%",
      "same_meter_pure_burn_turn_active": false,
      "must_avoid_new_tool_turn_without_specific_delta_goal": true,
      "max_tool_roundtrips_soft": 0,
      "preserves_return_obligation": true,
      "blocking_reply_contract": null,
      "action_bundle": {
        "preserves_return_obligation": true
      }
    }
  }
}
EOF

proof_json="${tmp_dir}/mcp_zero_roundtrip_stop_loss.json"

python3 - <<'PY' >"${proof_json}"
import json
import subprocess

proc = subprocess.Popen(
    ["./scripts/run_mcp_stdio.sh"],
    cwd="/home/art/agent-memory-index",
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)

messages = [
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "proof-mcp-zero-roundtrip-stop-loss", "version": "1.0"},
        },
    },
    {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}},
    {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "amai_context_pack",
            "arguments": {
                "project": "amai",
                "namespace": "continuity",
                "query": "proof zero roundtrip stop loss",
                "token_source_kind": "proof_mcp_context_pack",
                "persist": False,
            },
        },
    },
]

responses = []
for message in messages:
    proc.stdin.write(json.dumps(message) + "\n")
    proc.stdin.flush()

while len(responses) < 2:
    line = proc.stdout.readline()
    if not line:
        break
    payload = json.loads(line)
    if payload.get("id") in (1, 2):
        responses.append(payload)

proc.kill()
stderr = proc.stderr.read()

print(json.dumps({"responses": responses, "stderr": stderr}, ensure_ascii=False))
PY

jq -e '
  .responses[1].result.isError == true
  and .responses[1].result.structuredContent.error_taxonomy.amai_error_code == "tool_blocked_by_live_client_budget_gate"
  and .responses[1].result.structuredContent.same_meter_pure_burn_turn_active == false
  and .responses[1].result.structuredContent.expensive_tool_turn_stop_loss_active == true
  and .responses[1].result.structuredContent.expensive_tool_turn_stop_loss_reason == "zero_tool_roundtrips_live_gate"
  and .responses[1].result.structuredContent.client_budget_reply_gate.reply_execution_gate.max_tool_roundtrips_soft == 0
  and (.responses[1].result.content[0].text | contains("rotate into a fresh chat before retrying this tool"))
' "${proof_json}" >/dev/null

printf 'proof_mcp_client_budget_zero_roundtrip_stop_loss: PASS\n'
