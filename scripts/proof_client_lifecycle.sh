#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

temp_home="$(mktemp -d)"
trap 'rm -rf "${temp_home}"' EXIT

RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client codex --yes --skip-stack --skip-release-build
test -f "${temp_home}/.codex/config.toml"
grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/disconnect_local.sh --client codex
if grep -q '\[mcp_servers.amai\]' "${temp_home}/.codex/config.toml"; then
  echo "proof_client_lifecycle: codex server entry still present after disconnect"
  exit 1
fi

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/onboard_local.sh --client cursor --yes --skip-stack --skip-release-build
test -f "${temp_home}/.cursor/mcp.json"
grep -q '"amai"' "${temp_home}/.cursor/mcp.json"

HOME="${temp_home}" RUSTUP_HOME="${RUSTUP_HOME}" CARGO_HOME="${CARGO_HOME}" ./scripts/disconnect_local.sh --client cursor
if [[ -f "${temp_home}/.cursor/mcp.json" ]] && grep -q '"amai"' "${temp_home}/.cursor/mcp.json"; then
  echo "proof_client_lifecycle: cursor server entry still present after disconnect"
  exit 1
fi

./scripts/onboard_local.sh --client claude-code --yes --skip-stack --skip-release-build
test -f .mcp.json
grep -q '"amai"' .mcp.json

./scripts/disconnect_local.sh --client claude-code
if [[ -f .mcp.json ]] && grep -q '"amai"' .mcp.json; then
  echo "proof_client_lifecycle: claude-code server entry still present after disconnect"
  exit 1
fi

echo "proof_client_lifecycle: ok"
