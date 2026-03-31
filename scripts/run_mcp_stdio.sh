#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
source ./scripts/load_env.sh
./scripts/cleanup_mcp_orphans.sh "${repo_root}" >/dev/null 2>&1 || true

if command -v cargo >/dev/null 2>&1; then
  exec cargo run --release --quiet -- mcp serve
fi

if [[ -x ./target/release/amai ]]; then
  exec ./target/release/amai mcp serve
fi

printf 'Amai MCP runner requires cargo or ./target/release/amai\n' >&2
exit 1
