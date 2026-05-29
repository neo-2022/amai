#!/usr/bin/env bash
set -euo pipefail

OLLAMA_BASE_URL="${OLLAMA_BASE_URL:-http://127.0.0.1:11434}"
MODEL="${OLLAMA_CHAT_MODEL:-}"
SYSTEM_PROMPT=""
PROMPT=""
OUTPUT_MODE="text"
TEMPERATURE=""
TOP_P="${OLLAMA_TOP_P:-}"
TOP_K="${OLLAMA_TOP_K:-}"
THINKING_MODE="${OLLAMA_THINKING_MODE:-off}"
CONNECT_TIMEOUT_SECONDS="${OLLAMA_CONNECT_TIMEOUT_SECONDS:-5}"
TOTAL_TIMEOUT_SECONDS="${OLLAMA_TOTAL_TIMEOUT_SECONDS:-75}"
RETRY_COUNT="${OLLAMA_RETRY_COUNT:-1}"
RETRY_DELAY_SECONDS="${OLLAMA_RETRY_DELAY_SECONDS:-1}"
FALLBACK_MODEL="${OLLAMA_FALLBACK_MODEL:-}"
FALLBACK_EXIT_CODES="${OLLAMA_FALLBACK_EXIT_CODES:-7,28}"
STATE_DB="${VSCODE_STATE_DB:-$HOME/.config/Code/User/globalStorage/state.vscdb}"

