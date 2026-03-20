#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tmp_home="$(mktemp -d)"
trap 'rm -rf "$tmp_home"' EXIT
orig_home="${HOME:-}"
orig_cargo_home="${CARGO_HOME:-${orig_home}/.cargo}"
orig_rustup_home="${RUSTUP_HOME:-${orig_home}/.rustup}"
export HOME="$tmp_home"
export CARGO_HOME="$orig_cargo_home"
export RUSTUP_HOME="$orig_rustup_home"
onboarding_out="$tmp_home/onboarding.out"
disconnect_out="$tmp_home/disconnect.out"

./scripts/onboard_remote_client.sh \
  --client codex \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai >"$onboarding_out"

config_path="$HOME/.codex/config.toml"
test -f "$config_path"
grep -q 'command = "ssh"' "$config_path"
grep -q 'ops@example-host' "$config_path"
grep -q "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh" "$config_path"
grep -q 'launcher_mode: remote_ssh' "$onboarding_out"

./scripts/disconnect_local.sh --client codex >"$disconnect_out"
test ! -f "$config_path"
grep -q 'server_removed: true' "$disconnect_out"
grep -q 'file_purged: true' "$disconnect_out"

echo "proof_remote_onboarding: ok"
