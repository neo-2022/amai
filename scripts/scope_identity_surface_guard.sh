#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd -P)"

json_mode=0
if [[ "${1:-}" == "--json" ]]; then
  json_mode=1
fi

dsn="$(grep '^AMI_POSTGRES_DSN=' .env | cut -d= -f2-)"

readarray -t direct_insert_hits < <(
  rg -n "INSERT INTO ami\\.memory_items" src scripts sql 2>/dev/null \
    | grep -v '^src/postgres.rs:' \
    | grep -v '^scripts/proof_scope_identity_control_plane.sh:' \
    | grep -v '^scripts/proof_typed_memory_envelope_contract.sh:' \
    | grep -v '^scripts/proof_runtime_sufficiency_router.sh:'
)

readarray -t retrieval_mentions < <(
  rg -n '\bmemory_items\b' src/retrieval.rs 2>/dev/null
)

direct_insert_json='[]'
if ((${#direct_insert_hits[@]} > 0)); then
  direct_insert_json="$(printf '%s\n' "${direct_insert_hits[@]}" | jq -Rsc 'split("\n")[:-1]')"
fi

retrieval_mentions_json='[]'
if ((${#retrieval_mentions[@]} > 0)); then
  retrieval_mentions_json="$(printf '%s\n' "${retrieval_mentions[@]}" | jq -Rsc 'split("\n")[:-1]')"
fi

unexpected_import_packet_refs="$(
  psql "$dsn" -Atqc "
    SELECT COALESCE(string_agg(table_name, ',' ORDER BY table_name), '')
    FROM information_schema.columns
    WHERE table_schema = 'ami'
      AND column_name = 'import_packet_id'
      AND table_name NOT IN ('import_packets', 'memory_items', 'memory_raw_events')
  "
)"

unexpected_source_project_tables="$(
  psql "$dsn" -Atqc "
    SELECT COALESCE(string_agg(table_name, ',' ORDER BY table_name), '')
    FROM information_schema.columns
    WHERE table_schema = 'ami'
      AND column_name = 'source_project_id'
      AND table_name NOT IN (
        'import_packets',
        'project_relations',
        'project_links',
        'shared_assets',
        'memory_items',
        'memory_raw_events'
      )
  "
)"

memory_items_trigger_present="$(
  psql "$dsn" -Atqc "
    SELECT EXISTS (
      SELECT 1
      FROM pg_trigger
      WHERE tgrelid = 'ami.memory_items'::regclass
        AND tgname = 'trg_ami_memory_items_enforce_import_packet'
        AND NOT tgisinternal
    )::text
  "
)"

memory_items_constraint_present="$(
  psql "$dsn" -Atqc "
    SELECT EXISTS (
      SELECT 1
      FROM pg_constraint
      WHERE conrelid = 'ami.memory_items'::regclass
        AND conname = 'memory_items_cross_project_import_pair_check'
    )::text
  "
)"

trigger_function_text="$(
  psql "$dsn" -Atqc "
    SELECT pg_get_functiondef('ami.enforce_memory_item_import_packet()'::regprocedure)
  "
)"

has_local_scope_clause="false"
has_borrowed_imported_clause="false"
has_verified_copy_scope_clause="false"
if [[ "$trigger_function_text" == *"local memory item visibility_scope"* ]]; then
  has_local_scope_clause="true"
fi
if [[ "$trigger_function_text" == *"borrowed/unverified memory item must keep imported visibility_scope"* ]]; then
  has_borrowed_imported_clause="true"
fi
if [[ "$trigger_function_text" == *"verified local copy memory item visibility_scope"* ]]; then
  has_verified_copy_scope_clause="true"
fi

payload="$(
  jq -n \
    --arg repo_root "$repo_root" \
    --argjson direct_insert_hits "$direct_insert_json" \
    --argjson retrieval_mentions "$retrieval_mentions_json" \
    --arg unexpected_import_packet_refs "$unexpected_import_packet_refs" \
    --arg unexpected_source_project_tables "$unexpected_source_project_tables" \
    --arg memory_items_trigger_present "$memory_items_trigger_present" \
    --arg memory_items_constraint_present "$memory_items_constraint_present" \
    --arg has_local_scope_clause "$has_local_scope_clause" \
    --arg has_borrowed_imported_clause "$has_borrowed_imported_clause" \
    --arg has_verified_copy_scope_clause "$has_verified_copy_scope_clause" \
    '
    {
      artifact_version: "scope-identity-surface-guard-v1",
      repo_root: $repo_root,
      direct_memory_item_insert_hits: $direct_insert_hits,
      retrieval_memory_items_mentions: $retrieval_mentions,
      unexpected_import_packet_ref_tables: (
        if ($unexpected_import_packet_refs | length) == 0 then [] else ($unexpected_import_packet_refs | split(",")) end
      ),
      unexpected_source_project_tables: (
        if ($unexpected_source_project_tables | length) == 0 then [] else ($unexpected_source_project_tables | split(",")) end
      ),
      memory_items_trigger_present: ($memory_items_trigger_present == "true"),
      memory_items_constraint_present: ($memory_items_constraint_present == "true"),
      trigger_has_local_scope_clause: ($has_local_scope_clause == "true"),
      trigger_has_borrowed_imported_clause: ($has_borrowed_imported_clause == "true"),
      trigger_has_verified_copy_scope_clause: ($has_verified_copy_scope_clause == "true")
    }
    | .status = (
        if (.direct_memory_item_insert_hits | length) > 0
          or (.retrieval_memory_items_mentions | length) > 0
          or (.unexpected_import_packet_ref_tables | length) > 0
          or (.unexpected_source_project_tables | length) > 0
          or (.memory_items_trigger_present | not)
          or (.memory_items_constraint_present | not)
          or (.trigger_has_local_scope_clause | not)
          or (.trigger_has_borrowed_imported_clause | not)
          or (.trigger_has_verified_copy_scope_clause | not)
        then "fail"
        else "pass"
        end
      )
    '
)"

if [[ "$json_mode" -eq 1 ]]; then
  printf '%s\n' "$payload"
else
  printf '%s\n' "$payload" | jq .
fi

if [[ "$(printf '%s\n' "$payload" | jq -r '.status')" != "pass" ]]; then
  exit 1
fi
