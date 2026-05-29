#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

default_output="$(AMAI_NO_INSTALL_PROMPT=1 ./scripts/preflight.sh --stack-profile default)"
lite_output="$(AMAI_NO_INSTALL_PROMPT=1 ./scripts/preflight.sh --stack-profile lite_vps)"
auto_output="$(AMAI_NO_INSTALL_PROMPT=1 ./scripts/preflight.sh)"

printf '%s\n' "$default_output" | rg 'Профиль: Workstation Full \(default\)' >/dev/null
printf '%s\n' "$default_output" | rg '^Итог: машина подходит$' >/dev/null
printf '%s\n' "$default_output" | rg '^- Жёсткие proof и benchmark-контуры: да$' >/dev/null
printf '%s\n' "$lite_output" | rg 'Профиль: Lite VPS \(lite_vps\)' >/dev/null
printf '%s\n' "$lite_output" | rg '^- Жёсткие proof и benchmark-контуры: нет$' >/dev/null
printf '%s\n' "$lite_output" | rg '^- Удалённый режим здесь уместен: да$' >/dev/null
printf '%s\n' "$auto_output" | rg '^Профили установки:$' >/dev/null
printf '%s\n' "$auto_output" | rg '^1\. Workstation Full \(default\) — подходит$' >/dev/null
printf '%s\n' "$auto_output" | rg '^2\. Lite VPS \(lite_vps\) — подходит$' >/dev/null

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
