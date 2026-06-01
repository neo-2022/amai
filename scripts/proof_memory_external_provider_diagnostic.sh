#!/usr/bin/env bash
set -euo pipefail
trap 'echo "proof_memory_external_provider_diagnostic.sh failed at line $LINENO" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

API_BASE_URL_RAW="${AMAI_PROVIDER_DIAGNOSTIC_API_BASE_URL:-}"
CLIENT_MODEL="${AMAI_PROVIDER_DIAGNOSTIC_MODEL:-}"
OFFICIAL_MODEL="${AMAI_PROVIDER_DIAGNOSTIC_OFFICIAL_MODEL:-gpt-4o-2024-08-06}"
API_KEY_ENV="${AMAI_PROVIDER_DIAGNOSTIC_API_KEY_ENV:-OPENAI_API_KEY}"
PROMPT="${AMAI_PROVIDER_DIAGNOSTIC_PROMPT:-reply with ok only}"
OUT_DIR="${AMAI_PROVIDER_DIAGNOSTIC_OUT_DIR:-$REPO_ROOT/tmp/external-memory-provider-diagnostic}"

MODELS_JSON="$OUT_DIR/models.json"
RESPONSES_PAYLOAD="$OUT_DIR/responses-payload.json"
RESPONSES_JSON="$OUT_DIR/responses-probe.json"
CHAT_CLIENT_PAYLOAD="$OUT_DIR/chat-client-payload.json"
CHAT_CLIENT_JSON="$OUT_DIR/chat-client-probe.json"
CHAT_OFFICIAL_PAYLOAD="$OUT_DIR/chat-official-payload.json"
CHAT_OFFICIAL_JSON="$OUT_DIR/chat-official-probe.json"
SUMMARY_JSON="$OUT_DIR/summary.json"

normalize_api_base_url() {
  printf '%s' "$1" \
    | sed -e 's/^[[:space:]]*//' \
          -e 's/[[:space:]]*$//' \
          -e 's#/chat/completions/*$##' \
          -e 's#/responses/*$##' \
          -e 's#/*$##'
}

extract_error_code() {
  local file="$1"
  jq -r '.error.code // empty' "$file"
}

extract_error_message() {
  local file="$1"
  jq -r '.error.message // empty' "$file"
}

extract_responses_text() {
  local file="$1"
  jq -r '
    .output_text
    // (.output[]? | select(.type == "message") | .content[]? | select(.type == "output_text") | .text)
    // (.output | select(type == "object") | .text)
    // empty
  ' "$file" | head -n 1
}

echo "== Amai external memory provider diagnostic =="

for required_tool in cargo curl jq; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    echo "required tool not found: $required_tool" >&2
    exit 2
  fi
done

API_BASE_URL="$(normalize_api_base_url "$API_BASE_URL_RAW")"
if [[ -z "${API_BASE_URL}" ]]; then
  echo "AMAI_PROVIDER_DIAGNOSTIC_API_BASE_URL must be set to an OpenAI-compatible /v1 base URL" >&2
  exit 2
fi

if [[ -z "${CLIENT_MODEL//[[:space:]]/}" ]]; then
  echo "AMAI_PROVIDER_DIAGNOSTIC_MODEL must be set" >&2
  exit 2
fi

if [[ -z "${API_KEY_ENV//[[:space:]]/}" ]]; then
  echo "AMAI_PROVIDER_DIAGNOSTIC_API_KEY_ENV must name an environment variable" >&2
  exit 2
fi

API_KEY_VALUE="${!API_KEY_ENV:-}"
if [[ -z "${API_KEY_VALUE//[[:space:]]/}" ]]; then
  echo "configured API key env is empty: $API_KEY_ENV" >&2
  exit 2
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

responses_endpoint="${API_BASE_URL}/responses"
chat_endpoint="${API_BASE_URL}/chat/completions"
models_endpoint="${API_BASE_URL}/models"

