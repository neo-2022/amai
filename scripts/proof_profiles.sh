#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

default_output="$(./scripts/preflight.sh --stack-profile default)"
lite_output="$(./scripts/preflight.sh --stack-profile lite_vps)"

printf '%s\n' "$default_output" | rg 'Профиль: Workstation Full \(default\)' >/dev/null
printf '%s\n' "$default_output" | rg '^Итог: машина подходит$' >/dev/null
printf '%s\n' "$default_output" | rg '^- supports_peak_benchmarks: true$' >/dev/null
printf '%s\n' "$lite_output" | rg 'Профиль: Lite VPS \(lite_vps\)' >/dev/null
printf '%s\n' "$lite_output" | rg '^- supports_peak_benchmarks: false$' >/dev/null
printf '%s\n' "$lite_output" | rg '^- remote_mode_recommended: true$' >/dev/null

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
./scripts/onboard_lite_vps.sh \
  --client generic \
  --yes \
  --skip-stack \
  --skip-release-build \
  --output "$tmp_dir/generic-mcp.json" \
  > /dev/null
test -f "$tmp_dir/generic-mcp.json"
