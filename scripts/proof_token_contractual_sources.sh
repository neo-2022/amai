#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

internal_provider_billed_tokens="$(
  cargo run --release --quiet -- observe token-report --budget-profile codex_5h --include-verify-events true \
    | jq -r '.token_budget_report.statement_previews.lifetime.internal_provider_billed_tokens'
)"

provider_cost="$(python3 - <<'PY' "$internal_provider_billed_tokens"
import sys
tokens = int(sys.argv[1])
print((tokens / 1000.0) * 2.0)
PY
)"

cat >"$tmpdir/provider_rate_card.json" <<'JSON'
{
  "schema_version": "provider-rate-card-v1",
  "rate_card_version": "proof-rate-card-v1",
  "currency_profile": "USD",
  "provider": "openai",
  "default_input_cost_per_1k_tokens": 2.0,
  "default_output_cost_per_1k_tokens": 1.0
}
JSON

cat >"$tmpdir/provider_usage_export.json" <<JSON
{
  "schema_version": "provider-usage-export-v1",
  "provider": "openai",
  "currency_profile": "USD",
  "scopes": [
    {
      "scope_code": "lifetime",
      "total_tokens": $internal_provider_billed_tokens,
      "provider_cost_amount": $provider_cost,
      "currency_profile": "USD"
    }
  ]
}
JSON

cat >"$tmpdir/provider_invoice_export.json" <<JSON
{
  "schema_version": "provider-invoice-export-v1",
  "provider": "openai",
  "currency_profile": "USD",
  "scopes": [
    {
      "scope_code": "lifetime",
      "invoice_amount": $provider_cost,
      "currency_profile": "USD",
      "invoice_id": "proof-invoice-1"
    }
  ]
}
JSON

cat >"$tmpdir/infra_cost_profile.json" <<'JSON'
{
  "schema_version": "infra-cost-profile-v1",
  "infra_cost_profile_version": "proof-infra-cost-v1",
  "currency_profile": "USD",
  "provider": "amai",
  "cost_per_1k_internal_billed_tokens": 0.25,
  "cost_per_live_event": 0.0001,
  "fixed_scope_cost_amount": 0.0
}
JSON

AMAI_PROVIDER_RATE_CARD_PATH="$tmpdir/provider_rate_card.json" \
AMAI_PROVIDER_USAGE_EXPORT_PATH="$tmpdir/provider_usage_export.json" \
AMAI_PROVIDER_INVOICE_EXPORT_PATH="$tmpdir/provider_invoice_export.json" \
AMAI_INFRA_COST_PROFILE_PATH="$tmpdir/infra_cost_profile.json" \
  cargo run --release --quiet -- observe token-contractual-sources \
  --scope lifetime \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-contractual-sources.json

python3 - <<'PY'
import json
from pathlib import Path

payload = json.loads(Path("/tmp/amai-proof-token-contractual-sources.json").read_text())["token_contractual_sources"]

assert payload["external_truth_sources"]["provider_usage_export"]["status"] == "configured_existing_path", payload
assert payload["rate_card"]["status"] == "priced_bound", payload
assert payload["provider_usage_binding"]["status"] == "usage_and_cost_bound", payload
assert payload["provider_invoice_binding"]["status"] == "invoice_bound", payload
assert payload["infra_cost_profile"]["status"] == "priced_bound", payload
assert payload["reconciliation_preview"]["reconciliation_state"] == "external_usage_and_invoice_aligned_report_only", payload
assert payload["margin_scope"]["margin_state"] == "priced_preview_report_only", payload
assert payload["statement_export_preview"]["scope_code"] == "lifetime", payload
PY

printf 'proof_token_contractual_sources: PASS\n'
