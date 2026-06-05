#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

echo "[proof_lifecycle_policy_simulate_measured_validation] targeted Rust policy simulation tests"
cargo test --quiet lifecycle_policy_simulation

echo "[proof_lifecycle_policy_simulate_measured_validation] forgetting consolidation proof with Queue 3 validation assertions"
./scripts/proof_forgetting_consolidation.sh

echo "proof_lifecycle_policy_simulate_measured_validation: ok"
