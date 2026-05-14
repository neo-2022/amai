#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command_path="${repo_root}/scripts/run_mcp_stdio.sh"

write_mcp() {
  local output="$1"
  mkdir -p "$(dirname "${output}")"
  python3 - "$output" "$command_path" "$repo_root" <<'PY'
import json
import pathlib
import sys

output = pathlib.Path(sys.argv[1])
command_path = sys.argv[2]
repo_root = sys.argv[3]

payload = {"servers": {}}
if output.is_file():
    try:
        payload = json.loads(output.read_text(encoding="utf-8"))
    except Exception:
        payload = {"servers": {}}
if not isinstance(payload, dict):
    payload = {"servers": {}}
servers = payload.get("servers")
if not isinstance(servers, dict):
    payload["servers"] = {}
servers = payload["servers"]

servers["amai"] = {
    "type": "stdio",
    "command": command_path,
    "args": [],
    "cwd": repo_root,
}

output.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY
  printf 'Amai MCP user config ensured: %s\n' "${output}"
}

write_mcp "${HOME}/.config/Code/User/mcp.json"
write_mcp "${HOME}/.config/VSCodium/User/mcp.json"
write_mcp "${HOME}/.vscode-oss/User/mcp.json"
