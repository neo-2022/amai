#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

repo_root="$(pwd)"
project_code="proof_token_reclassify_$(date +%s%N)"
project_root="/tmp/${project_code}"
bootstrap_file="/tmp/${project_code}.md"
rewritten_source_kind="proof_token_reclassify_context_pack"

cleanup() {
  rm -rf "${project_root}" "${bootstrap_file}"
}
trap cleanup EXIT

mkdir -p "${project_root}"
cat >"${project_root}/README.md" <<'EOF'
# Proof Token Ledger Reclassify
EOF

cat >"${bootstrap_file}" <<'EOF'
# Token ledger reclassify seed

- repair path proof seed
EOF

./target/release/amai bootstrap stack >/dev/null
./target/release/amai continuity import \
  --project "${project_code}" \
  --display-name "Proof Token Ledger Reclassify" \
  --repo-root "${project_root}" \
  --namespace continuity \
  --bootstrap-file "${bootstrap_file}" \
  --transcript-limit 0 >/dev/null

./target/release/amai context pack \
  --project "${project_code}" \
  --namespace continuity \
  --query "token ledger reclassify proof" \
  --retrieval-mode local_strict \
  --disable-cache >/dev/null

before_row="$(
  psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<SQL
SELECT
  payload->'token_budget_event'->>'source_kind' AS source_kind,
  payload->'token_budget_event'->>'traffic_class' AS traffic_class
FROM ami.observability_snapshots
WHERE snapshot_kind = 'token_budget_event'
  AND payload->'_observability'->>'scope_project_code' = '${project_code}'
ORDER BY created_at DESC
LIMIT 1;
SQL
)"

IFS=$'\t' read -r before_source_kind before_traffic_class <<<"${before_row}"
test "${before_source_kind}" = "live_context_pack"
test "${before_traffic_class}" = "live"

./target/release/amai observe repair-token-ledger \
  --apply \
  --project "${project_code}" \
  --namespace continuity \
  --source-kind live_context_pack \
  --rewrite-source-kind "${rewritten_source_kind}" \
  --repair-reason proof_token_ledger_reclassify >/dev/null

after_row="$(
  psql "$AMI_POSTGRES_DSN" -At -F $'\t' <<SQL
SELECT
  payload->'token_budget_event'->>'source_kind' AS source_kind,
  payload->'token_budget_event'->>'traffic_class' AS traffic_class,
  payload->'token_budget_event'->'usage_state'->>'excluded_reason_code' AS excluded_reason_code,
  payload->'_observability'->>'source_kind' AS observability_source_kind,
  payload->'token_budget_event'->'repair'->'operator_source_kind_rewrite'->>'repair_reason' AS repair_reason
FROM ami.observability_snapshots
WHERE snapshot_kind = 'token_budget_event'
  AND payload->'_observability'->>'scope_project_code' = '${project_code}'
ORDER BY created_at DESC
LIMIT 1;
SQL
)"

IFS=$'\t' read -r after_source_kind after_traffic_class after_excluded after_observability_source after_reason <<<"${after_row}"
test "${after_source_kind}" = "${rewritten_source_kind}"
test "${after_traffic_class}" = "proof"
test "${after_excluded}" = "non_live_other"
test "${after_observability_source}" = "${rewritten_source_kind}"
test "${after_reason}" = "proof_token_ledger_reclassify"

printf 'proof_token_ledger_reclassify: PASS\n'
