#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-freeze-close.json

python3 - <<'PY'
import json
from pathlib import Path

payload = json.loads(Path("/tmp/amai-proof-token-freeze-close.json").read_text())["token_budget_report"]
contract = payload["settlement_contract"]
preview = payload["statement_previews"]["current_session"]
summary = payload["contractual_statement_summaries"]["current_session"]
period = preview["period"]

assert contract["statement_version"] == "settlement-preview-v4", contract
assert contract["freeze_close_policy_version"] == "freeze-close-v2", contract
assert contract["late_arrival_policy_version"] == "late-arrival-v2", contract
assert contract["settlement_lifecycle_model_version"] == "settlement-lifecycle-v4", contract
assert contract["statement_period_governance_version"] == "statement-period-governance-v2", contract
assert contract["freeze_close_status"] == "provisional_report_only", contract
assert contract["late_arrival_status"] == "deadline_from_latest_event_report_only", contract
assert contract["current_materialized_boundary"] == "measured_report_only", contract
assert contract["future_reserved_settlement_stages"] == [
    "billable_reserved",
    "settled_reserved",
    "invoiced_reserved",
    "credited_reserved",
    "disputed_reserved",
    "closed_reserved",
], contract

assert preview["statement_status"] == "report_only_preview", preview
assert preview["close_candidate"] is False, preview
assert preview["settlement_stage"] in {
    "empty_report_only",
    "measured_open_report_only",
    "measured_review_ready_report_only",
    "measured_adjusted_report_only",
    "measured_pending_adjustment_report_only",
    "measured_disputed_report_only",
}, preview
assert preview["settlement_stage_family"] in {"empty", "measured_report_only"}, preview
assert preview["next_settlement_stage_candidate"] in {
    "awaiting_measured_usage",
    "review_ready_blocked",
    "billable_blocked",
    "billable_reserved",
}, preview
assert isinstance(preview["next_settlement_stage_blockers"], list), preview
assert preview["future_reserved_settlement_stages"] == contract["future_reserved_settlement_stages"], (preview, contract)
assert preview["transactional_statuses"]["measured"]["boundary"] == "measured_report_only", preview
assert preview["transactional_statuses"]["billable"]["boundary"] == "reserved_future", preview
assert preview["close_readiness"] in {
    "provisionally_stable_report_only",
    "provisionally_blocked_report_only",
}, preview
assert preview["provisional_close_state"] in {
    "report_only_preview_provisionally_stable",
    "report_only_preview_provisional_hold",
}, preview
assert isinstance(preview["provisional_close_candidate"], bool), preview
assert isinstance(preview["provisional_close_barriers"], list), preview
assert isinstance(preview["billing_close_barriers"], list), preview
assert period["model_version"] == "statement-period-governance-v2", period
assert period["close_at_epoch_ms"] is None, period
assert period["provisional_close_candidate"] == preview["provisional_close_candidate"], (period, preview)
assert period["provisional_close_barriers"] == preview["provisional_close_barriers"], (period, preview)

if preview["provisional_close_candidate"]:
    assert preview["close_readiness"] == "provisionally_stable_report_only", preview
    assert preview["provisional_close_state"] == "report_only_preview_provisionally_stable", preview
else:
    assert preview["close_readiness"] == "provisionally_blocked_report_only", preview
    assert preview["provisional_close_state"] == "report_only_preview_provisional_hold", preview

assert summary["provisional_close_state"] == preview["provisional_close_state"], (summary, preview)
assert summary["provisional_close_candidate"] == preview["provisional_close_candidate"], (summary, preview)
assert summary["settlement_stage"] == preview["settlement_stage"], (summary, preview)
assert summary["settlement_stage_family"] == preview["settlement_stage_family"], (summary, preview)
assert summary["next_settlement_stage_candidate"] == preview["next_settlement_stage_candidate"], (summary, preview)
assert summary["next_settlement_stage_blockers"] == preview["next_settlement_stage_blockers"], (summary, preview)
assert summary["transactional_statuses"] == preview["transactional_statuses"], (summary, preview)
assert summary["provisional_close_barriers"] == preview["provisional_close_barriers"], (summary, preview)
assert summary["billing_close_barriers"] == preview["billing_close_barriers"], (summary, preview)
assert summary["provisional_close_earliest_at_epoch_ms"] == period["provisional_close_earliest_at_epoch_ms"], (summary, period)
assert summary["late_arrival_deadline_epoch_ms"] == period["late_arrival_deadline_epoch_ms"], (summary, period)
PY

printf 'proof_token_freeze_close_semantics: PASS\n'
