#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${REPO_ROOT}/scripts/load_env.sh"

cd "${REPO_ROOT}"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Awarnings"
export CARGO_TERM_COLOR=never

dsn="${AMI_POSTGRES_DSN:-$(grep '^AMI_POSTGRES_DSN=' "${REPO_ROOT}/.env" | cut -d= -f2-)}"

run_amai_json() {
  ./scripts/amai_exec.sh "$@" --json | tail -n 1
}

run_amai_last_line() {
  local output_file
  output_file="$(mktemp)"
  ./scripts/amai_exec.sh "$@" >"${output_file}" 2>/dev/null
  python3 - "${output_file}" <<'PY'
import pathlib
import sys

lines = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
for line in reversed(lines):
    if line.strip():
        print(line)
        break
else:
    raise SystemExit("run_amai_last_line: no non-empty output line captured")
PY
  rm -f "${output_file}"
}

suffix="$(amai_unique_suffix)"
workspace_code="stage6_manual_ws_${suffix}"
other_workspace_code="stage6_manual_ws_other_${suffix}"
private_project="stage6_private_${suffix}"
shared_project="stage6_shared_${suffix}"
linked_source_project="stage6_linked_source_${suffix}"
linked_target_project="stage6_linked_target_${suffix}"
unrelated_project="stage6_unrelated_${suffix}"
other_workspace_project="stage6_other_ws_${suffix}"
private_agent_a="stage6_agent_a_${suffix}"
private_agent_b="stage6_agent_b_${suffix}"
transfer_policy_code="stage6_transfer_${suffix}"
access_policy_code="stage6_access_${suffix}"
shared_asset_code="stage6_org_global_asset_${suffix}"
private_root="${REPO_ROOT}/tmp/stage6_manual/private_${suffix}"
shared_root="${REPO_ROOT}/tmp/stage6_manual/shared_${suffix}"
linked_source_root="${REPO_ROOT}/tmp/stage6_manual/linked_source_${suffix}"
linked_target_root="${REPO_ROOT}/tmp/stage6_manual/linked_target_${suffix}"
unrelated_root="${REPO_ROOT}/tmp/stage6_manual/unrelated_${suffix}"
other_workspace_root="${REPO_ROOT}/tmp/stage6_manual/other_workspace_${suffix}"

mkdir -p \
  "${private_root}" \
  "${shared_root}" \
  "${linked_source_root}" \
  "${linked_target_root}" \
  "${unrelated_root}" \
  "${other_workspace_root}"

printf 'stage6 manual acceptance setup: %s\n' "${suffix}"

./scripts/amai_exec.sh workspace ensure \
  --code "${workspace_code}" \
  --display-name "Stage6 Manual Workspace ${suffix}" >/dev/null

./scripts/amai_exec.sh workspace ensure \
  --code "${other_workspace_code}" \
  --display-name "Stage6 Manual Other Workspace ${suffix}" >/dev/null

./scripts/amai_exec.sh project register \
  --code "${private_project}" \
  --display-name "Stage6 Private ${suffix}" \
  --repo-root "${private_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope agent_private >/dev/null

./scripts/amai_exec.sh project register \
  --code "${shared_project}" \
  --display-name "Stage6 Shared ${suffix}" \
  --repo-root "${shared_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope project_shared >/dev/null

./scripts/amai_exec.sh project register \
  --code "${linked_source_project}" \
  --display-name "Stage6 Linked Source ${suffix}" \
  --repo-root "${linked_source_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope project_shared >/dev/null

./scripts/amai_exec.sh project register \
  --code "${linked_target_project}" \
  --display-name "Stage6 Linked Target ${suffix}" \
  --repo-root "${linked_target_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope cross_project_linked >/dev/null

./scripts/amai_exec.sh project register \
  --code "${unrelated_project}" \
  --display-name "Stage6 Unrelated ${suffix}" \
  --repo-root "${unrelated_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope project_shared >/dev/null

./scripts/amai_exec.sh project register \
  --code "${other_workspace_project}" \
  --display-name "Stage6 Other Workspace ${suffix}" \
  --repo-root "${other_workspace_root}" \
  --workspace "${other_workspace_code}" \
  --visibility-scope cross_project_linked >/dev/null

for project_code in \
  "${private_project}" \
  "${shared_project}" \
  "${linked_source_project}" \
  "${linked_target_project}" \
  "${unrelated_project}" \
  "${other_workspace_project}"
