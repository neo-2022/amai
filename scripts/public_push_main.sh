#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

remote_name="${1:-github}"
remote_ref="refs/heads/main"
expected_repo_ssh="github.com:neo-2022/amai.git"
expected_repo_https="github.com/neo-2022/amai.git"

if ! git remote get-url "$remote_name" >/dev/null 2>&1; then
  echo "public_push_main: remote '$remote_name' not found." >&2
  exit 1
fi

remote_url="$(git remote get-url "$remote_name")"
if [[ "$remote_url" != *"$expected_repo_ssh"* && "$remote_url" != *"$expected_repo_https"* ]]; then
  echo "public_push_main: remote '$remote_name' is not neo-2022/amai." >&2
  echo "remote url: $remote_url" >&2
  exit 1
fi

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$current_branch" != "main" ]]; then
  echo "public_push_main: switch to 'main' first. Current branch: $current_branch" >&2
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "public_push_main: working tree is not clean. Commit/stash first." >&2
  exit 1
fi

git fetch "$remote_name" main --quiet

local_sha="$(git rev-parse HEAD)"
if git rev-parse --verify "$remote_name/main" >/dev/null 2>&1; then
  remote_sha="$(git rev-parse "$remote_name/main")"
else
  remote_sha="0000000000000000000000000000000000000000"
fi

printf '%s %s %s %s\n' \
  "refs/heads/main" \
  "$local_sha" \
  "$remote_ref" \
  "$remote_sha" | \
  "$repo_root/scripts/github_public_push_guard.sh" "$remote_name" "$remote_url"

git push "$remote_name" HEAD:main

