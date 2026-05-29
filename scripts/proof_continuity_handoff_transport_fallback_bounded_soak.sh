#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
fakebin="${tmpdir}/bin"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"
latency_path="${tmpdir}/latency.tsv"
mkdir -p "${fakebin}"

iterations=12
max_single_ms=5000
max_total_ms=25000

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
  echo "proof_continuity_handoff_transport_fallback_bounded_soak: missing ./target/release/amai" >&2
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
    exit 28
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

total_ms=0
max_seen_ms=0

for i in $(seq 1 "${iterations}"); do
  headline="proof bounded soak handoff ${i}"
  next_step="verify bounded transport fallback soak ${i}"
  if [[ -f "${handoff_path}" ]]; then
    before_sha="$(sha256sum "${handoff_path}" | awk '{print $1}')"
  else
    before_sha="absent"
  fi
  started_ms="$(date +%s%3N)"
  payload="$(
    PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 \
      timeout 10s ./scripts/continuity_handoff.sh \
        --project amai \
        --namespace continuity \
        --headline "${headline}" \
        --next-step "${next_step}"
  )"
  ended_ms="$(date +%s%3N)"
  elapsed_ms="$((ended_ms - started_ms))"
  total_ms="$((total_ms + elapsed_ms))"
  if (( elapsed_ms > max_seen_ms )); then
    max_seen_ms="${elapsed_ms}"
  fi
  printf '%s\t%s\n' "${i}" "${elapsed_ms}" >>"${latency_path}"

  printf '%s\n' "${payload}" | jq -e \
    --arg headline "${headline}" \
    --arg next_step "${next_step}" \
    '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
    >/dev/null

  grep -Fq -- "- headline: ${headline}" "${handoff_path}"
  grep -Fq -- "- next_step: ${next_step}" "${handoff_path}"

  after_sha="$(sha256sum "${handoff_path}" | awk '{print $1}')"
  if [[ "${before_sha}" == "${after_sha}" ]]; then
    echo "proof_continuity_handoff_transport_fallback_bounded_soak: fallback iteration ${i} did not materialize a new canonical handoff" >&2
    exit 1
  fi

  headline_line_count="$(grep -c '^- headline:' "${handoff_path}")"
  next_step_line_count="$(grep -c '^- next_step:' "${handoff_path}")"
  if [[ "${headline_line_count}" -ne 1 ]] || [[ "${next_step_line_count}" -ne 1 ]]; then
    echo "proof_continuity_handoff_transport_fallback_bounded_soak: canonical handoff drifted during iteration ${i}" >&2
    cat "${handoff_path}" >&2
    exit 1
  fi

  if (( elapsed_ms > max_single_ms )); then
    echo "proof_continuity_handoff_transport_fallback_bounded_soak: iteration ${i} exceeded max_single_ms (${elapsed_ms} > ${max_single_ms})" >&2
    cat "${latency_path}" >&2
    exit 1
  fi
done

if (( total_ms > max_total_ms )); then
  echo "proof_continuity_handoff_transport_fallback_bounded_soak: total elapsed exceeded max_total_ms (${total_ms} > ${max_total_ms})" >&2
  cat "${latency_path}" >&2
  exit 1
fi

echo "proof_continuity_handoff_transport_fallback_bounded_soak: PASS (iterations=${iterations}, max_ms=${max_seen_ms}, total_ms=${total_ms})"
