#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

has_stack_profile=0
for arg in "$@"; do
  case "$arg" in
    --stack-profile|--stack-profile=*)
      has_stack_profile=1
      ;;
  esac
done

if [[ $has_stack_profile -eq 0 && "${AMAI_NO_INSTALL_PROMPT:-0}" != "1" ]]; then
  if [[ "${AMAI_FORCE_INTERACTIVE_PROMPT:-0}" == "1" || ( -t 0 && -t 1 ) ]]; then
    exec ./scripts/preflight.sh "$@"
  fi
fi

exec cargo run --quiet -- bootstrap install "$@"
