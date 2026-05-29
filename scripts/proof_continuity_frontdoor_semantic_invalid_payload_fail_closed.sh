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
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"kind":"client_budget_compact_chat_requested"},"handoff":{"headline":"   ","next_step":"   "}}}'
    ;;
  *"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":95,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":" "}}}'
    ;;
  *"/api/continuity-handoff")
    printf '%s\n' '{"continuity_handoff":{"headline":"x","next_step":"   ","project":{"code":"amai"},"namespace":{"code":"continuity"}},"status":"accepted"}'
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
  >/tmp/proof_continuity_compact_chat_semantic_invalid_payload.out \
  2>/tmp/proof_continuity_compact_chat_semantic_invalid_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact chat semantic-invalid payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact-chat exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_compact_chat_semantic_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity compact chat: invalid API payload" /tmp/proof_continuity_compact_chat_semantic_invalid_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/client-budget-compact-chat")
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":" "},"namespace":{"code":" "},"operator_notice":{},"handoff":{"headline":"headline","next_step":"next"}}}'
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
  >/tmp/proof_continuity_compact_chat_missing_ids_payload.out \
  2>/tmp/proof_continuity_compact_chat_missing_ids_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact chat missing-id payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact-chat missing-id exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_compact_chat_missing_ids_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity compact chat: invalid API payload" /tmp/proof_continuity_compact_chat_missing_ids_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/client-budget-compact-chat")
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"chat_start_restore":{"prompt_text":"   "},"operator_notice":{"kind":"client_budget_compact_chat_requested","required_host_action":"open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"},"handoff":{"headline":"headline","next_step":"next"}}}'
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
  >/tmp/proof_continuity_compact_chat_missing_prompt_payload.out \
  2>/tmp/proof_continuity_compact_chat_missing_prompt_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact chat missing-prompt payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact-chat missing-prompt exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_compact_chat_missing_prompt_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity compact chat: invalid API payload" /tmp/proof_continuity_compact_chat_missing_prompt_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/client-budget-compact-chat")
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"chat_start_restore":{"prompt_text":"restore prompt"},"operator_notice":{"kind":"client_budget_compact_chat_requested","required_host_action":"   ","launch_clean_chat_command":"   "},"handoff":{"headline":"headline","next_step":"next"}}}'
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
  >/tmp/proof_continuity_compact_chat_missing_launch_guidance_payload.out \
  2>/tmp/proof_continuity_compact_chat_missing_launch_guidance_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact chat missing-launch-guidance payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected compact-chat missing-launch-guidance exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_compact_chat_missing_launch_guidance_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity compact chat: invalid API payload" /tmp/proof_continuity_compact_chat_missing_launch_guidance_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/client-budget-compact-chat")
    printf '%s\n' '{"continuity_compact_chat":{"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"kind":"client_budget_compact_chat_requested"},"handoff":{"headline":"   ","next_step":"   "}}}'
    ;;
  *"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":95,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":" "}}}'
    ;;
  *"/api/continuity-handoff")
    printf '%s\n' '{"continuity_handoff":{"headline":"x","next_step":"   ","project":{"code":"amai"},"namespace":{"code":"continuity"}},"status":"accepted"}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh \
    --project amai \
    --namespace continuity \
    --percent 90 \
  >/tmp/proof_continuity_client_budget_target_semantic_invalid_payload.out \
  2>/tmp/proof_continuity_client_budget_target_semantic_invalid_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected client-budget-target semantic-invalid payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected client-budget-target exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_client_budget_target_semantic_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity client budget target: invalid API payload" /tmp/proof_continuity_client_budget_target_semantic_invalid_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":"90","project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":"set budget"}}}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh \
    --project amai \
    --namespace continuity \
    --percent 90 \
  >/tmp/proof_continuity_client_budget_target_type_invalid_payload.out \
  2>/tmp/proof_continuity_client_budget_target_type_invalid_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected client-budget-target type-invalid payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected client-budget-target type-invalid exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_client_budget_target_type_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity client budget target: invalid API payload" /tmp/proof_continuity_client_budget_target_type_invalid_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/client-budget-target")
    printf '%s\n' '{"client_budget_target_update":{"target_percent":110,"project":{"code":"amai"},"namespace":{"code":"continuity"},"operator_notice":{"exact_chat_command":"экономия_90","message_text":"set budget"}}}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh \
    --project amai \
    --namespace continuity \
    --percent 90 \
  >/tmp/proof_continuity_client_budget_target_out_of_range_payload.out \
  2>/tmp/proof_continuity_client_budget_target_out_of_range_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected client-budget-target out-of-range payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected client-budget-target out-of-range exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_client_budget_target_out_of_range_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity client budget target: invalid API payload" /tmp/proof_continuity_client_budget_target_out_of_range_payload.err

cat > "${fakebin}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
last_arg="${@: -1}"
case "${last_arg}" in
  *"/api/continuity-handoff")
    printf '%s\n' '{"continuity_handoff":{"headline":"x","next_step":"next","project":{"code":" "},"namespace":{"code":"continuity"}},"status":"ok"}'
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_handoff.sh \
    --project amai \
    --namespace continuity \
    --headline "probe" \
    --next-step "probe" \
  >/tmp/proof_continuity_handoff_optional_field_invalid_payload.out \
  2>/tmp/proof_continuity_handoff_optional_field_invalid_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected handoff optional-field invalid payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected handoff optional-field invalid exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_handoff_optional_field_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity handoff: invalid API payload" /tmp/proof_continuity_handoff_optional_field_invalid_payload.err

status=0
if PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_handoff.sh \
    --project amai \
    --namespace continuity \
    --headline "probe" \
    --next-step "probe" \
  >/tmp/proof_continuity_handoff_semantic_invalid_payload.out \
  2>/tmp/proof_continuity_handoff_semantic_invalid_payload.err; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected handoff semantic-invalid payload to fail closed" >&2
  exit 1
else
  status=$?
fi
if [[ "${status}" -ne 12 ]]; then
  echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: expected handoff exit code 12, got ${status}" >&2
  cat /tmp/proof_continuity_handoff_semantic_invalid_payload.err >&2 || true
  exit 1
fi
grep -Fq "continuity handoff: invalid API payload" /tmp/proof_continuity_handoff_semantic_invalid_payload.err

echo "proof_continuity_frontdoor_semantic_invalid_payload_fail_closed: PASS"
