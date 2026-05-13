#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons_root="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
svg_src="${repo_root}/brand/amai_mark.svg"
png_src="${repo_root}/tools/vscode-amai-bridge/media/amai-extension.png"
ico_src="${repo_root}/brand/favicon.ico"

mkdir -p \
  "${icons_root}/scalable/apps" \
  "${icons_root}/16x16/apps" \
  "${icons_root}/22x22/apps" \
  "${icons_root}/24x24/apps" \
  "${icons_root}/32x32/apps" \
  "${icons_root}/48x48/apps" \
  "${icons_root}/64x64/apps" \
  "${icons_root}/128x128/apps" \
  "${icons_root}/256x256/apps"

if [[ -f "${svg_src}" ]]; then
  cp -f "${svg_src}" "${icons_root}/scalable/apps/amai.svg"
fi

generate_png_size() {
  local size="$1"
  local dst="${icons_root}/${size}x${size}/apps/amai.png"
  if command -v rsvg-convert >/dev/null 2>&1 && [[ -f "${svg_src}" ]]; then
    rsvg-convert -w "${size}" -h "${size}" "${svg_src}" -o "${dst}" >/dev/null 2>&1 && return 0
  fi
  if command -v magick >/dev/null 2>&1 && [[ -f "${svg_src}" ]]; then
    magick -background none "${svg_src}" -resize "${size}x${size}" "${dst}" >/dev/null 2>&1 && return 0
  fi
  if command -v convert >/dev/null 2>&1 && [[ -f "${svg_src}" ]]; then
    convert -background none "${svg_src}" -resize "${size}x${size}" "${dst}" >/dev/null 2>&1 && return 0
  fi
  if [[ -f "${png_src}" ]]; then
    if command -v magick >/dev/null 2>&1; then
      magick "${png_src}" -resize "${size}x${size}" "${dst}" >/dev/null 2>&1 && return 0
    fi
    if command -v convert >/dev/null 2>&1; then
      convert "${png_src}" -resize "${size}x${size}" "${dst}" >/dev/null 2>&1 && return 0
    fi
    cp -f "${png_src}" "${dst}" && return 0
  fi
  if [[ -f "${ico_src}" ]]; then
    cp -f "${ico_src}" "${dst}" && return 0
  fi
  return 1
}

for size in 16 22 24 32 48 64 128 256; do
  generate_png_size "${size}" || true
done

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${icons_root}" >/dev/null 2>&1 || true
fi

echo "Amai tray icon installed"
