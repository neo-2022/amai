#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/proof_art_continuity_migration.sh >/tmp/amai-art-continuity-migration-proof.log

last_output="$(cargo run --quiet -- continuity answer \
  --project art \
  --namespace continuity \
  --intent last_chat \
  --json)"

printf '%s\n' "$last_output" | jq -e '.retrieval_science.suite_key == "continuity_answer"' >/dev/null
printf '%s\n' "$last_output" | jq -e '.continuity_answer.intent == "last_chat"' >/dev/null
printf '%s\n' "$last_output" | jq -e '.continuity_answer.canonical_eval.verdict_counts.recovered_useful == 1' >/dev/null
printf '%s\n' "$last_output" | jq -e '.continuity_answer.canonical_eval.probes[0].name == "continuity_answer_recovered_useful"' >/dev/null
printf '%s\n' "$last_output" | jq -e '.continuity_answer.included_reasons_summary != null' >/dev/null
printf '%s\n' "$last_output" | jq -e '.continuity_answer.excluded_reasons_summary != null' >/dev/null
printf '%s\n' "$last_output" | jq -e '.continuity_answer.answer_text | contains("Почему вошёл текущий контекст:")' >/dev/null

previous_output="$(cargo run --quiet -- continuity answer \
  --project art \
  --namespace continuity \
  --chat-reference previous \
  --messages-count 2 \
  --json)"

printf '%s\n' "$previous_output" | jq -e '.continuity_answer.intent == "previous_chat"' >/dev/null
printf '%s\n' "$previous_output" | jq -e '.continuity_answer.chat_lookup.found == true' >/dev/null
printf '%s\n' "$previous_output" | jq -e '.continuity_answer.canonical_eval.verdict_counts.recovered_useful == 1' >/dev/null
printf '%s\n' "$previous_output" | jq -e '.continuity_answer.canonical_eval.probes[0].name == "previous_chat_answer_recovered_useful"' >/dev/null

missing_time_output="$(cargo run --quiet -- continuity answer \
  --project art \
  --namespace continuity \
  --at-time-rfc3339 2099-01-01T12:00:00Z \
  --json)"

printf '%s\n' "$missing_time_output" | jq -e '.continuity_answer.intent == "chat_at_time"' >/dev/null
printf '%s\n' "$missing_time_output" | jq -e '.continuity_answer.chat_lookup.found == false' >/dev/null
printf '%s\n' "$missing_time_output" | jq -e '.continuity_answer.canonical_eval.verdict_counts.hit_correct_target == 1' >/dev/null
printf '%s\n' "$missing_time_output" | jq -e '.continuity_answer.canonical_eval.probes[0].name == "exact_time_answer_fail_closed"' >/dev/null

question_output="$(AMAI_ALLOW_EXPENSIVE_TOOL_TURN=1 ./scripts/chat_lookup.sh \
  --project art \
  --namespace continuity \
  --question "что было в прошлом чате, какие последние два сообщения?" \
  --json)"

printf '%s\n' "$question_output" | jq -e '.continuity_answer.intent == "previous_chat"' >/dev/null
printf '%s\n' "$question_output" | jq -e '.continuity_answer.include_chat_messages == true' >/dev/null
printf '%s\n' "$question_output" | jq -e '.continuity_answer.canonical_eval.verdict_counts.recovered_useful == 1' >/dev/null

echo "proof_art_continuity_answer: PASS"
