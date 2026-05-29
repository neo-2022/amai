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
  for path in scripts/ensure_observe_frontdoor.sh; do
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
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"kind":"client_budget_compact_chat_requested"},"handoff":{"headline":"compact headline","next_step":"compact next"}}}'
    ;;
  *"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":90,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":"budget target ready"}}}'
    ;;
  *"/api/continuity-handoff")
    printf '%s\n' '{"continuity_handoff":{"headline":"handoff headline","next_step":"   ","project":{"code":"amai"},"namespace":{"code":"continuity"}},"status":"accepted"}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

handoff_path="state/continuity-imports/amai/live-handoff.md"
before_sha="$(sha256sum "$handoff_path" | awk '{print $1}')"

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_compact_chat.sh --project amai --namespace continuity --json \
  >/tmp/proof_continuity_frontdoor_state_integrity_compact.out
jq -e '.handoff.headline == "compact headline"' /tmp/proof_continuity_frontdoor_state_integrity_compact.out >/dev/null

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh --project amai --namespace continuity --percent 90 \
  >/tmp/proof_continuity_frontdoor_state_integrity_target.out
jq -e '.target_percent == 90' /tmp/proof_continuity_frontdoor_state_integrity_target.out >/dev/null

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_handoff.sh --project amai --namespace continuity --headline "local headline" --next-step "local next" \
  >/tmp/proof_continuity_frontdoor_state_integrity_handoff.out \
  2>/tmp/proof_continuity_frontdoor_state_integrity_handoff.err; then
  echo "proof_continuity_frontdoor_state_integrity: expected invalid handoff payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_state_integrity: expected handoff exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_frontdoor_state_integrity_handoff.err >&2 || true
  exit 1
fi
grep -Fq "continuity handoff: invalid API payload" /tmp/proof_continuity_frontdoor_state_integrity_handoff.err

after_sha="$(sha256sum "$handoff_path" | awk '{print $1}')"
if [[ "$before_sha" != "$after_sha" ]]; then
  echo "proof_continuity_frontdoor_state_integrity: live handoff mutated after fail-closed inter-script sequence" >&2
  exit 1
fi

echo "proof_continuity_frontdoor_state_integrity: PASS"
