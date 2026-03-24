#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

./scripts/proof_token_ledger.sh >/tmp/amai-proof-token-contractual-bootstrap.out

cargo run --release --quiet -- observe token-report \
  --budget-profile codex_5h >/tmp/amai-proof-token-contractual-base.json

python3 - <<'PY' "$tmpdir"
import json
import sys
from pathlib import Path

tmpdir = Path(sys.argv[1])
report = json.loads(Path("/tmp/amai-proof-token-contractual-base.json").read_text())
root = report["token_budget_report"]
scopes = []
for scope in ("current_session", "rolling_window", "lifetime"):
    preview = root["statement_previews"].get(scope)
    if not preview:
        continue
    billed = int(preview.get("internal_provider_billed_tokens") or 0)
    scopes.append(
        {
            "scope_code": scope,
            "input_tokens": billed,
            "output_tokens": 0,
            "invoice_amount": round((billed / 1000.0) * 0.01, 12),
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
  --budget-profile codex_5h >/tmp/amai-proof-token-contractual-priced.json

python3 - <<'PY'
import json
from pathlib import Path

report = json.loads(Path("/tmp/amai-proof-token-contractual-priced.json").read_text())
root = report["token_budget_report"]

assert root["rate_card"]["status"] == "priced_bound", root["rate_card"]
assert root["infra_cost_profile"]["status"] == "priced_bound", root["infra_cost_profile"]
assert root["margin_contract"]["status"] == "priced_preview_report_only", root["margin_contract"]
assert root["margin_contract"]["money_margin_enabled"] is True, root["margin_contract"]

scope = "rolling_window"
if root["contractual_statement_summaries"][scope] is None:
    scope = "current_session"

summary = root["contractual_statement_summaries"][scope]
margin = root["margin_view"][scope]
statement_export = root["statement_export_previews"][scope]

assert summary["reconciliation_state"] == "external_usage_and_invoice_aligned_report_only", summary
assert summary["margin_state"] == "priced_preview_report_only", summary
assert summary["external_provider_cost_amount"] is not None, summary
assert margin["customer_saved_amount_lower_bound"] is not None, margin
assert margin["amai_infra_cost_amount"] is not None, margin
assert margin["margin_amount"] is not None, margin
assert statement_export["line_item_surfaces"]["margin_scope"]["margin_state"] == "priced_preview_report_only", statement_export
PY