do
  ./scripts/amai_exec.sh namespace ensure \
    --project "${project_code}" \
    --code review \
    --display-name Review \
    --retrieval-mode local_strict >/dev/null
done

./scripts/amai_exec.sh agent ensure \
  --workspace "${workspace_code}" \
  --code "${private_agent_a}" \
  --display-name "Stage6 Agent A ${suffix}" \
  --visibility-scope agent_private >/dev/null

./scripts/amai_exec.sh agent ensure \
  --workspace "${workspace_code}" \
  --code "${private_agent_b}" \
  --display-name "Stage6 Agent B ${suffix}" \
  --visibility-scope agent_private >/dev/null

if ./scripts/amai_exec.sh memory create-item \
  --project "${private_project}" \
  --namespace review \
  --item-kind fact \
  --title "private item without owner ${suffix}" \
  --summary "must fail" \
  --sensitivity-class internal \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --derivation-kind operator_write \
  --metadata-json '{"surface":"stage6_manual","case":"private_without_owner"}' >/dev/null 2>&1
then
  printf 'expected private memory write without owner_agent to fail-closed\n' >&2
  exit 1
fi

private_item_json="$(run_amai_json memory create-item \
  --project "${private_project}" \
  --namespace review \
  --owner-agent "${private_agent_a}" \
  --item-kind fact \
  --identity-key "stage6-private-${suffix}" \
  --title "private item ${suffix}" \
  --summary "owner bound private item" \
  --sensitivity-class internal \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "event:stage6-private:${suffix}" \
  --artifact-ref "artifact://stage6/private/${suffix}" \
  --message-ref "message:stage6-private:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"private_with_owner"}' \
  --derivation-kind extract \
  --metadata-json '{"surface":"stage6_manual","case":"private_with_owner"}')"

private_item_id="$(printf '%s\n' "${private_item_json}" | jq -r '.memory_item_id')"
private_owner_id="$(psql "${dsn}" -Atqc "
  SELECT a.code
  FROM ami.memory_items mi
  INNER JOIN ami.agents a ON a.agent_id = mi.owner_agent_id
  WHERE mi.memory_item_id = '${private_item_id}'
")"
test "${private_owner_id}" = "${private_agent_a}"

shared_item_json="$(run_amai_json memory create-item \
  --project "${shared_project}" \
  --namespace review \
  --item-kind fact \
  --identity-key "stage6-shared-${suffix}" \
  --title "shared item ${suffix}" \
  --summary "shared item without owner" \
  --sensitivity-class internal \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "event:stage6-shared:${suffix}" \
  --artifact-ref "artifact://stage6/shared/${suffix}" \
  --message-ref "message:stage6-shared:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"shared_without_owner"}' \
  --derivation-kind extract \
  --metadata-json '{"surface":"stage6_manual","case":"shared_without_owner"}')"

shared_item_id="$(printf '%s\n' "${shared_item_json}" | jq -r '.memory_item_id')"
shared_owner_id="$(psql "${dsn}" -Atqc "
  SELECT COALESCE(owner_agent_id::text, '')
  FROM ami.memory_items
  WHERE memory_item_id = '${shared_item_id}'
")"
test -z "${shared_owner_id}"

pre_relation_context="$(run_amai_last_line context pack \
  --project "${linked_source_project}" \
  --namespace review \
  --query "stage6 manual visibility ${suffix}" \
  --retrieval-mode local_plus_related \
  --disable-cache \
  --token-source-kind proof_stage6_manual_acceptance)"
pre_relation_context_pack_id="$(printf '%s\n' "${pre_relation_context}" | jq -r '.context_pack_id')"
pre_relation_unrelated_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${pre_relation_context_pack_id}'
      AND elem->>'project_code' = '${unrelated_project}'
  )::text
")"
test "${pre_relation_unrelated_visible}" = "false"

./scripts/amai_exec.sh transfer-policy ensure \
  --workspace "${workspace_code}" \
  --code "${transfer_policy_code}" \
  --display-name "Stage6 Transfer ${suffix}" \
  --default-decision borrowed_unverified \
  --allow-cross-project-read \
  --allow-import \
  --requires-human-approval >/dev/null

./scripts/amai_exec.sh relation add \
  --source "${linked_source_project}" \
  --target "${linked_target_project}" \
  --relation-type depends_on \
  --project-link-type shared_codebase \
  --shared-contour stage6_manual_acceptance \
  --visibility-scope cross_project_linked \
  --relation-status active \
  --requires-approval \
  --transfer-policy "${transfer_policy_code}" \
  --access-mode local_plus_related >/dev/null

