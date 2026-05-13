#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/amai"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/amai"
mkdir -p "${config_dir}" "${state_dir}"

notifications_disabled_file="${config_dir}/tray_notifications_disabled"
last_status_file="${state_dir}/tray_last_status.txt"
tray_pid_file="${state_dir}/tray.pid"

is_gui_available() {
  [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]
}

resolve_tray_icon() {
  local candidate
  for candidate in \
    "${repo_root}/brand/amai_mark.svg" \
    "${repo_root}/tools/vscode-amai-bridge/media/amai-extension.svg" \
    "${repo_root}/tools/vscode-amai-bridge/media/amai-extension.png" \
    "${repo_root}/brand/amai_mark.svg"
  do
    [[ -f "${candidate}" ]] && { printf '%s\n' "${candidate}"; return 0; }
  done
  printf '%s\n' "applications-system"
}

status_connected() {
  local mcp_file cmd_path
  for mcp_file in "$HOME/.config/Code/User/mcp.json" "$HOME/.config/VSCodium/User/mcp.json"; do
    [[ -f "${mcp_file}" ]] || continue
    if command -v jq >/dev/null 2>&1; then
      if ! jq -e '.servers.amai' "${mcp_file}" >/dev/null 2>&1; then
        continue
      fi
    else
      if ! grep -Eq '"amai"[[:space:]]*:' "${mcp_file}"; then
        continue
      fi
    fi
    if command -v jq >/dev/null 2>&1; then
      cmd_path="$(jq -r '.servers.amai.command // empty' "${mcp_file}" 2>/dev/null || true)"
      if [[ -n "${cmd_path}" && -x "${cmd_path}" ]]; then
        echo "connected"
        return
      fi
    else
      if grep -Eq 'run_mcp_stdio\.sh' "${mcp_file}"; then
        echo "connected"
        return
      fi
    fi
  done
  echo "disconnected"
}

show_info() {
  local text="$1"
  if command -v zenity >/dev/null 2>&1 && is_gui_available; then
    zenity --info --title="Amai" --text="$text" >/dev/null 2>&1 && return
  fi
  printf '%s\n' "$text"
}

confirm_action() {
  local text="$1"
  if command -v zenity >/dev/null 2>&1 && is_gui_available; then
    zenity --question --title="Amai" --text="$text" >/dev/null 2>&1 && return 0
  fi
  printf '%s [y/N]: ' "$text"
  read -r reply
  [[ "${reply,,}" == "y" || "${reply,,}" == "yes" || "${reply,,}" == "д" || "${reply,,}" == "да" ]]
}

action_connect() {
  "${repo_root}/scripts/install_amai.sh" --client vscode --stack-profile default --yes
  "${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null 2>&1 || true
  show_info "Amai подключена к VS Code/Codium."
}

action_check() {
  local status
  status="$(status_connected)"
  if [[ "$status" == "connected" ]]; then
    show_info "Статус: Amai подключена."
  else
    show_info "Статус: Amai не подключена."
  fi
}

action_repair() {
  "${repo_root}/scripts/install_vscode_amai_bridge.sh" >/dev/null 2>&1 || true
  "${repo_root}/scripts/install_amai.sh" --client vscode --stack-profile default --yes
  show_info "Исправление завершено. Проверьте подключение в VS Code/Codium."
}

action_toggle_notifications() {
  if [[ -f "${notifications_disabled_file}" ]]; then
    rm -f "${notifications_disabled_file}"
    show_info "Уведомления Amai включены."
  else
    : > "${notifications_disabled_file}"
    show_info "Уведомления Amai отключены."
  fi
}

action_remove_full() {
  if ! confirm_action "Полностью удалить Amai и локальные данные?"; then
    show_info "Удаление отменено."
    return 0
  fi
  "${repo_root}/scripts/purge_amai_vscode_host.sh"
}

show_status_only() {
  local status
  status="$(status_connected)"
  if [[ "$status" == "connected" ]]; then
    echo "Amai подключена"
  else
    echo "Amai не подключена"
  fi
}

