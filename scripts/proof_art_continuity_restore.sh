#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_art_continuity_migration.sh >/tmp/amai-art-continuity-migration-proof.log

restore_output="$(./scripts/continuity_restore.sh --project art --namespace continuity)"

printf '%s\n' "$restore_output" | jq -e '.retrieval_science.suite_key == "continuity_restore"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.eval_verdict_model_version == "memory-eval-verdict-v1"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.verdict_counts.recovered_useful == 2' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.probes[0].name == "chat_start_restore_recovered_useful"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.continuity_restore.canonical_eval.probes[1].name == "working_state_restore_recovered_useful"' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.prompt_text | contains("CHAT_START_RESTORE")' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.included_reasons_summary != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.excluded_reasons_summary != null' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.chat_start_restore.prompt_text | contains("Почему вошёл последний контекст:")' >/dev/null
printf '%s\n' "$restore_output" | jq -e '.working_state_restore.state_lineage.authoritative_event_id != ""' >/dev/null

echo "proof_art_continuity_restore: PASS"
