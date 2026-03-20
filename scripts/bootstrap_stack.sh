#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

stack_profile="${AMI_STACK_PROFILE:-default}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --stack-profile)
      stack_profile="${2:?missing value for --stack-profile}"
      shift 2
      ;;
    *)
      echo "unsupported bootstrap_stack.sh argument: $1" >&2
      exit 1
      ;;
  esac
done

export AMI_STACK_PROFILE="${stack_profile}"

if [[ "${AMAI_SKIP_STACK_PREFLIGHT:-0}" != "1" ]]; then
  cargo run -- bootstrap preflight --stack-profile "${stack_profile}"
fi
docker compose up -d --remove-orphans
cargo run -- bootstrap stack

if [[ -n "${AMI_WARMUP_PROJECTS:-}" ]]; then
  ./scripts/warmup_cache.sh
fi
