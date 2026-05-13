#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
autostart_dir="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
desktop_file="${autostart_dir}/amai-tray.desktop"

mkdir -p "${autostart_dir}"

icon_path="${repo_root}/brand/amai_mark.svg"
if [[ ! -f "${icon_path}" ]]; then
  icon_path="${repo_root}/tools/vscode-amai-bridge/media/amai-extension.png"
fi
if [[ ! -f "${icon_path}" ]]; then
  icon_path="applications-system"
fi

cat > "${desktop_file}" <<EOF
[Desktop Entry]
Type=Application
Name=Amai
Comment=Amai tray menu and quick actions
Exec=${repo_root}/scripts/run_amai_tray.sh
Icon=amai
Terminal=false
TryExec=${repo_root}/scripts/run_amai_tray.sh
X-GNOME-Autostart-enabled=true
EOF

printf 'Amai tray autostart installed: %s\n' "${desktop_file}"

if [[ -x "${repo_root}/scripts/install_amai_app_launcher.sh" ]]; then
  "${repo_root}/scripts/install_amai_app_launcher.sh" >/dev/null 2>&1 || true
fi
if [[ -x "${repo_root}/scripts/install_amai_tray_icon.sh" ]]; then
  "${repo_root}/scripts/install_amai_tray_icon.sh" >/dev/null 2>&1 || true
fi
