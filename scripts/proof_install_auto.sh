#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

install_out="$tmp_dir/install.out"
remove_out="$tmp_dir/remove.out"
target_file="$tmp_dir/mcp.json"
state_file="$tmp_dir/install-state.json"

printf '1\nДА\n' | CARGO_TARGET_DIR="$(pwd)/target" AMAI_FORCE_INTERACTIVE_PROMPT=1 AMAI_INSTALL_STATE_PATH="$state_file" ./scripts/install_amai.sh \
  --client vscode \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >"$install_out"

test -f "$target_file"
test -f "$state_file"
rg '^Amai готов$' "$install_out" >/dev/null
rg '^Клиент: VS Code / Codium$' "$install_out" >/dev/null
rg '^Выбранный профиль: default$' "$install_out" >/dev/null
rg '^Startup contract для клиента: пропущен в compact install contour$' "$install_out" >/dev/null
rg '^Client runtime artifact: VS Code bridge установлен$' "$install_out" >/dev/null
printf '1\nДА\n' | CARGO_TARGET_DIR="$(pwd)/target" AMAI_FORCE_INTERACTIVE_PROMPT=1 AMAI_INSTALL_STATE_PATH="$state_file" ./scripts/install_amai.sh \
  --client vscode \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >>"$install_out"
test "$(rg -o '"amai"' "$target_file" | wc -l | tr -d ' ')" = "1"
test "$(rg -c '^Amai готов$' "$install_out" | tr -d ' ')" = "2"

./scripts/remove_amai.sh \
  --output "$target_file" \
  >"$remove_out"

rg '^server_removed: true$' "$remove_out" >/dev/null
