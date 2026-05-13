#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

work_dir="$(mktemp -d)"

menu_script="${repo_root}/scripts/amai_tray_menu.sh"
menu_backup="${work_dir}/amai_tray_menu.sh.bak"
cp -a "${menu_script}" "${menu_backup}"

tray_bin="${repo_root}/target/release/amai-tray"
tray_mode_backup=""
if [[ -e "${tray_bin}" ]]; then
  tray_mode_backup="$(stat -c '%a' "${tray_bin}")"
  chmod -x "${tray_bin}" || true
fi

cleanup() {
  cp -f "${menu_backup}" "${menu_script}" || true
  if [[ -n "${tray_mode_backup}" ]]; then
    chmod "${tray_mode_backup}" "${tray_bin}" || true
  fi
  rm -rf "${work_dir}" || true
}
trap cleanup EXIT

cat > "${menu_script}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "${1:-}" >> "${AMAI_TEST_MENU_LOG:?}"
exit 0
EOF
chmod +x "${menu_script}"

mock_bin_dir="${work_dir}/bin"
mkdir -p "${mock_bin_dir}"

cat > "${mock_bin_dir}/cargo" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "${mock_bin_dir}/cargo"

cat > "${mock_bin_dir}/zenity" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "${mock_bin_dir}/zenity"

state_dir_base="${work_dir}/state"
mkdir -p "${state_dir_base}"

# Case 1: no GUI -> safe exit
env -u DISPLAY -u WAYLAND_DISPLAY XDG_STATE_HOME="${state_dir_base}/nogui" \
  "${repo_root}/scripts/run_amai_tray.sh" >/tmp/proof_tray_nogui.log 2>&1

# Case 2: Wayland without working rust tray -> user-facing fallback message
wayland_out="$(
  env -u DISPLAY WAYLAND_DISPLAY="wayland-0" XDG_STATE_HOME="${state_dir_base}/wayland" \
    PATH="${mock_bin_dir}:${PATH}" \
    "${repo_root}/scripts/run_amai_tray.sh" 2>&1 || true
)"
printf '%s\n' "${wayland_out}" | rg -q "не поддерживается в текущей Wayland-сессии"

echo "proof_tray_wayland_x11_fallback: PASS"
