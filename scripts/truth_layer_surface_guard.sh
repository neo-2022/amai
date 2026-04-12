#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

dsn="$(grep '^AMI_POSTGRES_DSN=' .env | cut -d= -f2-)"
if [[ -z "${dsn}" ]]; then
  echo '{"ok":false,"reason":"missing_ami_postgres_dsn"}'
  exit 1
fi

required_entities=(
  workspace
  project
  project_link
  memory_item
  memory_edge
  memory_conflict
  memory_provenance
  skill_card
  policy_rule
  retrieval_trace
  restore_pack
  import_packet
  quarantine_item
)

required_surfaces=(
  workspaces
  projects
  project_links
  memory_items
  memory_edges
  memory_conflicts
  memory_provenance
  skill_cards
  policy_rules
  retrieval_traces
  restore_packs
  import_packets
  quarantine_items
  truth_layer_surface_registry
)

missing_surfaces_json="$(
  python3 - "$dsn" "${required_surfaces[@]}" <<'PY'
import json, subprocess, sys
dsn = sys.argv[1]
surfaces = sys.argv[2:]
missing = []
for surface in surfaces:
    query = f"SELECT to_regclass('ami.{surface}') IS NOT NULL"
    result = subprocess.run(
        ["psql", dsn, "-Atqc", query],
        check=True,
        capture_output=True,
        text=True,
    )
    if result.stdout.strip() != "t":
        missing.append(surface)
print(json.dumps(missing))
PY
)"

registry_payload="$(
  psql "$dsn" -Atqc "
    SELECT json_agg(row_to_json(t) ORDER BY truth_entity_code)::text
    FROM (
      SELECT truth_entity_code, canonical_surface_name, canonical_surface_kind, surface_role, notes
      FROM ami.truth_layer_surface_registry
    ) t
  "
)"

python3 - "$missing_surfaces_json" "$registry_payload" "${required_entities[@]}" <<'PY'
import json
import sys

missing_surfaces = json.loads(sys.argv[1])
registry = json.loads(sys.argv[2] or "[]")
required_entities = sys.argv[3:]
registry_by_entity = {entry["truth_entity_code"]: entry for entry in registry}
missing_entities = [entity for entity in required_entities if entity not in registry_by_entity]

ok = not missing_surfaces and not missing_entities
print(json.dumps({
    "artifact_version": "truth-layer-surface-guard-v1",
    "ok": ok,
    "missing_surfaces": missing_surfaces,
    "missing_registry_entities": missing_entities,
    "registry": registry,
}, ensure_ascii=False, indent=2))
if not ok:
    raise SystemExit(1)
PY