usage() {
  cat >&2 <<'EOF'
Usage:
  ollama_chat.sh [--model <name>] [--system <text>] [--prompt <text>] [--json]
                 [--temperature <value>] [--top-p <value>] [--top-k <value>]
                 [--thinking <on|off>]
  echo "Your prompt" | ollama_chat.sh [--model <name>] [--system <text>] [--json]
                 [--temperature <value>] [--top-p <value>] [--top-k <value>]
                 [--thinking <on|off>]

Model resolution order:
  1. --model
  2. OLLAMA_CHAT_MODEL
  3. Current VS Code selected chat model, if it is ollama/Ollama/<model>

Environment:
  OLLAMA_BASE_URL      Ollama API base URL. Default: http://127.0.0.1:11434
  OLLAMA_CHAT_MODEL    Default model if --model is omitted.
  OLLAMA_TOP_P         Optional top_p sampling value.
  OLLAMA_TOP_K         Optional top_k sampling value.
  OLLAMA_THINKING_MODE Thinking mode: on|off. Default: off.
  OLLAMA_CONNECT_TIMEOUT_SECONDS  Curl connect timeout. Default: 5
  OLLAMA_TOTAL_TIMEOUT_SECONDS    Total request timeout. Default: 75
  OLLAMA_RETRY_COUNT              Curl retry count for transient errors. Default: 1
  OLLAMA_RETRY_DELAY_SECONDS      Delay between retries. Default: 1
  OLLAMA_FALLBACK_MODEL           Optional explicit fallback model. Used only for configured failure exit codes.
  OLLAMA_FALLBACK_EXIT_CODES      Comma-separated curl exit codes that may trigger fallback. Default: 7,28
  VSCODE_STATE_DB      Override VS Code state.vscdb path.
EOF
  exit 2
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

resolve_vscode_model() {
  local raw=""

  if ! command -v sqlite3 >/dev/null 2>&1; then
    return 1
  fi

  if [[ ! -f "$STATE_DB" ]]; then
    return 1
  fi

  raw="$(sqlite3 "$STATE_DB" "select value from ItemTable where key='chat.currentLanguageModel.panel';" 2>/dev/null || true)"
  raw="$(trim "$raw")"
  if [[ "$raw" == ollama/Ollama/* ]]; then
    printf '%s' "${raw#ollama/Ollama/}"
    return 0
  fi

  return 1
}

resolve_model() {
  if [[ -n "$MODEL" ]]; then
    printf '%s' "$MODEL"
    return 0
  fi

  if resolve_vscode_model >/dev/null 2>&1; then
    resolve_vscode_model
    return 0
  fi

  echo "Не удалось определить Ollama model. Передайте --model или задайте OLLAMA_CHAT_MODEL." >&2
  echo "Доступные модели:" >&2
  curl -fsS "$OLLAMA_BASE_URL/api/tags" | jq -r '.models[].name' >&2 || true
  return 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model)
      MODEL="$2"
      shift 2
      ;;
    --system)
      SYSTEM_PROMPT="$2"
      shift 2
      ;;
    --prompt)
      PROMPT="$2"
      shift 2
      ;;
    --json)
      OUTPUT_MODE="json"
      shift
      ;;
    --temperature)
      TEMPERATURE="$2"
      shift 2
      ;;
    --top-p)
      TOP_P="$2"
      shift 2
      ;;
    --top-k)
      TOP_K="$2"
      shift 2
      ;;
    --thinking)
      THINKING_MODE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      ;;
  esac
done

if [[ -z "$PROMPT" && ! -t 0 ]]; then
  PROMPT="$(cat)"
fi

PROMPT="$(trim "$PROMPT")"
if [[ -z "$PROMPT" ]]; then
  echo "Пустой prompt. Передайте --prompt или stdin." >&2
  exit 2
fi

MODEL="$(resolve_model)"

case "$THINKING_MODE" in
  on)
    if [[ -n "$SYSTEM_PROMPT" ]]; then
      SYSTEM_PROMPT="<|think|>

$SYSTEM_PROMPT"
    else
      SYSTEM_PROMPT="<|think|>"
    fi
    ;;
  off)
    ;;
  *)
    echo "Неверный thinking mode: $THINKING_MODE. Используйте on|off." >&2
    exit 2
    ;;
esac

messages_json="$(jq -cn \
  --arg system "$SYSTEM_PROMPT" \
  --arg prompt "$PROMPT" '
    [
      ($system | select(length > 0) | {role:"system", content:.}),
      {role:"user", content:$prompt}
    ] | map(select(. != null))
  ')"

temperature_json='null'
top_p_json='null'
top_k_json='null'

if [[ -n "$TEMPERATURE" ]]; then
  temperature_json="$TEMPERATURE"
fi

if [[ -n "$TOP_P" ]]; then
  top_p_json="$TOP_P"
fi

if [[ -n "$TOP_K" ]]; then
  top_k_json="$TOP_K"
fi

payload_filter='{
  model: $model,
  messages: $messages,
  stream: false,
  options: (
    {}
    + (if ($temperature? != null) then {temperature: $temperature} else {} end)
    + (if ($top_p? != null) then {top_p: $top_p} else {} end)
    + (if ($top_k? != null) then {top_k: $top_k} else {} end)
  )
}'

should_fallback_for_status() {
  local status="$1"
  local codes=",${FALLBACK_EXIT_CODES},"
  [[ "$codes" == *",${status},"* ]]
}

run_chat_request() {
  local request_model="$1"
  local curl_stderr
  curl_stderr="$(mktemp)"
  local response=""
  local curl_status=0
  local -a payload_args=(
    --arg model "$request_model"
    --argjson messages "$messages_json"
    --argjson temperature "$temperature_json"
    --argjson top_p "$top_p_json"
    --argjson top_k "$top_k_json"
  )

  set +e
  response="$(
    jq -cn "${payload_args[@]}" "$payload_filter" \
    | curl -fsS \
        --connect-timeout "$CONNECT_TIMEOUT_SECONDS" \
        --max-time "$TOTAL_TIMEOUT_SECONDS" \
        --retry "$RETRY_COUNT" \
        --retry-delay "$RETRY_DELAY_SECONDS" \
        --retry-all-errors \
        -H 'Content-Type: application/json' \
        -d @- \
        "$OLLAMA_BASE_URL/api/chat" 2>"$curl_stderr"
  )"
  curl_status=$?
  set -e

  if [[ "$curl_status" -ne 0 ]]; then
    local curl_error
    curl_error="$(cat "$curl_stderr" 2>/dev/null || true)"
    rm -f "$curl_stderr"
    printf '%s\037%s' "$curl_status" "$curl_error"
    return 1
  fi

  rm -f "$curl_stderr"
  printf '%s' "$response"
}

set +e
response="$(run_chat_request "$MODEL")"
request_status=$?
set -e

if [[ "$request_status" -ne 0 ]]; then
  curl_status="${response%%$'\037'*}"
  curl_error="${response#*$'\037'}"
  case "$curl_status" in
    28)
      echo "Ollama request timed out after ${TOTAL_TIMEOUT_SECONDS}s (connect ${CONNECT_TIMEOUT_SECONDS}s) for model ${MODEL} at ${OLLAMA_BASE_URL}." >&2
      ;;
    7)
      echo "Ollama backend is unreachable at ${OLLAMA_BASE_URL} for model ${MODEL}." >&2
      ;;
    22)
      echo "Ollama returned an HTTP error for model ${MODEL} at ${OLLAMA_BASE_URL}." >&2
      ;;
    *)
      echo "Ollama request failed for model ${MODEL} at ${OLLAMA_BASE_URL} (curl exit ${curl_status})." >&2
      ;;
  esac
  if [[ -n "$curl_error" ]]; then
    printf '%s\n' "$curl_error" >&2
  fi
  if [[ -n "$FALLBACK_MODEL" && "$FALLBACK_MODEL" != "$MODEL" ]] && should_fallback_for_status "$curl_status"; then
    echo "Falling back from model ${MODEL} to ${FALLBACK_MODEL} after curl exit ${curl_status}." >&2
    set +e
    response="$(run_chat_request "$FALLBACK_MODEL")"
    fallback_status=$?
    set -e
    if [[ "$fallback_status" -eq 0 ]]; then
      MODEL="$FALLBACK_MODEL"
    else
      fallback_curl_status="${response%%$'\037'*}"
      fallback_curl_error="${response#*$'\037'}"
      echo "Fallback model ${FALLBACK_MODEL} also failed (curl exit ${fallback_curl_status})." >&2
      if [[ -n "$fallback_curl_error" ]]; then
        printf '%s\n' "$fallback_curl_error" >&2
      fi
      exit "${fallback_curl_status}"
    fi
  else
    exit "$curl_status"
  fi
fi

if [[ "$OUTPUT_MODE" == "json" ]]; then
  printf '%s\n' "$response"
  exit 0
fi

printf '%s\n' "$(printf '%s' "$response" | jq -r '.message.content // empty')"
