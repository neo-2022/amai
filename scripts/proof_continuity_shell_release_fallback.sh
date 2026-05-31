#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
compact_chat_prompt_path=".amai/continuity/compact-chat-prompt.txt"
tmpdir="$(mktemp -d)"
handoff_snapshot_path="${tmpdir}/live-handoff.snapshot"
handoff_state_path="${tmpdir}/live-handoff.state"
compact_chat_prompt_snapshot_path="${tmpdir}/compact-chat-prompt.snapshot"
compact_chat_prompt_state_path="${tmpdir}/compact-chat-prompt.state"
launcher_path="./scripts/amai_exec.sh"
ensure_observe_frontdoor_path="./scripts/ensure_observe_frontdoor.sh"
launcher_mode_before="$(stat -c '%a' "${launcher_path}")"
launcher_size_before="$(stat -c '%s' "${launcher_path}")"
ensure_observe_frontdoor_mode_before="$(stat -c '%a' "${ensure_observe_frontdoor_path}")"
if [[ "${launcher_size_before}" -le 0 ]]; then
  echo "proof_continuity_shell_release_fallback: corrupted ${launcher_path} before proof" >&2
  exit 1
fi
backup="$(mktemp ./scripts/amai_exec.sh.backup.XXXXXX)"
ensure_observe_frontdoor_backup="$(mktemp ./scripts/ensure_observe_frontdoor.sh.backup.XXXXXX)"
launcher_log="${tmpdir}/amai_exec.log"
mv "${launcher_path}" "${backup}"
mv "${ensure_observe_frontdoor_path}" "${ensure_observe_frontdoor_backup}"
cat > "${launcher_path}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "\$*" >> "${launcher_log}"
exec "${backup}" "\$@"
EOF
chmod "${launcher_mode_before}" "${launcher_path}"
cat > "${ensure_observe_frontdoor_path}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exit 1
EOF
chmod "${ensure_observe_frontdoor_mode_before}" "${ensure_observe_frontdoor_path}"

startup_state_json="$(
  ./scripts/continuity_startup_state.sh --repo-root /home/art/agent-memory-index --json 2>/dev/null || true
)"
original_headline="$(printf '%s\n' "${startup_state_json}" | jq -r '.startup_runtime_state.execctl_active_lease.headline // empty' 2>/dev/null || true)"
original_next_step="$(printf '%s\n' "${startup_state_json}" | jq -r '.startup_runtime_state.execctl_active_lease.next_step // empty' 2>/dev/null || true)"
original_target_percent="$(printf '%s\n' "${startup_state_json}" | jq -r '.startup_runtime_state.client_budget_guard.client_budget_target_percent // empty' 2>/dev/null || true)"

