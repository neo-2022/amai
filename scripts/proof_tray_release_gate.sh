#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

tray_home="$(mktemp -d)"
trap 'rm -rf "${tray_home}"' EXIT

HOME="${tray_home}" XDG_CONFIG_HOME="${tray_home}/.config" XDG_STATE_HOME="${tray_home}/.state" \
  ./scripts/amai_tray_menu.sh --status >/tmp/proof_tray_release_status.txt
rg -n '^Amai (не )?подключена$' /tmp/proof_tray_release_status.txt >/dev/null

HOME="${tray_home}" XDG_CONFIG_HOME="${tray_home}/.config" XDG_STATE_HOME="${tray_home}/.state" \
  ./scripts/amai_tray_menu.sh --toggle-notifications >/dev/null
test -f "${tray_home}/.config/amai/tray_notifications_disabled"
HOME="${tray_home}" XDG_CONFIG_HOME="${tray_home}/.config" XDG_STATE_HOME="${tray_home}/.state" \
  ./scripts/amai_tray_menu.sh --toggle-notifications >/dev/null
test ! -f "${tray_home}/.config/amai/tray_notifications_disabled"

./scripts/proof_tray_wayland_x11_fallback.sh

echo "proof_tray_release_gate: PASS"
