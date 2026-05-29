#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release --quiet

python3 <<'PY'
import json
import os
import select
import subprocess
import sys
from pathlib import Path

repo_root = Path.cwd()
env = os.environ.copy()
env["AMAI_FORCE_CONTINUITY_STARTUP_STALE_IMPORT_MISS"] = "1"
env["AMAI_CLIENT_KEY"] = "codex"

proc = subprocess.Popen(
    [str(repo_root / "target/release/amai"), "mcp", "serve"],
    cwd=repo_root,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=None,
    text=True,
    env=env,
)


def fail(message: str) -> None:
    proc.kill()
    proc.wait(timeout=5)
    raise SystemExit(message)


def request(req_id: int, method: str, params: dict) -> dict:
    payload = {
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    }
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()
    ready, _, _ = select.select([proc.stdout], [], [], 30)
    if not ready:
        fail(f"timed out waiting for MCP response to {method}")
    line = proc.stdout.readline()
    if not line:
        fail(f"no MCP response for {method}")
    response = json.loads(line)
    if response.get("id") != req_id:
        fail(f"MCP response id mismatch for {method}: {response}")
    if "error" in response:
        fail(f"MCP request {method} failed: {response['error']}")
    return response["result"]


try:
    init = request(
        1,
        "initialize",
        {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "proof-mcp-reconcile", "version": "1"},
        },
    )
    if init["serverInfo"]["name"] != "Art-memory-agent-index":
        fail(f"unexpected MCP server: {init}")

    proc.stdin.write(
        json.dumps(
            {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}
        )
        + "\n"
    )
    proc.stdin.flush()

    result = request(
        2,
        "tools/call",
        {
            "name": "amai_continuity_startup",
            "arguments": {
                "project": "amai",
                "repo_root": str(repo_root),
                "namespace": "continuity",
                "token_source_kind": "proof_mcp_continuity_startup_reconcile",
            },
        },
    )
    if result.get("isError") is True:
        fail(f"tool returned isError=true: {result}")
    content = result.get("structuredContent", {})
    reconcile = content.get("tool_runtime_reconcile", {})
    if reconcile.get("applied") is not True:
        fail(f"tool_runtime_reconcile.applied not true: {content}")
    if reconcile.get("classification") != "stale_embedded_mcp_session":
        fail(f"unexpected reconcile classification: {content}")
    if reconcile.get("continue_from_local_startup_payload") is not True:
        fail(f"tool did not continue from local startup payload: {content}")
    if reconcile.get("mcp_reconnect_required") is not True:
        fail(f"tool did not require MCP reconnect after reconcile: {content}")
    reconnect_helper = reconcile.get("reconnect_helper", {})
    if reconnect_helper.get("preferred_client_key") != "codex":
        fail(f"unexpected reconnect helper client: {content}")
    if reconnect_helper.get("shell_helper_command") != "./scripts/reconnect_local.sh --client codex":
        fail(f"unexpected reconnect shell helper command: {content}")
    if reconnect_helper.get("bootstrap_command") != "./scripts/amai_exec.sh bootstrap reconnect --client codex --yes":
        fail(f"unexpected reconnect bootstrap command: {content}")
    if reconnect_helper.get("peer_session_safety") != "orphan_only_cleanup_no_disconnect":
        fail(f"unexpected reconnect safety contract: {content}")
    if content["continuity_startup"]["project"]["code"] != "amai":
        fail(f"unexpected project in structuredContent: {content}")
    if not content["chat_start_restore"]["prompt_text"].startswith("CHAT_START_RESTORE"):
        fail(f"startup prompt missing after reconcile: {content}")
finally:
    proc.kill()
    proc.wait(timeout=5)
PY

echo "proof_mcp_continuity_startup_reconcile: PASS"
