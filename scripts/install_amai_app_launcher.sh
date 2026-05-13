#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
desktop_file="${apps_dir}/amai.desktop"

mkdir -p "${apps_dir}"

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
Version=1.0
Name=Amai
Comment=Amai control menu
Exec=/bin/bash -lc '${repo_root}/scripts/run_amai_tray.sh'
Icon=amai
Terminal=false
Categories=Utility;Development;
StartupNotify=true
EOF

printf 'Amai launcher installed: %s\n' "${desktop_file}"
