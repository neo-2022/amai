#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "${SCRIPT_DIR}/.."
source ./scripts/load_env.sh

mkdir -p tmp

step() {
  echo "[proof_observability] $*"
}

observe_refresh_wait_seconds() {
  python3 - "$PWD/src/observe/observe_models.rs" <<'PY'
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
timeout_match = re.search(r"OBSERVE_REFRESH_TIMEOUT_MS: u64 = ([\d_]+);", source)
grace_match = re.search(r"OBSERVE_REFRESH_STUCK_GRACE_MS: u64 = ([\d_]+);", source)
if not timeout_match or not grace_match:
    print(135)
    raise SystemExit(0)

timeout_ms = int(timeout_match.group(1).replace("_", ""))
grace_ms = int(grace_match.group(1).replace("_", ""))
wait_seconds = ((timeout_ms + grace_ms + 9_999) // 1000) + 10
print(max(wait_seconds, 135))
PY
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

clear_workspace_restore_pack_rows() {
  psql "$AMI_POSTGRES_DSN" -v ON_ERROR_STOP=1 >/dev/null <<'SQL'
DELETE FROM ami.restore_packs rp
USING ami.projects p, ami.namespaces n
WHERE rp.project_id = p.project_id
  AND rp.namespace_id = n.namespace_id
  AND p.code = 'amai'
  AND n.code = 'continuity'
  AND rp.pack_kind = 'workspace_restore_pack';
SQL
}

clear_workspace_restore_pack_rows

postgres_deadlocks_total() {
  psql "$AMI_POSTGRES_DSN" -Atqc "
    SELECT COALESCE(deadlocks, 0)::bigint
    FROM pg_stat_database
    WHERE datname = current_database()
  "
}

export AMI_OBSERVE_BIND="0.0.0.0:$(pick_free_port)"
observe_port="${AMI_OBSERVE_BIND##*:}"
export AMI_PROMETHEUS_PORT="$(pick_free_port)"
export AMI_GRAFANA_PORT="$(pick_free_port)"
export AMI_PROMETHEUS_SCRAPE_TARGET="host.docker.internal:${observe_port}"

step "build release binary"
cargo build --release --quiet
step "prove observability guardrails"
cargo run --release --quiet -- observe guardrails
step "persist fresh system snapshot baseline for exporter deadlock delta"
rm -f tmp/proof_observability_baseline_snapshot.json
cargo run --release --quiet -- observe snapshot > tmp/proof_observability_baseline_snapshot.json
baseline_deadlocks_total="$(jq -r '.postgres.deadlocks_total' tmp/proof_observability_baseline_snapshot.json)"
if [[ -z "${baseline_deadlocks_total}" || "${baseline_deadlocks_total}" == "null" ]]; then
  echo "proof_observability: baseline snapshot did not expose postgres.deadlocks_total" >&2
  exit 1
fi
proof_window_deadlocks_before="$(postgres_deadlocks_total)"
if [[ "${proof_window_deadlocks_before}" != "${baseline_deadlocks_total}" ]]; then
  echo "proof_observability: baseline snapshot deadlocks_total drifted from live postgres counter" >&2
  echo "baseline snapshot deadlocks_total=${baseline_deadlocks_total}" >&2
  echo "live postgres deadlocks_total=${proof_window_deadlocks_before}" >&2
  exit 1
fi
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
exporter_ready_attempts="$(observe_refresh_wait_seconds)"
for _ in $(seq 1 "${exporter_ready_attempts}"); do
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
printf '%s' "$dashboard_json" > tmp/proof_observability_dashboard.json
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
printf '%s' "$dashboard_json" | jq -e '
  .service_cards
  | any(
      .title == "Capacity forecast"
      and (.note | type == "string")
      and (.note | contains("Forecast-only"))
      and (.title_tooltip | type == "string")
      and (.title_tooltip | contains("Не authority"))
      and (.rows | any(.label == "History scope"))
    )
' >/dev/null
printf '%s' "$dashboard_json" | jq -e '
  .service_cards
  | any(
      .title == "Regression explain"
      and (.note | type == "string")
      and (.note | contains("insufficient sample"))
      and (.title_tooltip | type == "string")
      and (.title_tooltip | contains("Queue 4"))
      and (.rows | any(.label == "Benchmark pass"))
      and (.rows | any(.label == "Stale error"))
      and (.rows | any(.label == "Retrieval helpful"))
    )
' >/dev/null
printf '%s' "$dashboard_json" | jq -e '
  .benchmark_cards
  | any(.title == "Memory task matrix compare")
' >/dev/null
printf '%s' "$dashboard_json" | jq -e '
  .benchmark_cards
  | any(.title == "MCP task matrix compare")
' >/dev/null
printf '%s' "$dashboard_json" | jq -e '
  .benchmark_cards
  | any(
      .title == "Memory task matrix compare"
      and (.headline_value | type == "string")
      and (.headline_value | contains("drift measured"))
      and (.headline_value | contains("promotion candidate_ready_for_measured_approval"))
      and (.headline_value | contains("approval pending_human_review"))
      and (.table.rows | any(
        .label == "Score drift"
        and (.values[1] | type == "string")
        and (.values[1] | startswith("measured"))
      ))
      and (.table.rows | any(
        .label == "Promotion"
        and (.values[1] | type == "string")
        and (.values[1] | contains("candidate_ready_for_measured_approval"))
      ))
      and (.table.rows | any(
        .label == "Measured approval"
        and (.values[1] | type == "string")
        and (.values[1] | contains("pending_human_review"))
      ))
    )
' >/dev/null
printf '%s' "$dashboard_json" | jq -e '
  .benchmark_cards
  | any(
      .title == "MCP task matrix compare"
      and (.headline_value | type == "string")
      and (.headline_value | contains("drift measured"))
      and (.headline_value | contains("promotion candidate_ready_for_measured_approval"))
      and (.headline_value | contains("approval pending_human_review"))
      and (.table.rows | any(
        .label == "Score drift"
        and (.values[1] | type == "string")
        and (.values[1] | startswith("not_applicable"))
      ))
      and (.table.rows | any(
        .label == "Promotion"
        and (.values[1] | type == "string")
        and (.values[1] | contains("candidate_ready_for_measured_approval"))
      ))
      and (.table.rows | any(
        .label == "Measured approval"
        and (.values[1] | type == "string")
        and (.values[1] | contains("pending_human_review"))
      ))
    )
' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/api/dashboard-live-summary" | rg '"headline"|"active_agent_card"|"top_cards"' >/dev/null
api_reply_prefix="$(curl -fsS "http://127.0.0.1:${observe_port}/api/client-limit-hourly-burn" | jq -r '.reply_prefix')"
script_reply_prefix="$("${SCRIPT_DIR}/client_budget_system_markers.sh" | jq -r '.economic_markers.repo_owned_burn_guard_prefix')"
python3 - "$api_reply_prefix" "$script_reply_prefix" <<'PY'
import re
import sys

api = sys.argv[1]
script = sys.argv[2]

pattern = re.compile(r"^Burn guard: (экономия|переплата) ([0-9]+(?:\.[0-9]+)?)%$")

def parse(prefix: str):
    prefix = prefix.strip()
    if prefix == "Burn guard: н/д":
        return ("missing", None)
    if prefix == "Burn guard: 1:1":
        return ("aligned", 0.0)
    match = pattern.match(prefix)
    if match:
        return (match.group(1), float(match.group(2)))
    return (None, None)

api_class, api_value = parse(api)
script_class, script_value = parse(script)

if api_class is None or script_class is None:
    print("Burn guard prefix script produced unreadable value", file=sys.stderr)
    print(f"api:    {api}", file=sys.stderr)
    print(f"script: {script}", file=sys.stderr)
    raise SystemExit(1)

if api_class != script_class:
    print("Burn guard prefix script classification drifted from /api/client-limit-hourly-burn", file=sys.stderr)
    print(f"api:    {api}", file=sys.stderr)
    print(f"script: {script}", file=sys.stderr)
    raise SystemExit(1)

if api_class == "missing":
    if api != "Burn guard: н/д" or script != "Burn guard: н/д":
        print("Burn guard missing state drifted from /api/client-limit-hourly-burn", file=sys.stderr)
        print(f"api:    {api}", file=sys.stderr)
        print(f"script: {script}", file=sys.stderr)
        raise SystemExit(1)
else:
    if abs(api_value - script_value) > 10.0:
        print("Burn guard prefix script drifted from /api/client-limit-hourly-burn", file=sys.stderr)
        print(f"api:    {api}", file=sys.stderr)
        print(f"script: {script}", file=sys.stderr)
        raise SystemExit(1)
PY
snapshot_json="$(curl -fsS "http://127.0.0.1:${observe_port}/api/snapshot")"
printf '%s' "$snapshot_json" > tmp/proof_observability_snapshot.json
printf '%s' "$snapshot_json" | rg '"sla"|"postgres"|"token_budget_report"' >/dev/null
printf '%s' "$snapshot_json" | jq -e '
  .governance_surface.governance_surface_version == "governance-surface-v4"
  and (.governance_surface.forgetting_job_breakdown.pruning_job != null)
  and (.governance_surface.forgetting_job_breakdown.cold_archive_job != null)
  and (.governance_surface.forgetting_job_breakdown.revalidation_job != null)
  and (.governance_surface.forgetting_job_breakdown.de_duplication_job != null)
  and (.governance_surface.forgetting_job_breakdown.summarization_job != null)
  and (.governance_surface.forgetting_action_breakdown != null)
  and (.governance_surface.lifecycle_risk_summary.lifecycle_policy_review_summary.summary_version == "lifecycle-policy-review-summary-v1")
  and (.governance_surface.lifecycle_risk_summary.lifecycle_policy_review_summary.status == "advisory")
  and (.governance_surface.lifecycle_risk_summary.lifecycle_policy_review_summary.review_packet_state == "review_packet_ready")
  and (.governance_surface.lifecycle_risk_summary.lifecycle_policy_review_summary.approval_state == "pending_human_review")
  and (.governance_surface.lifecycle_risk_summary.lifecycle_policy_review_summary.measured_improvement_state == "not_measured_requires_holdout_or_post_action_outcomes")
' >/dev/null
printf '%s' "$snapshot_json" | jq -e '
  (.latest_memory_task_matrix.memory_task_matrix.statistics.statistics_version == "benchmark-statistics-v1")
  and (.latest_mcp_task_matrix.mcp_task_matrix.statistics.statistics_version == "benchmark-statistics-v1")
  and (.latest_memory_task_matrix.memory_task_matrix.statistics.methods.score_distribution_drift.status == "measured")
  and (.latest_mcp_task_matrix.mcp_task_matrix.statistics.methods.score_distribution_drift.status == "not_applicable")
  and (.latest_memory_task_matrix.memory_task_matrix.statistics.drift_summary.status == "measured")
  and (.latest_mcp_task_matrix.mcp_task_matrix.statistics.drift_summary.status == "measured")
  and (.latest_memory_task_matrix.memory_task_matrix.measured_approval.verdict == "pending_human_review")
	  and (.latest_mcp_task_matrix.mcp_task_matrix.measured_approval.verdict == "pending_human_review")
	' >/dev/null
printf '%s' "$snapshot_json" | jq -e '
  .latest_memory_task_matrix.memory_task_matrix.statistics.methods as $memory
  | .latest_mcp_task_matrix.mcp_task_matrix.statistics.methods as $mcp
  | (
      $memory.score_delta_confidence_interval.status == "measured"
      and ($memory.score_delta_confidence_interval.delta | type == "number")
      and ($memory.score_delta_confidence_interval.lower | type == "number")
      and ($memory.score_delta_confidence_interval.upper | type == "number")
    )
  and (
      $memory.score_distribution_drift.status == "measured"
      and ($memory.score_distribution_drift.value | type == "number")
    )
  and (
      $mcp.mean_delta_confidence_interval.status == "measured"
      and ($mcp.mean_delta_confidence_interval.delta | type == "number")
      and ($mcp.mean_delta_confidence_interval.lower | type == "number")
      and ($mcp.mean_delta_confidence_interval.upper | type == "number")
    )
  and (
      $mcp.score_distribution_drift.status == "not_applicable"
      and ($mcp.score_distribution_drift.reason | type == "string")
    )
' >/dev/null
dashboard_snapshot_pair="$(jq -n \
  --slurpfile dashboard tmp/proof_observability_dashboard.json \
  --slurpfile snapshot tmp/proof_observability_snapshot.json \
  '
  $dashboard[0] as $dashboard
  | $snapshot[0] as $snapshot
  |
  def card($title):
    $dashboard.benchmark_cards[] | select(.title == $title);
  {
    memory_card: card("Memory task matrix compare").source_identity,
    memory_snapshot: {
      snapshot_key: "latest_memory_task_matrix",
      payload_root: "memory_task_matrix",
      matrix: $snapshot.latest_memory_task_matrix.memory_task_matrix.matrix,
      source_event_id: $snapshot.latest_memory_task_matrix._observability.source_event_id,
      scope_project_code: $snapshot.latest_memory_task_matrix._observability.scope_project_code,
      scope_namespace_code: $snapshot.latest_memory_task_matrix._observability.scope_namespace_code,
      captured_at_epoch_ms: $snapshot.latest_memory_task_matrix._observability.captured_at_epoch_ms,
      payload_sha256: $snapshot.latest_memory_task_matrix._observability.payload_sha256,
      baseline_run_id: $snapshot.latest_memory_task_matrix.memory_task_matrix.statistics.baseline_run_id,
      candidate_run_id: $snapshot.latest_memory_task_matrix.memory_task_matrix.statistics.candidate_run_id
    },
    mcp_card: card("MCP task matrix compare").source_identity,
    mcp_snapshot: {
      snapshot_key: "latest_mcp_task_matrix",
      payload_root: "mcp_task_matrix",
      matrix: $snapshot.latest_mcp_task_matrix.mcp_task_matrix.matrix,
      source_event_id: $snapshot.latest_mcp_task_matrix._observability.source_event_id,
      scope_project_code: $snapshot.latest_mcp_task_matrix._observability.scope_project_code,
      scope_namespace_code: $snapshot.latest_mcp_task_matrix._observability.scope_namespace_code,
      captured_at_epoch_ms: $snapshot.latest_mcp_task_matrix._observability.captured_at_epoch_ms,
      payload_sha256: $snapshot.latest_mcp_task_matrix._observability.payload_sha256,
      baseline_run_id: $snapshot.latest_mcp_task_matrix.mcp_task_matrix.statistics.baseline_run_id,
      candidate_run_id: $snapshot.latest_mcp_task_matrix.mcp_task_matrix.statistics.candidate_run_id
    }
  }
')"
printf '%s' "$dashboard_snapshot_pair" | jq -e '.memory_card == .memory_snapshot and .mcp_card == .mcp_snapshot' >/dev/null
printf '%s' "$snapshot_json" | jq -e '
  .capacity_forecast.model_version == "capacity-forecast-v1"
  and .capacity_forecast.surface_role == "read_only_capacity_forecast"
  and .capacity_forecast.guardrails.runtime_authority == false
  and .capacity_forecast.guardrails.routing_authority == false
  and .capacity_forecast.guardrails.truth_authority == false
  and (.capacity_forecast.history_scope.mode | type == "string")
  and (.capacity_forecast.families | length >= 1)
  and (.capacity_forecast.families[0].family_key == "nats_events")
  and (.capacity_forecast.families[0].windows | length >= 2)
  and (
    [.capacity_forecast.families[0].windows[].status]
    | all(. == "measured" or . == "insufficient_sample")
  )
' >/dev/null
printf '%s' "$snapshot_json" | jq -e '
  .regression_explain.model_version == "regression-explain-v1"
  and .regression_explain.surface_role == "read_only_explainability"
  and .regression_explain.guardrails.routing_authority == false
  and .regression_explain.guardrails.truth_authority == false
  and .regression_explain.guardrails.forgetting_authority == false
  and (.regression_explain.outcomes | length >= 3)
  and (
    [.regression_explain.outcomes[].status]
    | all(. == "measured" or . == "insufficient_sample" or . == "not_materialized")
  )
' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/" | rg '/brand/amai_mark.svg|/favicon.ico' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/brand/amai_mark.svg" | rg '<svg' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/brand/amai_lockup.svg" | rg '<svg' >/dev/null
curl -fsS "http://127.0.0.1:${observe_port}/favicon.ico" >/dev/null

step "start monitoring profile"
./scripts/monitoring_up.sh

step "verify prometheus and grafana health"
curl -fsS "http://127.0.0.1:${AMI_PROMETHEUS_PORT}/api/v1/query?query=amai_qdrant_index_optimize_queue" | rg '"status":"success"' >/dev/null
curl -fsS "http://127.0.0.1:${AMI_GRAFANA_PORT}/api/health" | rg '"database"[[:space:]]*:[[:space:]]*"ok"' >/dev/null

proof_window_deadlocks_after="$(postgres_deadlocks_total)"
if [[ "${proof_window_deadlocks_after}" != "${proof_window_deadlocks_before}" ]]; then
  echo "proof_observability: postgres deadlocks increased during exporter proof window" >&2
  echo "baseline deadlocks_total=${proof_window_deadlocks_before}" >&2
  echo "after exporter proof window deadlocks_total=${proof_window_deadlocks_after}" >&2
  exit 1
fi

step "build live snapshot"
cargo run --release --quiet -- observe snapshot
step "run live sla-check"
cargo run --release --quiet -- observe sla-check

cleanup
trap - EXIT

printf 'proof_observability: ok\n'
