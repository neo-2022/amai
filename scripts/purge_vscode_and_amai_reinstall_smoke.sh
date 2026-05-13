#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  purge_vscode_and_amai_reinstall_smoke.sh --i-understand

Danger:
  This script removes Visual Studio Code (deb + snap) and Amai from the current user/machine,
  then performs a fresh GitHub install smoke run.

It is intended for destructive "clean machine" reproduction and should be run only on the target host.
EOF
}

if [[ "${1:-}" != "--i-understand" ]]; then
  usage
  exit 2
fi

say() { printf '%s\n' "$*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

sudo_maybe() {
  if [[ "${EUID}" -eq 0 ]]; then
    "$@"
    return
  fi
  if have sudo; then
    sudo "$@"
    return
  fi
  say "ERROR: sudo is required for this step: $*"
  exit 1
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
managed_clone_root="${AMAI_GITHUB_CLONE_DIR:-${HOME}/.local/share/amai/repo}"

say "== Stop VS Code processes"
pkill -u "${USER}" -f '/usr/share/code/code' 2>/dev/null || true
pkill -u "${USER}" -f '/snap/code/.*/usr/share/code/code' 2>/dev/null || true
pkill -u "${USER}" -f '^/snap/bin/code' 2>/dev/null || true

say "== Remove VS Code (deb/apt) if present"
if have dpkg && dpkg -s code >/dev/null 2>&1; then
  sudo_maybe apt-get remove -y --purge code || true
  sudo_maybe apt-get autoremove -y || true
fi

say "== Remove VS Code (snap) if present"
if have snap && snap list code >/dev/null 2>&1; then
  sudo_maybe snap remove --purge code || true
fi

say "== Remove VS Code user config/traces"
rm -rf "${HOME}/.config/Code" \
  "${HOME}/.vscode" \
  "${HOME}/.vscode-oss" \
  "${HOME}/snap/code" \
  "${HOME}/.local/share/applications/code.desktop" \
  "${HOME}/.local/share/applications/code_code.desktop" \
  "${HOME}/.local/share/applications/code_code-url-handler.desktop" || true
if have update-desktop-database; then
  update-desktop-database "${HOME}/.local/share/applications" || true
fi

say "== Remove Amai via bootstrap (best-effort)"
if [[ -d "${managed_clone_root}" && -x "${managed_clone_root}/scripts/remove_amai.sh" ]]; then
  (cd "${managed_clone_root}" && ./scripts/remove_amai.sh --client vscode || true)
fi

say "== Remove Amai managed clone and local state"
rm -rf "${HOME}/.local/share/amai" || true

say "== Fresh install from GitHub (default profile, VS Code client)"
if curl -fsSL --max-time 10 https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh >/dev/null 2>&1; then
  bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) \
    --client vscode \
    --stack-profile default \
    --yes
else
  say "WARN: raw.githubusercontent.com is not reachable; falling back to git/tarball bootstrap"
  if have git; then
    clone_dir="${managed_clone_root}"
    if [[ -d "${clone_dir}/.git" ]]; then
      git -C "${clone_dir}" fetch --depth 1 origin || true
      git -C "${clone_dir}" checkout --force main || true
      git -C "${clone_dir}" reset --hard origin/main || true
    else
      rm -rf "${clone_dir}" || true
      git clone --depth 1 https://github.com/neo-2022/amai.git "${clone_dir}"
    fi
    (cd "${clone_dir}" && ./scripts/install_amai.sh --client vscode --stack-profile default --yes)
  else
    tmp="$(mktemp -d)"
    trap "rm -rf '${tmp}'" RETURN
    curl -fL --retry 5 --retry-delay 1 --retry-all-errors -o "${tmp}/amai.tgz" \
      https://codeload.github.com/neo-2022/amai/tar.gz/refs/heads/main
    tar -xzf "${tmp}/amai.tgz" -C "${tmp}"
    bash "${tmp}/amai-main/scripts/install_amai.sh" --client vscode --stack-profile default --yes
  fi
fi

say "== Post-check: stack status"
if [[ -x "${managed_clone_root}/scripts/status.sh" ]]; then
  "${managed_clone_root}/scripts/status.sh"
else
  say "WARN: expected ${managed_clone_root}/scripts/status.sh after install, but not found."
fi

say "OK: purge+reinstall smoke finished"
