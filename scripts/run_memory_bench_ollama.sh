#!/usr/bin/env bash
set -euo pipefail

MODEL="${MODEL:-phi3:mini}"
REQUESTS_PATH=""
PREDICTIONS_PATH=""
STATUS_PATH=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model)
      MODEL="$2"
      shift 2
      ;;
    --requests)
      REQUESTS_PATH="$2"
      shift 2
      ;;
    --predictions)
      PREDICTIONS_PATH="$2"
      shift 2
      ;;
    *)
      echo "Unknown аргумент: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$REQUESTS_PATH" || -z "$PREDICTIONS_PATH" ]]; then
  echo "Usage: $0 --requests <requests.jsonl> --predictions <predictions.jsonl> [--model <model>]" >&2
  exit 2
fi

if [[ ! -f "$REQUESTS_PATH" ]]; then
  echo "requests.jsonl не найден: $REQUESTS_PATH" >&2
  exit 2
fi

mkdir -p "$(dirname "$PREDICTIONS_PATH")"
touch "$PREDICTIONS_PATH"
STATUS_PATH="${PREDICTIONS_PATH}.status.json"

total_requests="$(wc -l < "$REQUESTS_PATH" | tr -d ' ')"
completed_count=0
last_case_id=""

write_status() {
  local stage="$1"
  local now_ms
  now_ms="$(date +%s%3N)"
  jq -cn \
    --arg stage "$stage" \
    --arg model "$MODEL" \
    --arg requests_path "$REQUESTS_PATH" \
    --arg predictions_path "$PREDICTIONS_PATH" \
    --arg last_case_id "$last_case_id" \
    --argjson total "$total_requests" \
    --argjson completed "$completed_count" \
    --argjson updated_at_epoch_ms "$now_ms" \
    '{stage:$stage,model:$model,requests_path:$requests_path,predictions_path:$predictions_path,total_requests:$total,completed:$completed,last_case_id:$last_case_id,updated_at_epoch_ms:$updated_at_epoch_ms}' > "$STATUS_PATH"
}

declare -A seen_case_ids=()
while IFS= read -r existing_line; do
  existing_id="$(printf '%s' "$existing_line" | jq -r '.case_id // empty')"
  if [[ -n "$existing_id" ]]; then
    seen_case_ids["$existing_id"]=1
    completed_count=$((completed_count + 1))
  fi
done < "$PREDICTIONS_PATH"

echo "Running Ollama model: $MODEL" >&2
echo "Requests: $REQUESTS_PATH" >&2
echo "Predictions: $PREDICTIONS_PATH" >&2

write_status "running"

while IFS= read -r line; do
  case_id="$(printf '%s' "$line" | jq -r '.case_id // empty')"
  prompt="$(printf '%s' "$line" | jq -r '.prompt // empty')"
  if [[ -z "$case_id" || -z "$prompt" ]]; then
    continue
  fi
  if [[ -n "${seen_case_ids[$case_id]:-}" ]]; then
    continue
  fi
  raw_output="$(printf '%s' "$prompt" | ollama run "$MODEL" --nowordwrap)"
  predicted="$(printf '%s' "$raw_output" | tr '\n' ' ' | sed -E 's/[[:space:]]+/ /g' | sed -E 's/^ +| +$//g')"
  jq -cn --arg case_id "$case_id" --arg predicted "$predicted" '{case_id:$case_id,predicted_answer:$predicted}' >> "$PREDICTIONS_PATH"
  printf '\n' >> "$PREDICTIONS_PATH"
  seen_case_ids["$case_id"]=1
  completed_count=$((completed_count + 1))
  last_case_id="$case_id"
  write_status "running"
done < "$REQUESTS_PATH"

write_status "done"
echo "Done: wrote $(wc -l < "$PREDICTIONS_PATH") predictions" >&2
