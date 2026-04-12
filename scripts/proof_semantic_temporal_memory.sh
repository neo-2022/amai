#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

./scripts/bootstrap_stack.sh

cargo test --quiet \
  postgres::tests::apply_memory_card_update_supersedes_prior_current_fact_for_same_subject_predicate_and_preserves_temporal_slices \
  -- --exact

cargo test --quiet \
  postgres::tests::create_memory_card_rejects_duplicate_current_truth_fact_triple \
  -- --exact

cargo test --quiet \
  postgres::tests::search_memory_cards_matches_generic_nl_queries_against_fact_fields_and_time_slice \
  -- --exact

cargo test --quiet \
  postgres::tests::retracting_memory_card_closes_temporal_window_for_latest_and_future_slices \
  -- --exact

cargo test --quiet \
  postgres::tests::latest_memory_card_search_prefers_current_verified_over_conflicted_candidates \
  -- --exact

cargo test --quiet \
  retrieval::tests::context_pack_decision_trace_explains_historical_temporal_hits \
  -- --exact

cargo test --quiet \
  retrieval::tests::proof_context_pack_disables_same_thread_cache_reuse_compaction \
  -- --exact

printf 'proof_semantic_temporal_memory: ok\n'
