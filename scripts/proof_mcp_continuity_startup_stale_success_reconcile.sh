#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release --quiet

state_file=".amai/continuity/project-chat-startup-state.json"
mkdir -p state/locks
exec 9>state/locks/startup_runtime_state_mutation.lock
flock --exclusive 9

./scripts/continuity_startup.sh --repo-root "$(pwd)" --namespace continuity --json >/dev/null

tmp_dir="$(mktemp -d)"
backup_state="${tmp_dir}/project-chat-startup-state.backup.json"
cp "${state_file}" "${backup_state}"
trap 'cp "${backup_state}" "${state_file}"; rm -rf "${tmp_dir}"' EXIT

python3 <<'PY'
import json
import os
import select
import subprocess
from pathlib import Path

repo_root = Path.cwd()
env = os.environ.copy()
env["AMAI_CLIENT_KEY"] = "codex"
env["AMAI_FORCE_CONTINUITY_STARTUP_STALE_RUNTIME_ARTIFACT_AFTER_SUCCESS"] = "once"

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
    payload = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
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
            "clientInfo": {"name": "proof-mcp-stale-success", "version": "1"},
        },
    )
    if init["serverInfo"]["name"] != "Art-memory-agent-index":
        fail(f"unexpected MCP server: {init}")

    proc.stdin.write(
        json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}) + "\n"
    )
    proc.stdin.flush()

    def assert_reconciled(req_id: int, arguments: dict, expect_reconcile: bool) -> None:
        result = request(
            req_id,
            "tools/call",
            {
                "name": "amai_continuity_startup",
                "arguments": arguments,
            },
        )
        if result.get("isError") is True:
            fail(f"tool returned isError=true: {result}")
        content = result.get("structuredContent", {})
        stale_markers = {
            "STALE PREVIEW HEADLINE MUST NOT LEAK",
            "STALE PREVIEW NEXT STEP MUST NOT LEAK",
        }
        observed_values = [
            content.get("continuity_startup_summary", {}).get("headline"),
            content.get("continuity_startup_summary", {}).get("next_step"),
            content.get("continuity_startup", {}).get("handoff_summary", {}).get("headline"),
            content.get("continuity_startup", {}).get("handoff_summary", {}).get("next_step"),
        ]
        for observed in observed_values:
            if observed in stale_markers:
                fail(f"stale preview marker leaked into startup payload: {content}")
        reconcile = content.get("tool_runtime_reconcile", {})
        if expect_reconcile:
            if reconcile.get("applied") is not True:
                fail(f"tool_runtime_reconcile.applied not true: {content}")
            if reconcile.get("classification") != "stale_embedded_mcp_session":
                fail(f"unexpected reconcile classification: {content}")
            if reconcile.get("continue_from_local_startup_payload") is not True:
                fail(f"tool did not continue from local startup payload: {content}")
            if reconcile.get("same_session_continuation_allowed") is not True:
                fail(f"tool did not allow same-session continuation after stale-success reconcile: {content}")
            if reconcile.get("operator_action_required") is not False:
                fail(f"tool still required operator action after stale-success reconcile: {content}")
            if reconcile.get("mcp_reconnect_required") is not False:
                fail(f"tool still marked reconnect as required after stale-success reconcile: {content}")
            if reconcile.get("reconnect_helper_diagnostic_only") is not True:
                fail(f"tool did not keep reconnect helper diagnostic-only after stale-success reconcile: {content}")
            if content.get("continuity_startup_summary", {}).get("headline") != content.get("continuity_startup", {}).get("handoff_summary", {}).get("headline"):
                fail(f"summary headline diverged from public handoff summary after reconcile: {content}")
            if content.get("continuity_startup_summary", {}).get("next_step") != content.get("continuity_startup", {}).get("handoff_summary", {}).get("next_step"):
                fail(f"summary next_step diverged from public handoff summary after reconcile: {content}")
        elif reconcile not in ({}, None):
            fail(f"unexpected second-pass reconcile after same-session self-heal: {content}")

    assert_reconciled(
        2,
        {
            "project": "amai",
            "repo_root": str(repo_root),
            "namespace": "continuity",
            "token_source_kind": "proof_mcp_continuity_startup_stale_success_reconcile",
        },
        True,
    )
    assert_reconciled(
        3,
        {
            "project": "amai",
            "repo_root": str(repo_root),
            "namespace": "continuity",
            "token_source_kind": "proof_mcp_continuity_startup_stale_success_reconcile_same_session_followup",
        },
        False,
    )
    assert_reconciled(
        4,
        {
            "project": "amai",
            "namespace": "continuity",
            "token_source_kind": "proof_mcp_continuity_startup_stale_success_reconcile_project_only_followup",
        },
        False,
    )
finally:
    proc.kill()
    proc.wait(timeout=5)
PY

restored_output="$(./scripts/continuity_startup_state.sh --repo-root "$(pwd)" --json)"
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.gate_semantics_consistent == true' >/dev/null
printf '%s\n' "${restored_output}" | jq -e '.startup_runtime_state.workflow_promotion_state.source_event_match == true' >/dev/null

echo "proof_mcp_continuity_startup_stale_success_reconcile: PASS"
