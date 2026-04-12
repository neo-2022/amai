#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
dsn="$(grep '^AMI_POSTGRES_DSN=' "${repo_root}/.env" | cut -d= -f2-)"
suffix="$$"
workspace_code="scope_stage1_${suffix}"
team_code="core_${suffix}"
role_code="operator_${suffix}"
agent_code="codex_${suffix}"
policy_bind_code="project_fact_reader_${suffix}"
source_project="scope_src_${suffix}"
target_project="scope_dst_${suffix}"
policy_code="borrow_guard_${suffix}"
shared_asset_code="shared_dep_${suffix}"
source_root="$(mktemp -d)"
target_root="$(mktemp -d)"
memory_item_id=""
imported_memory_item_id=""
promoted_packet_id=""
promoted_memory_item_id=""

canonical_path() {
  local path="$1"
  (
    cd "${path}"
    pwd -P
  )
}

cleanup() {
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.scope_override_events WHERE reason LIKE 'stage1 scope proof %'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.shared_asset_projects WHERE shared_asset_id IN (SELECT shared_asset_id FROM ami.shared_assets WHERE code = '${shared_asset_code}')" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.shared_assets WHERE code = '${shared_asset_code}'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.import_packets WHERE summary LIKE 'stage1 scope proof ${suffix}%'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.project_relations WHERE relation_type = 'depends_on' AND shared_contour = 'stage1_scope_proof'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.access_policies WHERE code = '${policy_bind_code}'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.agents WHERE code = '${agent_code}'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.agent_roles WHERE code = '${role_code}'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.projects WHERE code IN ('${source_project}', '${target_project}')" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.teams WHERE code = '${team_code}'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.transfer_policies WHERE code = '${policy_code}'" >/dev/null 2>&1 || true
  psql "${dsn}" -v ON_ERROR_STOP=1 -qc "DELETE FROM ami.workspaces WHERE code = '${workspace_code}'" >/dev/null 2>&1 || true
  rm -rf "${source_root}" "${target_root}"
}
trap cleanup EXIT

source_root="$(canonical_path "${source_root}")"
target_root="$(canonical_path "${target_root}")"

cd "${repo_root}"

./scripts/scope_identity_surface_guard.sh --json >/dev/null

psql "${dsn}" -v ON_ERROR_STOP=1 -f "${repo_root}/sql/000_bootstrap.sql" >/dev/null
cargo build --quiet --release

./target/release/amai workspace ensure \
  --code "${workspace_code}" \
  --display-name "Stage 1 Workspace ${suffix}" >/dev/null

./target/release/amai team ensure \
  --workspace "${workspace_code}" \
  --code "${team_code}" \
  --display-name "Core Team ${suffix}" >/dev/null

./target/release/amai role ensure \
  --workspace "${workspace_code}" \
  --code "${role_code}" \
  --display-name "Operator ${suffix}" >/dev/null

./target/release/amai agent ensure \
  --workspace "${workspace_code}" \
  --team "${team_code}" \
  --role "${role_code}" \
  --code "${agent_code}" \
  --display-name "Codex ${suffix}" \
  --visibility-scope team_shared >/dev/null

./target/release/amai project register \
  --code "${source_project}" \
  --display-name "Stage 1 Source ${suffix}" \
  --repo-root "${source_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope project_shared >/dev/null

./target/release/amai project register \
  --code "${target_project}" \
  --display-name "Stage 1 Target ${suffix}" \
  --repo-root "${target_root}" \
  --workspace "${workspace_code}" \
  --visibility-scope cross_project_linked >/dev/null

./target/release/amai transfer-policy ensure \
  --workspace "${workspace_code}" \
  --code "${policy_code}" \
  --display-name "Borrow Guard ${suffix}" \
  --default-decision borrowed_unverified \
  --allow-cross-project-read \
  --allow-import \
  --requires-human-approval >/dev/null

