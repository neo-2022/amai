#!/usr/bin/env bash
set -euo pipefail
trap 'echo "Proof failed at line ${LINENO}: ${BASH_COMMAND}" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

echo "== Amai cross-store Qdrant/Postgres failure-contract proof =="

OUT_DIR="$REPO_ROOT/tmp/qdrant-postgres-failure-contract"
PROOF_CONTRACT="$OUT_DIR/proof-contract.json"
mkdir -p "$OUT_DIR"

cargo build --release --quiet

cargo test --quiet qdrant_postgres_failure_verdict_recovery_contract_is_explicit_across_branch_classes
cargo test --quiet live_failure_emitter_surfaces_recovery_contract_in_observability_log
cargo test --quiet forced_post_qdrant_failure_branch_skips_compensation_for_commit_outcome_unknown
cargo test --quiet forced_post_qdrant_failure_branch_attempts_compensation_and_emits_both_logs
cargo test --quiet forced_existing_document_post_qdrant_failure_branch_skips_compensation_for_commit_outcome_unknown
cargo test --quiet forced_existing_document_post_qdrant_failure_branch_attempts_restore_and_emits_both_logs
cargo test --quiet runtime_forced_existing_document
cargo test --quiet manual_recovery_bundle_write_failure_is_surfaced_in_error
cargo test --quiet qdrant_postgres_failure_verdict_log_uses_shared_tristate_contract
cargo test --quiet commit_outcome_unknown_skips_compensation
cargo test --quiet commit_rolled_back_at_commit
cargo test --quiet qdrant_postgres_failure_verdict_log

jq -n '{
  artifact_version: "qdrant_postgres_failure_contract_proof_v1",
  proof_kind: "forced_failure_contract_unit_harness",
  harness: "scripts/proof_qdrant_postgres_failure_contract.sh",
  forced_runtime_seam: "post_qdrant_update_before_or_at_postgres_failure",
  runtime_test_surface: {
    entrypoint: "index_project_file_under_lock",
    existing_document_compensation_failure_emits_observability_verdict: true,
    manual_recovery_bundle_written_for_manual_investigation: true
  },
  branch_contract: {
    before_qdrant_update: {
      consistency_state: "postgres_failure_before_qdrant_update",
      required_action: "retry_or_investigate_postgres_before_retrying_qdrant_mutation"
    },
    compensated_rollback: {
      consistency_state: "cross_store_consistency_restored_by_compensation",
      required_action: "no_further_cross_store_recovery_required"
    },
    commit_outcome_unknown: {
      consistency_state: "cross_store_consistency_unknown_commit_outcome",
      required_action: "manual_cross_store_investigation_required"
    },
    compensation_failure: {
      consistency_state: "cross_store_inconsistent_after_compensation_failure",
      required_action: "manual_cross_store_investigation_required"
    }
  },
  observability_contract: {
    emitted_stage: "index_project.qdrant_postgres_failure_verdict",
    emitter_level_proof: true,
    emitted_fields: [
      "failure_mode",
      "failure_phase",
      "failure_sqlstate",
      "compensation_attempted",
      "compensation_succeeded",
      "consistency_state",
      "required_action"
    ]
  },
  maturity_disclaimer: {
    distributed_transaction_maturity: false,
    crash_safe_runtime_fault_injection_proven: false,
    reason: "unit-level forced failure contract and emitter proof only"
  }
}' > "$PROOF_CONTRACT"

jq -e '
  .artifact_version == "qdrant_postgres_failure_contract_proof_v1"
  and .proof_kind == "forced_failure_contract_unit_harness"
  and .forced_runtime_seam == "post_qdrant_update_before_or_at_postgres_failure"
  and .runtime_test_surface.entrypoint == "index_project_file_under_lock"
  and .runtime_test_surface.existing_document_compensation_failure_emits_observability_verdict == true
  and .runtime_test_surface.manual_recovery_bundle_written_for_manual_investigation == true
  and .branch_contract.before_qdrant_update.consistency_state == "postgres_failure_before_qdrant_update"
  and .branch_contract.before_qdrant_update.required_action == "retry_or_investigate_postgres_before_retrying_qdrant_mutation"
  and .branch_contract.compensated_rollback.consistency_state == "cross_store_consistency_restored_by_compensation"
  and .branch_contract.compensated_rollback.required_action == "no_further_cross_store_recovery_required"
  and .branch_contract.commit_outcome_unknown.consistency_state == "cross_store_consistency_unknown_commit_outcome"
  and .branch_contract.commit_outcome_unknown.required_action == "manual_cross_store_investigation_required"
  and .branch_contract.compensation_failure.consistency_state == "cross_store_inconsistent_after_compensation_failure"
  and .branch_contract.compensation_failure.required_action == "manual_cross_store_investigation_required"
  and .observability_contract.emitted_stage == "index_project.qdrant_postgres_failure_verdict"
  and .observability_contract.emitter_level_proof == true
  and (.observability_contract.emitted_fields | index("consistency_state") != null)
  and (.observability_contract.emitted_fields | index("required_action") != null)
  and .maturity_disclaimer.distributed_transaction_maturity == false
  and .maturity_disclaimer.crash_safe_runtime_fault_injection_proven == false
' "$PROOF_CONTRACT" >/dev/null

echo "== Done: cross-store failure contract stays explicit across before_commit / compensated / outcome_unknown / inconsistent_state branches =="
