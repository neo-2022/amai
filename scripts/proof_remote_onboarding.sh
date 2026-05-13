#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tmp_home="$(mktemp -d)"
snapshot_dir="${tmp_home}/repo-snapshots"
fake_bin="${tmp_home}/fake-bin"
fake_remote_root="${tmp_home}/fake-remote"
mkdir -p "${snapshot_dir}"
mkdir -p "${fake_bin}" "${fake_remote_root}"
trap 'rm -rf "$tmp_home"' EXIT

orig_home="${HOME:-}"
orig_cargo_home="${CARGO_HOME:-${orig_home}/.cargo}"
orig_rustup_home="${RUSTUP_HOME:-${orig_home}/.rustup}"
export HOME="$tmp_home"
export CARGO_HOME="$orig_cargo_home"
export RUSTUP_HOME="$orig_rustup_home"
export AMAI_FAKE_REMOTE_ROOT="$fake_remote_root"

cat > "${fake_bin}/ssh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

destination="${1:-}"
shift || true
remote_command="$*"
remote_root="${AMAI_FAKE_REMOTE_ROOT:?}/srv/amai"

if [[ "${destination}" != "ops@example-host" ]]; then
  echo "fake ssh: unexpected destination: ${destination}" >&2
  exit 1
fi

if [[ "${remote_command}" == *"mkdir -p"* && "${remote_command}" == *"/srv/amai"* && "${remote_command}" == *"cleanup_paths"* ]]; then
  mkdir -p "${remote_root}"
  rm -rf \
    "${remote_root}/.fastembed_cache" \
    "${remote_root}/output" \
    "${remote_root}/target" \
    "${remote_root}/state" \
    "${remote_root}/tmp"
  exit 0
fi

if [[ "${remote_command}" == "cd '/srv/amai' && tar -xf -" ]]; then
  mkdir -p "${remote_root}"
  tar -C "${remote_root}" -xf -
  exit 0
fi

if [[ "${remote_command}" == "test -f '/srv/amai/Cargo.toml' && test -f '/srv/amai/scripts/run_mcp_stdio.sh'" ]]; then
  test -f "${remote_root}/Cargo.toml"
  test -f "${remote_root}/scripts/run_mcp_stdio.sh"
  exit 0
fi

echo "fake ssh: unsupported command: ${remote_command}" >&2
exit 1
EOF
chmod +x "${fake_bin}/ssh"
export PATH="${fake_bin}:${PATH}"

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
    './scripts/reconnect_local.sh --client hermes'
  local bytes
  bytes="$(wc -c <"${path}")"
  if (( bytes > max_bytes )); then
    echo "proof_remote_onboarding: ${path} is too large for compact Hermes startup (${bytes} > ${max_bytes})"
    exit 1
  fi
}

snapshot_file "AGENTS.md" "AGENTS.md"
snapshot_file ".github/instructions/amai-continuity-startup.instructions.md" "vscode-startup"
snapshot_file ".cursor/rules/amai-continuity-startup.mdc" "cursor-rule"
snapshot_file "CLAUDE.md" "CLAUDE.md"
snapshot_file ".mcp.json" "repo-mcp-json"
snapshot_file ".hermes.md" "hermes-startup"
snapshot_file ".openclaw/AGENTS.md" "openclaw-startup"

run_remote_onboarding() {
  local client="$1"
  local output_path="$2"
  local onboarding_out="${tmp_home}/${client}-onboarding.out"

  ./scripts/onboard_remote_client.sh \
    --client "${client}" \
    --ssh-destination ops@example-host \
    --remote-repo-root /srv/amai >"${onboarding_out}"

  test -f "${output_path}"
  grep -q 'ops@example-host' "${output_path}"
  grep -q 'Режим подключения: удалённый через SSH' "${onboarding_out}"
  grep -q 'Сервер: ops@example-host' "${onboarding_out}"
  grep -q 'Удалённый путь: /srv/amai' "${onboarding_out}"
}

run_remote_onboarding "vscode" "${repo_root}/.vscode/mcp.json"
grep -q '"command": "ssh"' "${repo_root}/.vscode/mcp.json"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "${repo_root}/.vscode/mcp.json"
test -f .github/instructions/amai-continuity-startup.instructions.md
assert_contains_all .github/instructions/amai-continuity-startup.instructions.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  './scripts/reconnect_local.sh --client vscode'
./scripts/disconnect_local.sh --client vscode >/dev/null
test ! -f "${repo_root}/.vscode/mcp.json"
if [[ -f .github/instructions/amai-continuity-startup.instructions.md ]]; then
  echo "proof_remote_onboarding: vscode startup instructions still present after disconnect"
  exit 1
fi

