#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
tmpdir="$(mktemp -d)"
fakebin="${tmpdir}/bin"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"
mkdir -p "${fakebin}"

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
  echo "proof_continuity_handoff_transport_failure_matrix: missing ./target/release/amai" >&2
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
    exit "${AMAI_FAKE_CURL_EXIT_CODE:?}"
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${fakebin}/curl"

codes=(7 28 56)
max_single_ms=5000

for code in "${codes[@]}"; do
  headline="proof transport matrix handoff ${code}"
  next_step="verify fallback for curl exit ${code}"
  started_ms="$(date +%s%3N)"
  payload="$(
    PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_CURL_EXIT_CODE="${code}" \
      timeout 10s ./scripts/continuity_handoff.sh \
        --project amai \
        --namespace continuity \
        --headline "${headline}" \
        --next-step "${next_step}"
  )"
  ended_ms="$(date +%s%3N)"
  elapsed_ms="$((ended_ms - started_ms))"

  printf '%s\n' "${payload}" | jq -e \
    --arg headline "${headline}" \
    --arg next_step "${next_step}" \
    '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
    >/dev/null

  grep -Fq -- "- headline: ${headline}" "${handoff_path}"
  grep -Fq -- "- next_step: ${next_step}" "${handoff_path}"

  if (( elapsed_ms > max_single_ms )); then
    echo "proof_continuity_handoff_transport_failure_matrix: curl exit ${code} exceeded max_single_ms (${elapsed_ms} > ${max_single_ms})" >&2
    exit 1
  fi
done

echo "proof_continuity_handoff_transport_failure_matrix: PASS"
