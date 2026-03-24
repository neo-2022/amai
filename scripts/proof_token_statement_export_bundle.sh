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
statement_export = json.loads((root / "statement_export_preview.json").read_text())
evidence_pack = json.loads((root / "contractual_evidence_pack.json").read_text())
contractual_sources = json.loads((root / "token_contractual_sources.json").read_text())

assert manifest["bundle_version"] == "token-statement-export-bundle-v1", manifest
assert manifest["scope_code"] == "lifetime", manifest
assert statement_export["scope_code"] == "lifetime", statement_export
assert evidence_pack["scope_code"] == "lifetime", evidence_pack
assert contractual_sources["scope_code"] == "lifetime", contractual_sources
assert manifest["statement_preview_id"] == statement_export["statement_preview_id"], manifest
assert evidence_pack["included_events_hash"] == statement_export["included_events_hash"], evidence_pack
assert contractual_sources["statement_export_preview"]["statement_preview_id"] == statement_export["statement_preview_id"], contractual_sources
PY

printf 'proof_token_statement_export_bundle: PASS\n'
