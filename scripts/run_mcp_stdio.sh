#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
source ./scripts/load_env.sh

if [[ -x ./target/release/amai ]]; then
  exec ./target/release/amai mcp serve
fi

exec cargo run --release --quiet -- mcp serve
