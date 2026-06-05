#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

input_file="${1:-.amai/continuity/specialist-team-signoff-input.json}"
state_file=".amai/continuity/project-chat-startup-state.json"
startup_contract=".amai/onboarding/project-chat-startup-contract.json"
source_manifest=".amai/continuity/specialist-team-signoff-source.json"
signoff_file=".amai/continuity/specialist-team-signoff.json"
signature_file=".amai/continuity/specialist-team-signoff-source.json.sig"
namespace="amai-specialist-signoff"
trust_dir="${HOME}/.local/share/amai/signoff-trust"
trust_key="${trust_dir}/signoff_ed25519"
allowed_signers="${trust_dir}/allowed_signers"

die() {
  echo "materialize_specialist_signoff: $*" >&2
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

assert_outside_worktree() {
  local path="$1"
  local workspace_canon path_canon
  workspace_canon="$(readlink -f .)"
  path_canon="$(readlink -f "${path}")"
  case "${path_canon}" in
    "${workspace_canon}" | "${workspace_canon}"/*)
      die "trust root must stay outside worktree: ${path_canon}"
      ;;
  esac
}

assert_not_group_or_world_writable() {
  local path="$1"
  local mode owner
  mode="$(stat -c '%a' "${path}")"
  owner="$(stat -c '%U' "${path}")"
  if [[ "${owner}" != "$(id -un)" ]]; then
    die "trust root path is not owned by current user: ${path}"
  fi
  if (( (8#${mode} & 8#0022) != 0 )); then
    die "trust root path is group/world writable: ${path}"
  fi
}

test -f "${input_file}" || die "missing signoff input: ${input_file}"
test -f "${state_file}" || die "missing startup runtime state: ${state_file}"
test -f "${startup_contract}" || die "missing startup contract: ${startup_contract}"

test -d "${trust_dir}" || die "missing pre-provisioned trust directory; run scripts/provision_specialist_signoff_trust.sh out-of-band"
assert_outside_worktree "${trust_dir}"
assert_not_group_or_world_writable "${trust_dir}"

if [[ -e "${allowed_signers}" && -L "${allowed_signers}" ]]; then
  die "allowed_signers must not be a symlink"
fi
if [[ -e "${trust_key}" && -L "${trust_key}" ]]; then
  die "signoff key must not be a symlink"
fi

test -f "${trust_key}" || die "missing pre-provisioned signing key; run scripts/provision_specialist_signoff_trust.sh out-of-band"
test -f "${allowed_signers}" || die "missing pre-provisioned allowed_signers; run scripts/provision_specialist_signoff_trust.sh out-of-band"
expected_allowed_line="amai-specialist-signoff $(ssh-keygen -y -f "${trust_key}")"
current_allowed_line="$(cat "${allowed_signers}")"
if [[ "${current_allowed_line}" != "${expected_allowed_line}" ]]; then
  die "allowed_signers does not match the existing signing key"
fi
assert_not_group_or_world_writable "${trust_key}"
assert_outside_worktree "${allowed_signers}"
assert_not_group_or_world_writable "${allowed_signers}"

validation_summary="$(
  cargo run --quiet -- verify workflow-trace \
    --state "${state_file}" \
    --startup-contract "${startup_contract}" \
    --input "${input_file}"
)"
workflow_trace_hash="$(printf '%s\n' "${validation_summary}" | jq -r '.workflow_execution_trace_hash')"
guard_snapshot_hash="$(printf '%s\n' "${validation_summary}" | jq -r '.guard_snapshot_hash')"
evidence_manifest_hash="$(printf '%s\n' "${validation_summary}" | jq -r '.evidence_manifest_hash')"
consensus_fingerprint="$(printf '%s\n' "${validation_summary}" | jq -r '.consensus_fingerprint')"
test -n "${workflow_trace_hash}" || die "missing workflow trace hash from verifier"
test -n "${guard_snapshot_hash}" || die "missing guard snapshot hash from verifier"
test -n "${evidence_manifest_hash}" || die "missing evidence manifest hash from verifier"
test -n "${consensus_fingerprint}" || die "missing consensus fingerprint from verifier"

generated_at_epoch_ms="$(./scripts/epoch_ms.sh)"
max_age_ms="$(jq -r '.max_age_ms // 1800000' "${input_file}")"
state_sha="$(sha256_file "${state_file}")"
startup_contract_sha="$(jq -r '.startup_contract_sha256 // empty' "${startup_contract}")"
plan_hash="$(sha256_json_filter '.plan_items' "${input_file}")"
proof_bundle_hash="$(sha256_json_filter '.proof_bundle' "${input_file}")"
allowed_signers_sha="$(sha256_file "${allowed_signers}")"

current_redirect_id="$(jq -r '.workflow_promotion_state.current_user_redirect_id // empty' "${state_file}")"
promoted_redirect_id="$(jq -r '.workflow_promotion_state.promoted_user_redirect_id // empty' "${state_file}")"
source_event_match="$(jq -r '.workflow_promotion_state.source_event_match // false' "${state_file}")"
lineage_redirect_id="$(jq -r '.working_state_restore_lineage.authoritative_event_id // empty' "${state_file}")"
active_lease_source_event_id="$(jq -r '.continuity_startup_summary.execctl_active_lease.source_event_id // empty' "${state_file}")"
active_workline_headline="$(jq -r '.workflow_promotion_state.active_workline_headline // empty' "${state_file}")"
workflow_lease_headline="$(jq -r '.workflow_promotion_state.active_lease_headline // empty' "${state_file}")"
summary_headline="$(jq -r '.continuity_startup_summary.headline // empty' "${state_file}")"
summary_lease_headline="$(jq -r '.continuity_startup_summary.execctl_active_lease.headline // empty' "${state_file}")"
top_level_lease_headline="$(jq -r '.execctl_active_lease.headline // empty' "${state_file}")"
lineage_headline="$(jq -r '.working_state_restore_lineage.authoritative_headline // empty' "${state_file}")"
active_workline_source_kind="$(jq -r '.workflow_promotion_state.active_workline_source_kind // empty' "${state_file}")"
active_lease_source_kind="$(jq -r '.workflow_promotion_state.active_lease_source_kind // empty' "${state_file}")"
summary_lease_source_kind="$(jq -r '.continuity_startup_summary.execctl_active_lease.source_kind // empty' "${state_file}")"
top_level_lease_source_kind="$(jq -r '.execctl_active_lease.source_kind // empty' "${state_file}")"
lineage_source_kind="$(jq -r '.working_state_restore_lineage.authoritative_source_kind // empty' "${state_file}")"
headline_match="$(jq -r '.workflow_promotion_state.headline_match // false' "${state_file}")"
source_kind_match="$(jq -r '.workflow_promotion_state.source_kind_match // false' "${state_file}")"

test -n "${current_redirect_id}" || die "missing current_user_redirect_id"
test -n "${promoted_redirect_id}" || die "missing promoted_user_redirect_id"
test -n "${lineage_redirect_id}" || die "missing working_state_restore_lineage.authoritative_event_id"
test -n "${active_lease_source_event_id}" || die "missing continuity_startup_summary.execctl_active_lease.source_event_id"
test -n "${active_workline_headline}" || die "missing active workline headline"
test -n "${workflow_lease_headline}" || die "missing workflow active lease headline"
test -n "${summary_headline}" || die "missing summary headline"
test -n "${summary_lease_headline}" || die "missing summary active lease headline"
test -n "${top_level_lease_headline}" || die "missing top-level active lease headline"
test -n "${lineage_headline}" || die "missing lineage authoritative headline"
test -n "${active_workline_source_kind}" || die "missing active workline source_kind"
test -n "${active_lease_source_kind}" || die "missing active lease source_kind"
test -n "${summary_lease_source_kind}" || die "missing summary active lease source_kind"
test -n "${top_level_lease_source_kind}" || die "missing top-level active lease source_kind"
test -n "${lineage_source_kind}" || die "missing lineage authoritative source_kind"
[[ "${source_event_match}" == "true" ]] || die "workflow promotion source event does not match"
[[ "${headline_match}" == "true" ]] || die "workflow promotion headline does not match"
[[ "${source_kind_match}" == "true" ]] || die "workflow promotion source_kind does not match"
[[ "${active_workline_headline}" == "${workflow_lease_headline}" ]] || die "workflow active headline mismatch"
[[ "${active_workline_headline}" == "${summary_headline}" ]] || die "summary headline mismatch"
[[ "${active_workline_headline}" == "${summary_lease_headline}" ]] || die "summary active lease headline mismatch"
[[ "${active_workline_headline}" == "${top_level_lease_headline}" ]] || die "top-level active lease headline mismatch"
[[ "${active_workline_headline}" == "${lineage_headline}" ]] || die "lineage authoritative headline mismatch"
[[ "${active_workline_source_kind}" == "${active_lease_source_kind}" ]] || die "workflow source_kind mismatch"
[[ "${active_workline_source_kind}" == "${summary_lease_source_kind}" ]] || die "summary active lease source_kind mismatch"
[[ "${active_workline_source_kind}" == "${top_level_lease_source_kind}" ]] || die "top-level active lease source_kind mismatch"
[[ "${active_workline_source_kind}" == "${lineage_source_kind}" ]] || die "lineage authoritative source_kind mismatch"
[[ "${current_redirect_id}" == "${promoted_redirect_id}" ]] || die "current/promoted redirect mismatch"
[[ "${current_redirect_id}" == "${lineage_redirect_id}" ]] || die "current redirect does not match working-state lineage"
[[ "${current_redirect_id}" == "${active_lease_source_event_id}" ]] || die "current redirect does not match active lease source event"

tmp_dir="$(mktemp -d "$(dirname "${source_manifest}")/.signoff-tmp.XXXXXX")"
backup_dir="$(mktemp -d "$(dirname "${source_manifest}")/.signoff-backup.XXXXXX")"
commit_done=0
cleanup() {
  local status=$?
  if [[ "${commit_done}" != "1" ]]; then
    if [[ -f "${backup_dir}/source.json" ]]; then cp "${backup_dir}/source.json" "${source_manifest}"; fi
    if [[ -f "${backup_dir}/source.json.sig" ]]; then cp "${backup_dir}/source.json.sig" "${signature_file}"; fi
    if [[ -f "${backup_dir}/signoff.json" ]]; then cp "${backup_dir}/signoff.json" "${signoff_file}"; fi
  fi
  rm -rf "${tmp_dir}" "${backup_dir}"
  exit "${status}"
}
trap cleanup EXIT

[[ ! -f "${source_manifest}" ]] || cp "${source_manifest}" "${backup_dir}/source.json"
[[ ! -f "${signature_file}" ]] || cp "${signature_file}" "${backup_dir}/source.json.sig"
[[ ! -f "${signoff_file}" ]] || cp "${signoff_file}" "${backup_dir}/signoff.json"

tmp_manifest="${tmp_dir}/specialist-team-signoff-source.json"
tmp_signoff="${tmp_dir}/specialist-team-signoff.json"

jq -nS \
  --slurpfile input "${input_file}" \
  --slurpfile state "${state_file}" \
  --argjson generated_at_epoch_ms "${generated_at_epoch_ms}" \
  --argjson max_age_ms "${max_age_ms}" \
  --arg state_sha "${state_sha}" \
  --arg startup_contract_sha "${startup_contract_sha}" \
  --arg plan_hash "${plan_hash}" \
  --arg proof_bundle_hash "${proof_bundle_hash}" \
  --arg workflow_trace_hash "${workflow_trace_hash}" \
  --arg guard_snapshot_hash "${guard_snapshot_hash}" \
  --arg evidence_manifest_hash "${evidence_manifest_hash}" \
  --arg consensus_fingerprint "${consensus_fingerprint}" \
  --arg current_redirect_id "${current_redirect_id}" \
  --arg promoted_redirect_id "${promoted_redirect_id}" \
  --arg lineage_redirect_id "${lineage_redirect_id}" \
  --arg active_lease_source_event_id "${active_lease_source_event_id}" \
  --arg active_workline_headline "${active_workline_headline}" \
  --arg active_workline_source_kind "${active_workline_source_kind}" \
  --arg active_lease_source_kind "${active_lease_source_kind}" \
  --arg allowed_signers_path "${allowed_signers}" \
  --arg allowed_signers_sha "${allowed_signers_sha}" '
  ($input[0]) as $i
  | ($state[0]) as $s
  | {
      artifact_version: "specialist-team-signoff-source-v2",
      generated_by: "scripts/materialize_specialist_signoff.sh",
      generated_at_epoch_ms: $generated_at_epoch_ms,
      max_age_ms: $max_age_ms,
      startup_runtime_state_sha256: $state_sha,
      startup_contract_sha256: $startup_contract_sha,
      stable_workline_identity: {
        current_user_redirect_id: $current_redirect_id,
        promoted_user_redirect_id: $promoted_redirect_id,
        working_state_lineage_authoritative_event_id: $lineage_redirect_id,
        active_lease_source_event_id: $active_lease_source_event_id,
        active_workline_headline: $active_workline_headline,
        active_workline_source_kind: $active_workline_source_kind,
        active_lease_source_kind: $active_lease_source_kind,
        startup_contract_sha256: $startup_contract_sha
      },
      workflow_promotion: {
        current_user_redirect_id: $current_redirect_id,
        promoted_user_redirect_id: $promoted_redirect_id,
        source_event_match: true
      },
      redirect_identity: {
        current_user_redirect_id: $current_redirect_id,
        promoted_user_redirect_id: $promoted_redirect_id,
        working_state_lineage_authoritative_event_id: $lineage_redirect_id,
        active_lease_source_event_id: $active_lease_source_event_id
      },
      plan_items: $i.plan_items,
      plan_hash: $plan_hash,
      proof_bundle: $i.proof_bundle,
      proof_bundle_hash: $proof_bundle_hash,
      evidence_manifest: $i.evidence_manifest,
      evidence_manifest_hash: $evidence_manifest_hash,
      required_roles: $i.required_roles,
      specialists: $i.specialists,
      open_objections: ($i.open_objections // []),
      open_objections_count: ($i.open_objections_count // 0),
      final_bughunter_pass: $i.final_bughunter_pass,
      workflow_execution_trace: $i.workflow_execution_trace,
      workflow_execution_trace_hash: $workflow_trace_hash,
      guard_snapshot_hash: $guard_snapshot_hash,
      consensus_fingerprint: $consensus_fingerprint,
      workflow_trace_validation_summary: {
        validator: "amai verify workflow-trace",
        status: "ok",
        workflow_execution_trace_hash: $workflow_trace_hash,
        guard_snapshot_hash: $guard_snapshot_hash,
        evidence_manifest_hash: $evidence_manifest_hash,
        consensus_fingerprint: $consensus_fingerprint
      },
      trust_boundary: {
        trust_kind: "local_trust_root_not_nonrepudiation",
        note: "The signature proves this local trust root signed the validated v2 trace; it is not proof against a compromised local signer."
      },
      external_reference_policy: $s.agent_workflow_guard.external_reference_policy,
      trust_root: {
        allowed_signers_path: $allowed_signers_path,
        allowed_signers_sha256: $allowed_signers_sha,
        signature_namespace: "amai-specialist-signoff"
      }
    }
  ' >"${tmp_manifest}"

ssh-keygen -Y sign -q -f "${trust_key}" -n "${namespace}" "${tmp_manifest}" >/dev/null
ssh-keygen -Y verify \
  -f "${allowed_signers}" \
  -I "amai-specialist-signoff" \
  -n "${namespace}" \
  -s "${tmp_manifest}.sig" \
  <"${tmp_manifest}" >/dev/null 2>&1 || die "new source manifest signature verification failed"

source_manifest_sha="$(sha256_file "${tmp_manifest}")"
signature_sha="$(sha256_file "${tmp_manifest}.sig")"

jq -nS \
  --slurpfile manifest "${tmp_manifest}" \
  --arg source_manifest_path "${source_manifest}" \
  --arg source_manifest_sha "${source_manifest_sha}" \
  --arg signature_path "${signature_file}" \
  --arg signature_sha "${signature_sha}" \
  --arg allowed_signers_path "${allowed_signers}" \
  --arg allowed_signers_sha "${allowed_signers_sha}" '
  ($manifest[0]) as $m
  | {
      artifact_version: "specialist-team-signoff-v2",
      guard_version: "agent-workflow-guard-v2",
      source_manifest_path: $source_manifest_path,
      source_manifest_sha256: $source_manifest_sha,
      source_manifest_signature_path: $signature_path,
      source_manifest_signature_sha256: $signature_sha,
      trust_root_allowed_signers: $allowed_signers_path,
      trust_root_allowed_signers_sha256: $allowed_signers_sha,
      generated_at_epoch_ms: $m.generated_at_epoch_ms,
      max_age_ms: $m.max_age_ms,
      startup_runtime_state_sha256: $m.startup_runtime_state_sha256,
      startup_contract_sha256: $m.startup_contract_sha256,
      stable_workline_identity: $m.stable_workline_identity,
      workflow_promotion: $m.workflow_promotion,
      redirect_identity: $m.redirect_identity,
      plan_hash: $m.plan_hash,
      proof_bundle_hash: $m.proof_bundle_hash,
      evidence_manifest_hash: $m.evidence_manifest_hash,
      required_roles: $m.required_roles,
      specialists: $m.specialists,
      open_objections: $m.open_objections,
      open_objections_count: $m.open_objections_count,
      final_bughunter_pass: $m.final_bughunter_pass,
      workflow_execution_trace_hash: $m.workflow_execution_trace_hash,
      guard_snapshot_hash: $m.guard_snapshot_hash,
      consensus_fingerprint: $m.consensus_fingerprint,
      trust_boundary: $m.trust_boundary,
      external_reference_policy: $m.external_reference_policy
    }
  ' >"${tmp_signoff}"

mv "${tmp_manifest}" "${source_manifest}"
mv "${tmp_manifest}.sig" "${signature_file}"
mv "${tmp_signoff}" "${signoff_file}"
commit_done=1

echo "materialize_specialist_signoff: ok"
