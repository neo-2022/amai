#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

if [[ $# -lt 2 ]]; then
  echo "github_public_push_guard: expected <remote_name> <remote_url>" >&2
  exit 2
fi

remote_name="$1"
remote_url="$2"

if [[ "$remote_name" != "github" && "$remote_url" != *"github.com:neo-2022/amai.git"* && "$remote_url" != *"github.com/neo-2022/amai.git"* ]]; then
  exit 0
fi

allowlist_file="config/github_public_allowlist.txt"
if [[ ! -f "$allowlist_file" ]]; then
  echo "github_public_push_guard: missing allowlist $allowlist_file" >&2
  exit 1
fi

declare -a changed_files=()
declare -a allow_patterns=()

while read -r local_ref local_sha remote_ref remote_sha; do
  [[ -z "${local_ref:-}" ]] && continue
  range=""
  if [[ "${remote_sha:-}" == "0000000000000000000000000000000000000000" ]]; then
    range="$local_sha"
  else
    range="${remote_sha}..${local_sha}"
  fi

  while IFS= read -r file_path; do
    [[ -z "$file_path" ]] && continue
    changed_files+=("$file_path")
  done < <(git diff --name-only "$range")
done

if (( ${#changed_files[@]} == 0 )); then
  exit 0
fi

while IFS= read -r raw_pattern; do
  pattern="${raw_pattern#"${raw_pattern%%[![:space:]]*}"}"
  pattern="${pattern%"${pattern##*[![:space:]]}"}"
  [[ -z "$pattern" ]] && continue
  [[ "$pattern" == \#* ]] && continue
  allow_patterns+=("$pattern")
done < "$allowlist_file"

if (( ${#allow_patterns[@]} == 0 )); then
  echo "github_public_push_guard: allowlist is empty: $allowlist_file" >&2
  exit 1
fi

declare -a blocked_paths=()
for file_path in "${changed_files[@]}"; do
  allowed=0
  for pattern in "${allow_patterns[@]}"; do
    if [[ "$file_path" == $pattern ]]; then
      allowed=1
      break
    fi
  done
  if (( allowed == 0 )); then
    blocked_paths+=("$file_path")
  fi
done

if (( ${#blocked_paths[@]} > 0 )); then
  printf '%s\n' "Push to GitHub blocked by guard: path is outside public install allowlist." >&2
  printf '%s\n' "Allowed patterns: $allowlist_file" >&2
  printf '%s\n' "Blocked paths:" >&2
  printf ' - %s\n' "${blocked_paths[@]}" >&2
  exit 1
fi

exit 0
