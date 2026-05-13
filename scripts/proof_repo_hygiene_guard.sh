#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
./scripts/repo_hygiene_guard.sh --json
echo "proof_repo_hygiene_guard: ok"
