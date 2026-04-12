#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

temp_home="$(mktemp -d)"
export AMAI_INSTALL_STATE_PATH="${temp_home}/install_state.json"

RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
snapshot_dir="${temp_home}/repo-snapshots"
mkdir -p "${snapshot_dir}"

snapshot_file() {
  local path="$1"
  local key="$2"
  local state_path="${snapshot_dir}/${key}.state"
  local data_path="${snapshot_dir}/${key}.data"
  if [[ -e "${path}" ]]; then
    printf 'present\n' >"${state_path}"
    cp "${path}" "${data_path}"
  else
    printf 'absent\n' >"${state_path}"
  fi
}

restore_file_from_snapshot() {
  local path="$1"
  local key="$2"
  local state_path="${snapshot_dir}/${key}.state"
  local data_path="${snapshot_dir}/${key}.data"
  [[ -f "${state_path}" ]] || return
  if [[ "$(cat "${state_path}")" == "absent" ]]; then
    rm -f "${path}"
    return
  fi
  mkdir -p "$(dirname "${path}")"
  cp "${data_path}" "${path}"
}

cleanup() {
  restore_file_from_snapshot "AGENTS.md" "AGENTS.md"
  rm -rf "${temp_home}"
}

trap cleanup EXIT

snapshot_file "AGENTS.md" "AGENTS.md"

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client codex --yes --skip-stack --skip-release-build >/dev/null

TEMP_HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" python3 - <<'PY'
import os
import signal
import subprocess
import sys
import time

repo_root = os.getcwd()
temp_home = os.environ["TEMP_HOME"]
rustup_home = os.environ["RUSTUP_HOME"]
cargo_home = os.environ["CARGO_HOME"]


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
        raise RuntimeError("proof_client_reconnect: failed to create fake orphan MCP process")
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


fake_orphan_pid = spawn_fake_orphan()
try:
    subprocess.run(
        ["./scripts/reconnect_local.sh", "--client", "codex"],
        check=True,
        stdout=subprocess.DEVNULL,
        env={
            **os.environ,
            "HOME": temp_home,
            "RUSTUP_HOME": rustup_home,
            "CARGO_HOME": cargo_home,
        },
    )

    config_path = os.path.join(temp_home, ".codex", "config.toml")
    if not os.path.isfile(config_path):
        raise RuntimeError("proof_client_reconnect: codex config missing after reconnect")
    config_text = open(config_path, "r", encoding="utf-8").read()
    if "[mcp_servers.amai]" not in config_text:
        raise RuntimeError("proof_client_reconnect: Amai config missing after reconnect")

    agents_text = open("AGENTS.md", "r", encoding="utf-8").read()
    if "AMAI MANAGED STARTUP INSTRUCTIONS v1" not in agents_text:
        raise RuntimeError("proof_client_reconnect: AGENTS startup block missing after reconnect")

    time.sleep(0.2)
    if os.path.exists(f"/proc/{fake_orphan_pid}"):
        raise RuntimeError("proof_client_reconnect: orphan MCP process survived reconnect")

    print("proof_client_reconnect: ok")
except Exception as exc:
    cleanup_pid(fake_orphan_pid)
    print(str(exc), file=sys.stderr)
    sys.exit(1)
PY
