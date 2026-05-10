#!/usr/bin/env bash
set -euo pipefail

can_run_plain_docker() {
  command -v docker >/dev/null 2>&1 || return 1
  docker info >/dev/null 2>&1
}

can_run_docker_via_group_switch() {
  command -v docker >/dev/null 2>&1 || return 1
  command -v sg >/dev/null 2>&1 || return 1
  getent group docker 2>/dev/null | grep -Eq "(^|:|,)$USER(,|$)" || return 1
  sg docker -c 'docker info >/dev/null 2>&1'
}

can_run_docker_via_sudo() {
  command -v docker >/dev/null 2>&1 || return 1
  command -v sudo >/dev/null 2>&1 || return 1
  sudo -n docker info >/dev/null 2>&1
}

quoted_command() {
  printf '%q ' docker "$@"
}

if can_run_plain_docker; then
  exec docker "$@"
fi

if can_run_docker_via_group_switch; then
  exec sg docker -c "$(quoted_command "$@")"
fi

if can_run_docker_via_sudo; then
  exec sudo -n docker "$@"
fi

exec docker "$@"
