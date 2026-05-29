#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
exec ./scripts/amai_exec.sh continuity enrich-thread-index "$@"
