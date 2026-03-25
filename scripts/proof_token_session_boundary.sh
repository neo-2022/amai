#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

repo_root="$(pwd)"
project_code="proof_session_boundary_$(date +%s%N)"
project_root="/tmp/${project_code}"
bootstrap_file="/tmp/${project_code}.md"

cleanup() {
  rm -rf "${project_root}" "${bootstrap_file}"
}
trap cleanup EXIT

mkdir -p "${project_root}"
cat >"${project_root}/README.md" <<'EOF'
# Proof Session Boundary
EOF

cat >"${bootstrap_file}" <<'EOF'
# Session boundary seed

- continuity startup proof seed
EOF

./target/release/amai bootstrap stack >/dev/null
./target/release/amai continuity import \
  --project "${project_code}" \
  --display-name "Proof Session Boundary" \
  --repo-root "${project_root}" \
  --namespace continuity \
  --bootstrap-file "${bootstrap_file}" \
  --transcript-limit 0 >/dev/null

./target/release/amai continuity startup \
  --project "${project_code}" \
  --namespace continuity \
  --json >/dev/null

./target/release/amai continuity startup \
  --project "${project_code}" \
  --namespace continuity \
  --json >/dev/null

./target/release/amai context pack \
  --project "${project_code}" \
  --namespace continuity \
  --query "session boundary proof" \
  --retrieval-mode local_strict \
  --disable-cache >/dev/null

row="$(
  psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<SQL
WITH startup_events AS (
  SELECT
    snapshot_id,
    payload->'token_budget_event'->>'session_id' AS session_id,
    (payload->'token_budget_event'->>'timestamp_utc')::bigint AS timestamp_utc
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'token_budget_event'
    AND payload->'_observability'->>'scope_project_code' = '${project_code}'
    AND payload->'token_budget_event'->>'source_kind' = 'live_continuity_startup'
  ORDER BY timestamp_utc DESC
  LIMIT 2
),
latest_context AS (
  SELECT
    payload->'token_budget_event'->>'session_id' AS session_id
  FROM ami.observability_snapshots
  WHERE snapshot_kind = 'token_budget_event'
    AND payload->'_observability'->>'scope_project_code' = '${project_code}'
    AND payload->'token_budget_event'->>'source_kind' = 'live_context_pack'
  ORDER BY (payload->'token_budget_event'->>'timestamp_utc')::bigint DESC
  LIMIT 1
)
SELECT
  MAX(CASE WHEN rn = 1 THEN session_id END) AS latest_startup_session,
  MAX(CASE WHEN rn = 2 THEN session_id END) AS previous_startup_session,
  (SELECT session_id FROM latest_context) AS latest_context_session
FROM (
  SELECT session_id, ROW_NUMBER() OVER (ORDER BY timestamp_utc DESC) AS rn
  FROM startup_events
) ranked;
SQL
)"

IFS=$'\t' read -r latest_startup_session previous_startup_session latest_context_session <<<"${row}"

test -n "${latest_startup_session}"
test -n "${previous_startup_session}"
test -n "${latest_context_session}"
test "${latest_startup_session}" != "${previous_startup_session}"
test "${latest_context_session}" = "${latest_startup_session}"

printf 'proof_token_session_boundary: PASS\n'
