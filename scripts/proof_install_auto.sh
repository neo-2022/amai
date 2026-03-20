#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

install_out="$tmp_dir/install.out"
remove_out="$tmp_dir/remove.out"
target_file="$tmp_dir/mcp.json"
state_file="$tmp_dir/install-state.json"

printf '1\nДА\n' | AMAI_FORCE_INTERACTIVE_PROMPT=1 AMAI_INSTALL_STATE_PATH="$state_file" ./scripts/install_amai.sh \
  --client vscode \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >"$install_out"

test -f "$target_file"
test -f "$state_file"
rg '^Amai готов$' "$install_out" >/dev/null
rg '^Результат: Amai установлен впервые\.$' "$install_out" >/dev/null
rg '^Клиент: VS Code$' "$install_out" >/dev/null
rg '^Выбранный профиль: Workstation Full \(default\)$' "$install_out" >/dev/null
printf '1\nДА\n' | AMAI_FORCE_INTERACTIVE_PROMPT=1 AMAI_INSTALL_STATE_PATH="$state_file" ./scripts/install_amai.sh \
  --client vscode \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >>"$install_out"
test "$(rg -o '"amai"' "$target_file" | wc -l | tr -d ' ')" = "1"
rg '^Результат: Amai уже был установлен\. Обновление не требовалось; текущая версия уже актуальна\.$' "$install_out" >/dev/null

./scripts/remove_amai.sh \
  --output "$target_file" \
  >"$remove_out"

rg '^server_removed: true$' "$remove_out" >/dev/null
