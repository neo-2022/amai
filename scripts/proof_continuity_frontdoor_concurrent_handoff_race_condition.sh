#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"
proof_tmp="${tmpdir}/runs"
mkdir -p "${proof_tmp}"

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
  echo "proof_continuity_frontdoor_concurrent_handoff_race_condition: missing ./target/release/amai" >&2
  exit 1
fi

declare -a pids=()
declare -a headlines=()
declare -a next_steps=()
workers=5

for i in $(seq 1 "${workers}"); do
  headline="proof concurrent handoff ${i}"
  next_step="verify concurrent handoff writer ${i}"
  headlines+=("${headline}")
  next_steps+=("${next_step}")
  (
    AMI_OBSERVE_BIND=127.0.0.1:1 \
      timeout 20s ./scripts/continuity_handoff.sh \
        --project amai \
        --namespace continuity \
        --headline "${headline}" \
        --next-step "${next_step}" \
      >"${proof_tmp}/handoff-${i}.out" \
      2>"${proof_tmp}/handoff-${i}.err"
  ) &
  pids+=($!)
done

for pid in "${pids[@]}"; do
  wait "${pid}"
done

for i in $(seq 1 "${workers}"); do
  jq -e \
    --arg headline "${headlines[$((i-1))]}" \
    --arg next_step "${next_steps[$((i-1))]}" \
    '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
    "${proof_tmp}/handoff-${i}.out" >/dev/null
done

headline_line_count="$(grep -c '^- headline:' "${handoff_path}")"
next_step_line_count="$(grep -c '^- next_step:' "${handoff_path}")"
if [[ "${headline_line_count}" -ne 1 ]] || [[ "${next_step_line_count}" -ne 1 ]]; then
  echo "proof_continuity_frontdoor_concurrent_handoff_race_condition: canonical live handoff has duplicated or missing headline/next_step lines" >&2
  cat "${handoff_path}" >&2
  exit 1
fi

final_headline="$(sed -n 's/^- headline: //p' "${handoff_path}")"
final_next_step="$(sed -n 's/^- next_step: //p' "${handoff_path}")"

match_found=false
for i in $(seq 1 "${workers}"); do
  expected_headline="${headlines[$((i-1))]}"
  expected_next_step="${next_steps[$((i-1))]}"
  if [[ "${final_headline}" == "${expected_headline}" ]] && [[ "${final_next_step}" == "${expected_next_step}" ]]; then
    match_found=true
    break
  fi
done

if [[ "${match_found}" != "true" ]]; then
  echo "proof_continuity_frontdoor_concurrent_handoff_race_condition: final canonical handoff does not match any completed writer" >&2
  cat "${handoff_path}" >&2
  exit 1
fi

echo "proof_continuity_frontdoor_concurrent_handoff_race_condition: PASS"
