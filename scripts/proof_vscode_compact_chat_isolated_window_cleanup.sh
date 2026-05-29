#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

snapshot_temp_host_pids() {
  python3 - <<'PY'
from pathlib import Path
pids = []
for entry in Path('/proc').iterdir():
    if not entry.name.isdigit():
        continue
    try:
        raw = (entry / 'cmdline').read_bytes()
    except OSError:
        continue
    parts = [p.decode('utf-8', 'ignore') for p in raw.split(b'\0') if p]
    joined = ' '.join(parts)
    if '/tmp/tmp.' in joined and '--user-data-dir' in joined and 'python3 - <<' not in joined and '/bin/bash -c python3 - <<' not in joined:
        pids.append(entry.name)
print('\n'.join(sorted(pids)))
PY
}

before_pids="$(snapshot_temp_host_pids)"

"${repo_root}/scripts/proof_vscode_compact_chat_isolated_host_uri_delivery_boundary.sh" >/dev/null
"${repo_root}/scripts/proof_vscode_compact_chat_isolated_direct_uri_startup_boundary.sh" >/dev/null

sleep 2
after_pids="$(snapshot_temp_host_pids)"

if [[ "${before_pids}" != "${after_pids}" ]]; then
  echo "proof_vscode_compact_chat_isolated_window_cleanup: temp VS Code host set changed" >&2
  echo "before:" >&2
  printf '%s\n' "${before_pids}" >&2
  echo "after:" >&2
  printf '%s\n' "${after_pids}" >&2
  exit 1
fi

printf 'proof_vscode_compact_chat_isolated_window_cleanup: ok\n'
