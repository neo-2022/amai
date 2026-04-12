#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Awarnings"
export CARGO_TERM_COLOR=never

run_amai_last_line() {
  local output_file
  output_file="$(mktemp)"
  cargo run --quiet -- "$@" >"${output_file}" 2>/dev/null
  python3 - "${output_file}" <<'PY'
import pathlib
import sys

lines = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
for line in reversed(lines):
    if line.strip():
        print(line)
        break
else:
    raise SystemExit("run_amai_last_line: no non-empty output line captured")
PY
  rm -f "${output_file}"
}

run_amai_capture() {
  local output_file
  output_file="$(mktemp)"
  cargo run --quiet -- "$@" >"${output_file}" 2>/dev/null
  cat "${output_file}"
  rm -f "${output_file}"
}

extract_uuid() {
  python3 -c 'import re,sys; m=re.search(r"[0-9a-f-]{36}", sys.stdin.read()); assert m, "uuid not found"; print(m.group(0))'
}

./scripts/bootstrap_stack.sh >/dev/null

suffix="manual_stage5_$(date +%s%N)"
namespace_code="review_${suffix}"
project_code="project_alpha"

run_amai_capture namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name "${namespace_code}" \
  --retrieval-mode local_strict >/dev/null

fact_subject="infra.server.region.${suffix}"
old_fact_id="$(
  run_amai_capture memory create-card \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --title "Region old ${suffix}" \
    --summary "Server region is eu-west." \
    --body "old region body" \
    --provenance-json '{"source_event_ids":["event:old"],"artifact_refs":["artifact://old"],"message_refs":["thread:old"],"evidence_span":{"kind":"memory_card","case":"manual_old"},"source_kind":"manual_stage5"}' \
    --fact-subject "${fact_subject}" \
    --fact-predicate current_region \
    --fact-object eu-west \
    --truth-state current \
    --verification-state verified \
    --status active \
    --observed-at-epoch-ms 1000 \
    --recorded-at-epoch-ms 1001 \
    --valid-from-epoch-ms 1000 \
    --last-verified-at-epoch-ms 1002 | extract_uuid
)"
new_fact_id="$(
  run_amai_capture memory apply-card-update \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --title "Region new ${suffix}" \
    --summary "Server region moved to us-east." \
    --body "new region body" \
    --tag semantic \
    --tag temporal \
    --provenance-json '{"source_event_ids":["event:new"],"artifact_refs":["artifact://new"],"message_refs":["thread:new"],"evidence_span":{"kind":"memory_card","case":"manual_new"},"source_kind":"manual_stage5"}' \
    --fact-subject "${fact_subject}" \
    --fact-predicate current_region \
    --fact-object us-east \
    --truth-state current \
    --verification-state verified \
    --status active \
    --observed-at-epoch-ms 2000 \
    --recorded-at-epoch-ms 2001 \
    --valid-from-epoch-ms 2000 \
    --last-verified-at-epoch-ms 2002 | extract_uuid
)"
historical_region_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "What is the current region of ${fact_subject}?" \
    --limit-chunks 10 \
    --at-epoch-ms 1500 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"
future_region_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "What is the current region of ${fact_subject}?" \
    --limit-chunks 10 \
    --at-epoch-ms 2500 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"
future_only_backflow_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "When did ${fact_subject} move to us-east?" \
    --limit-chunks 10 \
    --at-epoch-ms 1500 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"

retract_subject="service.status.${suffix}"
retracted_fact_id="$(
  run_amai_capture memory create-card \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --title "Retract base ${suffix}" \
    --summary "Service status was stable." \
    --body "retract body" \
    --provenance-json '{"source_event_ids":["event:retract"],"artifact_refs":["artifact://retract"],"message_refs":["thread:retract"],"evidence_span":{"kind":"memory_card","case":"manual_retract"},"source_kind":"manual_stage5"}' \
    --fact-subject "${retract_subject}" \
    --fact-predicate deployment_state \
    --fact-object stable \
    --truth-state current \
    --verification-state verified \
    --status active \
    --observed-at-epoch-ms 1000 \
    --recorded-at-epoch-ms 1001 \
    --valid-from-epoch-ms 1000 \
    --last-verified-at-epoch-ms 1002 | extract_uuid
)"
run_amai_capture memory update-card-truth-state \
  --memory-card-id "${retracted_fact_id}" \
  --truth-state retracted \
  --verification-state verified \
  --status inactive \
  --last-verified-at-epoch-ms 2000 >/dev/null
retracted_fact_payload="$(run_amai_last_line memory get-card --memory-card-id "${retracted_fact_id}")"
retracted_historical_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "${retract_subject}" \
    --limit-chunks 10 \
    --at-epoch-ms 1500 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"
retracted_future_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "${retract_subject}" \
    --limit-chunks 10 \
    --at-epoch-ms 2500 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"
