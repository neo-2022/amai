#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

intent="last_chat"
include_chat_messages=true
include_flag_seen=false
args=()

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
      if [[ "${2:-}" == "previous" && "$intent" == "last_chat" ]]; then
        intent="previous_chat"
      fi
      args+=("$1" "$2")
      shift 2
      ;;
    --chat-reference=*)
      if [[ "${1#*=}" == "previous" && "$intent" == "last_chat" ]]; then
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
    *)
      args+=("$1")
      shift
      ;;
  esac
done

final_args=(cargo run --quiet -- continuity answer --intent "$intent")
if [[ "$include_chat_messages" == true && "$include_flag_seen" == false ]]; then
  final_args+=(--include-chat-messages)
fi
final_args+=("${args[@]}")

exec "${final_args[@]}"
