#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
export AMAI_REPO_ROOT="${repo_root}"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/amai"
mkdir -p "${state_dir}"
tray_pid_file="${state_dir}/rust_tray.pid"
tray_log_file="${state_dir}/tray_launcher.log"

log_line() {
  printf '%s %s\n' "$(date '+%F %T')" "$*" >> "${tray_log_file}"
}

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
  if [[ ! -x ./target/release/amai-tray ]]; then
    log_line "rust tray binary missing: ${repo_root}/target/release/amai-tray"
    return 1
  fi
  ./target/release/amai-tray &
  local tray_pid="$!"
  local started_at
  started_at="$(date +%s)"
  echo "${tray_pid}" > "${tray_pid_file}"
  sleep 3
  if kill -0 "${tray_pid}" 2>/dev/null; then
    if ! wait "${tray_pid}"; then
      cleanup_pid "${tray_pid}"
      return 1
    fi
    local finished_at runtime_sec
    finished_at="$(date +%s)"
    runtime_sec="$((finished_at - started_at))"
    if [[ "${runtime_sec}" -lt 10 ]]; then
      log_line "rust tray exited too fast (${runtime_sec}s)"
      cleanup_pid "${tray_pid}"
      return 1
    fi
    cleanup_pid "${tray_pid}"
    return 0
  fi
  cleanup_pid "${tray_pid}"
  return 1
}

if [[ "${1:-}" == "--menu" ]]; then
  log_line "launcher mode=menu"
  exec "${repo_root}/scripts/amai_tray_menu.sh" --menu
fi

if [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
  log_line "exit no-gui DISPLAY='${DISPLAY:-}' WAYLAND_DISPLAY='${WAYLAND_DISPLAY:-}'"
  exit 0
fi

appindicator_missing=0
if [[ "${XDG_CURRENT_DESKTOP:-}" == *GNOME* ]] && ! is_appindicator_enabled; then
  try_enable_appindicator || true
  if ! is_appindicator_enabled; then
    appindicator_missing=1
  fi
fi

if ! ensure_single_instance; then
  log_line "skip existing rust tray pid"
  exit 0
fi

log_line "start DISPLAY='${DISPLAY:-}' WAYLAND='${WAYLAND_DISPLAY:-}' XDG_SESSION_TYPE='${XDG_SESSION_TYPE:-}'"
if start_rust_tray; then
  log_line "rust tray exit normal"
  exit 0
fi

if [[ -n "${DISPLAY:-}" ]]; then
  if [[ "${appindicator_missing}" -eq 1 ]]; then
    show_info "AppIndicator GNOME пока не активен. Если значок Amai не появился — выполните один раз выход/вход в сессию."
  fi
  if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
    log_line "fallback wayland -> yad tray"
  else
    log_line "fallback x11 -> yad tray"
  fi
  exec "${repo_root}/scripts/amai_tray_menu.sh" --tray
fi

if [[ "${appindicator_missing}" -eq 1 ]]; then
  show_info "AppIndicator GNOME пока не активен. Если значок Amai не появился — выполните один раз выход/вход в сессию."
fi
show_info "Трей Amai не поддерживается в текущей Wayland-сессии. Используйте приложение Amai из меню приложений."
