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
period_start="$(
  cargo run --release --quiet -- observe token-report --budget-profile codex_5h --include-verify-events true \
    | jq -r '.token_budget_report.statement_previews.lifetime.period.period_start_epoch_ms'
)"
period_end="$(
  cargo run --release --quiet -- observe token-report --budget-profile codex_5h --include-verify-events true \
    | jq -r '.token_budget_report.statement_previews.lifetime.period.period_end_epoch_ms'
)"
period_start=$((period_start - 600000))
period_end=$((period_end + 600000))

provider_cost="$(python3 - <<'PY' "$internal_provider_billed_tokens"
import sys
tokens = int(sys.argv[1])
print((tokens / 1000.0) * 2.0)
PY
)"

cat >"$tmpdir/provider_rate_card.json" <<JSON
{
  "schema_version": "provider-rate-card-v1",
  "rate_card_version": "proof-rate-card-v1",
  "currency_profile": "USD",
  "provider": "openai",
  "default_input_cost_per_1k_tokens": 2.0,
  "default_output_cost_per_1k_tokens": 1.0,
  "effective_from_epoch_ms": $period_start,
  "effective_to_epoch_ms": $period_end
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
      "currency_profile": "USD",
      "period_start_epoch_ms": $period_start,
      "period_end_epoch_ms": $period_end
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
      "invoice_id": "proof-invoice-1",
      "period_start_epoch_ms": $period_start,
      "period_end_epoch_ms": $period_end
    }
  ]
}
JSON

cat >"$tmpdir/infra_cost_profile.json" <<JSON
{
  "schema_version": "infra-cost-profile-v1",
  "infra_cost_profile_version": "proof-infra-cost-v1",
  "currency_profile": "USD",
  "provider": "amai",
  "cost_per_1k_internal_billed_tokens": 0.25,
  "cost_per_live_event": 0.0001,
  "fixed_scope_cost_amount": 0.0,
  "effective_from_epoch_ms": $period_start,
  "effective_to_epoch_ms": $period_end
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
assert payload["reconciliation_preview"]["usage_truth_completeness_state"] == "provider_usage_bound", payload
assert payload["reconciliation_preview"]["money_truth_completeness_state"] == "provider_cost_and_invoice_bound", payload
assert payload["reconciliation_preview"]["reconciliation_readiness_state"] == "usage_cost_and_invoice_truth_ready", payload
assert payload["reconciliation_preview"]["provider_usage_scope_alignment_state"] == "scope_period_aligned", payload
assert payload["reconciliation_preview"]["provider_invoice_scope_alignment_state"] == "scope_period_aligned", payload
assert payload["reconciliation_preview"]["rate_card_scope_alignment_state"] == "scope_period_aligned", payload
assert payload["reconciliation_preview"]["rate_card_provider_alignment_state"] == "provider_identity_aligned", payload
assert payload["reconciliation_preview"]["invoice_provider_alignment_state"] == "provider_identity_aligned", payload
assert payload["reconciliation_preview"]["provider_identity_state"] == "provider_identity_aligned", payload
assert payload["reconciliation_preview"]["temporal_truth_state"] == "scope_period_aligned", payload
assert payload["margin_scope"]["margin_state"] == "priced_preview_report_only", payload
assert payload["margin_scope"]["margin_confidence_state"] == "aligned_report_only", payload
assert payload["margin_scope"]["rate_card_scope_alignment_state"] == "scope_period_aligned", payload
assert payload["margin_scope"]["infra_cost_scope_alignment_state"] == "scope_period_aligned", payload
assert payload["margin_scope"]["provider_identity_state"] == "provider_identity_aligned", payload
assert payload["margin_scope"]["temporal_truth_state"] == "scope_period_aligned", payload
assert payload["statement_export_preview"]["scope_code"] == "lifetime", payload
PY

printf 'proof_token_contractual_sources: PASS\n'
