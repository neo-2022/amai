#!/usr/bin/env bash
set -euo pipefail

# Phase D proof: workspace restore pack enrichment.
# Creates a clean temporary project, records a handoff with an active file path,
# seeds runtime state via continuity_startup, and checks that the returned bundle
# contains recent_thread_highlights and active_editor_state in the
# workspace_restore_pack.

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:/usr/bin:/bin"

cargo build --release --quiet

temp_repo="$(mktemp -d)"
temp_details="$(mktemp)"
temp_bundle="$(mktemp)"
cleanup() {
  rm -rf "${temp_repo}" "${temp_details}" "${temp_bundle}"
}
trap cleanup EXIT

project_code="proof_restore_pack_enrichment_$(date +%s%N)"

# The handoff details text must contain an absolute /home/... path so that the
# working_state event picks it up as an active file.
cat > "${temp_details}" <<'EOF'
Phase D restore pack enrichment proof.
Active file: /home/art/agent-memory-index/src/mcp.rs
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
jq -e '.workspace_restore_pack.active_editor_state.open_files | index("/home/art/agent-memory-index/src/mcp.rs") != null' "${temp_bundle}" >/dev/null
jq -e '.workspace_restore_pack.active_editor_state.focused_file != null' "${temp_bundle}" >/dev/null
jq -e '.workspace_restore_pack | has("recent_thread_highlights")' "${temp_bundle}" >/dev/null

echo "proof_workspace_restore_pack_enrichment: ok"
