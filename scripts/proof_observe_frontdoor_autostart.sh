#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ ! -x ./target/release/amai ]]; then
  echo "proof_observe_frontdoor_autostart: missing ./target/release/amai" >&2
  exit 1
fi

bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
host="${bind%:*}"
port="${bind##*:}"
case "$host" in
  ""|"0.0.0.0"|"::"|"[::]")
    host="127.0.0.1"
    ;;
  \[*\])
    host="${host#[}"
    host="${host%]}"
    ;;
esac

base_url="http://${host}:${port}/"
handoff_path="state/continuity-imports/amai/live-handoff.md"
headline="proof observe frontdoor autostart"
next_step="prove helper can materialize observe server before shell handoff API fast path"

./scripts/ensure_observe_frontdoor.sh --path /api/continuity-handoff
curl -fsS --max-time 3 "${base_url}" >/dev/null

payload="$(
  timeout 8s ./scripts/continuity_handoff.sh \
    --project amai \
    --namespace continuity \
    --headline "${headline}" \
    --next-step "${next_step}"
)"

printf '%s\n' "${payload}" | jq -e \
  --arg headline "${headline}" \
  --arg next_step "${next_step}" \
  '.continuity_handoff.headline == $headline and .continuity_handoff.next_step == $next_step' \
  >/dev/null

grep -Fq -- "- headline: ${headline}" "${handoff_path}"
grep -Fq -- "- next_step: ${next_step}" "${handoff_path}"

timeout 20s ./scripts/client_budget_root_cause.sh >/tmp/proof_observe_frontdoor_root_cause.out 2>/tmp/proof_observe_frontdoor_root_cause.err
timeout 20s ./scripts/client_budget_gate.sh --json >/tmp/proof_observe_frontdoor_gate.out 2>/tmp/proof_observe_frontdoor_gate.err

echo "proof_observe_frontdoor_autostart: PASS"
