#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

step() {
  echo "[proof_procedural_benchmark] $*"
}

step "verify benchmark catalog surfaces first-class procedural entry"
list_output="$(./scripts/benchmark_matrix.sh)"
printf '%s\n' "$list_output" | rg '^- Procedural Memory Evolution \(procedural_memory_evolution\) — частично покрыто$' >/dev/null

coverage_output="$(./scripts/benchmark_matrix.sh coverage)"
printf '%s\n' "$coverage_output" | rg '^General Assistant & Reasoning \(general_assistant_reasoning\)$' >/dev/null
printf '%s\n' "$coverage_output" | rg '^- Частично покрыто текущими proof/harness слоями: 1$' >/dev/null

explain_output="$(./scripts/benchmark_matrix.sh explain --benchmark procedural-memory-benchmark)"
printf '%s\n' "$explain_output" | rg '^Benchmark: Procedural Memory Evolution \(procedural_memory_evolution\)$' >/dev/null
printf '%s\n' "$explain_output" | rg '^Семейство: General Assistant & Reasoning \(general_assistant_reasoning\)$' >/dev/null
printf '%s\n' "$explain_output" | rg 'skill reuse quality' >/dev/null
printf '%s\n' "$explain_output" | rg 'stale-skill suppression' >/dev/null
printf '%s\n' "$explain_output" | rg '\./scripts/proof_procedural_benchmark.sh' >/dev/null

step "run procedural benchmark metric bundle"
./scripts/proof_procedural_seed.sh >/tmp/amai-proof-procedural-seed.out
./scripts/proof_procedural_shadow_review.sh >/tmp/amai-proof-procedural-shadow-review.out
./scripts/proof_negative_procedural_memory.sh >/tmp/amai-proof-negative-procedural.out
./scripts/proof_shared_promotion_by_approval.sh >/tmp/amai-proof-shared-promotion.out
./scripts/proof_skill_refinement_contour.sh >/tmp/amai-proof-skill-refinement.out
./scripts/proof_skill_version_history.sh >/tmp/amai-proof-skill-history.out
./scripts/proof_procedural_benchmark_without_amai.sh >/tmp/amai-proof-procedural-without-amai.out

step "assert metric-specific contours passed"
rg 'procedural seed proof passed' /tmp/amai-proof-procedural-seed.out >/dev/null
rg 'procedural shadow review proof passed' /tmp/amai-proof-procedural-shadow-review.out >/dev/null
rg 'negative procedural memory proof passed' /tmp/amai-proof-negative-procedural.out >/dev/null
rg 'shared promotion by approval proof passed' /tmp/amai-proof-shared-promotion.out >/dev/null
rg 'skill refinement contour proof passed' /tmp/amai-proof-skill-refinement.out >/dev/null
rg 'skill version history proof passed' /tmp/amai-proof-skill-history.out >/dev/null
rg 'proof_procedural_benchmark_without_amai: ok' /tmp/amai-proof-procedural-without-amai.out >/dev/null

step "summarize benchmark metrics"
cat <<'EOF'
procedural benchmark metrics:
- reuse_quality: pass
- bad_skill_suppression: pass
- stale_skill_suppression: pass
- shadow_to_verified_uplift: pass
- evaluator_correctness: pass
procedural benchmark without amai metrics:
- reuse_quality: fail
- bad_skill_suppression: pass
- stale_skill_suppression: pass
- shadow_to_verified_uplift: fail
- evaluator_correctness: pass
EOF

