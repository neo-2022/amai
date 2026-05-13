#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

python3 - <<PY
import os
import signal
import subprocess
import sys
import time

repo_root = ${repo_root@Q}


def orphan_parent_kind(pid: int) -> str | None:
    try:
        with open(f"/proc/{pid}/stat", "r", encoding="utf-8") as fh:
            stat = fh.read()
    except FileNotFoundError:
        return None
    try:
        ppid = stat.rsplit(") ", 1)[1].split()[1]
    except IndexError:
        return None
    if ppid == "1":
        return "pid1"
    try:
        with open(f"/proc/{ppid}/cmdline", "rb") as fh:
            parent_cmd = fh.read().replace(b"\0", b" ").decode("utf-8", "replace")
    except FileNotFoundError:
        return None
    if "systemd --user" in parent_cmd:
        return "systemd-user"
    return None


def spawn_fake_orphan() -> int:
    read_fd, write_fd = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(read_fd)
        os.setsid()
        grandchild = os.fork()
        if grandchild == 0:
            os.chdir(repo_root)
            os.close(write_fd)
            os.execl("/bin/sleep", "./target/release/amai mcp serve", "600")
        os.write(write_fd, str(grandchild).encode("utf-8"))
        os.close(write_fd)
        os._exit(0)

    os.close(write_fd)
    grandchild_pid = os.read(read_fd, 64).decode("utf-8").strip()
    os.close(read_fd)
    os.waitpid(pid, 0)
    if not grandchild_pid.isdigit():
        raise RuntimeError("failed to create fake orphan MCP process")
    return int(grandchild_pid)


def cleanup_pid(pid: int) -> None:
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    time.sleep(0.1)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        return


orphan_pid = spawn_fake_orphan()
try:
    for _ in range(50):
        if os.path.exists(f"/proc/{orphan_pid}") and orphan_parent_kind(orphan_pid):
            break
        time.sleep(0.1)

    if not os.path.exists(f"/proc/{orphan_pid}"):
        raise RuntimeError("fake orphan MCP process exited too early")

    parent_kind = orphan_parent_kind(orphan_pid)
    if not parent_kind:
        with open(f"/proc/{orphan_pid}/stat", "r", encoding="utf-8") as fh:
            stat = fh.read()
        try:
            ppid = stat.rsplit(") ", 1)[1].split()[1]
        except IndexError:
            ppid = "<unparseable>"
        raise RuntimeError(f"fake MCP process did not become orphaned (ppid={ppid})")

    subprocess.run(
        [os.path.join(repo_root, "scripts/cleanup_mcp_orphans.sh"), repo_root],
        check=True,
    )
    time.sleep(0.2)

    if os.path.exists(f"/proc/{orphan_pid}"):
        raise RuntimeError(
            f"cleanup helper did not remove orphaned MCP process {orphan_pid}"
        )

    print("proof_mcp_orphan_cleanup: PASS")
except Exception as exc:
    cleanup_pid(orphan_pid)
    print(str(exc), file=sys.stderr)
    sys.exit(1)
PY
