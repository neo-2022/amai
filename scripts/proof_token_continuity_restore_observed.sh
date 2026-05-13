#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

source_kind="proof_continuity_startup_$(date +%s%N)"
report_path="/tmp/amai-proof-token-continuity-restore-observed.json"

./target/release/amai continuity startup \
  --project art \
  --namespace continuity \
  --json \
  --token-source-kind "$source_kind" >/tmp/amai-proof-continuity-startup.json

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
assert summary["observed_continuity_restore_tokens"] > 0, summary
assert summary["non_live_events_count"] >= 1, summary
assert summary["observed_whole_cycle_with_amai_tokens"] >= summary["observed_continuity_restore_tokens"], summary
PY

printf 'proof_token_continuity_restore_observed: PASS\n'
