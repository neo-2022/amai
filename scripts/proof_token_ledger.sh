#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

./scripts/proof_token_benchmark_suite.sh >/tmp/amai-proof-token-suite.out

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-context-pack.out

cargo run --release --quiet -- observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >/tmp/amai-proof-token-ledger.out

python3 - <<'PY'
import json
from pathlib import Path

report = json.loads(Path("/tmp/amai-proof-token-ledger.out").read_text())
root = report["token_budget_report"]

assert root["profile"]["code"] == "codex_5h", root["profile"]
assert root["current_session"]["events_total"] >= 1, root["current_session"]
assert root["lifetime"]["events_total"] >= 1, root["lifetime"]
assert root["lifetime"]["total_saved_tokens"] > 0, root["lifetime"]
assert root["lifetime"]["savings_percent"] > 0, root["lifetime"]
assert any(
    item["source_kind"] in {"proof_context_pack", "verify_token_benchmark"}
    for item in root["source_breakdown"]
), root["source_breakdown"]
PY
