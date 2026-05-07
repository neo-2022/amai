#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
unit_name="${AMAI_STACK_AUTOSTART_UNIT_NAME:-amai-stack.service}"
unit_dir="${AMAI_STACK_AUTOSTART_UNIT_DIR:-${HOME}/.config/systemd/user}"
unit_path="${unit_dir}/${unit_name}"
launcher_script="${repo_root}/scripts/run_stack_service.sh"
path_env="${HOME}/.local/bin:${HOME}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

mkdir -p "${unit_dir}"

cat >"${unit_path}" <<EOF
[Unit]
Description=Amai local stack bootstrap

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=${repo_root}
Environment=PATH=${path_env}
ExecStart=${launcher_script}

[Install]
WantedBy=default.target
EOF

if [[ "${AMAI_STACK_AUTOSTART_SKIP_SYSTEMCTL:-0}" == "1" ]]; then
  echo "Amai stack autostart unit rendered at ${unit_path}"
  exit 0
fi

if ! systemctl --user show-environment >/dev/null 2>&1; then
  echo "systemctl --user is unavailable; cannot install managed Amai stack autostart" >&2
  exit 1
fi

systemctl --user daemon-reload
systemctl --user enable --now "${unit_name}" >/dev/null

if ! systemctl --user is-active --quiet "${unit_name}"; then
  echo "Amai stack autostart service failed to activate: ${unit_name}" >&2
  journalctl --user -u "${unit_name}" -n 60 --no-pager >&2 || true
  exit 1
fi

echo "Amai stack autostart ready: ${unit_name}"
echo "Unit: ${unit_path}"
