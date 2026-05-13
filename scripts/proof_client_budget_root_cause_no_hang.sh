#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

AMAI_EXEC_PATH="${SCRIPT_DIR}/amai_exec.sh"
CACHE_PATH="${REPO_ROOT}/state/observe/client_budget_surfaces_cache.json"

backup_path="$(mktemp)"
cp "${AMAI_EXEC_PATH}" "${backup_path}"

cleanup() {
  mv -f "${backup_path}" "${AMAI_EXEC_PATH}"
  chmod +x "${AMAI_EXEC_PATH}" || true
  rm -f "${CACHE_PATH}"
}
trap cleanup EXIT

# Force the script down the "no cache + api down + local observe fallback" path.
rm -f "${CACHE_PATH}"

cat >"${AMAI_EXEC_PATH}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
sleep 60
EOF
chmod +x "${AMAI_EXEC_PATH}"

start_ms="$(date +%s%3N)"
set +e
AMI_OBSERVE_BIND=127.0.0.1:1 "${SCRIPT_DIR}/client_budget_root_cause.sh" >/tmp/proof_client_budget_root_cause_no_hang.out 2>/tmp/proof_client_budget_root_cause_no_hang.err
status=$?
set -e
end_ms="$(date +%s%3N)"
elapsed_ms=$(( end_ms - start_ms ))

# This proof is about "no hang". Any non-zero exit is acceptable, but it must return quickly.
if [[ "${elapsed_ms}" -gt 16000 ]]; then
  echo "proof_client_budget_root_cause_no_hang: FAIL (elapsed_ms=${elapsed_ms}, status=${status})" >&2
  echo "stdout_bytes=$(wc -c </tmp/proof_client_budget_root_cause_no_hang.out)" >&2
  echo "stderr_bytes=$(wc -c </tmp/proof_client_budget_root_cause_no_hang.err)" >&2
  exit 1
fi

if [[ "${status}" -eq 0 ]]; then
  echo "proof_client_budget_root_cause_no_hang: FAIL (expected non-zero exit on forced hang, got status=0, elapsed_ms=${elapsed_ms})" >&2
  exit 1
fi

echo "proof_client_budget_root_cause_no_hang: PASS (elapsed_ms=${elapsed_ms}, status=${status})"

