#!/usr/bin/env bash

detect_vscode_extensions_roots() {
  local reason="default"
  if [[ -n "${AMAI_VSCODE_EXTENSIONS_ROOT:-}" ]]; then
    reason="override_env"
    printf '%s\t%s\n' "${AMAI_VSCODE_EXTENSIONS_ROOT}" "${reason}"
    return 0
  fi

  local code_path code_realpath
  code_path="$(command -v code 2>/dev/null || true)"
  code_realpath=""
  if [[ -n "${code_path}" ]]; then
    code_realpath="$(readlink -f "${code_path}" 2>/dev/null || printf '%s' "${code_path}")"
  fi

  if [[ "${code_realpath}" == *"/snap/"* ]] || ( [[ -x /usr/bin/snap ]] && snap list code >/dev/null 2>&1 ); then
    reason="vscode_snap"
    if [[ -d "${HOME}/snap/code/common/.vscode" || -d "${HOME}/snap/code/common" ]]; then
      printf '%s\t%s\n' "${HOME}/snap/code/common/.vscode/extensions" "${reason}"
      return 0
    fi
    if [[ -d "${HOME}/snap/code/current/.vscode" || -d "${HOME}/snap/code/current" ]]; then
      printf '%s\t%s\n' "${HOME}/snap/code/current/.vscode/extensions" "${reason}"
      return 0
    fi
    printf '%s\t%s\n' "${HOME}/snap/code/common/.vscode/extensions" "${reason}"
    return 0
  fi

  local roots=()
  for root in "${HOME}/.vscode/extensions" "${HOME}/.vscode-oss/extensions"; do
    if compgen -G "${root}/openai.chatgpt-*" >/dev/null 2>&1; then
      roots+=("${root}")
    fi
  done
  if [[ "${#roots[@]}" -gt 0 ]]; then
    reason="openai_chatgpt_installed"
    for root in "${roots[@]}"; do
      printf '%s\t%s\n' "${root}" "${reason}"
    done
    return 0
  fi

  if [[ "${code_realpath}" == *codium* || "${code_realpath}" == *VSCodium* || -d "${HOME}/.config/VSCodium" || -d "${HOME}/.vscode-oss" ]]; then
    reason="codium_or_oss"
    printf '%s\t%s\n' "${HOME}/.vscode-oss/extensions" "${reason}"
    return 0
  fi

  printf '%s\t%s\n' "${HOME}/.vscode/extensions" "${reason}"
}