./target/release/amai relation add \
  --source "${source_project}" \
  --target "${target_project}" \
  --relation-type depends_on \
  --project-link-type shared_codebase \
  --shared-contour stage1_scope_proof \
  --visibility-scope cross_project_linked \
  --relation-status active \
  --requires-approval \
  --transfer-policy "${policy_code}" \
  --access-mode local_plus_related >/dev/null

pre_policy_context="$(
  ./target/release/amai context pack \
    --project "${source_project}" \
    --namespace default \
    --query "stage1-scope-read-${suffix}" \
    --retrieval-mode local_plus_related \
    --disable-cache
)"
pre_policy_context_pack_id="$(printf '%s\n' "${pre_policy_context}" | sed -n 's/.*"context_pack_id":"\([^"]*\)".*/\1/p')"
pre_policy_target_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${pre_policy_context_pack_id}'
      AND elem->>'project_code' = '${target_project}'
  )::text
")"

if [ "${pre_policy_target_visible}" = "true" ]; then
  printf 'expected linked project to stay unreadable before explicit access policy\n' >&2
  exit 1
fi

if ./target/release/amai import-packet create \
  --source-project "${source_project}" \
  --target-project "${target_project}" \
  --transfer-policy "${policy_code}" \
  --requested-by-agent "${agent_code}" \
  --status borrowed_unverified \
  --summary "stage1 scope proof ${suffix} default deny" \
  --reason "stage1 scope proof ${suffix} default deny" \
  --imported-by-agent-scope cross_project_linked \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --can-promote-after-verification \
  --memory-object-id "memory::default-deny::${suffix}" \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-import-default-deny:${suffix}" \
  --artifact-ref "artifact://stage1/import-default-deny/${suffix}" \
  --message-ref "message:stage1-import-default-deny:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"import_packet_default_deny\",\"suffix\":\"${suffix}\"}" >/dev/null 2>&1; then
  printf 'expected default_deny import before access policy grant\n' >&2
  exit 1
fi

./target/release/amai access-policy ensure \
  --workspace "${workspace_code}" \
  --role "${role_code}" \
  --team "${team_code}" \
  --project "${source_project}" \
  --code "${policy_bind_code}" \
  --display-name "Project Fact Reader ${suffix}" \
  --object-class fact \
  --scope-type cross_project_linked \
  --precedence 250 \
  --can-read \
  --can-link \
  --can-import \
  --can-promote \
  --can-approve-transfer \
  --human-override \
  --override-reason "stage1 scope proof ${suffix}" >/dev/null

post_policy_context="$(
  ./target/release/amai context pack \
    --project "${source_project}" \
    --namespace default \
    --query "stage1-scope-read-${suffix}" \
    --retrieval-mode local_plus_related \
    --disable-cache
)"
post_policy_context_pack_id="$(printf '%s\n' "${post_policy_context}" | sed -n 's/.*"context_pack_id":"\([^"]*\)".*/\1/p')"
post_policy_target_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${post_policy_context_pack_id}'
      AND elem->>'project_code' = '${target_project}'
  )::text
")"

if [ "${post_policy_target_visible}" != "true" ]; then
  printf 'expected linked project to become visible only after explicit access policy\n' >&2
  exit 1
fi

./target/release/amai shared-asset ensure \
  --workspace "${workspace_code}" \
  --code "${shared_asset_code}" \
  --display-name "Shared Dependency ${suffix}" \
  --asset-kind dependency \
  --source-project "${source_project}" \
  --transfer-policy "${policy_code}" \
  --visibility-scope cross_project_linked \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-asset:${suffix}" \
  --artifact-ref "artifact://stage1/shared-asset/${suffix}" \
  --message-ref "message:stage1-shared-asset:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"shared_asset_ensure\",\"suffix\":\"${suffix}\"}" >/dev/null

./target/release/amai shared-asset bind \
  --asset "${shared_asset_code}" \
  --project "${source_project}" \
  --binding-kind owner \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-bind-owner:${suffix}" \
  --artifact-ref "artifact://stage1/shared-asset-bind-owner/${suffix}" \
  --message-ref "message:stage1-bind-owner:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"shared_asset_bind_owner\",\"suffix\":\"${suffix}\"}" >/dev/null

./target/release/amai shared-asset bind \
  --asset "${shared_asset_code}" \
  --project "${target_project}" \
  --binding-kind consumer \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-bind-consumer:${suffix}" \
  --artifact-ref "artifact://stage1/shared-asset-bind-consumer/${suffix}" \
  --message-ref "message:stage1-bind-consumer:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"shared_asset_bind_consumer\",\"suffix\":\"${suffix}\"}" >/dev/null

packet_output="$(./target/release/amai import-packet create \
  --source-project "${source_project}" \
  --target-project "${target_project}" \
  --transfer-policy "${policy_code}" \
  --requested-by-agent "${agent_code}" \
  --status borrowed_unverified \
  --summary "stage1 scope proof ${suffix}" \
  --reason "stage1 scope proof ${suffix}" \
  --imported-by-agent-scope cross_project_linked \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --can-promote-after-verification \
  --memory-object-id "memory::${suffix}" \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-import:${suffix}" \
  --artifact-ref "artifact://stage1/${suffix}" \
  --message-ref "message:stage1-import:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"import_packet_create\",\"suffix\":\"${suffix}\"}")"

