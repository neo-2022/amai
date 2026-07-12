#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
export AMAI_PLATFORM_THREAD_ID="proof-continuity-frontdoor-concurrent-handoff"
export AMAI_AGENT_SCOPE="proof-continuity-frontdoor-concurrent-handoff"

tmpdir="$(mktemp -d)"
project_code="proof_frontdoor_race_$(date +%s%N)"
repo_root="${tmpdir}/repo"
handoff_path="state/continuity-imports/${project_code}/live-handoff.md"
proof_tmp="${tmpdir}/runs"
mkdir -p "${proof_tmp}"

cleanup() {
  rm -rf "state/continuity-imports/${project_code}"
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_continuity_frontdoor_concurrent_handoff_race_condition: missing ./target/release/amai" >&2
  exit 1
fi

mkdir -p "${repo_root}"
./target/release/amai project register \
  --code "${project_code}" \
  --display-name "Frontdoor race proof" \
  --repo-root "${repo_root}" \
  --workspace default >/dev/null
./target/release/amai namespace ensure \
  --project "${project_code}" \
  --code continuity \
  --display-name Continuity >/dev/null

declare -a pids=()
declare -a headlines=()
declare -a next_steps=()
workers=2

for i in $(seq 1 "${workers}"); do
  headline="proof concurrent handoff"
  next_step="verify concurrent handoff writers"
  headlines+=("${headline}")
  next_steps+=("${next_step}")
  (
    timeout 60s ./target/release/amai continuity handoff \
        --project "${project_code}" \
        --namespace continuity \
        --headline "${headline}" \
        --next-step "${next_step}" \
      >"${proof_tmp}/handoff-${i}.out" \
      2>"${proof_tmp}/handoff-${i}.err"
  ) &
  pids+=($!)
done

writer_failures=0
for pid in "${pids[@]}"; do
  if ! wait "${pid}"; then
    writer_failures=$((writer_failures + 1))
  fi
done
if (( writer_failures > 0 )); then
  for err in "${proof_tmp}"/*.err; do
    cat "${err}" >&2
  done
  exit 1
fi

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