show_menu() {
  local status_label notif_label choice
  status_label="$(show_status_only)"
  if [[ -f "${notifications_disabled_file}" ]]; then
    notif_label="Включить уведомления"
  else
    notif_label="Не показывать уведомления"
  fi

  if command -v zenity >/dev/null 2>&1 && is_gui_available; then
    choice="$(
      zenity --list \
        --title="Amai" \
        --text="Статус: ${status_label}\n\nВыберите действие" \
        --column="Действие" \
        "Подключить к VS Code/Codium" \
        "Проверить подключение" \
        "Исправить автоматически" \
        "${notif_label}" \
        "Удалить Amai полностью" \
        --height=360 --width=500
    )"
  else
    echo "Статус: ${status_label}"
    echo "1) Подключить к VS Code/Codium"
    echo "2) Проверить подключение"
    echo "3) Исправить автоматически"
    echo "4) ${notif_label}"
    echo "5) Удалить Amai полностью"
    printf "Выбор [1-5]: "
    read -r raw_choice
    case "${raw_choice}" in
      1) choice="Подключить к VS Code/Codium" ;;
      2) choice="Проверить подключение" ;;
      3) choice="Исправить автоматически" ;;
      4) choice="${notif_label}" ;;
      5) choice="Удалить Amai полностью" ;;
      *) choice="" ;;
    esac
  fi

  case "${choice:-}" in
    "Подключить к VS Code/Codium")
      action_connect
      ;;
    "Проверить подключение")
      action_check
      ;;
    "Исправить автоматически")
      action_repair
      ;;
    "Не показывать уведомления"|"Включить уведомления")
      action_toggle_notifications
      ;;
    "Удалить Amai полностью")
      action_remove_full
      ;;
  esac
}

run_tray() {
  if ! is_gui_available; then
    exit 0
  fi

  if [[ -n "${WAYLAND_DISPLAY:-}" && "${AMAI_ENABLE_EXPERIMENTAL_TRAY:-0}" != "1" ]]; then
    exit 0
  fi

  if [[ -f "${tray_pid_file}" ]] && kill -0 "$(cat "${tray_pid_file}" 2>/dev/null)" 2>/dev/null; then
    exit 0
  fi
  echo "$$" > "${tray_pid_file}"
  trap 'rm -f "${tray_pid_file}"' EXIT

  if ! command -v yad >/dev/null 2>&1; then
    show_menu
    exit 0
  fi

  local -a yad_cmd=("env" "GDK_BACKEND=x11" "yad")
  if [[ "${AMAI_TRAY_BACKEND:-}" == "yad-native" ]]; then
    yad_cmd=("yad")
  fi

  local warned_fallback=0
  while true; do
    local status_text
    local tray_icon
    local notif_label
    status_text="$(show_status_only)"
    tray_icon="$(resolve_tray_icon)"
    if [[ -f "${notifications_disabled_file}" ]]; then
      notif_label="Включить уведомления"
    else
      notif_label="Не показывать уведомления"
    fi
    printf '%s\n' "${status_text}" > "${last_status_file}"
    local notify_out=""
    notify_out="$("${yad_cmd[@]}" --notification \
      --image="${tray_icon}" \
      --text="Amai: ${status_text}" \
      --command="${repo_root}/scripts/amai_tray_menu.sh --menu" 2>&1 || true)"
    if [[ "${warned_fallback}" -eq 0 ]] && [[ "${notify_out}" == *"not supported outside X11"* ]]; then
      warned_fallback=1
      show_info "Трей в этом режиме ограничен. Используйте запуск Amai из меню приложений."
    fi
    sleep 2
  done
}

case "${1:---menu}" in
  --tray) run_tray ;;
  --menu) show_menu ;;
  --status) show_status_only ;;
  --connect) action_connect ;;
  --check) action_check ;;
  --repair) action_repair ;;
  --toggle-notifications) action_toggle_notifications ;;
  --remove-full) action_remove_full ;;
  *) show_menu ;;
esac
