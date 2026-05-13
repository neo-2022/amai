#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

source_kind="proof_mcp_assistant_generation_$(date +%s%N)"
report_path="/tmp/amai-proof-token-mcp-assistant-generation.json"

./target/release/amai verify mcp \
  --project art \
  --namespace continuity \
  --query "same meter mcp assistant generation" \
  --retrieval-mode local_strict \
  --proof-scope token-ledger \
  --token-source-kind "$source_kind" >/tmp/amai-proof-token-mcp-assistant-generation-verify.json

./target/release/amai observe token-report \
  --budget-profile codex_5h \
  --include-verify-events true >"$report_path"

SOURCE_KIND="$source_kind" REPORT_PATH="$report_path" python3 - <<'PY'
import json
import os
from pathlib import Path

source_kind = os.environ["SOURCE_KIND"]
report = json.loads(Path(os.environ["REPORT_PATH"]).read_text())["token_budget_report"]
entry = next(
    item for item in report["source_breakdown"] if item["source_kind"] == source_kind
)
summary = entry["summary"]

assert summary["events_total"] >= 1, summary
assert summary["observed_assistant_generation_tokens"] > 0, summary
PY

printf 'proof_token_mcp_assistant_generation: PASS\n'
