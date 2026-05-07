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
related_project_code="project_beta"

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
historical_region_cached_first_stdout="$(mktemp)"
historical_region_cached_first_stderr="$(mktemp)"
cargo run --quiet -- context pack \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --query "What is the current region of ${fact_subject}?" \
  --limit-chunks 10 \
  --at-epoch-ms 1500 \
  --token-source-kind cli >"${historical_region_cached_first_stdout}" \
  2>"${historical_region_cached_first_stderr}"
historical_region_cached_first="$(tail -n 1 "${historical_region_cached_first_stdout}")"
historical_region_cached_first_cache_hit="$(
  if rg -q "context pack cache hit" "${historical_region_cached_first_stderr}"; then
    printf 'true'
  else
    printf 'false'
  fi
)"
rm -f "${historical_region_cached_first_stdout}" "${historical_region_cached_first_stderr}"

historical_region_cached_second_stdout="$(mktemp)"
historical_region_cached_second_stderr="$(mktemp)"
cargo run --quiet -- context pack \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --query "What is the current region of ${fact_subject}?" \
  --limit-chunks 10 \
  --at-epoch-ms 1500 \
  --token-source-kind cli >"${historical_region_cached_second_stdout}" \
  2>"${historical_region_cached_second_stderr}"
historical_region_cached_second="$(tail -n 1 "${historical_region_cached_second_stdout}")"
historical_region_cached_second_cache_hit="$(
  if rg -q "context pack cache hit" "${historical_region_cached_second_stderr}"; then
    printf 'true'
  else
    printf 'false'
  fi
)"
rm -f "${historical_region_cached_second_stdout}" "${historical_region_cached_second_stderr}"

future_region_cached_stdout="$(mktemp)"
future_region_cached_stderr="$(mktemp)"
cargo run --quiet -- context pack \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --query "What is the current region of ${fact_subject}?" \
  --limit-chunks 10 \
  --at-epoch-ms 2500 \
  --token-source-kind cli >"${future_region_cached_stdout}" \
  2>"${future_region_cached_stderr}"
future_region_cached="$(tail -n 1 "${future_region_cached_stdout}")"
future_region_cached_cache_hit="$(
  if rg -q "context pack cache hit" "${future_region_cached_stderr}"; then
    printf 'true'
  else
    printf 'false'
  fi
)"
rm -f "${future_region_cached_stdout}" "${future_region_cached_stderr}"
historical_region_trace_hits="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "What is the current region of ${fact_subject}?" \
    --limit-chunks 10 \
    --at-epoch-ms 1500 \
    --token-source-kind proof_context_pack \
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

run_amai_capture project register \
  --code "${related_project_code}" \
  --display-name "Project Beta" \
  --repo-root "$PWD/fixtures/project_beta" >/dev/null

run_amai_capture namespace ensure \
  --project "${related_project_code}" \
  --code "${namespace_code}" \
  --display-name "${namespace_code}" \
  --retrieval-mode local_strict >/dev/null

related_fact_subject="infra.related.region.${suffix}"
related_fact_id="$(
  run_amai_capture memory create-card \
    --project "${related_project_code}" \
    --namespace "${namespace_code}" \
    --title "Related region ${suffix}" \
    --summary "Related project region is eu-central." \
    --body "related region body" \
    --provenance-json '{"source_event_ids":["event:related"],"artifact_refs":["artifact://related"],"message_refs":["thread:related"],"evidence_span":{"kind":"memory_card","case":"manual_related"},"source_kind":"manual_stage5"}' \
    --fact-subject "${related_fact_subject}" \
    --fact-predicate current_region \
    --fact-object eu-central \
    --truth-state current \
    --verification-state verified \
    --status active \
    --observed-at-epoch-ms 1000 \
    --recorded-at-epoch-ms 1001 \
    --valid-from-epoch-ms 1000 \
    --last-verified-at-epoch-ms 1002 | extract_uuid
)"

