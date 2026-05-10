#!/usr/bin/env bash
set -euo pipefail

auto_prereqs_enabled() {
  [[ "${AMAI_AUTO_INSTALL_PREREQS:-1}" != "0" ]]
}

load_os_release() {
  [[ -f /etc/os-release ]] || return 1
  # shellcheck disable=SC1091
  source /etc/os-release
}

is_debian_like_verified_host() {
  load_os_release || return 1
  [[ "${ID:-}" == "ubuntu" || "${ID:-}" == "debian" || " ${ID_LIKE:-} " == *" debian "* ]]
}

run_as_root() {
  if [[ "${EUID}" -eq 0 ]]; then
    "$@"
    return
  fi
  if command -v sudo >/dev/null 2>&1; then
    sudo "$@"
    return
  fi
  echo "Amai automatic prerequisite bootstrap requires root or sudo on verified Ubuntu/Debian hosts." >&2
  return 1
}

apt_package_available() {
  apt-cache show "$1" >/dev/null 2>&1
}

apt_install_missing() {
  local package
  local missing=()
  for package in "$@"; do
    dpkg -s "${package}" >/dev/null 2>&1 || missing+=("${package}")
  done
  [[ "${#missing[@]}" -gt 0 ]] || return 0
  export DEBIAN_FRONTEND=noninteractive
  run_as_root apt-get update
  run_as_root apt-get install -y "${missing[@]}"
}

ensure_basic_packages() {
  apt_install_missing git curl ca-certificates build-essential pkg-config libssl-dev
}

ensure_rust_toolchain() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
    rustup show active-toolchain >/dev/null 2>&1 || true
    rustup default stable >/dev/null 2>&1 || true
    rustup toolchain install stable >/dev/null 2>&1 || true
    return 0
  fi

  if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  fi

  export PATH="${HOME}/.cargo/bin:${PATH}"
  rustup default stable >/dev/null 2>&1 || true
  rustup toolchain install stable >/dev/null 2>&1 || true
  command -v cargo >/dev/null 2>&1 || {
    echo "Amai prerequisite bootstrap installed rustup but cargo is still unavailable." >&2
    return 1
  }
  command -v rustc >/dev/null 2>&1 || {
    echo "Amai prerequisite bootstrap installed rustup but rustc is still unavailable." >&2
    return 1
  }
}

ensure_docker_stack_packages() {
  local compose_package="docker-compose-v2"
  if ! apt_package_available "${compose_package}"; then
    compose_package="docker-compose-plugin"
  fi
  apt_install_missing docker.io "${compose_package}"
  if command -v systemctl >/dev/null 2>&1; then
    run_as_root systemctl enable --now docker >/dev/null 2>&1 || true
  fi
  if getent group docker >/dev/null 2>&1; then
    getent group docker | grep -Eq "(^|:|,)$USER(,|$)" || run_as_root usermod -aG docker "$USER"
  fi
}

ensure_verified_linux_prereqs() {
  local require_docker_stack="${1:-0}"

  auto_prereqs_enabled || return 0
  is_debian_like_verified_host || return 0

  ensure_basic_packages
  ensure_rust_toolchain
  if [[ "${require_docker_stack}" == "1" ]]; then
    ensure_docker_stack_packages
  fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  ensure_verified_linux_prereqs "${1:-0}"
fi
