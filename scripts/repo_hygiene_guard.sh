#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

json_mode=0
if [[ "${1:-}" == "--json" ]]; then
  json_mode=1
fi

repo_root="$(pwd)"
status_value="ok"

canonical_doc_paths=(
  "README.md"
  "docs/AGENT_START_HERE.md"
  "docs/ARCHITECTURE.md"
  "docs/IMPLEMENTATION_GATES.md"
  "docs/IMPLEMENTATION_STATUS.md"
  "docs/OPERATIONS.md"
  "docs/TOKEN_LEDGER.md"
  "config/benchmark_matrix.toml"
)

portable_absolute_repo_refs="$(
  {
    for path in "${canonical_doc_paths[@]}"; do
      [[ -f "$path" ]] || continue
      rg -n --fixed-strings "$repo_root" "$path" || true
    done
  } | sed '/^$/d'
)"

missing_exec_scripts="$(
  while IFS= read -r script_path; do
    [[ -n "$script_path" ]] || continue
    if head -n 1 "$script_path" | grep -q '^#!' && [[ ! -x "$script_path" ]]; then
      printf '%s\n' "$script_path"
    fi
  done < <(find scripts -type f | sort)
)"

startup_status_raw="$(./target/debug/amai status 2>/dev/null || true)"
startup_status_line="$(printf '%s\n' "$startup_status_raw" | rg '^startup_artifacts:' -m 1 || true)"
startup_runtime_line="$(printf '%s\n' "$startup_status_raw" | rg '^startup_runtime_state:' -m 1 || true)"
agent_preflight_json="$(./scripts/agent_preflight.sh --json 2>/dev/null)"
agent_preflight_status="$(
  printf '%s\n' "$agent_preflight_json" |
    jq -r 'if has("agent_preflight_summary") then "ok" else "invalid" end'
)"
fmt_output=""
fmt_status="ok"
if ! fmt_output="$(cargo fmt --check 2>&1)"; then
  fmt_status="failed"
fi
ops_security_defaults_status="ok"
ops_security_defaults_output=""
if ! ops_security_defaults_output="$(./scripts/proof_ops_security_defaults.sh 2>&1)"; then
  ops_security_defaults_status="failed"
fi
nats_auth_render_status="ok"
nats_auth_render_output=""
if ! nats_auth_render_output="$(./scripts/proof_nats_auth_render.sh 2>&1)"; then
  nats_auth_render_status="failed"
fi
security_hardening_status="ok"
security_hardening_output=""
if ! security_hardening_output="$(./scripts/proof_security_hardening_contract.sh 2>&1)"; then
  security_hardening_status="failed"
fi

issues=()

if [[ -n "$portable_absolute_repo_refs" ]]; then
  status_value="drift_detected"
  issues+=("portable_absolute_repo_refs")
fi

if [[ -n "$missing_exec_scripts" ]]; then
  status_value="drift_detected"
  issues+=("missing_executable_bits")
fi

if [[ "$agent_preflight_status" != "ok" ]]; then
  status_value="drift_detected"
  issues+=("agent_preflight_not_materialized")
fi

if [[ "$startup_status_line" != startup_artifacts:\ ok* ]]; then
  status_value="drift_detected"
  issues+=("startup_artifacts_not_ok")
fi

if [[ "$startup_runtime_line" != startup_runtime_state:\ ok* ]]; then
  status_value="drift_detected"
  issues+=("startup_runtime_state_not_ok")
fi

if [[ "$fmt_status" != "ok" ]]; then
  status_value="drift_detected"
  issues+=("cargo_fmt_check_failed")
fi

if [[ "$ops_security_defaults_status" != "ok" ]]; then
  status_value="drift_detected"
  issues+=("ops_security_defaults_failed")
fi

if [[ "$nats_auth_render_status" != "ok" ]]; then
  status_value="drift_detected"
  issues+=("nats_auth_render_failed")
fi

if [[ "$security_hardening_status" != "ok" ]]; then
  status_value="drift_detected"
  issues+=("security_hardening_contract_failed")
fi

portable_absolute_repo_refs_json="$(printf '%s\n' "$portable_absolute_repo_refs" | jq -R -s 'split("\n") | map(select(length > 0))')"
missing_exec_scripts_json="$(printf '%s\n' "$missing_exec_scripts" | jq -R -s 'split("\n") | map(select(length > 0))')"
issues_json="$(printf '%s\n' "${issues[@]-}" | jq -R -s 'split("\n") | map(select(length > 0))')"
fmt_excerpt_json="$(printf '%s\n' "$fmt_output" | sed -n '1,80p' | jq -R -s '.')"
ops_security_defaults_excerpt_json="$(printf '%s\n' "$ops_security_defaults_output" | sed -n '1,80p' | jq -R -s '.')"
nats_auth_render_excerpt_json="$(printf '%s\n' "$nats_auth_render_output" | sed -n '1,80p' | jq -R -s '.')"
security_hardening_excerpt_json="$(printf '%s\n' "$security_hardening_output" | sed -n '1,80p' | jq -R -s '.')"

payload="$(jq -n \
  --arg status "$status_value" \
  --arg repo_root "$repo_root" \
  --arg startup_status_line "$startup_status_line" \
  --arg startup_runtime_line "$startup_runtime_line" \
  --arg agent_preflight_status "$agent_preflight_status" \
  --arg fmt_status "$fmt_status" \
  --arg ops_security_defaults_status "$ops_security_defaults_status" \
  --arg nats_auth_render_status "$nats_auth_render_status" \
  --arg security_hardening_status "$security_hardening_status" \
  --argjson issues "$issues_json" \
  --argjson portable_absolute_repo_refs "$portable_absolute_repo_refs_json" \
  --argjson missing_exec_scripts "$missing_exec_scripts_json" \
  --argjson fmt_excerpt "$fmt_excerpt_json" \
  --argjson ops_security_defaults_excerpt "$ops_security_defaults_excerpt_json" \
  --argjson nats_auth_render_excerpt "$nats_auth_render_excerpt_json" \
  --argjson security_hardening_excerpt "$security_hardening_excerpt_json" \
  '{
    status: $status,
    repo_root: $repo_root,
    checks: {
      startup_artifacts: $startup_status_line,
      startup_runtime_state: $startup_runtime_line,
      agent_preflight_status: $agent_preflight_status,
      cargo_fmt_check: $fmt_status,
      ops_security_defaults: $ops_security_defaults_status,
      nats_auth_render: $nats_auth_render_status,
      security_hardening_contract: $security_hardening_status
    },
    issues: $issues,
    portable_absolute_repo_refs: $portable_absolute_repo_refs,
    missing_executable_scripts: $missing_exec_scripts,
    cargo_fmt_excerpt: $fmt_excerpt,
    ops_security_defaults_excerpt: $ops_security_defaults_excerpt,
    nats_auth_render_excerpt: $nats_auth_render_excerpt,
    security_hardening_excerpt: $security_hardening_excerpt
  }')"

if [[ "$json_mode" -eq 1 ]]; then
  printf '%s\n' "$payload"
else
  printf '%s\n' "$payload" | jq .
fi

if [[ "$status_value" != "ok" ]]; then
  exit 1
fi