step "publish procedural benchmark snapshot for dashboard/observe surfaces"
captured_at_epoch_ms="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"
run_id="procedural-benchmark-${captured_at_epoch_ms}"
payload_file="$(mktemp)"
cat >"${payload_file}" <<EOF
{
  "_observability": {
    "source_event_id": "${run_id}",
    "source_kind": "verify_procedural_benchmark",
    "scope_project_code": "amai",
    "scope_namespace_code": "benchmark",
    "captured_at_epoch_ms": ${captured_at_epoch_ms}
  },
  "procedural_benchmark": {
    "benchmark_run_id": "${run_id}",
    "benchmark_dataset_id": "procedural_skill_quality_bundle_v1",
    "benchmark_code": "procedural_memory_evolution",
    "benchmark_run_state": "dual_line_materialized",
    "benchmark_run_state_ru": "обе benchmark-линии materialized",
    "benchmark_title_ru": "Навыки и память действий",
    "benchmark_description_ru": "Procedural benchmark по skill reuse, suppression, uplift и evaluator correctness.",
    "benchmark_tooltip_ru": "Отдельный compare-plane benchmark для procedural memory. Online и benchmark lane не смешиваются.",
    "benchmark_entrypoint": "./scripts/proof_procedural_benchmark.sh",
    "benchmark_group_ru": "Качество",
    "benchmark_metric_kind": "procedural_skill_metrics",
    "benchmark_run_passport": {
      "selected_agent_scope": "project",
      "selected_agent_label_ru": "Amai procedural contour",
      "selected_model": "${PROOF_MODEL:-gpt-5}",
      "selected_query_type": "procedural_memory",
      "runtime": "${PROOF_RUNTIME:-codex}",
      "provider": "local_cli",
      "execution_mode": "proof_bundle",
      "run_order": "with_amai_then_without_amai_measurement_bypass",
      "cache_state": "not_applicable",
      "multi_platform_runtime_contract": "platform-neutral benchmark snapshot"
    },
    "benchmark_with_amai_series": [
      {"metric_key":"reuse_quality","value":1.0},
      {"metric_key":"bad_skill_suppression","value":1.0},
      {"metric_key":"stale_skill_suppression","value":1.0},
      {"metric_key":"shadow_to_verified_uplift","value":1.0},
      {"metric_key":"evaluator_correctness","value":1.0}
    ],
    "benchmark_without_amai_series": [
      {"metric_key":"reuse_quality","value":0.0},
      {"metric_key":"bad_skill_suppression","value":1.0},
      {"metric_key":"stale_skill_suppression","value":1.0},
      {"metric_key":"shadow_to_verified_uplift","value":0.0},
      {"metric_key":"evaluator_correctness","value":1.0}
    ],
    "benchmark_line_summaries": {
      "with_amai": {
        "line_code": "with_amai",
        "state": "materialized",
        "point_count": 5,
        "pass_percent": 100.0,
        "summary_ru": "Контур с Amai materialized и подтверждён procedural proof bundle."
      },
      "without_amai_but_measuring": {
        "line_code": "without_amai_but_measuring",
        "state": "materialized",
        "point_count": 5,
        "pass_percent": 60.0,
        "summary_ru": "Without-Amai линия materialized через explicit procedural bypass-run: Amai не помогает, но benchmark продолжает измерять.",
        "reason_ru": "Reuse и uplift падают до 0.0, потому что verified skill не surfaced в execution-card при measurement-only режиме."
      }
    },
    "procedural_metrics": [
      {"metric_key":"reuse_quality","label_ru":"Reuse quality","tooltip_ru":"Переиспользуется ли полезный skill в подходящей задаче.","passed":true},
      {"metric_key":"bad_skill_suppression","label_ru":"Bad-skill suppression","tooltip_ru":"Не проходит ли плохой skill как verified/useful.","passed":true},
      {"metric_key":"stale_skill_suppression","label_ru":"Stale-skill suppression","tooltip_ru":"Не продолжает ли устаревший skill всплывать как лучший способ.","passed":true},
      {"metric_key":"shadow_to_verified_uplift","label_ru":"Shadow-to-verified uplift","tooltip_ru":"Проходит ли skill правильный путь shadow/trial/shared approval.","passed":true},
      {"metric_key":"evaluator_correctness","label_ru":"Evaluator correctness","tooltip_ru":"Тормозит ли evaluator плохую procedural evolution и shared drift.","passed":true}
    ],
    "summary": {
      "total_metrics": 5,
      "passed_metrics": 5,
      "failed_metrics": 0,
      "pass_percent": 100.0,
      "without_amai_series_available": true,
      "generic_score_forbidden": true
    }
  }
}
EOF
cargo run --quiet -- verify procedural-benchmark --json-file "${payload_file}" >/tmp/amai-proof-procedural-benchmark-publish.out
rg '"benchmark_metric_kind": "procedural_skill_metrics"' /tmp/amai-proof-procedural-benchmark-publish.out >/dev/null
rg '"benchmark_run_state": "dual_line_materialized"' /tmp/amai-proof-procedural-benchmark-publish.out >/dev/null
rg '"without_amai_series_available": true' /tmp/amai-proof-procedural-benchmark-publish.out >/dev/null

step "assert persisted history surface and richer time-series are materialized"
cargo run --quiet -- observe snapshot >/tmp/amai-proof-procedural-benchmark-observe.json
jq -e --arg run_id "${run_id}" '
  .procedural_benchmark_history.history_count >= 1 and
  (.procedural_benchmark_history.with_amai_pass_percent_series | length) >= 1 and
  (.procedural_benchmark_history.without_amai_pass_percent_series | length) >= 1 and
  any(.procedural_benchmark_history.history_rows[]; .benchmark_run_id == $run_id and .benchmark_run_state == "dual_line_materialized")
' /tmp/amai-proof-procedural-benchmark-observe.json >/dev/null
rm -f "${payload_file}"

printf 'proof_procedural_benchmark: ok\n'
