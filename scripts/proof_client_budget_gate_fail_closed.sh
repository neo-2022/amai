#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

backup_dir="$(mktemp -d)"

move_if_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    mkdir -p "${backup_dir}/$(dirname "$path")"
    mv "$path" "${backup_dir}/$path"
  fi
}

restore_all() {
  local path
  for path in \
    scripts/client_budget_root_cause.sh \
    scripts/amai_exec.sh \
    target/release/amai \
    target/debug/amai \
    state/observe/client_budget_surfaces_cache.json \
    state/observe/client_budget_gate_cache.json; do
    if [[ -e "${backup_dir}/$path" ]]; then
      mkdir -p "$(dirname "$path")"
      mv "${backup_dir}/$path" "$path"
    fi
  done
  rm -rf "${backup_dir}"
}

trap restore_all EXIT

move_if_exists scripts/client_budget_root_cause.sh
move_if_exists scripts/amai_exec.sh
move_if_exists target/release/amai
move_if_exists target/debug/amai
move_if_exists state/observe/client_budget_surfaces_cache.json
move_if_exists state/observe/client_budget_gate_cache.json

status=0
if env -u CODEX_THREAD_ID PATH=/usr/bin:/bin AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/client_budget_gate.sh --enforce-reply-gate \
  >/tmp/proof_client_budget_gate_fail_closed.out \
  2>/tmp/proof_client_budget_gate_fail_closed.err; then
  echo "proof_client_budget_gate_fail_closed: expected client_budget_gate.sh to fail closed without any payload source" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_client_budget_gate_fail_closed: expected exit code 12, got ${status}" >&2
  cat /tmp/proof_client_budget_gate_fail_closed.err >&2 || true
  exit 1
fi

grep -Fq "client budget gate: no gate payload available" /tmp/proof_client_budget_gate_fail_closed.err

echo "proof_client_budget_gate_fail_closed: PASS"
