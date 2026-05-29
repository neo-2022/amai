#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

cache1="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.json"
cache2="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.thread-${CODEX_THREAD_ID:-}.json"
cache3="${REPO_ROOT}/state/observe/client_budget_gate_cache.json"
cache4="${REPO_ROOT}/state/observe/client_budget_gate_cache.thread-${CODEX_THREAD_ID:-}.json"
tmpdir="$(mktemp -d)"

move_if_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    mkdir -p "${tmpdir}/$(dirname "$path")"
    mv "$path" "${tmpdir}/$path"
  fi
}

cleanup() {
  local path
  for path in \
    "${cache1}" \
    "${cache2}" \
    "${cache3}" \
    "${cache4}" \
    "${SCRIPT_DIR}/client_budget_root_cause.sh" \
    "${SCRIPT_DIR}/client_budget_gate.sh" \
    "${REPO_ROOT}/target/release/amai" \
    "${REPO_ROOT}/target/debug/amai"; do
    if [[ -e "${tmpdir}/$path" ]]; then
      mkdir -p "$(dirname "$path")"
      mv "${tmpdir}/$path" "$path"
    else
      rm -f "$path"
    fi
  done
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

move_if_exists "${cache1}"
move_if_exists "${cache2}"
move_if_exists "${cache3}"
move_if_exists "${cache4}"
move_if_exists "${SCRIPT_DIR}/client_budget_root_cause.sh"
move_if_exists "${SCRIPT_DIR}/client_budget_gate.sh"
move_if_exists "${REPO_ROOT}/target/release/amai"
move_if_exists "${REPO_ROOT}/target/debug/amai"

set +e
output="$(
  env -u CODEX_THREAD_ID \
    AMI_OBSERVE_BIND=127.0.0.1:1 \
    "${SCRIPT_DIR}/client_budget_system_markers.sh" 2>&1
)"
rc=$?
set -e

if [[ "${rc}" -ne 12 ]]; then
  echo "proof_client_budget_system_markers_fail_closed: expected exit code 12, got ${rc}" >&2
  printf '%s\n' "${output}" >&2
  exit 1
fi

grep -Fq "client budget system markers: no root cause payload available" <<<"${output}"

echo "proof_client_budget_system_markers_fail_closed: PASS"
