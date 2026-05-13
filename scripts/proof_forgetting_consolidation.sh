#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

step() {
  echo "[proof_forgetting_consolidation] $*"
}

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

project_code="amai"
suffix="$(amai_unique_suffix)"
namespace_code="proof-forgetting-${suffix}"
dsn="$(grep '^AMI_POSTGRES_DSN=' "$(dirname "$0")/../.env" | cut -d= -f2-)"

step "bootstrap stack"
./scripts/bootstrap_stack.sh >/dev/null

step "ensure namespace ${namespace_code}"
cargo run --quiet -- namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" >/dev/null

# ────────────────────────────────────────────────────────────────────────
# 1. Create test items with diverse lifecycle characteristics
# ────────────────────────────────────────────────────────────────────────
step "create ephemeral item with expired TTL"
ephemeral_expired="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Ephemeral expired fact" \
  --summary "This should be pruned" \
  --derivation-kind "summary" \
  --retention-class "ephemeral" \
  --ttl-epoch-ms 1000 \
  --utility-score 0.01 \
  --freshness-score 0.01 \
  --source-event-id "proof-forgetting-ephemeral-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/ephemeral" \
  --json)"
ephemeral_expired_id="$(printf '%s' "${ephemeral_expired}" | jq -r '.memory_item_id')"
step "ephemeral expired item: ${ephemeral_expired_id}"

step "create durable raw_capture item (must NOT be pruned)"
durable_raw="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Immutable raw evidence" \
  --summary "Raw evidence that must survive any pruning" \
  --derivation-kind "raw_capture" \
  --retention-class "durable" \
  --utility-score 0.5 \
  --freshness-score 0.5 \
  --source-event-id "proof-forgetting-durable-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/durable" \
  --json)"
durable_raw_id="$(printf '%s' "${durable_raw}" | jq -r '.memory_item_id')"
step "durable raw item: ${durable_raw_id}"

step "create standard summary item with low freshness (archivable)"
standard_stale="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Stale standard summary" \
  --summary "Should be archived to cold tier" \
  --derivation-kind "summary" \
  --retention-class "standard" \
  --utility-score 0.02 \
  --freshness-score 0.01 \
  --source-event-id "proof-forgetting-stale-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/stale" \
  --json)"
standard_stale_id="$(printf '%s' "${standard_stale}" | jq -r '.memory_item_id')"
step "standard stale item: ${standard_stale_id}"

step "create operator_write item with low freshness (must NOT be pruned or archived)"
operator_write="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "policy" \
  --title "Operator immutable policy" \
  --summary "Operator-written policy must survive all forgetting" \
  --derivation-kind "operator_write" \
  --retention-class "standard" \
  --utility-score 0.01 \
  --freshness-score 0.001 \
  --json)"
operator_write_id="$(printf '%s' "${operator_write}" | jq -r '.memory_item_id')"
step "operator_write item: ${operator_write_id}"

step "create legal_hold item with low scores (must NOT be pruned)"
legal_hold="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Legal hold evidence" \
  --summary "Under legal hold - never touch" \
  --derivation-kind "summary" \
  --retention-class "legal_hold" \
  --utility-score 0.001 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-legal-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/legal" \
  --json)"
legal_hold_id="$(printf '%s' "${legal_hold}" | jq -r '.memory_item_id')"
step "legal_hold item: ${legal_hold_id}"

step "create revalidation-target item (standard, low freshness, truth_state=current)"
reval_target="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Stale knowledge needing revalidation" \
  --summary "This fact has very low freshness and needs review" \
  --derivation-kind "extract" \
  --retention-class "standard" \
  --utility-score 0.3 \
  --freshness-score 0.02 \
  --source-event-id "proof-forgetting-reval-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/reval" \
  --json)"
reval_target_id="$(printf '%s' "${reval_target}" | jq -r '.memory_item_id')"
step "revalidation target item: ${reval_target_id}"

step "create ephemeral retain_forever item with low utility (must NOT be pruned by low-utility path)"
ephemeral_retain="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --title "Ephemeral retain forever" \
  --summary "Protected from low-utility prune by retain_forever" \
  --derivation-kind "summary" \
  --retention-class "ephemeral" \
  --utility-score 0.001 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-ephemeral-retain-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/ephemeral-retain" \
  --json)"
ephemeral_retain_id="$(printf '%s' "${ephemeral_retain}" | jq -r '.memory_item_id')"
step "ephemeral retain_forever item: ${ephemeral_retain_id}"

