#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tmp_dir="$(mktemp -d)"
output_json="$tmp_dir/vscode-mcp.json"
output_toml="$tmp_dir/codex-mcp.toml"
trap 'rm -rf "$tmp_dir"' EXIT

cargo run --quiet -- mcp config \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$output_json" >/dev/null

grep -q '"command": "ssh"' "$output_json"
grep -q 'ops@example-host' "$output_json"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$output_json"

cargo run --quiet -- mcp config \
  --client codex \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai \
  --output "$output_toml" >/dev/null

grep -q 'command = "ssh"' "$output_toml"
grep -q 'ops@example-host' "$output_toml"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$output_toml"

echo "proof_remote_ssh_config: ok"
