#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

original_args=("$@")

extract_client_budget_target_projection() {
  local payload="${1:-}"
  [[ -n "$payload" ]] || return 1
  printf '%s\n' "$payload" | jq -c '
    if (.client_budget_target_update | type) == "object" then
      .client_budget_target_update as $projection
      | if (($projection.target_percent | type) == "number" and ($projection.target_percent | floor) == $projection.target_percent and ($projection.target_percent >= 0 and $projection.target_percent <= 100) and (($projection.target_percent % 10) == 0))
          and (($projection.project.code | type) == "string" and (($projection.project.code | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.namespace.code | type) == "string" and (($projection.namespace.code | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.operator_notice.exact_chat_command | type) == "string" and (($projection.operator_notice.exact_chat_command | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($projection.operator_notice.message_text | type) == "string" and (($projection.operator_notice.message_text | gsub("^\\s+|\\s+$"; "")) | length) > 0)
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
percent=""
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
    --percent)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      percent="$2"
      shift 2
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

"$SCRIPT_DIR/ensure_observe_frontdoor.sh" --path "/api/client-budget-target" >/dev/null 2>&1 || true

if [[ "$api_supported" == "true" ]] \
  && [[ -n "$percent" ]] \
  && { [[ -z "$repo_root" ]] || [[ "$repo_root" == "$REPO_ROOT" ]]; } \
  && command -v curl >/dev/null 2>&1 \
  && command -v jq >/dev/null 2>&1; then
  json_payload="$(
    jq -n \
      --arg project "$project" \
      --arg namespace "$namespace" \
      --argjson percent "$percent" '
        {
          project: (if ($project | length) > 0 then $project else null end),
          namespace: $namespace,
          percent: $percent
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
        "http://${observe_host}:${observe_port}/api/client-budget-target" 2>/dev/null || true
    )"
    if [[ -n "$api_payload" ]]; then
      target_projection="$(extract_client_budget_target_projection "$api_payload" || true)"
      if [[ -z "$target_projection" ]]; then
        echo "continuity client budget target: invalid API payload" >&2
        exit 12
      fi
      printf '%s\n' "$target_projection"
      exit 0
    fi
  fi
fi

if [[ -x "$REPO_ROOT/target/release/amai" ]]; then
  exec "$REPO_ROOT/target/release/amai" continuity client-budget-target "${original_args[@]}"
fi

exec "$SCRIPT_DIR/amai_exec.sh" continuity client-budget-target "${original_args[@]}"
