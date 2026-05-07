#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

amai_bin="$repo_root/target/debug/amai"
if [[ ! -x "$amai_bin" ]]; then
  cargo build --quiet >/dev/null
fi

vscode_output="$tmp_dir/vscode-mcp.json"
"$amai_bin" mcp config \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$vscode_output" >/dev/null
grep -q '"command": "ssh"' "$vscode_output"
grep -q 'ops@example-host' "$vscode_output"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$vscode_output"

cursor_output="$tmp_dir/cursor-mcp.json"
"$amai_bin" mcp config \
  --client cursor \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$cursor_output" >/dev/null
grep -q '"command": "ssh"' "$cursor_output"
grep -q 'ops@example-host' "$cursor_output"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$cursor_output"

claude_code_output="$tmp_dir/claude-code-mcp.json"
"$amai_bin" mcp config \
  --client claude-code \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$claude_code_output" >/dev/null
grep -q '"command": "ssh"' "$claude_code_output"
grep -q 'ops@example-host' "$claude_code_output"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$claude_code_output"

codex_output="$tmp_dir/codex-mcp.toml"
"$amai_bin" mcp config \
  --client codex \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$codex_output" >/dev/null
grep -q 'command = "ssh"' "$codex_output"
grep -q 'ops@example-host' "$codex_output"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$codex_output"

hermes_output="$tmp_dir/hermes-mcp.yaml"
"$amai_bin" mcp config \
  --client hermes \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$hermes_output" >/dev/null
grep -q "command: 'ssh'" "$hermes_output"
grep -q "ops@example-host" "$hermes_output"
grep -q "cd ''/srv/amai'' && ./scripts/run_mcp_stdio.sh" "$hermes_output"

openclaw_output="$tmp_dir/openclaw-mcp.json"
"$amai_bin" mcp config \
  --client openclaw \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$openclaw_output" >/dev/null
grep -q '"command": "ssh"' "$openclaw_output"
grep -q 'ops@example-host' "$openclaw_output"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$openclaw_output"

echo "proof_remote_ssh_config: ok"
