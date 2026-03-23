#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output="$(
  AMAI_REPO_ROOT="$(pwd)" \
  cargo run --quiet --bin memory -- search continuity --project art --namespace continuity
)"

printf '%s\n' "$output" | grep -F "Amai memory search" >/dev/null
printf '%s\n' "$output" | grep -F "Почему вошло:" >/dev/null
printf '%s\n' "$output" | grep -F "Почему часть не вошла:" >/dev/null
printf '%s\n' "$output" | grep -F "Найдено записей:" >/dev/null

echo "proof_memory_bridge_search: PASS"
