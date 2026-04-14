#[cfg(test)]
mod tests {
    use super::{
        ContextPackStats, ContextPackTimings, LocalContextPackEntry, PreparedContextPack,
        SemanticTimings, apply_semantic_relevance_guard, build_context_pack_decision_trace,
        build_retrieval_trace_insert, build_same_thread_cache_reuse_payload, cache_key,
        degradation_probe_stale_fast_cache, degradation_proof_scenarios,
        exact_document_lookup_queries, fast_cache_key, local_fast_context_pack_cache_put,
        model_visible_context_pack_payload, normalized_safe_relative_path, query_terms,
        selected_fast_cache_key, semantic_fallback_result, semantic_hit_has_query_overlap,
        semantic_skip_fallback_result, semantic_skip_reason_for_router, semantic_skipped_result,
        should_bypass_fast_context_pack_cache, should_emit_same_thread_cache_reuse_payload,
        should_prefer_document_lookup_only, should_try_exact_document_lookup,
        should_try_exact_symbol_lookup, should_use_compact_workspace_graph_enrichment,
        should_use_minimal_document_workspace_graph, should_use_minimal_symbol_workspace_graph,
        structured_layer_sufficiency, summary_layer_sufficiency, synthetic_chunk_hit,
        try_execute_context_pack_fast_cached, with_whole_cycle_observed_overrides,
    };
    use crate::cli::ContextPackArgs;
    use crate::config::AppConfig;
    use crate::postgres::{DocumentHit, ProjectRecord, SymbolHit};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    include!("retrieval_tests_router.inc");
    include!("retrieval_tests_payload.inc");
    include!("retrieval_tests_workspace.inc");
}
