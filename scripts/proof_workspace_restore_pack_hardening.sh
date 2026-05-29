#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Awarnings"
export CARGO_TERM_COLOR=never

step() {
  echo "[proof_workspace_restore_pack_hardening] $*"
}

step "stale replay guard remains fail-closed for continuity handoff/import selection"
cargo test --quiet continuity::tests::latest_handoff_selection_prefers_semantic_capture_time_over_replay_created_at -- --exact
cargo test --quiet continuity::tests::latest_import_selection_prefers_semantic_import_time_over_replay_created_at -- --exact

step "workspace restore pack rejects missing or mismatched source snapshots"
cargo test --quiet postgres::tests::create_restore_pack_policy_scope_filter_requires_source_snapshot_for_workspace_restore_pack -- --exact
cargo test --quiet postgres::tests::create_restore_pack_policy_scope_filter_rejects_snapshot_scope_mismatch -- --exact

step "workspace restore pack rejects poisoned evidence span"
cargo test --quiet postgres::tests::create_restore_pack_verification_conflict_check_detects_poisoned_evidence_span -- --exact

step "workspace restore pack builder stays fail-closed on malformed or raw-procedural surfaces"
cargo test --quiet working_state::tests::build_workspace_restore_pack_handles_malformed_surface_fail_closed -- --exact
cargo test --quiet working_state::tests::build_workspace_restore_pack_does_not_surface_raw_procedural_note_without_card -- --exact

step "workspace restore pack hardening proof passed"
