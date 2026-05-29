#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASE_SYSTEM_FILE="$REPO_ROOT/prompts/side_agents/gemma_programmer_system.txt"

MODEL="${MODEL:-gemma4:e4b}"
MODE="review"
PROMPT=""
JSON_MODE=0
TEMPERATURE="${TEMPERATURE:-}"
TOP_P="${TOP_P:-}"
TOP_K="${TOP_K:-}"
THINKING_MODE="${THINKING_MODE:-}"
RESPONSE_LANGUAGE="${RESPONSE_LANGUAGE:-English}"
OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT="${OLLAMA_TOTAL_TIMEOUT_SECONDS:-}"
OLLAMA_CONNECT_TIMEOUT_SECONDS_DEFAULT="${OLLAMA_CONNECT_TIMEOUT_SECONDS:-}"
OLLAMA_FALLBACK_MODEL_DEFAULT="${OLLAMA_FALLBACK_MODEL:-}"

usage() {
  cat >&2 <<'EOF'
Usage:
  gemma_code_assist.sh [--model <name>] [--mode <review|bug|plan|json|patch|split>] [--prompt <text>] [--json]
                       [--thinking <on|off>]
  echo "task" | gemma_code_assist.sh [--mode <...>] [--json]

Modes:
  review   code review / critique / risk finding
  bug      root-cause hypotheses and safest fix direction
  plan     bounded engineering plan
  json     strict machine-readable answer; caller should specify desired schema
  patch    draft patch approach, not authoritative final patch
  split    monolith/domain split plan with target files and symbol moves
EOF
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model)
      MODEL="$2"
      shift 2
      ;;
    --mode)
      MODE="$2"
      shift 2
      ;;
    --prompt)
      PROMPT="$2"
      shift 2
      ;;
    --json)
      JSON_MODE=1
      shift
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

if [[ -z "${PROMPT//[[:space:]]/}" ]]; then
  echo "Пустой prompt. Передайте --prompt или stdin." >&2
  exit 2
fi

if [[ ! -f "$BASE_SYSTEM_FILE" ]]; then
  echo "Не найден system prompt: $BASE_SYSTEM_FILE" >&2
  exit 1
fi

BASE_SYSTEM="$(cat "$BASE_SYSTEM_FILE")"

case "$MODE" in
  review)
    MODE_SYSTEM="$(cat <<'EOF'
Текущий режим: code review.
Главная цель: найти баги, риски, регрессии, missing tests и слабые assumptions.
Не ограничивайся summary. Findings важнее praise.
Если дефектов не видишь, так и скажи, но назови residual risks.
EOF
)"
    ;;
  bug)
    MODE_SYSTEM="$(cat <<'EOF'
Текущий режим: bug triage.
Дай root-cause hypotheses по вероятности.
Отдельно скажи, что observed_fact, а что inference.
Предлагай safest fix direction, а не risky rewrite.
EOF
)"
    ;;
  plan)
    MODE_SYSTEM="$(cat <<'EOF'
Текущий режим: engineering plan.
Сфокусируйся на порядке шагов, рисках и proof/non-regression.
Не расписывай лишнюю архитектурную воду.
EOF
)"
    ;;
  json)
    MODE_SYSTEM="$(cat <<'EOF'
Текущий режим: strict JSON.
Выводи только один JSON object.
Без markdown fences.
Без пояснений до и после.
Если чего-то не хватает, ставь string value `unknown` или `insufficient_evidence`.
EOF
)"
    ;;
  patch)
    MODE_SYSTEM="$(cat <<'EOF'
Текущий режим: patch drafting.
Дай минимальный надёжный patch approach.
Не утверждай, что patch безопасен без local verification.
Отмечай likely touch points, invariants и regression risks.
EOF
)"
    ;;
  split)
    MODE_SYSTEM="$(cat <<'EOF'
Текущий режим: monolith/domain split.
Твоя задача — не просто сказать "файл большой", а предложить реальную декомпозицию.
Обязательно:
- выдели домены;
- предложи target files/modules;
- распредели symbol/function groups по ним;
- назови shared contracts/invariants;
- укажи migration order;
- укажи regression/proof checklist.
Запрещено делить по произвольным line-count или cosmetical причинам.
Опирайся на domain boundaries и project maintainability standard.
EOF
)"
    ;;
  *)
    echo "Неизвестный --mode: $MODE" >&2
    usage
    ;;
