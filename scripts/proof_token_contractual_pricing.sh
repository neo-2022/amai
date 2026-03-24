#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

./scripts/proof_token_ledger.sh >/tmp/amai-proof-token-contractual-bootstrap.out

cargo run --release --quiet -- observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-contractual-base.json

python3 - <<'PY' "$tmpdir"
import json
import sys
from pathlib import Path

tmpdir = Path(sys.argv[1])
report = json.loads(Path("/tmp/amai-proof-token-contractual-base.json").read_text())
root = report["token_budget_report"]
scopes = []
earliest_start = None
latest_end = None
slack_ms = 600_000
for scope in ("current_session", "rolling_window", "lifetime"):
    preview = root["statement_previews"].get(scope)
    if not preview:
        continue
    billed = int(preview.get("internal_provider_billed_tokens") or 0)
    period = preview.get("period") or {}
    period_start = period.get("period_start_epoch_ms")
    period_end = period.get("period_end_epoch_ms")
    if period_start is not None:
        earliest_start = period_start if earliest_start is None else min(earliest_start, period_start)
    if period_end is not None:
        latest_end = period_end if latest_end is None else max(latest_end, period_end)
    scopes.append(
        {
            "scope_code": scope,
            "input_tokens": billed,
            "output_tokens": 0,
            "invoice_amount": round((billed / 1000.0) * 0.01, 12),
            "period_start_epoch_ms": None if period_start is None else period_start - slack_ms,
            "period_end_epoch_ms": None if period_end is None else period_end + slack_ms,
        }
    )

(tmpdir / "provider_rate_card.toml").write_text(
    "\n".join(
        [
            'schema_version = "provider-rate-card-v1"',
            'rate_card_version = "demo-priced-v1"',
            'currency_profile = "USD"',
            'provider = "demo-provider"',
            "default_input_cost_per_1k_tokens = 0.01",
            "default_output_cost_per_1k_tokens = 0.02",
            f"effective_from_epoch_ms = {(earliest_start - slack_ms) if earliest_start is not None else 0}",
            f"effective_to_epoch_ms = {(latest_end + slack_ms) if latest_end is not None else 0}",
            "",
        ]
    )
)

(tmpdir / "infra_cost_profile.toml").write_text(
    "\n".join(
        [
            'schema_version = "infra-cost-profile-v1"',
            'infra_cost_profile_version = "demo-infra-v1"',
            'currency_profile = "USD"',
            'provider = "amai-self-hosted"',
            "cost_per_1k_internal_billed_tokens = 0.002",
            "cost_per_live_event = 0.0005",
            "fixed_scope_cost_amount = 0.01",
            f"effective_from_epoch_ms = {(earliest_start - slack_ms) if earliest_start is not None else 0}",
            f"effective_to_epoch_ms = {(latest_end + slack_ms) if latest_end is not None else 0}",
            "",
        ]
    )
)

(tmpdir / "provider_usage.json").write_text(
    json.dumps(
        {
            "schema_version": "provider-usage-export-v1",
            "provider": "demo-provider",
            "currency_profile": "USD",
            "scopes": scopes,
        },
        indent=2,
    )
)

(tmpdir / "provider_invoice.json").write_text(
    json.dumps(
        {
            "schema_version": "provider-invoice-export-v1",
            "provider": "demo-provider",
            "currency_profile": "USD",
            "scopes": [
                {
                    "scope_code": scope["scope_code"],
                    "invoice_amount": scope["invoice_amount"],
                    "currency_profile": "USD",
                    "invoice_id": f"inv-{scope['scope_code']}",
                    "period_start_epoch_ms": scope["period_start_epoch_ms"],
                    "period_end_epoch_ms": scope["period_end_epoch_ms"],
                }
                for scope in scopes
            ],
        },
        indent=2,
    )
)
PY

AMAI_PROVIDER_RATE_CARD_PATH="$tmpdir/provider_rate_card.toml" \
AMAI_PROVIDER_USAGE_EXPORT_PATH="$tmpdir/provider_usage.json" \
AMAI_PROVIDER_INVOICE_EXPORT_PATH="$tmpdir/provider_invoice.json" \
AMAI_INFRA_COST_PROFILE_PATH="$tmpdir/infra_cost_profile.toml" \
  cargo run --release --quiet -- observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-contractual-priced.json

python3 - <<'PY'
import json
from pathlib import Path

report = json.loads(Path("/tmp/amai-proof-token-contractual-priced.json").read_text())
root = report["token_budget_report"]

