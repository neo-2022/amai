#!/usr/bin/env bash
set -euo pipefail

show_done_dialog() {
  local message="Amai, VS Code/Codium и их следы удалены полностью."
  local title="Amai purge complete"
  if command -v zenity >/dev/null 2>&1; then
    zenity --info --title="${title}" --text="${message}" --ok-label="OK" || true
    return
  fi
  if command -v kdialog >/dev/null 2>&1; then
    kdialog --title "${title}" --msgbox "${message}" || true
    return
  fi
  if command -v xmessage >/dev/null 2>&1; then
    xmessage -center "${message}" || true
    return
  fi
  printf '%s\n' "${message}"
}

stop_processes() {
  sudo pkill -9 -f '/VSCode-linux-x64/code|/usr/lib64/codium/codium|/usr/bin/code|/bin/code|amai|ami-postgres|ami-qdrant|ami-minio|ami-nats' 2>/dev/null || true
}

purge_user_tree() {
  local user_home="$1"
  sudo rm -rf \
    "${user_home}/.local/share/amai" \
    "${user_home}/.config/Code" \
    "${user_home}/.vscode" \
    "${user_home}/.vscode-oss" \
    "${user_home}/.config/VSCodium" \
    "${user_home}/.local/share/VSCodium" \
    "${user_home}/.local/opt/VSCode-linux-x64" \
    "${user_home}/.local/bin/code" \
    "${user_home}/.local/bin/codium" \
    "${user_home}/.cache/Code" \
    "${user_home}/.cache/vscode" \
    "${user_home}/.cache/vscodium" \
    "${user_home}/.local/share/code" \
    "${user_home}/.local/share/vscode" \
    "${user_home}/.local/share/applications/code.desktop"
}

purge_system_tree() {
  sudo rm -rf \
    /usr/lib/code \
    /usr/lib64/codium \
    /usr/share/code \
    /usr/share/applications/code.desktop \
    /usr/share/applications/codium.desktop \
    /usr/share/icons/hicolor/*/apps/code.png \
    /usr/share/icons/hicolor/*/apps/codium.png \
    /opt/visual-studio-code \
    /usr/bin/code \
    /bin/code \
    /usr/bin/codium \
    /bin/codium
}

main() {
  if ! command -v sudo >/dev/null 2>&1; then
    echo "sudo is required" >&2
    exit 1
  fi

  sudo -v
  stop_processes

  purge_user_tree "${HOME}"

  for home in /home/*; do
    [[ -d "${home}" ]] || continue
    [[ "${home}" == "${HOME}" ]] && continue
    purge_user_tree "${home}"
  done

  purge_system_tree
  stop_processes

  show_done_dialog
}

main "$@"
