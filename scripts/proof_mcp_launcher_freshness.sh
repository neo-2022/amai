#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bootstrap_stack.sh >/dev/null

if ! command -v cargo >/dev/null 2>&1; then
  echo "proof_mcp_launcher_freshness: SKIP (cargo not available)" >&2
  exit 0
fi

cargo build --release --quiet

real_cargo="$(command -v cargo)"
shim_dir="$(mktemp -d)"
marker_file="$(mktemp)"
cleanup() {
  rm -rf "${shim_dir}"
  rm -f "${marker_file}"
}
trap cleanup EXIT

cat >"${shim_dir}/cargo" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >"${marker_file}"
exec "${real_cargo}" "\$@"
EOF
chmod +x "${shim_dir}/cargo"
export AMAI_PROOF_SHIM_DIR="${shim_dir}"
export AMAI_PROOF_MARKER_FILE="${marker_file}"

python3 - <<'PY'
import json
import os
import subprocess
import sys

shim_dir = os.environ["AMAI_PROOF_SHIM_DIR"]
marker_file = os.environ["AMAI_PROOF_MARKER_FILE"]
env = os.environ.copy()
env["PATH"] = shim_dir + os.pathsep + env["PATH"]

proc = subprocess.Popen(
    ["./scripts/run_mcp_stdio.sh"],
    cwd="/home/art/agent-memory-index",
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    env=env,
)

def request(req_id, method, params):
    payload = {
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    }
    proc.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    if not line:
        stderr = proc.stderr.read()
        raise RuntimeError(f"runner returned no response; stderr={stderr!r}")
    response = json.loads(line)
    if response.get("id") != req_id:
        raise RuntimeError(f"unexpected response id: {response!r}")
    return response

try:
    init = request(
        1,
        "initialize",
        {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "proof-mcp-launcher-freshness", "version": "1.0"},
        },
    )
    server = init["result"]["serverInfo"]
    if server["name"] != "Art-memory-agent-index":
        raise RuntimeError(f"unexpected server info: {server!r}")

    proc.stdin.write(
        json.dumps(
            {
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {},
            },
            ensure_ascii=False,
        )
        + "\n"
    )
    proc.stdin.flush()

    startup = request(
        2,
        "tools/call",
        {
            "name": "amai_continuity_startup",
            "arguments": {
                "project": "bug_bounty",
                "namespace": "continuity",
                "token_source_kind": "proof_mcp_continuity_startup",
            },
        },
    )
    result = startup["result"]
    if result.get("isError"):
        raise RuntimeError(f"startup unexpectedly failed: {result!r}")
    structured = result["structuredContent"]
    summary = structured["continuity_startup_summary"]
    if summary["project_code"] != "bug_bounty":
        raise RuntimeError(f"wrong project restored: {summary!r}")
    if summary["namespace_code"] != "continuity":
        raise RuntimeError(f"wrong namespace restored: {summary!r}")
    if summary["prompt_text_present"] is not True:
        raise RuntimeError(f"prompt_text missing: {summary!r}")

    default_startup = request(
        3,
        "tools/call",
        {
            "name": "amai_continuity_startup",
            "arguments": {
                "project": "bug_bounty",
                "namespace": "default",
                "token_source_kind": "proof_mcp_continuity_startup",
            },
        },
    )
    default_result = default_startup["result"]
    if default_result.get("isError"):
        raise RuntimeError(f"default namespace startup failed: {default_result!r}")
    default_structured = default_result["structuredContent"]
    default_summary = default_structured["continuity_startup_summary"]
    if default_summary["project_code"] != "bug_bounty":
        raise RuntimeError(f"default startup lost project binding: {default_summary!r}")
    if default_summary["prompt_text_present"] is not True:
        raise RuntimeError(f"default startup lost prompt_text: {default_summary!r}")

    continuity_source = default_structured["continuity_startup"].get("continuity_source", {})
    source_mode = continuity_source.get("mode")
    if source_mode not in {
        "scoped_import",
        "continuity_namespace_fallback_import",
        "working_state_fallback",
    }:
        raise RuntimeError(f"unexpected default continuity source: {continuity_source!r}")

    with open(marker_file, "r", encoding="utf-8") as fh:
        cargo_invocation = fh.read().strip()
    expected_prefix = "run --release --quiet -- mcp serve"
    if cargo_invocation != expected_prefix:
        raise RuntimeError(
            f"runner did not choose cargo path first: {cargo_invocation!r}"
        )
finally:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)

print("proof_mcp_launcher_freshness: PASS")
PY