packet_id="$(printf '%s\n' "${packet_output}" | awk '{print $4}')"

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    'project_shared',
    'fact',
    'stage1 local wrong scope ${suffix}',
    'proposed',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n
    ON n.project_id = p.project_id
   AND n.code = 'default'
  WHERE p.code = '${target_project}'
" >/dev/null 2>&1; then
  printf 'expected local memory item with wrong project scope to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    src.project_id,
    '${packet_id}'::uuid,
    'project_shared',
    'fact',
    'stage1 borrowed masquerade local visibility ${suffix}',
    'proposed',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects src
  JOIN ami.projects dst
    ON dst.code = '${target_project}'
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE src.code = '${source_project}'
" >/dev/null 2>&1; then
  printf 'expected borrowed/unverified memory item with local visibility to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    src.project_id,
    '${packet_id}'::uuid,
    'imported',
    'fact',
    'stage1 borrowed masquerade current truth ${suffix}',
    'current',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects src
  JOIN ami.projects dst
    ON dst.code = '${target_project}'
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE src.code = '${source_project}'
" >/dev/null 2>&1; then
  printf 'expected borrowed/unverified memory item with current truth_state to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    src.project_id,
    '${packet_id}'::uuid,
    'imported',
    'fact',
    'stage1 borrowed masquerade verified truth ${suffix}',
    'proposed',
    'verified',
    'hot',
    '{}'::jsonb
  FROM ami.projects src
  JOIN ami.projects dst
    ON dst.code = '${target_project}'
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE src.code = '${source_project}'
" >/dev/null 2>&1; then
  printf 'expected borrowed/unverified memory item with verified verification_state to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    src.project_id,
    'imported',
    'fact',
    'stage1 bypass without import packet ${suffix}',
    'proposed',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects src
  JOIN ami.projects dst
    ON dst.code = '${target_project}'
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE src.code = '${source_project}'
" >/dev/null 2>&1; then
  printf 'expected cross-project memory item without import_packet to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    '${packet_id}'::uuid,
    'imported',
    'fact',
    'stage1 bypass without source project ${suffix}',
    'proposed',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects dst
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE dst.code = '${target_project}'
" >/dev/null 2>&1; then
  printf 'expected cross-project memory item without source_project_id to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    dst.project_id,
    '${packet_id}'::uuid,
    'imported',
    'fact',
    'stage1 bypass with mismatched source ${suffix}',
    'proposed',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects dst
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE dst.code = '${target_project}'
" >/dev/null 2>&1; then
  printf 'expected cross-project memory item with mismatched source project to fail-closed\n' >&2
  exit 1
