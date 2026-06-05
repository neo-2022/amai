#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
fakebin="${tmpdir}/bin"
mkdir -p "${fakebin}"

move_if_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    mkdir -p "${tmpdir}/$(dirname "$path")"
    mv "$path" "${tmpdir}/$path"
  fi
}

restore_all() {
  local path
  if [[ -n "${stale_holder_pid:-}" ]] && kill -0 "${stale_holder_pid}" >/dev/null 2>&1; then
    kill "${stale_holder_pid}" >/dev/null 2>&1 || true
    wait "${stale_holder_pid}" >/dev/null 2>&1 || true
  fi
  for path in target/release/amai; do
    if [[ -e "${tmpdir}/$path" ]]; then
      mkdir -p "$(dirname "$path")"
      mv "${tmpdir}/$path" "$path"
    fi
  done
  rm -rf "${tmpdir}"
}

trap restore_all EXIT

move_if_exists target/release/amai

binary_marker="${tmpdir}/binary-invoked"
stale_pid_file="${tmpdir}/stale.pid"

sleep 600 &
stale_holder_pid="$!"
printf '%s\n' "${stale_holder_pid}" > "${stale_pid_file}"

export AMAI_PROOF_BINARY_MARKER="${binary_marker}"
export AMAI_PROOF_STALE_PID_FILE="${stale_pid_file}"

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exit 0
EOF
chmod +x "${fakebin}/curl"

cat > "${fakebin}/pgrep" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat "${AMAI_PROOF_STALE_PID_FILE}"
EOF
chmod +x "${fakebin}/pgrep"

cat > "${fakebin}/readlink" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  /proc/*/exe)
    printf '%s\n' '/home/art/agent-memory-index/target/release/amai (deleted)'
    ;;
  *)
    exec /usr/bin/readlink "$@"
    ;;
esac
EOF
chmod +x "${fakebin}/readlink"

cat > "${fakebin}/flock" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "-n" && "${2:-}" == "9" ]]; then
  if kill -0 "$(cat "${AMAI_PROOF_STALE_PID_FILE}")" >/dev/null 2>&1; then
    exit 1
  fi
  exit 0
fi
exit 0
EOF
chmod +x "${fakebin}/flock"

mkdir -p target/release
cat > target/release/amai <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "binary invoked: $*" > "${AMAI_PROOF_BINARY_MARKER}"
exit 0
EOF
chmod +x target/release/amai

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=0.0.0.0:9464 \
  ./scripts/ensure_observe_frontdoor.sh --path /api/continuity-handoff

for _ in $(seq 1 50); do
  [[ -f "${binary_marker}" ]] && break
  sleep 0.1
done

if [[ ! -f "${binary_marker}" ]]; then
  echo "proof_observe_frontdoor_stale_restart: launcher did not reach fake binary" >&2
  exit 1
fi

if ! kill -0 "${stale_holder_pid}" >/dev/null 2>&1; then
  :
else
  echo "proof_observe_frontdoor_stale_restart: stale holder still alive after restart" >&2
  exit 1
fi

echo "proof_observe_frontdoor_stale_restart: PASS"
