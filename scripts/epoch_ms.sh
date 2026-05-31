#!/usr/bin/env bash
set -euo pipefail

epoch_nanos="$(date +%s%N 2>/dev/null || true)"
if [[ "${epoch_nanos}" =~ ^[0-9]+$ ]]; then
  printf '%s\n' "$((epoch_nanos / 1000000))"
  exit 0
fi

python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
