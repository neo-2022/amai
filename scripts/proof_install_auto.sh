#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

install_out="$tmp_dir/install.out"
remove_out="$tmp_dir/remove.out"
target_file="$tmp_dir/mcp.json"

printf '1\nДА\n' | AMAI_FORCE_INTERACTIVE_PROMPT=1 ./scripts/install_amai.sh \
  --client vscode \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >"$install_out"

test -f "$target_file"
rg '^client: vscode$' "$install_out" >/dev/null
rg '^stack_profile: default$' "$install_out" >/dev/null
rg '^repeat_install_note: если запустить установку ещё раз, Amai не создаст вторую запись, а аккуратно пересинхронизирует текущую\.$' "$install_out" >/dev/null
printf '1\nДА\n' | AMAI_FORCE_INTERACTIVE_PROMPT=1 ./scripts/install_amai.sh \
  --client vscode \
  --skip-stack \
  --skip-release-build \
  --output "$target_file" \
  >>"$install_out"
test "$(rg -o '"amai"' "$target_file" | wc -l | tr -d ' ')" = "1"
rg '^client: vscode$' "$install_out" >/dev/null

./scripts/remove_amai.sh \
  --output "$target_file" \
  >"$remove_out"

rg '^server_removed: true$' "$remove_out" >/dev/null
