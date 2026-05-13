#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

sample_seconds=8
json_mode=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --seconds)
      sample_seconds="${2:?missing value for --seconds}"
      shift 2
      ;;
    --json)
      json_mode=true
      shift
      ;;
    *)
      echo "usage: $0 [--seconds N] [--json]" >&2
      exit 2
      ;;
  esac
done

if ! [[ "$sample_seconds" =~ ^[0-9]+$ ]] || [[ "$sample_seconds" -lt 1 ]]; then
  echo "--seconds must be a positive integer" >&2
  exit 2
fi

canonical_bind="${AMI_OBSERVE_BIND:-0.0.0.0:9464}"
observe_pid="$(pgrep -fo "^./target/release/amai observe serve --bind ${canonical_bind}$" 2>/dev/null || true)"
clk_tck="$(getconf CLK_TCK)"
cpu_count="$(nproc)"

ps_top="$(
  ps -eo pid,ppid,pcpu,pmem,etime,cmd --sort=-pcpu | awk '
    NR == 1 { print; next }
    /\/usr\/share\/code\/code|node\.mojom\.NodeService|codex app-server|kilo serve|observe serve/ { print }
  ' | head -n 20
)"

code_status="$(code --status 2>&1 || true)"
status_rows="$(
  printf '%s\n' "$code_status" | awk '
    /^CPU %/ { table = 1; next }
    /^Workspace Stats:/ { table = 0 }
    table && NF {
      pid = $3
      $1 = ""; $2 = ""; $3 = ""
      sub(/^[[:space:]]+/, "", $0)
      if ($0 ~ / --status$/ || $0 ~ /vscode_live_cpu_truth\.sh/ || $0 ~ /electron-nodejs \(cli\.js\)/) {
        next
      }
      print pid "\t" $0
    }
  '
)"

declare -A labels=()
declare -a ordered_pids=()

record_pid() {
  local pid="$1"
  local label="$2"
  [[ -n "$pid" ]] || return 0
  [[ -r "/proc/$pid/stat" ]] || return 0
  if [[ -z "${labels[$pid]:-}" ]]; then
    ordered_pids+=("$pid")
  fi
  labels["$pid"]="$label"
}

while IFS= read -r row; do
  [[ -n "$row" ]] || continue
  if [[ "$row" =~ ^[[:space:]]*PID[[:space:]] ]]; then
    continue
  fi
  pid="$(awk '{print $1}' <<<"$row")"
  command_text="$(awk '{ $1=""; $2=""; $3=""; $4=""; $5=""; sub(/^[[:space:]]+/, ""); print }' <<<"$row")"
  record_pid "$pid" "$command_text"
done <<<"$ps_top"

while IFS=$'\t' read -r pid process_label; do
  [[ -n "$pid" ]] || continue
  record_pid "$pid" "code --status: ${process_label}"
done <<<"$status_rows"

if [[ -n "$observe_pid" ]]; then
  record_pid "$observe_pid" "observe serve (${canonical_bind})"
fi

declare -A start_ticks=()
declare -A end_ticks=()

read_ticks() {
  local pid="$1"
  awk '{print $14 + $15}' "/proc/$pid/stat" 2>/dev/null || true
}

for pid in "${ordered_pids[@]}"; do
  start_ticks["$pid"]="$(read_ticks "$pid")"
done

sleep "$sample_seconds"

live_rows=()
for pid in "${ordered_pids[@]}"; do
  if [[ ! -r "/proc/$pid/stat" ]]; then
    live_rows+=("pid=${pid}\tlive_cpu=missing\tlabel=${labels[$pid]}")
    continue
  fi
  end_ticks["$pid"]="$(read_ticks "$pid")"
  if [[ -z "${start_ticks[$pid]}" || -z "${end_ticks[$pid]}" ]]; then
    live_rows+=("pid=${pid}\tlive_cpu=missing\tlabel=${labels[$pid]}")
    continue
  fi
  delta_ticks="$(( ${end_ticks[$pid]%.*} - ${start_ticks[$pid]%.*} ))"
  live_cpu="$(awk -v d="$delta_ticks" -v secs="$sample_seconds" -v clk="$clk_tck" -v cpus="$cpu_count" 'BEGIN { printf "%.2f", (d / (secs * clk)) * 100 / cpus }')"
  live_rows+=("pid=${pid}\tlive_cpu=${live_cpu}\tlabel=${labels[$pid]}")
done

healthz_payload=""
healthz_status="unavailable"
if [[ -n "$observe_pid" ]]; then
  healthz_url="http://127.0.0.1:${canonical_bind##*:}/healthz"
  if healthz_payload="$(curl -fsS "$healthz_url" 2>/dev/null)"; then
    healthz_status="ok"
  fi
fi

if $json_mode; then
  {
    printf '{'
    printf '"sample_seconds":%s,' "$sample_seconds"
    printf '"cpu_count":%s,' "$cpu_count"
    printf '"canonical_observe_bind":"%s",' "$canonical_bind"
    printf '"observe_pid":%s,' "${observe_pid:-null}"
    printf '"ps_top":'
    printf '%s\n' "$ps_top" | jq -R -s .
    printf ','
    printf '"code_status":'
    printf '%s\n' "$code_status" | jq -R -s .
    printf ','
    printf '"live_sample":['
    first=1
    for row in "${live_rows[@]}"; do
      pid="$(sed -E 's/^pid=([0-9]+).*/\1/' <<<"$row")"
      live_cpu="$(sed -E 's/.*live_cpu=([^[:space:]]+).*/\1/' <<<"$row")"
      label="${row#*$'\t'label=}"
      if [[ $first -eq 0 ]]; then
        printf ','
      fi
      first=0
      jq -cn --arg pid "$pid" --arg live_cpu "$live_cpu" --arg label "$label" '{pid: ($pid|tonumber), live_cpu: $live_cpu, label: $label}'
    done
    printf '],'
    printf '"observe_healthz_status":"%s",' "$healthz_status"
    printf '"observe_healthz":'
    printf '%s\n' "${healthz_payload:-}" | jq -R -s .
    printf '}'
  } | jq .
  exit 0
fi

echo "vscode live cpu truth"
echo "sample_seconds: ${sample_seconds}"
echo "cpu_count: ${cpu_count}"
echo "canonical_observe_bind: ${canonical_bind}"
if [[ -n "$observe_pid" ]]; then
  echo "observe_pid: ${observe_pid}"
fi
echo
echo "top lifetime tail (ps):"
printf '%s\n' "$ps_top"
echo
echo "live sample (/proc windowed):"
for row in "${live_rows[@]}"; do
  printf '  %s\n' "$row"
done
echo
echo "code --status:"
printf '%s\n' "$code_status"
echo
echo "observe healthz (${healthz_status}):"
if [[ -n "$healthz_payload" ]]; then
  printf '%s\n' "$healthz_payload"
else
  echo "  unavailable"
fi
echo
echo "note: ps %CPU for long-lived Electron/zygote processes is lifetime-biased; treat the live sample and code --status as authoritative."
