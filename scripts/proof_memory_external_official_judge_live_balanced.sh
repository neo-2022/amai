#!/usr/bin/env bash
set -euo pipefail
trap 'echo "proof_memory_external_official_judge_live_balanced.sh failed at line $LINENO" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

DATASET="longmemeval_s_cleaned"
RAW_DATASET="${AMAI_LONGMEMEVAL_RAW_DATASET:-$REPO_ROOT/state/external-benchmarks/datasets/${DATASET}.json}"
OUT_ROOT="$REPO_ROOT/tmp/external-memory-official-live-balanced/$DATASET"
SOURCE="$OUT_ROOT/source-balanced.json"
PREPARED="$OUT_ROOT/prepared"
API_KEY_ENV="${AMAI_OFFICIAL_JUDGE_API_KEY_ENV:-OPENAI_API_KEY}"

QUESTION_TYPES_JSON='[
  "single-session-user",
  "single-session-preference",
  "single-session-assistant",
  "multi-session",
  "temporal-reasoning",
  "knowledge-update"
]'

echo "== Amai external memory official LongMemEval balanced live proof =="

for required_tool in cargo jq wc; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    echo "required tool not found: $required_tool" >&2
    exit 2
  fi
done

if [[ ! -r "$RAW_DATASET" ]]; then
  echo "LongMemEval raw dataset is not readable: $RAW_DATASET" >&2
  exit 3
fi

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

jq --argjson question_types "$QUESTION_TYPES_JSON" '
  [ $question_types[] as $question_type
    | first(.[] | select(.question_type == $question_type)) ]
' "$RAW_DATASET" >"$SOURCE"

jq -e --argjson question_types "$QUESTION_TYPES_JSON" '
  length == ($question_types | length)
  and all(.[]; type == "object")
  and ([.[].question_type] | sort) == ($question_types | sort)
  and all(.[]; (.question_id | type) == "string" and (.question | type) == "string" and ((.answer | type) as $t | $t == "string" or $t == "number" or $t == "boolean"))
' "$SOURCE" >/dev/null

cargo run --quiet -- benchmark external-memory-prepare \
  --benchmark longmemeval \
  --dataset "$DATASET" \
  --source-path "$SOURCE" \
  --output-dir "$PREPARED"

jq -e '
  .dataset_code == "longmemeval_s_cleaned"
  and .dataset_path_source_kind == "explicit_source_path"
  and .stats.total == 6
  and .stats.missing_question == 0
  and .stats.missing_context == 0
  and .stats.missing_answer == 0
  and .stats.missing_id == 0
' "$PREPARED/manifest.json" >/dev/null

jq -s -e --argjson question_types "$QUESTION_TYPES_JSON" '
  length == ($question_types | length)
  and ([.[].metadata.question_type] | sort) == ($question_types | sort)
' "$PREPARED/cases.jsonl" >/dev/null

cargo run --quiet -- benchmark external-memory-run \
  --requests "$PREPARED/requests.jsonl" \
  --predictions "$PREPARED/predictions.jsonl" \
  --project amai \
  --namespace external_memory_official_live_balanced_longmemeval \
  --status "$PREPARED/status.json"

jq -e '
  .stage == "done"
  and .total_requests == 6
  and .completed == 6
' "$PREPARED/status.json" >/dev/null

AMAI_EXTERNAL_MEMORY_REAL_DIR="$PREPARED" \
AMAI_OFFICIAL_JUDGE_API_KEY_ENV="$API_KEY_ENV" \
  ./scripts/proof_memory_external_official_judge_live_bounded.sh

if [[ -n "${!API_KEY_ENV:-}" ]]; then
  jq -e '
    .all_official_task_types_present == true
    and .status == "reconciled"
    and .official_metrics_reconciled == true
    and .official_upstream_scorer_parity == false
  ' "$PREPARED/official-live-score-reconcile.json" >/dev/null
else
  jq -e '
    .status == "blocked"
    and (.validation_blocking_reasons | index("official_judge_api_key_not_materialized") != null)
    and .official_eval_log_materialized == false
    and .official_upstream_scorer_parity == false
  ' "$PREPARED/official-live-judge-summary.json" >/dev/null
  if grep -q 'REDACTED_OFFICIAL_JUDGE_API_KEY' "$PREPARED/official-live-judge-summary.json"; then
    echo "balanced missing-key summary must not contain redaction marker when no key value was materialized" >&2
    exit 7
  fi
  test ! -e "$PREPARED/official-live-eval-results.jsonl"
fi

echo "== Done: balanced LongMemEval official live proof guarded across all six question types =="
