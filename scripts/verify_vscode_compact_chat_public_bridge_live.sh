#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
source "${repo_root}/scripts/vscode_extensions_roots.sh"
state_path="${repo_root}/.amai/onboarding/vscode-public-bridge-live-state.json"
timeout_seconds="${AMAI_VSCODE_BRIDGE_LIVE_TIMEOUT_SECONDS:-30}"
record=0
dirty_surface_pattern="untitled:${repo_root}/vscode%3A/amai\\.amai-vscode-bridge/open-clean-chat"
tmp_root="$(mktemp -d)"
cleanup() {
  if [[ -n "${user_dir:-}" ]]; then
    "${repo_root}/scripts/close_vscode_temp_host.sh" "${user_dir}" >/dev/null 2>&1 || true
  fi
  rm -rf "${tmp_root}"
}
trap cleanup EXIT

while [[ $# -gt 0 ]]; do
  case "$1" in
    --record)
      record=1
      shift
      ;;
    --timeout-seconds)
      timeout_seconds="${2:?missing value for --timeout-seconds}"
      shift 2
      ;;
    *)
      echo "verify_vscode_compact_chat_public_bridge_live: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

command -v jq >/dev/null || { echo "verify_vscode_compact_chat_public_bridge_live: missing jq" >&2; exit 1; }
command -v code >/dev/null || { echo "verify_vscode_compact_chat_public_bridge_live: missing code" >&2; exit 1; }

expected_bridge_version="$(jq -r '.version // empty' "${repo_root}/tools/vscode-amai-bridge/package.json")"
if [[ -z "${expected_bridge_version}" ]]; then
  echo "verify_vscode_compact_chat_public_bridge_live: failed to read expected bridge version" >&2
  exit 1
fi

mapfile -t detected_roots_with_reason < <(detect_vscode_extensions_roots)
if [[ "${#detected_roots_with_reason[@]}" -eq 0 ]]; then
  echo "verify_vscode_compact_chat_public_bridge_live: failed to detect extensions roots" >&2
  exit 1
fi

extensions_roots=()
extensions_root_reasons=()
for entry in "${detected_roots_with_reason[@]}"; do
  extensions_roots+=("${entry%%$'\t'*}")
  extensions_root_reasons+=("${entry#*$'\t'}")
done
printf 'verify_vscode_compact_chat_public_bridge_live: extensions roots: %s (reasons=%s, code=%s)\n' \
  "$(printf '%s ' "${extensions_roots[@]}")" \
  "$(printf '%s ' "${extensions_root_reasons[@]}")" \
  "$(command -v code 2>/dev/null || true)" >&2

find_installed_extension_dir() {
  local extension_id="$1"
  local root
  for root in "${extensions_roots[@]}"; do
    find "${root}" -maxdepth 1 -type d -name "${extension_id}-*" | sort | tail -n1
  done
}

count_dirty_surface_events() {
  local logs_root="$1"
  local matches
  matches="$(
    find "${logs_root}/logs" -type f -path '*window*/renderer.log' -exec rg -n "${dirty_surface_pattern}" {} + 2>/dev/null \
      || true
  )"
  if [[ -z "${matches}" ]]; then
    echo 0
  else
    printf '%s\n' "${matches}" | wc -l | tr -d ' '
  fi
}

bridge_dir=""
installed_bridge_supports_visible_surface=false
chatgpt_dir=""
for root in "${extensions_roots[@]}"; do
  candidate_bridge_dir="${root}/amai.amai-vscode-bridge-${expected_bridge_version}"
  candidate_chatgpt_dir="$(find "${root}" -maxdepth 1 -type d -name 'openai.chatgpt-*' | sort | tail -n1)"
  if [[ -d "${candidate_bridge_dir}" && -d "${candidate_chatgpt_dir}" ]]; then
    bridge_dir="${candidate_bridge_dir}"
    chatgpt_dir="${candidate_chatgpt_dir}"
    if rg -q 'visible_surface|collectVisibleSurfaceState|publicBridgeIdentity' "${bridge_dir}/extension.js" 2>/dev/null; then
      installed_bridge_supports_visible_surface=true
    fi
    break
  fi