run_remote_onboarding "cursor" "${HOME}/.cursor/mcp.json"
grep -q '"command": "ssh"' "${HOME}/.cursor/mcp.json"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "${HOME}/.cursor/mcp.json"
test -f .cursor/rules/amai-continuity-startup.mdc
assert_contains_all .cursor/rules/amai-continuity-startup.mdc \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  './scripts/reconnect_local.sh --client cursor'
./scripts/disconnect_local.sh --client cursor >/dev/null
test ! -f "${HOME}/.cursor/mcp.json"
if [[ -f .cursor/rules/amai-continuity-startup.mdc ]]; then
  echo "proof_remote_onboarding: cursor startup instructions still present after disconnect"
  exit 1
fi

run_remote_onboarding "codex" "${HOME}/.codex/config.toml"
grep -q 'command = "ssh"' "${HOME}/.codex/config.toml"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "${HOME}/.codex/config.toml"
test -f AGENTS.md
assert_contains_all AGENTS.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  './scripts/reconnect_local.sh --client codex'
./scripts/disconnect_local.sh --client codex >/dev/null
test ! -f "${HOME}/.codex/config.toml"
if grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' AGENTS.md; then
  echo "proof_remote_onboarding: codex startup instructions still present after disconnect"
  exit 1
fi
restore_file_from_snapshot "AGENTS.md" "AGENTS.md"

run_remote_onboarding "claude-code" "${repo_root}/.mcp.json"
grep -q '"command": "ssh"' "${repo_root}/.mcp.json"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "${repo_root}/.mcp.json"
test -f CLAUDE.md
assert_contains_all CLAUDE.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  './scripts/reconnect_local.sh --client claude-code'
./scripts/disconnect_local.sh --client claude-code >/dev/null
test ! -f "${repo_root}/.mcp.json"
if [[ -f CLAUDE.md ]] && grep -Fq 'AMAI MANAGED STARTUP INSTRUCTIONS v1' CLAUDE.md; then
  echo "proof_remote_onboarding: claude-code startup instructions still present after disconnect"
  exit 1
fi

run_remote_onboarding "hermes" "${HOME}/.hermes/config.yaml"
grep -q "command: 'ssh'" "${HOME}/.hermes/config.yaml"
grep -q "cd ''/srv/amai'' && ./scripts/run_mcp_stdio.sh" "${HOME}/.hermes/config.yaml"
test -f .hermes.md
assert_hermes_compact_startup .hermes.md 4000
./scripts/disconnect_local.sh --client hermes >/dev/null
test ! -f "${HOME}/.hermes/config.yaml"
if [[ -f .hermes.md ]]; then
  echo "proof_remote_onboarding: hermes startup instructions still present after disconnect"
  exit 1
fi

mkdir -p "${HOME}/.openclaw"
cat > "${HOME}/.openclaw/openclaw.json" <<'EOF'
{
  // existing JSON5 comment must survive remote OpenClaw onboarding
  gateway: {
    mode: 'local',
  },
}
EOF
run_remote_onboarding "openclaw" "${HOME}/.openclaw/openclaw.json"
grep -q '"command": "ssh"' "${HOME}/.openclaw/openclaw.json"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "${HOME}/.openclaw/openclaw.json"
grep -q '"gateway"' "${HOME}/.openclaw/openclaw.json"
HOME="${HOME}" openclaw mcp show amai --json | grep -q 'ops@example-host'
test -f .openclaw/AGENTS.md
assert_contains_all .openclaw/AGENTS.md \
  'AMAI MANAGED STARTUP INSTRUCTIONS v1' \
  './scripts/reconnect_local.sh --client openclaw'
HOME="${HOME}" openclaw agents list --json | jq -e --arg workspace "${repo_root}/.openclaw" '.[] | select(.workspace == $workspace)' >/dev/null
./scripts/disconnect_local.sh --client openclaw >/dev/null
if HOME="${HOME}" openclaw mcp show amai --json >/dev/null 2>&1; then
  echo "proof_remote_onboarding: openclaw config still contains amai after disconnect"
  exit 1
fi
grep -q '"gateway"' "${HOME}/.openclaw/openclaw.json"
if HOME="${HOME}" openclaw agents list --json | jq -e --arg workspace "${repo_root}/.openclaw" '.[] | select(.workspace == $workspace)' >/dev/null; then
  echo "proof_remote_onboarding: openclaw project agent still present after disconnect"
  exit 1
fi
if [[ -f .openclaw/AGENTS.md ]]; then
  echo "proof_remote_onboarding: openclaw startup instructions still present after disconnect"
  exit 1
fi

restore_file_from_snapshot "AGENTS.md" "AGENTS.md"
restore_file_from_snapshot ".github/instructions/amai-continuity-startup.instructions.md" "vscode-startup"
restore_file_from_snapshot ".cursor/rules/amai-continuity-startup.mdc" "cursor-rule"
restore_file_from_snapshot "CLAUDE.md" "CLAUDE.md"
restore_file_from_snapshot ".mcp.json" "repo-mcp-json"
restore_file_from_snapshot ".hermes.md" "hermes-startup"
restore_file_from_snapshot ".openclaw/AGENTS.md" "openclaw-startup"

echo "proof_remote_onboarding: ok"
