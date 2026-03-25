#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
bundle_dir="$tmpdir/token-statement-export"

./target/release/amai observe token-statement-export \
  --scope lifetime \
  --budget-profile codex_5h \
  --include-verify-events true \
  --output-dir "$bundle_dir" >/tmp/amai-proof-token-statement-export-path.txt

python3 - <<'PY'
import json
from pathlib import Path

bundle_dir = Path("/tmp/amai-proof-token-statement-export-path.txt").read_text().strip()
root = Path(bundle_dir)
manifest = json.loads((root / "manifest.json").read_text())
settlement_report = json.loads((root / "settlement_report_preview.json").read_text())
statement_export = json.loads((root / "statement_export_preview.json").read_text())
evidence_pack = json.loads((root / "contractual_evidence_pack.json").read_text())
contractual_sources = json.loads((root / "token_contractual_sources.json").read_text())

assert manifest["bundle_version"] == "token-statement-export-bundle-v2", manifest
assert manifest["scope_code"] == "lifetime", manifest
assert settlement_report["scope_code"] == "lifetime", settlement_report
assert statement_export["scope_code"] == "lifetime", statement_export
assert evidence_pack["scope_code"] == "lifetime", evidence_pack
assert contractual_sources["scope_code"] == "lifetime", contractual_sources
assert statement_export["model_version"] == "contractual-statement-export-v15", statement_export
assert settlement_report["model_version"] == "settlement-report-preview-v6", settlement_report
assert evidence_pack["pack_version"] == "contractual-evidence-pack-v15", evidence_pack
assert statement_export["contractual_readiness_model_version"] == "contractual-readiness-v1", statement_export
assert settlement_report["contractual_readiness_model_version"] == "contractual-readiness-v1", settlement_report
assert evidence_pack["contractual_readiness_model_version"] == "contractual-readiness-v1", evidence_pack
assert statement_export["customer_contractual_boundary"]["model_version"] == "customer-contractual-boundary-v1", statement_export
assert settlement_report["customer_contractual_boundary"]["model_version"] == "customer-contractual-boundary-v1", settlement_report
assert evidence_pack["customer_contractual_boundary"]["model_version"] == "customer-contractual-boundary-v1", evidence_pack
assert statement_export["internal_money_arithmetic_readiness_state"] is not None, statement_export
assert settlement_report["internal_money_arithmetic_readiness_state"] is not None, settlement_report
assert evidence_pack["internal_money_arithmetic_readiness_state"] is not None, evidence_pack
assert statement_export["contractual_settlement_readiness_state"] is not None, statement_export
assert settlement_report["contractual_settlement_readiness_state"] is not None, settlement_report
assert evidence_pack["contractual_settlement_readiness_state"] is not None, evidence_pack
assert statement_export["external_truth_manifest"]["manifest_hash"], statement_export
assert settlement_report["external_truth_manifest_hash"], settlement_report
assert evidence_pack["external_truth_manifest"]["manifest_hash"], evidence_pack
assert contractual_sources["external_truth_manifest"]["manifest_hash"], contractual_sources
assert settlement_report["settlement_report_id"], settlement_report
assert statement_export["rate_card_truth_completeness_state"] is not None, statement_export
assert statement_export["provider_cost_truth_completeness_state"] is not None, statement_export
assert statement_export["invoice_evidence_completeness_state"] is not None, statement_export
assert statement_export["pricing_truth_completeness_state"] is not None, statement_export
assert statement_export["customer_savings_money_truth_completeness_state"] is not None, statement_export
assert statement_export["amai_cost_truth_completeness_state"] is not None, statement_export
assert statement_export["margin_truth_completeness_state"] is not None, statement_export
assert statement_export["margin_readiness_state"] is not None, statement_export
assert statement_export["required_sources_for_usage_truth"] is not None, statement_export
assert statement_export["required_sources_for_margin_truth"] is not None, statement_export
assert evidence_pack["rate_card_truth_completeness_state"] is not None, evidence_pack
assert evidence_pack["provider_cost_truth_completeness_state"] is not None, evidence_pack
assert evidence_pack["invoice_evidence_completeness_state"] is not None, evidence_pack
assert evidence_pack["pricing_truth_completeness_state"] is not None, evidence_pack
assert evidence_pack["customer_savings_money_truth_completeness_state"] is not None, evidence_pack
assert evidence_pack["amai_cost_truth_completeness_state"] is not None, evidence_pack
assert evidence_pack["margin_truth_completeness_state"] is not None, evidence_pack
assert evidence_pack["margin_readiness_state"] is not None, evidence_pack
assert evidence_pack["required_sources_for_usage_truth"] is not None, evidence_pack
assert evidence_pack["required_sources_for_margin_truth"] is not None, evidence_pack
assert statement_export["provider_identity_state"] in {"provider_identity_aligned", "provider_identity_unchecked"}, statement_export
assert statement_export["rate_card_status"] is not None, statement_export
assert statement_export["transactional_statuses"]["billable"]["boundary"] == "reserved_future", statement_export
assert evidence_pack["transactional_statuses"]["measured"]["boundary"] in {"not_started", "measured_report_only"}, evidence_pack
assert contractual_sources["transactional_statuses"] == evidence_pack["transactional_statuses"], contractual_sources
assert statement_export["export_semantics"]["surface_kind"] == "customer_review_report_only", statement_export
assert evidence_pack["export_semantics"]["surface_kind"] == "customer_evidence_pack_report_only", evidence_pack
assert statement_export["customer_contractual_boundary"]["surface_kind"] == "customer_review_report_only", statement_export
assert settlement_report["customer_contractual_boundary"]["surface_kind"] == "customer_settlement_report_preview_report_only", settlement_report
assert evidence_pack["customer_contractual_boundary"]["surface_kind"] == "customer_evidence_pack_report_only", evidence_pack
assert contractual_sources["customer_contractual_boundary"]["surface_kind"] == "customer_contractual_sources_report_only", contractual_sources
assert contractual_sources["customer_contractual_boundary"]["operational_telemetry_included"] is False, contractual_sources
assert manifest["surface_kind"] == "customer_review_bundle_report_only", manifest
assert manifest["customer_contractual_boundary"]["surface_kind"] == "customer_review_bundle_report_only", manifest
assert manifest["statement_preview_id"] == statement_export["statement_preview_id"], manifest
assert manifest["files"]["settlement_report_preview"] == "settlement_report_preview.json", manifest
assert evidence_pack["included_events_hash"] == statement_export["included_events_hash"], evidence_pack
assert contractual_sources["statement_export_preview"]["statement_preview_id"] == statement_export["statement_preview_id"], contractual_sources
assert contractual_sources["settlement_report_preview"]["settlement_report_id"] == settlement_report["settlement_report_id"], contractual_sources
assert statement_export["settlement_report_preview"]["settlement_report_id"] == settlement_report["settlement_report_id"], statement_export
assert evidence_pack["settlement_report_preview"]["settlement_report_id"] == settlement_report["settlement_report_id"], evidence_pack
PY

printf 'proof_token_statement_export_bundle: PASS\n'