models_http_status="$(
  curl -sS "$models_endpoint" \
    -H "Authorization: Bearer $API_KEY_VALUE" \
    -o "$MODELS_JSON" \
    -w '%{http_code}'
)"

if [[ "$models_http_status" != "200" ]]; then
  echo "models probe failed with HTTP $models_http_status" >&2
  jq -c '.error // .' "$MODELS_JSON" >&2 || true
  exit 3
fi

jq -n \
  --arg model "$CLIENT_MODEL" \
  --arg input "$PROMPT" \
  '{model: $model, input: $input}' >"$RESPONSES_PAYLOAD"

responses_http_status="$(
  curl -sS "$responses_endpoint" \
    -H "Authorization: Bearer $API_KEY_VALUE" \
    -H 'Content-Type: application/json' \
    -d @"$RESPONSES_PAYLOAD" \
    -o "$RESPONSES_JSON" \
    -w '%{http_code}'
)"

jq -n \
  --arg model "$CLIENT_MODEL" \
  --arg prompt "$PROMPT" \
  '{model: $model, messages: [{role: "user", content: $prompt}], max_tokens: 8}' >"$CHAT_CLIENT_PAYLOAD"

chat_client_http_status="$(
  curl -sS "$chat_endpoint" \
    -H "Authorization: Bearer $API_KEY_VALUE" \
    -H 'Content-Type: application/json' \
    -d @"$CHAT_CLIENT_PAYLOAD" \
    -o "$CHAT_CLIENT_JSON" \
    -w '%{http_code}'
)"

jq -n \
  --arg model "$OFFICIAL_MODEL" \
  --arg prompt "$PROMPT" \
  '{model: $model, messages: [{role: "user", content: $prompt}], max_tokens: 8}' >"$CHAT_OFFICIAL_PAYLOAD"

chat_official_http_status="$(
  curl -sS "$chat_endpoint" \
    -H "Authorization: Bearer $API_KEY_VALUE" \
    -H 'Content-Type: application/json' \
    -d @"$CHAT_OFFICIAL_PAYLOAD" \
    -o "$CHAT_OFFICIAL_JSON" \
    -w '%{http_code}'
)"

client_model_listed="$(jq -r --arg model "$CLIENT_MODEL" 'any(.data[]?; .id == $model)' "$MODELS_JSON")"
official_model_listed="$(jq -r --arg model "$OFFICIAL_MODEL" 'any(.data[]?; .id == $model)' "$MODELS_JSON")"
available_models_count="$(jq -r '(.data // []) | length' "$MODELS_JSON")"
available_models_json="$(jq -c '[.data[]?.id]' "$MODELS_JSON")"

responses_success=false
if [[ "$responses_http_status" == "200" ]]; then
  responses_success=true
fi

chat_client_success=false
if [[ "$chat_client_http_status" == "200" ]]; then
  chat_client_success=true
fi

chat_official_success=false
if [[ "$chat_official_http_status" == "200" ]]; then
  chat_official_success=true
fi

likely_blocker="unknown"
verdict="blocked"
if [[ "$responses_success" == "true" && "$official_model_listed" != "true" ]]; then
  likely_blocker="official_judge_model_not_exposed_by_provider_catalog"
  verdict="provider_live_but_official_judge_model_unavailable"
elif [[ "$responses_success" == "true" && "$chat_client_success" != "true" ]]; then
  likely_blocker="provider_responses_path_live_but_chat_completions_path_not_live"
  verdict="provider_live_but_chat_completions_probe_failed"
elif [[ "$responses_success" == "true" && "$chat_client_success" == "true" ]]; then
  likely_blocker="official_judge_model_or_contract_mismatch_only"
  verdict="provider_live_for_client_and_chat_probe"
elif [[ "$responses_success" != "true" ]]; then
  likely_blocker="provider_responses_path_not_live"
  verdict="provider_live_path_not_confirmed"
fi

