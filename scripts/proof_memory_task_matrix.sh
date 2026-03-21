#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

output="$(cargo run --release --quiet -- verify memory-matrix --matrix letta_memory_local)"

printf '%s\n' "$output" | rg '"matrix": "letta_memory_local"' >/dev/null
printf '%s\n' "$output" | rg '"tasks_failed": 0' >/dev/null
printf '%s\n' "$output" | rg '"success_rate": 1\.0' >/dev/null
printf '%s\n' "$output" | rg '"mean_score": 1\.0' >/dev/null
printf '%s\n' "$output" | rg '"class": "read"' >/dev/null
printf '%s\n' "$output" | rg '"class": "write"' >/dev/null
printf '%s\n' "$output" | rg '"class": "update"' >/dev/null
printf '%s\n' "$output" | rg '"class": "isolation"' >/dev/null
printf '%s\n' "$output" | rg '"layer": "core"' >/dev/null
printf '%s\n' "$output" | rg '"layer": "archival"' >/dev/null

printf 'proof_memory_task_matrix: ok\n'
