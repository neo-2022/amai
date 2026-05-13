#!/usr/bin/env bash
set -euo pipefail

AMAI_DIALOG_TIMEOUT_SEC="${AMAI_DIALOG_TIMEOUT_SEC:-25}"

gui_dialog_allowed() {
  if [[ "${AMAI_FORCE_GUI_DIALOGS:-0}" == "1" ]]; then
    return 0
  fi
  [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]] || return 1
  [[ -t 1 ]] || return 1
  return 0
}

show_done_dialog() {
  local message="Amai, VS Code/Codium и их следы удалены полностью."
  local title="Amai purge complete"
  if gui_dialog_allowed; then
    if command -v zenity >/dev/null 2>&1; then
      zenity --info --title="${title}" --text="${message}" --ok-label="OK" --timeout="${AMAI_DIALOG_TIMEOUT_SEC}" || true
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
  fi
  printf '%s\n' "${message}"
}

stop_processes() {
  local self_pid="$$"
  local parent_pid="${PPID:-0}"
  local pids
  local pattern
  for pattern in \
    '/VSCode-linux-x64/code' \
    '/usr/lib64/codium/codium' \
    '^/usr/bin/code' \
    '^/bin/code' \
    '^/usr/bin/codium' \
    '^/bin/codium' \
    'podman.*ami-postgres' \
    'podman.*ami-qdrant' \
    'podman.*ami-minio' \
    'podman.*ami-nats'
  do
    pids="$(pgrep -f "${pattern}" 2>/dev/null || true)"
    [[ -n "${pids}" ]] || continue
    while IFS= read -r pid; do
      [[ -n "${pid}" ]] || continue
      [[ "${pid}" == "${self_pid}" ]] && continue
      [[ "${pid}" == "${parent_pid}" ]] && continue
      sudo kill -9 "${pid}" 2>/dev/null || true
    done <<< "${pids}"
  done
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

remove_packaged_editors() {
  local pkg
  local status
  for pkg in codium code; do
    if command -v dpkg-query >/dev/null 2>&1; then
      status="$(dpkg-query -W -f='${Status}' "${pkg}" 2>/dev/null || true)"
      if [[ -n "${status}" && "${status}" != *"not-installed"* ]]; then
        sudo apt-get purge -y "${pkg}" >/dev/null 2>&1 \
          || sudo dpkg --purge --force-all "${pkg}" >/dev/null 2>&1 \
          || true
      fi
    fi

    if command -v rpm >/dev/null 2>&1 && rpm -q "${pkg}" >/dev/null 2>&1; then
      if command -v dnf >/dev/null 2>&1; then
        sudo dnf -y remove "${pkg}" >/dev/null 2>&1 || true
      elif command -v yum >/dev/null 2>&1; then
        sudo yum -y remove "${pkg}" >/dev/null 2>&1 || true
      elif command -v zypper >/dev/null 2>&1; then
        sudo zypper --non-interactive remove "${pkg}" >/dev/null 2>&1 || true
      else
        sudo rpm -e --nodeps "${pkg}" >/dev/null 2>&1 || true
      fi
    fi

    if command -v snap >/dev/null 2>&1 && snap list "${pkg}" >/dev/null 2>&1; then
      sudo snap remove --purge "${pkg}" >/dev/null 2>&1 || true
    fi
  done
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

  remove_packaged_editors
  purge_system_tree
  stop_processes

  show_done_dialog
}

main "$@"
