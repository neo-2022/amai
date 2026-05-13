#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${REPO_ROOT}/scripts/load_env.sh"

cd "${REPO_ROOT}"

cargo test --quiet postgres::tests::memory_policy_scope_filter_rejects_quarantine_visibility -- --exact
cargo test --quiet postgres::tests::memory_card_policy_scope_filter_rejects_quarantine_visibility -- --exact
cargo test --quiet postgres::tests::task_node_policy_scope_filter_rejects_quarantine_visibility -- --exact
cargo test --quiet postgres::tests::skill_card_policy_scope_filter_rejects_quarantine_visibility -- --exact
cargo test --quiet postgres::tests::create_memory_item_requires_controlled_import_packet_for_cross_project_basis -- --exact
cargo test --quiet postgres::tests::create_memory_item_rejects_import_packet_target_mismatch -- --exact
cargo test --quiet postgres::tests::ensure_shared_asset_surfaces_stage2_provenance_fields_for_org_global -- --exact
cargo test --quiet postgres::tests::bind_shared_asset_to_project_allows_org_global_within_workspace -- --exact
cargo test --quiet postgres::tests::bind_shared_asset_to_project_uses_workspace_scoped_asset_lookup -- --exact

printf 'proof_shared_private_memory_hardening: ok\n'