pre_policy_context="$(run_amai_last_line context pack \
  --project "${linked_source_project}" \
  --namespace review \
  --query "stage6 manual linked visibility ${suffix}" \
  --retrieval-mode local_plus_related \
  --disable-cache \
  --token-source-kind proof_stage6_manual_acceptance)"
pre_policy_context_pack_id="$(printf '%s\n' "${pre_policy_context}" | jq -r '.context_pack_id')"
pre_policy_target_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${pre_policy_context_pack_id}'
      AND elem->>'project_code' = '${linked_target_project}'
  )::text
")"
test "${pre_policy_target_visible}" = "false"

if ./scripts/amai_exec.sh import-packet create \
  --source-project "${linked_source_project}" \
  --target-project "${linked_target_project}" \
  --transfer-policy "${transfer_policy_code}" \
  --requested-by-agent "${private_agent_a}" \
  --status borrowed_unverified \
  --summary "stage6 manual pre-policy ${suffix}" \
  --reason "stage6 manual pre-policy ${suffix}" \
  --imported-by-agent-scope cross_project_linked \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --can-promote-after-verification \
  --memory-object-id "memory::pre-policy::${suffix}" >/dev/null 2>&1
then
  printf 'expected import-packet create to fail before explicit access policy grant\n' >&2
  exit 1
fi

./scripts/amai_exec.sh access-policy ensure \
  --workspace "${workspace_code}" \
  --project "${linked_source_project}" \
  --code "${access_policy_code}" \
  --display-name "Stage6 Cross Read ${suffix}" \
  --object-class fact \
  --scope-type cross_project_linked \
  --precedence 250 \
  --can-read \
  --can-link \
  --can-import \
  --can-promote \
  --can-approve-transfer \
  --human-override \
  --override-reason "stage6 manual acceptance ${suffix}" >/dev/null

post_policy_context="$(run_amai_last_line context pack \
  --project "${linked_source_project}" \
  --namespace review \
  --query "stage6 manual linked visibility ${suffix}" \
  --retrieval-mode local_plus_related \
  --disable-cache \
  --token-source-kind proof_stage6_manual_acceptance)"
post_policy_context_pack_id="$(printf '%s\n' "${post_policy_context}" | jq -r '.context_pack_id')"
post_policy_target_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${post_policy_context_pack_id}'
      AND elem->>'project_code' = '${linked_target_project}'
  )::text
")"
post_policy_unrelated_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${post_policy_context_pack_id}'
      AND elem->>'project_code' = '${unrelated_project}'
  )::text
")"
test "${post_policy_target_visible}" = "true"
test "${post_policy_unrelated_visible}" = "false"

packet_json="$(run_amai_json import-packet create \
  --source-project "${linked_source_project}" \
  --target-project "${linked_target_project}" \
  --transfer-policy "${transfer_policy_code}" \
  --requested-by-agent "${private_agent_a}" \
  --status borrowed_unverified \
  --summary "stage6 manual packet ${suffix}" \
  --reason "stage6 manual packet ${suffix}" \
  --imported-by-agent-scope cross_project_linked \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --can-promote-after-verification \
  --memory-object-id "memory::stage6::${suffix}" \
  --artifact-ref "artifact://stage6/import/${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"controlled_transfer"}')"
packet_id="$(printf '%s\n' "${packet_json}" | jq -r '.import_packet_id')"

imported_item_json="$(run_amai_json memory create-item \
  --project "${linked_target_project}" \
  --namespace review \
  --source-project "${linked_source_project}" \
  --import-packet-id "${packet_id}" \
  --item-kind fact \
  --identity-key "stage6-imported-${suffix}" \
  --title "imported item ${suffix}" \
  --summary "controlled import" \
  --sensitivity-class internal \
  --truth-state proposed \
  --trust-state proposed \
  --verification-state unverified \
  --lifecycle-state hot \
  --source-event-id "event:stage6-imported:${suffix}" \
  --artifact-ref "artifact://stage6/imported-item/${suffix}" \
  --message-ref "message:stage6-imported:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"imported_item"}' \
  --derivation-kind extract \
  --metadata-json '{"surface":"stage6_manual","case":"imported_item"}')"
imported_item_id="$(printf '%s\n' "${imported_item_json}" | jq -r '.memory_item_id')"
imported_packet_match="$(psql "${dsn}" -Atqc "
  SELECT (import_packet_id::text = '${packet_id}')::text
  FROM ami.memory_items
  WHERE memory_item_id = '${imported_item_id}'
