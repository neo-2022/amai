#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

source "${SCRIPT_DIR}/load_env.sh"

resolve_in_subshell() {
  local setup="${1:-}"
  (
    unset AMAI_PLATFORM_THREAD_ID CODEX_THREAD_ID
    if [[ -n "${setup}" ]]; then
      eval "${setup}"
    fi
    resolve_amai_thread_id_or_empty_from_env
  )
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local label="$3"
  if [[ "${expected}" != "${actual}" ]]; then
    echo "proof_thread_binding_env_aliases: ${label}: expected '${expected}', got '${actual}'" >&2
    exit 1
  fi
}

assert_eq "" "$(resolve_in_subshell)" "missing aliases"
assert_eq "legacy-thread" \
  "$(resolve_in_subshell 'CODEX_THREAD_ID=legacy-thread')" \
  "legacy alias fallback"
assert_eq "platform-thread" \
  "$(resolve_in_subshell 'AMAI_PLATFORM_THREAD_ID=platform-thread; CODEX_THREAD_ID=platform-thread')" \
  "matching aliases"
assert_eq "trimmed-thread" \
  "$(resolve_in_subshell "AMAI_PLATFORM_THREAD_ID='  trimmed-thread  '")" \
  "trimmed platform alias"

conflict_stderr="$(mktemp)"
if (
  unset AMAI_PLATFORM_THREAD_ID CODEX_THREAD_ID
  AMAI_PLATFORM_THREAD_ID=platform-thread
  CODEX_THREAD_ID=legacy-thread
  resolve_amai_thread_id_from_env
) > /dev/null 2>"${conflict_stderr}"; then
  echo "proof_thread_binding_env_aliases: conflicting aliases unexpectedly resolved" >&2
  rm -f "${conflict_stderr}"
  exit 1
fi
grep -Fq "conflicting thread identity aliases" "${conflict_stderr}"

if (
  unset AMAI_PLATFORM_THREAD_ID CODEX_THREAD_ID
  AMAI_PLATFORM_THREAD_ID=platform-thread
  CODEX_THREAD_ID=legacy-thread
  resolve_amai_thread_id_or_empty_from_env
) > /dev/null 2>"${conflict_stderr}"; then
  echo "proof_thread_binding_env_aliases: conflict-tolerant wrapper unexpectedly swallowed alias conflict" >&2
  rm -f "${conflict_stderr}"
  exit 1
fi
grep -Fq "conflicting thread identity aliases" "${conflict_stderr}"
rm -f "${conflict_stderr}"

for consumer in \
  "${SCRIPT_DIR}/client_budget_gate.sh" \
  "${SCRIPT_DIR}/client_budget_root_cause.sh"
do
  conflict_stderr="$(mktemp)"
  if (
    unset AMAI_PLATFORM_THREAD_ID CODEX_THREAD_ID
    AMAI_PLATFORM_THREAD_ID=platform-thread \
      CODEX_THREAD_ID=legacy-thread \
      AMI_OBSERVE_BIND=127.0.0.1:1 \
      "${consumer}" --enforce-reply-gate
  ) > /dev/null 2>"${conflict_stderr}"; then
    echo "proof_thread_binding_env_aliases: $(basename "${consumer}") swallowed alias conflict" >&2
    rm -f "${conflict_stderr}"
    exit 1
  fi
  grep -Fq "conflicting thread identity aliases" "${conflict_stderr}"
  rm -f "${conflict_stderr}"
done

echo "proof_thread_binding_env_aliases: PASS"
