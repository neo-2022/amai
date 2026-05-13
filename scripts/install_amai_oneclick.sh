#!/usr/bin/env bash
set -euo pipefail

AMAI_REPO_URL="${AMAI_REPO_URL:-https://github.com/neo-2022/amai.git}"
AMAI_CLONE_DIR="${AMAI_CLONE_DIR:-$HOME/.local/share/amai/repo}"
AMAI_STACK_PROFILE="${AMAI_STACK_PROFILE:-default}"
AMAI_CLIENT="${AMAI_CLIENT:-vscode}"

show_info() {
  local title="$1"
  local text="$2"
  if command -v zenity >/dev/null 2>&1; then
    zenity --info --title="$title" --text="$text" --ok-label="OK" || true
    return
  fi
  if command -v kdialog >/dev/null 2>&1; then
    kdialog --title "$title" --msgbox "$text" || true
    return
  fi
  if command -v xmessage >/dev/null 2>&1; then
    xmessage -center "$text" || true
    return
  fi
  printf '%s\n' "$text"
}

show_error() {
  local title="$1"
  local text="$2"
  if command -v zenity >/dev/null 2>&1; then
    zenity --error --title="$title" --text="$text" --ok-label="OK" || true
    return
  fi
  if command -v kdialog >/dev/null 2>&1; then
    kdialog --title "$title" --error "$text" || true
    return
  fi
  if command -v xmessage >/dev/null 2>&1; then
    xmessage -center "$text" || true
    return
  fi
  printf '%s\n' "$text" >&2
}

verify_install() {
  local repo="$AMAI_CLONE_DIR"
  [[ -d "$repo" ]] || return 1
  [[ -f "$repo/.env" ]] || return 1
  [[ -f "$repo/scripts/run_mcp_stdio.sh" ]] || return 1
  case "$AMAI_CLIENT" in
    vscode)
      [[ -f "$repo/.vscode/mcp.json" || -f "$HOME/.config/Code/User/mcp.json" || -f "$HOME/.config/VSCodium/User/mcp.json" ]] || return 1
      ;;
    generic)
      [[ -f "$repo/tmp/onboarding/generic-mcp.json" ]] || return 1
      ;;
  esac
}

main() {
  local log_file="$HOME/.cache/amai/oneclick-install.log"
  mkdir -p "$(dirname "$log_file")"
  : > "$log_file"

  {
    echo "[1/2] Installing Amai..."
    bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) \
      --repo-url "$AMAI_REPO_URL" \
      --clone-dir "$AMAI_CLONE_DIR" \
      --client "$AMAI_CLIENT" \
      --stack-profile "$AMAI_STACK_PROFILE" \
      --yes

    echo "[2/2] Verifying..."
    verify_install
  } >>"$log_file" 2>&1 || {
    show_error "Amai install failed" "Установка Amai завершилась с ошибкой.\n\nЧто сделать:\n1) Открой лог: $log_file\n2) Проверьте интернет-доступ к github.com и open-vsx.org\n3) Повторите запуск этой же команды.\n\nЕсли ошибка сохраняется — пришлите лог."
    exit 1
  }

  show_info "Amai installed" "Amai установлен успешно.\n\nПуть: $AMAI_CLONE_DIR\nКлиент: $AMAI_CLIENT\n\nНажмите OK."
}

main "$@"
