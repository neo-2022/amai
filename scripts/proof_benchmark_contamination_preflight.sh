#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

clean_output="$(./scripts/benchmark_contamination_preflight.sh --json)"
printf '%s\n' "$clean_output" | jq -e '.status == "pass"' >/dev/null

fake_pid=""
cleanup() {
  if [[ -n "$fake_pid" ]]; then
    kill "$fake_pid" >/dev/null 2>&1 || true
    wait "$fake_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

bash -lc 'exec -a "target/debug/amai observe serve --bind 127.0.0.1:9465" sleep 30' &
fake_pid="$!"
sleep 1

if ./scripts/benchmark_contamination_preflight.sh --json >/tmp/benchmark_contamination_preflight_fail.json 2>/dev/null; then
  echo "proof_benchmark_contamination_preflight: expected contamination preflight to fail" >&2
  exit 1
fi

jq -e '.status == "fail"' /tmp/benchmark_contamination_preflight_fail.json >/dev/null
jq -e '.blocking_observe_instances | length >= 1' /tmp/benchmark_contamination_preflight_fail.json >/dev/null

cleanup
trap - EXIT

rm -f /tmp/benchmark_contamination_preflight_fail.json

printf 'proof_benchmark_contamination_preflight: ok\n'
