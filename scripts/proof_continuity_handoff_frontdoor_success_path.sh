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
  for path in scripts/ensure_observe_frontdoor.sh target/release/amai scripts/amai_exec.sh; do
    if [[ -e "${tmpdir}/$path" ]]; then
      mkdir -p "$(dirname "$path")"
      mv "${tmpdir}/$path" "$path"
    fi
  done
  rm -rf "${tmpdir}"
}

trap restore_all EXIT

move_if_exists scripts/ensure_observe_frontdoor.sh
cat > scripts/ensure_observe_frontdoor.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exit 0
EOF
chmod +x scripts/ensure_observe_frontdoor.sh

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/continuity-handoff")
    printf '%s\n' '{"continuity_handoff":{"headline":"api success headline","next_step":"api success next","project":{"code":"amai"},"namespace":{"code":"continuity"}},"status":"ok"}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

move_if_exists target/release/amai
move_if_exists scripts/amai_exec.sh

payload="$(
  PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
    ./scripts/continuity_handoff.sh \
      --project amai \
      --namespace continuity \
      --headline "local headline" \
      --next-step "local next"
)"

printf '%s\n' "${payload}" | jq -e '
  .status == "ok"
  and .continuity_handoff.headline == "api success headline"
  and .continuity_handoff.next_step == "api success next"
  and .continuity_handoff.project.code == "amai"
  and .continuity_handoff.namespace.code == "continuity"
' >/dev/null

echo "proof_continuity_handoff_frontdoor_success_path: PASS"