done

if [[ -z "${bridge_dir}" ]]; then
  echo "verify_vscode_compact_chat_public_bridge_live: expected bridge bundle ${expected_bridge_version} is not installed in any detected extensions root" >&2
  exit 1
fi

ext_dir="${tmp_root}/extensions"
user_dir="${tmp_root}/data"
mkdir -p "${ext_dir}" "${user_dir}"
cp -R "${chatgpt_dir}" "${ext_dir}/"
cp -R "${bridge_dir}" "${ext_dir}/amai.amai-vscode-bridge-${expected_bridge_version}"

prompt_file="${tmp_root}/prompt.txt"
result_file="${tmp_root}/result.json"
request_file="${repo_root}/.amai/onboarding/vscode-public-bridge-request.json"
printf 'Amai live public bridge probe %s\n' "$(date -Is)" > "${prompt_file}"

encode_uri_component() {
  jq -rn --arg value "$1" '$value|@uri'
}

source_uri_text="vscode://amai.amai-vscode-bridge/open-clean-chat?prompt_file=$(encode_uri_component "${prompt_file}")&result_file=$(encode_uri_component "${result_file}")&repo_root=$(encode_uri_component "${repo_root}")&target=sidebar&auto_submit=0"

dirty_surface_before="$(count_dirty_surface_events "${user_dir}")"
mkdir -p "$(dirname "${request_file}")"
request_tmp="$(mktemp "$(dirname "${request_file}")/vscode-public-bridge-request.XXXXXX")"
jq -n \
  --arg prompt_file "${prompt_file}" \
  --arg result_file "${result_file}" \
  --arg repo_root "${repo_root}" \
  --arg target "sidebar" \
  --arg source_uri_text "${source_uri_text}" \
  '{
    kind: "open-clean-chat",
    prompt_file: $prompt_file,
    result_file: $result_file,
    repo_root: $repo_root,
    target: $target,
    auto_submit: false,
    source_uri_text: $source_uri_text
  }' > "${request_tmp}"
mv "${request_tmp}" "${request_file}"

if ! code --user-data-dir "${user_dir}" --extensions-dir "${ext_dir}" --new-window "${repo_root}" >/dev/null 2>&1; then
  echo "verify_vscode_compact_chat_public_bridge_live: code --new-window failed for isolated bridge workspace request" >&2
  exit 1
fi

