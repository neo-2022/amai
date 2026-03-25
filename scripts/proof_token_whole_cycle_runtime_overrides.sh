#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

source_kind="proof_context_pack_whole_cycle_runtime_$(date +%s%N)"
report_path="/tmp/amai-proof-token-whole-cycle-runtime-overrides.json"

./target/release/amai context pack \
  --project art \
  --namespace continuity \
  --query "whole cycle runtime override proof" \
  --token-source-kind "$source_kind" \
  --client-prompt-tokens 42 \
  --assistant-generation-tokens 24 \
  --tool-overhead-tokens 7 \
  --continuity-restore-tokens 3 >/tmp/amai-proof-token-whole-cycle-runtime-overrides-context-pack.json

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
assert summary["non_live_events_count"] >= 1, summary
assert summary["observed_client_prompt_tokens"] == 42, summary
assert summary["observed_assistant_generation_tokens"] == 24, summary
assert summary["observed_tool_overhead_tokens"] == 7, summary
assert summary["observed_continuity_restore_tokens"] == 3, summary
assert summary["observed_whole_cycle_with_amai_tokens"] >= 76, summary
PY

printf 'proof_token_whole_cycle_runtime_overrides: PASS\n'
