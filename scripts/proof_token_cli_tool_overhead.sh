#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

source_kind="proof_cli_context_pack_$(date +%s%N)"
context_pack_raw="/tmp/amai-proof-token-cli-tool-overhead.json"
report_path="/tmp/amai-proof-token-cli-tool-overhead-report.json"

./target/release/amai context pack \
  --project art \
  --query "cli tool overhead proof" \
  --retrieval-mode local_strict \
  --token-source-kind "$source_kind" >"$context_pack_raw"

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
assert summary["observed_tool_overhead_tokens"] > 0, summary
PY

printf 'proof_token_cli_tool_overhead: PASS\n'
