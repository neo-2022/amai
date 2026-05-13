#!/usr/bin/env bash
set -euo pipefail

canonical_bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
heavy_cpu_threshold="${AMI_BENCHMARK_HEAVY_CPU_THRESHOLD:-50}"
json_mode=false
strict_heavy=false

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --json)
      json_mode=true
      ;;
    --strict-heavy)
      strict_heavy=true
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

if [[ "${AMI_BENCHMARK_STRICT_HEAVY:-}" == "1" || "${AMI_BENCHMARK_STRICT_HEAVY:-}" == "true" ]]; then
  strict_heavy=true
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
  ps -eo pid,pcpu,comm,args --sort=-pcpu | awk -v threshold="$heavy_cpu_threshold" '
    NR > 1 && $2 + 0 >= threshold + 0 && $3 !~ /^(amai|postgres|docker|ps|awk)$/ {
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
declare -a strict_heavy_processes=()
if $strict_heavy; then
  for line in "${heavy_processes[@]}"; do
    [[ -z "$line" ]] && continue
    strict_heavy_processes+=("$line")
  done
  if (( ${#strict_heavy_processes[@]} > 0 )); then
    status="fail"
    reasons+=("heavy external process detected in strict benchmark mode")
  fi
fi

reasons_json=$(json_array "${reasons[@]}")
observe_json=$(json_array "${observe_instances[@]}")
blocking_observe_json=$(json_array "${blocking_observe_instances[@]}")
blocking_benchmark_json=$(json_array "${blocking_benchmark_instances[@]}")
heavy_json=$(json_array "${heavy_processes[@]}")
strict_heavy_json=$(json_array "${strict_heavy_processes[@]}")

if $json_mode; then
  jq -n \
    --arg status "$status" \
    --arg canonical_bind "$canonical_bind" \
    --arg heavy_cpu_threshold "$heavy_cpu_threshold" \
    --argjson strict_heavy "$strict_heavy" \
    --argjson canonical_release_count "$canonical_release_count" \
    --argjson observe_instances "$observe_json" \
    --argjson blocking_observe_instances "$blocking_observe_json" \
    --argjson blocking_benchmark_instances "$blocking_benchmark_json" \
    --argjson heavy_external_processes "$heavy_json" \
    --argjson strict_heavy_processes "$strict_heavy_json" \
    --argjson reasons "$reasons_json" \
    '{
      status: $status,
      canonical_bind: $canonical_bind,
      heavy_cpu_threshold: ($heavy_cpu_threshold | tonumber),
      strict_heavy: $strict_heavy,
      canonical_release_count: $canonical_release_count,
      observe_instances: $observe_instances,
      blocking_observe_instances: $blocking_observe_instances,
      blocking_benchmark_instances: $blocking_benchmark_instances,
      heavy_external_processes: $heavy_external_processes,
      strict_heavy_processes: $strict_heavy_processes,
      reasons: $reasons
    }'
else
  echo "benchmark contamination preflight"
  echo "canonical observe bind: ${canonical_bind}"
  echo "heavy CPU threshold: ${heavy_cpu_threshold}%"
  echo "strict heavy process mode: ${strict_heavy}"
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
    if $strict_heavy; then
      echo "heavy external processes (blocking):"
    else
      echo "heavy external processes (advisory):"
    fi
    printf '  %s\n' "${heavy_processes[@]}"
  fi
fi

if [[ "$status" != "pass" ]]; then
  exit 1
fi