esac

if [[ -z "$TEMPERATURE" ]]; then
  case "$MODE" in
    split) TEMPERATURE="0.15" ;;
    json) TEMPERATURE="0.1" ;;
    *) TEMPERATURE="0.2" ;;
  esac
fi

if [[ -z "$TOP_P" ]]; then
  TOP_P="0.95"
fi

if [[ -z "$TOP_K" ]]; then
  TOP_K="64"
fi

if [[ -z "$THINKING_MODE" ]]; then
  case "$MODE" in
    split|plan) THINKING_MODE="on" ;;
    *) THINKING_MODE="off" ;;
  esac
fi

if [[ -z "$OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT" ]]; then
  case "$MODE" in
    split) OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT="120" ;;
    plan) OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT="90" ;;
    json) OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT="45" ;;
    *) OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT="75" ;;
  esac
fi

if [[ -z "$OLLAMA_CONNECT_TIMEOUT_SECONDS_DEFAULT" ]]; then
  OLLAMA_CONNECT_TIMEOUT_SECONDS_DEFAULT="5"
fi

if [[ -z "$OLLAMA_FALLBACK_MODEL_DEFAULT" && "$MODEL" == "gemma4:31b-cloud" ]]; then
  OLLAMA_FALLBACK_MODEL_DEFAULT="gemma4:e4b"
fi

SYSTEM_PROMPT="${BASE_SYSTEM}"$'\n\n'"${MODE_SYSTEM}"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n\n'"Default response language: ${RESPONSE_LANGUAGE}. Use another language only if explicitly requested."
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n\n'"Canonical repo root: ${REPO_ROOT}."
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"Project standards and laws are in:"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"- ${REPO_ROOT}/AGENTS.md"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"- ${REPO_ROOT}/docs/AGENT_START_HERE.md"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"- ${REPO_ROOT}/docs/MAINTAINABILITY_ENFORCEMENT.md"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"- ${REPO_ROOT}/docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"- ${REPO_ROOT}/docs/IMPLEMENTATION_STATUS.md"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"- ${REPO_ROOT}/docs/IMPLEMENTATION_GATES.md"
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n\n'"Active mode defaults: temperature=${TEMPERATURE}, top_p=${TOP_P}, top_k=${TOP_K}, thinking=${THINKING_MODE}."
SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"Launcher timeout budget: total=${OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT}s, connect=${OLLAMA_CONNECT_TIMEOUT_SECONDS_DEFAULT}s."
if [[ -n "$OLLAMA_FALLBACK_MODEL_DEFAULT" && "$OLLAMA_FALLBACK_MODEL_DEFAULT" != "$MODEL" ]]; then
  SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n'"Launcher fallback model on timeout/unreachable: ${OLLAMA_FALLBACK_MODEL_DEFAULT}."
fi

if [[ "$JSON_MODE" -eq 1 && "$MODE" != "json" ]]; then
  SYSTEM_PROMPT="${SYSTEM_PROMPT}"$'\n\n'"Выводи только один JSON object без markdown fencing."
fi

cmd=(
  "$REPO_ROOT/scripts/ollama_chat.sh"
  --model "$MODEL"
  --system "$SYSTEM_PROMPT"
  --temperature "$TEMPERATURE"
  --top-p "$TOP_P"
  --top-k "$TOP_K"
  --thinking "$THINKING_MODE"
  --prompt "$PROMPT"
)

if [[ "$JSON_MODE" -eq 1 ]]; then
  cmd+=(--json)
fi

export OLLAMA_TOTAL_TIMEOUT_SECONDS="$OLLAMA_TOTAL_TIMEOUT_SECONDS_DEFAULT"
export OLLAMA_CONNECT_TIMEOUT_SECONDS="$OLLAMA_CONNECT_TIMEOUT_SECONDS_DEFAULT"
export OLLAMA_FALLBACK_MODEL="$OLLAMA_FALLBACK_MODEL_DEFAULT"

exec "${cmd[@]}"