step "create verified_write_back item with low freshness (must NOT be revalidated/pruned/compacted)"
verified_writeback="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --identity-key "proof-verified-writeback-${suffix}" \
  --title "Verified writeback evidence" \
  --summary "Verified writeback must survive automated forgetting" \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "proof-forgetting-writeback-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/writeback/${suffix}" \
  --message-ref "message:proof-forgetting-writeback:${suffix}" \
  --evidence-span-json '{"source":"proof","kind":"raw_log","range":"1-2"}' \
  --derivation-kind verified_write_back \
  --utility-score 0.01 \
  --freshness-score 0.001 \
  --observed-at-epoch-ms 4000 \
  --recorded-at-epoch-ms 4001 \
  --valid-from-epoch-ms 4000 \
  --last-verified-at-epoch-ms 4002 \
  --metadata-json '{"writeback_evidence":{"escalated":true,"verified":true,"confirmed_via":"raw_evidence"}}' \
  --json)"
verified_writeback_id="$(printf '%s' "${verified_writeback}" | jq -r '.memory_item_id')"
step "verified_write_back item: ${verified_writeback_id}"

step "create retain_forever duplicate pair with low freshness (must NOT be revalidated/archived/compacted)"
retain_forever_a="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --identity-key "proof-retain-forever-${suffix}" \
  --title "Retain forever duplicate" \
  --summary "Retain forever duplicate A" \
  --derivation-kind "summary" \
  --retention-class "standard" \
  --truth-state current \
  --utility-score 0.02 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-retain-a-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/retain/a" \
  --json)"
retain_forever_a_id="$(printf '%s' "${retain_forever_a}" | jq -r '.memory_item_id')"

retain_forever_b="$(cargo run --quiet -- memory create-item \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --item-kind "fact" \
  --identity-key "proof-retain-forever-b-${suffix}" \
  --title "Retain forever duplicate B seed" \
  --summary "Retain forever duplicate B" \
  --derivation-kind "summary" \
  --retention-class "standard" \
  --truth-state current \
  --utility-score 0.01 \
  --freshness-score 0.001 \
  --source-event-id "proof-forgetting-retain-b-${suffix}" \
  --artifact-ref "artifact://proof/forgetting/retain/b" \
  --json)"
retain_forever_b_id="$(printf '%s' "${retain_forever_b}" | jq -r '.memory_item_id')"
step "retain_forever items: ${retain_forever_a_id} ${retain_forever_b_id}"

# ────────────────────────────────────────────────────────────────────────
# 1b. Simulate verified fact for revalidation target
# ────────────────────────────────────────────────────────────────────────
step "set truth_state=current on revalidation target (simulate verified fact)"
psql "${dsn}" -qc "UPDATE ami.memory_items SET truth_state='current' WHERE memory_item_id='${reval_target_id}'"

step "set decay_policy=retain_forever on duplicate pair"
psql "${dsn}" -qc "UPDATE ami.memory_items SET decay_policy='retain_forever' WHERE memory_item_id IN ('${retain_forever_a_id}', '${retain_forever_b_id}')"

step "set decay_policy=retain_forever on ephemeral protected item"
psql "${dsn}" -qc "UPDATE ami.memory_items SET decay_policy='retain_forever' WHERE memory_item_id='${ephemeral_retain_id}'"

step "reshape retain_forever pair into duplicate identity/title fixture for dedup hostile check"
psql "${dsn}" -qc "
  UPDATE ami.memory_items
  SET identity_key='proof-retain-forever-${suffix}',
      title='Retain forever duplicate'
  WHERE memory_item_id IN ('${retain_forever_a_id}', '${retain_forever_b_id}')
"

# ────────────────────────────────────────────────────────────────────────
# 2. Touch access and verify counter
# ────────────────────────────────────────────────────────────────────────
step "touch access on durable raw item"
cargo run --quiet -- memory touch-access --memory-item-id "${durable_raw_id}"

step "verify access_count incremented"
access_count="$(psql "${dsn}" -tA -c "SELECT access_count FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${access_count}" -ge 1 ]] || fail "access_count was not incremented: ${access_count}"
step "access_count = ${access_count} (pass)"

step "verify last_accessed_at is set"
last_accessed="$(psql "${dsn}" -tA -c "SELECT last_accessed_at IS NOT NULL FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${last_accessed}" == "t" ]] || fail "last_accessed_at not set after touch"
step "last_accessed_at set (pass)"

