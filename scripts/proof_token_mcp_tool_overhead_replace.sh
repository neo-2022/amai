#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

if [[ ! -x ./target/release/amai ]]; then
  cargo build --release >/dev/null
fi

seed_id="$(date +%s%N)"
query="proof mcp tool overhead replace ${seed_id}"
source_kind="proof_mcp_tool_overhead_replace_${seed_id}"
cli_output="/tmp/amai-proof-token-mcp-tool-overhead-replace-cli.json"
mcp_output="/tmp/amai-proof-token-mcp-tool-overhead-replace-mcp.json"

./target/release/amai context pack \
  --project amai \
  --namespace continuity \
  --query "$query" \
  --limit-documents 4 \
  --limit-chunks 6 \
  --token-source-kind "$source_kind" >"$cli_output"

QUERY="$query" SOURCE_KIND="$source_kind" MCP_OUTPUT="$mcp_output" python3 - <<'PY'
import json
import os
import subprocess

repo = "/home/art/agent-memory-index"
query = os.environ["QUERY"]
source_kind = os.environ["SOURCE_KIND"]
output_path = os.environ["MCP_OUTPUT"]

proc = subprocess.Popen(
    ["./scripts/run_mcp_stdio.sh"],
    cwd=repo,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
)


def send(message):
    proc.stdin.write(json.dumps(message) + "\n")
    proc.stdin.flush()


def recv_for(target_id):
    while True:
        line = proc.stdout.readline()
        if not line:
            stderr = proc.stderr.read()
            raise RuntimeError(f"EOF waiting for MCP response id={target_id}; stderr={stderr}")
        line = line.strip()
        if not line:
            continue
        message = json.loads(line)
        if message.get("id") == target_id:
            return message


send(
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "proof-token-mcp-tool-overhead-replace", "version": "1.0"},
        },
    }
)
init = recv_for(1)
send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
send(
    {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "amai_context_pack",
            "arguments": {
                "project": "amai",
                "namespace": "continuity",
                "query": query,
                "token_source_kind": source_kind,
                "limit_chunks": 6,
                "limit_documents": 4,
            },
        },
    }
)
response = recv_for(2)
with open(output_path, "w", encoding="utf-8") as fh:
    json.dump({"initialize": init, "call": response}, fh)
proc.terminate()
try:
    proc.wait(timeout=3)
except subprocess.TimeoutExpired:
    proc.kill()
PY

QUERY="$query" SOURCE_KIND="$source_kind" CLI_OUTPUT="$cli_output" MCP_OUTPUT="$mcp_output" python3 - <<'PY'
import json
import os
import subprocess

repo = "/home/art/agent-memory-index"
query = os.environ["QUERY"]
source_kind = os.environ["SOURCE_KIND"]
cli_output = json.loads(open(os.environ["CLI_OUTPUT"], encoding="utf-8").read())
mcp_output = json.loads(open(os.environ["MCP_OUTPUT"], encoding="utf-8").read())

assert cli_output["context_pack_id"], cli_output

structured = mcp_output["call"]["result"]["structuredContent"]
assert mcp_output["call"].get("error") is None, mcp_output
assert mcp_output["call"]["result"].get("isError") is not True, mcp_output
assert structured["stats"]["cache_hit"] is True, structured
assert structured["stats"]["context_pack_id"], structured

sql = f"""
SELECT
  payload #>> '{{token_budget_event,context_pack_id}}' AS context_pack_id,
  payload #>> '{{token_budget_event,whole_cycle_observed,tool_overhead_tokens}}' AS tool_overhead,
  payload #>> '{{token_budget_event,whole_cycle_observed_source,tool_overhead,state}}' AS source_state
FROM ami.observability_snapshots
WHERE snapshot_kind='token_budget_event'
  AND payload #>> '{{token_budget_event,query}}' = '{query}'
  AND payload #>> '{{token_budget_event,source_kind}}' = '{source_kind}'
ORDER BY created_at DESC
LIMIT 2;
"""
psql = subprocess.run(
    ["bash", "-lc", f"source ./scripts/load_env.sh && psql \"$AMI_POSTGRES_DSN\" -At -F $'\\t' -c \"{sql}\""],
    cwd=repo,
    check=True,
    capture_output=True,
    text=True,
)
rows = [line.split("\t") for line in psql.stdout.splitlines() if line.strip()]
assert len(rows) >= 2, rows
latest = rows[0]
previous = rows[1]
assert latest[1].isdigit() and int(latest[1]) > 0, rows
assert latest[2] == "context_pack_mcp_structured_content_materialized", rows
assert previous[2] == "context_pack_cli_model_visible_output_materialized", rows
PY

printf 'proof_token_mcp_tool_overhead_replace: PASS\n'
