#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons_root="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
svg_src="${repo_root}/brand/amai_mark.svg"
png_src="${repo_root}/tools/vscode-amai-bridge/media/amai-extension.png"

mkdir -p "${icons_root}/scalable/apps" "${icons_root}/64x64/apps" "${icons_root}/32x32/apps"

if [[ -f "${svg_src}" ]]; then
  cp -f "${svg_src}" "${icons_root}/scalable/apps/amai.svg"
fi
if [[ -f "${png_src}" ]]; then
  cp -f "${png_src}" "${icons_root}/64x64/apps/amai.png"
  cp -f "${png_src}" "${icons_root}/32x32/apps/amai.png"
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${icons_root}" >/dev/null 2>&1 || true
fi

echo "Amai tray icon installed"
