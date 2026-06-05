#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_file=".amai/continuity/project-chat-startup-state.json"
signoff_input_file=".amai/continuity/specialist-team-signoff-input.json"
signoff_source_manifest=".amai/continuity/specialist-team-signoff-source.json"
signoff_file=".amai/continuity/specialist-team-signoff.json"
signature_file=".amai/continuity/specialist-team-signoff-source.json.sig"
allowed_signers="${HOME}/.local/share/amai/signoff-trust/allowed_signers"
signature_namespace="amai-specialist-signoff"
signoff_lock_dir="state/locks"
signoff_lock_file="${signoff_lock_dir}/before_report_signoff.lock"
startup_state_lock_file="${signoff_lock_dir}/startup_runtime_state_mutation.lock"

step() {
  echo "[proof_before_report] $*"
}

die() {
  echo "[proof_before_report] $*" >&2
  exit 1
}

canonical_path() {
  readlink -f "$1"
}

assert_existing_regular_not_symlink() {
  local path="$1"
  test -f "${path}" || die "missing required file: ${path}"
  [[ ! -L "${path}" ]] || die "symlink is not allowed: ${path}"
}

assert_outside_worktree() {
  local path="$1"
  local workspace_canon path_canon
  workspace_canon="$(canonical_path .)"
  path_canon="$(canonical_path "${path}")"
  case "${path_canon}" in
    "${workspace_canon}" | "${workspace_canon}"/*)
      die "trust root must stay outside worktree: ${path_canon}"
      ;;
  esac
}

assert_not_group_or_world_writable() {
  local path="$1"
  local mode links owner
  mode="$(stat -c '%a' "${path}")"
  links="$(stat -c '%h' "${path}")"
  owner="$(stat -c '%U' "${path}")"
  if [[ "${owner}" != "$(id -un)" ]]; then
    die "path is not owned by current user: ${path}"
  fi
  if (( (8#${mode} & 8#0022) != 0 )); then
    die "path is group/world writable: ${path}"
  fi
  if [[ -f "${path}" && "${links}" != "1" ]]; then
    die "hard-linked trust file is not allowed: ${path}"
  fi
}

workflow_redirect_identity() {
  jq -r '
    [
      (.workflow_promotion_state.current_user_redirect_id // ""),
      (.workflow_promotion_state.promoted_user_redirect_id // ""),
      (.working_state_restore_lineage.authoritative_event_id // ""),
      (.continuity_startup_summary.execctl_active_lease.source_event_id // "")
    ] | @tsv
  ' "${state_file}"
}

capture_workflow_redirect_identity() {
  test -f "${state_file}" || die "missing startup runtime state: ${state_file}"
  local current promoted lineage lease
  IFS=$'\t' read -r current promoted lineage lease < <(workflow_redirect_identity)
  test -n "${current}" || die "missing current user redirect id"
  test -n "${promoted}" || die "missing promoted user redirect id"
  test -n "${lineage}" || die "missing working-state lineage event id"
  test -n "${lease}" || die "missing active lease source event id"
  [[ "${current}" == "${promoted}" ]] || die "current/promoted redirect mismatch"
  [[ "${current}" == "${lineage}" ]] || die "current redirect does not match working-state lineage"
  [[ "${current}" == "${lease}" ]] || die "current redirect does not match active lease source event"
  printf '%s\t%s\t%s\t%s\n' "${current}" "${promoted}" "${lineage}" "${lease}"
}

assert_same_workflow_redirect_identity() {
  local expected="$1"
  local phase="$2"
  local actual
  actual="$(capture_workflow_redirect_identity)"
  [[ "${actual}" == "${expected}" ]] || die "${phase}: workflow redirect identity changed during before-report proof"
}

workflow_semantic_identity() {
  jq -r '
    [
      (.workflow_promotion_state.active_workline_headline // ""),
      (.workflow_promotion_state.active_lease_headline // ""),
      (.continuity_startup_summary.headline // ""),
      (.continuity_startup_summary.execctl_active_lease.headline // ""),
      (.execctl_active_lease.headline // ""),
      (.working_state_restore_lineage.authoritative_headline // ""),
      (.workflow_promotion_state.active_workline_source_kind // ""),
      (.workflow_promotion_state.active_lease_source_kind // ""),
      (.continuity_startup_summary.execctl_active_lease.source_kind // ""),
      (.execctl_active_lease.source_kind // ""),
      (.working_state_restore_lineage.authoritative_source_kind // ""),
      ((.workflow_promotion_state.headline_match // false) | tostring),
      ((.workflow_promotion_state.source_kind_match // false) | tostring)
    ] | @tsv
  ' "${state_file}"
}

capture_workflow_semantic_identity() {
  test -f "${state_file}" || die "missing startup runtime state: ${state_file}"
  local active lease summary summary_lease top_lease lineage source_workline source_lease source_summary source_top source_lineage headline_match source_kind_match
  IFS=$'\t' read -r active lease summary summary_lease top_lease lineage source_workline source_lease source_summary source_top source_lineage headline_match source_kind_match < <(workflow_semantic_identity)
  test -n "${active}" || die "missing workflow active workline headline"
  test -n "${lease}" || die "missing workflow active lease headline"
  test -n "${summary}" || die "missing startup summary headline"
  test -n "${summary_lease}" || die "missing startup summary active lease headline"
  test -n "${top_lease}" || die "missing top-level active lease headline"
  test -n "${lineage}" || die "missing lineage authoritative headline"
  test -n "${source_workline}" || die "missing workflow active workline source_kind"
  test -n "${source_lease}" || die "missing workflow active lease source_kind"
  test -n "${source_summary}" || die "missing startup summary active lease source_kind"
  test -n "${source_top}" || die "missing top-level active lease source_kind"
  test -n "${source_lineage}" || die "missing lineage authoritative source_kind"
  [[ "${headline_match}" == "true" ]] || die "workflow promotion headline_match is not true"
  [[ "${source_kind_match}" == "true" ]] || die "workflow promotion source_kind_match is not true"
  [[ "${active}" == "${lease}" ]] || die "workflow active headline mismatch"
  [[ "${active}" == "${summary}" ]] || die "startup summary headline mismatch"
  [[ "${active}" == "${summary_lease}" ]] || die "startup summary active lease headline mismatch"
  [[ "${active}" == "${top_lease}" ]] || die "top-level active lease headline mismatch"
  [[ "${active}" == "${lineage}" ]] || die "lineage authoritative headline mismatch"
  [[ "${source_workline}" == "${source_lease}" ]] || die "workflow source_kind mismatch"
  [[ "${source_workline}" == "${source_summary}" ]] || die "startup summary active lease source_kind mismatch"
  [[ "${source_workline}" == "${source_top}" ]] || die "top-level active lease source_kind mismatch"
  [[ "${source_workline}" == "${source_lineage}" ]] || die "lineage authoritative source_kind mismatch"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${active}" "${lease}" "${summary}" "${summary_lease}" "${top_lease}" "${lineage}" \
    "${source_workline}" "${source_lease}" "${source_summary}" "${source_top}" "${source_lineage}" \
    "${headline_match}" "${source_kind_match}"
}

assert_same_workflow_semantic_identity() {
  local expected="$1"
  local phase="$2"
  local actual
  actual="$(capture_workflow_semantic_identity)"
  [[ "${actual}" == "${expected}" ]] || die "${phase}: workflow semantic identity changed during before-report proof"
}

consensus_fingerprint() {
  local file="$1"
  cargo run --quiet -- verify workflow-trace \
    --state "${state_file}" \
    --startup-contract ".amai/onboarding/project-chat-startup-contract.json" \
    --input "${file}" \
    | jq -r '.consensus_fingerprint'
}

signoff_tuple_hash() {
  for path in "${signoff_source_manifest}" "${signature_file}" "${signoff_file}"; do
    test -f "${path}" || die "missing signoff tuple file: ${path}"
    sha256sum "${path}"
  done | sha256sum | awk '{print $1}'
}

signed_manifest_redirect_identity() {
  local file="$1"
  jq -r '
    [
      (.redirect_identity.current_user_redirect_id // ""),
      (.redirect_identity.promoted_user_redirect_id // ""),
      (.redirect_identity.working_state_lineage_authoritative_event_id // ""),
      (.redirect_identity.active_lease_source_event_id // "")
    ] | @tsv
  ' "${file}"
}

assert_signed_manifest_redirect_identity_matches_expected() {
  local expected="$1"
  local actual current promoted lineage lease
  actual="$(signed_manifest_redirect_identity "${signoff_source_manifest}")"
  IFS=$'\t' read -r current promoted lineage lease < <(printf '%s\n' "${actual}")
  test -n "${current}" || die "signed manifest missing redirect_identity.current_user_redirect_id"
  test -n "${promoted}" || die "signed manifest missing redirect_identity.promoted_user_redirect_id"
  test -n "${lineage}" || die "signed manifest missing redirect_identity.working_state_lineage_authoritative_event_id"
  test -n "${lease}" || die "signed manifest missing redirect_identity.active_lease_source_event_id"
  [[ "${actual}" == "${expected}" ]] || die "signed manifest redirect identity does not match the current startup state"
}

assert_signed_consensus_manifest_matches_input() {
  assert_existing_regular_not_symlink "${signoff_input_file}"
  assert_existing_regular_not_symlink "${signoff_source_manifest}"
  assert_existing_regular_not_symlink "${signature_file}"
  assert_existing_regular_not_symlink "${allowed_signers}"
  assert_outside_worktree "${allowed_signers}"
  assert_not_group_or_world_writable "$(dirname "${allowed_signers}")"
  assert_not_group_or_world_writable "${allowed_signers}"
  ssh-keygen -Y verify \
    -f "${allowed_signers}" \
    -I "amai-specialist-signoff" \
    -n "${signature_namespace}" \
    -s "${signature_file}" \
    <"${signoff_source_manifest}" >/dev/null 2>&1 \
    || die "signed specialist consensus source manifest verification failed"
  assert_signed_manifest_redirect_identity_matches_expected "$(capture_workflow_redirect_identity)"
  local input_consensus_hash signed_consensus_hash
  input_consensus_hash="$(consensus_fingerprint "${signoff_input_file}")"
  signed_consensus_hash="$(consensus_fingerprint "${signoff_source_manifest}")"
  [[ "${input_consensus_hash}" == "${signed_consensus_hash}" ]] \
    || die "signoff input does not match the previously signed consensus"
}

assert_signoff_input_matches_signed_consensus() {
  local expected_consensus_hash="$1"
  test -f "${signoff_input_file}" || die "missing signoff input: ${signoff_input_file}"
  test -f "${signoff_source_manifest}" || die "missing signed signoff source manifest: ${signoff_source_manifest}"
  local input_consensus_hash signed_consensus_hash
  input_consensus_hash="$(consensus_fingerprint "${signoff_input_file}")"
  signed_consensus_hash="$(consensus_fingerprint "${signoff_source_manifest}")"
  [[ "${signed_consensus_hash}" == "${expected_consensus_hash}" ]] || die "signed consensus changed during before-report proof"
  [[ "${input_consensus_hash}" == "${expected_consensus_hash}" ]] || die "signoff input does not match the previously signed consensus"
}

ensure_preexisting_specialist_signoff_guard() {
  ./scripts/proof_specialist_signoff.sh >/dev/null \
    || die "specialist signoff is missing or stale; run ./scripts/materialize_specialist_signoff.sh ${signoff_input_file} before proof_before_report"
}

refresh_startup_and_verify_signoff() {
  local expected_identity="$1"
  local expected_semantic_identity="$2"
  local expected_consensus_hash="$3"
  mkdir -p "${signoff_lock_dir}"
  (
    flock --exclusive 9
    flock --exclusive 8
    refresh_startup_and_verify_signoff_locked "${expected_identity}" "${expected_semantic_identity}" "${expected_consensus_hash}"
  ) 9>"${signoff_lock_file}" 8>"${startup_state_lock_file}"
}

refresh_startup_and_verify_signoff_locked() {
  local expected_identity="$1"
  local expected_semantic_identity="$2"
  local expected_consensus_hash="$3"
  ./scripts/continuity_startup.sh --repo-root "$(pwd)" --namespace continuity --json >/dev/null
  assert_same_workflow_redirect_identity "${expected_identity}" "startup refresh"
  assert_same_workflow_semantic_identity "${expected_semantic_identity}" "startup refresh"
  assert_signed_manifest_redirect_identity_matches_expected "${expected_identity}"
  assert_signoff_input_matches_signed_consensus "${expected_consensus_hash}"
  ./scripts/proof_specialist_signoff.sh >/dev/null
  assert_same_workflow_redirect_identity "${expected_identity}" "post-signoff verification"
  assert_same_workflow_semantic_identity "${expected_semantic_identity}" "post-signoff verification"
  assert_signoff_input_matches_signed_consensus "${expected_consensus_hash}"
}

proof_before_report_guard_self_test() {
  local original_state_file original_signoff_input_file original_signoff_source_manifest
  local original_signature_file original_allowed_signers original_signoff_file
  original_state_file="${state_file}"
  original_signoff_input_file="${signoff_input_file}"
  original_signoff_source_manifest="${signoff_source_manifest}"
  original_signature_file="${signature_file}"
  original_allowed_signers="${allowed_signers}"
  original_signoff_file="${signoff_file}"
  local tmp_dir
  tmp_dir="$(mktemp -d)"

  cp "${original_state_file}" "${tmp_dir}/state.json"
  cp "${original_signoff_input_file}" "${tmp_dir}/signoff-input.json"
  cp "${original_signoff_source_manifest}" "${tmp_dir}/signoff-source.json"
  cp "${original_signature_file}" "${tmp_dir}/signoff-source.json.sig"
  cp "${original_allowed_signers}" "${tmp_dir}/allowed_signers"
  cp "${original_signoff_file}" "${tmp_dir}/signoff.json"

  state_file="${tmp_dir}/state.json"
  signoff_input_file="${tmp_dir}/signoff-input.json"
  signoff_source_manifest="${tmp_dir}/signoff-source.json"
  signature_file="${tmp_dir}/signoff-source.json.sig"
  allowed_signers="${tmp_dir}/allowed_signers"
  signoff_file="${tmp_dir}/signoff.json"

  local expected_identity expected_semantic expected_consensus aligned_state_file
  expected_identity="$(capture_workflow_redirect_identity)"
  expected_semantic="$(capture_workflow_semantic_identity)"
  expected_consensus="$(consensus_fingerprint "${signoff_source_manifest}")"
  aligned_state_file="${tmp_dir}/state-aligned-to-signed-manifest.json"
  jq \
    --slurpfile manifest "${tmp_dir}/signoff-source.json" '
      ($manifest[0].redirect_identity // {}) as $identity
      | .workflow_promotion_state.current_user_redirect_id =
          ($identity.current_user_redirect_id // .workflow_promotion_state.current_user_redirect_id)
      | .workflow_promotion_state.promoted_user_redirect_id =
          ($identity.promoted_user_redirect_id // .workflow_promotion_state.promoted_user_redirect_id)
      | .working_state_restore_lineage.authoritative_event_id =
          ($identity.working_state_lineage_authoritative_event_id // .working_state_restore_lineage.authoritative_event_id)
      | .continuity_startup_summary.execctl_active_lease.source_event_id =
          ($identity.active_lease_source_event_id // .continuity_startup_summary.execctl_active_lease.source_event_id)
    ' "${tmp_dir}/state.json" >"${aligned_state_file}"

  jq '.workflow_promotion_state.promoted_user_redirect_id = "forged-redirect"' \
    "${tmp_dir}/state.json" >"${tmp_dir}/state-mismatch.json"
  state_file="${tmp_dir}/state-mismatch.json"
  if (assert_same_workflow_redirect_identity "${expected_identity}" "self-test redirect mismatch") 2>/dev/null; then
    die "self-test: redirect mismatch unexpectedly passed"
  fi

  state_file="${aligned_state_file}"
  assert_signed_consensus_manifest_matches_input
  jq '
    .workflow_promotion_state.current_user_redirect_id = "replayed-redirect"
    | .workflow_promotion_state.promoted_user_redirect_id = "replayed-redirect"
    | .working_state_restore_lineage.authoritative_event_id = "replayed-redirect"
    | .continuity_startup_summary.execctl_active_lease.source_event_id = "replayed-redirect"
  ' "${aligned_state_file}" >"${tmp_dir}/state-replayed-identity.json"
  state_file="${tmp_dir}/state-replayed-identity.json"
  if (assert_signed_consensus_manifest_matches_input) 2>/dev/null; then
    die "self-test: replayed redirect identity unexpectedly passed"
  fi

  state_file="${aligned_state_file}"
  jq '.workflow_promotion_state.active_workline_headline = "forged active line"
      | .workflow_promotion_state.headline_match = true' \
    "${aligned_state_file}" >"${tmp_dir}/state-headline-forged.json"
  state_file="${tmp_dir}/state-headline-forged.json"
  if (assert_same_workflow_semantic_identity "${expected_semantic}" "self-test semantic headline drift") 2>/dev/null; then
    die "self-test: semantic headline drift unexpectedly passed"
  fi

  state_file="${aligned_state_file}"
  jq '.workflow_promotion_state.active_workline_source_kind = "context_pack"
      | .workflow_promotion_state.source_kind_match = true' \
    "${aligned_state_file}" >"${tmp_dir}/state-source-kind-forged.json"
  state_file="${tmp_dir}/state-source-kind-forged.json"
  if (assert_same_workflow_semantic_identity "${expected_semantic}" "self-test semantic source_kind drift") 2>/dev/null; then
    die "self-test: semantic source_kind drift unexpectedly passed"
  fi

  state_file="${aligned_state_file}"
  jq '.open_objections_count = 1' \
    "${tmp_dir}/signoff-input.json" >"${tmp_dir}/signoff-input-open-objection.json"
  signoff_input_file="${tmp_dir}/signoff-input-open-objection.json"
  if (assert_signoff_input_matches_signed_consensus "${expected_consensus}") 2>/dev/null; then
    die "self-test: changed consensus unexpectedly passed"
  fi

  signoff_input_file="${tmp_dir}/signoff-input.json"
  assert_signed_consensus_manifest_matches_input
  jq '.max_age_ms = 1' \
    "${tmp_dir}/signoff-input.json" >"${tmp_dir}/signoff-input-max-age-drift.json"
  signoff_input_file="${tmp_dir}/signoff-input-max-age-drift.json"
  if (assert_signed_consensus_manifest_matches_input) 2>/dev/null; then
    die "self-test: max_age drift unexpectedly passed"
  fi

  signoff_input_file="${tmp_dir}/signoff-input.json"
  jq '.proof_bundle.commands[0].status = "failed"' \
    "${tmp_dir}/signoff-source.json" >"${tmp_dir}/signoff-source-tampered.json"
  signoff_source_manifest="${tmp_dir}/signoff-source-tampered.json"
  if (assert_signed_consensus_manifest_matches_input) 2>/dev/null; then
    die "self-test: tampered signed consensus unexpectedly passed"
  fi

  state_file="${original_state_file}"
  signoff_input_file="${original_signoff_input_file}"
  signoff_source_manifest="${original_signoff_source_manifest}"
  signature_file="${original_signature_file}"
  allowed_signers="${original_allowed_signers}"
  signoff_file="${original_signoff_file}"
  rm -rf "${tmp_dir}"
}

step "before-report guard self-test"
proof_before_report_guard_self_test

initial_signoff_tuple_hash="$(signoff_tuple_hash)"

step "pre-existing specialist signoff guard"
ensure_preexisting_specialist_signoff_guard
expected_workflow_redirect_identity="$(capture_workflow_redirect_identity)"
expected_workflow_semantic_identity="$(capture_workflow_semantic_identity)"
expected_consensus_hash="$(consensus_fingerprint "${signoff_source_manifest}")"

step "initial startup refresh and signoff verification"
refresh_startup_and_verify_signoff "${expected_workflow_redirect_identity}" "${expected_workflow_semantic_identity}" "${expected_consensus_hash}"
./scripts/proof_specialist_signoff.sh

step "startup redirect freshness guard"
./scripts/proof_startup_redirect_freshness.sh

step "agent workflow guard"
./scripts/proof_workflow_before_report.sh

step "startup gate targeted rust tests"
cargo test --quiet startup_gate
cargo test --quiet continuity_startup_summary_fallback_gate
cargo test --quiet load_startup_context_rebuilds_stale_restore_when_handoff_is_newer
cargo test --quiet inspect_startup_runtime_state_fails_closed_on_missing_execctl_active_lease_source_event_id

step "thread binding alias conflict guard"
cargo test --quiet thread_binding::tests -- --test-threads=1
cargo test --quiet record_handoff_event_rejects_conflicting_thread_identity_aliases -- --test-threads=1
cargo test --quiet record_handoff_event_rejects_conflicting_agent_scope_aliases -- --test-threads=1
cargo test --quiet preferred_thread_id_for_repo_rejects_conflicting_thread_aliases -- --test-threads=1
cargo test --quiet current_chat_tail_rejects_conflicting_thread_aliases -- --test-threads=1
cargo test --quiet latest_rollout_client_meter_observation_rejects_conflicting_thread_aliases -- --test-threads=1
./scripts/proof_thread_binding_env_aliases.sh
./scripts/proof_continuity_handoff_alias_conflict_fail_closed.sh

step "continuity ownership targeted rust tests"
cargo test --quiet record_handoff_event_rejects_foreign_thread_when_active_lease_lacks_thread_binding
cargo test --quiet record_handoff_event_rejects_foreign_thread_when_previous_restore_is_last_owner_proof
cargo test --quiet startup_refresh_does_not_rebind_without_live_thread_binding
cargo test --quiet startup_refresh_does_not_rebind_threadless_lease_to_foreign_thread
cargo test --quiet startup_refresh_reloads_live_source_event_after_scope_lock_wait
cargo test --quiet guard_maintenance_does_not_rebind_without_live_thread_binding
cargo test --quiet guard_maintenance_does_not_rebind_threadless_lease_to_foreign_thread
cargo test --quiet guard_maintenance_reloads_live_owner_after_scope_lock_wait
cargo test --quiet restore_context_thread_id_hint_accepts_fresh_lease_bound_restore
cargo test --quiet restore_context_thread_id_hint_rejects_unbound_restore_thread_id
cargo test --quiet record_handoff_event_rejects_same_thread_competing_line_without_explicit_promotion
cargo test --quiet record_handoff_event_allows_same_thread_progress_without_explicit_promotion
cargo test --quiet record_handoff_event_allows_same_thread_to_advance_active_line_with_explicit_promotion
cargo test --quiet record_handoff_event_normalizes_non_competing_requested_promotion_to_false -- --test-threads=1
cargo test --quiet promoted_handoff_without_promotion_contract_is_not_startup_eligible
cargo test --quiet latest_handoff_selection_prefers_newer_same_workline_progress_once_fake_promotion_is_normalized_away -- --test-threads=1
cargo test --quiet latest_handoff_selection_skips_non_promoted_side_agent_scope
cargo test --quiet context_pack_previous_handoff_binding_preserves_source_kind_only_handoff_lineage -- --test-threads=1
cargo test --quiet semantic_handoff_replay_authoritative_event_keeps_identity_fields -- --test-threads=1
cargo test --quiet refresh_restore_identity_value_preserves_handoff_owner_over_context_pack -- --test-threads=1

step "working-state restore and scoped observability targeted rust tests"
cargo test --quiet latest_working_state_restore_snapshot_for_project_uses_legacy_null_scope_rows -- --test-threads=1
cargo test --quiet latest_working_state_restore_snapshot_for_project_prefers_scoped_row_over_legacy_fallback -- --test-threads=1
cargo test --quiet legacy_working_state_restore_lookup_query_shape_uses_compatibility_index -- --test-threads=1
cargo test --quiet scoped_mcp_task_matrix_lookup_ignores_unscoped_rows -- --test-threads=1
bash -lc 'source ./scripts/load_env.sh && cargo test --quiet bootstrap_schema_marks_app_role_grants_current -- --test-threads=1'

step "continuity handoff alias conflict hostile proof"
./scripts/proof_continuity_handoff_alias_conflict_fail_closed.sh

step "threadless continuity hostile proof"
./scripts/proof_continuity_threadless_handoff_fail_closed.sh

step "same-thread same-scope competing handoff hostile proof"
./scripts/proof_continuity_same_thread_same_scope_competing_handoff_fail_closed.sh

step "startup runtime fail-closed guard"
./scripts/proof_startup_runtime_state_fail_closed.sh

step "observe frontdoor stale restart guard"
./scripts/proof_observe_frontdoor_stale_restart.sh

step "continuity reconnect hostile guard"
./scripts/proof_continuity_reconcile_reconnect_hostile.sh

step "MCP stale-success reconcile guard"
./scripts/proof_mcp_continuity_startup_stale_success_reconcile.sh

step "repo hygiene guard"
./scripts/proof_repo_hygiene_guard.sh

step "maintainability gate"
./scripts/proof_maintainability_gate.sh

step "implementation status sync"
./scripts/proof_implementation_status_sync_guard.sh

step "project hardening bundle"
./scripts/proof_hardening.sh

step "retrieval accuracy bundle"
./scripts/proof_accuracy.sh

step "token ledger bundle"
./scripts/proof_token_ledger.sh

step "token report overhead autosync"
./scripts/proof_token_report_tool_overhead_autosync.sh

step "lifecycle policy simulation measured validation"
./scripts/proof_lifecycle_policy_simulate_measured_validation.sh

step "client budget / observability bundle"
./scripts/proof_observability.sh

step "mcp bundle"
./scripts/proof_mcp.sh

step "workspace restore hardening"
./scripts/proof_workspace_restore_pack_hardening.sh

step "scope identity control plane"
./scripts/proof_scope_identity_control_plane.sh

step "project registration canonicalization"
./scripts/proof_project_registration_canonicalization.sh

step "project relocation contour"
./scripts/proof_project_relocation_contour.sh

step "task graph integrity"
./scripts/proof_commitment_task_graph_integrity.sh

step "graph-first startup restore projection"
./scripts/proof_graph_first_startup_restore_projection.sh

step "final startup redirect freshness guard"
./scripts/proof_startup_redirect_freshness.sh

expected_workflow_redirect_identity="$(capture_workflow_redirect_identity)"
expected_workflow_semantic_identity="$(capture_workflow_semantic_identity)"

step "final signoff verification"
refresh_startup_and_verify_signoff "${expected_workflow_redirect_identity}" "${expected_workflow_semantic_identity}" "${expected_consensus_hash}"

step "final agent workflow guard"
./scripts/proof_workflow_before_report.sh

step "specialist signoff guard"
./scripts/proof_specialist_signoff.sh

final_signoff_tuple_hash="$(signoff_tuple_hash)"
[[ "${initial_signoff_tuple_hash}" == "${final_signoff_tuple_hash}" ]] \
  || die "signoff tuple changed during verify-only before-report proof"

step "before-report deep guard passed"
