#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

WINDOWS_ISO_PATH=""
WORK_DIR=""
KEEP_VM="${KEEP_VM:-false}"
TIMEOUT_SECONDS="${WINDOWS_VM_TIMEOUT_SECONDS:-5400}"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/proof_windows_vm_lab.sh --iso-path /path/to/windows.iso [--work-dir /path/to/workdir] [--keep-vm]

Что делает:
- собирает no-prompt Windows ISO для честного unattended boot;
- собирает FAT payload с Autounattend + SetupComplete + validation harness;
- прогоняет Windows VM до fail-closed proof локального install path;
- вытаскивает evidence и проверяет expected fail-closed message.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iso-path)
      WINDOWS_ISO_PATH="${2:?missing value for --iso-path}"
      shift 2
      ;;
    --work-dir)
      WORK_DIR="${2:?missing value for --work-dir}"
      shift 2
      ;;
    --keep-vm)
      KEEP_VM="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "${WINDOWS_ISO_PATH}" ]]; then
  echo "--iso-path is required" >&2
  exit 2
fi

require_tool() {
  local tool="${1:?tool required}"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required tool not found: $tool" >&2
    exit 2
  fi
}

require_tool qemu-system-x86_64
require_tool qemu-img
require_tool xorriso
require_tool genisoimage
require_tool mkfs.fat
require_tool swtpm
require_tool 7z
require_tool python3
require_tool md5sum
require_tool sudo