# ────────────────────────────────────────────────────────────────────────
# 3. Revalidation (BEFORE consolidation so items are still active)
# ────────────────────────────────────────────────────────────────────────
step "run revalidate on items with freshness < 0.05"
revalidate_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind revalidation_job \
  --freshness-threshold 0.05)"
revalidate_job_kind="$(printf '%s' "${revalidate_output}" | jq -r '.job_kind')"
[[ "${revalidate_job_kind}" == "revalidation_job" ]] || fail "unexpected revalidation job kind: ${revalidate_job_kind}"
revalidated_count="$(printf '%s' "${revalidate_output}" | jq '.action_count')"
step "revalidated ${revalidated_count} items"

step "verify revalidation target is now pending_review"
reval_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${reval_target_id}'")"
[[ "${reval_status}" == "pending_review" ]] || fail "revalidation target should be pending_review, got: ${reval_status}"
step "revalidation target pending_review (pass)"

step "verify operator_write NOT revalidated (immune)"
op_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${operator_write_id}'")"
[[ "${op_status}" == "active" ]] || fail "operator_write should remain active, got: ${op_status}"
step "operator_write immune to revalidation (pass)"

step "verify verified_write_back NOT revalidated (immune)"
writeback_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${verified_writeback_id}'")"
[[ "${writeback_status}" == "active" ]] || fail "verified_write_back should remain active, got: ${writeback_status}"
step "verified_write_back immune to revalidation (pass)"

step "verify retain_forever pair NOT revalidated (immune)"
retain_reval_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status != 'active'")"
[[ "${retain_reval_count}" == "0" ]] || fail "retain_forever items should remain active after revalidation, got non-active count: ${retain_reval_count}"
step "retain_forever immune to revalidation (pass)"

# ────────────────────────────────────────────────────────────────────────
# 4. Explainability: verify audit log populated from revalidation
# ────────────────────────────────────────────────────────────────────────
step "verify explainability: audit log has revalidation reason"
explain_output="$(cargo run --quiet -- memory explain-forgetting --memory-item-id "${reval_target_id}")"
explain_count="$(printf '%s' "${explain_output}" | jq 'length')"
[[ "${explain_count}" -ge 1 ]] || fail "no audit log entries for revalidated item"
explain_reason="$(printf '%s' "${explain_output}" | jq -r '.[0].reason')"
[[ "${explain_reason}" == *"freshness"* ]] || fail "audit reason should mention freshness, got: ${explain_reason}"
step "explainability: reason='${explain_reason}' (pass)"

# ────────────────────────────────────────────────────────────────────────
# 5. Prune expired items
# ────────────────────────────────────────────────────────────────────────
step "run prune (should prune ephemeral expired item)"
now_epoch_ms="$(date +%s%3N)"
prune_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind pruning_job \
  --now-epoch-ms "${now_epoch_ms}" \
  --utility-threshold 0.05)"
prune_job_kind="$(printf '%s' "${prune_output}" | jq -r '.job_kind')"
[[ "${prune_job_kind}" == "pruning_job" ]] || fail "unexpected pruning job kind: ${prune_job_kind}"
pruned_count="$(printf '%s' "${prune_output}" | jq '.action_count')"
step "pruned ${pruned_count} items"
[[ "${pruned_count}" -ge 1 ]] || fail "expected at least 1 pruned item"

step "verify ephemeral item is now pruned"
consolidation_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${ephemeral_expired_id}'")"
[[ "${consolidation_status}" == "pruned" ]] || fail "ephemeral item consolidation_status should be pruned, got: ${consolidation_status}"
step "ephemeral item pruned (pass)"

# ────────────────────────────────────────────────────────────────────────
# 6. Hostile negative path: immutable items survive aggressive pruning
# ────────────────────────────────────────────────────────────────────────
step "HOSTILE: verify durable raw_capture item NOT pruned"
durable_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${durable_status}" == "active" ]] || fail "durable raw_capture must remain active, got: ${durable_status}"
step "durable raw_capture protected (pass)"

step "HOSTILE: verify operator_write item NOT pruned"
op_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${operator_write_id}'")"
[[ "${op_status2}" == "active" ]] || fail "operator_write must remain active, got: ${op_status2}"
step "operator_write protected (pass)"

