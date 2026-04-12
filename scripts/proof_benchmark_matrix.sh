#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

list_output="$(./scripts/benchmark_matrix.sh)"
printf '%s\n' "$list_output" | rg '^Amai benchmark matrix$' >/dev/null
printf '%s\n' "$list_output" | rg '^Function Calling & Tool Use \(function_calling_tool_use\)$' >/dev/null
printf '%s\n' "$list_output" | rg '^- LiveMCPBench \(live_mcpbench\) — частично покрыто$' >/dev/null
printf '%s\n' "$list_output" | rg '^- MCP-Universe \(mcp_universe\) — частично покрыто$' >/dev/null
printf '%s\n' "$list_output" | rg '^- Procedural Memory Evolution \(procedural_memory_evolution\) — частично покрыто$' >/dev/null
printf '%s\n' "$list_output" | rg '^Coding & Software Engineering \(coding_software_engineering\)$' >/dev/null

coverage_output="$(./scripts/benchmark_matrix.sh coverage)"
printf '%s\n' "$coverage_output" | rg '^Amai benchmark coverage$' >/dev/null
printf '%s\n' "$coverage_output" | rg '^- Частично покрыто текущими proof/harness слоями: 3$' >/dev/null
printf '%s\n' "$coverage_output" | rg '^Function Calling & Tool Use \(function_calling_tool_use\)$' >/dev/null
printf '%s\n' "$coverage_output" | rg '^- Частично покрыто текущими proof/harness слоями: 2$' >/dev/null
printf '%s\n' "$coverage_output" | rg '^General Assistant & Reasoning \(general_assistant_reasoning\)$' >/dev/null
printf '%s\n' "$coverage_output" | rg '^- Частично покрыто текущими proof/harness слоями: 1$' >/dev/null

live_mcp_output="$(./scripts/benchmark_matrix.sh explain --benchmark live-mcpbench)"
printf '%s\n' "$live_mcp_output" | rg '^Benchmark: LiveMCPBench \(live_mcpbench\)$' >/dev/null
printf '%s\n' "$live_mcp_output" | rg '^Семейство: Function Calling & Tool Use \(function_calling_tool_use\)$' >/dev/null
printf '%s\n' "$live_mcp_output" | rg 'MCP task matrix' >/dev/null

procedural_output="$(./scripts/benchmark_matrix.sh explain --benchmark procedural-memory-benchmark)"
printf '%s\n' "$procedural_output" | rg '^Benchmark: Procedural Memory Evolution \(procedural_memory_evolution\)$' >/dev/null
printf '%s\n' "$procedural_output" | rg 'stale-skill suppression' >/dev/null
printf '%s\n' "$procedural_output" | rg 'proof_procedural_benchmark.sh' >/dev/null

swe_output="$(./scripts/benchmark_matrix.sh explain --benchmark 'SWE-bench Verified')"
printf '%s\n' "$swe_output" | rg '^Benchmark: SWE-bench Verified \(swe_bench_verified\)$' >/dev/null
printf '%s\n' "$swe_output" | rg 'bugfix context retrieval' >/dev/null

printf 'proof_benchmark_matrix: ok\n'
