#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
registry="$tmpdir/token_adjustment_registry.json"

./scripts/proof_token_ledger.sh >/tmp/amai-proof-token-adjustment-bootstrap.out

AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH="$registry" \
  cargo run --release --quiet -- observe token-adjustment-add \
  --scope lifetime \
  --kind adjustment_entry \
  --status applied_report_only \
  --reason-code contaminated_live_session \
  --tokens-delta=-200 >/tmp/amai-proof-token-adjustment-add.json

AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH="$registry" \
  cargo run --release --quiet -- observe token-adjustment-registry \
  --scope lifetime >/tmp/amai-proof-token-adjustment-registry.json

AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH="$registry" \
  cargo run --release --quiet -- observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-adjustment-report.json

python3 - <<'PY'
import json
from pathlib import Path

added = json.loads(Path("/tmp/amai-proof-token-adjustment-add.json").read_text())
registry = json.loads(Path("/tmp/amai-proof-token-adjustment-registry.json").read_text())
report = json.loads(Path("/tmp/amai-proof-token-adjustment-report.json").read_text())

assert added["token_adjustment_add"]["entry"]["scope_code"] == "lifetime", added
assert added["token_adjustment_add"]["entry"]["status"] == "applied_report_only", added
assert registry["scope_summary"]["applied_entries_count"] == 1, registry
assert registry["scope_summary"]["applied_tokens_delta"] == -200, registry

preview = report["token_budget_report"]["statement_previews"]["lifetime"]
summary = report["token_budget_report"]["contractual_statement_summaries"]["lifetime"]

assert preview["adjustment_preview"]["applied_entries_count"] == 1, preview
assert preview["adjustment_preview"]["net_tokens_delta"] == -200, preview
assert preview["lifecycle_state"] == "measured_non_billable_adjusted_report_only", preview
assert preview["adjusted_measured_non_billable_lower_bound_tokens"] == (
    preview["measured_non_billable_lower_bound_tokens"] - 200
), preview
assert summary["adjustment_state"] == "applied_report_only", summary
PY

printf 'proof_token_adjustment_registry: PASS\n'