step "HOSTILE: verify legal_hold item NOT pruned"
legal_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${legal_hold_id}'")"
[[ "${legal_status}" == "active" ]] || fail "legal_hold must remain active, got: ${legal_status}"
step "legal_hold protected (pass)"

step "HOSTILE: verify verified_write_back item NOT pruned"
writeback_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${verified_writeback_id}'")"
[[ "${writeback_status2}" == "active" ]] || fail "verified_write_back must remain active, got: ${writeback_status2}"
step "verified_write_back protected (pass)"

step "HOSTILE: verify retain_forever items NOT pruned"
retain_prune_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status != 'active'")"
[[ "${retain_prune_count}" == "0" ]] || fail "retain_forever items must remain active after prune, got non-active count: ${retain_prune_count}"
step "retain_forever protected from prune (pass)"

step "HOSTILE: verify low-utility ephemeral retain_forever item NOT pruned"
ephemeral_retain_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${ephemeral_retain_id}'")"
[[ "${ephemeral_retain_status}" == "active" ]] || fail "ephemeral retain_forever item must remain active after low-utility prune, got: ${ephemeral_retain_status}"
step "ephemeral retain_forever protected from low-utility prune (pass)"

# ────────────────────────────────────────────────────────────────────────
# 7. Explainability: pruned item has audit trail
# ────────────────────────────────────────────────────────────────────────
step "verify explainability: pruned item has audit log"
prune_explain="$(cargo run --quiet -- memory explain-forgetting --memory-item-id "${ephemeral_expired_id}")"
prune_explain_count="$(printf '%s' "${prune_explain}" | jq 'length')"
[[ "${prune_explain_count}" -ge 1 ]] || fail "no audit log for pruned item"
prune_action="$(printf '%s' "${prune_explain}" | jq -r '.[0].action')"
[[ "${prune_action}" == "prune_ttl_expired" || "${prune_action}" == "prune_low_utility" ]] || fail "unexpected prune action: ${prune_action}"
step "pruned item explainable: action=${prune_action} (pass)"

# ────────────────────────────────────────────────────────────────────────
# 8. Archive cold tier
# ────────────────────────────────────────────────────────────────────────
step "run cold_archive_job with stale-days=0 (aggressive: should archive stale standard item)"
archive_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind cold_archive_job \
  --stale-days 0)"
archive_job_kind="$(printf '%s' "${archive_output}" | jq -r '.job_kind')"
[[ "${archive_job_kind}" == "cold_archive_job" ]] || fail "unexpected archive job kind: ${archive_job_kind}"
archived_count="$(printf '%s' "${archive_output}" | jq '.action_count')"
step "archived ${archived_count} items"

step "verify standard stale item is now archived"
stale_status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${standard_stale_id}'")"
[[ "${stale_status}" == "archived" ]] || fail "stale standard item should be archived, got: ${stale_status}"
step "stale standard item archived (pass)"

step "verify archived item retention_class changed to archive"
archive_rc="$(psql "${dsn}" -tA -c "SELECT retention_class FROM ami.memory_items WHERE memory_item_id = '${standard_stale_id}'")"
[[ "${archive_rc}" == "archive" ]] || fail "archived item retention_class should be 'archive', got: ${archive_rc}"
step "retention_class=archive (pass)"

# ────────────────────────────────────────────────────────────────────────
# 9. Hostile: immutable items survive archival
# ────────────────────────────────────────────────────────────────────────
step "HOSTILE: verify durable raw_capture NOT archived even with stale-days=0"
durable_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${durable_raw_id}'")"
[[ "${durable_status2}" == "active" ]] || fail "durable raw_capture must remain active after archive, got: ${durable_status2}"
step "durable raw_capture survived archival (pass)"

step "HOSTILE: verify legal_hold NOT archived even with stale-days=0"
legal_status2="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${legal_hold_id}'")"
[[ "${legal_status2}" == "active" ]] || fail "legal_hold must remain active after archive, got: ${legal_status2}"
step "legal_hold survived archival (pass)"

step "HOSTILE: verify verified_write_back NOT archived"
writeback_status3="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${verified_writeback_id}'")"
[[ "${writeback_status3}" == "active" ]] || fail "verified_write_back must remain active after archive, got: ${writeback_status3}"
step "verified_write_back survived archival (pass)"

