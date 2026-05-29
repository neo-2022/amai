#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

manifest="$(./scripts/sync_remote_repo.sh --list)"

grep -Fxq 'src/benchmark_measured_approval.rs' <<<"${manifest}"
grep -Fxq 'src/benchmark_promotion.rs' <<<"${manifest}"
grep -Fxq 'src/benchmark_statistics.rs' <<<"${manifest}"
grep -Fxq 'scripts/run_mcp_stdio.sh' <<<"${manifest}"
grep -Fxq '.cargo/config.toml' <<<"${manifest}"
grep -Fxq 'Cargo.toml' <<<"${manifest}"
grep -Fxq '.env.example' <<<"${manifest}"

if grep -Eq '^target/' <<<"${manifest}"; then
  echo "proof_remote_repo_sync_payload: target/ leaked into remote repo payload"
  exit 1
fi

if grep -Eq '^state/' <<<"${manifest}"; then
  echo "proof_remote_repo_sync_payload: state/ leaked into remote repo payload"
  exit 1
fi

if grep -Eq '^tmp/' <<<"${manifest}"; then
  echo "proof_remote_repo_sync_payload: tmp/ leaked into remote repo payload"
  exit 1
fi

if grep -Eq '^output/' <<<"${manifest}"; then
  echo "proof_remote_repo_sync_payload: output/ leaked into remote repo payload"
  exit 1
fi

if grep -Eq '^\\.fastembed_cache/' <<<"${manifest}"; then
  echo "proof_remote_repo_sync_payload: .fastembed_cache/ leaked into remote repo payload"
  exit 1
fi

echo "proof_remote_repo_sync_payload: ok"
