#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REPORT_JSON="$(./target/release/amai observe token-report)"

jq -e '
  .token_budget_report.suitability_contract.model_version == "token-suitability-v1"
' <<<"$REPORT_JSON" >/dev/null

jq -e '
  .token_budget_report.suitability_contract.surfaces
  | map(.code)
  | index("product_kpi")
' <<<"$REPORT_JSON" >/dev/null

jq -e '
  .token_budget_report.contractual_statement_summaries.current_session.suitability.surfaces
  | has("billing_amount")
' <<<"$REPORT_JSON" >/dev/null

jq -e '
  .token_budget_report.statement_export_previews.current_session.suitability.surfaces
  | has("contractual_export")
' <<<"$REPORT_JSON" >/dev/null

echo "PASS: token suitability contract and scope suitability surfaces are materialized"
