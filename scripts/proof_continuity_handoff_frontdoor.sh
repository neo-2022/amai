#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

handoff_path="state/continuity-imports/amai/live-handoff.md"
ensure_observe_frontdoor_path="./scripts/ensure_observe_frontdoor.sh"
tmpdir="$(mktemp -d)"
snapshot_path="${tmpdir}/live-handoff.snapshot"
state_path="${tmpdir}/live-handoff.state"
ensure_observe_frontdoor_mode_before="$(stat -c '%a' "${ensure_observe_frontdoor_path}")"
ensure_observe_frontdoor_backup="$(mktemp ./scripts/ensure_observe_frontdoor.sh.backup.XXXXXX)"
startup_state_json="$(
  ./scripts/continuity_startup_state.sh --repo-root /home/art/agent-memory-index --json 2>/dev/null || true
)"
original_headline="$(printf '%s\n' "${startup_state_json}" | jq -r '.startup_runtime_state.execctl_active_lease.headline // empty' 2>/dev/null || true)"
original_next_step="$(printf '%s\n' "${startup_state_json}" | jq -r '.startup_runtime_state.execctl_active_lease.next_step // empty' 2>/dev/null || true)"
mv "${ensure_observe_frontdoor_path}" "${ensure_observe_frontdoor_backup}"
cat > "${ensure_observe_frontdoor_path}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exit 1
EOF
chmod "${ensure_observe_frontdoor_mode_before}" "${ensure_observe_frontdoor_path}"

cleanup() {
  if [[ -f "${ensure_observe_frontdoor_backup}" ]]; then
    mv "${ensure_observe_frontdoor_backup}" "${ensure_observe_frontdoor_path}"
    chmod "${ensure_observe_frontdoor_mode_before}" "${ensure_observe_frontdoor_path}"
  fi
  if [[ -n "${original_headline}" ]] && [[ -n "${original_next_step}" ]]; then
    AMI_OBSERVE_BIND=127.0.0.1:1 \
      ./scripts/continuity_handoff.sh \
        --project amai \
        --namespace continuity \
        --headline "${original_headline}" \
        --next-step "${original_next_step}" \
        --resolve-current-goal >/dev/null 2>&1 || true
  elif [[ -f "${state_path}" ]] && [[ "$(cat "${state_path}")" == "present" ]]; then
    mkdir -p "$(dirname "${handoff_path}")"
    cp "${snapshot_path}" "${handoff_path}"
  else
    rm -f "${handoff_path}"
  fi
  if [[ ! -x "${ensure_observe_frontdoor_path}" ]] || [[ ! -s "${ensure_observe_frontdoor_path}" ]]; then
    echo "proof_continuity_handoff_frontdoor: failed to restore ${ensure_observe_frontdoor_path}" >&2
    exit 1
  fi
  ./scripts/continuity_startup.sh --repo-root /home/art/agent-memory-index --namespace continuity --json >/dev/null 2>&1 || true
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

if [[ -f "${handoff_path}" ]]; then
  printf 'present' > "${state_path}"
  cp "${handoff_path}" "${snapshot_path}"
else
  printf 'absent' > "${state_path}"
fi

if [[ ! -x ./target/release/amai && ! -x ./target/debug/amai ]]; then
  echo "proof_continuity_handoff_frontdoor: timing assertion requires a prebuilt local amai binary for launcher fallback" >&2
  exit 1
fi

headline="proof handoff frontdoor"
next_step="prove shell fallback uses canonical launcher"

start_epoch_ms="$(./scripts/epoch_ms.sh)"
api_payload="$(
  AMI_OBSERVE_BIND=127.0.0.1:1 \
    timeout 8s ./scripts/continuity_handoff.sh \
      --project amai \
      --namespace continuity \
      --headline "${headline}" \
      --next-step "${next_step}"
)"
end_epoch_ms="$(./scripts/epoch_ms.sh)"
elapsed_ms="$((end_epoch_ms - start_epoch_ms))"

printf '%s\n' "${api_payload}" | jq -e \
  --arg headline "${headline}" \
  --arg next_step "${next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  >/dev/null

if (( elapsed_ms >= 8000 )); then
  echo "proof_continuity_handoff_frontdoor: shell front-door hit timeout-budget instead of fast launcher fallback (${elapsed_ms} ms)" >&2
  exit 1
fi

grep -Fq -- "- headline: ${headline}" "${handoff_path}"
grep -Fq -- "- next_step: ${next_step}" "${handoff_path}"

echo "proof_continuity_handoff_frontdoor: PASS launcher fallback (${elapsed_ms} ms)"