run_amai_capture relation add \
  --source "${project_code}" \
  --target "${related_project_code}" \
  --relation-type shared_runtime \
  --shared-contour "stage5_temporal_${suffix}" \
  --access-mode local_plus_related >/dev/null

run_amai_capture namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name "${namespace_code}" \
  --retrieval-mode local_plus_related >/dev/null

related_hits_warm_cached="$(
  run_amai_last_line context pack \
    --project "${project_code}" \
    --namespace "${namespace_code}" \
    --query "${related_fact_subject}" \
    --limit-chunks 10 \
    --token-source-kind cli
)"

run_amai_capture relation update \
  --source "${project_code}" \
  --target "${related_project_code}" \
  --relation-type shared_runtime \
  --shared-contour "stage5_temporal_${suffix}" \
  --access-mode local_strict \
  --relation-status forbidden \
  --override-reason "stage5 temporal cache invalidation proof ${suffix}" >/dev/null

run_amai_capture namespace ensure \
  --project "${project_code}" \
  --code "${namespace_code}" \
  --display-name "${namespace_code}" \
  --retrieval-mode local_strict >/dev/null

related_hits_after_policy_change_stdout="$(mktemp)"
related_hits_after_policy_change_stderr="$(mktemp)"
cargo run --quiet -- context pack \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --query "${related_fact_subject}" \
  --limit-chunks 10 \
  --token-source-kind verify_context_pack >"${related_hits_after_policy_change_stdout}" \
  2>"${related_hits_after_policy_change_stderr}"
related_hits_after_policy_change="$(tail -n 1 "${related_hits_after_policy_change_stdout}")"
related_hits_after_policy_change_cache_hit="$(
  if rg -q "context pack cache hit" "${related_hits_after_policy_change_stderr}"; then
    printf 'true'
  else
    printf 'false'
  fi
)"
rm -f "${related_hits_after_policy_change_stdout}" "${related_hits_after_policy_change_stderr}"

python3 - <<'PY' \
  "${namespace_code}" \
  "${historical_region_hits}" \
  "${future_region_hits}" \
  "${future_only_backflow_hits}" \
  "${historical_region_cached_first}" \
  "${historical_region_cached_first_cache_hit}" \
  "${historical_region_cached_second}" \
  "${historical_region_cached_second_cache_hit}" \
  "${future_region_cached}" \
  "${future_region_cached_cache_hit}" \
  "${historical_region_trace_hits}" \
  "${old_fact_id}" \
  "${new_fact_id}" \
  "${retracted_fact_payload}" \
  "${retracted_historical_hits}" \
  "${retracted_future_hits}" \
  "${retracted_latest_hits}" \
  "${retracted_fact_id}" \
  "${owner_hits}" \
  "${owner_current_id}" \
  "${owner_conflicted_id}" \
  "${related_hits_warm_cached}" \
  "${related_hits_after_policy_change}" \
  "${related_hits_after_policy_change_cache_hit}" \
  "${related_fact_id}"
import json
import sys

namespace_code = sys.argv[1]
historical_region_hits = json.loads(sys.argv[2])
future_region_hits = json.loads(sys.argv[3])
future_only_backflow_hits = json.loads(sys.argv[4])
historical_region_cached_first = json.loads(sys.argv[5])
historical_region_cached_first_cache_hit = sys.argv[6] == "true"
historical_region_cached_second = json.loads(sys.argv[7])
historical_region_cached_second_cache_hit = sys.argv[8] == "true"
future_region_cached = json.loads(sys.argv[9])
future_region_cached_cache_hit = sys.argv[10] == "true"
historical_region_trace_hits = json.loads(sys.argv[11])
old_fact_id = sys.argv[12]
new_fact_id = sys.argv[13]
retracted_fact_payload = json.loads(sys.argv[14])
retracted_historical_hits = json.loads(sys.argv[15])
retracted_future_hits = json.loads(sys.argv[16])
retracted_latest_hits = json.loads(sys.argv[17])
retracted_fact_id = sys.argv[18]
owner_hits = json.loads(sys.argv[19])
owner_current_id = sys.argv[20]
owner_conflicted_id = sys.argv[21]
related_hits_warm_cached = json.loads(sys.argv[22])
related_hits_after_policy_change = json.loads(sys.argv[23])
related_hits_after_policy_change_cache_hit = sys.argv[24] == "true"
related_fact_id = sys.argv[25]

