#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

original_args=("$@")

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
      printf '%s\n' "$api_payload" | jq '.client_budget_target_update'
      exit 0
    fi
  fi
fi

exec "$SCRIPT_DIR/amai_exec.sh" continuity client-budget-target "${original_args[@]}"
