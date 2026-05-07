#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

source "$SCRIPT_DIR/load_env.sh"

original_args=("$@")

extract_handoff_api_response() {
  local payload="${1:-}"
  [[ -n "$payload" ]] || return 1
  printf '%s\n' "$payload" | jq -c '
    if (.continuity_handoff | type) == "object" then
      . as $root
      | .continuity_handoff as $handoff
      | if (($handoff.headline | type) == "string" and (($handoff.headline | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($handoff.next_step | type) == "string" and (($handoff.next_step | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($handoff.project.code | type) == "string" and (($handoff.project.code | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and (($handoff.namespace.code | type) == "string" and (($handoff.namespace.code | gsub("^\\s+|\\s+$"; "")) | length) > 0)
          and ($root.status == "ok")
        then
          $root
        else
          empty
        end
    else
      empty
    end
  ' 2>/dev/null
}

exec_release_or_exec() {
  if [[ -x "$REPO_ROOT/target/release/amai" ]]; then
    exec "$REPO_ROOT/target/release/amai" continuity handoff "${original_args[@]}"
  fi
  exec "$SCRIPT_DIR/amai_exec.sh" continuity handoff "${original_args[@]}"
}

project=""
namespace="continuity"
headline=""
next_step=""
details_file=""
resolve_current_goal=false
declare -a resolved_headlines=()
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
    --headline)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      headline="$2"
      shift 2
      ;;
    --next-step)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      next_step="$2"
      shift 2
      ;;
    --details-file)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      details_file="$2"
      shift 2
      ;;
    --resolved-headline)
      if (($# < 2)); then
        api_supported=false
        break
      fi
      resolved_headlines+=("$2")
      shift 2
      ;;
    --resolve-current-goal)
      resolve_current_goal=true
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

"$SCRIPT_DIR/ensure_observe_frontdoor.sh" --path "/api/continuity-handoff" >/dev/null 2>&1 || true

if [[ "$api_supported" == "true" ]] \
  && [[ -n "$project" ]] \
  && [[ -n "$headline" ]] \
  && [[ -n "$next_step" ]] \
  && command -v curl >/dev/null 2>&1 \
  && command -v jq >/dev/null 2>&1; then
  details_text=""
  if [[ -n "$details_file" ]]; then
    if [[ ! -f "$details_file" ]]; then
      exec_release_or_exec
    fi
    details_text="$(cat "$details_file")"
  fi
  if ((${#resolved_headlines[@]} == 0)); then
    resolved_headlines_json='[]'
  else
    resolved_headlines_json="$(
      printf '%s\n' "${resolved_headlines[@]}" | jq -Rsc 'split("\n")[:-1]' 2>/dev/null || true
    )"
    [[ -n "$resolved_headlines_json" ]] || resolved_headlines_json='[]'
  fi
  json_payload="$(
    jq -n \
      --arg project "$project" \
      --arg namespace "$namespace" \
      --arg headline "$headline" \
      --arg next_step "$next_step" \
      --arg details "$details_text" \
      --argjson resolve_current_goal "$([[ "$resolve_current_goal" == "true" ]] && echo true || echo false)" \
      --argjson resolved_headlines "$resolved_headlines_json" '
        {
          project: $project,
          namespace: $namespace,
          headline: $headline,
          next_step: $next_step,
          details: (if ($details | length) > 0 then $details else null end),
          resolve_current_goal: $resolve_current_goal,
          resolved_headlines: $resolved_headlines
        }
      ' 2>/dev/null || true
  )"
  if [[ -n "$json_payload" ]]; then
    api_url="http://${observe_host}:${observe_port}/api/continuity-handoff"
    api_payload="$(
      curl \
        --silent \
        --show-error \
        --fail \
        --max-time 2 \
        -H 'Content-Type: application/json' \
        -d "$json_payload" \
        "$api_url" 2>/dev/null || true
    )"
    if [[ -n "$api_payload" ]]; then
      validated_payload="$(extract_handoff_api_response "$api_payload" || true)"
      if [[ -z "$validated_payload" ]]; then
        echo "continuity handoff: invalid API payload" >&2
        exit 12
      fi
      printf '%s\n' "$validated_payload"
      exit 0
    fi
  fi
fi

exec_release_or_exec