assert root["rate_card"]["status"] == "priced_bound", root["rate_card"]
assert root["infra_cost_profile"]["status"] == "priced_bound", root["infra_cost_profile"]
assert root["external_truth_sources"]["infra_cost_profile"]["status"] == "configured_existing_path", root["external_truth_sources"]
assert root["reconciliation_contract"]["contract_version"] == "provider-reconciliation-v9", root["reconciliation_contract"]
assert root["reconciliation_contract"]["provider_cost_truth_completeness_state"] == "provider_cost_bound", root["reconciliation_contract"]
assert root["reconciliation_contract"]["invoice_evidence_completeness_state"] == "provider_invoice_bound", root["reconciliation_contract"]
assert root["reconciliation_contract"]["provider_identity_state"] == "provider_identity_aligned", root["reconciliation_contract"]
assert root["reconciliation_contract"]["source_requirements"]["required_sources_for_usage_truth"] == ["provider_usage_export"], root["reconciliation_contract"]
assert root["reconciliation_contract"]["source_requirements"]["required_sources_for_cost_truth"] == ["provider_rate_card", "provider_usage_export"], root["reconciliation_contract"]
assert root["reconciliation_contract"]["source_requirements"]["optional_sources_for_invoice_evidence"] == ["provider_invoice_export"], root["reconciliation_contract"]
assert root["reconciliation_contract"]["source_requirements"]["unready_required_sources_for_usage_truth"] == [], root["reconciliation_contract"]
assert root["reconciliation_contract"]["source_requirements"]["unready_required_sources_for_cost_truth"] == [], root["reconciliation_contract"]
assert root["reconciliation_contract"]["source_requirements"]["unready_optional_sources_for_invoice_evidence"] == [], root["reconciliation_contract"]
assert root["external_truth_manifest"]["manifest_hash"], root["external_truth_manifest"]
assert root["external_truth_manifest"]["entries"]["provider_rate_card"]["source_sha256"], root["external_truth_manifest"]
assert root["margin_contract"]["status"] == "priced_preview_report_only", root["margin_contract"]
assert root["margin_contract"]["model_version"] == "margin-view-v7", root["margin_contract"]
assert root["margin_contract"]["pricing_truth_completeness_state"] == "pricing_truth_ready", root["margin_contract"]
assert root["margin_contract"]["provider_identity_state"] == "provider_identity_aligned", root["margin_contract"]
assert root["margin_contract"]["money_margin_enabled"] is True, root["margin_contract"]
assert root["margin_contract"]["source_requirements"]["required_sources_for_margin_truth"] == ["infra_cost_profile", "provider_rate_card", "provider_usage_export"], root["margin_contract"]
assert root["margin_contract"]["source_requirements"]["unready_required_sources_for_margin_truth"] == [], root["margin_contract"]

scope = "rolling_window"
if root["contractual_statement_summaries"][scope] is None:
    scope = "current_session"

summary = root["contractual_statement_summaries"][scope]
margin = root["margin_view"][scope]
statement_export = root["statement_export_previews"][scope]

assert summary["reconciliation_state"] == "external_usage_and_invoice_aligned_report_only", summary
assert summary["required_sources_for_usage_truth"] == ["provider_usage_export"], summary
assert summary["required_sources_for_cost_truth"] == ["provider_rate_card", "provider_usage_export"], summary
assert summary["optional_sources_for_invoice_evidence"] == ["provider_invoice_export"], summary
assert summary["unready_required_sources_for_usage_truth"] == [], summary
assert summary["unready_required_sources_for_cost_truth"] == [], summary
assert summary["unready_optional_sources_for_invoice_evidence"] == [], summary
assert summary["provider_usage_scope_alignment_state"] == "scope_period_aligned", summary
assert summary["provider_invoice_scope_alignment_state"] == "scope_period_aligned", summary
assert summary["rate_card_scope_alignment_state"] == "scope_period_aligned", summary
assert summary["rate_card_status"] == "priced_bound", summary
assert summary["rate_card_version"] == "demo-priced-v1", summary
assert summary["rate_card_provider"] == "demo-provider", summary
assert summary["rate_card_currency_profile"] == "USD", summary
assert summary["rate_card_provider_alignment_state"] == "provider_identity_aligned", summary
assert summary["invoice_provider_alignment_state"] == "provider_identity_aligned", summary
assert summary["provider_identity_state"] == "provider_identity_aligned", summary
assert summary["rate_card_truth_completeness_state"] == "rate_card_priced_bound", summary
assert summary["provider_cost_truth_completeness_state"] == "provider_cost_bound", summary
assert summary["invoice_evidence_completeness_state"] == "provider_invoice_bound", summary
assert summary["pricing_truth_completeness_state"] == "pricing_truth_ready", summary
assert summary["required_sources_for_margin_truth"] == ["infra_cost_profile", "provider_rate_card", "provider_usage_export"], summary
assert summary["unready_required_sources_for_margin_truth"] == [], summary
assert summary["reconciliation_temporal_truth_state"] == "scope_period_aligned", summary
assert summary["margin_state"] == "priced_preview_report_only", summary
assert summary["margin_confidence_state"] == "aligned_report_only", summary
assert summary["margin_readiness_state"] == "preview_ready_report_only", summary
assert summary["margin_provider_identity_state"] == "provider_identity_aligned", summary
assert summary["margin_blocking_reasons"] == [], summary
assert summary["margin_temporal_truth_state"] == "scope_period_aligned", summary
assert summary["external_provider_cost_amount"] is not None, summary
assert margin["customer_saved_amount_lower_bound"] is not None, margin
assert margin["amai_infra_cost_amount"] is not None, margin
assert margin["margin_amount"] is not None, margin
assert margin["rate_card_scope_alignment_state"] == "scope_period_aligned", margin
assert margin["infra_cost_scope_alignment_state"] == "scope_period_aligned", margin
assert margin["provider_identity_state"] == "provider_identity_aligned", margin
assert margin["temporal_truth_state"] == "scope_period_aligned", margin
assert statement_export["rate_card_version"] == "demo-priced-v1", statement_export
assert statement_export["provider_cost_truth_completeness_state"] == "provider_cost_bound", statement_export
assert statement_export["invoice_evidence_completeness_state"] == "provider_invoice_bound", statement_export
assert statement_export["settlement_report_preview"]["model_version"] == "settlement-report-preview-v3", statement_export
assert statement_export["required_sources_for_usage_truth"] == ["provider_usage_export"], statement_export
assert statement_export["required_sources_for_margin_truth"] == ["infra_cost_profile", "provider_rate_card", "provider_usage_export"], statement_export
assert statement_export["settlement_report_preview"]["settlement_report_id"], statement_export
assert statement_export["rate_card_provider"] == "demo-provider", statement_export
assert statement_export["rate_card_currency_profile"] == "USD", statement_export
assert statement_export["line_item_surfaces"]["margin_scope"]["margin_state"] == "priced_preview_report_only", statement_export
PY

printf 'proof_token_contractual_pricing: PASS\n'
