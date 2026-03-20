#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

default_output="$(cargo run --quiet -- bootstrap preflight --stack-profile default)"
lite_output="$(cargo run --quiet -- bootstrap preflight --stack-profile lite_vps)"

printf '%s\n' "$default_output" | rg '^deployment profile: default$' >/dev/null
printf '%s\n' "$default_output" | rg '^supports peak benchmarks: true$' >/dev/null
printf '%s\n' "$lite_output" | rg '^deployment profile: lite_vps$' >/dev/null
printf '%s\n' "$lite_output" | rg '^supports peak benchmarks: false$' >/dev/null
printf '%s\n' "$lite_output" | rg '^remote mode recommended: true$' >/dev/null

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
./scripts/onboard_lite_vps.sh \
  --client generic \
  --skip-stack \
  --skip-release-build \
  --output "$tmp_dir/generic-mcp.json" \
  > /dev/null
test -f "$tmp_dir/generic-mcp.json"
