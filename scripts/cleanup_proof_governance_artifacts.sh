#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--help" ]]; then
  cat <<'EOF'
Cleanup proof-only governance artifacts from local Postgres.

Usage:
  scripts/cleanup_proof_governance_artifacts.sh [--apply]

Dry-run by default. Use --apply to update rows.
EOF
  exit 0
fi

apply=false
if [[ "${1:-}" == "--apply" ]]; then
  apply=true
fi

if [[ ! -f ".env" ]]; then
  echo "missing .env" >&2
  exit 1
fi

set -a
source .env
set +a

if [[ -z "${AMI_POSTGRES_DSN:-}" ]]; then
  echo "AMI_POSTGRES_DSN is empty" >&2
  exit 1
fi

echo "== quarantine_items candidates (proof/smoke only) =="
psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<'SQL'
SELECT quarantine_item_id,
       entity_kind,
       entity_id,
       quarantine_state,
       quarantine_reason,
       COALESCE(source_kind, '') AS source_kind,
       COALESCE(evidence->>'proof', '') AS proof_tag,
       COALESCE(evidence_span->>'surface', '') AS surface_tag
FROM ami.quarantine_items
WHERE quarantine_state = 'active'
  AND (
    quarantine_reason LIKE 'proof quarantine %'
    OR quarantine_reason LIKE 'smoke quarantine %'
    OR evidence->>'proof' LIKE 'stage%'
    OR evidence_span->>'surface' = 'runtime-test'
  )
ORDER BY quarantined_at_epoch_ms DESC NULLS LAST;
SQL

echo "== memory_conflicts candidates (proof/runtime) =="
psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<'SQL'
SELECT memory_conflict_id,
       conflict_kind,
       conflict_state,
       COALESCE(summary, '') AS summary,
       COALESCE(source_kind, '') AS source_kind,
       COALESCE(evidence->>'proof', '') AS proof_tag
FROM ami.memory_conflicts
WHERE conflict_state = 'open'
  AND (
    (summary = 'truth conflict detected' AND source_kind = 'verification_conflict_runtime')
    OR (summary LIKE 'cli conflict summary %' AND source_kind = 'runtime_cli')
    OR (summary LIKE 'cli get conflict %' AND source_kind = 'runtime_cli')
    OR evidence->>'proof' LIKE 'cli-get-conflict%'
  )
ORDER BY detected_at_epoch_ms DESC NULLS LAST;
SQL

if [[ "${apply}" != true ]]; then
  echo "dry-run only. Re-run with --apply to update states."
  exit 0
fi

now_ms="$(date +%s)000"

echo "== applying cleanup =="
psql "$AMI_POSTGRES_DSN" -v ON_ERROR_STOP=1 <<SQL
UPDATE ami.quarantine_items
SET quarantine_state = 'released',
    released_at_epoch_ms = COALESCE(released_at_epoch_ms, ${now_ms})
WHERE quarantine_state = 'active'
  AND (
    quarantine_reason LIKE 'proof quarantine %'
    OR quarantine_reason LIKE 'smoke quarantine %'
    OR evidence->>'proof' LIKE 'stage%'
    OR evidence_span->>'surface' = 'runtime-test'
  );

UPDATE ami.memory_conflicts
SET conflict_state = 'resolved',
    resolved_at_epoch_ms = COALESCE(resolved_at_epoch_ms, ${now_ms})
WHERE conflict_state = 'open'
  AND (
    (summary = 'truth conflict detected' AND source_kind = 'verification_conflict_runtime')
    OR (summary LIKE 'cli conflict summary %' AND source_kind = 'runtime_cli')
    OR (summary LIKE 'cli get conflict %' AND source_kind = 'runtime_cli')
    OR evidence->>'proof' LIKE 'cli-get-conflict%'
  );
SQL

echo "cleanup done."
