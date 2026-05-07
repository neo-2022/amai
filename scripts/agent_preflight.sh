#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
./scripts/sync_startup_contract_sha.sh >/dev/null
exec ./scripts/amai_exec.sh bootstrap agent-preflight "$@"