resolve_ovmf_code() {
  local candidate
  for candidate in \
    /usr/share/OVMF/OVMF_CODE_4M.fd \
    /usr/share/OVMF/OVMF_CODE.fd \
    /usr/share/edk2/ovmf/OVMF_CODE.fd \
    /usr/share/edk2-ovmf/x64/OVMF_CODE.fd
  do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

resolve_ovmf_vars_template() {
  local candidate
  for candidate in \
    /usr/share/OVMF/OVMF_VARS_4M.fd \
    /usr/share/OVMF/OVMF_VARS.fd \
    /usr/share/edk2/ovmf/OVMF_VARS.fd \
    /usr/share/edk2-ovmf/x64/OVMF_VARS.fd
  do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

select_qemu_accel_args() {
  if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    QEMU_ACCEL_NAME="kvm"
    QEMU_ACCEL_ARGS=(-machine q35,accel=kvm)
  else
    QEMU_ACCEL_NAME="tcg"
    QEMU_ACCEL_ARGS=(-machine q35,accel=tcg)
  fi
}

wait_for_pid_exit() {
  local pid_file="${1:?pid file required}"
  local timeout_seconds="${2:?timeout required}"
  local delay="${3:-10}"
  local pid
  pid="$(cat "$pid_file")"
  local elapsed=0
  while kill -0 "$pid" >/dev/null 2>&1; do
    if (( elapsed >= timeout_seconds )); then
      echo "timed out waiting for qemu to stop" >&2
      return 1
    fi
    sleep "$delay"
    elapsed=$((elapsed + delay))
  done
  return 0
}

force_stop_pid() {
  local pid_file="${1:?pid file required}"
  [[ -f "$pid_file" ]] || return 0
  local pid
  pid="$(cat "$pid_file" 2>/dev/null || true)"
  [[ -n "$pid" ]] || return 0
  kill "$pid" >/dev/null 2>&1 || true
  sleep 2
  if kill -0 "$pid" >/dev/null 2>&1; then
    kill -9 "$pid" >/dev/null 2>&1 || true
  fi
}

wait_for_file() {
  local path="${1:?path required}"
  local attempts="${2:-30}"
  local delay="${3:-1}"
  local i
  for i in $(seq 1 "$attempts"); do
    [[ -S "$path" || -f "$path" ]] && return 0
    sleep "$delay"
  done
  return 1
}

build_no_prompt_iso() {
  local source_iso="${1:?source iso required}"
  local output_iso="${2:?output iso required}"
  local extract_dir="${3:?extract dir required}"

  rm -rf "$extract_dir"
  mkdir -p "$extract_dir"
  7z x -y "$source_iso" "-o$extract_dir" >/dev/null
  if [[ ! -f "$extract_dir/efi/microsoft/boot/efisys_noprompt.bin" ]]; then
    echo "source ISO does not contain efi/microsoft/boot/efisys_noprompt.bin" >&2
    exit 2
  fi
  local volume_id
  volume_id="$(xorriso -indev "$source_iso" -pvd_info 2>/dev/null | sed -n "/Volume [Ii]d/ { s/.*Volume [Ii]d[[:space:]]*:[[:space:]]*'\\([^']*\\)'.*/\\1/p; t done; s/.*Volume [Ii]d[[:space:]]*:[[:space:]]*\\([^[:space:]]*\\).*/\\1/p; :done }" | head -n 1)"
  [[ -n "$volume_id" ]] || volume_id="CCCOMA_X64FRE_RU-RU_DV9"
  xorriso -as mkisofs \
    -iso-level 3 \
    -volid "$volume_id" \
    -eltorito-boot boot/etfsboot.com \
    -no-emul-boot \
    -boot-load-size 8 \
    -eltorito-alt-boot \
    -e efi/microsoft/boot/efisys_noprompt.bin \
    -no-emul-boot \
    -o "$output_iso" \
    "$extract_dir" >/dev/null
}

mount_payload_image() {
  local image_path="${1:?image path required}"
  local mount_dir="${2:?mount dir required}"
  sudo mount -o loop "$image_path" "$mount_dir"
}

unmount_payload_image() {
  local mount_dir="${1:?mount dir required}"
  sudo umount "$mount_dir" >/dev/null 2>&1 || true
}

build_payload_image() {
  local payload_dir="${1:?payload dir required}"
  local output_img="${2:?output image required}"
  local mount_dir="${3:?mount dir required}"

  rm -f "$output_img"
  truncate -s 1440K "$output_img"
  mkfs.fat "$output_img" >/dev/null
  mkdir -p "$mount_dir"
  mount_payload_image "$output_img" "$mount_dir"
  trap 'unmount_payload_image "'"$mount_dir"'"; rmdir "'"$mount_dir"'" >/dev/null 2>&1 || true' RETURN
  sudo cp -R "$payload_dir"/. "$mount_dir"/
  sudo sync
  unmount_payload_image "$mount_dir"
  trap - RETURN
  rmdir "$mount_dir" >/dev/null 2>&1 || true
}

WINDOWS_ISO_PATH="$(realpath "$WINDOWS_ISO_PATH")"
[[ -f "${WINDOWS_ISO_PATH}" ]] || { echo "iso not found: ${WINDOWS_ISO_PATH}" >&2; exit 2; }

if [[ -z "${WORK_DIR}" ]]; then
  WORK_DIR="${REPO_ROOT}/output/windows-vm-lab/$(date +%Y%m%d-%H%M%S)"
fi
WORK_DIR="$(mkdir -p "$WORK_DIR" && realpath "$WORK_DIR")"
ln -sfn "$WORK_DIR" "${REPO_ROOT}/output/windows-vm-lab/latest"

PAYLOAD_DIR="${WORK_DIR}/payload"
PAYLOAD_IMG="${WORK_DIR}/payload.img"
PAYLOAD_MOUNT_DIR="${WORK_DIR}/payload.mount"
PAYLOAD_EXTRACT_DIR="${WORK_DIR}/payload_extract"
WINDOWS_NOPROMPT_DIR="${WORK_DIR}/winiso_noprompt"
WINDOWS_NOPROMPT_ISO="${WORK_DIR}/$(basename "${WINDOWS_ISO_PATH%.iso}")_noprompt.iso"
QEMU_DISK="${WORK_DIR}/system.qcow2"
QEMU_PID_FILE="${WORK_DIR}/qemu.pid"
QEMU_SERIAL_LOG="${WORK_DIR}/serial.log"
SWTPM_DIR="${WORK_DIR}/swtpm"
SWTPM_SOCKET="${WORK_DIR}/swtpm.sock"
SWTPM_PID_FILE="${WORK_DIR}/swtpm.pid"
SUMMARY_PATH="${WORK_DIR}/evidence_windows_vm_lab_fail_closed.txt"

mkdir -p "$PAYLOAD_DIR" "$SWTPM_DIR"
cp "${REPO_ROOT}/scripts/windows_vm_lab/Autounattend.xml" "$PAYLOAD_DIR/Autounattend.xml"
cp "${REPO_ROOT}/scripts/windows_vm_lab/SetupComplete.cmd" "$PAYLOAD_DIR/SetupComplete.cmd"
cp "${REPO_ROOT}/scripts/windows_vm_lab/run_validation.cmd" "$PAYLOAD_DIR/run_validation.cmd"
cp "${REPO_ROOT}/scripts/windows_vm_lab/run_validation.ps1" "$PAYLOAD_DIR/run_validation.ps1"
cp "${REPO_ROOT}/scripts/install_amai.ps1" "$PAYLOAD_DIR/install_amai.ps1"
printf 'AMAI_WINDOWS_VM_PAYLOAD_MARKER\n' >"${PAYLOAD_DIR}/AMAI_WINDOWS_VM_PAYLOAD_MARKER.txt"

build_payload_image "$PAYLOAD_DIR" "$PAYLOAD_IMG" "$PAYLOAD_MOUNT_DIR"

if [[ ! -f "$WINDOWS_NOPROMPT_ISO" ]]; then
  build_no_prompt_iso "$WINDOWS_ISO_PATH" "$WINDOWS_NOPROMPT_ISO" "$WINDOWS_NOPROMPT_DIR"
fi

OVMF_CODE="$(resolve_ovmf_code)" || { echo "failed to locate OVMF code image" >&2; exit 2; }
OVMF_VARS_TEMPLATE="$(resolve_ovmf_vars_template)" || { echo "failed to locate OVMF vars template" >&2; exit 2; }
OVMF_VARS="${WORK_DIR}/OVMF_VARS.fd"
cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS"

qemu-img create -f qcow2 "$QEMU_DISK" 64G >/dev/null

cleanup() {
  if [[ "$KEEP_VM" != "true" ]]; then
    force_stop_pid "$QEMU_PID_FILE"
  fi
  if [[ -f "$SWTPM_PID_FILE" ]]; then
    local pid
    pid="$(cat "$SWTPM_PID_FILE" 2>/dev/null || true)"
    if [[ -n "$pid" ]]; then
      kill "$pid" >/dev/null 2>&1 || true
    fi
  fi
}
trap cleanup EXIT

swtpm socket \
  --tpmstate "dir=${SWTPM_DIR}" \
  --ctrl "type=unixio,path=${SWTPM_SOCKET}" \
  --tpm2 \
  --daemon \
  --pid "file=${SWTPM_PID_FILE}"
wait_for_file "$SWTPM_SOCKET" 30 1 || { echo "swtpm socket did not appear" >&2; exit 2; }

select_qemu_accel_args

qemu-system-x86_64 \
  "${QEMU_ACCEL_ARGS[@]}" \
  -cpu max \
  -smp 4 \
  -m 8192 \
  -rtc base=utc \
  -display none \
  -serial "file:${QEMU_SERIAL_LOG}" \
  -daemonize \
  -pidfile "$QEMU_PID_FILE" \
  -boot order=c,once=d,menu=off \
  -drive "if=pflash,format=raw,readonly=on,file=${OVMF_CODE}" \
  -drive "if=pflash,format=raw,file=${OVMF_VARS}" \
  -chardev "socket,id=chrtpm,path=${SWTPM_SOCKET}" \
  -tpmdev "emulator,id=tpm0,chardev=chrtpm" \
  -device tpm-tis,tpmdev=tpm0 \
  -drive "if=ide,format=qcow2,file=${QEMU_DISK}" \
  -drive "if=ide,media=cdrom,format=raw,readonly=on,file=${WINDOWS_NOPROMPT_ISO}" \
  -drive "if=floppy,format=raw,file=${PAYLOAD_IMG}" \
  -nic none

wait_for_pid_exit "$QEMU_PID_FILE" "$TIMEOUT_SECONDS" 10

rm -rf "$PAYLOAD_EXTRACT_DIR"
mkdir -p "$PAYLOAD_EXTRACT_DIR"
7z x -y "$PAYLOAD_IMG" "-o${PAYLOAD_EXTRACT_DIR}" >/dev/null

RESULT_PATH="${PAYLOAD_EXTRACT_DIR}/evidence/result.txt"
LOG_PATH="${PAYLOAD_EXTRACT_DIR}/evidence/install_amai_local_fail_closed.txt"

[[ -f "$RESULT_PATH" ]] || { echo "validation result.txt not found in payload evidence" >&2; exit 1; }
[[ -f "$LOG_PATH" ]] || { echo "validation fail-closed log not found in payload evidence" >&2; exit 1; }

RESULT_TEXT="$(tr -d '\r' < "$RESULT_PATH")"
LOG_TEXT="$(tr -d '\r' < "$LOG_PATH")"

grep -Fqx 'result=PASS' <<<"$RESULT_TEXT" || { printf '%s\n' "$RESULT_TEXT" >&2; exit 1; }
grep -Fq 'expected_message_present=True' <<<"$RESULT_TEXT" || grep -Fq 'expected_message_present=true' <<<"$RESULT_TEXT" || { printf '%s\n' "$RESULT_TEXT" >&2; exit 1; }
grep -Fq 'Local Windows bootstrap install is not supported yet.' <<<"$LOG_TEXT" || { printf '%s\n' "$LOG_TEXT" >&2; exit 1; }

cat >"$SUMMARY_PATH" <<EOF
EVIDENCE_WINDOWS_VM_LAB_FAIL_CLOSED
status=PASS
validation_mode=windows_vm_local_fail_closed
windows_iso=${WINDOWS_ISO_PATH}
windows_iso_md5=$(md5sum "$WINDOWS_ISO_PATH" | awk '{print $1}')
windows_noprompt_iso=${WINDOWS_NOPROMPT_ISO}
windows_noprompt_iso_md5=$(md5sum "$WINDOWS_NOPROMPT_ISO" | awk '{print $1}')
qemu_accel=${QEMU_ACCEL_NAME}
payload_image=${PAYLOAD_IMG}
payload_result=${RESULT_PATH}
payload_log=${LOG_PATH}
serial_log=${QEMU_SERIAL_LOG}
EOF

printf 'Windows VM fail-closed proof complete\n'
printf 'Artifacts: %s\n' "$WORK_DIR"
