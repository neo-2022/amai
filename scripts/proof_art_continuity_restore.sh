#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_art_continuity_migration.sh >/tmp/amai-art-continuity-migration-proof.log

restore_output="$(AMAI_ALLOW_EXPENSIVE_TOOL_TURN=1 ./scripts/continuity_restore.sh --project art --namespace continuity)"

printf '%s\n' "$restore_output" | jq -e '.retrieval_science.suite_key == "continuity_restore"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.verdict_counts.recovered_useful == 3' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.probes[0].name == "chat_start_restore_recovered_useful"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.probes[1].name == "working_state_restore_recovered_useful"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.probes[2].name == "workspace_restore_pack_recovered_useful"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.prompt_text | contains("CHAT_START_RESTORE")' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.included_reasons_summary != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.excluded_reasons_summary != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.workspace_restore_pack_summary != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.prompt_text | contains("Сначала:")' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.working_state_restore.state_lineage.authoritative_event_id != ""' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.workspace_restore_pack.active_commitments != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.workspace_restore_pack.active_constraints != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.workspace_restore_pack.important_artifacts != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.workspace_restore_pack.procedural_restore_policy.raw_procedural_archive_forbidden == true' >/dev/null

echo "proof_art_continuity_restore: PASS"
