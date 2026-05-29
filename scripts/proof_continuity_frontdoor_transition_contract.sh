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
mode="${AMAI_FAKE_FRONTDOOR_MODE:-valid_sequence}"
case "${mode}:${last_arg}" in
  valid_sequence:*"/api/client-budget-compact-chat")
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"chat_start_restore":{"prompt_text":"restore prompt"},"operator_notice":{"kind":"client_budget_compact_chat_requested","required_host_action":"open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"},"handoff":{"headline":"compact headline","next_step":"compact next"}}}'
    ;;
  valid_sequence:*"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":90,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":"budget target ready"}}}'
    ;;
  valid_sequence:*"/api/continuity-handoff")
    printf '%s\n' '{"continuity_handoff":{"headline":"handoff headline","next_step":"handoff next","project":{"code":"amai"},"namespace":{"code":"continuity"}},"status":"ok"}'
    ;;
  compact_receives_target_shape:*"/api/client-budget-compact-chat")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":90,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":"budget target ready"}}}'
    ;;
  target_receives_compact_shape:*"/api/client-budget-target")
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"kind":"client_budget_compact_chat_requested"},"handoff":{"headline":"compact headline","next_step":"compact next"}}}'
    ;;
  handoff_receives_target_shape:*"/api/continuity-handoff")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":90,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":"budget target ready"}}}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_FRONTDOOR_MODE=valid_sequence \
  ./scripts/continuity_compact_chat.sh --project amai --namespace continuity --json \
  >/tmp/proof_continuity_frontdoor_transition_compact.out
jq -e '
  .project.code == "amai"
  and .namespace.code == "continuity"
  and .chat_start_restore.prompt_text == "restore prompt"
  and .operator_notice.kind == "client_budget_compact_chat_requested"
  and .operator_notice.required_host_action == "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
  and .handoff.headline == "compact headline"
  and .handoff.next_step == "compact next"
' /tmp/proof_continuity_frontdoor_transition_compact.out >/dev/null

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_FRONTDOOR_MODE=valid_sequence \
  ./scripts/continuity_client_budget_target.sh --project amai --namespace continuity --percent 90 \
  >/tmp/proof_continuity_frontdoor_transition_target.out
jq -e '
  .target_percent == 90
  and .project.code == "amai"
  and .namespace.code == "continuity"
  and .operator_notice.exact_chat_command == "экономия_90"
  and .operator_notice.message_text == "budget target ready"
' /tmp/proof_continuity_frontdoor_transition_target.out >/dev/null

PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_FRONTDOOR_MODE=valid_sequence \
  ./scripts/continuity_handoff.sh --project amai --namespace continuity --headline "local headline" --next-step "local next" \
  >/tmp/proof_continuity_frontdoor_transition_handoff.out
jq -e '
  .status == "ok"
  and .continuity_handoff.headline == "handoff headline"
  and .continuity_handoff.next_step == "handoff next"
  and .continuity_handoff.project.code == "amai"
  and .continuity_handoff.namespace.code == "continuity"
' /tmp/proof_continuity_frontdoor_transition_handoff.out >/dev/null

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_FRONTDOOR_MODE=compact_receives_target_shape \
  ./scripts/continuity_compact_chat.sh --project amai --namespace continuity --json \
  >/tmp/proof_continuity_frontdoor_transition_compact_cross.out \
  2>/tmp/proof_continuity_frontdoor_transition_compact_cross.err; then
  echo "proof_continuity_frontdoor_transition_contract: expected compact-chat to reject client-budget-target payload shape" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_transition_contract: expected compact-chat cross-shape exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_frontdoor_transition_compact_cross.err >&2 || true
  exit 1
fi
grep -Fq "continuity compact chat: invalid API payload" /tmp/proof_continuity_frontdoor_transition_compact_cross.err

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_FRONTDOOR_MODE=target_receives_compact_shape \
  ./scripts/continuity_client_budget_target.sh --project amai --namespace continuity --percent 90 \
  >/tmp/proof_continuity_frontdoor_transition_target_cross.out \
  2>/tmp/proof_continuity_frontdoor_transition_target_cross.err; then
  echo "proof_continuity_frontdoor_transition_contract: expected client-budget-target to reject compact-chat payload shape" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_transition_contract: expected client-budget-target cross-shape exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_frontdoor_transition_target_cross.err >&2 || true
  exit 1
fi
grep -Fq "continuity client budget target: invalid API payload" /tmp/proof_continuity_frontdoor_transition_target_cross.err

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_FRONTDOOR_MODE=handoff_receives_target_shape \
  ./scripts/continuity_handoff.sh --project amai --namespace continuity --headline "local headline" --next-step "local next" \
  >/tmp/proof_continuity_frontdoor_transition_handoff_cross.out \
  2>/tmp/proof_continuity_frontdoor_transition_handoff_cross.err; then
  echo "proof_continuity_frontdoor_transition_contract: expected handoff to reject client-budget-target payload shape" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_transition_contract: expected handoff cross-shape exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_frontdoor_transition_handoff_cross.err >&2 || true
  exit 1
fi
grep -Fq "continuity handoff: invalid API payload" /tmp/proof_continuity_frontdoor_transition_handoff_cross.err

echo "proof_continuity_frontdoor_transition_contract: PASS"
