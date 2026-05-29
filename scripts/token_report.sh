#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

exec ./scripts/amai_exec.sh observe token-report "$@"
