#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

docker compose up -d --remove-orphans
cargo run -- bootstrap stack

if [[ -n "${AMI_WARMUP_PROJECTS:-}" ]]; then
  ./scripts/warmup_cache.sh
fi
