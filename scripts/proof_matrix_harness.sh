#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

out_root="${repo_root}/tmp/proof-matrix"
run_id="$(date +%Y%m%d-%H%M%S)"
run_dir="${out_root}/${run_id}"
mkdir -p "${run_dir}"
case_timeout="${CASE_TIMEOUT_SECONDS:-900}"

state_file="${run_dir}/install-state.json"
matrix_target="${run_dir}/matrix-mcp.json"
tray_home="${run_dir}/tray-home"
mkdir -p "${tray_home}"

cases=(M1 M2 M3 M4 M6 M8 M13 M14)

case_domain() {
  case "$1" in
    M1|M2) echo "install" ;;
    M3) echo "remove" ;;
    M4) echo "connect" ;;
    M6) echo "repair" ;;
    M8|M13|M14) echo "tray" ;;
    *) echo "unknown" ;;
  esac
}

case_flaky_note() {
  case "$1" in
    M1|M2|M4|M6) echo "possible_cold_build_delay" ;;
    *) echo "-" ;;
  esac
}

run_case() {
  local id="$1"
  local log="${run_dir}/${id}.log"
  local started ended duration status
  started="$(date +%s)"
  status="PASS"

  {
    echo "case=${id}"
    echo "domain=$(case_domain "$id")"
    echo "started=$(date -u +%FT%TZ)"
    case "${id}" in
      M1)
        timeout "${case_timeout}" ./scripts/proof_install_auto.sh
        ;;
      M2)
        timeout "${case_timeout}" ./scripts/proof_install_from_github.sh
        ;;
      M3)
        timeout "${case_timeout}" ./scripts/proof_remove_amai_cargo_debug_fallback.sh
        ;;
      M4)
        timeout "${case_timeout}" env AMAI_INSTALL_STATE_PATH="${state_file}" ./scripts/install_amai.sh \
          --client vscode \
          --stack-profile default \
          --skip-stack \
          --skip-release-build \
          --yes \
          --output "${matrix_target}"
        rg -n '"amai"\s*:' "${matrix_target}" >/dev/null
        ;;
      M6)
        timeout "${case_timeout}" env AMAI_INSTALL_STATE_PATH="${state_file}" ./scripts/install_amai.sh \
          --client vscode \
          --stack-profile default \
          --skip-stack \
          --skip-release-build \
          --yes \
          --output "${matrix_target}"
        test "$(rg -o '"amai"' "${matrix_target}" | wc -l | tr -d ' ')" = "1"
        ;;
      M8)
        HOME="${tray_home}" XDG_CONFIG_HOME="${tray_home}/.config" XDG_STATE_HOME="${tray_home}/.state" \
          ./scripts/amai_tray_menu.sh --status | tee "${run_dir}/${id}.status.txt"
        rg -n '^Amai (не )?подключена$' "${run_dir}/${id}.status.txt" >/dev/null
        ;;
      M13)
        HOME="${tray_home}" XDG_CONFIG_HOME="${tray_home}/.config" XDG_STATE_HOME="${tray_home}/.state" \
          ./scripts/amai_tray_menu.sh --toggle-notifications
        test -f "${tray_home}/.config/amai/tray_notifications_disabled"
        HOME="${tray_home}" XDG_CONFIG_HOME="${tray_home}/.config" XDG_STATE_HOME="${tray_home}/.state" \
          ./scripts/amai_tray_menu.sh --toggle-notifications
        test ! -f "${tray_home}/.config/amai/tray_notifications_disabled"
        ;;
      M14)
        timeout "${case_timeout}" ./scripts/proof_tray_wayland_x11_fallback.sh
        ;;
    esac
    echo "finished=$(date -u +%FT%TZ)"
  } >"${log}" 2>&1 || status="FAIL"

  ended="$(date +%s)"
  duration="$((ended - started))"
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "${id}" "$(case_domain "${id}")" "${status}" "${duration}" "$(case_flaky_note "${id}")" \
    >> "${run_dir}/summary.tsv"
}

printf 'case\tdomain\tstatus\tduration_s\tflaky_note\n' > "${run_dir}/summary.tsv"
for case_id in "${cases[@]}"; do
  run_case "${case_id}"
done

cat "${run_dir}/summary.tsv"
echo "logs_dir=${run_dir}"