step "HOSTILE: verify retain_forever pair NOT archived"
retain_archive_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status != 'active'")"
[[ "${retain_archive_count}" == "0" ]] || fail "retain_forever items must remain active after archive, got non-active count: ${retain_archive_count}"
step "retain_forever survived archival (pass)"

# ────────────────────────────────────────────────────────────────────────
# 10. Full consolidation run + immutable count
# ────────────────────────────────────────────────────────────────────────
step "run full consolidation"
consolidation_output="$(cargo run --quiet -- memory consolidate \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --now-epoch-ms "${now_epoch_ms}")"
immutable_protected="$(printf '%s' "${consolidation_output}" | jq '.immutable_protected_count')"
step "immutable protected items: ${immutable_protected}"
[[ "${immutable_protected}" -ge 7 ]] || fail "expected at least 7 immutable protected items (durable+operator_write+legal_hold+verified_write_back+retain_forever pair+ephemeral retain_forever), got: ${immutable_protected}"

step "verify consolidation report contract version"
contract_version="$(printf '%s' "${consolidation_output}" | jq -r '.contract_version')"
[[ "${contract_version}" == "forgetting-consolidation-v1" ]] || fail "unexpected contract version: ${contract_version}"
step "contract version verified (pass)"

step "verify safety invariant declared in report"
safety_inv="$(printf '%s' "${consolidation_output}" | jq -r '.safety_invariant')"
[[ "${safety_inv}" == *"raw_capture"* ]] || fail "safety invariant must mention raw_capture"
[[ "${safety_inv}" == *"operator_write"* ]] || fail "safety invariant must mention operator_write"
[[ "${safety_inv}" == *"verified_write_back"* ]] || fail "safety invariant must mention verified_write_back"
[[ "${safety_inv}" == *"durable"* ]] || fail "safety invariant must mention durable"
[[ "${safety_inv}" == *"legal_hold"* ]] || fail "safety invariant must mention legal_hold"
[[ "${safety_inv}" == *"retain_forever"* ]] || fail "safety invariant must mention retain_forever"
step "safety invariant complete (pass)"

step "verify retain_forever duplicate pair NOT compacted by dedup"
retain_compacted_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.memory_items WHERE memory_item_id IN ('${retain_forever_a_id}','${retain_forever_b_id}') AND consolidation_status = 'compacted'")"
[[ "${retain_compacted_count}" == "0" ]] || fail "retain_forever duplicate pair must not be compacted, got compacted count: ${retain_compacted_count}"
step "retain_forever pair immune to dedup (pass)"

# ────────────────────────────────────────────────────────────────────────
# 10b. Named job surface is materialized as runtime contract
# ────────────────────────────────────────────────────────────────────────
step "verify de_duplication_job surface is materialized"
dedup_job_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind de_duplication_job)"
dedup_job_kind="$(printf '%s' "${dedup_job_output}" | jq -r '.job_kind')"
[[ "${dedup_job_kind}" == "de_duplication_job" ]] || fail "unexpected de-dup job kind: ${dedup_job_kind}"
step "de_duplication_job surfaced (pass)"

step "verify compaction_job surface is materialized"
compaction_job_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind compaction_job)"
compaction_job_kind="$(printf '%s' "${compaction_job_output}" | jq -r '.job_kind')"
[[ "${compaction_job_kind}" == "compaction_job" ]] || fail "unexpected compaction job kind: ${compaction_job_kind}"
step "compaction_job surfaced (pass)"

step "verify summarization_job surface is materialized"
summarization_job_output="$(cargo run --quiet -- memory run-job \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --job-kind summarization_job)"
summarization_job_kind="$(printf '%s' "${summarization_job_output}" | jq -r '.job_kind')"
summarization_job_actions="$(printf '%s' "${summarization_job_output}" | jq '.action_count')"
[[ "${summarization_job_kind}" == "summarization_job" ]] || fail "unexpected summarization job kind: ${summarization_job_kind}"
[[ "${summarization_job_actions}" == "0" ]] || fail "summarization_job should currently be explicit no-op, got actions: ${summarization_job_actions}"
step "summarization_job surfaced as explicit no-op (pass)"

# ────────────────────────────────────────────────────────────────────────
# 11. Stale knowledge doesn't resurface: pruned items excluded from live DB query
# ────────────────────────────────────────────────────────────────────────
step "verify stale/pruned items not in active consolidation_status"
active_count="$(psql "${dsn}" -tA -c "
  SELECT COUNT(*) FROM ami.memory_items
  WHERE namespace_id = (
    SELECT namespace_id FROM ami.namespaces WHERE code = '${namespace_code}'
  ) AND consolidation_status = 'active'