responses_text_sample="$(extract_responses_text "$RESPONSES_JSON")"
chat_client_content="$(jq -r '.choices[0].message.content // empty' "$CHAT_CLIENT_JSON")"
chat_official_content="$(jq -r '.choices[0].message.content // empty' "$CHAT_OFFICIAL_JSON")"

jq -n \
  --arg boundary_version "external_memory_provider_diagnostic_v1" \
  --arg api_base_url "$API_BASE_URL" \
  --arg client_model "$CLIENT_MODEL" \
  --arg official_model "$OFFICIAL_MODEL" \
  --argjson available_models "$available_models_json" \
  --argjson available_models_count "$available_models_count" \
  --arg client_model_listed "$client_model_listed" \
  --arg official_model_listed "$official_model_listed" \
  --arg responses_http_status "$responses_http_status" \
  --arg responses_success "$responses_success" \
  --arg responses_text_sample "$responses_text_sample" \
  --arg responses_error_code "$(extract_error_code "$RESPONSES_JSON")" \
  --arg responses_error_message "$(extract_error_message "$RESPONSES_JSON")" \
  --arg responses_id "$(jq -r '.id // empty' "$RESPONSES_JSON")" \
  --arg responses_status "$(jq -r '.status // empty' "$RESPONSES_JSON")" \
  --arg chat_client_http_status "$chat_client_http_status" \
  --arg chat_client_success "$chat_client_success" \
  --arg chat_client_error_code "$(extract_error_code "$CHAT_CLIENT_JSON")" \
  --arg chat_client_error_message "$(extract_error_message "$CHAT_CLIENT_JSON")" \
  --arg chat_client_content "$chat_client_content" \
  --arg chat_official_http_status "$chat_official_http_status" \
  --arg chat_official_success "$chat_official_success" \
  --arg chat_official_error_code "$(extract_error_code "$CHAT_OFFICIAL_JSON")" \
  --arg chat_official_error_message "$(extract_error_message "$CHAT_OFFICIAL_JSON")" \
  --arg chat_official_content "$chat_official_content" \
  --arg likely_blocker "$likely_blocker" \
  --arg verdict "$verdict" \
  '{
    boundary_version: $boundary_version,
    api_base_url: $api_base_url,
    provider_diagnostic_model: $client_model,
    official_judge_model: $official_model,
    available_models_count: ($available_models_count | tonumber),
    available_models: $available_models,
    provider_model_listed: ($client_model_listed == "true"),
    official_judge_model_listed: ($official_model_listed == "true"),
    responses_probe: {
      http_status: ($responses_http_status | tonumber),
      success: ($responses_success == "true"),
      response_id: $responses_id,
      response_status: $responses_status,
      output_text_sample: $responses_text_sample,
      error_code: $responses_error_code,
      error_message: $responses_error_message
    },
    chat_completions_provider_model_probe: {
      http_status: ($chat_client_http_status | tonumber),
      success: ($chat_client_success == "true"),
      content_sample: $chat_client_content,
      error_code: $chat_client_error_code,
      error_message: $chat_client_error_message
    },
    chat_completions_official_model_probe: {
      http_status: ($chat_official_http_status | tonumber),
      success: ($chat_official_success == "true"),
      content_sample: $chat_official_content,
      error_code: $chat_official_error_code,
      error_message: $chat_official_error_message
    },
    likely_blocker: $likely_blocker,
    verdict: $verdict
  }' >"$SUMMARY_JSON"

cargo run --quiet -- benchmark external-memory-secret-scan \
  --output-dir "$OUT_DIR" \
  --secret-env "$API_KEY_ENV" >/dev/null

jq -e '
  .boundary_version == "external_memory_provider_diagnostic_v1"
  and (.available_models_count >= 1)
  and .provider_model_listed == true
  and .responses_probe.success == true
' "$SUMMARY_JSON" >/dev/null

cat "$SUMMARY_JSON"
echo
echo "== Done: provider diagnostic captured live service path and official-judge compatibility boundary =="
