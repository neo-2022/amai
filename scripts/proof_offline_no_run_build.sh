#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmp_cargo_home="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_cargo_home"
}
trap cleanup EXIT

CARGO_HOME="$tmp_cargo_home" \
CARGO_NET_OFFLINE=true \
cargo test --workspace --all-targets --no-run --offline --locked

echo "proof_offline_no_run_build: ok"
