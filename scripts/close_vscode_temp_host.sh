#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <user-data-dir>" >&2
  exit 2
fi

user_dir="$1"
if [[ -z "${user_dir}" ]]; then
  echo "close_vscode_temp_host: user-data-dir must be non-empty" >&2
  exit 2
fi

python3 - <<'PY' "${user_dir}"
import os
import signal
import sys
import time
from pathlib import Path

user_dir = sys.argv[1]
current_pid = os.getpid()
parent_pid = os.getppid()

def read_cmdline(pid: int):
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return None
    parts = [part for part in raw.split(b"\0") if part]
    try:
        return [part.decode("utf-8", "ignore") for part in parts]
    except Exception:
        return None

def matches_user_dir(cmdline):
    if not cmdline:
        return False
    joined = " ".join(cmdline)
    if f"--user-data-dir={user_dir}" in joined:
        return True
    for index, part in enumerate(cmdline[:-1]):
        if part == "--user-data-dir" and cmdline[index + 1] == user_dir:
            return True
    return False

def collect_pids():
    pids = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid in (current_pid, parent_pid):
            continue
        cmdline = read_cmdline(pid)
        if matches_user_dir(cmdline):
            pids.append(pid)
    return sorted(set(pids))

def terminate(pids, sig):
    for pid in pids:
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            pass
        except PermissionError:
            pass

def still_running(pids):
    alive = []
    for pid in pids:
        try:
            os.kill(pid, 0)
            alive.append(pid)
        except ProcessLookupError:
            pass
        except PermissionError:
            alive.append(pid)
    return alive

pids = collect_pids()
if not pids:
    sys.exit(0)

terminate(pids, signal.SIGTERM)
deadline = time.time() + 8.0
while time.time() < deadline:
    alive = still_running(pids)
    if not alive:
        sys.exit(0)
    time.sleep(0.25)

alive = still_running(pids)
if alive:
    terminate(alive, signal.SIGKILL)
PY
