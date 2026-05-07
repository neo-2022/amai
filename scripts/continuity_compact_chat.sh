#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

original_args=("$@")

extract_compact_chat_projection() {
  local payload="${1:-}"
  [[ -n "$payload" ]] || return 1
  printf '%s\n' "$payload" | jq -c '
    if (.continuity_compact_chat | type) == "object" then
      .continuity_compact_chat as $projection
      | if (($projection.project.code | type) == "string" and (($projection.project.code | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.namespace.code | type) == "string" and (($projection.namespace.code | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.operator_notice.kind | type) == "string" and (($projection.operator_notice.kind | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.handoff.headline | type) == "string" and (($projection.handoff.headline | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.handoff.next_step | type) == "string" and (($projection.handoff.next_step | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.chat_start_restore.prompt_text | type) == "string" and (($projection.chat_start_restore.prompt_text | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (
            (($projection.operator_notice.launch_clean_chat_command | type) == "string" and (($projection.operator_notice.launch_clean_chat_command | gsub("^\\s+|\\s+$"; "")) | length) > 0)
            or
            (($projection.operator_notice.required_host_action | type) == "string" and (($projection.operator_notice.required_host_action | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          )
        then
          $projection
        else
          empty
        end
    else
      empty
    end
  ' 2>/dev/null
}

project=""
namespace="continuity"
repo_root=""
json_requested=false
api_supported=true

while (($# > 0)); do
  case "$1" in
    --project)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      project="$2"
      shift 2
      ;;
    --namespace)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      namespace="$2"
      shift 2
      ;;
    --repo-root)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      repo_root="$2"
      shift 2
      ;;
    --json)
      json_requested=true
      shift
      ;;
    *)
      api_supported=false
      break
      ;;
  esac
done

observe_bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
observe_host="${observe_bind%:*}"
observe_port="${observe_bind##*:}"
case "$observe_host" in
  ""|"0.0.0.0"|"::"|"[::]")
    observe_host="127.0.0.1"
    ;;
  \[*\])
    observe_host="${observe_host#[}"
    observe_host="${observe_host%]}"
    ;;
esac

"$SCRIPT_DIR/ensure_observe_frontdoor.sh" --path "/api/client-budget-compact-chat" >/dev/null 2>&1 || true

if [[ "$api_supported" == "true" ]] \
  && [[ "$json_requested" == "true" ]] \
  && { [[ -z "$repo_root" ]] || [[ "$repo_root" == "$REPO_ROOT" ]]; } \
  && command -v curl >/dev/null 2>&1 \
  && command -v jq >/dev/null 2>&1; then
  json_payload="$(
    jq -n \
      --arg project "$project" \
      --arg namespace "$namespace" '
        {
          project: (if ($project | length) > 0 then $project else null end),
          namespace: $namespace,
          launch_host: false
        }
      ' 2>/dev/null || true
  )"
  if [[ -n "$json_payload" ]]; then
    api_payload="$(
      curl \
        --silent \
        --show-error \
        --fail \
        --max-time 2 \
        -H 'Content-Type: application/json' \
        -d "$json_payload" \
        "http://${observe_host}:${observe_port}/api/client-budget-compact-chat" 2>/dev/null || true
    )"
    if [[ -n "$api_payload" ]]; then
      compact_chat_projection="$(extract_compact_chat_projection "$api_payload" || true)"
      if [[ -z "$compact_chat_projection" ]]; then
        echo "continuity compact chat: invalid API payload" >&2
        exit 12
      fi
      printf '%s\n' "$compact_chat_projection"
      exit 0
    fi
  fi
fi

if [[ -x "$REPO_ROOT/target/release/amai" ]]; then
  exec "$REPO_ROOT/target/release/amai" continuity compact-chat "${original_args[@]}"
fi

exec "$SCRIPT_DIR/amai_exec.sh" continuity compact-chat "${original_args[@]}"