fi

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    src.workspace_id,
    src.project_id,
    n.namespace_id,
    src.project_id,
    '${packet_id}'::uuid,
    'imported',
    'fact',
    'stage1 bypass with mismatched target ${suffix}',
    'proposed',
    'unverified',
    'hot',
    '{}'::jsonb
  FROM ami.projects src
  JOIN ami.namespaces n
    ON n.project_id = src.project_id
   AND n.code = 'default'
  WHERE src.code = '${source_project}'
" >/dev/null 2>&1; then
  printf 'expected cross-project memory item with mismatched target project to fail-closed\n' >&2
  exit 1
fi

imported_memory_item_id="$(
  psql "${dsn}" -Atqc "
    INSERT INTO ami.memory_items(
      workspace_id,
      project_id,
      namespace_id,
      source_project_id,
      import_packet_id,
      visibility_scope,
      item_kind,
      title,
      truth_state,
      verification_state,
      lifecycle_state,
      metadata
    )
    SELECT
      dst.workspace_id,
      dst.project_id,
      n.namespace_id,
      src.project_id,
      '${packet_id}'::uuid,
      'imported',
      'fact',
      'stage1 imported fact ${suffix}',
      'proposed',
      'unverified',
      'hot',
      jsonb_build_object('proof', 'stage1_scope_identity_control_plane')
    FROM ami.projects src
    JOIN ami.projects dst
      ON dst.code = '${target_project}'
    JOIN ami.namespaces n
      ON n.project_id = dst.project_id
     AND n.code = 'default'
    WHERE src.code = '${source_project}'
    RETURNING memory_item_id
  "
)"

promoted_packet_output="$(./target/release/amai import-packet create \
  --source-project "${source_project}" \
  --target-project "${target_project}" \
  --transfer-policy "${policy_code}" \
  --requested-by-agent "${agent_code}" \
  --status borrowed_unverified \
  --summary "stage1 scope proof ${suffix} promotable" \
  --reason "stage1 scope proof ${suffix} promotable" \
  --imported-by-agent-scope cross_project_linked \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --can-promote-after-verification \
  --memory-object-id "memory::promotable::${suffix}" \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-import-promotable:${suffix}" \
  --artifact-ref "artifact://stage1/import-promotable/${suffix}" \
  --message-ref "message:stage1-import-promotable:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"import_packet_promotable\",\"suffix\":\"${suffix}\"}")"

promoted_packet_id="$(printf '%s\n' "${promoted_packet_output}" | awk '{print $4}')"

./target/release/amai import-packet update \
  --import-packet-id "${promoted_packet_id}" \
  --status verified \
  --borrowed-status verified_local_copy \
  --verification-state verified \
  --trust-state verified \
  --actor-agent "${agent_code}" \
  --reason "stage1 scope proof ${suffix} promote verified local copy" >/dev/null

if psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    source_project_id,
    import_packet_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    verification_state,
    lifecycle_state,
    metadata
  )
  SELECT
    dst.workspace_id,
    dst.project_id,
    n.namespace_id,
    src.project_id,
    '${promoted_packet_id}'::uuid,
    'project_shared',
    'fact',
    'stage1 verified local copy wrong scope ${suffix}',
    'current',
    'verified',
    'hot',
    '{}'::jsonb
  FROM ami.projects src
  JOIN ami.projects dst
    ON dst.code = '${target_project}'
  JOIN ami.namespaces n
    ON n.project_id = dst.project_id
   AND n.code = 'default'
  WHERE src.code = '${source_project}'
" >/dev/null 2>&1; then
  printf 'expected verified local copy memory item with wrong target scope to fail-closed\n' >&2
  exit 1
fi

promoted_memory_item_id="$(
  psql "${dsn}" -Atqc "
    INSERT INTO ami.memory_items(
      workspace_id,
      project_id,
      namespace_id,
      source_project_id,
      import_packet_id,
      visibility_scope,
      item_kind,
      title,
      truth_state,
      verification_state,
      lifecycle_state,
      metadata
    )
    SELECT
      dst.workspace_id,
      dst.project_id,
      n.namespace_id,
      src.project_id,
      '${promoted_packet_id}'::uuid,
      dst.visibility_scope,
      'fact',
      'stage1 verified local copy fact ${suffix}',
      'current',
      'verified',
      'hot',
      jsonb_build_object('proof', 'stage1_verified_local_copy')
    FROM ami.projects src
    JOIN ami.projects dst
      ON dst.code = '${target_project}'
    JOIN ami.namespaces n
      ON n.project_id = dst.project_id
     AND n.code = 'default'
    WHERE src.code = '${source_project}'
    RETURNING memory_item_id
  "
)"