for _ in $(seq 1 "${timeout_seconds}"); do
  if [[ -f "${result_file}" ]]; then
    result_json="$(cat "${result_file}")"
    status="$(jq -r '.status // empty' "${result_file}")"
    authority="$(jq -r '.public_bridge.authority // empty' "${result_file}")"
    runtime_bridge_version="$(jq -r '.public_bridge.version // empty' "${result_file}")"
    ui_cleanup_success="$(jq -r 'if .ui_cleanup.success == null then "missing" else (.ui_cleanup.success | tostring) end' "${result_file}")"
    ui_cleanup_requested="$(jq -r 'if .ui_cleanup.uri_cleanup_requested == null then "missing" else (.ui_cleanup.uri_cleanup_requested | tostring) end' "${result_file}")"
    ui_cleanup_matching_tabs_after="$(jq -r '.ui_cleanup.matching_tabs_after // -1' "${result_file}")"
    ui_cleanup_active_bridge_after="$(jq -r 'if .ui_cleanup.active_editor_matches_bridge_uri_after == null then "missing" else (.ui_cleanup.active_editor_matches_bridge_uri_after | tostring) end' "${result_file}")"
    if [[ "${status}" == "launch_requested" && "${authority}" == "amai.amai-vscode-bridge" ]]; then
      dirty_surface_after="$(count_dirty_surface_events "${user_dir}")"
      if [[ "${runtime_bridge_version}" != "${expected_bridge_version}" ]]; then
        printf '%s\n' "${result_json}" >&2
        echo "verify_vscode_compact_chat_public_bridge_live: bridge runtime version mismatch (expected ${expected_bridge_version}, got ${runtime_bridge_version:-missing})" >&2
        exit 1
      fi
      if [[ "${ui_cleanup_requested}" != "true" || "${ui_cleanup_success}" != "true" || "${ui_cleanup_matching_tabs_after}" != "0" || "${ui_cleanup_active_bridge_after}" != "false" ]]; then
        printf '%s\n' "${result_json}" >&2
        echo "verify_vscode_compact_chat_public_bridge_live: bridge UI cleanup contract not satisfied (stale running-profile bridge runtime or incomplete cleanup result)" >&2
        exit 1
      fi
      if [[ "${dirty_surface_after}" != "${dirty_surface_before}" ]]; then
        printf '%s\n' "${result_json}" >&2
        echo "verify_vscode_compact_chat_public_bridge_live: dirty bridge surface count changed (${dirty_surface_before} -> ${dirty_surface_after})" >&2
        exit 1
      fi
      if [[ ${record} -eq 1 ]]; then
        mkdir -p "$(dirname "${state_path}")"
        jq -n \
          --arg verified_at "$(date -Is)" \
          --arg status "live_launch_verified" \
          --arg authority "${authority}" \
          --arg repo_root "${repo_root}" \
          --arg result_file "${result_file}" \
          --arg prompt_file "${prompt_file}" \
          --argjson dirty_surface_before "${dirty_surface_before}" \
          --argjson dirty_surface_after "${dirty_surface_after}" \
          --slurpfile bridge_result "${result_file}" \
          '{
            status: $status,
            verified_at: $verified_at,
            repo_root: $repo_root,
            public_bridge: {
              authority: $authority,
              version: ($bridge_result[0].public_bridge.version // null)
            },
            source_bundle_capabilities: {
              ui_cleanup: true,
              visible_surface: $source_bundle_supports_visible_surface
            },
            runtime_capabilities: {
              ui_cleanup: ($bridge_result[0].public_bridge.capabilities.ui_cleanup // false),
              visible_surface: ($bridge_result[0].public_bridge.capabilities.visible_surface // false),
              visible_surface_payload_present: ($bridge_result[0].visible_surface != null)
            },
            runtime_capability_drift: {
              visible_surface_missing_from_runtime_result:
                ($source_bundle_supports_visible_surface
                  and (($bridge_result[0].public_bridge.capabilities.visible_surface // false) != true
                    or ($bridge_result[0].visible_surface == null)))
            },
            dirty_surface: {
              pattern: "untitled:/home/art/agent-memory-index/vscode%3A/amai.amai-vscode-bridge/open-clean-chat",
              before_count: $dirty_surface_before,
              after_count: $dirty_surface_after
            },
            ui_cleanup: ($bridge_result[0].ui_cleanup // null),
            bridge_result: $bridge_result[0],
            probe: {
              result_file: $result_file,
              prompt_file: $prompt_file
            }
          }' \
          --argjson source_bundle_supports_visible_surface "$installed_bridge_supports_visible_surface" > "${state_path}"
      fi
      printf '%s\n' "${result_json}"
      exit 0
    fi
    if [[ "${status}" == "launch_failed" ]]; then
      printf '%s\n' "${result_json}" >&2
      echo "verify_vscode_compact_chat_public_bridge_live: bridge reported launch_failed" >&2
      exit 1
    fi
  fi
  sleep 1
done

if [[ -f "${result_file}" ]]; then
  cat "${result_file}" >&2
fi
echo "verify_vscode_compact_chat_public_bridge_live: result file not written within ${timeout_seconds}s" >&2
exit 1
