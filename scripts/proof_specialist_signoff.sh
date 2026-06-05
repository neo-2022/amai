#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_file=".amai/continuity/project-chat-startup-state.json"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
signoff_file="${1:-.amai/continuity/specialist-team-signoff.json}"
source_manifest="${2:-.amai/continuity/specialist-team-signoff-source.json}"
signature_file="${3:-.amai/continuity/specialist-team-signoff-source.json.sig}"
allowed_signers="${HOME}/.local/share/amai/signoff-trust/allowed_signers"
signature_namespace="amai-specialist-signoff"

die() {
  echo "proof_specialist_signoff: $*" >&2
  exit 1
}

sha256_file() {
  sha256sum "$1" | awk '{print $1}'
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

verify_workflow_trace() {
  local input="$1"
  cargo run --quiet -- verify workflow-trace \
    --state "${state_file}" \
    --startup-contract "${startup_contract}" \
    --input "${input}"
}

assert_existing_regular_not_symlink "${state_file}"
assert_existing_regular_not_symlink "${startup_contract}"
assert_existing_regular_not_symlink "${signoff_file}"
assert_existing_regular_not_symlink "${source_manifest}"
assert_existing_regular_not_symlink "${signature_file}"
assert_existing_regular_not_symlink "${allowed_signers}"
assert_outside_worktree "${allowed_signers}"
assert_not_group_or_world_writable "$(dirname "${allowed_signers}")"
assert_not_group_or_world_writable "${allowed_signers}"

expected_manifest_canon="$(canonical_path ".amai/continuity/specialist-team-signoff-source.json")"
expected_signoff_canon="$(canonical_path ".amai/continuity/specialist-team-signoff.json")"
expected_signature_canon="$(canonical_path ".amai/continuity/specialist-team-signoff-source.json.sig")"
manifest_canon="$(canonical_path "${source_manifest}")"
signoff_canon="$(canonical_path "${signoff_file}")"
signature_canon="$(canonical_path "${signature_file}")"
allowed_signers_canon="$(canonical_path "${allowed_signers}")"

[[ "${manifest_canon}" == "${expected_manifest_canon}" ]] || die "unexpected source manifest path"
[[ "${signoff_canon}" == "${expected_signoff_canon}" ]] || die "unexpected signoff path"
[[ "${signature_canon}" == "${expected_signature_canon}" ]] || die "unexpected signature path"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT
state_snapshot="${tmp_dir}/state.json"
startup_contract_snapshot="${tmp_dir}/startup-contract.json"
signoff_snapshot="${tmp_dir}/signoff.json"
manifest_snapshot="${tmp_dir}/source-manifest.json"
signature_snapshot="${tmp_dir}/source-manifest.json.sig"
allowed_signers_snapshot="${tmp_dir}/allowed_signers"
cp "${state_file}" "${state_snapshot}"
cp "${startup_contract}" "${startup_contract_snapshot}"
cp "${signoff_canon}" "${signoff_snapshot}"
cp "${manifest_canon}" "${manifest_snapshot}"
cp "${signature_canon}" "${signature_snapshot}"
cp "${allowed_signers_canon}" "${allowed_signers_snapshot}"

state_file="${state_snapshot}"
startup_contract="${startup_contract_snapshot}"

ssh-keygen -Y verify \
  -f "${allowed_signers_snapshot}" \
  -I "amai-specialist-signoff" \
  -n "${signature_namespace}" \
  -s "${signature_snapshot}" \
  <"${manifest_snapshot}" >/dev/null 2>&1 || die "source manifest signature verification failed"

manifest_sha="$(sha256_file "${manifest_snapshot}")"
signature_sha="$(sha256_file "${signature_snapshot}")"
allowed_signers_sha="$(sha256_file "${allowed_signers_snapshot}")"
validation_summary="$(verify_workflow_trace "${manifest_snapshot}")"
workflow_trace_hash="$(printf '%s\n' "${validation_summary}" | jq -r '.workflow_execution_trace_hash')"
guard_snapshot_hash="$(printf '%s\n' "${validation_summary}" | jq -r '.guard_snapshot_hash')"
evidence_manifest_hash="$(printf '%s\n' "${validation_summary}" | jq -r '.evidence_manifest_hash')"
consensus_fingerprint="$(printf '%s\n' "${validation_summary}" | jq -r '.consensus_fingerprint')"
now_ms="$(./scripts/epoch_ms.sh)"

validate_pair() {
  local signoff="$1"
  local manifest="$2"
  local test_manifest_sha test_workflow_trace_hash test_guard_snapshot_hash test_evidence_manifest_hash test_consensus_fingerprint
  test_manifest_sha="$(sha256_file "${manifest}")"
  local test_summary
  test_summary="$(verify_workflow_trace "${manifest}")"
  test_workflow_trace_hash="$(printf '%s\n' "${test_summary}" | jq -r '.workflow_execution_trace_hash')"
  test_guard_snapshot_hash="$(printf '%s\n' "${test_summary}" | jq -r '.guard_snapshot_hash')"
  test_evidence_manifest_hash="$(printf '%s\n' "${test_summary}" | jq -r '.evidence_manifest_hash')"
  test_consensus_fingerprint="$(printf '%s\n' "${test_summary}" | jq -r '.consensus_fingerprint')"
  jq -e \
    --arg manifest_sha "${test_manifest_sha}" \
    --arg signature_sha "${signature_sha}" \
    --arg allowed_signers_path "${allowed_signers}" \
    --arg allowed_signers_sha "${allowed_signers_sha}" \
    --arg workflow_trace_hash "${test_workflow_trace_hash}" \
    --arg guard_snapshot_hash "${test_guard_snapshot_hash}" \
    --arg evidence_manifest_hash "${test_evidence_manifest_hash}" \
    --arg consensus_fingerprint "${test_consensus_fingerprint}" \
    --argjson now_ms "${now_ms}" \
    --slurpfile m "${manifest}" '
      ($m[0]) as $manifest
      | .artifact_version == "specialist-team-signoff-v2"
      and .guard_version == "agent-workflow-guard-v2"
      and .source_manifest_path == ".amai/continuity/specialist-team-signoff-source.json"
      and .source_manifest_sha256 == $manifest_sha
      and .source_manifest_signature_path == ".amai/continuity/specialist-team-signoff-source.json.sig"
      and .source_manifest_signature_sha256 == $signature_sha
      and .trust_root_allowed_signers == $allowed_signers_path
      and .trust_root_allowed_signers_sha256 == $allowed_signers_sha
      and (.generated_at_epoch_ms | type == "number")
      and (.max_age_ms | type == "number")
      and (.generated_at_epoch_ms <= $now_ms)
      and (($now_ms - .generated_at_epoch_ms) <= .max_age_ms)
      and $manifest.artifact_version == "specialist-team-signoff-source-v2"
      and $manifest.generated_by == "scripts/materialize_specialist_signoff.sh"
      and ($manifest.startup_runtime_state_sha256 | type == "string")
      and ($manifest.startup_runtime_state_sha256 | test("^[0-9a-f]{64}$"))
      and .startup_runtime_state_sha256 == $manifest.startup_runtime_state_sha256
      and .stable_workline_identity == $manifest.stable_workline_identity
      and .workflow_promotion == $manifest.workflow_promotion
      and .redirect_identity == $manifest.redirect_identity
      and .stable_workline_identity.active_workline_headline == .final_bughunter_pass.scope
      and $manifest.stable_workline_identity.active_workline_headline == $manifest.final_bughunter_pass.scope
      and $manifest.workflow_execution_trace.workflow_scope.active_workline_headline == $manifest.final_bughunter_pass.scope
      and .plan_hash == $manifest.plan_hash
      and .proof_bundle_hash == $manifest.proof_bundle_hash
      and .required_roles == $manifest.required_roles
      and .specialists == $manifest.specialists
      and .open_objections == $manifest.open_objections
      and .open_objections_count == $manifest.open_objections_count
      and .final_bughunter_pass == $manifest.final_bughunter_pass
      and .workflow_execution_trace_hash == $workflow_trace_hash
      and $manifest.workflow_execution_trace_hash == $workflow_trace_hash
      and .guard_snapshot_hash == $guard_snapshot_hash
      and $manifest.guard_snapshot_hash == $guard_snapshot_hash
      and .evidence_manifest_hash == $evidence_manifest_hash
      and $manifest.evidence_manifest_hash == $evidence_manifest_hash
      and .consensus_fingerprint == $consensus_fingerprint
      and $manifest.consensus_fingerprint == $consensus_fingerprint
      and .trust_boundary.trust_kind == "local_trust_root_not_nonrepudiation"
      and $manifest.trust_boundary.trust_kind == "local_trust_root_not_nonrepudiation"
      and .external_reference_policy.source_allowlist_kind == "official_or_primary_sources_only"
      and .external_reference_policy.advisory_only == true
      and .external_reference_policy.local_corroboration_required_before_truth_or_runtime_change == true
      and .external_reference_policy.must_not_override_local_contracts_or_gates == true
    ' "${signoff}" >/dev/null
}

validate_pair "${signoff_snapshot}" "${manifest_snapshot}"

if validate_pair "${tmp_dir}/missing-signoff.json" "${manifest_snapshot}" 2>/dev/null; then
  die "missing final signoff unexpectedly passed"
fi

if validate_pair "${signoff_snapshot}" "${tmp_dir}/missing-source.json" 2>/dev/null; then
  die "missing source manifest unexpectedly passed"
fi

jq '.source_manifest_sha256 = "bad-sha"' \
  "${signoff_snapshot}" >"${tmp_dir}/bad-source-manifest-hash.json"
if validate_pair "${tmp_dir}/bad-source-manifest-hash.json" "${manifest_snapshot}" 2>/dev/null; then
  die "bad source manifest hash unexpectedly passed"
fi

jq 'del(.workflow_execution_trace)' "${manifest_snapshot}" >"${tmp_dir}/missing-trace.json"
if verify_workflow_trace "${tmp_dir}/missing-trace.json" >/dev/null 2>&1; then
  die "missing workflow trace unexpectedly passed"
fi

jq '.workflow_execution_trace.stage_records[2].stage = "implementation"' \
  "${manifest_snapshot}" >"${tmp_dir}/stage-order-drift.json"
if verify_workflow_trace "${tmp_dir}/stage-order-drift.json" >/dev/null 2>&1; then
  die "stage order drift unexpectedly passed"
fi

jq 'del(.plan_items[0].goal)' "${manifest_snapshot}" >"${tmp_dir}/missing-plan-goal.json"
if verify_workflow_trace "${tmp_dir}/missing-plan-goal.json" >/dev/null 2>&1; then
  die "missing plan goal unexpectedly passed"
fi

jq '.final_bughunter_pass.scope = "wrong active line"' \
  "${manifest_snapshot}" >"${tmp_dir}/final-bughunter-scope-drift.json"
if verify_workflow_trace "${tmp_dir}/final-bughunter-scope-drift.json" >/dev/null 2>&1; then
  die "final bughunter scope drift unexpectedly passed"
fi

jq '.required_roles = (.required_roles[0:5])' "${manifest_snapshot}" >"${tmp_dir}/missing-role.json"
if verify_workflow_trace "${tmp_dir}/missing-role.json" >/dev/null 2>&1; then
  die "missing role unexpectedly passed"
fi

jq '.required_roles += ["legacy_verification"]' "${manifest_snapshot}" >"${tmp_dir}/extra-role.json"
if verify_workflow_trace "${tmp_dir}/extra-role.json" >/dev/null 2>&1; then
  die "extra role unexpectedly passed"
fi

jq '.workflow_execution_trace.plan_item_reviews[0].implementation.after_team_critique = false' \
  "${manifest_snapshot}" >"${tmp_dir}/implementation-before-critique.json"
if verify_workflow_trace "${tmp_dir}/implementation-before-critique.json" >/dev/null 2>&1; then
  die "implementation before critique unexpectedly passed"
fi

jq '.workflow_execution_trace.plan_item_reviews[0].team_verification.dimensions = (.workflow_execution_trace.plan_item_reviews[0].team_verification.dimensions[0:9])' \
  "${manifest_snapshot}" >"${tmp_dir}/missing-verification-dimension.json"
if verify_workflow_trace "${tmp_dir}/missing-verification-dimension.json" >/dev/null 2>&1; then
  die "missing verification dimension unexpectedly passed"
fi

jq '.workflow_execution_trace.final_audit.open_issues = [{"id":"open"}] | .workflow_execution_trace.final_audit.report_allowed = false' \
  "${manifest_snapshot}" >"${tmp_dir}/open-final-audit-issue.json"
if verify_workflow_trace "${tmp_dir}/open-final-audit-issue.json" >/dev/null 2>&1; then
  die "open final audit issue unexpectedly passed"
fi

jq '.proof_bundle.commands[0].status = "failed"' \
  "${manifest_snapshot}" >"${tmp_dir}/failed-proof-command.json"
if verify_workflow_trace "${tmp_dir}/failed-proof-command.json" >/dev/null 2>&1; then
  die "failed proof command unexpectedly passed"
fi

jq '.proof_bundle.commands[0].evidence_sha256 = "1111111111111111111111111111111111111111111111111111111111111111"' \
  "${manifest_snapshot}" >"${tmp_dir}/placeholder-proof-evidence.json"
if verify_workflow_trace "${tmp_dir}/placeholder-proof-evidence.json" >/dev/null 2>&1; then
  die "placeholder proof evidence unexpectedly passed"
fi

jq '.proof_bundle.commands[0].evidence_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"' \
  "${manifest_snapshot}" >"${tmp_dir}/unbacked-proof-evidence.json"
if verify_workflow_trace "${tmp_dir}/unbacked-proof-evidence.json" >/dev/null 2>&1; then
  die "unbacked proof evidence unexpectedly passed"
fi

jq '.proof_bundle.commands[0].evidence_sha256 = .final_bughunter_pass.evidence_sha256
    | .evidence_manifest.items = [.evidence_manifest.items[] | select(.id != "proof:1")]' \
  "${manifest_snapshot}" >"${tmp_dir}/cross-kind-proof-evidence.json"
if verify_workflow_trace "${tmp_dir}/cross-kind-proof-evidence.json" >/dev/null 2>&1; then
  die "cross-kind proof evidence unexpectedly passed"
fi

jq '.proof_bundle.commands[0].command = "forged command"' \
  "${manifest_snapshot}" >"${tmp_dir}/proof-command-metadata-mismatch.json"
if verify_workflow_trace "${tmp_dir}/proof-command-metadata-mismatch.json" >/dev/null 2>&1; then
  die "proof command metadata mismatch unexpectedly passed"
fi

jq '.evidence_manifest.items[0].sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"' \
  "${manifest_snapshot}" >"${tmp_dir}/evidence-file-hash-mismatch.json"
if verify_workflow_trace "${tmp_dir}/evidence-file-hash-mismatch.json" >/dev/null 2>&1; then
  die "evidence file hash mismatch unexpectedly passed"
fi

jq '.evidence_manifest.items[0].path = ".amai/continuity/workflow-evidence/missing-evidence.json"' \
  "${manifest_snapshot}" >"${tmp_dir}/missing-evidence-file.json"
if verify_workflow_trace "${tmp_dir}/missing-evidence-file.json" >/dev/null 2>&1; then
  die "missing evidence file unexpectedly passed"
fi

jq '.stable_workline_identity.current_user_redirect_id = "replayed-identity"' \
  "${signoff_snapshot}" >"${tmp_dir}/replayed-signoff-identity.json"
if validate_pair "${tmp_dir}/replayed-signoff-identity.json" "${manifest_snapshot}" 2>/dev/null; then
  die "replayed signoff identity unexpectedly passed"
fi

jq '.generated_at_epoch_ms = 1' \
  "${signoff_snapshot}" >"${tmp_dir}/stale-signoff.json"
if validate_pair "${tmp_dir}/stale-signoff.json" "${manifest_snapshot}" 2>/dev/null; then
  die "stale signoff unexpectedly passed"
fi

wrong_trust_dir="${tmp_dir}/wrong-trust"
mkdir -p "${wrong_trust_dir}"
ssh-keygen -q -t ed25519 -N "" -C "wrong-amai-specialist-signoff" -f "${wrong_trust_dir}/signoff_ed25519"
printf 'amai-specialist-signoff %s\n' "$(cat "${wrong_trust_dir}/signoff_ed25519.pub")" >"${wrong_trust_dir}/allowed_signers"
if [[ "$(canonical_path "${wrong_trust_dir}/allowed_signers")" == "${allowed_signers_canon}" ]]; then
  die "wrong trust-root fixture collided with canonical trust root"
fi
if ssh-keygen -Y verify \
  -f "${wrong_trust_dir}/allowed_signers" \
  -I "amai-specialist-signoff" \
  -n "${signature_namespace}" \
  -s "${signature_snapshot}" \
  <"${manifest_snapshot}" >/dev/null 2>&1; then
  die "wrong trust-root unexpectedly verified the source manifest"
fi

echo "proof_specialist_signoff: ok"
