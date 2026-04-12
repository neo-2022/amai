#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT
proof_json="${tmp_dir}/mcp_zero_roundtrip_stop_loss.json"

python3 - <<'PY' >"${proof_json}"
import json
import subprocess
import time

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
            "query": "final live advisory-only check",
            "token_source_kind": "proof_mcp_context_pack",
            "persist": False,
        },
        },
    },
]

responses = []
deadline = time.time() + 20
for message in messages:
    proc.stdin.write(json.dumps(message) + "\n")
    proc.stdin.flush()

while len(responses) < 2 and time.time() < deadline:
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
  (.responses | length) == 2
  and .stderr == ""
  and
  .responses[1].result.isError != true
  and .responses[1].result.structuredContent != null
  and .responses[1].result.structuredContent.context_pack.project.code == "amai"
' "${proof_json}" >/dev/null

printf 'proof_mcp_client_budget_zero_roundtrip_stop_loss: PASS\n'
