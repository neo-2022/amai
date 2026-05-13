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
  echo "proof_continuity_handoff_frontdoor: missing ./target/release/amai" >&2
  exit 1
fi

headline="proof handoff frontdoor"
next_step="prove shell fallback uses release binary"

start_epoch_ms="$(date +%s%3N)"
api_payload="$(
  AMI_OBSERVE_BIND=127.0.0.1:1 \
    timeout 8s ./scripts/continuity_handoff.sh \
      --project amai \
      --namespace continuity \
      --headline "${headline}" \
      --next-step "${next_step}"
)"
end_epoch_ms="$(date +%s%3N)"
elapsed_ms="$((end_epoch_ms - start_epoch_ms))"

printf '%s\n' "${api_payload}" | jq -e \
  --arg headline "${headline}" \
  --arg next_step "${next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  >/dev/null

if (( elapsed_ms >= 8000 )); then
  echo "proof_continuity_handoff_frontdoor: shell front-door hit timeout-budget instead of fast fallback (${elapsed_ms} ms)" >&2
  exit 1
fi

grep -Fq -- "- headline: ${headline}" "${handoff_path}"
grep -Fq -- "- next_step: ${next_step}" "${handoff_path}"

echo "proof_continuity_handoff_frontdoor: PASS (${elapsed_ms} ms)"
