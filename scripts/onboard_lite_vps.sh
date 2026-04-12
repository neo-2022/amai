#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
./scripts/sync_startup_contract_sha.sh >/dev/null

exec ./scripts/amai_exec.sh bootstrap onboarding --stack-profile lite_vps "$@"
