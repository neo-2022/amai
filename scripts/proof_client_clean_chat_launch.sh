#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo test --quiet compact_chat_clean_launch_surface
cargo test --quiet maybe_launch_compact_chat_host
cargo test --quiet compact_chat_notice_kind_preserves_fail_closed_host_states
cargo test --quiet compact_chat_response_payload_keeps_summary_and_notice_launch_contract_aligned
cargo test --quiet dashboard_html_keeps_compact_chat_assist_path_source_first

echo "proof_client_clean_chat_launch: ok"
