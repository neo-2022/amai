#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

mkdir -p tmp

step() {
  echo "[proof_observability] $*"
}

dump_monitoring_debug() {
  docker compose --profile monitoring ps || true
  docker logs ami-prometheus --tail 120 || true
  docker logs ami-grafana --tail 120 || true
}

pick_free_port() {
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

step "bootstrap stack"
./scripts/bootstrap_stack.sh
step "proof accuracy"
./scripts/proof_accuracy.sh
step "proof load"
./scripts/proof_load.sh
step "proof token benchmark"
./scripts/proof_token_benchmark.sh

export AMI_OBSERVE_BIND="0.0.0.0:$(pick_free_port)"
observe_port="${AMI_OBSERVE_BIND##*:}"
export AMI_PROMETHEUS_PORT="$(pick_free_port)"
export AMI_GRAFANA_PORT="$(pick_free_port)"
export AMI_PROMETHEUS_SCRAPE_TARGET="host.docker.internal:${observe_port}"

step "build release binary"
cargo build --release --quiet
step "prove observability guardrails"
cargo run --release --quiet -- observe guardrails
rm -f tmp/observe-exporter.log
step "start observe exporter on ${AMI_OBSERVE_BIND}"
./scripts/run_observe_exporter.sh > tmp/observe-exporter.log 2>&1 &
observe_pid=$!
cleanup() {
  kill "$observe_pid" >/dev/null 2>&1 || true
  docker compose --profile monitoring stop prometheus grafana >/dev/null 2>&1 || true
}
trap cleanup EXIT

exporter_ready=0
for _ in $(seq 1 90); do
  if ! kill -0 "$observe_pid" >/dev/null 2>&1; then
    cat tmp/observe-exporter.log
    exit 1
  fi
  healthz_payload="$(curl --silent --show-error --max-time 3 "http://127.0.0.1:${observe_port}/healthz" || true)"
  if [[ -n "${healthz_payload}" ]] && printf '%s' "${healthz_payload}" | jq -e '.status == "up"' >/dev/null; then
    exporter_ready=1
    break
  fi
  printf '%s' "${healthz_payload}" > tmp/observe-healthz-last.json || true
  sleep 1
done
if [[ "${exporter_ready}" -ne 1 ]]; then
  cat tmp/observe-exporter.log
  if [[ -s tmp/observe-healthz-last.json ]]; then
    echo "last healthz payload:"
    cat tmp/observe-healthz-last.json
  fi
  exit 1
fi

step "verify exported metrics payload"
curl -fsS "http://127.0.0.1:${observe_port}/metrics" | rg '^amai_(qdrant_index_optimize_queue|nats_consumer_lag_msgs|postgres_replica_lag_seconds|retrieval_hot_p95_ms|retrieval_cold_p95_ms|tokens_savings_factor|tokens_savings_percent) ' >/dev/null

step "verify human dashboard endpoints"
curl -fsS "http://127.0.0.1:${observe_port}/" | rg 'Amai Human Dashboard|Главная польза прямо сейчас|Польза проекта видна сразу' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/" | rg '/api/dashboard-live-summary|syncDashboardLiveSummary' >/dev/null
if curl -fsS "http://127.0.0.1:${observe_port}/" | rg '/api/active-agent-budget-live|syncActiveAgentBudgetLiveCard|fetchActiveAgentBudgetLivePayload' >/dev/null; then
  echo "human dashboard page still exposes split active-agent live poller" >&2
  exit 1
fi
curl -fsS "http://127.0.0.1:${observe_port}/api/dashboard" | rg '"top_cards"|"service_cards"|"glossary"' >/dev/null
dashboard_json="$(curl -fsS "http://127.0.0.1:${observe_port}/api/dashboard")"
printf '%s' "$dashboard_json" | jq -e '
  .service_cards
  | any(.title == "Жизненный цикл памяти")
' >/dev/null
printf '%s' "$dashboard_json" | jq -e '
  .service_cards[]
  | select(.title == "Жизненный цикл памяти")
  | (.rows | any(.label == "Pruning"))
    and (.rows | any(.label == "Archive"))
    and (.rows | any(.label == "Revalidation"))
    and (.rows | any(.label == "Dedup / compaction"))
' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/api/dashboard-live-summary" | rg '"headline"|"active_agent_card"|"top_cards"' >/dev/null
api_reply_prefix="$(curl -fsS "http://127.0.0.1:${observe_port}/api/client-limit-hourly-burn" | jq -r '.reply_prefix')"
script_reply_prefix="$(AMI_OBSERVE_BIND="${AMI_OBSERVE_BIND}" /home/art/.codex/skills/vscode-5h-kpi-prefix/scripts/read_kpi_prefix.sh)"
if [[ "$api_reply_prefix" != "$script_reply_prefix" ]]; then
  echo "5h KPI prefix script drifted from /api/client-limit-hourly-burn" >&2
  echo "api:    $api_reply_prefix" >&2
  echo "script: $script_reply_prefix" >&2
  exit 1
fi
curl -fsS "http://127.0.0.1:${observe_port}/api/snapshot" | rg '"sla"|"postgres"|"token_budget_report"' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/api/snapshot" | jq -e '
  .governance_surface.governance_surface_version == "governance-surface-v2"
  and (.governance_surface.forgetting_job_breakdown.pruning_job != null)
  and (.governance_surface.forgetting_job_breakdown.cold_archive_job != null)
  and (.governance_surface.forgetting_job_breakdown.revalidation_job != null)
  and (.governance_surface.forgetting_action_breakdown != null)
' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/" | rg '/brand/amai_mark.svg|/favicon.ico' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/brand/amai_mark.svg" | rg '<svg' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/brand/amai_lockup.svg" | rg '<svg' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/favicon.ico" >/dev/null

step "start monitoring profile"
./scripts/monitoring_up.sh
step "wait for prometheus"
prometheus_ready=0
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:${AMI_PROMETHEUS_PORT}/-/ready" >/dev/null; then
    prometheus_ready=1
    break
  fi
  sleep 1
done
if [[ "${prometheus_ready}" -ne 1 ]]; then
  dump_monitoring_debug
  exit 1
fi

step "wait for grafana"
grafana_ready=0
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:${AMI_GRAFANA_PORT}/api/health" | rg '"database"[[:space:]]*:[[:space:]]*"ok"' >/dev/null; then
    grafana_ready=1
    break
  fi
  sleep 1
done
if [[ "${grafana_ready}" -ne 1 ]]; then
  dump_monitoring_debug
  exit 1
fi

step "verify prometheus and grafana health"
curl -fsS "http://127.0.0.1:${AMI_PROMETHEUS_PORT}/api/v1/query?query=amai_qdrant_index_optimize_queue" | rg '"status":"success"' >/dev/null
curl -fsS "http://127.0.0.1:${AMI_GRAFANA_PORT}/api/health" | rg '"database"[[:space:]]*:[[:space:]]*"ok"' >/dev/null

step "build live snapshot"
cargo run --release --quiet -- observe snapshot
step "run live sla-check"
cargo run --release --quiet -- observe sla-check

cleanup
trap - EXIT

printf 'proof_observability: ok\n'
