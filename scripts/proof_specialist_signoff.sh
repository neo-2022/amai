#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

state_file=".amai/continuity/project-chat-startup-state.json"
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

sha256_json_filter() {
  local filter="$1"
  local file="$2"
  jq -cS "${filter}" "${file}" | sha256sum | awk '{print $1}'
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

assert_existing_regular_not_symlink "${state_file}"
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
signoff_snapshot="${tmp_dir}/signoff.json"
manifest_snapshot="${tmp_dir}/source-manifest.json"
signature_snapshot="${tmp_dir}/source-manifest.json.sig"
allowed_signers_snapshot="${tmp_dir}/allowed_signers"
cp "${state_file}" "${state_snapshot}"
cp "${signoff_canon}" "${signoff_snapshot}"
cp "${manifest_canon}" "${manifest_snapshot}"
cp "${signature_canon}" "${signature_snapshot}"
cp "${allowed_signers_canon}" "${allowed_signers_snapshot}"

# Verify external manifest bytes before trusting any manifest fields.
ssh-keygen -Y verify \
  -f "${allowed_signers_snapshot}" \
  -I "amai-specialist-signoff" \
  -n "${signature_namespace}" \
  -s "${signature_snapshot}" \
  <"${manifest_snapshot}" >/dev/null 2>&1 || die "source manifest signature verification failed"

current_redirect_id="$(jq -r '.workflow_promotion_state.current_user_redirect_id // empty' "${state_snapshot}")"
promoted_redirect_id="$(jq -r '.workflow_promotion_state.promoted_user_redirect_id // empty' "${state_snapshot}")"
workflow_promotion_event_id="$(jq -r '.workflow_promotion_state.workflow_promotion_event_id // empty' "${state_snapshot}")"
lineage_redirect_id="$(jq -r '.working_state_restore_lineage.authoritative_event_id // empty' "${state_snapshot}")"
active_lease_source_event_id="$(jq -r '.continuity_startup_summary.execctl_active_lease.source_event_id // empty' "${state_snapshot}")"
state_sha="$(sha256_file "${state_snapshot}")"
manifest_sha="$(sha256_file "${manifest_snapshot}")"
signature_sha="$(sha256_file "${signature_snapshot}")"
allowed_signers_sha="$(sha256_file "${allowed_signers_snapshot}")"
plan_hash="$(sha256_json_filter '.plan_items' "${manifest_snapshot}")"
proof_bundle_hash="$(sha256_json_filter '.proof_bundle' "${manifest_snapshot}")"
now_ms="$(./scripts/epoch_ms.sh)"

test -n "${current_redirect_id}" || die "missing current redirect id"
test -n "${promoted_redirect_id}" || die "missing promoted redirect id"
test -n "${workflow_promotion_event_id}" || die "missing workflow promotion event id"
test -n "${lineage_redirect_id}" || die "missing working-state lineage event id"
test -n "${active_lease_source_event_id}" || die "missing active lease source event id"
[[ "${current_redirect_id}" == "${promoted_redirect_id}" ]] || die "current/promoted redirect mismatch"
[[ "${current_redirect_id}" == "${lineage_redirect_id}" ]] || die "current redirect does not match working-state lineage"
[[ "${current_redirect_id}" == "${active_lease_source_event_id}" ]] || die "current redirect does not match active lease source event"

validate_pair() {
  local signoff="$1"
  local manifest="$2"
  jq -e \
    --arg current_redirect_id "${current_redirect_id}" \
    --arg promoted_redirect_id "${promoted_redirect_id}" \
    --arg workflow_promotion_event_id "${workflow_promotion_event_id}" \
    --arg lineage_redirect_id "${lineage_redirect_id}" \
    --arg active_lease_source_event_id "${active_lease_source_event_id}" \
    --arg state_sha "${state_sha}" \
    --arg manifest_sha "${manifest_sha}" \
    --arg signature_sha "${signature_sha}" \
    --arg allowed_signers_path "${allowed_signers}" \
    --arg allowed_signers_sha "${allowed_signers_sha}" \
    --arg plan_hash "${plan_hash}" \
    --arg proof_bundle_hash "${proof_bundle_hash}" \
    --argjson now_ms "${now_ms}" \
    --slurpfile m "${manifest}" '
      ($m[0]) as $manifest
      | .artifact_version == "specialist-team-signoff-v1"
      and .guard_version == "agent-workflow-guard-v1"
      and .source_manifest_path == ".amai/continuity/specialist-team-signoff-source.json"
      and .source_manifest_sha256 == $manifest_sha
      and .source_manifest_signature_path == ".amai/continuity/specialist-team-signoff-source.json.sig"
      and .source_manifest_signature_sha256 == $signature_sha
      and .trust_root_allowed_signers == $allowed_signers_path
      and .trust_root_allowed_signers_sha256 == $allowed_signers_sha
      and .startup_runtime_state_sha256 == $state_sha
      and .workflow_promotion.current_user_redirect_id == $current_redirect_id
      and .workflow_promotion.promoted_user_redirect_id == $promoted_redirect_id
      and .workflow_promotion.workflow_promotion_event_id == $workflow_promotion_event_id
      and .workflow_promotion.source_event_match == true
      and .redirect_identity.current_user_redirect_id == $current_redirect_id
      and .redirect_identity.promoted_user_redirect_id == $promoted_redirect_id
      and .redirect_identity.working_state_lineage_authoritative_event_id == $lineage_redirect_id
      and .redirect_identity.active_lease_source_event_id == $active_lease_source_event_id
      and .plan_hash == $plan_hash
      and .proof_bundle_hash == $proof_bundle_hash
      and (.generated_at_epoch_ms | type == "number")
      and (.max_age_ms | type == "number")
      and (.generated_at_epoch_ms <= $now_ms)
      and (($now_ms - .generated_at_epoch_ms) <= .max_age_ms)
      and $manifest.artifact_version == "specialist-team-signoff-source-v1"
      and $manifest.generated_by == "scripts/materialize_specialist_signoff.sh"
      and $manifest.startup_runtime_state_sha256 == $state_sha
      and $manifest.workflow_promotion.current_user_redirect_id == $current_redirect_id
      and $manifest.workflow_promotion.promoted_user_redirect_id == $promoted_redirect_id
      and $manifest.workflow_promotion.workflow_promotion_event_id == $workflow_promotion_event_id
      and $manifest.workflow_promotion.source_event_match == true
      and $manifest.redirect_identity.current_user_redirect_id == $current_redirect_id
      and $manifest.redirect_identity.promoted_user_redirect_id == $promoted_redirect_id
      and $manifest.redirect_identity.working_state_lineage_authoritative_event_id == $lineage_redirect_id
      and $manifest.redirect_identity.active_lease_source_event_id == $active_lease_source_event_id
      and $manifest.plan_hash == $plan_hash
      and $manifest.proof_bundle_hash == $proof_bundle_hash
      and ($manifest.plan_items | type == "array" and length >= 1)
      and ($manifest.proof_bundle.commands | type == "array" and length >= 1)
      and all($manifest.proof_bundle.commands[]; .status == "passed" and (.command | type == "string" and length > 0))
      and (($manifest.required_roles | sort) == ["architecture","security_workflow","verification"])
      and ((.required_roles | sort) == ["architecture","security_workflow","verification"])
      and ($manifest.specialists | type == "array" and length >= 3)
      and (($manifest.specialists | map(.role_id) | sort) == ["architecture","security_workflow","verification"])
      and ((.specialists | map(.role_id) | sort) == ["architecture","security_workflow","verification"])
      and all($manifest.specialists[]; .decision == "CONSENSUS_GREEN" and (.agent_id | type == "string" and length > 0))
      and all(.specialists[]; .decision == "CONSENSUS_GREEN" and (.agent_id | type == "string" and length > 0))
      and $manifest.open_objections_count == 0
      and .open_objections_count == 0
      and $manifest.final_bughunter_pass.status == "CONSENSUS_GREEN"
      and $manifest.final_bughunter_pass.report_allowed == true
      and .final_bughunter_pass.status == "CONSENSUS_GREEN"
      and .final_bughunter_pass.report_allowed == true
      and $manifest.external_reference_policy.source_allowlist_kind == "official_or_primary_sources_only"
      and $manifest.external_reference_policy.advisory_only == true
      and $manifest.external_reference_policy.local_corroboration_required_before_truth_or_runtime_change == true
      and $manifest.external_reference_policy.must_not_override_local_contracts_or_gates == true
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

jq '.workflow_promotion.workflow_promotion_event_id = "replayed-event"' \
  "${signoff_snapshot}" >"${tmp_dir}/replayed-promotion-event.json"
if validate_pair "${tmp_dir}/replayed-promotion-event.json" "${manifest_snapshot}" 2>/dev/null; then
  die "replayed promotion event unexpectedly passed"
fi

jq '.redirect_identity.active_lease_source_event_id = "replayed-lease"' \
  "${signoff_snapshot}" >"${tmp_dir}/replayed-redirect-identity.json"
if validate_pair "${tmp_dir}/replayed-redirect-identity.json" "${manifest_snapshot}" 2>/dev/null; then
  die "replayed redirect identity unexpectedly passed"
fi

jq '.specialists[0].decision = "BLOCKED" | .open_objections_count = 1' \
  "${signoff_snapshot}" >"${tmp_dir}/open-objection.json"
if validate_pair "${tmp_dir}/open-objection.json" "${manifest_snapshot}" 2>/dev/null; then
  die "open objection unexpectedly passed"
fi

jq '.generated_at_epoch_ms = 1' \
  "${signoff_snapshot}" >"${tmp_dir}/stale-signoff.json"
if validate_pair "${tmp_dir}/stale-signoff.json" "${manifest_snapshot}" 2>/dev/null; then
  die "stale signoff unexpectedly passed"
fi

cp "${manifest_snapshot}" "${tmp_dir}/tampered-manifest.json"
jq '.proof_bundle.commands[0].status = "failed"' \
  "${tmp_dir}/tampered-manifest.json" >"${tmp_dir}/tampered-manifest-2.json"
if ssh-keygen -Y verify \
  -f "${allowed_signers_snapshot}" \
  -I "amai-specialist-signoff" \
  -n "${signature_namespace}" \
  -s "${signature_snapshot}" \
  <"${tmp_dir}/tampered-manifest-2.json" >/dev/null 2>&1; then
  die "tampered manifest unexpectedly passed signature verification"
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

test_manifest="${tmp_dir}/failed-proof-valid-signature.json"
jq '.proof_bundle.commands[0].status = "failed"' "${manifest_snapshot}" >"${test_manifest}"
test_manifest_sha="$(sha256_file "${test_manifest}")"
jq --arg test_manifest_sha "${test_manifest_sha}" '.source_manifest_sha256 = $test_manifest_sha' \
  "${signoff_snapshot}" >"${tmp_dir}/failed-proof-signoff.json"
old_manifest_sha="${manifest_sha}"
manifest_sha="${test_manifest_sha}"
proof_bundle_hash="$(sha256_json_filter '.proof_bundle' "${test_manifest}")"
if validate_pair "${tmp_dir}/failed-proof-signoff.json" "${test_manifest}" 2>/dev/null; then
  die "failed proof in signed manifest unexpectedly passed"
fi
manifest_sha="${old_manifest_sha}"

echo "proof_specialist_signoff: ok"
