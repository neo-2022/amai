#!/usr/bin/env bash
set -euo pipefail

# Phase D proof: workspace restore pack enrichment.
# Creates a clean temporary project, records a handoff with an active file path,
# seeds runtime state via continuity_startup, and checks that the returned bundle
# contains active_editor_state and does not attach unrelated thread highlights.

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:/usr/bin:/bin"

cargo build --release --quiet

proof_root="${HOME}/amai-proof-workspaces"
mkdir -p "${proof_root}"
temp_repo="$(mktemp -d -p "${proof_root}" restore-pack-enrichment.XXXXXX)"
temp_details="$(mktemp)"
temp_bundle="$(mktemp)"
cleanup() {
  rm -rf "${temp_repo}" "${temp_details}" "${temp_bundle}"
}
trap cleanup EXIT

project_code="proof_restore_pack_enrichment_$(date +%s%N)"

# The handoff details text contains a file from the temporary project so the
# working_state event picks it up as an active file without leaking repo paths.
active_file="${temp_repo}/src/proof.rs"
mkdir -p "$(dirname "${active_file}")"
touch "${active_file}"
cat > "${temp_details}" <<EOF
Phase D restore pack enrichment proof.
Active file: ${active_file}
EOF

cargo run --release --quiet -- project register \
  --code "${project_code}" \
  --display-name "Proof restore pack enrichment" \
  --repo-root "${temp_repo}" \
  --workspace default >/dev/null
cargo run --release --quiet -- namespace ensure \
  --project "${project_code}" \
  --code continuity \
  --display-name Continuity >/dev/null
cargo run --release --quiet -- continuity handoff \
  --project "${project_code}" \
  --namespace continuity \
  --headline "Phase D active editor state" \
  --next-step "Verify recent_thread_highlights and active_editor_state in restore pack" \
  --details-file "${temp_details}" \
  --promote-active-workline >/dev/null

./scripts/continuity_startup.sh \
  --repo-root "${temp_repo}" \
  --namespace continuity \
  --json >"${temp_bundle}"

jq -e '.workspace_restore_pack.pack_kind == "workspace_restore_pack"' "${temp_bundle}" >/dev/null
jq -e --arg active_file "${active_file}" '.workspace_restore_pack.active_editor_state.open_files | index($active_file) != null' "${temp_bundle}" >/dev/null
jq -e --arg active_file "${active_file}" '.workspace_restore_pack.active_editor_state.focused_file == $active_file' "${temp_bundle}" >/dev/null
jq -e '.workspace_restore_pack.recent_thread_highlights == null' "${temp_bundle}" >/dev/null

echo "proof_workspace_restore_pack_enrichment: ok"
