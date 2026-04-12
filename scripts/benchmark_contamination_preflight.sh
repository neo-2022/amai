#!/usr/bin/env bash
set -euo pipefail

canonical_bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
json_mode=false

if [[ "${1:-}" == "--json" ]]; then
  json_mode=true
fi

json_array() {
  if [[ "$#" -eq 0 ]]; then
    printf '[]'
  else
    printf '%s\n' "$@" | jq -R . | jq -s .
  fi
}

mapfile -t observe_instances < <(
  pgrep -af '(^|/)amai observe serve|target/(debug|release)/amai observe serve|cargo run --.* observe serve' || true
)
mapfile -t benchmark_instances < <(
  pgrep -af 'target/(debug|release)/amai verify (load|benchmark|accuracy|memory-matrix)|cargo run --.* verify (load|benchmark|accuracy|memory-matrix)' || true
)
mapfile -t heavy_processes < <(
  ps -eo pid,pcpu,comm,args --sort=-pcpu | awk '
    NR > 1 && $2 + 0 >= 50 && $3 !~ /^(amai|postgres|docker|ps|awk)$/ {
      print $0
    }
  ' | head -n 8
)

canonical_release_count=0
declare -a blocking_observe_instances=()
for line in "${observe_instances[@]}"; do
  [[ -z "$line" ]] && continue
  if [[ "$line" == *"target/release/amai observe serve"* && "$line" == *"--bind ${canonical_bind}"* ]]; then
    canonical_release_count=$((canonical_release_count + 1))
    continue
  fi
  blocking_observe_instances+=("$line")
done

declare -a blocking_benchmark_instances=()
for line in "${benchmark_instances[@]}"; do
  [[ -z "$line" ]] && continue
  blocking_benchmark_instances+=("$line")
done

status="pass"
declare -a reasons=()
if (( canonical_release_count > 1 )); then
  status="fail"
  reasons+=("multiple canonical observe servers are running")
fi
if (( ${#blocking_observe_instances[@]} > 0 )); then
  status="fail"
  reasons+=("non-canonical observe server instance detected")
fi
if (( ${#blocking_benchmark_instances[@]} > 0 )); then
  status="fail"
  reasons+=("parallel benchmark or verify lane detected")
fi

reasons_json=$(json_array "${reasons[@]}")
observe_json=$(json_array "${observe_instances[@]}")
blocking_observe_json=$(json_array "${blocking_observe_instances[@]}")
blocking_benchmark_json=$(json_array "${blocking_benchmark_instances[@]}")
heavy_json=$(json_array "${heavy_processes[@]}")

if $json_mode; then
  jq -n \
    --arg status "$status" \
    --arg canonical_bind "$canonical_bind" \
    --argjson canonical_release_count "$canonical_release_count" \
    --argjson observe_instances "$observe_json" \
    --argjson blocking_observe_instances "$blocking_observe_json" \
    --argjson blocking_benchmark_instances "$blocking_benchmark_json" \
    --argjson heavy_external_processes "$heavy_json" \
    --argjson reasons "$reasons_json" \
    '{
      status: $status,
      canonical_bind: $canonical_bind,
      canonical_release_count: $canonical_release_count,
      observe_instances: $observe_instances,
      blocking_observe_instances: $blocking_observe_instances,
      blocking_benchmark_instances: $blocking_benchmark_instances,
      heavy_external_processes: $heavy_external_processes,
      reasons: $reasons
    }'
else
  echo "benchmark contamination preflight"
  echo "canonical observe bind: ${canonical_bind}"
  echo "canonical release observe count: ${canonical_release_count}"
  if (( ${#blocking_observe_instances[@]} > 0 )); then
    echo "blocking observe instances:"
    printf '  %s\n' "${blocking_observe_instances[@]}"
  fi
  if (( ${#blocking_benchmark_instances[@]} > 0 )); then
    echo "blocking benchmark instances:"
    printf '  %s\n' "${blocking_benchmark_instances[@]}"
  fi
  if (( ${#heavy_processes[@]} > 0 )); then
    echo "heavy external processes (advisory):"
    printf '  %s\n' "${heavy_processes[@]}"
  fi
fi

if [[ "$status" != "pass" ]]; then
  exit 1
fi