")"
test "${imported_packet_match}" = "true"
imported_visibility_scope="$(psql "${dsn}" -Atqc "
  SELECT visibility_scope
  FROM ami.memory_items
  WHERE memory_item_id = '${imported_item_id}'
")"
test "${imported_visibility_scope}" = "imported"

if ./scripts/amai_exec.sh shared-asset ensure \
  --workspace "${workspace_code}" \
  --code "${shared_asset_code}" \
  --display-name "Stage6 Missing Policy ${suffix}" \
  --asset-kind artifact \
  --source-project "${linked_source_project}" \
  --visibility-scope org_global >/dev/null 2>&1
then
  printf 'expected org_global shared asset without transfer policy to fail-closed\n' >&2
  exit 1
fi

run_amai_json shared-asset ensure \
  --workspace "${workspace_code}" \
  --code "${shared_asset_code}" \
  --display-name "Stage6 Org Global ${suffix}" \
  --asset-kind artifact \
  --source-project "${linked_source_project}" \
  --transfer-policy "${transfer_policy_code}" \
  --visibility-scope org_global \
  --source-event-id "event:stage6-asset-a:${suffix}" \
  --artifact-ref "artifact://stage6/asset-a/${suffix}" \
  --message-ref "message:stage6-asset-a:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"org_global_workspace_a"}' >/dev/null

./scripts/amai_exec.sh transfer-policy ensure \
  --workspace "${other_workspace_code}" \
  --code "${transfer_policy_code}_other" \
  --display-name "Stage6 Other Transfer ${suffix}" \
  --default-decision borrowed_unverified \
  --allow-cross-project-read \
  --allow-import \
  --requires-human-approval >/dev/null

run_amai_json shared-asset ensure \
  --workspace "${other_workspace_code}" \
  --code "${shared_asset_code}" \
  --display-name "Stage6 Org Global Other ${suffix}" \
  --asset-kind artifact \
  --source-project "${other_workspace_project}" \
  --transfer-policy "${transfer_policy_code}_other" \
  --visibility-scope org_global \
  --source-event-id "event:stage6-asset-b:${suffix}" \
  --artifact-ref "artifact://stage6/asset-b/${suffix}" \
  --message-ref "message:stage6-asset-b:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"org_global_workspace_b"}' >/dev/null

./scripts/amai_exec.sh shared-asset bind \
  --asset "${shared_asset_code}" \
  --project "${linked_target_project}" \
  --binding-kind consumer \
  --source-event-id "event:stage6-bind-a:${suffix}" \
  --artifact-ref "artifact://stage6/bind-a/${suffix}" \
  --message-ref "message:stage6-bind-a:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"bind_workspace_a"}' >/dev/null

bound_workspace_code="$(psql "${dsn}" -Atqc "
  SELECT w.code
  FROM ami.shared_asset_projects sap
  INNER JOIN ami.shared_assets sa ON sa.shared_asset_id = sap.shared_asset_id
  INNER JOIN ami.workspaces w ON w.workspace_id = sa.workspace_id
  INNER JOIN ami.projects p ON p.project_id = sap.project_id
  WHERE sa.code = '${shared_asset_code}'
    AND p.code = '${linked_target_project}'
")"
test "${bound_workspace_code}" = "${workspace_code}"

./scripts/amai_exec.sh shared-asset bind \
  --asset "${shared_asset_code}" \
  --project "${other_workspace_project}" \
  --binding-kind consumer \
  --source-event-id "event:stage6-bind-other:${suffix}" \
  --artifact-ref "artifact://stage6/bind-other/${suffix}" \
  --message-ref "message:stage6-bind-other:${suffix}" \
  --evidence-span-json '{"surface":"stage6_manual","case":"bind_other_workspace"}' >/dev/null

bound_other_workspace_code="$(psql "${dsn}" -Atqc "
  SELECT w.code
  FROM ami.shared_asset_projects sap
  INNER JOIN ami.shared_assets sa ON sa.shared_asset_id = sap.shared_asset_id
  INNER JOIN ami.workspaces w ON w.workspace_id = sa.workspace_id
  INNER JOIN ami.projects p ON p.project_id = sap.project_id
  WHERE sa.code = '${shared_asset_code}'
    AND p.code = '${other_workspace_project}'
")"
test "${bound_other_workspace_code}" = "${other_workspace_code}"

printf 'proof_shared_private_memory_manual_acceptance: ok\n'