def memory_card_ids(payload):
    retrieval = payload.get("retrieval")
    if not isinstance(retrieval, dict):
        return []
    return [item["memory_card_id"] for item in retrieval.get("memory_cards", [])]

assert old_fact_id in memory_card_ids(historical_region_hits), historical_region_hits
assert new_fact_id not in memory_card_ids(historical_region_hits), historical_region_hits
assert new_fact_id in memory_card_ids(future_region_hits), future_region_hits
assert old_fact_id not in memory_card_ids(future_region_hits), future_region_hits
assert memory_card_ids(future_only_backflow_hits) == [], future_only_backflow_hits
assert old_fact_id in memory_card_ids(historical_region_cached_first), historical_region_cached_first
assert historical_region_cached_first_cache_hit is False, historical_region_cached_first_cache_hit
assert historical_region_cached_second_cache_hit is True, historical_region_cached_second_cache_hit
assert historical_region_cached_second["cache_reuse_reference"]["state"] == "same_thread_context_pack_replay", historical_region_cached_second
assert new_fact_id in memory_card_ids(future_region_cached), future_region_cached
assert old_fact_id not in memory_card_ids(future_region_cached), future_region_cached
assert future_region_cached_cache_hit is False, future_region_cached_cache_hit
assert historical_region_trace_hits["decision_trace"]["rerank_legality_relevance"]["temporal_legality"]["status"] == "applied_exact_time_slice", historical_region_trace_hits
assert historical_region_trace_hits["decision_trace"]["rerank_legality_relevance"]["temporal_legality"]["prefilter_memory_cards"] >= 2, historical_region_trace_hits
assert historical_region_trace_hits["decision_trace"]["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_cards_by_temporal_window"] >= 1, historical_region_trace_hits
assert any(
    item.get("memory_card_id") == new_fact_id
    or item.get("title", "").startswith("Region new ")
    for item in historical_region_trace_hits["decision_trace"]["rerank_legality_relevance"]["temporal_legality"]["excluded_memory_card_candidates"]
), historical_region_trace_hits

assert retracted_fact_payload["truth_state"] == "retracted", retracted_fact_payload
assert retracted_fact_payload["valid_to_epoch_ms"] == 2000, retracted_fact_payload
assert retracted_fact_id in memory_card_ids(retracted_historical_hits), retracted_historical_hits
assert retracted_fact_id not in memory_card_ids(retracted_future_hits), retracted_future_hits
assert retracted_fact_id not in memory_card_ids(retracted_latest_hits), retracted_latest_hits

owner_ids = memory_card_ids(owner_hits)
assert owner_ids and owner_ids[0] == owner_current_id, owner_hits
assert owner_conflicted_id in owner_ids, owner_hits
assert related_fact_id in memory_card_ids(related_hits_warm_cached), related_hits_warm_cached
assert related_hits_after_policy_change_cache_hit is False, related_hits_after_policy_change_cache_hit
assert related_fact_id not in memory_card_ids(related_hits_after_policy_change), related_hits_after_policy_change

print(json.dumps({
    "namespace": namespace_code,
    "generic_old_new": "pass",
    "future_only_backflow": "pass",
    "temporal_cache_key_isolation": "pass",
    "temporal_trace_explainability": "pass",
    "retract_temporal_window": "pass",
    "mixed_state_ranking": "pass",
    "verify_context_pack_policy_bypass": "pass",
    "owner_result_order": owner_ids[:3],
}, ensure_ascii=False))
PY

printf 'proof_semantic_temporal_manual_acceptance: ok\n'
