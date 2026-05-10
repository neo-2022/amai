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
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::config::{Host, SslMode};
use tokio_postgres::{Client, Config as PostgresConfig, NoTls, Row};
use uuid::Uuid;

pub(crate) const BOOTSTRAP_SCHEMA_ADVISORY_LOCK_KEY: i64 = 0x414d41495f736368;
static BOOTSTRAP_SCHEMA_CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
#[cfg(test)]
static OBSERVABILITY_PROFILE_TEST_LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

#[path = "postgres/postgres_bootstrap_runtime.rs"]
mod postgres_bootstrap_runtime;
#[path = "postgres/postgres_context_provenance.rs"]
mod postgres_context_provenance;
#[path = "postgres/postgres_core_records.rs"]
mod postgres_core_records;
#[path = "postgres/postgres_import_packets.rs"]
mod postgres_import_packets;
#[path = "postgres/postgres_index_maintenance.rs"]
mod postgres_index_maintenance;
#[path = "postgres/postgres_inserts.rs"]
mod postgres_inserts;
#[path = "postgres/postgres_internal_structs.rs"]
mod postgres_internal_structs;
#[path = "postgres/postgres_link_inserts.rs"]
mod postgres_link_inserts;
#[path = "postgres/postgres_memory_edges_conflicts.rs"]
mod postgres_memory_edges_conflicts;
#[path = "postgres/postgres_memory_relation_edges.rs"]
mod postgres_memory_relation_edges;
#[path = "postgres/postgres_memory_runtime.rs"]
mod postgres_memory_runtime;
#[path = "postgres/postgres_observability.rs"]
mod postgres_observability;
#[path = "postgres/postgres_policy_rules.rs"]
mod postgres_policy_rules;
#[path = "postgres/postgres_project_namespace.rs"]
mod postgres_project_namespace;
#[path = "postgres/postgres_project_support.rs"]
mod postgres_project_support;
#[path = "postgres/postgres_quarantine.rs"]
mod postgres_quarantine;
#[path = "postgres/postgres_records.rs"]
mod postgres_records;
#[path = "postgres/postgres_row_mapping.rs"]
mod postgres_row_mapping;
#[path = "postgres/postgres_search_runtime.rs"]
mod postgres_search_runtime;
#[path = "postgres/postgres_shared_assets.rs"]
mod postgres_shared_assets;
#[path = "postgres/postgres_skills.rs"]
mod postgres_skills;
#[path = "postgres/postgres_workspace_access.rs"]
mod postgres_workspace_access;

pub use self::postgres_bootstrap_runtime::*;
#[cfg(test)]
use self::postgres_bootstrap_runtime::{
    bootstrap_schema_cache_contains, bootstrap_schema_cache_insert, safe_postgres_descriptor,
};
use self::postgres_bootstrap_runtime::{
    conflict_state_to_edge_state, conflict_state_to_edge_trust_state, sql_ident, sql_literal,
    validate_stage2_basis,
};
#[cfg(test)]
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
#[cfg(test)]
use self::postgres_memory_runtime::{
    augment_memory_item_metadata_with_stage2_runtime, build_memory_write_pipeline,
    derive_memory_item_source_kind, extract_memory_item_candidate, memory_item_has_recorded_basis,
    memory_write_async_index_subjects, memory_write_fan_out_subjects,
    metadata_marks_memory_item_poisoned, run_memory_item_policy_scope_filter,
    validate_memory_item_candidate, validate_memory_item_policy_scope_filter,
    validate_memory_item_verification_conflict_check,
};
use self::postgres_memory_runtime::{
    memory_item_record_from_row, memory_provenance_record_from_row, raw_evidence_record_from_row,
    resolve_scope_ids,
};
pub use self::postgres_observability::*;
#[cfg(test)]
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
#[cfg(test)]
use self::postgres_search_runtime::{exact_match_basename, exact_match_basename_stem};
pub use self::postgres_shared_assets::*;
pub use self::postgres_skills::*;
#[cfg(test)]
use self::postgres_skills::{
    SkillCardVerificationConflictCheck, evidence_span_marks_skill_card_poisoned,
    extract_skill_card_candidate, run_skill_card_policy_scope_filter,
    validate_skill_activity_basis, validate_skill_card_candidate,
    validate_skill_card_policy_scope_filter, validate_skill_card_verification_conflict_check,
    validate_skill_evidence_bundle_basis,
};
use self::postgres_skills::{
    augment_memory_provenance_evidence_span_with_stage2_preflight,
    augment_restore_pack_evidence_span_with_stage2_preflight,
    augment_retrieval_trace_evidence_span_with_stage2_preflight,
    canonical_candidate_class_from_hints, run_memory_provenance_policy_scope_filter,
    run_memory_provenance_verification_conflict_check, run_restore_pack_policy_scope_filter,
    run_restore_pack_verification_conflict_check, run_retrieval_trace_policy_scope_filter,
    run_retrieval_trace_verification_conflict_check, runtime_contract_for_candidate_class,
    validate_memory_provenance_policy_scope_filter,
    validate_memory_provenance_verification_conflict_check,
    validate_restore_pack_policy_scope_filter, validate_restore_pack_verification_conflict_check,
    validate_retrieval_trace_policy_scope_filter,
    validate_retrieval_trace_verification_conflict_check,
};
pub use self::postgres_workspace_access::*;

