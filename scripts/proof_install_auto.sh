#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

install_out="$tmp_dir/install.out"
remove_out="$tmp_dir/remove.out"
target_file="$tmp_dir/mcp.json"

./scripts/install_amai.sh \
  --yes \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >"$install_out"

test -f "$target_file"
rg '^client_resolution_mode: auto_detected$' "$install_out" >/dev/null
rg '^client: vscode$' "$install_out" >/dev/null

./scripts/remove_amai.sh \
  --output "$target_file" \
  >"$remove_out"

rg '^server_removed: true$' "$remove_out" >/dev/null
