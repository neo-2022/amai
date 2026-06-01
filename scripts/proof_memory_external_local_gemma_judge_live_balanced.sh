#!/usr/bin/env bash
set -euo pipefail
trap 'echo "proof_memory_external_local_gemma_judge_live_balanced.sh failed at line $LINENO" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

DATASET="longmemeval_s_cleaned"
RAW_DATASET="${AMAI_LONGMEMEVAL_RAW_DATASET:-$REPO_ROOT/state/external-benchmarks/datasets/${DATASET}.json}"
OUT_ROOT="${AMAI_LOCAL_JUDGE_BALANCED_OUT_DIR:-$REPO_ROOT/tmp/external-memory-local-gemma-live-balanced/$DATASET}"
SOURCE="$OUT_ROOT/source-balanced.json"
PREPARED="$OUT_ROOT/prepared"
MODEL="${AMAI_LOCAL_JUDGE_MODEL:-gemma4:e4b}"
OLLAMA_BASE_URL="${AMAI_LOCAL_JUDGE_OLLAMA_BASE_URL:-http://127.0.0.1:11434}"
EVAL_RESULTS="$PREPARED/local-gemma-eval-results.jsonl"
SUMMARY="$PREPARED/local-gemma-judge-summary.json"
SCORE="$PREPARED/local-gemma-score.json"

QUESTION_TYPES_JSON='[
  "single-session-user",
  "single-session-preference",
  "single-session-assistant",
  "multi-session",
  "temporal-reasoning",
  "knowledge-update"
]'

echo "== Amai external memory local Gemma LongMemEval balanced proof =="

for required_tool in cargo jq wc curl; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    echo "required tool not found: $required_tool" >&2
    exit 2
  fi
done

if [[ ! -r "$RAW_DATASET" ]]; then
  echo "LongMemEval raw dataset is not readable: $RAW_DATASET" >&2
  exit 3
fi

curl -fsS "$OLLAMA_BASE_URL/api/tags" \
  | jq -e --arg model "$MODEL" 'any(.models[]?; .name == $model)' >/dev/null

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
  --namespace external_memory_local_gemma_live_balanced_longmemeval \
  --status "$PREPARED/status.json"

jq -e '
  .stage == "done"
  and .total_requests == 6
  and .completed == 6
' "$PREPARED/status.json" >/dev/null

cargo run --quiet -- benchmark external-memory-local-judge \
  --cases "$PREPARED/cases.jsonl" \
  --predictions "$PREPARED/predictions.jsonl" \
  --eval-results "$EVAL_RESULTS" \
  --summary "$SUMMARY" \
  --ollama-base-url "$OLLAMA_BASE_URL" \
  --model "$MODEL"

jq -e --arg model "$MODEL" --arg ollama_base_url "$OLLAMA_BASE_URL" '
  .boundary_version == "external_memory_local_judge_execution_v1"
  and .bench == "longmemeval"
  and .dataset == "longmemeval_s_cleaned"
  and .status == "executed"
  and .case_count == 6
  and .prediction_count == 6
  and .eval_entries_written == 6
  and .live_local_llm_judge_run == true
  and .local_eval_log_materialized == true
  and .requested_model == $model
  and .default_local_model == "gemma4:e4b"
  and .local_judge_source_kind == "local_ollama_llm_judge_execution"
  and .local_judge_transport == "ollama_api_chat"
  and .ollama_base_url == ($ollama_base_url | sub("/+$"; ""))
  and .official_upstream_provenance_eligible == false
  and .official_upstream_scorer_parity == false
' "$SUMMARY" >/dev/null

eval_line_count="$(wc -l < "$EVAL_RESULTS" | tr -d '[:space:]')"
if [[ "$eval_line_count" -ne 6 ]]; then
  echo "expected 6 local Gemma eval entries, got $eval_line_count" >&2
  exit 6
fi

jq -s -e --arg model "$MODEL" '
  length == 6
  and all(.[]; (
    (.question_id | type) == "string"
    and (.hypothesis | type) == "string"
    and .autoeval_label.model == $model
    and ((.autoeval_label.label | type) == "boolean")
    and .local_judge_provenance.provenance_version == "external_memory_local_judge_provenance_v1"
    and .local_judge_provenance.source_kind == "local_ollama_llm_judge_execution"
    and .local_judge_provenance.judge_transport == "ollama_api_chat"
    and .local_judge_provenance.prompt_template_source_kind == "embedded_from_upstream_evaluate_qa_py"
    and (.local_judge_provenance.prompt_sha256 | test("^[0-9a-f]{64}$"))
    and .local_judge_provenance.official_upstream_provenance_eligible == false
  ))
' "$EVAL_RESULTS" >/dev/null

cargo run --quiet -- benchmark external-memory-local-score \
  --cases "$PREPARED/cases.jsonl" \
  --eval-results "$EVAL_RESULTS" \
  --output "$SCORE" \
  --expected-model "$MODEL"

jq -e --arg model "$MODEL" '
  .boundary_version == "external_memory_local_score_reconciliation_v1"
  and .bench == "longmemeval"
  and .dataset == "longmemeval_s_cleaned"
  and .status == "reconciled"
  and .case_count == 6
  and .eval_results_present == true
  and .eval_entries_total == 6
  and .valid_eval_entries == 6
  and .invalid_eval_entries == 0
  and .missing_required_fields == 0
  and .duplicate_question_ids == 0
  and .unexpected_eval_results == 0
  and .missing_case_results == 0
  and .model_mismatch_count == 0
  and .expected_model == $model
  and .all_official_task_types_present == true
  and .local_eval_log_contract_valid == true
  and .local_metrics_reconciled == true
  and .official_upstream_scorer_parity == false
  and (.metrics.overall_accuracy | type) == "number"
  and (.metrics.task_averaged_accuracy | type) == "number"
  and (
    if .metrics.abstention_count == 0
    then .metrics.abstention_accuracy == null
    else (.metrics.abstention_accuracy | type) == "number"
    end
  )
' "$SCORE" >/dev/null

echo "== Done: local Gemma LongMemEval balanced proof green (provider-independent, non-official lane) =="
