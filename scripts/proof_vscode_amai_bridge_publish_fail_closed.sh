#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

run_expect_failure() {
  local target="$1"
  local expected="$2"
  local log
  log="$(mktemp)"
  if env -u VSCE_PAT -u MARKETPLACE_TOKEN -u OVSX_PAT -u OPENVSX_TOKEN \
    "${repo_root}/scripts/publish_vscode_amai_bridge.sh" --target "${target}" >"${log}" 2>&1; then
    cat "${log}" >&2
    rm -f "${log}"
    echo "proof_vscode_amai_bridge_publish_fail_closed: expected ${target} publish to fail" >&2
    exit 1
  fi
  grep -F "${expected}" "${log}" >/dev/null
  rm -f "${log}"
}

run_expect_failure marketplace "missing marketplace publish token"
run_expect_failure openvsx "missing OpenVSX publish token"

printf 'proof_vscode_amai_bridge_publish_fail_closed: PASS\n'
