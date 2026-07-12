#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
export AMAI_PLATFORM_THREAD_ID="proof-continuity-handoff-transport-matrix"
export AMAI_AGENT_SCOPE="proof-continuity-handoff-transport-matrix"

tmpdir="$(mktemp -d)"
project_code="proof_handoff_transport_$(date +%s%N)"
repo_root="${tmpdir}/repo"
handoff_path="state/continuity-imports/${project_code}/live-handoff.md"
fakebin="${tmpdir}/bin"
mkdir -p "${fakebin}"

cleanup() {
  local path
  for path in scripts/ensure_observe_frontdoor.sh; do
    if [[ -e "${tmpdir}/$path" ]]; then
      mkdir -p "$(dirname "$path")"
      mv "${tmpdir}/$path" "$path"
    fi
  done
  rm -rf "state/continuity-imports/${project_code}"
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_continuity_handoff_transport_failure_matrix: missing ./target/release/amai" >&2
  exit 1
fi

mkdir -p "${repo_root}"
./target/release/amai project register \
  --code "${project_code}" \
  --display-name "Handoff transport proof" \
  --repo-root "${repo_root}" \
  --workspace default >/dev/null
./target/release/amai namespace ensure \
  --project "${project_code}" \
  --code continuity \
  --display-name Continuity >/dev/null

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
  headline="proof transport matrix handoff"
  next_step="verify fallback across curl transport failures"
  started_ms="$(./scripts/epoch_ms.sh)"
  payload="$(
    PATH="${fakebin}:/usr/bin:/bin" AMI_OBSERVE_BIND=127.0.0.1:1 AMAI_FAKE_CURL_EXIT_CODE="${code}" \
      timeout 10s ./scripts/continuity_handoff.sh \
        --project "${project_code}" \
        --namespace continuity \
        --headline "${headline}" \
        --next-step "${next_step}"
  )"
  ended_ms="$(./scripts/epoch_ms.sh)"
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
