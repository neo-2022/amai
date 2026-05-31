#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

input_file="${1:-.amai/continuity/specialist-team-signoff-input.json}"
state_file=".amai/continuity/project-chat-startup-state.json"
source_manifest=".amai/continuity/specialist-team-signoff-source.json"
signoff_file=".amai/continuity/specialist-team-signoff.json"
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

generated_at_epoch_ms="$(./scripts/epoch_ms.sh)"
max_age_ms="$(jq -r '.max_age_ms // 1800000' "${input_file}")"
state_sha="$(sha256_file "${state_file}")"
plan_hash="$(sha256_json_filter '.plan_items' "${input_file}")"
proof_bundle_hash="$(sha256_json_filter '.proof_bundle' "${input_file}")"
allowed_signers_sha="$(sha256_file "${allowed_signers}")"

current_redirect_id="$(jq -r '.workflow_promotion_state.current_user_redirect_id // empty' "${state_file}")"
promoted_redirect_id="$(jq -r '.workflow_promotion_state.promoted_user_redirect_id // empty' "${state_file}")"
workflow_promotion_event_id="$(jq -r '.workflow_promotion_state.workflow_promotion_event_id // empty' "${state_file}")"
source_event_match="$(jq -r '.workflow_promotion_state.source_event_match // false' "${state_file}")"
lineage_redirect_id="$(jq -r '.working_state_restore_lineage.authoritative_event_id // empty' "${state_file}")"
active_lease_source_event_id="$(jq -r '.continuity_startup_summary.execctl_active_lease.source_event_id // empty' "${state_file}")"

test -n "${current_redirect_id}" || die "missing current_user_redirect_id"
test -n "${promoted_redirect_id}" || die "missing promoted_user_redirect_id"
test -n "${workflow_promotion_event_id}" || die "missing workflow_promotion_event_id"
test -n "${lineage_redirect_id}" || die "missing working_state_restore_lineage.authoritative_event_id"
test -n "${active_lease_source_event_id}" || die "missing continuity_startup_summary.execctl_active_lease.source_event_id"
[[ "${source_event_match}" == "true" ]] || die "workflow promotion source event does not match"
[[ "${current_redirect_id}" == "${promoted_redirect_id}" ]] || die "current/promoted redirect mismatch"
[[ "${current_redirect_id}" == "${lineage_redirect_id}" ]] || die "current redirect does not match working-state lineage"
[[ "${current_redirect_id}" == "${active_lease_source_event_id}" ]] || die "current redirect does not match active lease source event"

tmp_manifest="$(mktemp)"
trap 'rm -f "${tmp_manifest}"' EXIT

jq -nS \
  --slurpfile input "${input_file}" \
  --slurpfile state "${state_file}" \
  --argjson generated_at_epoch_ms "${generated_at_epoch_ms}" \
  --argjson max_age_ms "${max_age_ms}" \
  --arg state_sha "${state_sha}" \
  --arg plan_hash "${plan_hash}" \
  --arg proof_bundle_hash "${proof_bundle_hash}" \
  --arg current_redirect_id "${current_redirect_id}" \
  --arg promoted_redirect_id "${promoted_redirect_id}" \
  --arg lineage_redirect_id "${lineage_redirect_id}" \
  --arg active_lease_source_event_id "${active_lease_source_event_id}" \
  --arg workflow_promotion_event_id "${workflow_promotion_event_id}" \
  --arg allowed_signers_path "${allowed_signers}" \
  --arg allowed_signers_sha "${allowed_signers_sha}" '
  ($input[0]) as $i
  | ($state[0]) as $s
  | {
      artifact_version: "specialist-team-signoff-source-v1",
      generated_by: "scripts/materialize_specialist_signoff.sh",
      generated_at_epoch_ms: $generated_at_epoch_ms,
      max_age_ms: $max_age_ms,
      startup_runtime_state_sha256: $state_sha,
      workflow_promotion: {
        current_user_redirect_id: $current_redirect_id,
        promoted_user_redirect_id: $promoted_redirect_id,
        workflow_promotion_event_id: $workflow_promotion_event_id,
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
      required_roles: ($i.required_roles // ["architecture", "verification", "security_workflow"]),
      specialists: $i.specialists,
      open_objections_count: ($i.open_objections_count // 0),
      final_bughunter_pass: $i.final_bughunter_pass,
      external_reference_policy: $s.agent_workflow_guard.external_reference_policy,
      trust_root: {
        allowed_signers_path: $allowed_signers_path,
        allowed_signers_sha256: $allowed_signers_sha,
        signature_namespace: "amai-specialist-signoff"
      }
    }
  ' >"${tmp_manifest}"

mkdir -p "$(dirname "${source_manifest}")"
mv "${tmp_manifest}" "${source_manifest}"
rm -f "${source_manifest}.sig"
ssh-keygen -Y sign -q -f "${trust_key}" -n "${namespace}" "${source_manifest}" >/dev/null

source_manifest_sha="$(sha256_file "${source_manifest}")"
signature_file="${source_manifest}.sig"
signature_sha="$(sha256_file "${signature_file}")"

jq -nS \
  --slurpfile manifest "${source_manifest}" \
  --arg source_manifest_path "${source_manifest}" \
  --arg source_manifest_sha "${source_manifest_sha}" \
  --arg signature_path "${signature_file}" \
  --arg signature_sha "${signature_sha}" \
  --arg allowed_signers_path "${allowed_signers}" \
  --arg allowed_signers_sha "${allowed_signers_sha}" '
  ($manifest[0]) as $m
  | {
      artifact_version: "specialist-team-signoff-v1",
      guard_version: "agent-workflow-guard-v1",
      source_manifest_path: $source_manifest_path,
      source_manifest_sha256: $source_manifest_sha,
      source_manifest_signature_path: $signature_path,
      source_manifest_signature_sha256: $signature_sha,
      trust_root_allowed_signers: $allowed_signers_path,
      trust_root_allowed_signers_sha256: $allowed_signers_sha,
      generated_at_epoch_ms: $m.generated_at_epoch_ms,
      max_age_ms: $m.max_age_ms,
      startup_runtime_state_sha256: $m.startup_runtime_state_sha256,
      workflow_promotion: $m.workflow_promotion,
      redirect_identity: $m.redirect_identity,
      plan_hash: $m.plan_hash,
      proof_bundle_hash: $m.proof_bundle_hash,
      required_roles: $m.required_roles,
      specialists: $m.specialists,
      open_objections_count: $m.open_objections_count,
      final_bughunter_pass: $m.final_bughunter_pass,
      external_reference_policy: $m.external_reference_policy
    }
  ' >"${signoff_file}"

echo "materialize_specialist_signoff: ok"
