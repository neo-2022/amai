#!/usr/bin/env bash
set -euo pipefail
trap 'echo "proof_memory_external_official_judge_api_failure.sh failed at line $LINENO" >&2' ERR

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$REPO_ROOT"

echo "== Amai external memory official LongMemEval API failure proof =="

for required_tool in cargo; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    echo "required tool not found: $required_tool" >&2
    exit 2
  fi
done

cargo test --quiet longmemeval_official_judge_

echo "== Done: official judge API failures are fail-closed without eval log materialization =="