cleanup() {
  if [[ -f "${backup}" ]]; then
    mv "${backup}" "${launcher_path}"
    chmod "${launcher_mode_before}" "${launcher_path}"
  fi
  if [[ -f "${ensure_observe_frontdoor_backup}" ]]; then
    mv "${ensure_observe_frontdoor_backup}" "${ensure_observe_frontdoor_path}"
    chmod "${ensure_observe_frontdoor_mode_before}" "${ensure_observe_frontdoor_path}"
  fi
  if [[ ! -x "${launcher_path}" ]] || [[ ! -s "${launcher_path}" ]]; then
    echo "proof_continuity_shell_release_fallback: failed to restore ${launcher_path}" >&2
    exit 1
  fi
  if [[ ! -x "${ensure_observe_frontdoor_path}" ]] || [[ ! -s "${ensure_observe_frontdoor_path}" ]]; then
    echo "proof_continuity_shell_release_fallback: failed to restore ${ensure_observe_frontdoor_path}" >&2
    exit 1
  fi
  if [[ -n "${original_target_percent}" ]]; then
    AMI_OBSERVE_BIND=127.0.0.1:1 \
      ./scripts/continuity_client_budget_target.sh \
        --project amai \
        --repo-root /home/art/agent-memory-index \
        --namespace continuity \
        --percent "${original_target_percent}" \
        --json >/dev/null 2>&1 || true
  fi
  if [[ -n "${original_headline}" ]] && [[ -n "${original_next_step}" ]]; then
    AMI_OBSERVE_BIND=127.0.0.1:1 \
      ./scripts/continuity_handoff.sh \
        --project amai \
        --namespace continuity \
        --headline "${original_headline}" \
        --next-step "${original_next_step}" \
        --resolve-current-goal >/dev/null 2>&1 || true
  elif [[ -f "${handoff_state_path}" ]] && [[ "$(cat "${handoff_state_path}")" == "present" ]]; then
    mkdir -p "$(dirname "${handoff_path}")"
    cp "${handoff_snapshot_path}" "${handoff_path}"
  else
    rm -f "${handoff_path}"
  fi
  if [[ -f "${compact_chat_prompt_state_path}" ]] && [[ "$(cat "${compact_chat_prompt_state_path}")" == "present" ]]; then
    mkdir -p "$(dirname "${compact_chat_prompt_path}")"
    cp "${compact_chat_prompt_snapshot_path}" "${compact_chat_prompt_path}"
  else
    rm -f "${compact_chat_prompt_path}"
  fi
  ./scripts/continuity_startup.sh --repo-root /home/art/agent-memory-index --namespace continuity --json >/dev/null 2>&1 || true
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

if [[ -f "${handoff_path}" ]]; then
  printf 'present' > "${handoff_state_path}"
  cp "${handoff_path}" "${handoff_snapshot_path}"
else
  printf 'absent' > "${handoff_state_path}"
fi

if [[ -f "${compact_chat_prompt_path}" ]]; then
  printf 'present' > "${compact_chat_prompt_state_path}"
  cp "${compact_chat_prompt_path}" "${compact_chat_prompt_snapshot_path}"
else
  printf 'absent' > "${compact_chat_prompt_state_path}"
fi

./scripts/continuity_startup.sh \
  --project amai \
  --repo-root /home/art/agent-memory-index \
  --namespace continuity \
  --json \
  | jq -e '.chat_start_restore != null and .continuity_startup.canonical_eval != null' >/dev/null

./scripts/continuity_startup_state.sh \
  --repo-root /home/art/agent-memory-index \
  --json \
  | jq -e '.startup_runtime_state.status == "ok"' >/dev/null

./scripts/continuity_restore.sh \
  --project amai \
  --repo-root /home/art/agent-memory-index \
  --namespace continuity \
  --json \
  | jq -e '.chat_start_restore != null and .working_state_restore != null and .continuity_restore.canonical_eval != null' >/dev/null

./scripts/continuity_answer.sh \
  --project amai \
  --repo-root /home/art/agent-memory-index \
  --namespace continuity \
  --json \
  --question "what is the current continuity handoff?" \
  | jq -e '.continuity_answer.answer_text != null and .continuity_answer.canonical_eval != null' >/dev/null

AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_compact_chat.sh \
    --project amai \
    --repo-root /home/art/agent-memory-index \
    --namespace continuity \
    --json \
    --headline "proof compact" \
    --next-step "prove continuity shell launcher routing" \
    | jq -e '.continuity_compact_chat != null' >/dev/null

AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_client_budget_target.sh \
    --project amai \
    --repo-root /home/art/agent-memory-index \
    --namespace continuity \
    --percent 90 \
    --json \
    | jq -e '.client_budget_target_update != null' >/dev/null

launcher_handoff_headline="proof launcher handoff"
launcher_handoff_next_step="prove continuity shell launcher routing"
AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_handoff.sh \
    --project amai \
    --namespace continuity \
    --headline "${launcher_handoff_headline}" \
    --next-step "${launcher_handoff_next_step}" \
    >/tmp/proof_continuity_handoff_release_only.out \
    2>/tmp/proof_continuity_handoff_release_only.err
jq -e \
  --arg headline "${launcher_handoff_headline}" \
  --arg next_step "${launcher_handoff_next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  /tmp/proof_continuity_handoff_release_only.out >/dev/null
grep -Fq -- "- headline: ${launcher_handoff_headline}" "${handoff_path}"
grep -Fq -- "- next_step: ${launcher_handoff_next_step}" "${handoff_path}"

missing_details_path="/tmp/amai-proof-missing-handoff-details.txt"
rm -f "${missing_details_path}"
if AMI_OBSERVE_BIND=127.0.0.1:1 ./scripts/continuity_handoff.sh \
  --project amai \
  --namespace continuity \
  --headline "proof missing details" \
  --next-step "prove continuity handoff missing-details branch" \
  --details-file "${missing_details_path}" \
  >/tmp/proof_continuity_handoff_missing_details.out \
  2>/tmp/proof_continuity_handoff_missing_details.err; then
  echo "proof_continuity_shell_release_fallback: continuity_handoff unexpectedly accepted missing details-file" >&2
  exit 1
fi
grep -Fq "failed to read ${missing_details_path}" /tmp/proof_continuity_handoff_missing_details.err

for expected in \
  "continuity startup" \
  "continuity startup-state" \
  "continuity restore" \
  "continuity answer" \
  "continuity compact-chat" \
  "continuity client-budget-target" \
  "continuity handoff"; do
  grep -Fq "${expected}" "${launcher_log}" || {
    echo "proof_continuity_shell_release_fallback: missing launcher invocation for ${expected}" >&2
    exit 1
  }
done

echo "proof_continuity_shell_release_fallback: PASS"
