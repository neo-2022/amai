#!/usr/bin/env bash
set -euo pipefail

script_path="$(readlink -f "${BASH_SOURCE[0]}")"
repo_root="$(cd "$(dirname "${script_path}")/.." && pwd)"
export AMAI_REPO_ROOT="${repo_root}"

if [[ -x "${repo_root}/target/release/memory" ]]; then
  exec "${repo_root}/target/release/memory" "$@"
fi

if [[ -x "${repo_root}/target/debug/memory" ]]; then
  exec "${repo_root}/target/debug/memory" "$@"
fi

exec cargo run --quiet --manifest-path "${repo_root}/Cargo.toml" --bin memory -- "$@"