./target/release/amai relation update \
  --source "${source_project}" \
  --target "${target_project}" \
  --relation-type depends_on \
  --shared-contour stage1_scope_proof \
  --visibility-scope quarantine \
  --relation-status forbidden \
  --requires-approval true \
  --actor-agent "${agent_code}" \
  --override-reason "stage1 scope proof ${suffix} relation revoked" >/dev/null

revoked_context="$(
  ./target/release/amai context pack \
    --project "${source_project}" \
    --namespace default \
    --query "stage1-scope-read-${suffix}" \
    --retrieval-mode local_plus_related \
    --disable-cache
)"
revoked_context_pack_id="$(printf '%s\n' "${revoked_context}" | sed -n 's/.*"context_pack_id":"\([^"]*\)".*/\1/p')"
revoked_target_visible="$(psql "${dsn}" -Atqc "
  SELECT EXISTS (
    SELECT 1
    FROM ami.context_packs cp,
         jsonb_array_elements(cp.visible_projects) AS elem
    WHERE cp.context_pack_id = '${revoked_context_pack_id}'
      AND elem->>'project_code' = '${target_project}'
  )::text
")"

if [ "${revoked_target_visible}" = "true" ]; then
  printf 'expected linked project visibility to fail-closed after relation revoke\n' >&2
  exit 1
fi

if ./target/release/amai import-packet create \
  --source-project "${source_project}" \
  --target-project "${target_project}" \
  --transfer-policy "${policy_code}" \
  --status borrowed_unverified \
  --summary "stage1 scope proof ${suffix} should fail" \
  --source-kind "stage1_scope_proof" \
  --source-event-id "event:stage1-import-revoked:${suffix}" \
  --artifact-ref "artifact://stage1/import-revoked/${suffix}" \
  --message-ref "message:stage1-import-revoked:${suffix}" \
  --evidence-span-json "{\"surface\":\"stage1_scope_proof\",\"case\":\"import_packet_after_revoke\",\"suffix\":\"${suffix}\"}" >/dev/null 2>&1; then
  printf 'expected fail-closed import after relation revoke\n' >&2
  exit 1
fi

if ./target/release/amai import-packet update \
  --import-packet-id "${packet_id}" \
  --status verified \
  --verification-state verified \
  --actor-agent "${agent_code}" \
  --reason "stage1 scope proof ${suffix} illegal promote" >/dev/null 2>&1; then
  printf 'expected fail-closed promotion after relation revoke\n' >&2
  exit 1
fi

./target/release/amai import-packet update \
  --import-packet-id "${packet_id}" \
  --status revoked \
  --reason "stage1 scope proof ${suffix} packet revoked" \
  --borrowed-status expired \
  --verification-state rejected \
  --trust-state disputed \
  --actor-agent "${agent_code}" >/dev/null

memory_item_id="$(
  psql "${dsn}" -Atqc "
    INSERT INTO ami.memory_items(
      workspace_id,
      project_id,
      namespace_id,
      visibility_scope,
      item_kind,
      title,
      truth_state,
      verification_state,
      lifecycle_state,
      source_event_ids,
      evidence_span,
      metadata
    )
    SELECT
      p.workspace_id,
      p.project_id,
      n.namespace_id,
      p.visibility_scope,
      'fact',
      'stage1 scoped fact ${suffix}',
      'proposed',
      'unverified',
      'hot',
      '[\"event:stage1-local:${suffix}\"]'::jsonb,
      '{\"proof\":\"stage1_scope_identity_control_plane\",\"line\":1}'::jsonb,
      '{}'::jsonb
    FROM ami.projects p
    JOIN ami.namespaces n
      ON n.project_id = p.project_id
     AND n.code = 'default'
    WHERE p.code = '${target_project}'
    RETURNING memory_item_id
  "
)"

test "$(psql "${dsn}" -Atqc "SELECT status FROM ami.workspaces WHERE code = '${workspace_code}'")" = "active"
test "$(psql "${dsn}" -Atqc "SELECT status FROM ami.agent_roles WHERE code = '${role_code}'")" = "active"
test "$(psql "${dsn}" -Atqc "SELECT visibility_scope FROM ami.agents WHERE code = '${agent_code}'")" = "team_shared"
test "$(psql "${dsn}" -Atqc "SELECT role_id IS NOT NULL FROM ami.agents WHERE code = '${agent_code}'")" = "t"
test "$(psql "${dsn}" -Atqc "SELECT visibility_scope FROM ami.projects WHERE code = '${source_project}'")" = "project_shared"
test "$(psql "${dsn}" -Atqc "SELECT visibility_scope FROM ami.projects WHERE code = '${target_project}'")" = "cross_project_linked"
test "$(psql "${dsn}" -Atqc "SELECT default_decision FROM ami.transfer_policies WHERE code = '${policy_code}'")" = "borrowed_unverified"
test "$(psql "${dsn}" -Atqc "SELECT can_import::text FROM ami.access_policies WHERE code = '${policy_bind_code}'")" = "true"
test "$(psql "${dsn}" -Atqc "SELECT can_promote::text FROM ami.access_policies WHERE code = '${policy_bind_code}'")" = "true"
test "$(psql "${dsn}" -Atqc "SELECT precedence::text FROM ami.access_policies WHERE code = '${policy_bind_code}'")" = "250"
test "$(psql "${dsn}" -Atqc "SELECT scope_type FROM ami.access_policies WHERE code = '${policy_bind_code}'")" = "cross_project_linked"
test "$(psql "${dsn}" -Atqc "SELECT project_link_type FROM ami.project_relations WHERE relation_type = 'depends_on' AND shared_contour = 'stage1_scope_proof' AND source_project_id = (SELECT project_id FROM ami.projects WHERE code = '${source_project}') AND target_project_id = (SELECT project_id FROM ami.projects WHERE code = '${target_project}')")" = "shared_codebase"
test "$(psql "${dsn}" -Atqc "SELECT relation_status FROM ami.project_relations WHERE relation_type = 'depends_on' AND shared_contour = 'stage1_scope_proof' AND source_project_id = (SELECT project_id FROM ami.projects WHERE code = '${source_project}') AND target_project_id = (SELECT project_id FROM ami.projects WHERE code = '${target_project}')")" = "forbidden"
test "$(psql "${dsn}" -Atqc "SELECT requires_approval::text FROM ami.project_relations WHERE relation_type = 'depends_on' AND shared_contour = 'stage1_scope_proof' AND source_project_id = (SELECT project_id FROM ami.projects WHERE code = '${source_project}') AND target_project_id = (SELECT project_id FROM ami.projects WHERE code = '${target_project}')")" = "true"
test "$(psql "${dsn}" -Atqc "SELECT count(*)::text FROM ami.shared_asset_projects WHERE shared_asset_id = (SELECT shared_asset_id FROM ami.shared_assets WHERE code = '${shared_asset_code}')")" = "2"
test "$(psql "${dsn}" -Atqc "SELECT allowed_by_project_link::text FROM ami.import_packets WHERE import_packet_id = '${packet_id}'")" = "true"
test "$(psql "${dsn}" -Atqc "SELECT status FROM ami.import_packets WHERE import_packet_id = '${packet_id}'")" = "revoked"
test "$(psql "${dsn}" -Atqc "SELECT borrowed_status FROM ami.import_packets WHERE import_packet_id = '${packet_id}'")" = "expired"
test "$(psql "${dsn}" -Atqc "SELECT verification_state FROM ami.import_packets WHERE import_packet_id = '${packet_id}'")" = "rejected"
test "$(psql "${dsn}" -Atqc "SELECT trust_state FROM ami.import_packets WHERE import_packet_id = '${packet_id}'")" = "disputed"
test "$(psql "${dsn}" -Atqc "SELECT status FROM ami.import_packets WHERE import_packet_id = '${promoted_packet_id}'")" = "verified"
test "$(psql "${dsn}" -Atqc "SELECT borrowed_status FROM ami.import_packets WHERE import_packet_id = '${promoted_packet_id}'")" = "verified_local_copy"
test "$(psql "${dsn}" -Atqc "SELECT verification_state FROM ami.import_packets WHERE import_packet_id = '${promoted_packet_id}'")" = "verified"
test "$(psql "${dsn}" -Atqc "SELECT trust_state FROM ami.import_packets WHERE import_packet_id = '${promoted_packet_id}'")" = "verified"
printf '%s\n' "${post_policy_context}" | jq -e 'has("memory_items") | not' >/dev/null
printf '%s\n' "${revoked_context}" | jq -e 'has("memory_items") | not' >/dev/null
test "$(psql "${dsn}" -Atqc "
  SELECT count(*)::text
  FROM information_schema.columns
  WHERE table_schema = 'ami'
    AND table_name IN (
      'memory_cards',
      'task_nodes',
      'task_events',
      'memory_edges',
      'memory_conflicts',
      'memory_provenance',
      'restore_packs',
      'policy_rules',
      'quarantine_items'
    )
    AND column_name IN ('source_project_id', 'import_packet_id')
")" = "0"
test "$(psql "${dsn}" -Atqc "
  SELECT count(*)::text
  FROM information_schema.columns
  WHERE table_schema = 'ami'
    AND table_name = 'memory_items'
    AND column_name IN ('source_project_id', 'import_packet_id')
")" = "2"
test "$(psql "${dsn}" -Atqc "
  SELECT count(*)::text
  FROM information_schema.columns
  WHERE table_schema = 'ami'
    AND table_name = 'memory_cards'
    AND column_name IN ('source_project_id', 'import_packet_id')
")" = "0"
test "$(psql "${dsn}" -Atqc "SELECT (source_project_id IS NOT NULL)::text FROM ami.memory_items WHERE memory_item_id = '${imported_memory_item_id}'")" = "true"
test "$(psql "${dsn}" -Atqc "SELECT import_packet_id::text FROM ami.memory_items WHERE memory_item_id = '${imported_memory_item_id}'")" = "${packet_id}"
test "$(psql "${dsn}" -Atqc "SELECT visibility_scope FROM ami.memory_items WHERE memory_item_id = '${imported_memory_item_id}'")" = "imported"
test "$(psql "${dsn}" -Atqc "SELECT truth_state FROM ami.memory_items WHERE memory_item_id = '${imported_memory_item_id}'")" = "proposed"
test "$(psql "${dsn}" -Atqc "SELECT verification_state FROM ami.memory_items WHERE memory_item_id = '${imported_memory_item_id}'")" = "unverified"
test "$(psql "${dsn}" -Atqc "SELECT visibility_scope FROM ami.memory_items WHERE memory_item_id = '${promoted_memory_item_id}'")" = "cross_project_linked"
test "$(psql "${dsn}" -Atqc "SELECT truth_state FROM ami.memory_items WHERE memory_item_id = '${promoted_memory_item_id}'")" = "current"
test "$(psql "${dsn}" -Atqc "SELECT verification_state FROM ami.memory_items WHERE memory_item_id = '${promoted_memory_item_id}'")" = "verified"
test "$(psql "${dsn}" -Atqc "SELECT visibility_scope FROM ami.memory_items WHERE memory_item_id = '${memory_item_id}'")" = "cross_project_linked"
test "$(psql "${dsn}" -Atqc "SELECT count(*)::text FROM ami.scope_override_events WHERE reason LIKE 'stage1 scope proof ${suffix}%'")" = "3"

printf 'proof_scope_identity_control_plane: PASS\n'
