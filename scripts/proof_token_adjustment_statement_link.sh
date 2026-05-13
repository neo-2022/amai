#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
registry="$tmpdir/token_adjustment_registry.json"

expected_statement_id="$(
  ./target/release/amai observe token-report --budget-profile codex_5h --include-verify-events true \
    | jq -r '.token_budget_report.statement_export_previews.lifetime.statement_preview_id'
)"
printf '%s' "$expected_statement_id" >/tmp/amai-proof-token-adjustment-statement-link.json.expected

AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH="$registry" \
  ./target/release/amai observe token-adjustment-add \
  --scope lifetime \
  --kind adjustment_entry \
  --status pending_review \
  --reason-code contaminated_live_session \
  --resolve-related-statement-id \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-adjustment-statement-link.json

python3 - <<'PY'
import json
from pathlib import Path

payload = json.loads(Path("/tmp/amai-proof-token-adjustment-statement-link.json").read_text())
entry = payload["token_adjustment_add"]["entry"]

expected_statement_id = Path("/tmp/amai-proof-token-adjustment-statement-link.json.expected").read_text().strip()
assert payload["token_adjustment_add"]["resolved_related_statement_id"] == expected_statement_id, payload
assert entry["related_statement_id"] == expected_statement_id, payload
assert entry["status"] == "pending_review", payload
PY

printf 'proof_token_adjustment_statement_link: PASS\n'
