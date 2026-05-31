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

consensus_fingerprint() {
  local file="$1"
  jq -cS '
    {
      max_age_ms: (.max_age_ms // 1800000),
      plan_items,
      proof_bundle,
      required_roles: (.required_roles // ["architecture", "verification", "security_workflow"]),
      specialists,
      open_objections_count: (.open_objections_count // 0),
      final_bughunter_pass
    }
  ' "${file}" | sha256sum | awk '{print $1}'
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
  if ./scripts/proof_specialist_signoff.sh >/dev/null; then
    return 0
  fi
  step "pre-existing specialist signoff stale; verifying signed consensus before authorized rebind"
  local expected_identity
  expected_identity="$(capture_workflow_redirect_identity)"
  assert_signed_consensus_manifest_matches_input
  ./scripts/materialize_specialist_signoff.sh "${signoff_input_file}" >/dev/null
  assert_same_workflow_redirect_identity "${expected_identity}" "authorized stale-signoff rebind"
  ./scripts/proof_specialist_signoff.sh >/dev/null
}

refresh_startup_and_rebind_signoff() {
  local expected_identity="$1"
  local expected_consensus_hash="$2"
  mkdir -p "${signoff_lock_dir}"
  (
    flock --exclusive 9
    refresh_startup_and_rebind_signoff_locked "${expected_identity}" "${expected_consensus_hash}"
  ) 9>"${signoff_lock_file}"
}

refresh_startup_and_rebind_signoff_locked() {
  local expected_identity="$1"
  local expected_consensus_hash="$2"
  ./scripts/continuity_startup.sh --repo-root "$(pwd)" --namespace continuity --json >/dev/null
  assert_same_workflow_redirect_identity "${expected_identity}" "startup refresh"
  assert_signed_manifest_redirect_identity_matches_expected "${expected_identity}"
  assert_signoff_input_matches_signed_consensus "${expected_consensus_hash}"
  ./scripts/materialize_specialist_signoff.sh "${signoff_input_file}" >/dev/null
  assert_same_workflow_redirect_identity "${expected_identity}" "signoff rebind"
  assert_signoff_input_matches_signed_consensus "${expected_consensus_hash}"
  ./scripts/proof_specialist_signoff.sh >/dev/null
  assert_same_workflow_redirect_identity "${expected_identity}" "post-signoff verification"
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

  local expected_identity expected_consensus aligned_state_file
  expected_identity="$(capture_workflow_redirect_identity)"
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

step "pre-existing specialist signoff guard"
ensure_preexisting_specialist_signoff_guard
expected_workflow_redirect_identity="$(capture_workflow_redirect_identity)"
expected_consensus_hash="$(consensus_fingerprint "${signoff_source_manifest}")"

step "initial startup refresh and authorized signoff rebind"
refresh_startup_and_rebind_signoff "${expected_workflow_redirect_identity}" "${expected_consensus_hash}"
./scripts/proof_specialist_signoff.sh

step "startup redirect freshness guard"
./scripts/proof_startup_redirect_freshness.sh

step "agent workflow guard"
./scripts/proof_workflow_before_report.sh

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

step "just-in-time startup refresh and authorized signoff rebind"
refresh_startup_and_rebind_signoff "${expected_workflow_redirect_identity}" "${expected_consensus_hash}"

step "final startup redirect freshness guard"
./scripts/proof_startup_redirect_freshness.sh

step "final agent workflow guard"
./scripts/proof_workflow_before_report.sh

step "specialist signoff guard"
./scripts/proof_specialist_signoff.sh

step "before-report deep guard passed"
