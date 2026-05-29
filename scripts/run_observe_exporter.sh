#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

if [[ -d "./state/tooling/cmake-venv/bin" ]]; then
  export PATH="$(pwd)/state/tooling/cmake-venv/bin:$PATH"
fi

bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
if [[ -x ./target/release/amai ]]; then
  exec ./target/release/amai observe serve --bind "${bind}"
fi
exec ./scripts/amai_exec.sh observe serve --bind "${bind}"
