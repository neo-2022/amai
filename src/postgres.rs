use crate::{config::AppConfig, observability_policy};
use anyhow::{Context, Result, anyhow};
use native_tls::TlsConnector as NativeTlsConnector;
use postgres_native_tls::MakeTlsConnector;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::config::{Host, SslMode};
use tokio_postgres::{Client, Config as PostgresConfig, NoTls, Row};
use uuid::Uuid;

const BOOTSTRAP_SCHEMA_ADVISORY_LOCK_KEY: i64 = 0x414d41495f736368;
static BOOTSTRAP_SCHEMA_CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

mod postgres_bootstrap_runtime;
mod postgres_context_provenance;
mod postgres_core_records;
mod postgres_import_packets;
mod postgres_index_maintenance;
mod postgres_inserts;
mod postgres_internal_structs;
mod postgres_link_inserts;
mod postgres_memory_edges_conflicts;
mod postgres_memory_relation_edges;
mod postgres_memory_runtime;
mod postgres_observability;
mod postgres_policy_rules;
mod postgres_project_namespace;
mod postgres_project_support;
mod postgres_quarantine;
mod postgres_records;
mod postgres_row_mapping;
mod postgres_search_runtime;
mod postgres_shared_assets;
mod postgres_skills;
mod postgres_workspace_access;

pub use self::postgres_bootstrap_runtime::*;
use self::postgres_bootstrap_runtime::{
    bootstrap_schema_cache_contains, bootstrap_schema_cache_insert, conflict_state_to_edge_state,
    conflict_state_to_edge_trust_state, safe_postgres_descriptor, sql_ident, sql_literal,
    validate_stage2_basis,
};
use self::postgres_context_provenance::validate_artifact_ref_basis;
pub use self::postgres_context_provenance::*;
pub use self::postgres_core_records::*;
pub use self::postgres_import_packets::*;
pub use self::postgres_index_maintenance::*;
pub use self::postgres_inserts::*;
use self::postgres_internal_structs::*;
pub use self::postgres_link_inserts::*;
pub use self::postgres_memory_edges_conflicts::*;
pub use self::postgres_memory_relation_edges::*;
pub use self::postgres_memory_runtime::*;
use self::postgres_memory_runtime::{
    augment_memory_item_metadata_with_stage2_runtime, build_memory_write_pipeline,
    derive_memory_item_source_kind, extract_memory_item_candidate, memory_item_has_recorded_basis,
    memory_item_record_from_row, memory_provenance_record_from_row,
    memory_write_async_index_subjects, memory_write_fan_out_subjects,
    metadata_marks_memory_item_poisoned, raw_evidence_record_from_row, resolve_scope_ids,
    run_memory_item_policy_scope_filter, validate_memory_item_candidate,
    validate_memory_item_policy_scope_filter, validate_memory_item_verification_conflict_check,
};
pub use self::postgres_observability::*;
use self::postgres_observability::{
    observability_conflict_error, observability_source_class, prepare_observability_payload,
    validate_observability_update,
};
pub use self::postgres_policy_rules::*;
pub use self::postgres_project_namespace::*;
pub use self::postgres_project_support::*;
use self::postgres_project_support::{
    ensure_cross_project_policy_access, find_project_link_context, get_bound_project_for_repo_root,
    get_project_workspace_id, record_scope_override_event, sync_project_repo_roots,
};
pub use self::postgres_quarantine::*;
pub use self::postgres_records::*;
use self::postgres_row_mapping::*;
pub use self::postgres_search_runtime::*;
use self::postgres_search_runtime::{exact_match_basename, exact_match_basename_stem};
pub use self::postgres_shared_assets::*;
pub use self::postgres_skills::*;
use self::postgres_skills::{
    SkillCardVerificationConflictCheck,
    augment_memory_provenance_evidence_span_with_stage2_preflight,
    augment_restore_pack_evidence_span_with_stage2_preflight,
    augment_retrieval_trace_evidence_span_with_stage2_preflight,
    canonical_candidate_class_from_hints, evidence_span_marks_skill_card_poisoned,
    extract_skill_card_candidate, run_memory_provenance_policy_scope_filter,
    run_memory_provenance_verification_conflict_check, run_restore_pack_policy_scope_filter,
    run_restore_pack_verification_conflict_check, run_retrieval_trace_policy_scope_filter,
    run_retrieval_trace_verification_conflict_check, run_skill_card_policy_scope_filter,
    runtime_contract_for_candidate_class, validate_memory_provenance_policy_scope_filter,
    validate_memory_provenance_verification_conflict_check,
    validate_restore_pack_policy_scope_filter, validate_restore_pack_verification_conflict_check,
    validate_retrieval_trace_policy_scope_filter,
    validate_retrieval_trace_verification_conflict_check, validate_skill_activity_basis,
    validate_skill_card_candidate, validate_skill_card_policy_scope_filter,
    validate_skill_card_verification_conflict_check, validate_skill_evidence_bundle_basis,
};
pub use self::postgres_workspace_access::*;

fn observability_profile_enabled() -> bool {
    env::var("AMAI_PROFILE_CONTINUITY")
        .map(|value| {
            let lowered = value.trim().to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn observability_profile_log(stage: &str, elapsed_ms: u128, extra: &str) {
    if observability_profile_enabled() {
        if extra.is_empty() {
            eprintln!(
                "[amai-observability-profile] stage={} elapsed_ms={}",
                stage, elapsed_ms
            );
        } else {
            eprintln!(
                "[amai-observability-profile] stage={} elapsed_ms={} {}",
                stage, elapsed_ms, extra
            );
        }
    }
}

#[cfg(test)]
#[path = "postgres/postgres_runtime_tests.rs"]
mod tests;
