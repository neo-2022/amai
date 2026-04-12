#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

json_mode=0
if [[ "${1:-}" == "--json" ]]; then
  json_mode=1
fi

artifact_path=".amai/onboarding/project-maintainability-gate-state.json"
artifact_version="workspace-implementation-status-sync-guard-v1"
required_gate_artifact_version="workspace-maintainability-gate-v1"

if [[ ! -f "$artifact_path" ]]; then
  echo "maintainability gate trace is missing: $artifact_path" >&2
  exit 1
fi

stored_gate_artifact_version="$(jq -r '.artifact_version // empty' "$artifact_path")"
stored_git_head="$(jq -r '.git_head // empty' "$artifact_path")"
stored_worktree_fingerprint_sha256="$(jq -r '.worktree_fingerprint_sha256 // empty' "$artifact_path")"
implementation_status_path="$(jq -r '.implementation_status_path // empty' "$artifact_path")"
stored_implementation_status_sha256="$(jq -r '.implementation_status_sha256 // empty' "$artifact_path")"
current_git_head="$(git rev-parse HEAD 2>/dev/null)"
current_worktree_status="$(git status --porcelain=v1 --untracked-files=all 2>/dev/null || true)"
current_worktree_fingerprint_sha256="$(printf '%s' "$current_worktree_status" | sha256sum | awk '{print $1}')"

failure_reasons_json='[]'

if [[ "$stored_gate_artifact_version" != "$required_gate_artifact_version" ]]; then
  failure_reasons_json="$(jq -c '. + ["gate_trace_artifact_version_mismatch"]' <<<"$failure_reasons_json")"
fi

if [[ -z "$implementation_status_path" ]]; then
  failure_reasons_json="$(jq -c '. + ["implementation_status_path_missing_in_gate_trace"]' <<<"$failure_reasons_json")"
elif [[ ! -f "$implementation_status_path" ]]; then
  failure_reasons_json="$(jq -c '. + ["implementation_status_file_missing"]' <<<"$failure_reasons_json")"
fi

current_implementation_status_sha256=""
if [[ -n "$implementation_status_path" ]] && [[ -f "$implementation_status_path" ]]; then
  current_implementation_status_sha256="$(sha256sum "$implementation_status_path" | awk '{print $1}')"
fi

if [[ -n "$implementation_status_path" ]] && [[ -f "$implementation_status_path" ]] && [[ "$stored_implementation_status_sha256" != "$current_implementation_status_sha256" ]]; then
  failure_reasons_json="$(jq -c '. + ["implementation_status_changed_since_gate_trace"]' <<<"$failure_reasons_json")"
fi

if [[ "$stored_git_head" != "$current_git_head" ]]; then
  failure_reasons_json="$(jq -c '. + ["git_head_changed_since_gate_trace"]' <<<"$failure_reasons_json")"
fi

if [[ "$stored_worktree_fingerprint_sha256" != "$current_worktree_fingerprint_sha256" ]]; then
  failure_reasons_json="$(jq -c '. + ["worktree_changed_since_gate_trace"]' <<<"$failure_reasons_json")"
fi

status_sync_allowed=true
if [[ "$failure_reasons_json" != "[]" ]]; then
  status_sync_allowed=false
fi

payload="$(jq -n \
  --arg artifact_version "$artifact_version" \
  --arg artifact_path "$artifact_path" \
  --arg implementation_status_path "$implementation_status_path" \
  --arg required_gate_artifact_version "$required_gate_artifact_version" \
  --arg stored_gate_artifact_version "$stored_gate_artifact_version" \
  --arg stored_implementation_status_sha256 "$stored_implementation_status_sha256" \
  --arg current_implementation_status_sha256 "$current_implementation_status_sha256" \
  --arg stored_git_head "$stored_git_head" \
  --arg current_git_head "$current_git_head" \
  --arg stored_worktree_fingerprint_sha256 "$stored_worktree_fingerprint_sha256" \
  --arg current_worktree_fingerprint_sha256 "$current_worktree_fingerprint_sha256" \
  --argjson status_sync_allowed "$status_sync_allowed" \
  --argjson failure_reasons "$failure_reasons_json" \
  '{
    artifact_version: $artifact_version,
    purpose: "machine-readable sync guard for significant IMPLEMENTATION_STATUS updates",
    gate_trace_artifact_path: $artifact_path,
    implementation_status_path: $implementation_status_path,
    required_gate_artifact_version: $required_gate_artifact_version,
    stored_gate_artifact_version: $stored_gate_artifact_version,
    stored_implementation_status_sha256: $stored_implementation_status_sha256,
    current_implementation_status_sha256: $current_implementation_status_sha256,
    implementation_status_matches_gate_trace: ($stored_implementation_status_sha256 == $current_implementation_status_sha256 and $current_implementation_status_sha256 != ""),
    stored_git_head: $stored_git_head,
    current_git_head: $current_git_head,
    git_head_matches: ($stored_git_head == $current_git_head),
    stored_worktree_fingerprint_sha256: $stored_worktree_fingerprint_sha256,
    current_worktree_fingerprint_sha256: $current_worktree_fingerprint_sha256,
    worktree_fingerprint_matches: ($stored_worktree_fingerprint_sha256 == $current_worktree_fingerprint_sha256),
    status_sync_allowed: $status_sync_allowed,
    failure_reasons: $failure_reasons,
    required_action_if_blocked: "After updating docs/IMPLEMENTATION_STATUS.md for significant work, re-run ./scripts/maintainability_gate.sh --json so the gate trace captures the new status snapshot, then re-run this guard."
  }')"

if [[ "$json_mode" -eq 1 ]]; then
  printf '%s\n' "$payload"
else
  echo "$payload"
fi

if [[ "$status_sync_allowed" != true ]]; then
  exit 1
fi
