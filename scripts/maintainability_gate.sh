#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

json_mode=0
if [[ "${1:-}" == "--json" ]]; then
  json_mode=1
fi

artifact_version="workspace-maintainability-gate-v1"
standard_source_path="docs/standards/MAINTAINABILITY_SUPPORTABILITY_EVOLVABILITY_ANTI_HARDCODING_STANDARD.md"
binding_path="docs/MAINTAINABILITY_ENFORCEMENT.md"
artifact_path=".amai/onboarding/project-maintainability-gate-state.json"
implementation_status_path="docs/IMPLEMENTATION_STATUS.md"

if [[ ! -f "$standard_source_path" ]]; then
  echo "maintainability standard is missing: $standard_source_path" >&2
  exit 1
fi

if [[ ! -f "$binding_path" ]]; then
  echo "project maintainability binding is missing: $binding_path" >&2
  exit 1
fi

if [[ ! -f "$implementation_status_path" ]]; then
  echo "implementation status file is missing: $implementation_status_path" >&2
  exit 1
fi

git_head="$(git rev-parse HEAD 2>/dev/null)"
observed_at_epoch_ms="$(date +%s%3N)"
worktree_status="$(git status --porcelain=v1 --untracked-files=all 2>/dev/null || true)"
worktree_fingerprint_sha256="$(printf '%s' "$worktree_status" | sha256sum | awk '{print $1}')"
implementation_status_sha256="$(sha256sum "$implementation_status_path" | awk '{print $1}')"

if [[ -z "$worktree_status" ]]; then
  worktree_entry_count=0
else
  worktree_entry_count="$(printf '%s\n' "$worktree_status" | wc -l | tr -d ' ')"
fi

payload="$(jq -n \
  --arg artifact_version "$artifact_version" \
  --arg standard_source_path "$standard_source_path" \
  --arg binding_path "$binding_path" \
  --arg artifact_path "$artifact_path" \
  --arg implementation_status_path "$implementation_status_path" \
  --arg implementation_status_sha256 "$implementation_status_sha256" \
  --arg repo_root "$(pwd)" \
  --arg git_head "$git_head" \
  --arg observed_at_epoch_ms "$observed_at_epoch_ms" \
  --arg worktree_fingerprint_sha256 "$worktree_fingerprint_sha256" \
  --argjson worktree_entry_count "$worktree_entry_count" \
  '{
    artifact_version: $artifact_version,
    purpose: "machine-readable maintainability/supportability/evolvability/anti-hardcoding gate for significant Amai changes",
    standard_source_path: $standard_source_path,
    project_binding_path: $binding_path,
    artifact_path: $artifact_path,
    implementation_status_path: $implementation_status_path,
    implementation_status_sha256: $implementation_status_sha256,
    repo_root: $repo_root,
    observed_at_epoch_ms: ($observed_at_epoch_ms | tonumber),
    git_head: $git_head,
    worktree_fingerprint_sha256: $worktree_fingerprint_sha256,
    worktree_entry_count: $worktree_entry_count,
    standard_storage_mode: "vendored_repo_copy",
    standard_sync_rule: "update the in-repo standard deliberately; do not depend on another repository at runtime",
    closure_guard_command: "./scripts/maintainability_stage_close_guard.sh --json",
    implementation_status_sync_guard_command: "./scripts/implementation_status_sync_guard.sh --json",
    checkbox_closure_requires_fresh_trace: true,
    checkbox_closure_rule: "A significant stage checkbox may be closed only if maintainability_stage_close_guard passes against the latest gate trace after the last meaningful worktree change.",
    implementation_status_sync_required_for_significant_status_update: true,
    implementation_status_sync_rule: "A significant IMPLEMENTATION_STATUS.md update is valid only if implementation_status_sync_guard passes against the latest gate trace after the status file was updated.",
    applies_when: [
      "stage-based implementation",
      "core runtime change",
      "schema or contract change",
      "policy/truth/provenance/scope change",
      "retrieval/continuity/restore/task-graph/procedural-memory change",
      "observability/evidence/audit/recovery change",
      "new config/registry/source-of-truth",
      "significant refactor of a critical zone"
    ],
    fail_closed_questions: [
      "what domain changes",
      "where source of truth lives",
      "whether truth is being replaced by projection",
      "whether a new hidden hardcode was introduced",
      "who owns code/tests/docs/rollback-recovery semantics",
      "which test layers are mandatory",
      "what rollback path exists",
      "what recovery path exists",
      "which docs/contracts/checklists must be updated",
      "whether change impact spread wider than planned"
    ],
    anti_hardcoding_rules: [
      "mutable rules must live in config, contracts, registries, policy layers, or source-of-truth docs",
      "do not duplicate one rule in multiple places",
      "do not hide environment-specific or project-specific values in code if they belong to a canonical source"
    ],
    required_documents: [
      "AGENTS.md",
      "docs/AGENT_START_HERE.md",
      "docs/IMPLEMENTATION_STATUS.md",
      "docs/IMPLEMENTATION_GATES.md",
      "docs/AMAI_GLOBAL_MEMORY_ROADMAP.md",
      "docs/MAINTAINABILITY_ENFORCEMENT.md"
    ],
    mandatory_outputs: [
      "proof or benchmark evidence",
      "root-cause plus fix if maintainability regression exists",
      "updated status/docs when contracts or workflow changed",
      "continuity handoff"
    ],
    known_current_reality: [
      "the repo already contains zones that do not fully satisfy the standard",
      "this gate is mandatory for new significant changes and remediation work",
      "legacy deviations must shrink, not grow"
    ],
    fail_closed_rules: [
      "if source of truth is less clear after the change, gate fails",
      "if a new hidden hardcode appears, gate fails",
      "if rollback or recovery becomes less clear, gate fails",
      "if mandatory test layers are undefined, gate fails",
      "if docs/status/gates drift after contract change, gate fails"
    ]
  }')"

mkdir -p "$(dirname "$artifact_path")"
printf '%s\n' "$payload" > "$artifact_path"

if [[ "$json_mode" -eq 1 ]]; then
  printf '%s\n' "$payload"
  exit 0
fi

echo "Amai maintainability gate"
echo "standard: $standard_source_path"
echo "binding:  $binding_path"
echo
echo "Этот gate обязателен для значимых изменений."
echo "Перед закрытием изменения нужно подтвердить:"
echo "- source of truth не размылся"
echo "- projection не подменила truth"
echo "- новый hidden hardcode не появился"
echo "- owner/tests/rollback/recovery понятны"
echo "- docs/status/gates обновлены"
echo "- stage checkbox for significant work may close only after maintainability_stage_close_guard passes"
echo
echo "Machine-readable output:"
echo "./scripts/maintainability_gate.sh --json"
echo "./scripts/maintainability_stage_close_guard.sh --json"
echo "./scripts/implementation_status_sync_guard.sh --json"
echo
echo "State artifact:"
echo "$artifact_path"
