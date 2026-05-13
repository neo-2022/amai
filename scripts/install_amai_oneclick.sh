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

has_amai_server_in_json() {
  local target_file="$1"
  [[ -f "$target_file" ]] || return 1
  rg -n '"amai"\s*:' "$target_file" >/dev/null 2>&1
}

collect_client_connection_report() {
  local repo="$AMAI_CLONE_DIR"
  local report_file="$HOME/.cache/amai/oneclick-connection-report.txt"
  : > "$report_file"

  local auto_connected=()
  local needs_manual=()

  # VS Code / Codium
  if command -v code >/dev/null 2>&1 || command -v codium >/dev/null 2>&1 || [[ -d "$HOME/.config/Code" || -d "$HOME/.config/VSCodium" || -d "$HOME/.vscode-oss" ]]; then
    if has_amai_server_in_json "$HOME/.config/Code/User/mcp.json" || has_amai_server_in_json "$HOME/.config/VSCodium/User/mcp.json" || has_amai_server_in_json "$repo/.vscode/mcp.json"; then
      auto_connected+=("VS Code / Codium — подключено автоматически")
    else
      needs_manual+=("VS Code / Codium — проверьте MCP: $repo/.vscode/mcp.json или ~/.config/Code/User/mcp.json")
    fi
  fi

  # Cursor
  if [[ -d "$HOME/.cursor" ]] || command -v cursor >/dev/null 2>&1; then
    if has_amai_server_in_json "$HOME/.cursor/mcp.json"; then
      auto_connected+=("Cursor — подключено автоматически")
    else
      needs_manual+=("Cursor — сгенерируйте/обновите: $repo/scripts/install_amai.sh --client cursor --yes")
    fi
  fi

  # Claude Code
  if command -v claude >/dev/null 2>&1 || [[ -f "$repo/.mcp.json" ]]; then
    if has_amai_server_in_json "$repo/.mcp.json"; then
      auto_connected+=("Claude Code — подключено автоматически (project-local .mcp.json)")
    else
      needs_manual+=("Claude Code — сгенерируйте: $repo/scripts/install_amai.sh --client claude-code --yes")
    fi
  fi

  # Hermes
  if [[ -d "$HOME/.hermes" ]] || command -v hermes >/dev/null 2>&1; then
    if [[ -f "$HOME/.hermes/config.yaml" ]] && rg -n 'amai|run_mcp_stdio\.sh' "$HOME/.hermes/config.yaml" >/dev/null 2>&1; then
      auto_connected+=("Hermes — подключено автоматически")
    else
      needs_manual+=("Hermes — сгенерируйте: $repo/scripts/install_amai.sh --client hermes --yes")
    fi
  fi

  # OpenClaw
  if [[ -d "$HOME/.openclaw" ]] || command -v openclaw >/dev/null 2>&1; then
    if [[ -f "$HOME/.openclaw/openclaw.json" ]] && rg -n '"amai"\s*:' "$HOME/.openclaw/openclaw.json" >/dev/null 2>&1; then
      auto_connected+=("OpenClaw — подключено автоматически")
    else
      needs_manual+=("OpenClaw — сгенерируйте: $repo/scripts/install_amai.sh --client openclaw --yes")
    fi
  fi

  {
    printf 'AUTO_CONNECTED_COUNT=%s\n' "${#auto_connected[@]}"
    printf 'NEEDS_MANUAL_COUNT=%s\n' "${#needs_manual[@]}"
    for item in "${auto_connected[@]}"; do
      printf 'AUTO: %s\n' "$item"
    done
    for item in "${needs_manual[@]}"; do
      printf 'MANUAL: %s\n' "$item"
    done
  } >> "$report_file"

  printf '%s\n' "$report_file"
}

build_success_message() {
  local report_file="$1"
  local msg
  msg=$'Amai установлен успешно.\n\n'
  msg+=$"Путь: $AMAI_CLONE_DIR"$'\n'
  msg+=$"Клиент установки: $AMAI_CLIENT"$'\n\n'

  local auto_count manual_count
  auto_count="$(awk -F= '/^AUTO_CONNECTED_COUNT=/{print $2}' "$report_file" | tail -n1)"
  manual_count="$(awk -F= '/^NEEDS_MANUAL_COUNT=/{print $2}' "$report_file" | tail -n1)"
  auto_count="${auto_count:-0}"
  manual_count="${manual_count:-0}"

  if [[ "$auto_count" -gt 0 ]]; then
    local auto_lines=()
    mapfile -t auto_lines < <(sed -n 's/^AUTO: /- /p' "$report_file")
    msg+=$'Подключено автоматически:\n'
    for line in "${auto_lines[@]}"; do
      msg+="$line"$'\n'
    done
    msg+=$'\n'
  fi

  if [[ "$manual_count" -gt 0 ]]; then
    local manual_lines=()
    mapfile -t manual_lines < <(sed -n 's/^MANUAL: /- /p' "$report_file")
    msg+=$'Нужно подключить вручную:\n'
    for line in "${manual_lines[@]}"; do
      msg+="$line"$'\n'
    done
    msg+=$'\n'
  fi

  msg+=$'Нажмите OK.'
  printf '%s' "$msg"
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

  local report_file success_text
  report_file="$(collect_client_connection_report)"
  success_text="$(build_success_message "$report_file")"
  show_info "Amai installed" "$success_text"
}

main "$@"
