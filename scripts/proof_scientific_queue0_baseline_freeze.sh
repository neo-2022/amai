#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_root="$(mktemp -d)"
output_dir="${tmp_root}/baseline-freeze"
allowlist_path="${tmp_root}/allowlist.txt"
manifest_path=""

cleanup() {
  rm -rf "${tmp_root}"
}

trap cleanup EXIT

mapfile -t plan_lines < <(./scripts/scientific_queue0_baseline_freeze.sh --print-plan)

expected_labels=(
  "agent_preflight"
  "maintainability_gate"
  "benchmark_coverage"
  "proof_memory_task_matrix"
  "proof_mcp_task_matrix"
  "proof_forgetting_consolidation"
  "proof_observability"
)

if [[ "${#plan_lines[@]}" -ne "${#expected_labels[@]}" ]]; then
  echo "proof_scientific_queue0_baseline_freeze: unexpected print-plan length"
  exit 1
fi

for i in "${!expected_labels[@]}"; do
  label="${expected_labels[$i]}"
  if [[ "${plan_lines[$i]}" != "${label}"$'\t'* ]]; then
    echo "proof_scientific_queue0_baseline_freeze: missing or reordered label ${label}"
    exit 1
  fi
done

cat > "${allowlist_path}" <<'EOF'
# queue0 allowlist smoke fixture
proof_observability
EOF

set +e
manifest_path="$(./scripts/scientific_queue0_baseline_freeze.sh \
  --output-dir "${output_dir}" \
  --baseline-allowlist "${allowlist_path}" \
  --require-clean-worktree)"
exit_code=$?
set -e

if [[ "${exit_code}" -ne 2 ]]; then
  echo "proof_scientific_queue0_baseline_freeze: dirty-worktree preflight should exit 2, got ${exit_code}"
  exit 1
fi

test -n "${manifest_path}"
test -f "${manifest_path}"
test -L "${output_dir}/latest"

jq -e '
  .artifact_version == "scientific-queue0-baseline-freeze-v1" and
  .queue_code == "queue0_preflight_baseline_freeze" and
  .status == "preflight_failed_dirty_worktree" and
  .require_clean_worktree == true and
  .command_count == 0 and
  .status_counts.passed == 0 and
  .status_counts.pre_existing_known_failure == 0 and
  .status_counts.current_failure == 0 and
  (.commands | type == "array" and length == 0) and
  (.dirty_worktree_excerpt | type == "array" and length > 0) and
  .baseline_allowlist.path != null and
  .baseline_allowlist.sha256 != null and
  (.baseline_allowlist.labels | type == "array" and length == 0) and
  .worktree_fingerprints.initial_sha256 != null and
  .worktree_fingerprints.final_sha256 == null
' "${manifest_path}" >/dev/null

echo "proof_scientific_queue0_baseline_freeze: ok"
