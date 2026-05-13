#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
fakebin="${tmpdir}/bin"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"
proof_tmp="${tmpdir}/runs"
mkdir -p "${fakebin}" "${proof_tmp}"

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

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_continuity_handoff_transport_failure_burst: missing ./target/release/amai" >&2
  exit 1
fi

mkdir -p "${tmpdir}/scripts"
if [[ -e scripts/ensure_observe_frontdoor.sh ]]; then
  mv scripts/ensure_observe_frontdoor.sh "${tmpdir}/scripts/ensure_observe_frontdoor.sh"
fi
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
  *"/api/continuity-handoff")
    exit 56
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

workers=8
max_total_ms=12000
declare -a pids=()
declare -a headlines=()
declare -a next_steps=()

start_epoch_ms="$(date +%s%3N)"

for i in $(seq 1 "${workers}"); do
  headline="proof transport burst handoff ${i}"
  next_step="verify burst fallback writer ${i}"
  headlines+=("${headline}")
  next_steps+=("${next_step}")
  (
    PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
      timeout 15s ./scripts/continuity_handoff.sh \
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

end_epoch_ms="$(date +%s%3N)"
elapsed_ms="$((end_epoch_ms - start_epoch_ms))"

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
  echo "proof_continuity_handoff_transport_failure_burst: canonical handoff drifted under burst fallback load" >&2
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
  echo "proof_continuity_handoff_transport_failure_burst: final canonical handoff does not match any completed burst writer" >&2
  cat "${handoff_path}" >&2
  exit 1
fi

if (( elapsed_ms > max_total_ms )); then
  echo "proof_continuity_handoff_transport_failure_burst: burst fallback exceeded max_total_ms (${elapsed_ms} > ${max_total_ms})" >&2
  exit 1
fi

echo "proof_continuity_handoff_transport_failure_burst: PASS (workers=${workers}, total_ms=${elapsed_ms})"
