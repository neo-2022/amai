#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

cache_path="${REPO_ROOT}/state/observe/client_budget_gate_cache.json"
thread_cache_path="${REPO_ROOT}/state/observe/client_budget_gate_cache.thread-${CODEX_THREAD_ID:-}.json"
startup_contract_path="${REPO_ROOT}/.amai/onboarding/project-chat-startup-contract.json"
tmpdir="$(mktemp -d)"

cleanup() {
  if [[ -f "${tmpdir}/cache" ]]; then
    mv "${tmpdir}/cache" "${cache_path}"
  else
    rm -f "${cache_path}"
  fi
  if [[ -n "${CODEX_THREAD_ID:-}" ]]; then
    if [[ -f "${tmpdir}/thread_cache" ]]; then
      mv "${tmpdir}/thread_cache" "${thread_cache_path}"
    else
      rm -f "${thread_cache_path}"
    fi
  fi
  if [[ -f "${tmpdir}/client_budget_gate.sh" ]]; then
    mv "${tmpdir}/client_budget_gate.sh" "${SCRIPT_DIR}/client_budget_gate.sh"
  fi
  if [[ -f "${tmpdir}/startup_contract.json" ]]; then
    mv "${tmpdir}/startup_contract.json" "${startup_contract_path}"
  fi
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

[[ -f "${cache_path}" ]] && mv "${cache_path}" "${tmpdir}/cache"
if [[ -n "${CODEX_THREAD_ID:-}" ]] && [[ -f "${thread_cache_path}" ]]; then
  mv "${thread_cache_path}" "${tmpdir}/thread_cache"
fi
mv "${SCRIPT_DIR}/client_budget_gate.sh" "${tmpdir}/client_budget_gate.sh"
cp "${startup_contract_path}" "${tmpdir}/startup_contract.json"
jq '
  .startup_contract.live_client_budget_enforcement.reply_blocking_removed = false
' "${tmpdir}/startup_contract.json" > "${startup_contract_path}"

set +e
output="$(
  env -u CODEX_THREAD_ID \
    AMI_OBSERVE_BIND=127.0.0.1:1 \
    "${SCRIPT_DIR}/client_budget_reply_gate.sh" 2>&1
)"
rc=$?
set -e

if [[ "${rc}" -ne 12 ]]; then
  echo "proof_client_budget_reply_gate_fail_closed: expected exit code 12, got ${rc}" >&2
  printf '%s\n' "${output}" >&2
  exit 1
fi

grep -Fq "client budget reply gate: no gate payload available" <<<"${output}"

echo "proof_client_budget_reply_gate_fail_closed: PASS"
