#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

temp_home="$(mktemp -d)"
export AMAI_INSTALL_STATE_PATH="${temp_home}/install_state.json"
export AMAI_STACK_AUTOSTART_UNIT_DIR="${temp_home}/systemd-user"
export AMAI_STACK_AUTOSTART_SKIP_SYSTEMCTL=1

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

snapshot_file ".hermes.md" "hermes-startup"

cleanup() {
  restore_file_from_snapshot ".hermes.md" "hermes-startup"
  rm -rf "${temp_home}"
}

trap cleanup EXIT

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client hermes --yes --skip-stack --skip-release-build >/dev/null

unit_path="${AMAI_STACK_AUTOSTART_UNIT_DIR}/amai-stack.service"
test -f "${unit_path}"
grep -Fq 'Description=Amai local stack bootstrap' "${unit_path}"
grep -Fq 'Type=oneshot' "${unit_path}"
grep -Fq 'RemainAfterExit=yes' "${unit_path}"
grep -Fq "WorkingDirectory=$(pwd)" "${unit_path}"
grep -Fq "ExecStart=$(pwd)/scripts/run_stack_service.sh" "${unit_path}"
grep -Fq 'WantedBy=default.target' "${unit_path}"
grep -Fq 'Environment=PATH=' "${unit_path}"

echo "proof_stack_autostart: ok"
