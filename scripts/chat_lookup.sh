#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if blocked_reply="$("$SCRIPT_DIR/client_budget_reply_gate.sh")"; then
  :
else
  status=$?
  if [[ $status -eq 10 ]]; then
    printf '%s\n' "$blocked_reply"
    exit 0
  fi
  exit $status
fi

cd "$SCRIPT_DIR/.."

intent="last_chat"
include_chat_messages=true
include_flag_seen=false
question_seen=false
args=()
freeform_question=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --intent)
      intent="$2"
      args+=("$1" "$2")
      shift 2
      ;;
    --intent=*)
      intent="${1#*=}"
      args+=("$1")
      shift
      ;;
    --chat-reference)
      if [[ "${2:-}" == previous* && "$intent" == "last_chat" ]]; then
        intent="previous_chat"
      fi
      args+=("$1" "$2")
      shift 2
      ;;
    --chat-reference=*)
      if [[ "${1#*=}" == previous* && "$intent" == "last_chat" ]]; then
        intent="previous_chat"
      fi
      args+=("$1")
      shift
      ;;
    --at-time-rfc3339|--at-time-rfc3339=*)
      if [[ "$intent" == "last_chat" ]]; then
        intent="chat_at_time"
      fi
      args+=("$1")
      if [[ "$1" == "--at-time-rfc3339" ]]; then
        args+=("$2")
        shift 2
      else
        shift
      fi
      ;;
    --include-chat-messages|--include-previous-chat-messages)
      include_chat_messages=true
      include_flag_seen=true
      args+=("$1")
      shift
      ;;
    --question)
      question_seen=true
      args+=("$1" "$2")
      shift 2
      ;;
    --question=*)
      question_seen=true
      args+=("$1")
      shift
      ;;
    --project|--namespace|--repo-root|--messages-count)
      args+=("$1" "$2")
      shift 2
      ;;
    --project=*|--namespace=*|--repo-root=*|--messages-count=*)
      args+=("$1")
      shift
      ;;
    --)
      shift
      while [[ $# -gt 0 ]]; do
        freeform_question+=("$1")
        shift
      done
      ;;
    *)
      if [[ "$1" == -* ]]; then
        args+=("$1")
        shift
      else
        freeform_question+=("$1")
        shift
      fi
      ;;
  esac
done

if [[ ${#freeform_question[@]} -gt 0 && "$question_seen" == false ]]; then
  args+=(--question "${freeform_question[*]}")
  question_seen=true
fi

final_args=(cargo run --quiet -- continuity answer --intent "$intent")
if [[ "$include_chat_messages" == true && "$include_flag_seen" == false && "$question_seen" == false ]]; then
  final_args+=(--include-chat-messages)
fi
final_args+=("${args[@]}")

exec "${final_args[@]}"
