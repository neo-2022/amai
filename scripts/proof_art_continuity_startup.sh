#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_art_continuity_migration.sh >/tmp/amai-art-continuity-migration-proof.log

startup_output="$(./scripts/continuity_startup.sh --project art --namespace continuity --json)"
art_repo_root="/home/art/Art"
startup_state_artifact="${art_repo_root}/.amai/continuity/project-chat-startup-state.json"

printf '%s\n' "$startup_output" | jq -e '.retrieval_science.suite_key == "continuity_startup"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.verdict_counts.recovered_useful == 3' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[0].name == "startup_summary_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[1].name == "chat_start_restore_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.continuity_startup.canonical_eval.probes[2].name == "working_state_restore_recovered_useful"' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.prompt_text | contains("CHAT_START_RESTORE")' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.included_reasons_summary != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.excluded_reasons_summary != null' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.chat_start_restore.prompt_text | contains("Почему вошёл последний контекст:")' >/dev/null
printf '%s\n' "$startup_output" | jq -e '.working_state_restore.state_lineage.authoritative_event_id != ""' >/dev/null
test -f "${startup_state_artifact}"
jq -e '.artifact_version == "workspace-startup-runtime-state-v2"' "${startup_state_artifact}" >/dev/null
jq -e '.source_tool == "amai_continuity_startup"' "${startup_state_artifact}" >/dev/null
jq -e '.source_summary_field == "continuity_startup_summary"' "${startup_state_artifact}" >/dev/null
jq -e '.startup_execution_gate.gate_version == "startup-execution-gate-v1"' "${startup_state_artifact}" >/dev/null
jq -e '.startup_execution_gate.no_silent_drop == true' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.prompt_text_present == true' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.startup_next_action.action_kind != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.required_return_task != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.project_task_tree != null' "${startup_state_artifact}" >/dev/null
jq -e '.continuity_startup_summary.project_task_ledger != null' "${startup_state_artifact}" >/dev/null
startup_state_output="$(./target/release/amai continuity startup-state --repo-root "${art_repo_root}" --json)"
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.status == "ok"' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.prompt_text_present == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.startup_next_action_present == true' >/dev/null
printf '%s\n' "$startup_state_output" | jq -e '.startup_runtime_state.startup_execution_gate_present == true' >/dev/null

echo "proof_art_continuity_startup: PASS"
