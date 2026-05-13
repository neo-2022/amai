#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
autostart_dir="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
desktop_file="${autostart_dir}/amai-vscode-connect-offer.desktop"

mkdir -p "$autostart_dir"

cat > "$desktop_file" <<EOF
[Desktop Entry]
Type=Application
Name=Amai VS Code Connect Offer
Comment=Offer to connect Amai when VS Code appears
Exec=${repo_root}/scripts/offer_vscode_connect.sh
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF

chmod 644 "$desktop_file"
echo "$desktop_file"
