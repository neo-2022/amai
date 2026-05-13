#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
export AMAI_REPO_ROOT="${repo_root}"

if [[ -x ./target/release/amai-tray ]]; then
  exec ./target/release/amai-tray
fi

exec cargo run --quiet --release --bin amai-tray
