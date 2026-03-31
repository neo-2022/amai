#!/usr/bin/env bash
set -euo pipefail

repo_root="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

if [[ "${AMAI_SKIP_MCP_ORPHAN_CLEANUP:-0}" == "1" ]]; then
  exit 0
fi

if [[ "$(uname -s)" != "Linux" || ! -d /proc ]]; then
  exit 0
fi

is_target_command() {
  local cmdline="$1"
  [[ "${cmdline}" == *" mcp serve"* ]] || return 1
  [[ "${cmdline}" == *"amai mcp serve"* ]] || return 1
}

process_cwd_matches_repo() {
  local pid="$1"
  local cwd
  cwd="$(readlink -f "/proc/${pid}/cwd" 2>/dev/null || true)"
  [[ -n "${cwd}" && "${cwd}" == "${repo_root}" ]]
}

process_is_orphan() {
  local pid="$1"
  local ppid
  local parent_cmd
  ppid="$(awk '{print $4}' "/proc/${pid}/stat" 2>/dev/null || true)"
  [[ -n "${ppid}" ]] || return 1
  if [[ "${ppid}" == "1" ]]; then
    return 0
  fi
  parent_cmd="$(tr '\0' ' ' < "/proc/${ppid}/cmdline" 2>/dev/null || true)"
  [[ "${parent_cmd}" == *"systemd --user"* ]]
}

terminate_pid() {
  local pid="$1"
  kill "${pid}" 2>/dev/null || return 0
  for _ in 1 2 3 4 5; do
    if [[ ! -d "/proc/${pid}" ]]; then
      return 0
    fi
    sleep 0.1
  done
  kill -9 "${pid}" 2>/dev/null || true
}

for proc_dir in /proc/[0-9]*; do
  [[ -d "${proc_dir}" ]] || continue
  pid="${proc_dir##*/}"
  [[ "${pid}" != "$$" ]] || continue
  [[ "${pid}" != "${BASHPID:-$$}" ]] || continue

  cmdline="$(tr '\0' ' ' < "${proc_dir}/cmdline" 2>/dev/null || true)"
  [[ -n "${cmdline}" ]] || continue

  is_target_command "${cmdline}" || continue
  process_cwd_matches_repo "${pid}" || continue
  process_is_orphan "${pid}" || continue
  terminate_pid "${pid}"
done