")"
total_count="$(psql "${dsn}" -tA -c "
  SELECT COUNT(*) FROM ami.memory_items
  WHERE namespace_id = (
    SELECT namespace_id FROM ami.namespaces WHERE code = '${namespace_code}'
  )
")"
step "active items: ${active_count} / total items: ${total_count}"
[[ "${active_count}" -lt "${total_count}" ]] || fail "expected some items to be non-active after forgetting operations"
step "stale items excluded from active set (pass)"

# ────────────────────────────────────────────────────────────────────────
# 12. End-to-end explainability: every non-active item has audit trail
# ────────────────────────────────────────────────────────────────────────
step "verify all non-active items have audit trail"
non_active_ids="$(psql "${dsn}" -tA -c "
  SELECT memory_item_id FROM ami.memory_items
  WHERE namespace_id = (
    SELECT namespace_id FROM ami.namespaces WHERE code = '${namespace_code}'
  ) AND consolidation_status != 'active'
")"
audit_missing=0
while IFS= read -r mid; do
  [[ -z "${mid}" ]] && continue
  audit_count="$(psql "${dsn}" -tA -c "SELECT COUNT(*) FROM ami.forgetting_audit_log WHERE memory_item_id = '${mid}'")"
  if [[ "${audit_count}" -lt 1 ]]; then
    echo "WARN: no audit trail for non-active item ${mid}" >&2
    audit_missing=$((audit_missing + 1))
  fi
done <<< "${non_active_ids}"
[[ "${audit_missing}" -eq 0 ]] || fail "${audit_missing} non-active items lack audit trail"
step "all non-active items have explainable audit trail (pass)"

step "verify lifecycle transition stats CLI surfaces Queue 2 derived contract"
transition_stats_output="$(cargo run --quiet -- memory transition-stats \
  --project "${project_code}" \
  --namespace "${namespace_code}")"
transition_contract="$(printf '%s' "${transition_stats_output}" | jq -r '.contract_version')"
[[ "${transition_contract}" == "lifecycle-transition-stats-v1" ]] || fail "unexpected transition stats contract: ${transition_contract}"
transition_rows="$(printf '%s' "${transition_stats_output}" | jq '.rows | length')"
[[ "${transition_rows}" -ge 3 ]] || fail "expected lifecycle transition rows, got ${transition_rows}"
for next_state in pruned archived pending_review; do
  transition_state_count="$(printf '%s' "${transition_stats_output}" | jq --arg state "${next_state}" '[.rows[] | select(.next_state == $state)] | length')"
  [[ "${transition_state_count}" -ge 1 ]] || fail "expected transition stats row for next_state=${next_state}, got ${transition_state_count}"
done
compaction_transition_count="$(printf '%s' "${transition_stats_output}" | jq '[.rows[] | select(.next_state == "compacted")] | length')"
compaction_job_actions="$(printf '%s' "${compaction_job_output}" | jq '.action_count')"
if [[ "${compaction_job_actions}" -gt 0 ]]; then
  [[ "${compaction_transition_count}" -ge 1 ]] || fail "expected compacted transition row when compaction_job produced actions"
fi
step "lifecycle transition stats CLI surfaces pruned/archived/pending_review and conditionally compacted rows (pass)"

step "verify lifecycle cohort-risk CLI surfaces Queue 3 advisory contract"
cohort_risk_output="$(cargo run --quiet -- memory cohort-risk \
  --project "${project_code}" \
  --namespace "${namespace_code}")"
