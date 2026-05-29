#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"

cleanup() {
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

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_continuity_handoff_restart_idempotency: missing ./target/release/amai" >&2
  exit 1
fi

headline="proof restart idempotent handoff"
next_step="verify identical replay after restart-like boundary"

run_once() {
  AMI_OBSERVE_BIND=127.0.0.1:1 \
    timeout 10s ./scripts/continuity_handoff.sh \
      --project amai \
      --namespace continuity \
      --headline "${headline}" \
      --next-step "${next_step}"
}

first_payload="$(run_once)"
printf '%s\n' "${first_payload}" | jq -e \
  --arg headline "${headline}" \
  --arg next_step "${next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  >/dev/null

first_headline_count="$(grep -c '^- headline:' "${handoff_path}")"
first_next_step_count="$(grep -c '^- next_step:' "${handoff_path}")"
if [[ "${first_headline_count}" -ne 1 ]] || [[ "${first_next_step_count}" -ne 1 ]]; then
  echo "proof_continuity_handoff_restart_idempotency: canonical handoff invalid after first write" >&2
  cat "${handoff_path}" >&2
  exit 1
fi

first_headline_value="$(sed -n 's/^- headline: //p' "${handoff_path}")"
first_next_step_value="$(sed -n 's/^- next_step: //p' "${handoff_path}")"

second_payload="$(run_once)"
printf '%s\n' "${second_payload}" | jq -e \
  --arg headline "${headline}" \
  --arg next_step "${next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  >/dev/null

second_headline_count="$(grep -c '^- headline:' "${handoff_path}")"
second_next_step_count="$(grep -c '^- next_step:' "${handoff_path}")"
if [[ "${second_headline_count}" -ne 1 ]] || [[ "${second_next_step_count}" -ne 1 ]]; then
  echo "proof_continuity_handoff_restart_idempotency: canonical handoff invalid after replay write" >&2
  cat "${handoff_path}" >&2
  exit 1
fi

second_headline_value="$(sed -n 's/^- headline: //p' "${handoff_path}")"
second_next_step_value="$(sed -n 's/^- next_step: //p' "${handoff_path}")"

if [[ "${first_headline_value}" != "${second_headline_value}" ]] || [[ "${first_next_step_value}" != "${second_next_step_value}" ]]; then
  echo "proof_continuity_handoff_restart_idempotency: replay changed canonical handoff payload for identical input" >&2
  cat "${handoff_path}" >&2
  exit 1
fi

echo "proof_continuity_handoff_restart_idempotency: PASS"
