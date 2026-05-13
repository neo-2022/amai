#!/usr/bin/env bash
set -euo pipefail

repo_root="${AMAI_REPO_ROOT:-$HOME/.local/share/amai/repo}"
stamp_file="${XDG_STATE_HOME:-$HOME/.local/state}/amai/vscode-connect-offer.stamp"
mkdir -p "$(dirname "$stamp_file")"
AMAI_DIALOG_TIMEOUT_SEC="${AMAI_DIALOG_TIMEOUT_SEC:-25}"

has_vscode_like() {
  command -v code >/dev/null 2>&1 || command -v codium >/dev/null 2>&1 || [[ -d "$HOME/.config/Code" || -d "$HOME/.config/VSCodium" || -d "$HOME/.vscode-oss" ]]
}

gui_dialog_allowed() {
  if [[ "${AMAI_FORCE_GUI_DIALOGS:-0}" == "1" ]]; then
    return 0
  fi
  [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]] || return 1
  [[ -t 1 ]] || return 1
  return 0
}

has_amai_mcp_in_vscode() {
  local file
  for file in \
    "$HOME/.config/Code/User/mcp.json" \
    "$HOME/.config/VSCodium/User/mcp.json" \
    "$HOME/.vscode-oss/User/mcp.json"
  do
    [[ -f "$file" ]] || continue
    if rg -n '"amai"\s*:' "$file" >/dev/null 2>&1; then
      return 0
    fi
  done
  return 1
}

connect_now() {
  [[ -d "$repo_root" ]] || return 1
  "$repo_root/scripts/install_amai.sh" --client vscode --stack-profile default --yes >/dev/null 2>&1
}

show_offer() {
  local title="Amai"
  local text="Обнаружен VS Code/Codium. Подключить Amai к VS Code сейчас?"
  gui_dialog_allowed || return 1
  if command -v zenity >/dev/null 2>&1; then
    zenity --question --title="$title" --text="$text" --ok-label="Подключить" --cancel-label="Позже"
    return $?
  fi
  if command -v kdialog >/dev/null 2>&1; then
    kdialog --title "$title" --yes-label "Подключить" --no-label "Позже" --yesno "$text"
    return $?
  fi
  return 1
}

show_result() {
  local ok="$1"
  local title="Amai"
  local text
  if [[ "$ok" == "1" ]]; then
    text="Amai подключена к VS Code/Codium."
  else
    text="Не удалось подключить Amai автоматически. Запустите: $repo_root/scripts/install_amai.sh --client vscode --yes"
  fi
  if gui_dialog_allowed; then
    if command -v zenity >/dev/null 2>&1; then
      zenity --info --title="$title" --text="$text" --ok-label="OK" --timeout="$AMAI_DIALOG_TIMEOUT_SEC" || true
      return
    fi
    if command -v kdialog >/dev/null 2>&1; then
      kdialog --title "$title" --msgbox "$text" || true
      return
    fi
  fi
  printf '%s\n' "$text"
}

main() {
  [[ -d "$repo_root" ]] || exit 0
  has_vscode_like || exit 0
  has_amai_mcp_in_vscode && exit 0

  # prevent nagging too frequently
  if [[ -f "$stamp_file" ]]; then
    last="$(cat "$stamp_file" 2>/dev/null || echo 0)"
    now="$(date +%s)"
    if [[ $((now - last)) -lt 21600 ]]; then
      exit 0
    fi
  fi
  date +%s > "$stamp_file"

  if show_offer; then
    if connect_now; then
      show_result 1
    else
      show_result 0
    fi
  fi
}

main "$@"
