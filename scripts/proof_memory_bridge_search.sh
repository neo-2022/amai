#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --quiet --release --bin amai --bin memory

output="$(
  AMAI_REPO_ROOT="$(pwd)" \
  ./target/release/memory search continuity --project art --namespace continuity
)"

printf '%s\n' "$output" | grep -F "Amai memory search" >/dev/null
printf '%s\n' "$output" | grep -F "Почему вошло:" >/dev/null
printf '%s\n' "$output" | grep -F "Почему часть не вошла:" >/dev/null
printf '%s\n' "$output" | grep -F "Найдено записей:" >/dev/null

echo "proof_memory_bridge_search: PASS"