retracted_latest_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "${retract_subject}" \
    --limit-chunks 10 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"

owner_subject="service.owner.${suffix}"
owner_current_id="$(
  run_amai_capture memory create-card \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --title "Owner current ${suffix}" \
    --summary "Current verified ownership fact." \
    --body "The service owner is team platform." \
    --provenance-json '{"source_event_ids":["event:owner-current"],"artifact_refs":["artifact://owner-current"],"message_refs":["thread:owner-current"],"evidence_span":{"kind":"memory_card","case":"manual_owner_current"},"source_kind":"manual_stage5"}' \
    --fact-subject "${owner_subject}" \
    --fact-predicate team \
    --fact-object platform \
    --truth-state current \
    --verification-state verified \
    --status active \
    --observed-at-epoch-ms 1000 \
    --recorded-at-epoch-ms 1001 \
    --valid-from-epoch-ms 1000 \
    --last-verified-at-epoch-ms 1002 | extract_uuid
)"
owner_conflicted_id="$(
  run_amai_capture memory create-card \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --title "Owner conflicted ${suffix}" \
    --summary "Conflicted ownership claim with fresher timestamp." \
    --body "The service owner might be team platform, but this claim is conflicted." \
    --provenance-json '{"source_event_ids":["event:owner-conflicted"],"artifact_refs":["artifact://owner-conflicted"],"message_refs":["thread:owner-conflicted"],"evidence_span":{"kind":"memory_card","case":"manual_owner_conflicted"},"source_kind":"manual_stage5"}' \
    --fact-subject "${owner_subject}" \
    --fact-predicate team \
    --fact-object platform \
    --truth-state conflicted \
    --verification-state disputed \
    --status active \
    --observed-at-epoch-ms 2000 \
    --recorded-at-epoch-ms 2001 \
    --valid-from-epoch-ms 2000 \
    --last-verified-at-epoch-ms 2002 | extract_uuid
)"
owner_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "${owner_subject}" \
    --limit-chunks 10 \
    --token-source-kind verify_context_pack \
    --disable-cache
)"

python3 - <<'PY' \
  "${namespace_code}" \
  "${historical_region_hits}" \
  "${future_region_hits}" \
  "${future_only_backflow_hits}" \
  "${old_fact_id}" \
  "${new_fact_id}" \
  "${retracted_fact_payload}" \
  "${retracted_historical_hits}" \
  "${retracted_future_hits}" \
  "${retracted_latest_hits}" \
  "${retracted_fact_id}" \
  "${owner_hits}" \
  "${owner_current_id}" \
  "${owner_conflicted_id}"
import json
import sys

namespace_code = sys.argv[1]
historical_region_hits = json.loads(sys.argv[2])
future_region_hits = json.loads(sys.argv[3])
future_only_backflow_hits = json.loads(sys.argv[4])
old_fact_id = sys.argv[5]
new_fact_id = sys.argv[6]
retracted_fact_payload = json.loads(sys.argv[7])
retracted_historical_hits = json.loads(sys.argv[8])
retracted_future_hits = json.loads(sys.argv[9])
retracted_latest_hits = json.loads(sys.argv[10])
retracted_fact_id = sys.argv[11]
owner_hits = json.loads(sys.argv[12])
owner_current_id = sys.argv[13]
owner_conflicted_id = sys.argv[14]

def memory_card_ids(payload):
    return [item["memory_card_id"] for item in payload["retrieval"]["memory_cards"]]

assert old_fact_id in memory_card_ids(historical_region_hits), historical_region_hits
assert new_fact_id not in memory_card_ids(historical_region_hits), historical_region_hits
assert new_fact_id in memory_card_ids(future_region_hits), future_region_hits
assert old_fact_id not in memory_card_ids(future_region_hits), future_region_hits
assert memory_card_ids(future_only_backflow_hits) == [], future_only_backflow_hits

assert retracted_fact_payload["truth_state"] == "retracted", retracted_fact_payload
assert retracted_fact_payload["valid_to_epoch_ms"] == 2000, retracted_fact_payload
assert retracted_fact_id in memory_card_ids(retracted_historical_hits), retracted_historical_hits
assert retracted_fact_id not in memory_card_ids(retracted_future_hits), retracted_future_hits
assert retracted_fact_id not in memory_card_ids(retracted_latest_hits), retracted_latest_hits

owner_ids = memory_card_ids(owner_hits)
assert owner_ids and owner_ids[0] == owner_current_id, owner_hits
assert owner_conflicted_id in owner_ids, owner_hits

print(json.dumps({
    "namespace": namespace_code,
    "generic_old_new": "pass",
    "future_only_backflow": "pass",
    "retract_temporal_window": "pass",
    "mixed_state_ranking": "pass",
    "owner_result_order": owner_ids[:3],
}, ensure_ascii=False))
PY

printf 'proof_semantic_temporal_manual_acceptance: ok\n'
