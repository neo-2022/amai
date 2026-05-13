#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
export AMAI_REPO_ROOT="${repo_root}"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/amai"
mkdir -p "${state_dir}"
tray_pid_file="${state_dir}/rust_tray.pid"

show_info() {
  local text="$1"
  if command -v zenity >/dev/null 2>&1 && [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
    zenity --info --title="Amai" --text="${text}" >/dev/null 2>&1 && return
  fi
  printf '%s\n' "${text}"
}

is_appindicator_enabled() {
  if ! command -v gsettings >/dev/null 2>&1; then
    return 1
  fi
  gsettings get org.gnome.shell enabled-extensions 2>/dev/null | rg -q "ubuntu-appindicators@ubuntu.com|appindicatorsupport@rgcjonas.gmail.com"
}

try_enable_appindicator() {
  command -v gnome-extensions >/dev/null 2>&1 || return 1
  gnome-extensions enable ubuntu-appindicators@ubuntu.com >/dev/null 2>&1 || true
  gnome-extensions enable appindicatorsupport@rgcjonas.gmail.com >/dev/null 2>&1 || true
  sleep 1
  is_appindicator_enabled
}

cleanup_pid() {
  if [[ -f "${tray_pid_file}" ]] && [[ "$(cat "${tray_pid_file}" 2>/dev/null || true)" == "$1" ]]; then
    rm -f "${tray_pid_file}"
  fi
}

ensure_single_instance() {
  if [[ -f "${tray_pid_file}" ]]; then
    local old_pid
    old_pid="$(cat "${tray_pid_file}" 2>/dev/null || true)"
    if [[ -n "${old_pid}" ]] && kill -0 "${old_pid}" 2>/dev/null; then
      return 1
    fi
    rm -f "${tray_pid_file}"
  fi
  return 0
}

start_rust_tray() {
  if [[ -x ./target/release/amai-tray ]]; then
    ./target/release/amai-tray &
  else
    cargo run --quiet --release --bin amai-tray &
  fi
  local tray_pid="$!"
  echo "${tray_pid}" > "${tray_pid_file}"
  sleep 3
  if kill -0 "${tray_pid}" 2>/dev/null; then
    wait "${tray_pid}"
    cleanup_pid "${tray_pid}"
    return 0
  fi
  cleanup_pid "${tray_pid}"
  return 1
}

if [[ "${1:-}" == "--menu" ]]; then
  exec "${repo_root}/scripts/amai_tray_menu.sh" --menu
fi

if [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
  exit 0
fi

if [[ "${XDG_CURRENT_DESKTOP:-}" == *GNOME* ]] && ! is_appindicator_enabled; then
  try_enable_appindicator || true
  if ! is_appindicator_enabled; then
    show_info "Для иконки Amai в трее нужно включить AppIndicator GNOME. Это уже попробовано автоматически. Если значка нет — выполните один раз выход/вход в сессию."
  fi
fi

if ! ensure_single_instance; then
  exit 0
fi

if start_rust_tray; then
  exit 0
fi

if [[ -n "${DISPLAY:-}" ]]; then
  exec "${repo_root}/scripts/amai_tray_menu.sh" --tray
fi

show_info "Трей Amai не поддерживается в текущей Wayland-сессии. Используйте приложение Amai из меню приложений."
