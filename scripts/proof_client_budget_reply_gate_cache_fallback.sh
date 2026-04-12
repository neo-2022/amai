#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CACHE_PATH="${REPO_ROOT}/state/observe/client_budget_gate_cache.json"

cd "${REPO_ROOT}"

reply_blocking_removed="false"
if command -v jq >/dev/null 2>&1 && [[ -f ".amai/onboarding/project-chat-startup-contract.json" ]]; then
  reply_blocking_removed="$(
    jq -r '
      .startup_contract.live_client_budget_enforcement.reply_blocking_removed
      // false
    ' .amai/onboarding/project-chat-startup-contract.json 2>/dev/null || printf 'false'
  )"
fi

backup_path=""
if [[ -f "${CACHE_PATH}" ]]; then
  backup_path="$(mktemp)"
  cp "${CACHE_PATH}" "${backup_path}"
fi

cleanup() {
  if [[ -n "${backup_path}" && -f "${backup_path}" ]]; then
    mkdir -p "$(dirname "${CACHE_PATH}")"
    mv "${backup_path}" "${CACHE_PATH}"
  else
    rm -f "${CACHE_PATH}"
  fi
}
trap cleanup EXIT

mkdir -p "$(dirname "${CACHE_PATH}")"
now_ms="$(date +%s%3N)"

cat >"${CACHE_PATH}" <<EOF
{
  "cache_version": "client-budget-gate-cache-v7",
  "fetched_at_epoch_ms": ${now_ms},
  "gate": {
    "client_budget_reply_gate": {
      "status_label": "дождись восстановления окна лимита",
      "observed_at_epoch_ms": ${now_ms},
      "max_guard_age_seconds": 10,
      "reply_execution_gate": {
        "action_kind": "wait_for_global_client_budget_recovery",
        "blocking": true,
        "must_rotate_before_reply": false,
        "must_wait_for_budget_recovery_before_reply": true,
        "reply_budget_mode": "compact_high_signal",
        "reply_prefix": "5ч KPI: переплата 12.34%"
      }
    }
  },
  "guard": {
    "observed_at_epoch_ms": ${now_ms},
    "reply_execution_gate": {
      "action_kind": "wait_for_global_client_budget_recovery",
      "blocking": true,
      "must_wait_for_budget_recovery_before_reply": true,
      "reply_prefix": "5ч KPI: переплата 12.34%"
    }
  }
}
EOF

set +e
output="$(
  env -u CODEX_THREAD_ID \
    AMI_OBSERVE_BIND=127.0.0.1:1 \
    "${SCRIPT_DIR}/client_budget_reply_gate.sh" 2>&1
)"
rc=$?
set -e

if [[ "${reply_blocking_removed}" == "true" ]]; then
  # In removed mode, reply gate must not block even if cache payload says it should.
  if [[ "${rc}" -ne 0 ]]; then
    echo "expected exit code 0 (reply_blocking_removed=true), got ${rc}" >&2
    exit 1
  fi
  if [[ -n "${output}" ]]; then
    echo "expected empty output (reply_blocking_removed=true), got non-empty output" >&2
    exit 1
  fi
  echo "proof_client_budget_reply_gate_cache_fallback: PASS (reply_blocking_removed=true)"
  exit 0
fi

if [[ "${rc}" -ne 10 ]]; then
  echo "expected exit code 10, got ${rc}" >&2
  exit 1
fi

expected_template="$(jq -r '
  .startup_contract.live_client_budget_enforcement.blocking_reply_template // empty
' .amai/onboarding/project-chat-startup-contract.json)"

printf '%s\n' "${output}" | jq -Rn \
  --arg expected_prefix "5ч KPI: переплата 12.34%" \
  --arg expected_template "${expected_template}" '
  [inputs] as $lines
  | ($lines | length) >= 2
    and $lines[0] == $expected_prefix
    and $lines[1] == $expected_template
' >/dev/null

echo "proof_client_budget_reply_gate_cache_fallback: PASS"
