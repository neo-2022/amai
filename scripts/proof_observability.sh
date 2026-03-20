#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bootstrap_stack.sh
./scripts/proof_accuracy.sh
./scripts/proof_load.sh
cargo run --release --quiet -- observe snapshot
cargo run --release --quiet -- observe sla-check
