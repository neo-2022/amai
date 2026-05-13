#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
fakebin="${tmpdir}/bin"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"
mkdir -p "${fakebin}"

move_if_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    mkdir -p "${tmpdir}/$(dirname "$path")"
    mv "$path" "${tmpdir}/$path"
  fi
}

cleanup() {
  local path
  for path in scripts/ensure_observe_frontdoor.sh; do
    if [[ -e "${tmpdir}/$path" ]]; then
      mkdir -p "$(dirname "$path")"
      mv "${tmpdir}/$path" "$path"
    fi
  done
  if [[ -f "${state_path}" ]] && [[ "$(cat "${state_path}")" == "present" ]]; then
    mkdir -p "$(dirname "${handoff_path}")"
    cp "${snapshot_path}" "${handoff_path}"
  else
    rm -f "${handoff_path}"
  fi
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

if [[ -f "${handoff_path}" ]]; then
  printf 'present' > "${state_path}"
  cp "${handoff_path}" "${snapshot_path}"
else
  printf 'absent' > "${state_path}"
fi

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
    exit 28
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_continuity_frontdoor_transport_fallback_sequence: missing ./target/release/amai" >&2
  exit 1
fi

headline="proof handoff transport fallback sequence"
next_step="prove inter-script handoff transport fallback preserves continuity"

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_compact_chat.sh --project amai --namespace continuity --json \
  >/tmp/proof_continuity_transport_fallback_compact.out
jq -e '
  .project.code == "amai"
  and .namespace.code == "continuity"
  and .handoff.headline == "compact headline"
' /tmp/proof_continuity_transport_fallback_compact.out >/dev/null

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh --project amai --namespace continuity --percent 90 \
  >/tmp/proof_continuity_transport_fallback_target.out
jq -e '
  .target_percent == 90
  and .operator_notice.exact_chat_command == "экономия_90"
' /tmp/proof_continuity_transport_fallback_target.out >/dev/null

start_epoch_ms="$(date +%s%3N)"
handoff_payload="$(
  PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
    timeout 8s ./scripts/continuity_handoff.sh \
      --project amai \
      --namespace continuity \
      --headline "${headline}" \
      --next-step "${next_step}"
)"
end_epoch_ms="$(date +%s%3N)"
elapsed_ms="$((end_epoch_ms - start_epoch_ms))"

printf '%s\n' "${handoff_payload}" | jq -e \
  --arg headline "${headline}" \
  --arg next_step "${next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  >/dev/null

if (( elapsed_ms >= 8000 )); then
  echo "proof_continuity_frontdoor_transport_fallback_sequence: shell sequence hit timeout-budget instead of fast handoff fallback (${elapsed_ms} ms)" >&2
  exit 1
fi

grep -Fq -- "- headline: ${headline}" "${handoff_path}"
grep -Fq -- "- next_step: ${next_step}" "${handoff_path}"

echo "proof_continuity_frontdoor_transport_fallback_sequence: PASS (${elapsed_ms} ms)"
