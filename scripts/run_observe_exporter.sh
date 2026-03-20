#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
exec cargo run --release --quiet -- observe serve --bind "${bind}"
