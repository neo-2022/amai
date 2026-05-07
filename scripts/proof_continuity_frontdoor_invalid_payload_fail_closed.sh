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
  for path in \
    scripts/ensure_observe_frontdoor.sh; do
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
  *"/api/client-budget-compact-chat")
    printf '%s\n' '{"continuity_compact_chat":"broken"}'
    ;;
  *"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":"broken"}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_compact_chat.sh \
    --project amai \
    --namespace continuity \
    --json \
  >/tmp/proof_continuity_compact_chat_invalid_payload.out \
  2>/tmp/proof_continuity_compact_chat_invalid_payload.err; then
  echo "proof_continuity_frontdoor_invalid_payload_fail_closed: expected compact chat malformed payload to fail closed" >&2
  exit 1
else
  status=$?
fi

if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_invalid_payload_fail_closed: expected compact-chat exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_compact_chat_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity compact chat: invalid API payload" /tmp/proof_continuity_compact_chat_invalid_payload.err

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh \
    --project amai \
    --namespace continuity \
    --percent 90 \
  >/tmp/proof_continuity_client_budget_target_invalid_payload.out \
  2>/tmp/proof_continuity_client_budget_target_invalid_payload.err; then
  echo "proof_continuity_frontdoor_invalid_payload_fail_closed: expected client-budget-target malformed payload to fail closed" >&2
  exit 1
else
  status=$?
fi

if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_invalid_payload_fail_closed: expected client-budget-target exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_client_budget_target_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity client budget target: invalid API payload" /tmp/proof_continuity_client_budget_target_invalid_payload.err

echo "proof_continuity_frontdoor_invalid_payload_fail_closed: PASS"
