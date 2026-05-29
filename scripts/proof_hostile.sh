#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bootstrap_stack.sh
cargo run --quiet -- verify hostile --scenario all
./scripts/status.sh
