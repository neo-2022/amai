#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo test --quiet execute_host_current_thread_control_launch_

echo "proof_host_current_thread_control_uri_launch: ok"