cohort_risk_contract="$(printf '%s' "${cohort_risk_output}" | jq -r '.contract_version')"
[[ "${cohort_risk_contract}" == "lifecycle-cohort-risk-v1" ]] || fail "unexpected cohort risk contract: ${cohort_risk_contract}"
cohort_risk_rows="$(printf '%s' "${cohort_risk_output}" | jq '.rows | length')"
[[ "${cohort_risk_rows}" -ge 1 ]] || fail "expected lifecycle cohort risk rows, got ${cohort_risk_rows}"
cohort_risk_invalid_states="$(printf '%s' "${cohort_risk_output}" | jq '[.rows[] | select((.expected_next_state | IN("active_hot","active_stale","pending_review","compacted","archived","pruned","protected","quarantined")) | not)] | length')"
[[ "${cohort_risk_invalid_states}" -eq 0 ]] || fail "cohort risk surfaced invalid expected_next_state"
cohort_risk_missing_summary="$(printf '%s' "${cohort_risk_output}" | jq '[.rows[] | select((.cohort_reason_summary | type != "string") or (.cohort_reason_summary == ""))] | length')"
[[ "${cohort_risk_missing_summary}" -eq 0 ]] || fail "cohort risk rows missing cohort_reason_summary"
cohort_risk_nonzero="$(printf '%s' "${cohort_risk_output}" | jq '[.rows[] | select(.pending_review_risk_7d > 0 or .archive_risk_30d > 0 or .prune_risk_30d > 0)] | length')"
[[ "${cohort_risk_nonzero}" -ge 1 ]] || fail "expected at least one non-zero lifecycle cohort risk row"
step "lifecycle cohort-risk CLI surfaces Queue 3 advisory contract (pass)"

step "verify lifecycle policy-simulate CLI surfaces Queue 3 approval contour without authority"
policy_simulate_output="$(cargo run --quiet -- memory policy-simulate \
  --project "${project_code}" \
  --namespace "${namespace_code}")"
policy_simulate_contract="$(printf '%s' "${policy_simulate_output}" | jq -r '.contract_version')"
[[ "${policy_simulate_contract}" == "lifecycle-policy-simulate-v1" ]] || fail "unexpected policy simulate contract: ${policy_simulate_contract}"
policy_simulate_authority="$(printf '%s' "${policy_simulate_output}" | jq -r '.authority_mode')"
[[ "${policy_simulate_authority}" == "advisory_only_no_runtime_authority" ]] || fail "policy simulate surfaced unexpected authority mode: ${policy_simulate_authority}"
policy_simulate_rows="$(printf '%s' "${policy_simulate_output}" | jq '.rows | length')"
[[ "${policy_simulate_rows}" -ge 1 ]] || fail "expected lifecycle policy simulation rows, got ${policy_simulate_rows}"
policy_simulate_invalid_actions="$(printf '%s' "${policy_simulate_output}" | jq '[.rows[] | select((.recommended_review_action | IN("hold_current_policy","review_revalidation_queue","review_archive_candidate","review_prune_candidate","observe_only")) | not)] | length')"
[[ "${policy_simulate_invalid_actions}" -eq 0 ]] || fail "policy simulate surfaced invalid recommended_review_action"
policy_simulate_invalid_urgency="$(printf '%s' "${policy_simulate_output}" | jq '[.rows[] | select((.urgency | IN("manual_only","high","medium","low")) | not)] | length')"
[[ "${policy_simulate_invalid_urgency}" -eq 0 ]] || fail "policy simulate surfaced invalid urgency"
policy_simulate_missing_blocker="$(printf '%s' "${policy_simulate_output}" | jq '[.rows[] | select((.blocking_reasons | index("advisory_only_no_runtime_authority")) == null)] | length')"
[[ "${policy_simulate_missing_blocker}" -eq 0 ]] || fail "policy simulate rows lost advisory-only blocker"
step "lifecycle policy-simulate CLI stays advisory-only and surfaces approval contour (pass)"

# ────────────────────────────────────────────────────────────────────────
# 13. Final integrity: immutable items untouched throughout entire proof
# ────────────────────────────────────────────────────────────────────────
step "FINAL: verify all immutable items still active"
for item_label_id in \
  "durable_raw:${durable_raw_id}" \
  "operator_write:${operator_write_id}" \
  "legal_hold:${legal_hold_id}" \
  "verified_write_back:${verified_writeback_id}" \
  "ephemeral_retain_forever:${ephemeral_retain_id}" \
  "retain_forever_a:${retain_forever_a_id}" \
  "retain_forever_b:${retain_forever_b_id}"; do
  label="${item_label_id%%:*}"
  iid="${item_label_id##*:}"
  status="$(psql "${dsn}" -tA -c "SELECT consolidation_status FROM ami.memory_items WHERE memory_item_id = '${iid}'")"
  [[ "${status}" == "active" ]] || fail "FINAL: ${label} item ${iid} should be active, got: ${status}"
done
step "all immutable items survived full proof lifecycle (pass)"

echo ""
echo "==========================================="
echo " forgetting/consolidation proof: ALL PASS"
echo "==========================================="