pub(crate) fn observability_profile_enabled() -> bool {
    env::var("AMAI_PROFILE_CONTINUITY")
        .map(|value| {
            let lowered = value.trim().to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub(crate) fn observability_profile_log(stage: &str, elapsed_ms: u128, extra: &str) {
    #[cfg(test)]
    {
        let entry = if extra.is_empty() {
            format!(
                "[amai-observability-profile] stage={} elapsed_ms={}",
                stage, elapsed_ms
            )
        } else {
            format!(
                "[amai-observability-profile] stage={} elapsed_ms={} {}",
                stage, elapsed_ms, extra
            )
        };
        OBSERVABILITY_PROFILE_TEST_LOGS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .expect("observability profile test log mutex poisoned")
            .push(entry);
    }
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
pub(crate) fn take_observability_profile_test_logs() -> Vec<String> {
    OBSERVABILITY_PROFILE_TEST_LOGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("observability profile test log mutex poisoned")
        .drain(..)
        .collect()
}

pub(crate) fn advisory_unlock_result(
    unlock_result: Result<bool>,
    release_error: impl Into<String>,
) -> Result<()> {
    match unlock_result {
        Ok(true) => Ok(()),
        Ok(false) => Err(anyhow!(
            "{}: pg_advisory_unlock returned false",
            release_error.into()
        )),
        Err(error) => Err(error),
    }
}

fn finalize_advisory_lock_scope<T>(result: Result<T>, unlock_result: Result<()>) -> Result<T> {
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(unlock_error)) => Err(unlock_error),
        (Err(error), Err(unlock_error)) => Err(anyhow!(
            "{error:#}\nsecondary unlock failure: {unlock_error:#}"
        )),
    }
}

pub(crate) async fn with_postgres_advisory_lock<T, F, Fut>(
    client: &Client,
    key: i64,
    acquire_error: impl Into<String>,
    release_error: impl Into<String>,
    f: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let acquire_error = acquire_error.into();
    let release_error = release_error.into();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&key])
        .await
        .with_context(|| acquire_error.clone())?;
    let result = f().await;
    let unlock_result = advisory_unlock_result(
        client
            .query_one("SELECT pg_advisory_unlock($1)", &[&key])
            .await
            .with_context(|| release_error.clone())
            .map(|row| row.get::<_, bool>(0)),
        release_error,
    );
    finalize_advisory_lock_scope(result, unlock_result)
}

pub(crate) async fn with_postgres_advisory_lock_mut<T, F>(
    client: &mut Client,
    key: i64,
    acquire_error: impl Into<String>,
    release_error: impl Into<String>,
    f: F,
) -> Result<T>
where
    F: FnOnce(&mut Client) -> Pin<Box<dyn Future<Output = Result<T>> + '_>>,
{
    let acquire_error = acquire_error.into();
    let release_error = release_error.into();
    client
        .query_one("SELECT pg_advisory_lock($1)", &[&key])
        .await
        .with_context(|| acquire_error.clone())?;
    let result = f(client).await;
    let unlock_result = advisory_unlock_result(
        client
            .query_one("SELECT pg_advisory_unlock($1)", &[&key])
            .await
            .with_context(|| release_error.clone())
            .map(|row| row.get::<_, bool>(0)),
        release_error,
    );
    finalize_advisory_lock_scope(result, unlock_result)
}

#[cfg(test)]
mod advisory_unlock_result_tests {
    use super::advisory_unlock_result;
    use anyhow::anyhow;

    #[test]
    fn advisory_unlock_result_accepts_true() {
        advisory_unlock_result(Ok(true), "release lock").expect("unlock true must pass");
    }

    #[test]
    fn advisory_unlock_result_rejects_false() {
        let error =
            advisory_unlock_result(Ok(false), "release lock").expect_err("unlock false must fail");
        assert!(format!("{error:#}").contains("pg_advisory_unlock returned false"));
    }

    #[test]
    fn advisory_unlock_result_preserves_driver_error() {
        let error = advisory_unlock_result(Err(anyhow!("driver failed")), "release lock")
            .expect_err("driver error must pass through");
        assert!(format!("{error:#}").contains("driver failed"));
    }
}

#[cfg(test)]
#[path = "postgres/postgres_runtime_tests.rs"]
mod tests;
