#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_continuity_shell_release_fallback: missing ./target/release/amai" >&2
  exit 1
fi

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
handoff_snapshot_path="${tmpdir}/live-handoff.snapshot"
handoff_state_path="${tmpdir}/live-handoff.state"
launcher_path="./scripts/amai_exec.sh"
launcher_mode_before="$(stat -c '%a' "${launcher_path}")"
launcher_size_before="$(stat -c '%s' "${launcher_path}")"
if [[ "${launcher_size_before}" -le 0 ]]; then
  echo "proof_continuity_shell_release_fallback: corrupted ${launcher_path} before proof" >&2
  exit 1
fi
backup="$(mktemp /tmp/amai_exec.sh.XXXXXX)"
mv "${launcher_path}" "${backup}"
cleanup() {
  if [[ -f "${backup}" ]]; then
    mv "${backup}" "${launcher_path}"
    chmod "${launcher_mode_before}" "${launcher_path}"
  fi
  if [[ ! -x "${launcher_path}" ]] || [[ ! -s "${launcher_path}" ]]; then
    echo "proof_continuity_shell_release_fallback: failed to restore ${launcher_path}" >&2
    exit 1
  fi
  if [[ -f "${handoff_state_path}" ]] && [[ "$(cat "${handoff_state_path}")" == "present" ]]; then
    mkdir -p "$(dirname "${handoff_path}")"
    cp "${handoff_snapshot_path}" "${handoff_path}"
  else
    rm -f "${handoff_path}"
  fi
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

if [[ -f "${handoff_path}" ]]; then
  printf 'present' > "${handoff_state_path}"
  cp "${handoff_path}" "${handoff_snapshot_path}"
else
  printf 'absent' > "${handoff_state_path}"
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

./scripts/continuity_compact_chat.sh \
  --project amai \
  --repo-root /home/art/agent-memory-index \
  --namespace continuity \
  --json \
  --headline "proof compact" \
  --next-step "prove release-only compact shell wrapper" \
  | jq -e '.continuity_compact_chat != null' >/dev/null

./scripts/continuity_client_budget_target.sh \
  --project amai \
  --repo-root /home/art/agent-memory-index \
  --namespace continuity \
  --percent 90 \
  --json \
  | jq -e '.client_budget_target_update != null' >/dev/null

release_only_handoff_headline="proof release-only handoff"
release_only_handoff_next_step="prove release-only handoff shell wrapper"
AMI_OBSERVE_BIND=127.0.0.1:1 \
  ./scripts/continuity_handoff.sh \
    --project amai \
    --namespace continuity \
    --headline "${release_only_handoff_headline}" \
    --next-step "${release_only_handoff_next_step}" \
    >/tmp/proof_continuity_handoff_release_only.out \
    2>/tmp/proof_continuity_handoff_release_only.err
jq -e \
  --arg headline "${release_only_handoff_headline}" \
  --arg next_step "${release_only_handoff_next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  /tmp/proof_continuity_handoff_release_only.out >/dev/null
grep -Fq -- "- headline: ${release_only_handoff_headline}" "${handoff_path}"
grep -Fq -- "- next_step: ${release_only_handoff_next_step}" "${handoff_path}"

missing_details_path="/tmp/amai-proof-missing-handoff-details.txt"
rm -f "${missing_details_path}"
if ./scripts/continuity_handoff.sh \
  --project amai \
  --namespace continuity \
  --headline "proof missing details" \
  --next-step "prove release-only handoff missing-details branch" \
  --details-file "${missing_details_path}" \
  >/tmp/proof_continuity_handoff_missing_details.out \
  2>/tmp/proof_continuity_handoff_missing_details.err; then
  echo "proof_continuity_shell_release_fallback: continuity_handoff unexpectedly accepted missing details-file" >&2
  exit 1
fi
grep -Fq "failed to read ${missing_details_path}" /tmp/proof_continuity_handoff_missing_details.err

launcher_mode_after="$(stat -c '%a' "${backup}")"
if [[ "${launcher_mode_after}" != "${launcher_mode_before}" ]]; then
  echo "proof_continuity_shell_release_fallback: launcher mode drifted in backup path" >&2
  exit 1
fi

echo "proof_continuity_shell_release_fallback: PASS"
