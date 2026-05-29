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

assert_file_matches_snapshot() {
  local path="$1"
  local key="$2"
  local state_path="${snapshot_dir}/${key}.state"
  local data_path="${snapshot_dir}/${key}.data"
  local state
  state="$(cat "${state_path}")"
  if [[ "${state}" == "absent" ]]; then
    if [[ -e "${path}" ]]; then
      echo "proof_client_reconnect: ${path} should have been removed"
      exit 1
    fi
    return
  fi
  if [[ ! -e "${path}" ]]; then
    echo "proof_client_reconnect: ${path} disappeared but existed before proof"
    exit 1
  fi
  if ! cmp -s "${path}" "${data_path}"; then
    echo "proof_client_reconnect: ${path} was not restored to its pre-proof state"
    exit 1
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

assert_contains_all() {
  local path="$1"
  shift
  local needle
  for needle in "$@"; do
    grep -Fq "${needle}" "${path}"
  done
}

assert_hermes_compact_startup() {
  local path="$1"
  local max_bytes="$2"
  assert_contains_all "${path}" \
    'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
    'compact contract-pointer' \
    'до любого другого Amai шага.' \
    './scripts/reconnect_local.sh --client hermes' \
    './scripts/amai_exec.sh bootstrap reconnect --client hermes --yes'
  local bytes
  bytes="$(wc -c <"${path}")"
  if (( bytes > max_bytes )); then
    echo "proof_client_reconnect: ${path} is too large for compact Hermes startup (${bytes} > ${max_bytes})"
    exit 1
  fi
}

spawn_fake_orphan() {
  TEMP_HOME="${temp_home}" python3 - <<'PY'
import os

repo_root = os.getcwd()
temp_home = os.environ["TEMP_HOME"]

read_fd, write_fd = os.pipe()
pid = os.fork()
if pid == 0:
    os.close(read_fd)
    os.setsid()
    grandchild = os.fork()
    if grandchild == 0:
        os.chdir(repo_root)
        os.environ["HOME"] = temp_home
        devnull = os.open("/dev/null", os.O_RDWR)
        os.dup2(devnull, 0)
        os.dup2(devnull, 1)
        os.dup2(devnull, 2)
        if devnull > 2:
            os.close(devnull)
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
print(grandchild_pid)
PY
}

cleanup_pid() {
  local pid="$1"
  kill "${pid}" 2>/dev/null || return 0
  sleep 0.1
  kill -9 "${pid}" 2>/dev/null || true
}

assert_orphan_gone() {
  local pid="$1"
  sleep 0.2
  if [[ -e "/proc/${pid}" ]]; then
    echo "proof_client_reconnect: orphan MCP process survived reconnect"
    cleanup_pid "${pid}"
    exit 1
  fi
}

run_client_command() {
  HOME="${temp_home}" \
    RUSTUP_HOME="${RUSTUP_HOME}" \
    CARGO_HOME="${CARGO_HOME}" \
    "$@"
}

reconnect_client() {
  local client="$1"
  local orphan_pid
  orphan_pid="$(spawn_fake_orphan)"
  run_client_command ./scripts/reconnect_local.sh --client "${client}" >/dev/null || {
    cleanup_pid "${orphan_pid}"
    exit 1
  }
  assert_orphan_gone "${orphan_pid}"
}

snapshot_file "AGENTS.md" "AGENTS.md"
snapshot_file ".github/instructions/amai-continuity-startup.instructions.md" "vscode-startup"
snapshot_file ".vscode/mcp.json" "vscode-mcp"
snapshot_file ".cursor/rules/amai-continuity-startup.mdc" "cursor-rule"
snapshot_file "CLAUDE.md" "CLAUDE.md"
snapshot_file ".mcp.json" "repo-mcp-json"
snapshot_file ".hermes.md" "hermes-startup"
snapshot_file ".openclaw/AGENTS.md" "openclaw-startup"

cleanup() {
  restore_file_from_snapshot "AGENTS.md" "AGENTS.md"
  restore_file_from_snapshot ".github/instructions/amai-continuity-startup.instructions.md" "vscode-startup"
  restore_file_from_snapshot ".vscode/mcp.json" "vscode-mcp"
  restore_file_from_snapshot ".cursor/rules/amai-continuity-startup.mdc" "cursor-rule"
  restore_file_from_snapshot "CLAUDE.md" "CLAUDE.md"
  restore_file_from_snapshot ".mcp.json" "repo-mcp-json"
  restore_file_from_snapshot ".hermes.md" "hermes-startup"
  restore_file_from_snapshot ".openclaw/AGENTS.md" "openclaw-startup"
  rm -rf "${temp_home}"
}

trap cleanup EXIT

run_client_command ./scripts/onboard_local.sh --client vscode --yes --skip-stack --skip-release-build >/dev/null
reconnect_client "vscode"
test -f .vscode/mcp.json
grep -q '"amai"' .vscode/mcp.json
test -f .github/instructions/amai-continuity-startup.instructions.md
assert_contains_all .github/instructions/amai-continuity-startup.instructions.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'amai_continuity_startup' \
  './scripts/reconnect_local.sh --client vscode' \
  './scripts/amai_exec.sh bootstrap reconnect --client vscode --yes'
run_client_command ./scripts/disconnect_local.sh --client vscode >/dev/null
if [[ -f .vscode/mcp.json ]] && grep -q '"amai"' .vscode/mcp.json; then
  echo "proof_client_reconnect: vscode config still contains amai after disconnect"
  exit 1
fi
if [[ -f .github/instructions/amai-continuity-startup.instructions.md ]] &&
  grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' .github/instructions/amai-continuity-startup.instructions.md; then
  echo "proof_client_reconnect: vscode startup instructions still contain amai after disconnect"
  exit 1
fi

run_client_command ./scripts/onboard_local.sh --client cursor --yes --skip-stack --skip-release-build >/dev/null
reconnect_client "cursor"
test -f "${temp_home}/.cursor/mcp.json"
grep -q '"amai"' "${temp_home}/.cursor/mcp.json"
test -f .cursor/rules/amai-continuity-startup.mdc
assert_contains_all .cursor/rules/amai-continuity-startup.mdc \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'amai_continuity_startup' \
  './scripts/reconnect_local.sh --client cursor' \
  './scripts/amai_exec.sh bootstrap reconnect --client cursor --yes'
run_client_command ./scripts/disconnect_local.sh --client cursor >/dev/null
if [[ -f "${temp_home}/.cursor/mcp.json" ]] && grep -q '"amai"' "${temp_home}/.cursor/mcp.json"; then
  echo "proof_client_reconnect: cursor config still contains amai after disconnect"
  exit 1
fi
if [[ -f .cursor/rules/amai-continuity-startup.mdc ]] &&
  grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' .cursor/rules/amai-continuity-startup.mdc; then
  echo "proof_client_reconnect: cursor startup instructions still contain amai after disconnect"
  exit 1
fi

run_client_command ./scripts/onboard_local.sh --client codex --yes --skip-stack --skip-release-build >/dev/null
reconnect_client "codex"
test -f "${temp_home}/.codex/config.toml"
grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"
test -f AGENTS.md
assert_contains_all AGENTS.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'amai_continuity_startup' \
  './scripts/reconnect_local.sh --client codex' \
  './scripts/amai_exec.sh bootstrap reconnect --client codex --yes'
run_client_command ./scripts/disconnect_local.sh --client codex >/dev/null
if [[ -f "${temp_home}/.codex/config.toml" ]] && grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"; then
  echo "proof_client_reconnect: codex config still contains amai after disconnect"
  exit 1
fi
if grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' AGENTS.md; then
  echo "proof_client_reconnect: codex startup instructions still contain amai after disconnect"
  exit 1
fi
restore_file_from_snapshot "AGENTS.md" "AGENTS.md"

run_client_command ./scripts/onboard_local.sh --client claude-code --yes --skip-stack --skip-release-build >/dev/null
reconnect_client "claude-code"
test -f .mcp.json
grep -q '"amai"' .mcp.json
test -f CLAUDE.md
assert_contains_all CLAUDE.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'amai_continuity_startup' \
  './scripts/reconnect_local.sh --client claude-code' \
  './scripts/amai_exec.sh bootstrap reconnect --client claude-code --yes'
run_client_command ./scripts/disconnect_local.sh --client claude-code >/dev/null
if [[ -f .mcp.json ]] && grep -q '"amai"' .mcp.json; then
  echo "proof_client_reconnect: claude-code config still contains amai after disconnect"
  exit 1
fi
if [[ -f CLAUDE.md ]] && grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' CLAUDE.md; then
  echo "proof_client_reconnect: claude-code startup instructions still contain amai after disconnect"
  exit 1
fi

run_client_command ./scripts/onboard_local.sh --client hermes --yes --skip-stack --skip-release-build >/dev/null
reconnect_client "hermes"
test -f "${temp_home}/.hermes/config.yaml"
grep -q '^mcp_servers:' "${temp_home}/.hermes/config.yaml"
grep -q '^  amai:' "${temp_home}/.hermes/config.yaml"
test -f .hermes.md
assert_hermes_compact_startup .hermes.md 4000
run_client_command ./scripts/disconnect_local.sh --client hermes >/dev/null
if [[ -f "${temp_home}/.hermes/config.yaml" ]] && grep -q '^  amai:' "${temp_home}/.hermes/config.yaml"; then
  echo "proof_client_reconnect: hermes config still contains amai after disconnect"
  exit 1
fi
if [[ -f .hermes.md ]] && grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' .hermes.md; then
  echo "proof_client_reconnect: hermes startup instructions still contain amai after disconnect"
  exit 1
fi

mkdir -p "${temp_home}/.openclaw"
cat > "${temp_home}/.openclaw/openclaw.json" <<'EOF'
{
  // existing JSON5 comment must not break OpenClaw reconnect
  gateway: {
    mode: 'local',
  },
}
EOF
run_client_command ./scripts/onboard_local.sh --client openclaw --yes --skip-stack --skip-release-build >/dev/null
reconnect_client "openclaw"
test -f "${temp_home}/.openclaw/openclaw.json"
grep -q '"gateway"' "${temp_home}/.openclaw/openclaw.json"
HOME="${temp_home}" openclaw mcp show amai --json | grep -q 'run_mcp_stdio'
HOME="${temp_home}" openclaw agents list --json | jq -e --arg workspace "$(pwd)/.openclaw" '.[] | select(.workspace == $workspace)' >/dev/null
test -f .openclaw/AGENTS.md
assert_contains_all .openclaw/AGENTS.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  'amai_continuity_startup' \
  './scripts/reconnect_local.sh --client openclaw' \
  './scripts/amai_exec.sh bootstrap reconnect --client openclaw --yes'
run_client_command ./scripts/disconnect_local.sh --client openclaw >/dev/null
if HOME="${temp_home}" openclaw mcp show amai --json >/dev/null 2>&1; then
  echo "proof_client_reconnect: openclaw config still contains amai after disconnect"
  exit 1
fi
grep -q '"gateway"' "${temp_home}/.openclaw/openclaw.json"
if HOME="${temp_home}" openclaw agents list --json | jq -e --arg workspace "$(pwd)/.openclaw" '.[] | select(.workspace == $workspace)' >/dev/null; then
  echo "proof_client_reconnect: openclaw project agent still present after disconnect"
  exit 1
fi
if [[ -f .openclaw/AGENTS.md ]] && grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' .openclaw/AGENTS.md; then
  echo "proof_client_reconnect: openclaw startup instructions still contain amai after disconnect"
  exit 1
fi

echo "proof_client_reconnect: ok"
